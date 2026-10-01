"""Daily prequential selective-refresh reference; never an actual trade ledger."""

from __future__ import annotations

import argparse
import json
from datetime import date, timedelta
from pathlib import Path

import joblib
import polars as pl

from .conservative_selective_training import fit_model, score
from .time_bucket_activity import activity_states
from .time_bucket_candidates import CORE, complete_cases
from .time_bucket_labels import (
    eligible_labels,
    label_contract,
    prediction_features_only,
    with_label_availability,
)
from .time_bucket_policy import cached_q5, choose_policy, selected_attempts
from .time_bucket_protocol import boundary, config
from .time_bucket_source_audit import checked_run, sha256, write_json
from .time_bucket_training import DECISION, IDENTITY, FitBudget, support


def reference_splits(panel: pl.DataFrame, day: date, frozen: dict) -> tuple[pl.DataFrame, pl.DataFrame]:
    cutoff = boundary(str(day))
    purge = pl.duration(seconds=frozen["purge_seconds"])
    allowed = panel.filter((pl.col("role") == "training") & (pl.col("window_end") + purge <= cutoff))
    if "label_use_at" in allowed:
        allowed = eligible_labels(allowed, cutoff)
    days = sorted(allowed["window_start"].dt.date().unique().to_list())
    count = frozen["reference"]["calibration_days"]
    if len(days) < count + frozen["reference"]["initial_fit_min_days"]:
        return allowed.head(0), allowed.head(0)
    calibration_days = days[-count:]
    calibration_start = boundary(str(calibration_days[0]))
    train = allowed.filter(pl.col("window_end") + purge <= calibration_start)
    if "label_use_at" in train:
        train = eligible_labels(train, calibration_start)
    calibration = allowed.filter(pl.col("window_start").dt.date().is_in(calibration_days)
                                 & (pl.col("window_start") >= calibration_start + purge))
    if train["window_start"].dt.date().n_unique() < frozen["reference"]["initial_fit_min_days"]:
        return train.head(0), calibration.head(0)
    return train, calibration


def generate(run: Path) -> None:
    frozen = config(run)
    label_identity = label_contract(run)
    prediction_features_only(CORE)
    eligibility = json.loads((run / "manifests/training-eligibility.json").read_text())
    if "conservative_selective_refresh" not in eligibility.get("accepted_candidates", []):
        raise ValueError("Reference direction/policy checkpoints are not eligible to train")
    identity = {"configuration": sha256(run / "inputs/tournament-freeze.json"),
                "features": CORE, "feature_freeze": sha256(run / "inputs/candidate-feature-freeze.json"),
                "core_panels": sha256(run / "manifests/core-panels.json"),
                "label_contract": label_identity,
                "code": {name: sha256(Path(__file__).with_name(name)) for name in [
                    "time_bucket_reference.py", "time_bucket_activity.py", "time_bucket_policy.py",
                    "conservative_selective_training.py", "time_bucket_protocol.py", "time_bucket_labels.py"]}}
    manifest_path = run / "manifests/simulated-refresh-reference.json"
    manifest = json.loads(manifest_path.read_text()) if manifest_path.exists() else {
        "identity": identity, "label": "simulated refresh activity", "days": [], "status": "in_progress"}
    if manifest["identity"] != identity:
        raise ValueError("Existing reference experiment identity changed")
    arm = {"features": CORE, "matched_products": []}
    columns = list(dict.fromkeys(IDENTITY + DECISION + CORE))
    frames = []
    paths = sorted((run / "datasets/core").glob("*.parquet"))
    core_manifest = json.loads((run / "manifests/core-panels.json").read_text())
    if core_manifest["status"] != "complete":
        raise ValueError("Reference fitting requires completed causal core panels")
    core_records = {Path(row["path"]): row for row in core_manifest["days"]}
    if set(paths) != set(core_records):
        raise ValueError("Reference core panel inventory changed")
    for path in paths:
        if sha256(path) != core_records[path]["sha256"]:
            raise ValueError("Reference core panel artifact changed")
        frame = pl.read_parquet(path, columns=columns).filter(
            (pl.col("entry_offset") == frozen["regular_entry_offset"]) & (pl.col("seconds_elapsed") < 260))
        complete = complete_cases(frame, arm)
        if complete.height:
            frames.append(complete)
    panel = with_label_availability(pl.concat(frames, how="vertical_relaxed"), label_identity)
    budget = FitBudget(run, frozen)
    completed = {x["date"]: x for x in manifest["days"]}
    for path in paths:
        day = date.fromisoformat(path.stem)
        if path.stem in completed:
            record = completed[path.stem]
            for artifact in record["artifacts"]:
                if sha256(Path(artifact["path"])) != artifact["sha256"]:
                    raise ValueError("Existing reference artifact changed")
            continue
        raw = pl.read_parquet(path, columns=columns).filter(
            (pl.col("entry_offset") == frozen["regular_entry_offset"]) & (pl.col("seconds_elapsed") < 260))
        today = complete_cases(raw, arm)
        train, calibration = reference_splits(panel, day, frozen)
        output = run / "predictions/simulated-refresh-reference" / path.name
        checkpoint = run / "checkpoints/simulated-refresh-reference" / path.stem
        if output.exists() or checkpoint.exists():
            raise ValueError("Unrecorded reference outputs require reconciliation; they cannot be overwritten")
        record = {"date": path.stem, "status": "insufficient_earlier_complete_case_support", "artifacts": [],
                  "training_markets": train["market_id"].n_unique(), "calibration_markets": calibration["market_id"].n_unique()}
        result = raw.select("point_id", "market_id", "decision_at").with_columns(
            pl.lit(None, dtype=pl.Float64).alias("probability_up"),
            pl.lit(False).alias("reference_known"), pl.lit(None, dtype=pl.Float64).alias("filled_quantity"),
            pl.lit(None, dtype=pl.Boolean).alias("selected_attempt"))
        if today.height and support(train, frozen["calibration"]["minimum_fit_markets"]) and support(calibration, frozen["calibration"]["minimum_calibration_markets"]):
            used = sum(x["status"] == "complete" for x in manifest["days"])
            if used >= frozen["reference"]["maximum_reference_fits"]:
                raise ValueError("Frozen reference fit count exhausted")
            budget.reserve(f"simulated-refresh-reference/{day}")
            model = fit_model(train, calibration, frozen["model"], frozen, frozen["seed"], features=tuple(CORE))
            scored_cal = score(calibration, model, frozen["calibration"]["safety_probability"])
            policy, search = choose_policy(scored_cal, frozen)
            scored = score(today, model, frozen["calibration"]["safety_probability"])
            attempts = cached_q5(selected_attempts(scored, policy, frozen)).select(
                "point_id", "filled_quantity", pl.lit(True).alias("selected_attempt"))
            # Non-admission is an observed zero only after full prediction/book evidence is established.
            observed = scored.join(attempts, on="point_id", how="left", validate="1:1").with_columns(
                (pl.col("up_fak_evidence_known") & pl.col("down_fak_evidence_known")).alias("reference_known"),
                pl.col("selected_attempt").fill_null(False))
            observed = observed.with_columns(
                pl.when(pl.col("reference_known") & ~pl.col("selected_attempt")).then(0.0)
                .otherwise(pl.col("filled_quantity")).alias("filled_quantity"))
            observed = observed.select("point_id", "probability_up", "reference_known", "filled_quantity", "selected_attempt")
            result = raw.select("point_id", "market_id", "decision_at").join(observed, on="point_id", how="left", validate="1:1")
            result = result.with_columns(pl.col("reference_known").fill_null(False))
            checkpoint.mkdir(parents=True, exist_ok=True)
            for name, value in [("direction", model[0]), ("calibration", model[1]), ("bundle", {"model": model, "policy": policy})]:
                target = checkpoint / f"{name}.joblib"
                joblib.dump(value, target, compress=3)
                record["artifacts"].append({"kind": name, "path": str(target), "sha256": sha256(target)})
            target = checkpoint / "policy.json"
            write_json(target, {"selected": policy, "search": search})
            record["artifacts"].append({"kind": "policy", "path": str(target), "sha256": sha256(target)})
            record.update(status="complete", latest_fit_market_end=str(train["window_end"].max()),
                          latest_calibration_market_end=str(calibration["window_end"].max()),
                          scored_start=str(today["decision_at"].min()), information_cutoff=boundary(str(day)).isoformat(),
                          excluded_roles=["calibration", "evaluation", "holdout"],
                          latest_fit_label_use_at=str(train["label_use_at"].max()),
                          latest_calibration_label_use_at=str(calibration["label_use_at"].max()),
                          conditional_on_label_timing_assumption=True)
        result = result.with_columns((pl.col("decision_at") + pl.duration(milliseconds=frozen["book"]["arrival_ms"]))
                                     .alias("evidence_available_at"))
        output.parent.mkdir(parents=True, exist_ok=True)
        result.write_parquet(output, compression="zstd")
        record["artifacts"].append({"kind": "reference-predictions", "path": str(output), "sha256": sha256(output)})
        manifest["days"].append(record)
        write_json(manifest_path, manifest)
        budget.finish()
        print(json.dumps({"date": path.stem, "status": record["status"]}), flush=True)
    manifest["status"] = "complete"
    write_json(manifest_path, manifest)
    build_activity_panels(run, paths, frozen)


def build_activity_panels(run: Path, paths: list[Path], frozen: dict) -> None:
    """Resume immutable quiet-state outputs without replacing completed evidence."""
    reference_path = run / "manifests/simulated-refresh-reference.json"
    reference_manifest = json.loads(reference_path.read_text())
    if reference_manifest["status"] != "complete":
        raise ValueError("Activity panels require completed reference predictions")
    core_manifest = json.loads((run / "manifests/core-panels.json").read_text())
    core_records = {Path(row["path"]): row for row in core_manifest["days"]}
    if core_manifest["status"] != "complete" or set(paths) != set(core_records):
        raise ValueError("Activity query panel inventory is incomplete or changed")
    if {row["date"] for row in reference_manifest["days"]} != {path.stem for path in paths}:
        raise ValueError("Reference predictions must cover every scheduled date, including unknown availability")
    for record in reference_manifest["days"]:
        for artifact in record["artifacts"]:
            if artifact["kind"] == "reference-predictions" and sha256(Path(artifact["path"])) != artifact["sha256"]:
                raise ValueError("Reference predictions changed before activity construction")
    identity = {"reference_manifest": sha256(reference_path),
                "core_panels": sha256(run / "manifests/core-panels.json"),
                "configuration": sha256(run / "inputs/tournament-freeze.json"),
                "activity_code": sha256(Path(__file__).with_name("time_bucket_activity.py"))}
    manifest_path = run / "manifests/simulated-refresh-panels.json"
    activity_manifest = json.loads(manifest_path.read_text()) if manifest_path.exists() else {
        "status": "in_progress", "identity": identity, "days": [],
        "label": "simulated refresh activity", "actual_incumbent_complementarity_proven": False}
    if activity_manifest["identity"] != identity:
        raise ValueError("Existing quiet-state panel identity changed")
    completed = {row["date"]: row for row in activity_manifest["days"]}
    for path in paths:
        if sha256(path) != core_records[path]["sha256"]:
            raise ValueError("Activity query panel artifact changed")
        if path.stem in completed:
            record = completed[path.stem]
            if sha256(Path(record["path"])) != record["sha256"]:
                raise ValueError("Existing quiet-state artifact changed")
            continue
        target = run / "datasets/simulated-refresh" / path.name
        if target.exists():
            raise ValueError("Unrecorded quiet-state outputs cannot be overwritten")
        day = date.fromisoformat(path.stem)
        previous = [run / "predictions/simulated-refresh-reference" / f"{d}.parquet" for d in [day - timedelta(days=1), day]]
        reference = pl.concat([pl.read_parquet(p) for p in previous if p.exists()], how="vertical_relaxed")
        query = pl.read_parquet(path, columns=["point_id", "decision_at"])
        result = query.hstack(activity_states(query, reference, frozen))
        target.parent.mkdir(parents=True, exist_ok=True)
        result.write_parquet(target, compression="zstd")
        activity_manifest["days"].append({"date": path.stem, "path": str(target), "sha256": sha256(target),
                                          "known_rows": result["reference_availability_known"].sum()})
        write_json(manifest_path, activity_manifest)
    activity_manifest["status"] = "complete"
    write_json(manifest_path, activity_manifest)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("run", type=Path)
    args = parser.parse_args()
    generate(checked_run(args.run))


if __name__ == "__main__":
    main()
