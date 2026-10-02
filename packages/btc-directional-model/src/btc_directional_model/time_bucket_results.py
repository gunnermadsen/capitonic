"""Frozen OOS robustness and conditional qualification from offline replay ledgers."""

from __future__ import annotations

import argparse
import json
import math
from datetime import timedelta
from pathlib import Path

import numpy as np
import polars as pl

from .time_bucket_policy import economic_metrics
from .time_bucket_protocol import boundary, config
from .time_bucket_source_audit import checked_run, sha256, write_json
from .time_bucket_tournament import arms_at_freeze

DIMS = ["economic_view", "order_type", "requested_quantity", "scenario", "control", "timing_control", "population_control"]


def valued(frame: pl.DataFrame) -> pl.DataFrame:
    return frame.filter(pl.col("evidence_known") & pl.col("exit_evidence_known")
                        & pl.col("net_pnl").is_finite() & pl.col("stress_pnl").is_finite())


def performance(frame: pl.DataFrame) -> dict:
    result = economic_metrics(frame)
    known = valued(frame)
    fills = known.filter(pl.col("filled_quantity") > 0)
    observed = frame.filter(pl.col("evidence_known") & (pl.col("filled_quantity") > 0))
    requested = known["requested_quantity"].sum()
    result.update(fill_ratio=known["filled_quantity"].sum() / requested if requested else None,
                  partial_fills=fills.filter(pl.col("filled_quantity") < pl.col("requested_quantity") - 1e-9).height,
                  filled_notional=observed["filled_notional"].sum(),
                  unfilled_shares=(known["requested_quantity"] - known["filled_quantity"]).sum(),
                  mean_execution_price=fills["filled_notional"].sum() / fills["filled_quantity"].sum() if fills.height else None,
                  unvalued_entry_fills=observed.height - fills.height)
    return result


def primary_ledger(frame: pl.DataFrame, arm: dict, *, quantity: float | None = 5, kind: str | None = "FAK") -> pl.DataFrame:
    selected = frame.filter((pl.col("economic_view") == "sequential_policy")
        & (pl.col("scenario") == "base") & (pl.col("timing_control") == "selected_wait_policy")
        & (pl.col("population_control") == "frozen_candidate_population")
        & (pl.col("control") == ("learned" if arm["head"] == "direction_and_exit" else "hold")))
    if quantity is not None:
        selected = selected.filter(pl.col("requested_quantity") == quantity)
    if kind is not None:
        selected = selected.filter(pl.col("order_type") == kind)
    return selected


def classification(frame: pl.DataFrame, bins: int) -> dict:
    scored = frame.filter(pl.col("evaluation_clock_eligible") & pl.col("probability_up").is_finite())
    p, y = scored["probability_up"].to_numpy(), scored["label_up"].to_numpy()
    result = {"scheduled_points": frame.height, "predictions": scored.height,
              "admissions": scored.filter(pl.col("admitted")).height,
              "direction_accuracy": float(np.mean((p >= .5) == y)) if len(p) else None,
              "brier_score": float(np.mean((p - y) ** 2)) if len(p) else None, "ece": None}
    if len(p):
        cell = np.minimum((p * bins).astype(int), bins - 1)
        result["ece"] = float(sum(np.sum(cell == i) / len(p) * abs(float(p[cell == i].mean()) - float(y[cell == i].mean()))
                                  for i in range(bins) if np.any(cell == i)))
    for actual in (0, 1):
        for predicted in (0, 1):
            result[f"actual_{actual}_predicted_{predicted}"] = int(np.sum((y == actual) & ((p >= .5) == predicted)))
    admitted = scored.filter(pl.col("admitted"))
    result["admitted_direction_accuracy"] = (admitted["chosen_side"] == admitted["official_outcome"]).mean() if admitted.height else None
    return result


def coverage_hours(predictions: pl.DataFrame, ledger: pl.DataFrame, arm: dict, frozen: dict) -> pl.DataFrame:
    rows = []
    expected = 12 * len(frozen["bucket_starts"]) * len(arm["entry_offsets"])
    observed = predictions.with_columns((pl.col("evaluation_clock_eligible") & pl.col("probability_up").is_finite()
        & pl.col("up_fak_evidence_known") & pl.col("down_fak_evidence_known")).fill_null(False).alias("observed"))
    for day in sorted(predictions["window_start"].dt.date().unique().to_list()):
        for hour in range(24):
            start = boundary(str(day)) + timedelta(hours=hour)
            finish = start + timedelta(hours=1)
            panel = observed.filter(pl.col("decision_at").is_between(start, finish, closed="left"))
            attempts = ledger.filter(pl.col("decision_at").is_between(start, finish, closed="left"))
            metrics = performance(attempts)
            n = int(panel["observed"].sum())
            complete = n == expected and panel["market_id"].n_unique() == 12 and metrics["unknown_attempts"] == 0
            predicted = panel["probability_up"].drop_nulls().len()
            if complete:
                status = "fully_covered" if metrics["observed_entry_fills"] else "no_trade_covered"
                if panel["admission_reason"].eq("policy_abstain").all():
                    status = "policy_disabled"
            elif predicted or metrics["observed_entry_fills"]:
                status = "partial_coverage"
            elif panel.height and panel["feature_eligible"].any():
                status = "untested"
            else:
                status = "missing_source"
            rows.append({"hour": start, "date": str(day), "coverage_status": status,
                "fully_covered": complete, "observed_points": n, "expected_points": expected,
                "prediction_points": predicted, "filled_trades": metrics["observed_entry_fills"] if predicted else None,
                "net_pnl": metrics["net_pnl"] if predicted else None,
                "stress_pnl": metrics["stress_pnl"] if predicted else None,
                "unvalued_entry_fills": metrics["unvalued_entry_fills"]})
    return pl.DataFrame(rows)


def daily_results(predictions: pl.DataFrame, ledger: pl.DataFrame, hours: pl.DataFrame) -> pl.DataFrame:
    rows = []
    for day in sorted(predictions["window_start"].dt.date().unique().to_list()):
        selected = ledger.filter(pl.col("decision_at").dt.date() == day)
        selected_hours = hours.filter(pl.col("date") == str(day))
        rows.append({"date": str(day), "fully_covered": selected_hours["fully_covered"].all(),
                     "observed_hours": selected_hours.filter(pl.col("prediction_points") > 0).height,
                     **performance(selected)})
    return pl.DataFrame(rows, infer_schema_length=None)


def robustness(daily: pl.DataFrame, frozen: dict) -> dict:
    days = daily.filter(pl.col("observed_hours") > 0)
    values = days["stress_pnl"].to_numpy()
    if not len(values):
        return {"observed_days": 0, "best_day": None, "worst_day": None, "best_day_deleted_stress": None,
                "worst_day_deleted_stress": None, "bootstrap_low": None, "bootstrap_high": None,
                "maximum_positive_day_share": None}
    best, worst = int(np.argmax(values)), int(np.argmin(values))
    rng = np.random.default_rng(frozen["seed"])
    draws = rng.choice(values, (frozen["bootstrap"]["replicates"], len(values)), replace=True).sum(axis=1)
    alpha = (1 - frozen["bootstrap"]["confidence"]) / 2
    positive_sum = values[values > 0].sum()
    return {"observed_days": len(values), "best_day": days["date"][best], "worst_day": days["date"][worst],
            "best_day_stress": float(values[best]), "worst_day_stress": float(values[worst]),
            "best_day_deleted_stress": float(values.sum() - values[best]),
            "worst_day_deleted_stress": float(values.sum() - values[worst]),
            "bootstrap_low": float(np.quantile(draws, alpha)), "bootstrap_high": float(np.quantile(draws, 1 - alpha)),
            "maximum_positive_day_share": float(max(values.max(), 0) / positive_sum) if positive_sum else None}


def neighboring_checks(run: Path, arm: dict, ledger: pl.DataFrame, frozen: dict) -> tuple[dict, list[dict]]:
    quantities: dict[str, list[pl.DataFrame]] = {}
    times: dict[int, list[pl.DataFrame]] = {}
    base = primary_ledger(ledger, arm, quantity=None)
    diagnostic = ledger.filter((pl.col("economic_view") == "bucket_diagnostic") & (pl.col("order_type") == "FAK")
        & (pl.col("scenario") == "base") & (pl.col("control") == ("learned" if arm["head"] == "direction_and_exit" else "hold")))
    for fold in frozen["folds"]:
        root = run / "checkpoints" / arm["candidate"] / arm["arm"] / fold["name"]
        record = json.loads((root / "manifest.json").read_text())
        if record["status"] != "complete":
            continue
        selected = json.loads((root / "quantity-selection.json").read_text())["selected"]
        pos = frozen["quantity_grid"].index(selected)
        for name, step in [("lower", -1), ("upper", 1)]:
            j = pos + step
            if 0 <= j < len(frozen["quantity_grid"]):
                quantities.setdefault(name, []).append(base.filter((pl.col("fold") == fold["name"])
                                                    & (pl.col("requested_quantity") == frozen["quantity_grid"][j])))
        policy = json.loads((root / "policy.json").read_text())["selected"]
        if policy:
            neighbors = {b + delta for b in policy["bucket_mask"] for delta in (-20, 20)} & set(frozen["bucket_starts"])
            for bucket in neighbors:
                times.setdefault(bucket, []).append(diagnostic.filter((pl.col("fold") == fold["name"]) & (pl.col("bucket_start") == bucket)))
    details = []
    for axis, mapping in [("quantity", quantities), ("time", times)]:
        for neighbor, frames in mapping.items():
            metrics = performance(pl.concat(frames) if frames else ledger.head(0))
            details.append({"axis": axis, "neighbor": str(neighbor), **metrics,
                            "pass": bool(frames) and metrics["fills"] >= 5 and metrics["stress_pnl"] >= 0})
    return {axis: any(x["axis"] == axis for x in details) and all(x["pass"] for x in details if x["axis"] == axis)
            for axis in ("quantity", "time")}, details


def qualification(metrics: dict, folds: list[dict], robust: dict, complete: dict, neighbors: dict, frozen: dict) -> dict:
    gate = frozen["qualification"]
    principal = [x for x in folds if x["fold"] != "holdout"]
    holdout = next(x for x in folds if x["fold"] == "holdout")
    positive_folds = sum(max(x["stress_pnl"], 0) for x in principal)
    concentration = max((max(x["stress_pnl"], 0) for x in principal), default=0) / positive_folds if positive_folds else None
    tests = {
        "positive_net": metrics["net_pnl"] > 0,
        "positive_stressed": metrics["stress_pnl"] > 0,
        "stressed_pf": metrics["stressed_profit_factor"] is not None and metrics["stressed_profit_factor"] > 1,
        "recovery": metrics["recovery_ratio"] is not None and metrics["recovery_ratio"] < 1,
        "positive_holdout": holdout["predictions"] > 0 and holdout["stress_pnl"] > 0,
        "three_positive_principal_folds": sum(x["stress_pnl"] > 0 for x in principal) >= gate["minimum_positive_principal_folds"],
        "best_day_deleted_nonnegative": robust["best_day_deleted_stress"] is not None and robust["best_day_deleted_stress"] >= 0,
        "day_concentration": robust["maximum_positive_day_share"] is not None and robust["maximum_positive_day_share"] <= gate["maximum_positive_day_share"],
        "fold_concentration": concentration is not None and concentration <= gate["maximum_positive_fold_share"],
        "market_diversity": metrics["markets"] >= gate["minimum_distinct_filled_markets"],
        "principal_samples": len(principal) == 4 and all(x["fills"] >= gate["minimum_fills_per_principal_fold"] for x in principal),
        "holdout_samples": holdout["fills"] >= gate["minimum_holdout_fills"],
        "adjacent_quantity": neighbors["quantity"], "adjacent_time": neighbors["time"],
        "fully_covered_days": complete["days"] > 0 and complete["net_pnl"] > 0 and complete["stress_pnl"] > 0
            and complete["stressed_profit_factor"] is not None and complete["stressed_profit_factor"] > 1
            and complete["recovery_ratio"] is not None and complete["recovery_ratio"] < 1,
        "known_selected_execution": metrics["unknown_attempts"] == 0,
    }
    return {"qualified": all(tests.values()), "gates": tests, "failed_gates": [k for k, v in tests.items() if not v],
            "maximum_positive_fold_share": concentration, "conditional_on_label_timing_assumption": True,
            "deployment_authorized": False}


def activity_bursts(ledger: pl.DataFrame, hours: pl.DataFrame, frozen: dict) -> list[dict]:
    fills = ledger.filter(pl.col("evidence_known") & (pl.col("filled_quantity") > 0)).sort("decision_at")
    groups, current, previous = [], [], None
    for row in fills.iter_rows(named=True):
        if previous is not None and (row["decision_at"] - previous).total_seconds() > frozen["charts"]["burst_max_gap_minutes"] * 60:
            groups.append(current)
            current = []
        current.append(row)
        previous = row["decision_at"]
    if current:
        groups.append(current)
    quiet_hours = {row["hour"] for row in hours.iter_rows(named=True) if row["coverage_status"] == "no_trade_covered"}
    result = []
    for rows in groups:
        first = rows[0]["decision_at"]
        cursor = first.replace(minute=0, second=0, microsecond=0) - timedelta(hours=1)
        minutes = 0
        while cursor in quiet_hours:
            minutes += 60
            cursor -= timedelta(hours=1)
        complete = all(row["exit_evidence_known"] and row["net_pnl"] is not None and row["stress_pnl"] is not None for row in rows)
        result.append({"start": first, "end": rows[-1]["decision_at"], "trade_count": len(rows),
                       "preceding_verified_quiet_minutes": float(minutes) if minutes else None,
                       "net_pnl": sum(row["net_pnl"] for row in rows) if complete else None,
                       "stress_pnl": sum(row["stress_pnl"] for row in rows) if complete else None,
                       "economics_complete": complete})
    return result


def write_table(run: Path, name: str, records: list[dict] | pl.DataFrame, outputs: list[dict]) -> pl.DataFrame:
    table = records if isinstance(records, pl.DataFrame) else pl.DataFrame(records, infer_schema_length=None)
    target = run / "metrics" / f"{name}.parquet"
    if target.exists():
        raise ValueError(f"Unrecorded result output cannot be overwritten: {target}")
    table.write_parquet(target, compression="zstd")
    outputs.append({"path": str(target), "sha256": sha256(target), "rows": table.height})
    return table


def composition(refresh: tuple[pl.DataFrame, pl.DataFrame], quiet: tuple[pl.DataFrame, pl.DataFrame],
                buckets: tuple[int, ...] = tuple(range(0, 260, 20)), offset: int = 19) -> tuple[dict, pl.DataFrame]:
    def covered(frame):
        frame = frame.filter(pl.col("entry_offset") == offset)
        return frame.group_by("market_id").agg((pl.col("evaluation_clock_eligible") & pl.col("probability_up").is_finite()
            & pl.col("up_fak_evidence_known") & pl.col("down_fak_evidence_known") & pl.col("reference_availability_known"))
            .fill_null(False).all().alias("known"), pl.len().alias("rows"),
            pl.col("bucket_start").n_unique().alias("buckets"),
            pl.col("bucket_start").is_in(buckets).all().alias("schedule"))\
            .filter(pl.col("known") & pl.col("schedule") & (pl.col("rows") == len(buckets))
                    & (pl.col("buckets") == len(buckets))).select("market_id")
    markets = covered(refresh[0]).join(covered(quiet[0]), on="market_id", how="inner")
    a = refresh[1].join(markets, on="market_id").with_columns(pl.lit(0).alias("priority"))
    b = quiet[1].join(markets, on="market_id").with_columns(pl.lit(1).alias("priority"))
    merged = pl.concat([a, b], how="diagonal_relaxed").sort("decision_at", "priority").unique("market_id", keep="first", maintain_order=True)
    baseline, specialist, combined = performance(a), performance(b), performance(merged)
    return {"matched_markets": markets.height, "refreshed": baseline, "quiet_independent_matched": specialist,
            "combined": combined, "incremental_stress_pnl": combined["stress_pnl"] - baseline["stress_pnl"],
            "reference": "simulated refresh activity", "actual_incumbent_complementarity_proven": False,
            "conditional_on_label_timing_assumption": True}, merged


def build(run: Path) -> None:
    frozen = config(run)
    replay_manifest = run / "manifests/execution-replay.json"
    predictions_manifest = run / "manifests/evaluation-predictions.json"
    replay = json.loads(replay_manifest.read_text())
    predictions = json.loads(predictions_manifest.read_text())
    if replay["status"] != "complete" or predictions["status"] != "complete":
        raise ValueError("All frozen OOS prediction and replay outputs must complete before qualification")
    manifest_path = run / "manifests/tournament-results.json"
    identity = {"replay": sha256(replay_manifest), "predictions": sha256(predictions_manifest),
                "label_contract": sha256(run / "inputs/label-availability-contract.json"),
                "reporting_contract": sha256(run / "inputs/qualification-reporting-contract.json"),
                "implementation": sha256(Path(__file__))}
    if manifest_path.exists():
        existing = json.loads(manifest_path.read_text())
        if existing["identity"] != identity or existing["status"] != "complete":
            raise ValueError("Result checkpoint changed or requires partial-output reconciliation")
        for artifact in existing["outputs"]:
            if sha256(Path(artifact["path"])) != artifact["sha256"]:
                raise ValueError("Recorded result output changed")
        return
    arms = arms_at_freeze(run)
    outputs, overview, fold_records, daily_records, hourly_records = [], [], [], [], []
    dimensions, bucket_records, class_records, rejection_records, neighbor_records, bursts = [], [], [], [], [], []
    qualifications, primaries = {}, {}
    activity_states, complete_day_checks = [], []
    for arm in arms:
        key = {"candidate": arm["candidate"], "arm": arm["arm"]}
        prediction_records = [r for r in predictions["days"] if all(r[k] == v for k, v in key.items())]
        trade_records = [r for r in replay["outputs"] if all(r[k] == v for k, v in key.items())]
        for record in prediction_records + trade_records:
            if sha256(Path(record["path"])) != record["sha256"]:
                raise ValueError("Frozen input changed before statistics")
        panel = pl.read_parquet([r["path"] for r in prediction_records])
        ledger = pl.read_parquet([r["path"] for r in trade_records])
        principal = primary_ledger(ledger, arm)
        hours = coverage_hours(panel, principal, arm, frozen)
        daily = daily_results(panel, principal, hours)
        robust = robustness(daily, frozen)
        metrics = performance(principal)
        cls = classification(panel, frozen["calibration"]["reliability_bins"])
        complete_dates = daily.filter(pl.col("fully_covered"))["date"].to_list()
        complete = {"days": len(complete_dates), **performance(principal.filter(pl.col("date").is_in(complete_dates)))}
        neighbors, details = neighboring_checks(run, arm, ledger, frozen)
        local_folds = []
        for fold in frozen["folds"]:
            pred = panel.filter(pl.col("fold") == fold["name"])
            local_folds.append({"fold": fold["name"], **performance(principal.filter(pl.col("fold") == fold["name"])),
                               **classification(pred, frozen["calibration"]["reliability_bins"])})
        q = qualification(metrics, local_folds, robust, complete, neighbors, frozen)
        covered_panel = panel.filter(pl.col("window_start").dt.strftime("%Y-%m-%d").is_in(complete_dates))
        covered_ledger = ledger.filter(pl.col("date").is_in(complete_dates))
        covered_primary = primary_ledger(covered_ledger, arm)
        covered_folds = [{"fold": fold["name"],
            **performance(covered_primary.filter(pl.col("fold") == fold["name"])),
            **classification(covered_panel.filter(pl.col("fold") == fold["name"]), frozen["calibration"]["reliability_bins"])}
            for fold in frozen["folds"]]
        covered_neighbors, _ = neighboring_checks(run, arm, covered_ledger, frozen)
        covered_qualification = qualification(complete, covered_folds,
            robustness(daily.filter(pl.col("fully_covered")), frozen), complete, covered_neighbors, frozen)
        q["gates"]["fully_covered_days"] = covered_qualification["qualified"]
        q["qualified"] = all(q["gates"].values())
        q["failed_gates"] = [name for name, passed in q["gates"].items() if not passed]
        complete_day_checks.append({**key, "days": len(complete_dates), **covered_qualification})
        qualifications[arm["candidate"] + "/" + arm["arm"]] = q
        overview.append({**key, "primary": arm["primary"], **metrics, **cls, **robust,
                         "fully_covered_days": len(complete_dates), "qualified": q["qualified"],
                         "label_timing_conditional": True})
        fold_records.extend({**key, **row} for row in local_folds)
        daily_records.extend({**key, **row} for row in daily.to_dicts())
        hourly_records.extend({**key, **row} for row in hours.to_dicts())
        neighbor_records.extend({**key, **row} for row in details)
        for dims, part in ledger.partition_by(DIMS, as_dict=True).items():
            dimensions.append({**key, **dict(zip(DIMS, dims, strict=True)), **performance(part)})
        for bucket in frozen["bucket_starts"]:
            for kind in ["FAK", "FOK"]:
                selected = ledger.filter((pl.col("economic_view") == "bucket_diagnostic") & (pl.col("bucket_start") == bucket)
                    & (pl.col("order_type") == kind) & (pl.col("control") == ("learned" if arm["head"] == "direction_and_exit" else "hold")))
                bucket_records.append({**key, "bucket_start": bucket, "order_type": kind, **performance(selected)})
        for side in ["up", "down"]:
            subset = panel.filter(pl.col("chosen_side") == side)
            class_records.append({**key, "side": side, **classification(subset, frozen["calibration"]["reliability_bins"]),
                                  **{f"filled_{k}": v for k, v in performance(principal.filter(pl.col("chosen_side") == side)).items()}})
        rejection_records.extend({**key, **row} for row in panel.group_by("fold", "admission_reason").len().to_dicts())
        states = panel.select("point_id", pl.when(~pl.col("reference_availability_known").fill_null(False))
            .then(pl.lit("unknown")).when(pl.col("reference_quiet") == 1).then(pl.lit("quiet"))
            .otherwise(pl.lit("active")).alias("reference_state"))
        state_ledger = principal.join(states, on="point_id", how="left", validate="m:1")
        for state in ["quiet", "active", "unknown"]:
            activity_states.append({**key, "reference_state": state, "reference": "simulated refresh activity",
                **performance(state_ledger.filter(pl.col("reference_state") == state))})
        if arm["primary"]:
            primaries[arm["candidate"]] = (panel, principal)
            bursts.extend({"candidate": arm["candidate"], **row} for row in activity_bursts(principal, hours, frozen))
        print(json.dumps({"summarized": arm["candidate"], "arm": arm["arm"]}), flush=True)
    combination, combined_ledger = composition(primaries["conservative_selective_refresh"], primaries["quiet_explorer"],
                                              tuple(frozen["bucket_starts"]), frozen["regular_entry_offset"])
    quiet_key = "quiet_explorer/primary"
    quiet_q = qualifications[quiet_key]
    extra = combination["incremental_stress_pnl"] > 0 and combination["matched_markets"] > 0
    quiet_q["gates"]["positive_incremental_refresh_composition"] = extra
    quiet_q["qualified"] = all(quiet_q["gates"].values())
    quiet_q["failed_gates"] = [k for k, v in quiet_q["gates"].items() if not v]
    for row in overview:
        if row["candidate"] == "quiet_explorer" and row["arm"] == "primary":
            row["qualified"] = quiet_q["qualified"]
    summaries = write_table(run, "arm-overview", overview, outputs)
    for name, records in [("fold-results", fold_records), ("daily-results", daily_records), ("hourly-results", hourly_records),
        ("capacity-stress-controls", dimensions), ("bucket-results", bucket_records), ("side-calibration", class_records),
        ("rejection-reasons", rejection_records), ("neighbor-stability", neighbor_records)]:
        write_table(run, name, records, outputs)
    write_table(run, "reference-activity-states", activity_states, outputs)
    rank_rows = sorted([x for x in overview if x["primary"] and x["predictions"] > 0], key=lambda x:
        (-x["stress_pnl"], -(x["stressed_profit_factor"] or 0), x["recovery_ratio"] if x["recovery_ratio"] is not None else math.inf, x["candidate"]))
    write_table(run, "candidate-ranking", [{**row, "rank": i + 1} for i, row in enumerate(rank_rows)], outputs)
    primary_hours = pl.DataFrame(hourly_records, infer_schema_length=None).filter(pl.col("arm") == "primary")
    write_table(run, "activity-hourly", primary_hours, outputs)
    burst_frame = pl.DataFrame(bursts, infer_schema_length=None) if bursts else pl.DataFrame(schema={
        "candidate": pl.String, "start": pl.Datetime("us", "UTC"), "end": pl.Datetime("us", "UTC"), "trade_count": pl.Int64,
        "preceding_verified_quiet_minutes": pl.Float64, "net_pnl": pl.Float64, "stress_pnl": pl.Float64, "economics_complete": pl.Boolean})
    write_table(run, "activity-bursts", burst_frame, outputs)
    write_table(run, "simulated-refresh-composition-ledger", combined_ledger, outputs)
    decision_path = run / "metrics/qualification.json"
    write_json(decision_path, {"conditional_on_label_timing_assumption": True, "assumption_is_not_independent_timestamp_proof": True,
        "candidates_and_arms": qualifications, "simulated_refresh_composition": combination,
        "fully_covered_day_qualification": complete_day_checks,
        "actual_incumbent_comparison": "unavailable evidence limitation; no actual paper/live complementarity claim",
        "qualification_gates_not_aspirational_targets": frozen["qualification"], "deployment_authorized": False})
    outputs.append({"path": str(decision_path), "sha256": sha256(decision_path)})
    write_json(manifest_path, {"identity": identity, "status": "complete", "outputs": outputs,
        "distinct_evaluated_candidates": len(rank_rows), "arms": summaries.height,
        "conditional_on_label_timing_assumption": True, "deployment_authorized": False,
        "interpretation": "Bucket counterfactuals remain separate from sequential economics. Missing/partial/untested coverage is not observed quiet. Bootstrap unit UTC day; lower bound reported, not a gate."})


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("run", type=Path)
    args = parser.parse_args()
    build(checked_run(args.run))


if __name__ == "__main__":
    main()
