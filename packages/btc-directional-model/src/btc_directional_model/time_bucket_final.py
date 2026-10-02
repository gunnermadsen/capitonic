"""Post-qualification final fits with frozen features, chronology and policies."""

from __future__ import annotations

import argparse
import json
from pathlib import Path

import joblib
import polars as pl

from .conservative_selective_training import fit_model
from .time_bucket_exit import fit_exit
from .time_bucket_labels import (
    eligible_labels,
    label_contract,
    prediction_features_only,
    with_label_availability,
)
from .time_bucket_protocol import boundary, config, split_rows
from .time_bucket_source_audit import checked_run, sha256, write_json
from .time_bucket_tournament import arms_at_freeze
from .time_bucket_training import FitBudget, load_arm, support, value_head


def latest_supported(run: Path, arm: dict, frozen: dict) -> tuple[dict, dict] | None:
    """Selection uses chronological support only; no evaluation metric is consulted."""
    for fold in reversed(frozen["folds"]):
        path = run / "checkpoints" / arm["candidate"] / arm["arm"] / fold["name"] / "manifest.json"
        record = json.loads(path.read_text())
        if record["status"] == "complete":
            for artifact in record["artifacts"]:
                if sha256(Path(artifact["path"])) != artifact["sha256"]:
                    raise ValueError("Frozen fold artifact changed before final fit")
            return fold, record
    return None


def final_rows(panel: pl.DataFrame, labels: dict, frozen: dict, fold: dict) -> tuple[pl.DataFrame, pl.DataFrame]:
    panel = with_label_availability(panel, labels)
    train = eligible_labels(split_rows(panel, frozen, fold, "training"), boundary(fold["calibration_start"]))
    calibration = eligible_labels(split_rows(panel, frozen, fold, "calibration"), boundary(fold["evaluation_start"]))
    if train.filter(pl.col("role") != "training").height or calibration.filter(pl.col("role") != "calibration").height:
        raise ValueError("Protected chronological roles cannot enter a final fit")
    return train, calibration


def build(run: Path) -> None:
    frozen, labels = config(run), label_contract(run)
    results_path = run / "manifests/tournament-results.json"
    results = json.loads(results_path.read_text())
    if results["status"] != "complete":
        raise ValueError("All evaluation, robustness and qualification must precede final fitting")
    for artifact in results["outputs"]:
        if sha256(Path(artifact["path"])) != artifact["sha256"]:
            raise ValueError("Qualification evidence changed before final fitting")
    qualifications = json.loads((run / "metrics/qualification.json").read_text())["candidates_and_arms"]
    manifest_path = run / "manifests/final-models.json"
    identity = {"results": sha256(results_path), "configuration": sha256(run / "inputs/tournament-freeze.json"),
        "features": sha256(run / "inputs/candidate-feature-freeze.json"),
        "reporting_contract": sha256(run / "inputs/qualification-reporting-contract.json"),
        "label_contract": sha256(run / "inputs/label-availability-contract.json"),
        "implementation": sha256(Path(__file__))}
    manifest = json.loads(manifest_path.read_text()) if manifest_path.exists() else {
        "identity": identity, "status": "in_progress", "candidates": [],
        "conditional_on_label_timing_assumption": True, "deployment_authorized": False}
    if manifest["identity"] != identity:
        raise ValueError("Final-fit identity changed")
    completed = {r["candidate"]: r for r in manifest["candidates"]}
    budget = FitBudget(run, frozen)
    for arm in arms_at_freeze(run):
        if not arm["primary"]:
            continue
        if arm["candidate"] in completed:
            for artifact in completed[arm["candidate"]].get("artifacts", []):
                if sha256(Path(artifact["path"])) != artifact["sha256"]:
                    raise ValueError("Existing final model changed")
            continue
        selected = latest_supported(run, arm, frozen)
        record = {"candidate": arm["candidate"], "arm": arm["arm"], "artifacts": [],
                  "qualification": qualifications[arm["candidate"] + "/" + arm["arm"]],
                  "status": "unsupported_no_final_artifact"}
        if selected is not None:
            fold, _prior = selected
            target = run / "models/final" / arm["candidate"]
            if target.exists():
                raise ValueError("Unrecorded final-fit outputs require reconciliation")
            prediction_features_only(arm["features"])
            train, calibration = final_rows(load_arm(run, arm), labels, frozen, fold)
            if not support(train, frozen["calibration"]["minimum_fit_markets"]) or not support(calibration, frozen["calibration"]["minimum_calibration_markets"]):
                raise ValueError("Verified final-fit support changed")
            prior_path = run / "checkpoints" / arm["candidate"] / arm["arm"] / fold["name"] / "model-bundle.joblib"
            prior_bundle = joblib.load(prior_path)
            exit_train, targets = None, None
            if arm["head"] == "direction_and_exit":
                exit_train, _ = final_rows(load_arm(run, arm, include_exit_points=True), labels, frozen, fold)
                targets = pl.scan_parquet(run / "datasets/exit-labels/*.parquet").filter(
                    (pl.col("order_type") == "FAK") & pl.col("sell_advantage_label").is_not_null()
                ).join(exit_train.lazy().select("point_id"), on="point_id", how="inner").collect(engine="streaming")
            budget.reserve(f"{arm['candidate']}/{arm['arm']}/final", 2 if arm["head"] != "direction" else 1)
            direction = fit_model(train, calibration, frozen["model"], frozen, frozen["seed"], features=tuple(arm["features"]))
            bundle = {"direction": direction,
                "value": value_head(train, arm["features"], frozen) if arm["head"] == "direction_and_value" else None,
                "exit": fit_exit(exit_train, targets, arm["features"], frozen) if arm["head"] == "direction_and_exit" else None,
                "policy": prior_bundle["policy"], "selected_quantity": prior_bundle["selected_quantity"],
                "exit_policy": prior_bundle.get("exit_policy")}
            target.mkdir(parents=True)
            for name, value in [("direction", direction[0]), ("probability-calibration", direction[1]),
                                ("model-bundle", bundle), ("admission-value", bundle["value"]), ("exit-sell", bundle["exit"])]:
                if value is None:
                    continue
                path = target / f"{name}.joblib"
                joblib.dump(value, path, compress=3)
                record["artifacts"].append({"kind": name, "path": str(path), "sha256": sha256(path)})
            record.update(status="complete", source_fold=fold["name"], features=arm["features"],
                prior_bundle_sha256=sha256(prior_path), train_rows=train.height, calibration_rows=calibration.height,
                latest_fit_label_use_at=str(train["label_use_at"].max()),
                latest_calibration_label_use_at=str(calibration["label_use_at"].max()),
                policy_unchanged=True, selected_quantity=bundle["selected_quantity"],
                outcome="qualified_conditional_research_artifact" if record["qualification"]["qualified"] else "research_only_not_qualified")
            write_json(target / "manifest.json", {"identity": identity, **record})
            budget.finish()
        manifest["candidates"].append(record)
        write_json(manifest_path, manifest)
        print(json.dumps({"final_candidate": arm["candidate"], "status": record["status"]}), flush=True)
    manifest["status"] = "complete_with_explicit_support_outcomes"
    write_json(manifest_path, manifest)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("run", type=Path)
    args = parser.parse_args()
    build(checked_run(args.run))


if __name__ == "__main__":
    main()
