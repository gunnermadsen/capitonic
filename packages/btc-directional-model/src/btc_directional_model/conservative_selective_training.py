"""Nested walk-forward training for one conservative BTC directional model."""

from __future__ import annotations

import argparse
import hashlib
import itertools
import json
import math
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
from sklearn.impute import SimpleImputer
from sklearn.linear_model import LogisticRegression
from sklearn.metrics import brier_score_loss, log_loss
from sklearn.pipeline import make_pipeline
from sklearn.preprocessing import StandardScaler

SCHEMA_VERSION = "btc-conservative-selective-training-v2"
PRICE_FEATURES = (
    "seconds_elapsed_scaled",
    "seconds_remaining_scaled",
    "btc_cross_venue_boundary_gap_bps",
    "btc_path_from_window_open_bps",
    "btc_return_1s_bps",
    "btc_return_5s_bps",
    "btc_return_15s_bps",
    "btc_return_30s_bps",
    "btc_return_60s_bps",
    "btc_return_90s_bps",
    "btc_return_120s_bps",
    "btc_return_180s_bps",
    "btc_realized_volatility_5s_bps",
    "btc_realized_volatility_15s_bps",
    "btc_realized_volatility_30s_bps",
    "btc_realized_volatility_60s_bps",
    "btc_realized_volatility_90s_bps",
    "btc_realized_volatility_120s_bps",
    "btc_realized_volatility_180s_bps",
    "btc_range_5s_bps",
    "btc_range_15s_bps",
    "btc_range_30s_bps",
    "btc_range_60s_bps",
    "btc_path_efficiency_30s",
    "btc_path_efficiency_60s",
    "btc_range_position_30s",
    "btc_range_position_60s",
    "btc_path_cross_count",
    "btc_boundary_cross_count",
    "btc_seconds_since_path_cross",
    "btc_seconds_since_boundary_cross",
    "btc_path_terminal_volatility_z",
    "btc_boundary_terminal_volatility_z",
    "btc_boundary_distance_velocity_5s_bps",
    "btc_boundary_momentum_alignment_5s",
    "btc_momentum_multihorizon_score",
    "btc_momentum_acceleration_5_vs_30",
    "btc_momentum_acceleration_15_vs_60",
    "btc_reversal_5_vs_30",
    "btc_path_max_favorable_excursion_bps",
    "btc_path_max_adverse_excursion_bps",
    "btc_path_pullback_from_favorable_extreme_bps",
    "btc_path_recovery_from_adverse_extreme_bps",
    "btc_seconds_since_path_high_scaled",
    "btc_seconds_since_path_low_scaled",
    "btc_volatility_shock_30_vs_120",
    "btc_volatility_shock_60_vs_180",
    "hour_sin",
    "hour_cos",
    "weekday_sin",
    "weekday_cos",
)
VOLUME_FEATURES = (
    "btc_log_quote_volume_5s",
    "btc_log_trade_count_5s",
    "btc_taker_buy_share_5s",
    "btc_log_quote_volume_30s",
    "btc_log_trade_count_30s",
    "btc_taker_buy_share_30s",
    "btc_log_quote_volume_60s",
    "btc_log_trade_count_60s",
    "btc_taker_buy_share_60s",
)
KEYS = (
    "market_id",
    "window_start",
    "official_outcome",
    "seconds_elapsed",
    "up_ask_vwap_5",
    "down_ask_vwap_5",
)


@dataclass(frozen=True)
class Policy:
    confidence_floor: float
    minimum_stressed_edge: float
    maximum_share_cost: float


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def parse(value: str) -> datetime:
    return datetime.fromisoformat(value)


def wilson_lower(wins: int, total: int, z: float = 1.96) -> float:
    if total == 0:
        return 0.0
    p = wins / total
    denominator = 1 + z * z / total
    center = p + z * z / (2 * total)
    spread = z * math.sqrt((p * (1 - p) + z * z / (4 * total)) / total)
    return (center - spread) / denominator


def expected_calibration_error(labels: np.ndarray, probabilities: np.ndarray, bins: int) -> float:
    edges = np.linspace(0, 1, bins + 1)
    result = 0.0
    for low, high in itertools.pairwise(edges):
        mask = (probabilities >= low) & (probabilities < high if high < 1 else probabilities <= 1)
        if mask.any():
            result += mask.mean() * abs(probabilities[mask].mean() - labels[mask].mean())
    return float(result)


def trade_metrics(rows: pl.DataFrame) -> dict[str, Any]:
    if rows.is_empty():
        return {
            "trades": 0,
            "wins": 0,
            "losses": 0,
            "precision": None,
            "wilson_lower": 0.0,
            "net_pnl": 0.0,
            "stress_pnl": 0.0,
            "profit_factor": None,
            "stressed_profit_factor": None,
            "compensation_ratio": None,
            "maximum_drawdown": 0.0,
            "longest_losing_streak": 0,
            "weeks": 0,
            "positive_month_fraction": None,
            "best_day_positive_pnl_share": None,
            "remove_best_day_stress_pnl": 0.0,
        }
    pnl = rows["net_pnl"].to_numpy()
    stress = rows["stress_pnl"].to_numpy()
    wins = int((pnl > 0).sum())
    losses = len(pnl) - wins
    positive, negative = pnl[pnl > 0], pnl[pnl <= 0]
    stress_positive, stress_negative = stress[pnl > 0], stress[pnl <= 0]
    mean_win = float(stress_positive.mean()) if len(stress_positive) else None
    mean_loss = float(-stress_negative.mean()) if len(stress_negative) else None
    compensation = mean_loss / mean_win if mean_loss is not None and mean_win else None
    gross_win, gross_loss = float(positive.sum()), float(-negative.sum())
    stress_gross_win, stress_gross_loss = (
        float(stress_positive.sum()),
        float(-stress_negative.sum()),
    )
    cumulative = np.cumsum(pnl)
    drawdown = np.maximum.accumulate(np.r_[0.0, cumulative])[-len(cumulative) :] - cumulative
    longest = current = 0
    for value in pnl:
        current = current + 1 if value <= 0 else 0
        longest = max(longest, current)
    daily = (
        rows.with_columns(pl.col("window_start").dt.date().alias("day"))
        .group_by("day")
        .agg(pl.col("stress_pnl").sum())
        .sort("day")
    )
    monthly = (
        rows.with_columns(pl.col("window_start").dt.truncate("1mo").alias("month"))
        .group_by("month")
        .agg(pl.col("stress_pnl").sum())
    )
    best_day = float(daily["stress_pnl"].max())
    positive_daily = float(daily.filter(pl.col("stress_pnl") > 0)["stress_pnl"].sum())
    return {
        "trades": len(pnl),
        "wins": wins,
        "losses": losses,
        "precision": wins / len(pnl),
        "wilson_lower": wilson_lower(wins, len(pnl)),
        "net_pnl": float(pnl.sum()),
        "stress_pnl": float(stress.sum()),
        "profit_factor": gross_win / gross_loss if gross_loss else None,
        "stressed_profit_factor": (
            stress_gross_win / stress_gross_loss if stress_gross_loss else None
        ),
        "mean_win": mean_win,
        "mean_loss_abs": mean_loss,
        "compensation_ratio": compensation,
        "maximum_drawdown": float(drawdown.max()),
        "longest_losing_streak": longest,
        "weeks": rows.with_columns(pl.col("window_start").dt.truncate("1w").alias("week"))[
            "week"
        ].n_unique(),
        "positive_month_fraction": float((monthly["stress_pnl"] > 0).mean()),
        "best_day_positive_pnl_share": best_day / positive_daily if positive_daily > 0 else None,
        "remove_best_day_stress_pnl": float(stress.sum() - best_day),
        "average_entry_second": float(rows["seconds_elapsed"].mean()),
        "average_entry_price": float(rows["share_cost"].mean()),
    }


def replay(
    frame: pl.DataFrame,
    policy: Policy,
    reserve: float,
    stress: float,
    quantity: int = 5,
) -> pl.DataFrame:
    up_vwap = f"up_ask_vwap_{quantity}"
    down_vwap = f"down_ask_vwap_{quantity}"
    if up_vwap not in frame.columns or down_vwap not in frame.columns:
        raise ValueError(f"missing VWAP columns for quantity {quantity}")
    side_up = pl.col("probability_up") >= 0.5
    eligible = (
        frame.with_columns(
            pl.when(side_up).then(pl.lit("up")).otherwise(pl.lit("down")).alias("side"),
            pl.when(side_up)
            .then(pl.col("conservative_probability_up"))
            .otherwise(1 - pl.col("conservative_probability_up"))
            .alias("confidence"),
            pl.when(side_up).then(pl.col(up_vwap)).otherwise(pl.col(down_vwap)).alias("share_cost"),
        )
        .with_columns(
            (pl.col("confidence") - pl.col("share_cost") - reserve - stress).alias("stressed_edge")
        )
        .filter(
            (pl.col("confidence") >= policy.confidence_floor)
            & (pl.col("stressed_edge") >= policy.minimum_stressed_edge)
            & (pl.col("share_cost") <= policy.maximum_share_cost)
            & pl.col("share_cost").is_not_null()
        )
        .sort(["market_id", "seconds_elapsed"])
        .group_by("market_id", maintain_order=True)
        .first()
        .sort("window_start")
    )
    won = pl.col("side") == pl.col("official_outcome")
    return eligible.with_columns(
        won.alias("won"),
        pl.when(won)
        .then(quantity * (1 - pl.col("share_cost") - reserve))
        .otherwise(-quantity * (pl.col("share_cost") + reserve))
        .alias("net_pnl"),
        pl.when(won)
        .then(quantity * (1 - pl.col("share_cost") - reserve - stress))
        .otherwise(-quantity * (pl.col("share_cost") + reserve + stress))
        .alias("stress_pnl"),
    )


def model_features(spec: dict[str, Any]) -> tuple[str, ...]:
    return (
        (*PRICE_FEATURES, *VOLUME_FEATURES) if spec["include_aggregate_volume"] else PRICE_FEATURES
    )


def weights(frame: pl.DataFrame) -> np.ndarray:
    return frame.select((1 / pl.len().over("market_id")).alias("weight"))["weight"].to_numpy()


def fit_model(
    fit: pl.DataFrame,
    calibration: pl.DataFrame,
    spec: dict[str, Any],
    raw: dict[str, Any],
    seed: int,
) -> tuple[Any, Any, tuple[str, ...], float]:
    features = model_features(spec)
    common = raw["model"]
    estimator = HistGradientBoostingClassifier(
        learning_rate=common["learning_rate"],
        max_iter=common["max_iter"],
        max_bins=common["max_bins"],
        max_leaf_nodes=spec["max_leaf_nodes"],
        min_samples_leaf=spec["min_samples_leaf"],
        l2_regularization=spec["l2_regularization"],
        early_stopping=False,
        random_state=seed,
    )
    labels = (fit["official_outcome"] == "up").cast(pl.Int8).to_numpy()
    estimator.fit(fit.select(features).to_numpy(), labels, sample_weight=weights(fit))
    raw_p = np.clip(
        estimator.predict_proba(calibration.select(features).to_numpy())[:, 1], 1e-6, 1 - 1e-6
    )
    logits = np.log(raw_p / (1 - raw_p)).reshape(-1, 1)
    calibrator = LogisticRegression(C=1.0, max_iter=500, random_state=seed)
    cal_labels = (calibration["official_outcome"] == "up").cast(pl.Int8).to_numpy()
    calibrator.fit(logits, cal_labels, sample_weight=weights(calibration))
    calibrated = calibrator.predict_proba(logits)[:, 1]
    ece = expected_calibration_error(cal_labels, calibrated, raw["calibration"]["reliability_bins"])
    return estimator, calibrator, features, ece


def fit_logistic_control(
    fit: pl.DataFrame,
    calibration: pl.DataFrame,
    features: tuple[str, ...],
    raw: dict[str, Any],
    seed: int,
) -> tuple[Any, Any, tuple[str, ...], float]:
    estimator = make_pipeline(
        SimpleImputer(strategy="median"),
        StandardScaler(),
        LogisticRegression(C=0.1, max_iter=300, random_state=seed),
    )
    labels = (fit["official_outcome"] == "up").cast(pl.Int8).to_numpy()
    estimator.fit(
        fit.select(features).to_numpy(),
        labels,
        logisticregression__sample_weight=weights(fit),
    )
    raw_p = np.clip(
        estimator.predict_proba(calibration.select(features).to_numpy())[:, 1],
        1e-6,
        1 - 1e-6,
    )
    logits = np.log(raw_p / (1 - raw_p)).reshape(-1, 1)
    calibrator = LogisticRegression(C=1.0, max_iter=500, random_state=seed)
    cal_labels = (calibration["official_outcome"] == "up").cast(pl.Int8).to_numpy()
    calibrator.fit(logits, cal_labels, sample_weight=weights(calibration))
    calibrated = calibrator.predict_proba(logits)[:, 1]
    ece = expected_calibration_error(cal_labels, calibrated, raw["calibration"]["reliability_bins"])
    return estimator, calibrator, features, ece


def score(
    frame: pl.DataFrame, model: tuple[Any, Any, tuple[str, ...], float], safety: float
) -> pl.DataFrame:
    estimator, calibrator, features, ece = model
    raw_p = np.clip(
        estimator.predict_proba(frame.select(features).to_numpy())[:, 1], 1e-6, 1 - 1e-6
    )
    logits = np.log(raw_p / (1 - raw_p)).reshape(-1, 1)
    probability = calibrator.predict_proba(logits)[:, 1]
    penalty = max(ece, safety)
    conservative = np.where(
        probability >= 0.5,
        np.maximum(0.5, probability - penalty),
        np.minimum(0.5, probability + penalty),
    )
    return frame.with_columns(
        pl.Series("probability_up", probability),
        pl.Series("conservative_probability_up", conservative),
    )


def select_policy(
    frame: pl.DataFrame,
    raw: dict[str, Any],
    quantity: int = 5,
) -> tuple[Policy | None, list[dict[str, Any]]]:
    p = raw["policy"]
    candidates = []
    for values in itertools.product(
        p["confidence_floors"], p["minimum_stressed_edges"], p["maximum_share_costs"]
    ):
        policy = Policy(*values)
        metrics = trade_metrics(
            replay(
                frame,
                policy,
                p["execution_reserve_per_share"],
                p["stress_slippage_per_share"],
                quantity,
            )
        )
        ratio = metrics["compensation_ratio"]
        qualifies = (
            metrics["trades"] >= p["minimum_selection_trades"]
            and metrics["precision"] >= p["minimum_selection_precision"]
            and metrics["stress_pnl"] > 0
            and ratio is not None
            and ratio <= p["maximum_compensation_ratio"]
        )
        candidates.append({"policy": asdict(policy), "qualifies": qualifies, **metrics})
    candidates.sort(
        key=lambda x: (
            x["qualifies"],
            x["wilson_lower"],
            x["stress_pnl"] / max(x["trades"], 1),
            -x["trades"],
        ),
        reverse=True,
    )
    return (
        Policy(**candidates[0]["policy"]) if candidates and candidates[0]["qualifies"] else None
    ), candidates


def probability_metrics(frame: pl.DataFrame, raw: dict[str, Any]) -> dict[str, float]:
    labels = (frame["official_outcome"] == "up").cast(pl.Int8).to_numpy()
    probabilities = frame["probability_up"].to_numpy()
    return {
        "brier": float(brier_score_loss(labels, probabilities)),
        "log_loss": float(log_loss(labels, probabilities)),
        "ece": expected_calibration_error(
            labels, probabilities, raw["calibration"]["reliability_bins"]
        ),
    }


def readiness(frame: pl.DataFrame, raw: dict[str, Any], panel: Path) -> dict[str, Any]:
    source = raw["source"]
    counts = frame.group_by("market_id").len()
    outcome_conflicts = (
        frame.group_by("market_id")
        .agg(pl.col("official_outcome").n_unique().alias("n"))
        .filter(pl.col("n") != 1)
        .height
    )
    payload = {
        "panel": str(panel),
        "sha256": sha256(panel),
        "rows": frame.height,
        "markets": frame["market_id"].n_unique(),
        "range_start": str(frame["window_start"].min()),
        "range_end": str(frame["window_start"].max()),
        "outcome_conflicts": outcome_conflicts,
        "checkpoint_count_min": int(counts["len"].min()),
        "checkpoint_count_max": int(counts["len"].max()),
        "execution_markets": frame.drop_nulls(["up_ask_vwap_5", "down_ask_vwap_5"])[
            "market_id"
        ].n_unique(),
        "price_features": list(PRICE_FEATURES),
        "volume_ablation_features": list(VOLUME_FEATURES),
        "forbidden_sources": [
            "rtds",
            "chainlink_refprice",
            "chainlink_candles",
            "kraken",
            "trade_prints",
            "l2",
            "open_interest",
            "polymarket_prices_as_features",
        ],
    }
    if (
        payload["sha256"] != source["panel_sha256"]
        or payload["rows"] != source["expected_rows"]
        or payload["markets"] != source["expected_markets"]
        or outcome_conflicts
        or payload["checkpoint_count_min"] != source["expected_checkpoints_per_market"]
        or payload["checkpoint_count_max"] != source["expected_checkpoints_per_market"]
    ):
        raise RuntimeError(f"source readiness failed: {payload}")
    return payload


def qualification(metrics: dict[str, Any], raw: dict[str, Any]) -> dict[str, Any]:
    q = raw["qualification"]
    t = metrics["trades"]
    checks = {
        "minimum_historical_trades": t["trades"] >= q["minimum_historical_trades"],
        "minimum_historical_weeks": t["weeks"] >= q["minimum_historical_weeks"],
        "minimum_precision": t["precision"] is not None
        and t["precision"] >= q["minimum_precision"],
        "minimum_historical_wilson": t["wilson_lower"] >= q["minimum_historical_wilson"],
        "maximum_ece": metrics["probability"]["ece"] <= q["maximum_ece"],
        "positive_stressed_pnl": t["stress_pnl"] > 0,
        "minimum_stressed_profit_factor": t["stressed_profit_factor"] is not None
        and t["stressed_profit_factor"] >= q["minimum_stressed_profit_factor"],
        "maximum_compensation_ratio": t["compensation_ratio"] is not None
        and t["compensation_ratio"] <= q["maximum_compensation_ratio"],
        "maximum_losing_streak": t["longest_losing_streak"] <= q["maximum_losing_streak"],
        "remove_best_day_positive": t["remove_best_day_stress_pnl"] > 0,
        "best_day_concentration": t["best_day_positive_pnl_share"] is not None
        and t["best_day_positive_pnl_share"] <= q["maximum_best_day_positive_pnl_share"],
        "positive_month_fraction": t["positive_month_fraction"] is not None
        and t["positive_month_fraction"] >= q["minimum_positive_month_fraction"],
    }
    return {"passed": all(checks.values()), "checks": checks}


def run(config: Path, output: Path) -> None:
    raw = tomllib.loads(config.read_text())
    if (
        not raw["training"]["training_only"]
        or not raw["training"]["paper_only"]
        or raw["training"]["live_capital_allowed"]
    ):
        raise RuntimeError("run must remain offline, training-only and paper-only")
    panel = Path(raw["source"]["panel"])
    columns = list(dict.fromkeys((*KEYS, *PRICE_FEATURES, *VOLUME_FEATURES)))
    frame = pl.read_parquet(panel, columns=columns).with_columns(
        [pl.col(c).cast(pl.Float64).fill_nan(None) for c in (*PRICE_FEATURES, *VOLUME_FEATURES)]
    )
    audit = readiness(frame, raw, panel)
    output.mkdir(parents=True, exist_ok=False)
    (output / "manifests").mkdir()
    (output / "predictions").mkdir()
    (output / "trades").mkdir()
    (output / "models").mkdir()
    (output / "metrics").mkdir()
    (output / "manifests" / "readiness.json").write_text(
        json.dumps(audit, indent=2, sort_keys=True) + "\n"
    )
    source_start, sealed_start, source_end = (
        parse(raw["source"][k]) for k in ("source_start", "sealed_start", "source_end")
    )
    seed = raw["training"]["random_seed"]
    calibration_days, selection_days, test_days = (
        raw["training"][k] for k in ("calibration_days", "selection_days", "test_days")
    )
    first_test = source_start + timedelta(
        days=raw["training"]["minimum_fit_days"] + calibration_days + selection_days
    )
    starts = []
    cursor = first_test
    while cursor + timedelta(days=test_days) <= sealed_start:
        starts.append(cursor)
        cursor += timedelta(days=test_days)
    summaries = []
    for spec in raw["models"]:
        fold_predictions, fold_trades, fold_details = [], [], []
        for index, test_start in enumerate(starts):
            fit_end = test_start - timedelta(days=calibration_days + selection_days)
            calibration_end = test_start - timedelta(days=selection_days)
            fit = frame.filter(
                (pl.col("window_start") >= source_start) & (pl.col("window_start") < fit_end)
            )
            cal = frame.filter(
                (pl.col("window_start") >= fit_end) & (pl.col("window_start") < calibration_end)
            )
            selection = frame.filter(
                (pl.col("window_start") >= calibration_end) & (pl.col("window_start") < test_start)
            )
            test = frame.filter(
                (pl.col("window_start") >= test_start)
                & (pl.col("window_start") < test_start + timedelta(days=test_days))
            )
            model = fit_model(fit, cal, spec, raw, seed)
            scored_selection = score(selection, model, raw["calibration"]["safety_floor"])
            policy, policy_search = select_policy(scored_selection, raw)
            scored_test = score(test, model, raw["calibration"]["safety_floor"]).with_columns(
                pl.lit(spec["name"]).alias("model_name"), pl.lit(index).alias("fold")
            )
            fold_predictions.append(scored_test)
            trades = (
                replay(
                    scored_test,
                    policy,
                    raw["policy"]["execution_reserve_per_share"],
                    raw["policy"]["stress_slippage_per_share"],
                )
                if policy
                else pl.DataFrame()
            )
            if not trades.is_empty():
                fold_trades.append(
                    trades.with_columns(
                        pl.lit(spec["name"]).alias("model_name"), pl.lit(index).alias("fold")
                    )
                )
            fold_details.append(
                {
                    "fold": index,
                    "fit_end": fit_end.isoformat(),
                    "calibration_end": calibration_end.isoformat(),
                    "test_start": test_start.isoformat(),
                    "test_end": (test_start + timedelta(days=test_days)).isoformat(),
                    "policy": asdict(policy) if policy else None,
                    "calibration_ece": model[3],
                    "policy_search_top": policy_search[:5],
                }
            )
        predictions = pl.concat(fold_predictions)
        trades = pl.concat(fold_trades) if fold_trades else pl.DataFrame()
        predictions.write_parquet(output / "predictions" / f"{spec['name']}.parquet")
        if not trades.is_empty():
            trades.write_parquet(output / "trades" / f"{spec['name']}.parquet")
        result = {
            "name": spec["name"],
            "spec": spec,
            "probability": probability_metrics(predictions, raw),
            "trades": trade_metrics(trades),
            "folds": fold_details,
        }
        result["qualification"] = qualification(result, raw)
        summaries.append(result)
    summaries.sort(
        key=lambda x: (
            x["qualification"]["passed"],
            x["trades"]["wilson_lower"],
            x["trades"]["stress_pnl"] / max(x["trades"]["trades"], 1),
        ),
        reverse=True,
    )
    winner = summaries[0]
    spec = winner["spec"]
    fit_end = sealed_start - timedelta(days=calibration_days + selection_days)
    calibration_end = sealed_start - timedelta(days=selection_days)
    fit = frame.filter(
        (pl.col("window_start") >= source_start) & (pl.col("window_start") < fit_end)
    )
    cal = frame.filter(
        (pl.col("window_start") >= fit_end) & (pl.col("window_start") < calibration_end)
    )
    selection = frame.filter(
        (pl.col("window_start") >= calibration_end) & (pl.col("window_start") < sealed_start)
    )
    sealed = frame.filter(
        (pl.col("window_start") >= sealed_start) & (pl.col("window_start") < source_end)
    )
    final_model = fit_model(fit, cal, spec, raw, seed)
    final_policy, final_search = select_policy(
        score(selection, final_model, raw["calibration"]["safety_floor"]), raw
    )
    sealed_scored = score(sealed, final_model, raw["calibration"]["safety_floor"])
    sealed_scored.write_parquet(output / "predictions" / "sealed.parquet")
    sealed_trades = (
        replay(
            sealed_scored,
            final_policy,
            raw["policy"]["execution_reserve_per_share"],
            raw["policy"]["stress_slippage_per_share"],
        )
        if final_policy
        else pl.DataFrame()
    )
    if not sealed_trades.is_empty():
        sealed_trades.write_parquet(output / "trades" / "sealed.parquet")
    sealed_result = {
        "policy": asdict(final_policy) if final_policy else None,
        "probability": probability_metrics(sealed_scored, raw),
        "trades": trade_metrics(sealed_trades),
        "policy_search_top": final_search[:10],
    }
    sealed_result["qualification"] = qualification(sealed_result, raw)
    controls = {}
    for control_name, control_features in (
        ("persistence", ("btc_cross_venue_boundary_gap_bps",)),
        ("regularized_logistic", PRICE_FEATURES),
    ):
        control_model = fit_logistic_control(fit, cal, control_features, raw, seed)
        control_policy, _ = select_policy(
            score(selection, control_model, raw["calibration"]["safety_floor"]), raw
        )
        control_scored = score(sealed, control_model, raw["calibration"]["safety_floor"])
        control_trades = (
            replay(
                control_scored,
                control_policy,
                raw["policy"]["execution_reserve_per_share"],
                raw["policy"]["stress_slippage_per_share"],
            )
            if control_policy
            else pl.DataFrame()
        )
        controls[control_name] = {
            "features": list(control_features),
            "policy": asdict(control_policy) if control_policy else None,
            "probability": probability_metrics(control_scored, raw),
            "trades": trade_metrics(control_trades),
        }
    control_passed = all(
        sealed_result["probability"][metric] < controls[name]["probability"][metric]
        for name in controls
        for metric in ("brier", "log_loss")
    )
    stability = []
    base_decisions = (
        {
            row["market_id"]: row["side"]
            for row in sealed_trades.select("market_id", "side").to_dicts()
        }
        if not sealed_trades.is_empty()
        else {}
    )
    for stability_seed in raw["training"]["stability_seeds"][1:]:
        alternate = fit_model(fit, cal, spec, raw, stability_seed)
        alternate_scored = score(sealed, alternate, raw["calibration"]["safety_floor"])
        alternate_trades = (
            replay(
                alternate_scored,
                final_policy,
                raw["policy"]["execution_reserve_per_share"],
                raw["policy"]["stress_slippage_per_share"],
            )
            if final_policy
            else pl.DataFrame()
        )
        alternate_decisions = (
            {
                row["market_id"]: row["side"]
                for row in alternate_trades.select("market_id", "side").to_dicts()
            }
            if not alternate_trades.is_empty()
            else {}
        )
        union = set(base_decisions) | set(alternate_decisions)
        common = set(base_decisions) & set(alternate_decisions)
        stability.append(
            {
                "seed": stability_seed,
                "admission_jaccard": len(common) / len(union) if union else 1.0,
                "direction_agreement": (
                    sum(base_decisions[key] == alternate_decisions[key] for key in common)
                    / len(common)
                    if common
                    else 1.0
                ),
                "trades": len(alternate_decisions),
            }
        )
    stability_passed = final_policy is not None and all(
        row["admission_jaccard"] >= 0.80 and row["direction_agreement"] >= 0.90 for row in stability
    )
    bundle = {
        "schema_version": SCHEMA_VERSION,
        "spec": spec,
        "features": final_model[2],
        "estimator": final_model[0],
        "calibrator": final_model[1],
        "calibration_ece": final_model[3],
        "policy": final_policy,
        "source_sha256": audit["sha256"],
    }
    git = {
        "branch": subprocess.check_output(["git", "branch", "--show-current"], text=True).strip(),
        "commit": subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip(),
        "dirty": bool(subprocess.check_output(["git", "status", "--porcelain"], text=True).strip()),
    }
    status = (
        "provisional_historical_pass"
        if winner["qualification"]["passed"]
        and sealed_result["qualification"]["passed"]
        and stability_passed
        and control_passed
        else "historical_failed"
    )
    model_path = (
        output
        / "models"
        / ("provisional.joblib" if status == "provisional_historical_pass" else "diagnostic.joblib")
    )
    joblib.dump(bundle, model_path, compress=3)
    metrics = {
        "schema_version": SCHEMA_VERSION,
        "status": status,
        "readiness": audit,
        "configuration_results": summaries,
        "selected": winner["name"],
        "sealed": sealed_result,
        "controls": {"passed": control_passed, "results": controls},
        "seed_stability": {"passed": stability_passed, "comparisons": stability},
        "git": git,
        "runtime": {
            "python": platform.python_version(),
            "numpy": np.__version__,
            "polars": pl.__version__,
            "sklearn": sklearn.__version__,
        },
        "config_sha256": sha256(config),
        "model_artifact": model_path.name,
        "model_sha256": sha256(model_path),
    }
    (output / "metrics" / "results.json").write_text(
        json.dumps(metrics, indent=2, sort_keys=True, default=str) + "\n"
    )
    (output / "README.md").write_text(
        f"# Conservative selective preliminary training\n\nStatus: `{metrics['status']}`\n\nGit: `{git['commit']}`\n\nSource: `{audit['sha256']}`\n\nModel: `{metrics['model_sha256']}`\n\nThis run is preliminary retrospective evidence. It does not authorize paper or live deployment.\n"
    )


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    run(args.config, args.output)
