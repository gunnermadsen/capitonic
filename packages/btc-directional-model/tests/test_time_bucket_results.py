"""Synthetic checks against the frozen qualification and reporting contract."""

import copy
import json
from datetime import UTC, datetime, timedelta
from pathlib import Path

import polars as pl
import pytest

from btc_directional_model import time_bucket_report, time_bucket_results
from btc_directional_model.time_bucket_results import (
    activity_bursts,
    composition,
    coverage_hours,
    daily_results,
    neighboring_checks,
    performance,
    qualification,
    robustness,
)
from btc_directional_model.time_bucket_source_audit import sha256

START = datetime(2026, 9, 14, tzinfo=UTC)
# Exact material thresholds in the run's pre-evaluation tournament/reporting freezes.
FROZEN = {
    "seed": 20260930,
    "bucket_starts": list(range(0, 260, 20)),
    "quantity_grid": [1, 5, 10, 15, 20, 25, 30, 40, 50, 75, 100, 125, 150, 175, 200],
    "folds": [{"name": name} for name in ["fold_1", "fold_2", "fold_3", "fold_4", "holdout"]],
    "qualification": {
        "minimum_net_pnl_exclusive": 0,
        "minimum_stressed_pnl_exclusive": 0,
        "minimum_stressed_pf_exclusive": 1,
        "maximum_recovery_exclusive": 1,
        "positive_holdout_required": True,
        "minimum_positive_principal_folds": 3,
        "principal_fold_count": 4,
        "best_day_deletion_minimum": 0,
        "maximum_positive_day_share": 0.5,
        "maximum_positive_fold_share": 0.5,
        "minimum_distinct_filled_markets": 50,
        "minimum_fills_per_principal_fold": 5,
        "minimum_holdout_fills": 10,
        "aspirational_targets": {
            "stressed_profit_factor": 1.25,
            "recovery_ratio": 0.75,
            "general_fills_per_fully_covered_day": [5, 8],
        },
    },
    "bootstrap": {
        "unit": "UTC day",
        "replicates": 1000,
        "confidence": 0.95,
        "lower_bound_is_gate": False,
    },
    "charts": {"burst_max_gap_minutes": 30},
}
ARM = {"candidate": "synthetic", "arm": "primary", "head": "direction", "entry_offsets": [19]}


def test_complete_summary_preserves_unsupported_quiet_and_hashes(tmp_path, monkeypatch):
    frozen = json.loads((Path(__file__).parents[1] / "configs/btc-time-bucket-tournament.json").read_text())
    arms = [{**ARM, "candidate": name, "primary": True, "features": []} for name in ["conservative_selective_refresh", "quiet_explorer"]]
    (tmp_path / "inputs").mkdir()
    (tmp_path / "metrics").mkdir()
    (tmp_path / "manifests").mkdir()
    for name in ["label-availability-contract", "qualification-reporting-contract"]:
        (tmp_path / "inputs" / f"{name}.json").write_text('{"synthetic":true}')
    preds, trades = [], []
    for arm in arms:
        for index, fold in enumerate(frozen["folds"]):
            start = datetime.fromisoformat(fold["evaluation_start"]).replace(tzinfo=UTC) + timedelta(hours=1)
            market = str(index)
            known = arm["candidate"] != "quiet_explorer"
            rows = market_predictions(market, start, probability_up=.6 if known else None,
                label_up=index % 2, official_outcome="up" if index % 2 else "down", chosen_side="up" if known else None,
                admitted=False, reference_quiet=1., fold=fold["name"])
            root = tmp_path / "checkpoints" / arm["candidate"] / "primary" / fold["name"]
            root.mkdir(parents=True)
            (root / "manifest.json").write_text('{"status":"insufficient_complete_case_support"}')
            pred = root / "synthetic-predictions.parquet"
            pl.DataFrame(rows, schema_overrides={"probability_up": pl.Float64, "chosen_side": pl.String}).write_parquet(pred)
            entry = root / "synthetic-ledger.parquet"
            ledger([trade(market, start, point_id=f"{market}:19", chosen_side="up", date=str(start.date()),
                fold=fold["name"], candidate=arm["candidate"], arm="primary")]).head(0).write_parquet(entry)
            for output, path in [(preds, pred), (trades, entry)]:
                output.append({"candidate": arm["candidate"], "arm": "primary", "date": str(start.date()),
                               "path": str(path), "sha256": sha256(path)})
    (tmp_path / "manifests/evaluation-predictions.json").write_text(json.dumps({"status": "complete", "days": preds}))
    (tmp_path / "manifests/execution-replay.json").write_text(json.dumps({"status": "complete", "outputs": trades}))
    monkeypatch.setattr(time_bucket_results, "config", lambda _: frozen)
    monkeypatch.setattr(time_bucket_results, "arms_at_freeze", lambda _: arms)
    time_bucket_results.build(tmp_path)
    manifest = json.loads((tmp_path / "manifests/tournament-results.json").read_text())
    assert manifest["status"] == "complete" and manifest["distinct_evaluated_candidates"] == 1
    for artifact in manifest["outputs"]:
        assert sha256(Path(artifact["path"])) == artifact["sha256"]
    decision = json.loads((tmp_path / "metrics/qualification.json").read_text())
    assert not decision["candidates_and_arms"]["quiet_explorer/primary"]["qualified"]
    assert decision["simulated_refresh_composition"]["matched_markets"] == 0
    time_bucket_results.build(tmp_path)
    (tmp_path / "manifests/final-models.json").write_text(json.dumps({"status": "complete_with_explicit_support_outcomes",
        "candidates": [{"candidate": arm["candidate"], "status": "unsupported_no_final_artifact"} for arm in arms]}))
    for name in ["complete-case-panels", "quiet-complete-case-panels"]:
        (tmp_path / "manifests" / f"{name}.json").write_text('{"days":[]}')
    monkeypatch.setattr(time_bucket_report, "config", lambda _: frozen)
    monkeypatch.setattr(time_bucket_report, "arms_at_freeze", lambda _: arms)
    time_bucket_report.build(tmp_path)
    report_manifest = json.loads((tmp_path / "manifests/tournament-report.json").read_text())
    assert report_manifest["status"] == "complete"
    for artifact in report_manifest["outputs"]:
        assert sha256(Path(artifact["path"])) == artifact["sha256"]
    assert "No OOS predictions: performance unavailable" in (tmp_path / "diagnostics/robustness/quiet_explorer.html").read_text()


def passing_gate_inputs():
    metrics = {
        "net_pnl": 10.0,
        "stress_pnl": 8.0,
        "stressed_profit_factor": 1.1,
        "recovery_ratio": 0.9,
        "markets": 50,
        "unknown_attempts": 0,
        "fills_per_day": 1.0,
    }
    folds = [
        {"fold": f"fold_{i + 1}", "stress_pnl": pnl, "fills": 5, "predictions": 10}
        for i, pnl in enumerate([2.0, 2.0, 2.0, -1.0])
    ]
    folds.append({"fold": "holdout", "stress_pnl": 2.0, "fills": 10, "predictions": 10})
    robust = {
        "best_day_deleted_stress": 0.0,
        "maximum_positive_day_share": 0.5,
        "bootstrap_low": -1000.0,
        "bootstrap_high": 1000.0,
    }
    complete = {"days": 1, **metrics}
    return metrics, folds, robust, complete, {"quantity": True, "time": True}


def test_aspirational_targets_and_negative_bootstrap_bound_are_not_gates():
    result = qualification(*passing_gate_inputs(), FROZEN)
    assert result["qualified"]
    assert result["failed_gates"] == []
    assert result["conditional_on_label_timing_assumption"] is True
    assert result["deployment_authorized"] is False


@pytest.mark.parametrize(
    "section,key,value,gate",
    [
        (0, "net_pnl", 0.0, "positive_net"),
        (0, "net_pnl", -1.0, "positive_net"),
        (0, "stress_pnl", -1.0, "positive_stressed"),
        (0, "stressed_profit_factor", 1.0, "stressed_pf"),
        (0, "stressed_profit_factor", None, "stressed_pf"),
        (0, "recovery_ratio", 1.0, "recovery"),
        (0, "markets", 49, "market_diversity"),
        (0, "unknown_attempts", 1, "known_selected_execution"),
        (2, "best_day_deleted_stress", -0.01, "best_day_deleted_nonnegative"),
        (2, "maximum_positive_day_share", 0.5001, "day_concentration"),
        (2, "maximum_positive_day_share", None, "day_concentration"),
        (3, "days", 0, "fully_covered_days"),
        (4, "quantity", False, "adjacent_quantity"),
        (4, "time", False, "adjacent_time"),
    ],
)
def test_each_mandatory_gate_failure_rejects_qualification(section, key, value, gate):
    inputs = passing_gate_inputs()
    inputs[section][key] = value
    result = qualification(*inputs, FROZEN)
    assert not result["qualified"]
    assert gate in result["failed_gates"]


def test_absent_holdout_predictions_and_fold_concentration_fail():
    inputs = passing_gate_inputs()
    inputs[1][-1].update(predictions=0, fills=0, stress_pnl=0.0)
    result = qualification(*inputs, FROZEN)
    assert {"positive_holdout", "holdout_samples"} <= set(result["failed_gates"])
    inputs = passing_gate_inputs()
    inputs[1][0]["stress_pnl"] = 100
    result = qualification(*inputs, FROZEN)
    assert "fold_concentration" in result["failed_gates"]
    inputs = passing_gate_inputs()
    inputs[1][0].update(stress_pnl=-1, fills=4)
    result = qualification(*inputs, FROZEN)
    assert {"three_positive_principal_folds", "principal_samples"} <= set(result["failed_gates"])


def trade(market="m", at=START, pnl=1.0, **changes):
    return {
        "point_id": market + ":attempt",
        "market_id": market,
        "decision_at": at,
        "evidence_known": True,
        "exit_evidence_known": True,
        "net_pnl": pnl,
        "stress_pnl": pnl,
        "requested_quantity": 5.0,
        "filled_quantity": 5.0,
        "filled_notional": 2.0,
        "economic_view": "sequential_policy",
        "scenario": "base",
        "timing_control": "selected_wait_policy",
        "population_control": "frozen_candidate_population",
        "control": "hold",
        "order_type": "FAK",
        "bucket_start": 80,
        "fold": "fold_1",
        **changes,
    }


def ledger(records):
    return pl.DataFrame(
        records,
        schema_overrides={
            "net_pnl": pl.Float64,
            "stress_pnl": pl.Float64,
            "requested_quantity": pl.Float64,
            "filled_quantity": pl.Float64,
            "filled_notional": pl.Float64,
        },
    )


def empty_ledger():
    return ledger([trade()]).head(0)


def market_predictions(market, start, **changes):
    return [
        {
            "market_id": market,
            "point_id": f"{market}:{b + 19}",
            "window_start": start,
            "decision_at": start + timedelta(seconds=b + 19),
            "bucket_start": b,
            "entry_offset": 19,
            "evaluation_clock_eligible": True,
            "probability_up": 0.6,
            "up_fak_evidence_known": True,
            "down_fak_evidence_known": True,
            "reference_availability_known": True,
            "feature_eligible": True,
            "admission_reason": "confidence_below_threshold",
            **changes,
        }
        for b in FROZEN["bucket_starts"]
    ]


def hour_predictions(hour, **changes):
    return [
        row
        for i in range(12)
        for row in market_predictions(
            f"h{hour}:m{i}", START + timedelta(hours=hour, minutes=5 * i), **changes
        )
    ]


def test_unknown_exit_is_not_valued_as_zero_or_counted_as_profitable_fill():
    frame = ledger(
        [
            trade("known", pnl=2),
            trade("unknown", pnl=100, stress_pnl=100, exit_evidence_known=False),
            trade("unknown_null", pnl=None, stress_pnl=None, exit_evidence_known=False),
        ]
    )
    result = performance(frame)
    assert result["fills"] == 1
    assert result["observed_entry_fills"] == 3
    assert result["unknown_attempts"] == 2
    assert result["unvalued_entry_fills"] == 2
    assert result["net_pnl"] == result["stress_pnl"] == 2
    assert result["filled_notional"] == 6


def test_hourly_gaps_partial_untested_disabled_and_observed_zero_are_distinct():
    rows = (
        hour_predictions(0)
        + hour_predictions(1)[:-1]
        + hour_predictions(2, probability_up=None)
        + hour_predictions(4, admission_reason="policy_abstain")
        + hour_predictions(5)
        + hour_predictions(6)
    )
    panel = pl.DataFrame(rows)
    trades = ledger(
        [
            trade(
                "h5:m0",
                START + timedelta(hours=5),
                pnl=None,
                stress_pnl=None,
                exit_evidence_known=False,
            ),
            trade("h6:m0", START + timedelta(hours=6)),
        ]
    )
    hours = coverage_hours(panel, trades, ARM, FROZEN)
    assert hours.height == 24
    assert hours["coverage_status"].head(7).to_list() == [
        "no_trade_covered",
        "partial_coverage",
        "untested",
        "missing_source",
        "policy_disabled",
        "partial_coverage",
        "fully_covered",
    ]
    assert hours["filled_trades"][0] == 0
    assert hours["filled_trades"][3] is None
    assert hours["net_pnl"][3] is None
    assert not hours["fully_covered"][1]
    assert not hours["fully_covered"][5]


def test_partial_utc_day_cannot_satisfy_fully_covered_day_gate():
    panel = pl.DataFrame([row for hour in range(24) for row in hour_predictions(hour)])
    full = coverage_hours(panel, empty_ledger(), ARM, FROZEN)
    assert daily_results(panel, empty_ledger(), full)["fully_covered"].item()
    partial = panel.head(panel.height - 1)
    hours = coverage_hours(partial, empty_ledger(), ARM, FROZEN)
    assert not daily_results(partial, empty_ledger(), hours)["fully_covered"].item()


def test_gap_hour_cannot_be_claimed_as_preceding_verified_quiet():
    hours = pl.DataFrame(
        {
            "hour": [START, START + timedelta(hours=1), START + timedelta(hours=2)],
            "coverage_status": ["no_trade_covered", "missing_source", "fully_covered"],
        }
    )
    bursts = activity_bursts(
        ledger([trade(at=START + timedelta(hours=2, minutes=5))]), hours, FROZEN
    )
    assert bursts[0]["preceding_verified_quiet_minutes"] is None
    assert bursts[0]["economics_complete"]
    unknown = activity_bursts(
        ledger(
            [trade(at=START + timedelta(hours=2), pnl=50, stress_pnl=50, exit_evidence_known=False)]
        ),
        hours,
        FROZEN,
    )
    assert unknown[0]["net_pnl"] is None
    assert unknown[0]["stress_pnl"] is None


def neighbor_run(tmp_path, selected):
    for fold in FROZEN["folds"]:
        root = tmp_path / "checkpoints" / ARM["candidate"] / ARM["arm"] / fold["name"]
        root.mkdir(parents=True)
        complete = fold["name"] == "fold_1"
        (root / "manifest.json").write_text(
            json.dumps({"status": "complete" if complete else "insufficient_support"})
        )
        if complete:
            (root / "quantity-selection.json").write_text(json.dumps({"selected": selected}))
            (root / "policy.json").write_text(json.dumps({"selected": {"bucket_mask": [80]}}))
    return tmp_path


def neighbor_ledger(quantities):
    records = [
        trade(f"quantity:{q}:m{i}", requested_quantity=float(q), filled_quantity=float(q))
        for q in quantities
        for i in range(5)
    ]
    records += [
        trade(f"bucket:{b}:m{i}", economic_view="bucket_diagnostic", bucket_start=b)
        for b in [60, 100]
        for i in range(5)
    ]
    return ledger(records)


def test_neighbor_evidence_is_required_and_negative_or_missing_is_not_zero(tmp_path):
    run = neighbor_run(tmp_path, 5)
    frame = neighbor_ledger([1, 10])
    passed, _ = neighboring_checks(run, ARM, frame, FROZEN)
    assert passed == {"quantity": True, "time": True}
    missing = frame.filter(
        ~((pl.col("economic_view") == "sequential_policy") & (pl.col("requested_quantity") == 10))
    )
    result, _ = neighboring_checks(run, ARM, missing, FROZEN)
    assert not result["quantity"]
    adverse = frame.with_columns(
        pl.when(pl.col("bucket_start") == 100)
        .then(-1.0)
        .otherwise(pl.col("stress_pnl"))
        .alias("stress_pnl")
    )
    result, _ = neighboring_checks(run, ARM, adverse, FROZEN)
    assert not result["time"]
    absent, _ = neighboring_checks(run, ARM, frame.head(0), FROZEN)
    assert absent == {"quantity": False, "time": False}


def test_endpoint_quantity_requires_only_its_available_immediate_neighbor(tmp_path):
    # Frozen language says BOTH AVAILABLE neighbors; no quantity below Q1 exists.
    run = neighbor_run(tmp_path, 1)
    result, _ = neighboring_checks(run, ARM, neighbor_ledger([5]), FROZEN)
    assert result == {"quantity": True, "time": True}


def test_composition_chronology_refresh_tie_priority_and_unknown_no_retry():
    markets = ["quiet_first", "tie", "unknown_first", "unfilled_first", "missing"]
    predictions = pl.DataFrame([r for market in markets for r in market_predictions(market, START)])
    quiet_predictions = predictions.with_columns(
        pl.when(pl.col("market_id") == "missing")
        .then(False)
        .otherwise(pl.col("reference_availability_known"))
        .alias("reference_availability_known")
    )
    refreshed = ledger(
        [
            trade("quiet_first", START + timedelta(seconds=10), candidate="refresh"),
            trade("tie", START + timedelta(seconds=10), candidate="refresh"),
            trade(
                "unknown_first",
                START + timedelta(seconds=5),
                pnl=None,
                stress_pnl=None,
                exit_evidence_known=False,
                candidate="refresh",
            ),
            trade(
                "unfilled_first",
                START + timedelta(seconds=5),
                pnl=0,
                stress_pnl=0,
                filled_quantity=0,
                filled_notional=0,
                candidate="refresh",
            ),
            trade("missing", candidate="refresh"),
        ]
    )
    quiet = ledger(
        [
            trade(
                market,
                START + timedelta(seconds=5 if market == "quiet_first" else 10),
                pnl=100,
                candidate="quiet",
            )
            for market in markets
        ]
    )
    result, merged = composition((predictions, refreshed), (quiet_predictions, quiet))
    selected = {r["market_id"]: r for r in merged.to_dicts()}
    assert result["matched_markets"] == 4
    assert selected["quiet_first"]["candidate"] == "quiet"
    assert selected["tie"]["candidate"] == "refresh"
    assert selected["unknown_first"]["candidate"] == "refresh"
    assert selected["unknown_first"]["stress_pnl"] is None
    assert selected["unfilled_first"]["candidate"] == "refresh"
    assert selected["unfilled_first"]["filled_quantity"] == 0
    assert merged["market_id"].n_unique() == merged.height == 4
    assert not result["actual_incumbent_complementarity_proven"]
    assert result["conditional_on_label_timing_assumption"]


def test_composition_requires_complete_scheduled_market_not_only_existing_rows():
    complete = pl.DataFrame(market_predictions("m", START))
    incomplete = complete.head(complete.height - 1)
    result, merged = composition(
        (complete, ledger([trade(candidate="refresh")])),
        (incomplete, ledger([trade(candidate="quiet")])),
    )
    assert result["matched_markets"] == 0
    assert merged.is_empty()


def test_utc_day_grouping_and_day_bootstrap_keep_intraday_returns_together():
    first = START + timedelta(hours=23, minutes=59)
    second = START + timedelta(days=1)
    trades = ledger(
        [
            trade("a", first - timedelta(seconds=1), pnl=11),
            trade("b", first, pnl=-6),
            trade("c", second, pnl=11),
            trade("d", second + timedelta(seconds=1), pnl=-6),
        ]
    )
    predictions = pl.DataFrame({"window_start": [START, second]})
    hours = pl.DataFrame(
        {
            "date": [str(START.date()), str(second.date())],
            "fully_covered": [True, True],
            "prediction_points": [156, 156],
        }
    )
    daily = daily_results(predictions, trades, hours)
    assert daily["stress_pnl"].to_list() == [5.0, 5.0]
    result = robustness(daily, FROZEN)
    # Resampling complete days has no variation; resampling four trades would.
    assert result["bootstrap_low"] == result["bootstrap_high"] == 10
    assert result["best_day_deleted_stress"] == result["worst_day_deleted_stress"] == 5
    assert result["maximum_positive_day_share"] == 0.5


def test_best_worst_day_deletion_and_unobserved_day_exclusion():
    daily = pl.DataFrame(
        {
            "date": ["2026-09-14", "2026-09-15", "2026-09-16", "2026-09-17"],
            "stress_pnl": [8.0, -3.0, 1.0, 999.0],
            "observed_hours": [24, 24, 24, 0],
        }
    )
    result = robustness(daily, copy.deepcopy(FROZEN))
    assert result["observed_days"] == 3
    assert result["best_day"] == "2026-09-14"
    assert result["worst_day"] == "2026-09-15"
    assert result["best_day_deleted_stress"] == -2
    assert result["worst_day_deleted_stress"] == 9
    assert result["maximum_positive_day_share"] == pytest.approx(8 / 9)
