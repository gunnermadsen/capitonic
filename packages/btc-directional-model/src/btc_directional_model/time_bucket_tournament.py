"""Checkpointed execution of the eight frozen offline tournament challengers."""

from __future__ import annotations

import argparse
import json
from pathlib import Path

import joblib
import polars as pl

from .time_bucket_candidates import complete_cases, registry
from .time_bucket_exploration import source_groups
from .time_bucket_flow_panels import FEATURES
from .time_bucket_policy import intentions
from .time_bucket_protocol import config, split_rows
from .time_bucket_source_audit import checked_run, sha256, write_json
from .time_bucket_training import (
    DECISION,
    IDENTITY,
    FitBudget,
    fit_fold,
    load_arm,
    read_day,
    score_bundle,
)


def arms_at_freeze(run: Path) -> list[dict]:
    record = json.loads((run / "inputs/candidate-feature-freeze.json").read_text())
    if record["arms"] != registry(FEATURES):
        raise ValueError("Candidate feature implementation differs from the pre-exploration freeze")
    return record["arms"]


def fit_candidates(run: Path, quiet: bool) -> None:
    frozen = config(run)
    budget = FitBudget(run, frozen)
    manifest_path = run / "manifests" / ("quiet-fold-training.json" if quiet else "independent-fold-training.json")
    manifest = {"status": "in_progress", "configuration": sha256(run / "inputs/tournament-freeze.json"),
                "feature_freeze": sha256(run / "inputs/candidate-feature-freeze.json"), "arms": []}
    for arm in arms_at_freeze(run):
        if bool(arm.get("quiet_only")) != quiet:
            continue
        panel = load_arm(run, arm)
        exit_panel = load_arm(run, arm, include_exit_points=True) if arm["head"] == "direction_and_exit" else None
        outcomes = []
        for fold in frozen["folds"]:
            result = fit_fold(run, panel, arm, fold, budget, exit_panel=exit_panel)
            outcomes.append({"fold": fold["name"], "status": result["status"],
                             "manifest": str(run / "checkpoints" / arm["candidate"] / arm["arm"] / fold["name"] / "manifest.json")})
            print(json.dumps({"candidate": arm["candidate"], "arm": arm["arm"], "fold": fold["name"], "status": result["status"]}), flush=True)
        manifest["arms"].append({"candidate": arm["candidate"], "arm": arm["arm"], "folds": outcomes})
        write_json(manifest_path, manifest)
    manifest["status"] = "complete_with_explicit_support_outcomes"
    write_json(manifest_path, manifest)


def score_evaluation(run: Path) -> None:
    """Write every scheduled prediction/rejection only after all fitted policies are frozen."""
    frozen = config(run)
    arms = arms_at_freeze(run)
    for name in ["independent-fold-training.json", "quiet-fold-training.json"]:
        if json.loads((run / "manifests" / name).read_text())["status"] != "complete_with_explicit_support_outcomes":
            raise ValueError("All eight candidate fitting outcomes must be frozen before evaluation scoring")
    panel_names = ["core-panels.json", "refprice-twap-panels.json", "flow-panels.json", "simulated-refresh-panels.json"]
    panels = {name: json.loads((run / "manifests" / name).read_text()) for name in panel_names}
    if any(panel["status"] != "complete" for panel in panels.values()):
        raise ValueError("All causal panel layers must be complete before OOS scoring")
    manifest_path = run / "manifests/evaluation-predictions.json"
    identity = {"configuration": sha256(run / "inputs/tournament-freeze.json"),
                "label_contract": sha256(run / "inputs/label-availability-contract.json"),
                "diagnostic_contract": sha256(run / "inputs/qualification-reporting-contract.json"),
                "features": sha256(run / "inputs/candidate-feature-freeze.json"),
                "panels": {name: sha256(run / "manifests" / name) for name in panel_names},
                "fit_manifests": {name: sha256(run / "manifests" / name) for name in [
                    "independent-fold-training.json", "quiet-fold-training.json"]},
                "code": {name: sha256(Path(__file__).with_name(name)) for name in [
                    "time_bucket_tournament.py", "time_bucket_training.py", "time_bucket_policy.py"]}}
    manifest = json.loads(manifest_path.read_text()) if manifest_path.exists() else {
        "identity": identity, "days": [], "status": "in_progress"}
    if manifest["identity"] != identity:
        raise ValueError("Evaluation identity changed after prediction generation started")
    completed = {(r["candidate"], r["arm"], r["date"]): r for r in manifest["days"]}
    for fold in frozen["folds"]:
        checkpoints = {}
        for arm in arms:
            root = run / "checkpoints" / arm["candidate"] / arm["arm"] / fold["name"]
            record = json.loads((root / "manifest.json").read_text())
            if record["status"] == "complete":
                for artifact in record["artifacts"]:
                    if sha256(Path(artifact["path"])) != artifact["sha256"]:
                        raise ValueError("Model or policy artifact changed before evaluation")
                bundle = joblib.load(root / "model-bundle.joblib")
            else:
                bundle = None
            checkpoints[(arm["candidate"], arm["arm"])] = (record, bundle)
        for path in sorted((run / "datasets/core").glob("*.parquet")):
            if not (fold["evaluation_start"] <= path.stem < fold["evaluation_end"]):
                continue
            for panel in panels.values():
                evidence = next(row for row in panel["days"] if row["date"] == path.stem)
                if sha256(Path(evidence["path"])) != evidence["sha256"]:
                    raise ValueError("Frozen causal evaluation panel changed")
            day = read_day(run, path.stem, ["refprice-twap", "flow", "simulated-refresh"])
            presence = [pl.all_horizontal(pl.col(field).is_not_null() if field in day else pl.lit(False)
                                        for field in fields).cast(pl.UInt64) * (1 << i)
                        for i, fields in enumerate(source_groups().values())]
            day = day.with_columns(pl.sum_horizontal(presence).alias("source_coverage_mask"),
                *(pl.col(f"{side}_log_ask_depth").exp().sub(1).alias(f"{side}_decision_ask_depth") for side in ["up", "down"]))
            evaluation_ids = split_rows(day, frozen, fold, "evaluation")["point_id"].implode()
            day = day.with_columns(pl.col("point_id").is_in(evaluation_ids).alias("evaluation_clock_eligible"))
            for arm in arms:
                key = (arm["candidate"], arm["arm"], path.stem)
                if key in completed:
                    if sha256(Path(completed[key]["path"])) != completed[key]["sha256"]:
                        raise ValueError("Existing evaluation prediction hash mismatch")
                    continue
                rows = day.filter(pl.col("entry_offset").is_in(arm["entry_offsets"]) & (pl.col("seconds_elapsed") < 260))
                # Quiet models can be scored on known active states for diagnostic comparison;
                # their deployable admission remains restricted to the frozen quiet population.
                eligible = complete_cases(rows.filter(pl.col("evaluation_clock_eligible")), {**arm, "quiet_only": False})
                checkpoint, bundle = checkpoints[(arm["candidate"], arm["arm"])]
                prediction_columns = ["probability_up", "conservative_probability_up"]
                if bundle is not None and eligible.height:
                    scored = score_bundle(eligible, bundle, frozen)
                    prediction_columns += [c for c in ["expected_value_up", "expected_value_down"] if c in scored]
                    predictions = scored.select("point_id", *prediction_columns)
                    result = rows.join(predictions, on="point_id", how="left", validate="1:1")
                else:
                    result = rows.with_columns(pl.lit(None, dtype=pl.Float64).alias(c) for c in prediction_columns)
                result = result.with_columns(pl.col("point_id").is_in(eligible["point_id"].implode()).alias("feature_eligible"))
                result = intentions(result, bundle["policy"] if bundle else None, frozen)
                diagnostic_policy = {**bundle["policy"], "bucket_mask": frozen["bucket_starts"]} if bundle and bundle["policy"] else None
                counterfactual = intentions(result, diagnostic_policy, frozen)
                result = result.with_columns(counterfactual["admitted"].alias("bucket_diagnostic_admitted"),
                    counterfactual["admission_reason"].alias("bucket_diagnostic_reason"))
                for name in ["chosen_probability", "decision_share_cost", "expected_gross_value_per_share"]:
                    if name not in result:
                        result = result.with_columns(pl.lit(None, dtype=pl.Float64).alias(name))
                if arm["head"] == "direction_and_value":
                    for name in ["expected_value_up", "expected_value_down"]:
                        if name not in result:
                            result = result.with_columns(pl.lit(None, dtype=pl.Float64).alias(name))
                        if name not in prediction_columns:
                            prediction_columns.append(name)
                result = result.with_columns(pl.col("admitted").alias("general_market_diagnostic_admitted"))
                if arm.get("quiet_only"):
                    result = result.with_columns(
                        (pl.col("admitted") & (pl.col("reference_quiet") == 1)).fill_null(False).alias("admitted"),
                        (pl.col("bucket_diagnostic_admitted") & (pl.col("reference_quiet") == 1)).fill_null(False).alias("bucket_diagnostic_admitted"),
                        pl.when(pl.col("feature_eligible") & (pl.col("reference_quiet") == 0))
                        .then(pl.lit("outside_quiet_population")).otherwise(pl.col("bucket_diagnostic_reason")).alias("bucket_diagnostic_reason"),
                        pl.when(pl.col("feature_eligible") & (pl.col("reference_quiet") == 0))
                        .then(pl.lit("outside_quiet_population")).otherwise(pl.col("admission_reason")).alias("admission_reason"))
                result = result.with_columns(
                    (1 - pl.col("probability_up")).alias("probability_down"),
                    pl.when(~pl.col("evaluation_clock_eligible")).then(pl.lit("purged_boundary_market"))
                    .when(~pl.col("feature_eligible")).then(pl.lit("missing_required_causal_features"))
                    .when(pl.lit(bundle is None)).then(pl.lit("model_unavailable_insufficient_support"))
                    .otherwise(pl.col("admission_reason")).alias("admission_reason"),
                    pl.lit(arm["candidate"]).alias("candidate"), pl.lit(arm["arm"]).alias("arm"),
                    pl.lit(fold["name"]).alias("fold"),
                    pl.lit(bundle["selected_quantity"] if bundle else None, dtype=pl.Float64).alias("selected_quantity"),
                    pl.lit(checkpoint["status"]).alias("checkpoint_status"))
                result = result.with_columns(
                    pl.when(~pl.col("evaluation_clock_eligible")).then(pl.lit("purged_boundary_market"))
                    .when(~pl.col("feature_eligible")).then(pl.lit("missing_required_causal_features"))
                    .when(pl.lit(bundle is None)).then(pl.lit("model_unavailable_insufficient_support"))
                    .otherwise(pl.col("bucket_diagnostic_reason")).alias("bucket_diagnostic_reason"))
                extra = ["candidate", "arm", "fold", "feature_eligible", "evaluation_clock_eligible",
                         "checkpoint_status", "selected_quantity", "probability_down", "admitted",
                         "admission_reason", "chosen_side", "expected_stressed_value_per_share"]
                extra += ["reference_quiet", "reference_availability_known", "general_market_diagnostic_admitted"]
                extra += ["bucket_diagnostic_admitted", "bucket_diagnostic_reason", "source_coverage_mask",
                          "up_decision_ask_depth", "down_decision_ask_depth"]
                extra += [c for c in ["chosen_probability", "decision_share_cost", "expected_gross_value_per_share"] if c in result]
                # Feature values and full lineage traces remain in immutable panel files.
                retained = list(dict.fromkeys(IDENTITY + DECISION + prediction_columns + extra))
                result = result.select(retained)
                target = run / "predictions/evaluation" / arm["candidate"] / arm["arm"] / path.name
                target.parent.mkdir(parents=True, exist_ok=True)
                if target.exists():
                    raise ValueError("Unrecorded evaluation output must not be overwritten")
                result.write_parquet(target, compression="zstd")
                record = {"candidate": arm["candidate"], "arm": arm["arm"], "date": path.stem,
                          "path": str(target), "sha256": sha256(target), "rows": result.height,
                          "feature_eligible_rows": eligible.height,
                          "prediction_rows": result["probability_up"].drop_nulls().len()}
                manifest["days"].append(record)
                write_json(manifest_path, manifest)
            print(json.dumps({"prediction_date": path.stem, "arms": len(arms)}), flush=True)
    manifest["status"] = "complete"
    write_json(manifest_path, manifest)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("run", type=Path)
    parser.add_argument("action", choices=["fit-independent", "fit-quiet", "score"])
    args = parser.parse_args()
    run = checked_run(args.run)
    if args.action == "score":
        score_evaluation(run)
    else:
        fit_candidates(run, args.action == "fit-quiet")


if __name__ == "__main__":
    main()
