"""From-scratch directional plus loss-risk cross-fitted training."""

from __future__ import annotations

import argparse
import itertools
import json
import platform
import subprocess
import tomllib
from dataclasses import asdict, dataclass
from datetime import datetime, timedelta
from pathlib import Path
from typing import Any

import joblib
import numpy as np
import polars as pl
import sklearn
from sklearn.ensemble import HistGradientBoostingClassifier
from sklearn.linear_model import LogisticRegression

from .conservative_selective_training import (
    KEYS,
    PRICE_FEATURES,
    Policy,
    expected_calibration_error,
    readiness,
    replay,
    sha256,
    trade_metrics,
    weights,
)
from .loss_streak_analysis import add_streak_ids

SCHEMA_VERSION = "btc-loss-aware-selective-training-v1"
RISK_FEATURES = (
    "directional_confidence",
    "share_cost",
    "stressed_edge",
    "seconds_elapsed_scaled",
    "selected_up",
    "btc_realized_volatility_5s_bps",
    "btc_realized_volatility_30s_bps",
    "btc_realized_volatility_60s_bps",
    "btc_realized_volatility_120s_bps",
    "btc_volatility_shock_30_vs_120",
    "btc_volatility_shock_60_vs_180",
    "btc_boundary_distance_velocity_5s_bps",
    "btc_boundary_momentum_alignment_5s",
    "btc_momentum_multihorizon_score",
    "btc_momentum_acceleration_5_vs_30",
    "btc_momentum_acceleration_15_vs_60",
    "btc_reversal_5_vs_30",
    "btc_path_efficiency_30s",
    "btc_path_efficiency_60s",
    "btc_range_position_60s",
    "btc_path_cross_count",
    "btc_boundary_cross_count",
    "btc_seconds_since_path_cross",
    "btc_seconds_since_boundary_cross",
    "btc_path_pullback_from_favorable_extreme_bps",
    "btc_path_recovery_from_adverse_extreme_bps",
)


@dataclass(frozen=True)
class CombinedPolicy:
    directional: Policy
    maximum_loss_probability: float


def parse(value: str) -> datetime:
    return datetime.fromisoformat(value)


def _fit_directional(
    fit: pl.DataFrame, calibration: pl.DataFrame, raw: dict[str, Any]
) -> tuple[Any, Any, float]:
    d = raw["directional"]
    estimator = HistGradientBoostingClassifier(
        learning_rate=d["learning_rate"],
        max_iter=d["max_iter"],
        max_bins=d["max_bins"],
        max_leaf_nodes=d["max_leaf_nodes"],
        min_samples_leaf=d["min_samples_leaf"],
        l2_regularization=d["l2_regularization"],
        early_stopping=False,
        random_state=raw["training"]["random_seed"],
    )
    y = (fit["official_outcome"] == "up").cast(pl.Int8).to_numpy()
    estimator.fit(fit.select(PRICE_FEATURES).to_numpy(), y, sample_weight=weights(fit))
    raw_p = np.clip(
        estimator.predict_proba(calibration.select(PRICE_FEATURES).to_numpy())[:, 1], 1e-6, 1 - 1e-6
    )
    logits = np.log(raw_p / (1 - raw_p)).reshape(-1, 1)
    calibrator = LogisticRegression(
        C=1.0, max_iter=500, random_state=raw["training"]["random_seed"]
    )
    cy = (calibration["official_outcome"] == "up").cast(pl.Int8).to_numpy()
    calibrator.fit(logits, cy, sample_weight=weights(calibration))
    calibrated = calibrator.predict_proba(logits)[:, 1]
    return estimator, calibrator, expected_calibration_error(cy, calibrated, 10)


def _score_directional(
    frame: pl.DataFrame, model: tuple[Any, Any, float], raw: dict[str, Any]
) -> pl.DataFrame:
    estimator, calibrator, ece = model
    raw_p = np.clip(
        estimator.predict_proba(frame.select(PRICE_FEATURES).to_numpy())[:, 1], 1e-6, 1 - 1e-6
    )
    probability = calibrator.predict_proba(np.log(raw_p / (1 - raw_p)).reshape(-1, 1))[:, 1]
    penalty = max(ece, raw["directional"]["calibration_safety_floor"])
    conservative = np.where(
        probability >= 0.5,
        np.maximum(0.5, probability - penalty),
        np.minimum(0.5, probability + penalty),
    )
    return frame.with_columns(
        pl.Series("directional_raw_up", raw_p),
        pl.Series("probability_up", probability),
        pl.Series("conservative_probability_up", conservative),
    )


def _candidate_rows(frame: pl.DataFrame, raw: dict[str, Any]) -> pl.DataFrame:
    c, p = raw["candidate"], raw["policy"]
    policy = Policy(c["confidence_floor"], c["minimum_stressed_edge"], c["maximum_share_cost"])
    candidates = replay(
        frame, policy, p["execution_reserve_per_share"], p["stress_slippage_per_share"]
    )
    if candidates.is_empty():
        return candidates
    return add_streak_ids(candidates).with_columns(
        (~pl.col("won")).cast(pl.Int8).alias("loss_label"),
        (pl.col("side") == "up").cast(pl.Float64).alias("selected_up"),
        pl.col("confidence").alias("directional_confidence"),
    )


def _risk_weights(frame: pl.DataFrame, raw: dict[str, Any]) -> np.ndarray:
    r = raw["risk"]
    severity = np.where(
        frame["loss_label"].to_numpy() == 1,
        np.minimum(r["severity_weight_cap"], 1 + np.abs(frame["stress_pnl"].to_numpy()) / 2.5),
        1.0,
    )
    streak = np.where(frame["loss_streak_length"].to_numpy() >= 2, r["streak_loss_weight"], 1.0)
    return severity * streak


def _fit_risk(fit: pl.DataFrame, calibration: pl.DataFrame, raw: dict[str, Any]) -> tuple[Any, Any]:
    r = raw["risk"]
    estimator = HistGradientBoostingClassifier(
        learning_rate=r["learning_rate"],
        max_iter=r["max_iter"],
        max_bins=r["max_bins"],
        max_leaf_nodes=r["max_leaf_nodes"],
        min_samples_leaf=r["min_samples_leaf"],
        l2_regularization=r["l2_regularization"],
        early_stopping=False,
        random_state=raw["training"]["random_seed"],
    )
    estimator.fit(
        fit.select(RISK_FEATURES).to_numpy(),
        fit["loss_label"].to_numpy(),
        sample_weight=_risk_weights(fit, raw),
    )
    raw_loss = np.clip(
        estimator.predict_proba(calibration.select(RISK_FEATURES).to_numpy())[:, 1], 1e-6, 1 - 1e-6
    )
    calibrator = LogisticRegression(
        C=1.0, max_iter=500, random_state=raw["training"]["random_seed"]
    )
    calibrator.fit(
        np.log(raw_loss / (1 - raw_loss)).reshape(-1, 1), calibration["loss_label"].to_numpy()
    )
    return estimator, calibrator


def _attach_risk(frame: pl.DataFrame, model: tuple[Any, Any]) -> pl.DataFrame:
    side_up = pl.col("probability_up") >= 0.5
    enriched = frame.with_columns(
        pl.max_horizontal(
            "conservative_probability_up", 1 - pl.col("conservative_probability_up")
        ).alias("directional_confidence"),
        pl.when(side_up)
        .then(pl.col("up_ask_vwap_5"))
        .otherwise(pl.col("down_ask_vwap_5"))
        .alias("share_cost"),
        side_up.cast(pl.Float64).alias("selected_up"),
    ).with_columns(
        (pl.col("directional_confidence") - pl.col("share_cost") - 0.015).alias("stressed_edge")
    )
    estimator, calibrator = model
    raw_loss = np.clip(
        estimator.predict_proba(enriched.select(RISK_FEATURES).to_numpy())[:, 1], 1e-6, 1 - 1e-6
    )
    probability = calibrator.predict_proba(np.log(raw_loss / (1 - raw_loss)).reshape(-1, 1))[:, 1]
    return enriched.with_columns(
        pl.Series("loss_probability", probability), pl.Series("loss_raw", raw_loss)
    )


def _combined_replay(
    frame: pl.DataFrame, policy: CombinedPolicy, raw: dict[str, Any]
) -> pl.DataFrame:
    filtered = frame.filter(pl.col("loss_probability") <= policy.maximum_loss_probability)
    p = raw["policy"]
    return replay(
        filtered,
        policy.directional,
        p["execution_reserve_per_share"],
        p["stress_slippage_per_share"],
    )


def _select_combined(
    frame: pl.DataFrame, raw: dict[str, Any]
) -> tuple[CombinedPolicy | None, list[dict[str, Any]]]:
    p = raw["policy"]
    rows = []
    for values in itertools.product(
        p["confidence_floors"],
        p["minimum_stressed_edges"],
        p["maximum_share_costs"],
        raw["risk"]["risk_thresholds"],
    ):
        policy = CombinedPolicy(Policy(*values[:3]), values[3])
        metrics = trade_metrics(_combined_replay(frame, policy, raw))
        ratio = metrics["compensation_ratio"]
        qualifies = (
            metrics["trades"] >= p["minimum_selection_trades"]
            and metrics["precision"] is not None
            and metrics["precision"] >= p["minimum_selection_precision"]
            and metrics["stress_pnl"] > 0
            and ratio is not None
            and ratio <= p["maximum_compensation_ratio"]
        )
        rows.append(
            {
                "policy": {
                    "directional": asdict(policy.directional),
                    "maximum_loss_probability": policy.maximum_loss_probability,
                },
                "qualifies": qualifies,
                **metrics,
            }
        )
    rows.sort(
        key=lambda x: (
            x["qualifies"],
            x["wilson_lower"],
            x["stress_pnl"] / max(x["trades"], 1),
            -x["trades"],
        ),
        reverse=True,
    )
    if not rows or not rows[0]["qualifies"]:
        return None, rows
    best = rows[0]["policy"]
    return CombinedPolicy(Policy(**best["directional"]), best["maximum_loss_probability"]), rows


def _outcome_checks(
    new: dict[str, Any],
    baseline: dict[str, Any],
    economics: dict[str, float | int],
    raw: dict[str, Any],
) -> dict[str, bool]:
    q = raw["qualification"]
    return {
        "precision": new["precision"] is not None and new["precision"] >= q["minimum_precision"],
        "wilson": new["wilson_lower"] >= q["minimum_wilson"],
        "stressed_pf": new["stressed_profit_factor"] is not None
        and new["stressed_profit_factor"] >= q["minimum_stressed_profit_factor"],
        "recovery": new["compensation_ratio"] is not None
        and new["compensation_ratio"] <= q["maximum_compensation_ratio"],
        "loss_streak": new["longest_losing_streak"] <= q["maximum_losing_streak"],
        "streak_improvement": new["longest_losing_streak"] < baseline["longest_losing_streak"],
        "pnl_retention": new["stress_pnl"]
        >= q["minimum_stress_pnl_retention"] * baseline["stress_pnl"],
        "remove_best_day_positive": new["remove_best_day_stress_pnl"] > 0,
        "rejection_economics": economics["loss_dollars_avoided"]
        > economics["win_dollars_rejected"],
    }


def _qualified(
    new: dict[str, Any],
    baseline: dict[str, Any],
    excluding: dict[str, Any],
    baseline_excluding: dict[str, Any],
    economics: dict[str, float | int],
    excluding_economics: dict[str, float | int],
    raw: dict[str, Any],
) -> dict[str, Any]:
    q = raw["qualification"]
    checks = {
        "trades": new["trades"] >= q["minimum_trades"],
        "weeks": new["weeks"] >= q["minimum_weeks"],
        **_outcome_checks(new, baseline, economics, raw),
    }
    outside_checks = _outcome_checks(excluding, baseline_excluding, excluding_economics, raw)
    checks["outside_may10"] = all(outside_checks.values())
    return {"passed": all(checks.values()), "checks": checks}


def _rejection_economics(
    new_ledger: pl.DataFrame, baseline_ledger: pl.DataFrame
) -> dict[str, float | int]:
    if baseline_ledger.is_empty():
        return {
            "baseline_trades_rejected": 0,
            "loss_dollars_avoided": 0.0,
            "win_dollars_rejected": 0.0,
            "net_rejection_value": 0.0,
        }
    new_ids = set(new_ledger["market_id"].to_list()) if not new_ledger.is_empty() else set()
    rejected = baseline_ledger.filter(~pl.col("market_id").is_in(new_ids))
    pnl = rejected["stress_pnl"].to_numpy()
    avoided = float(-pnl[pnl < 0].sum())
    forfeited = float(pnl[pnl > 0].sum())
    return {
        "baseline_trades_rejected": rejected.height,
        "loss_dollars_avoided": avoided,
        "win_dollars_rejected": forfeited,
        "net_rejection_value": avoided - forfeited,
    }


def run(config: Path, output: Path) -> None:
    raw = tomllib.loads(config.read_text())
    if (
        not raw["training"]["training_only"]
        or not raw["training"]["paper_only"]
        or raw["training"]["live_capital_allowed"]
    ):
        raise RuntimeError("loss-aware run must remain offline and paper-only")
    panel = Path(raw["source"]["panel"])
    columns = list(dict.fromkeys((*KEYS, *PRICE_FEATURES)))
    frame = pl.read_parquet(panel, columns=columns).with_columns(
        [pl.col(c).cast(pl.Float64).fill_nan(None) for c in PRICE_FEATURES]
    )
    audit_raw = {"source": raw["source"]}
    audit = readiness(frame, {"source": raw["source"]}, panel)
    del audit_raw
    start, end = parse(raw["source"]["source_start"]), parse(raw["source"]["source_end"])
    t = raw["training"]
    cal_days = t["calibration_days"]
    select_days = t["selection_days"]
    test_days = t["test_days"]
    first_test = start + timedelta(days=t["minimum_fit_days"] + cal_days + select_days)
    directional_parts = []
    cursor = first_test
    fold = 0
    while cursor < end:
        fit_end = cursor - timedelta(days=cal_days + select_days)
        cal_end = cursor - timedelta(days=select_days)
        fit = frame.filter((pl.col("window_start") >= start) & (pl.col("window_start") < fit_end))
        cal = frame.filter((pl.col("window_start") >= fit_end) & (pl.col("window_start") < cal_end))
        test_end = min(cursor + timedelta(days=test_days), end)
        test = frame.filter(
            (pl.col("window_start") >= cursor) & (pl.col("window_start") < test_end)
        )
        model = _fit_directional(fit, cal, raw)
        directional_parts.append(
            _score_directional(test, model, raw).with_columns(
                pl.lit(fold).alias("directional_fold")
            )
        )
        cursor += timedelta(days=test_days)
        fold += 1
    directional_oof = pl.concat(directional_parts).sort("window_start")
    candidates = _candidate_rows(directional_oof, raw)
    risk_outputs = []
    combined_trades = []
    baseline_trades = []
    fold_reports = []
    risk_start = candidates["window_start"].min() + timedelta(
        days=t["minimum_risk_fit_days"] + raw["risk"]["calibration_days"] + select_days
    )
    cursor = risk_start
    risk_fold = 0
    risk_end = directional_oof["window_start"].max() + timedelta(minutes=5)
    while cursor < risk_end:
        rfit_end = cursor - timedelta(days=raw["risk"]["calibration_days"] + select_days)
        rcal_end = cursor - timedelta(days=select_days)
        rfit = candidates.filter(pl.col("window_start") < rfit_end)
        rcal = candidates.filter(
            (pl.col("window_start") >= rfit_end) & (pl.col("window_start") < rcal_end)
        )
        selection = directional_oof.filter(
            (pl.col("window_start") >= rcal_end) & (pl.col("window_start") < cursor)
        )
        test_end = min(cursor + timedelta(days=test_days), risk_end)
        test = directional_oof.filter(
            (pl.col("window_start") >= cursor) & (pl.col("window_start") < test_end)
        )
        if rfit.height < 100 or rcal.height < 30:
            cursor += timedelta(days=test_days)
            continue
        risk_model = _fit_risk(rfit, rcal, raw)
        scored_selection = _attach_risk(selection, risk_model)
        combined_policy, search = _select_combined(scored_selection, raw)
        scored_test = _attach_risk(test, risk_model).with_columns(
            pl.lit(risk_fold).alias("risk_fold")
        )
        risk_outputs.append(scored_test)
        new_trades = (
            _combined_replay(scored_test, combined_policy, raw)
            if combined_policy
            else pl.DataFrame()
        )
        if not new_trades.is_empty():
            combined_trades.append(new_trades)
        # Matched baseline is chosen independently on the same selection block.
        best_base = None
        best_rows = []
        for vals in itertools.product(
            raw["policy"]["confidence_floors"],
            raw["policy"]["minimum_stressed_edges"],
            raw["policy"]["maximum_share_costs"],
        ):
            pol = Policy(*vals)
            met = trade_metrics(
                replay(
                    selection,
                    pol,
                    raw["policy"]["execution_reserve_per_share"],
                    raw["policy"]["stress_slippage_per_share"],
                )
            )
            ratio = met["compensation_ratio"]
            ok = (
                met["trades"] >= raw["policy"]["minimum_selection_trades"]
                and met["precision"] is not None
                and met["precision"] >= raw["policy"]["minimum_selection_precision"]
                and met["stress_pnl"] > 0
                and ratio is not None
                and ratio <= raw["policy"]["maximum_compensation_ratio"]
            )
            best_rows.append(
                (ok, met["wilson_lower"], met["stress_pnl"] / max(met["trades"], 1), pol)
            )
        best_rows.sort(key=lambda x: x[:3], reverse=True)
        best_base = best_rows[0][3] if best_rows[0][0] else None
        base_test = (
            replay(
                test,
                best_base,
                raw["policy"]["execution_reserve_per_share"],
                raw["policy"]["stress_slippage_per_share"],
            )
            if best_base
            else pl.DataFrame()
        )
        if not base_test.is_empty():
            baseline_trades.append(base_test)
        fold_reports.append(
            {
                "fold": risk_fold,
                "test_start": cursor.isoformat(),
                "combined_policy": asdict(combined_policy) if combined_policy else None,
                "risk_search_top": search[:5],
            }
        )
        cursor += timedelta(days=test_days)
        risk_fold += 1
    risk_oof = pl.concat(risk_outputs) if risk_outputs else pl.DataFrame()
    new_ledger = (
        pl.concat(combined_trades).sort("window_start") if combined_trades else pl.DataFrame()
    )
    base_ledger = (
        pl.concat(baseline_trades).sort("window_start") if baseline_trades else pl.DataFrame()
    )
    new_metrics, base_metrics = trade_metrics(new_ledger), trade_metrics(base_ledger)
    cutoff_start, cutoff_end = (
        datetime.fromisoformat("2026-05-10T00:00:00+00:00"),
        datetime.fromisoformat("2026-05-11T00:00:00+00:00"),
    )
    outside_may10 = (pl.col("window_start") < cutoff_start) | (pl.col("window_start") >= cutoff_end)
    new_excluding_ledger = new_ledger.filter(outside_may10)
    base_excluding_ledger = base_ledger.filter(outside_may10)
    excluding = trade_metrics(new_excluding_ledger)
    baseline_excluding = trade_metrics(base_excluding_ledger)
    rejection_economics = _rejection_economics(new_ledger, base_ledger)
    excluding_rejection_economics = _rejection_economics(
        new_excluding_ledger, base_excluding_ledger
    )
    gate = _qualified(
        new_metrics,
        base_metrics,
        excluding,
        baseline_excluding,
        rejection_economics,
        excluding_rejection_economics,
        raw,
    )
    # Final heads refit from full eligible coverage; calibration comes from cross-fitted predictions.
    final_directional = HistGradientBoostingClassifier(
        learning_rate=raw["directional"]["learning_rate"],
        max_iter=raw["directional"]["max_iter"],
        max_bins=raw["directional"]["max_bins"],
        max_leaf_nodes=raw["directional"]["max_leaf_nodes"],
        min_samples_leaf=raw["directional"]["min_samples_leaf"],
        l2_regularization=raw["directional"]["l2_regularization"],
        early_stopping=False,
        random_state=t["random_seed"],
    )
    final_directional.fit(
        frame.select(PRICE_FEATURES).to_numpy(),
        (frame["official_outcome"] == "up").cast(pl.Int8).to_numpy(),
        sample_weight=weights(frame),
    )
    dcal = LogisticRegression(C=1, max_iter=500, random_state=t["random_seed"])
    rp = np.clip(directional_oof["directional_raw_up"].to_numpy(), 1e-6, 1 - 1e-6)
    dcal.fit(
        np.log(rp / (1 - rp)).reshape(-1, 1),
        (directional_oof["official_outcome"] == "up").cast(pl.Int8).to_numpy(),
        sample_weight=weights(directional_oof),
    )
    final_risk = _fit_risk(
        candidates.filter(
            pl.col("window_start") < candidates["window_start"].max() - timedelta(days=7)
        ),
        candidates.filter(
            pl.col("window_start") >= candidates["window_start"].max() - timedelta(days=7)
        ),
        raw,
    )
    final_policy, _ = _select_combined(risk_oof, raw) if not risk_oof.is_empty() else (None, [])
    status = "historical_qualified" if gate["passed"] and final_policy else "historical_failed"
    output.mkdir(parents=True, exist_ok=False)
    for name in ("manifests", "predictions", "trades", "models", "metrics"):
        (output / name).mkdir()
    directional_oof.write_parquet(output / "predictions/directional-oof.parquet")
    candidates.write_parquet(output / "predictions/risk-candidates.parquet")
    if not risk_oof.is_empty():
        risk_oof.write_parquet(output / "predictions/risk-oof.parquet")
    if not new_ledger.is_empty():
        new_ledger.write_parquet(output / "trades/loss-aware.parquet")
    if not base_ledger.is_empty():
        base_ledger.write_parquet(output / "trades/baseline.parquet")
    artifact = (
        output
        / "models"
        / ("qualified.joblib" if status == "historical_qualified" else "diagnostic.joblib")
    )
    joblib.dump(
        {
            "schema_version": SCHEMA_VERSION,
            "directional_estimator": final_directional,
            "directional_calibrator": dcal,
            "risk_estimator": final_risk[0],
            "risk_calibrator": final_risk[1],
            "features": PRICE_FEATURES,
            "risk_features": RISK_FEATURES,
            "policy": final_policy,
            "source_sha256": audit["sha256"],
        },
        artifact,
        compress=3,
    )
    git = {
        "branch": subprocess.check_output(["git", "branch", "--show-current"], text=True).strip(),
        "commit": subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip(),
        "dirty": bool(subprocess.check_output(["git", "status", "--porcelain"], text=True).strip()),
    }
    metrics = {
        "schema_version": SCHEMA_VERSION,
        "status": status,
        "readiness": audit,
        "new": new_metrics,
        "baseline": base_metrics,
        "excluding_may10": excluding,
        "baseline_excluding_may10": baseline_excluding,
        "rejection_economics": rejection_economics,
        "excluding_may10_rejection_economics": excluding_rejection_economics,
        "qualification": gate,
        "folds": fold_reports,
        "final_policy": asdict(final_policy) if final_policy else None,
        "git": git,
        "runtime": {
            "python": platform.python_version(),
            "numpy": np.__version__,
            "polars": pl.__version__,
            "sklearn": sklearn.__version__,
        },
        "config_sha256": sha256(config),
        "model_artifact": artifact.name,
        "model_sha256": sha256(artifact),
    }
    (output / "metrics/results.json").write_text(
        json.dumps(metrics, indent=2, sort_keys=True, default=str) + "\n"
    )
    (output / "manifests/readiness.json").write_text(
        json.dumps(audit, indent=2, sort_keys=True) + "\n"
    )
    (output / "README.md").write_text(
        f"# Loss-aware selective training\n\nStatus: `{status}`\n\nGit: `{git['commit']}`\n\nModel: `{metrics['model_sha256']}`\n"
    )


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    run(args.config, args.output)
