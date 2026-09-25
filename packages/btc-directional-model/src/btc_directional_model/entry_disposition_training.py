"""Chronological out-of-fold directional and admission training for the tournament."""

from __future__ import annotations

import argparse
import json
from dataclasses import asdict
from datetime import UTC, datetime, timedelta
from pathlib import Path

import joblib
import numpy as np
import polars as pl
from sklearn.ensemble import HistGradientBoostingClassifier
from sklearn.linear_model import LogisticRegression

from .conservative_selective_training import (
    PRICE_FEATURES,
    VOLUME_FEATURES,
    Policy,
    expected_calibration_error,
    fit_model,
    replay,
    score,
    trade_metrics,
    weights,
)

BUY_FEATURES = (
    "direction_confidence", "ask_vwap_5", "seconds_elapsed_scaled",
    "btc_path_from_window_open_bps", "btc_return_5s_bps", "btc_return_30s_bps",
    "btc_return_60s_bps", "btc_realized_volatility_30s_bps", "btc_path_efficiency_30s",
    "btc_momentum_multihorizon_score", "btc_path_pullback_from_favorable_extreme_bps",
    "btc_path_recovery_from_adverse_extreme_bps", "btc_log_quote_volume_30s",
    "btc_taker_buy_share_30s",
)
SOURCE_COLUMNS = list(dict.fromkeys((
    "market_id", "window_start", "window_end", "official_outcome", "seconds_elapsed",
    *PRICE_FEATURES, *VOLUME_FEATURES, "up_ask_vwap_5", "down_ask_vwap_5",
)))
RAW = {
    "model": {"learning_rate": 0.04, "max_iter": 160, "max_bins": 127},
    "calibration": {"reliability_bins": 10},
}
SPEC = {"name": "direction-price-volume-l31", "include_aggregate_volume": True,
        "max_leaf_nodes": 31, "min_samples_leaf": 250, "l2_regularization": 6.0}
SEED = 20260924


def _boundary(value: str) -> datetime:
    return datetime.fromisoformat(value).astimezone(UTC)


def _base_scored(frame: pl.DataFrame) -> pl.DataFrame:
    return frame.with_columns(
        pl.when(pl.col("probability_up") >= 0.5).then(pl.lit("up")).otherwise(pl.lit("down")).alias("proposed_side"),
        pl.max_horizontal("probability_up", 1 - pl.col("probability_up")).alias("direction_confidence"),
        pl.when(pl.col("probability_up") >= 0.5).then(pl.col("up_ask_vwap_5"))
          .otherwise(pl.col("down_ask_vwap_5")).alias("ask_vwap_5"),
    ).with_columns((pl.col("proposed_side") == pl.col("official_outcome")).cast(pl.Int8).alias("won"))


def _admission_fit(fit: pl.DataFrame, calibration: pl.DataFrame) -> tuple:
    usable = fit.drop_nulls(["ask_vwap_5"])
    cal = calibration.drop_nulls(["ask_vwap_5"])
    if usable["won"].n_unique() < 2 or cal["won"].n_unique() < 2:
        raise RuntimeError("admission fit/calibration lacks both outcomes")
    model = HistGradientBoostingClassifier(
        learning_rate=0.05, max_iter=100, max_bins=127, max_leaf_nodes=15,
        min_samples_leaf=180, l2_regularization=8.0, early_stopping=False,
        random_state=SEED,
    )
    model.fit(usable.select(BUY_FEATURES).to_numpy(), usable["won"].to_numpy(), sample_weight=weights(usable))
    raw = np.clip(model.predict_proba(cal.select(BUY_FEATURES).to_numpy())[:, 1], 1e-6, 1 - 1e-6)
    calibrator = LogisticRegression(C=1, max_iter=300, random_state=SEED)
    calibrator.fit(np.log(raw / (1 - raw)).reshape(-1, 1), cal["won"].to_numpy(), sample_weight=weights(cal))
    p = calibrator.predict_proba(np.log(raw / (1 - raw)).reshape(-1, 1))[:, 1]
    ece = expected_calibration_error(cal["won"].to_numpy(), p, 10)
    return model, calibrator, float(ece)


def _admission_score(frame: pl.DataFrame, fitted: tuple) -> pl.DataFrame:
    model, calibrator, ece = fitted
    usable = _base_scored(frame)
    x = usable.select(BUY_FEATURES).to_numpy()
    raw = np.clip(model.predict_proba(x)[:, 1], 1e-6, 1 - 1e-6)
    p = calibrator.predict_proba(np.log(raw / (1 - raw)).reshape(-1, 1))[:, 1]
    conservative = np.maximum(0.5, p - max(ece, 0.02))
    adjusted_up = np.where(usable["proposed_side"].to_numpy() == "up", conservative, 1 - conservative)
    return usable.with_columns(
        pl.Series("admission_probability", p),
        pl.Series("conservative_probability_up", adjusted_up),
    )


def _select(scored: pl.DataFrame) -> tuple[Policy | None, list[dict]]:
    candidates = []
    for confidence in (0.70, 0.75, 0.80, 0.85):
        for edge in (0.03, 0.05, 0.08, 0.10):
            for cost in (0.55, 0.60, 0.65):
                policy = Policy(confidence, edge, cost)
                trades = replay(scored, policy, 0.005, 0.010)
                metrics = trade_metrics(trades)
                qualifies = (
                    metrics["trades"] >= 5 and metrics["precision"] is not None
                    and metrics["precision"] >= 0.70 and metrics["stress_pnl"] > 0
                    and metrics["compensation_ratio"] is not None
                    and metrics["compensation_ratio"] <= 2.0
                )
                candidates.append({"policy": asdict(policy), "qualifies": qualifies,
                                   "trades": metrics["trades"], "wilson": metrics["wilson_lower"],
                                   "stress_pnl": metrics["stress_pnl"]})
    candidates.sort(key=lambda row: (
        row["qualifies"], row["wilson"], row["stress_pnl"] / max(row["trades"], 1),
    ), reverse=True)
    return (Policy(**candidates[0]["policy"]) if candidates[0]["qualifies"] else None), candidates[:5]


def run(panel: Path, causal_dir: Path, output: Path) -> None:
    source = pl.read_parquet(panel, columns=SOURCE_COLUMNS).with_columns(
        pl.col(name).cast(pl.Float64).fill_nan(None) for name in (*PRICE_FEATURES, *VOLUME_FEATURES)
    )
    if source["market_id"].n_unique() < 50_000:
        raise RuntimeError("directional panel lacks expected full history")
    exact = (pl.scan_parquet(str(causal_dir / "date=*.parquet"), hive_partitioning=False)
             .select([name for name in SOURCE_COLUMNS if name != "window_end"])
             .with_columns((pl.col("window_start") + pl.duration(minutes=5)).alias("window_end"))
             .collect())
    start = _boundary("2026-05-04T00:00:00Z")
    final = _boundary("2026-09-24T00:00:00Z")
    exact_start = _boundary("2026-07-13T00:00:00Z")
    output.mkdir(parents=True, exist_ok=True)
    for name in ("models", "predictions", "metrics", "manifests"):
        (output / name).mkdir(exist_ok=True)
    prior_oof: list[pl.DataFrame] = []
    folds = []
    cursor = start
    fold = 0
    while cursor < final:
        end = min(cursor + timedelta(days=7), final)
        fit_end = cursor - timedelta(days=7)
        fit = source.filter((pl.col("window_start") < fit_end) & (pl.col("window_end") <= fit_end))
        calibration = source.filter((pl.col("window_start") >= fit_end) & (pl.col("window_start") < cursor))
        model = fit_model(fit, calibration, SPEC, RAW, SEED + fold)
        weekly = source.filter((pl.col("window_start") >= cursor) & (pl.col("window_start") < end))
        scored = score(weekly, model, 0.02)
        oof = _base_scored(scored.filter(pl.col("seconds_elapsed").is_between(60, 210)))
        prior_oof.append(oof)
        details = {"fold": fold, "test_start": cursor.isoformat(), "test_end": end.isoformat(),
                   "direction_fit_end": fit_end.isoformat(), "direction_calibration_ece": model[3],
                   "direction_fit_markets": fit["market_id"].n_unique(), "direction_calibration_markets": calibration["market_id"].n_unique()}
        if cursor >= exact_start:
            history = pl.concat(prior_oof[:-1], how="vertical")
            admission_fit_end = cursor - timedelta(days=14)
            admission_cal_end = cursor - timedelta(days=7)
            admit_fit = history.filter(pl.col("window_end") <= admission_fit_end)
            admit_cal = history.filter((pl.col("window_start") >= admission_fit_end) & (pl.col("window_start") < admission_cal_end))
            prior_selection = history.filter((pl.col("window_start") >= admission_cal_end) & (pl.col("window_start") < cursor))
            admission = _admission_fit(admit_fit, admit_cal)
            selection = _admission_score(prior_selection, admission)
            policy, ranking = _select(selection)
            test = exact.filter((pl.col("window_start") >= cursor) & (pl.col("window_start") < end))
            exact_scored = _admission_score(score(test, model, 0.02), admission).with_columns(pl.lit(fold).alias("fold"))
            exact_scored.write_parquet(output / "predictions" / f"buy-fold-{fold:02d}.parquet", compression="zstd")
            joblib.dump({"direction": model, "admission": admission, "policy": policy}, output / "models" / f"buy-fold-{fold:02d}.joblib")
            details.update({"admission_fit_end": admission_fit_end.isoformat(),
                            "admission_calibration_end": admission_cal_end.isoformat(),
                            "admission_fit_markets": admit_fit["market_id"].n_unique(),
                            "admission_calibration_ece": admission[2],
                            "selection_markets": prior_selection["market_id"].n_unique(),
                            "policy": asdict(policy) if policy else None,
                            "policy_search_top": ranking,
                            "exact_test_markets": test["market_id"].n_unique()})
        oof.select("market_id", "window_start", "official_outcome", "seconds_elapsed", "probability_up",
                   "up_ask_vwap_5", "down_ask_vwap_5").write_parquet(
            output / "predictions" / f"direction-fold-{fold:02d}.parquet", compression="zstd")
        folds.append(details)
        (output / "metrics" / "training-folds.json").write_text(json.dumps(folds, indent=2, default=str) + "\n")
        print(json.dumps(details, default=str), flush=True)
        cursor = end
        fold += 1


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--panel", type=Path, required=True)
    parser.add_argument("--causal-dir", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    run(args.panel, args.causal_dir, args.output)
