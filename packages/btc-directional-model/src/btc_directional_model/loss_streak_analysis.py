"""Analyze causal entry-time conditions shared by consecutive loss streaks."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any

import numpy as np
import polars as pl
from scipy.stats import mannwhitneyu

FEATURES = (
    "btc_realized_volatility_5s_bps", "btc_realized_volatility_30s_bps",
    "btc_realized_volatility_60s_bps", "btc_realized_volatility_120s_bps",
    "btc_volatility_shock_30_vs_120", "btc_volatility_shock_60_vs_180",
    "btc_range_30s_bps", "btc_range_60s_bps", "btc_path_efficiency_30s",
    "btc_path_efficiency_60s", "btc_range_position_30s", "btc_range_position_60s",
    "btc_path_from_window_open_bps", "btc_cross_venue_boundary_gap_bps",
    "btc_path_terminal_volatility_z", "btc_boundary_terminal_volatility_z",
    "btc_boundary_distance_velocity_5s_bps", "btc_boundary_momentum_alignment_5s",
    "btc_momentum_multihorizon_score", "btc_momentum_acceleration_5_vs_30",
    "btc_momentum_acceleration_15_vs_60", "btc_reversal_5_vs_30",
    "btc_path_cross_count", "btc_boundary_cross_count", "btc_seconds_since_path_cross",
    "btc_seconds_since_boundary_cross", "btc_path_pullback_from_favorable_extreme_bps",
    "btc_path_recovery_from_adverse_extreme_bps", "seconds_elapsed", "share_cost",
    "confidence", "stressed_edge",
)


def add_streak_ids(frame: pl.DataFrame) -> pl.DataFrame:
    ordered = frame.sort("window_start")
    ids: list[int | None] = []
    lengths: list[int] = []
    streak_id = 0
    start = 0
    won = ordered["won"].to_list()
    for index, value in enumerate(won + [True]):
        if not value and index < len(won):
            continue
        length = index - start
        if length:
            streak_id += 1
            ids.extend([streak_id] * length)
            lengths.extend([length] * length)
        if index < len(won):
            ids.append(None)
            lengths.append(0)
        start = index + 1
    return ordered.with_columns(
        pl.Series("loss_streak_id", ids, dtype=pl.Int64),
        pl.Series("loss_streak_length", lengths, dtype=pl.Int64),
    )


def _effect(feature: str, cohort: pl.DataFrame, reference: pl.DataFrame) -> dict[str, Any]:
    left = cohort[feature].drop_nulls().to_numpy()
    right = reference[feature].drop_nulls().to_numpy()
    if len(left) < 2 or len(right) < 2:
        return {"feature": feature, "cohort_n": len(left), "reference_n": len(right)}
    pooled = np.sqrt((np.var(left, ddof=1) + np.var(right, ddof=1)) / 2)
    standardized = float((np.mean(left) - np.mean(right)) / pooled) if pooled else 0.0
    _, p_value = mannwhitneyu(left, right, alternative="two-sided")
    return {
        "feature": feature, "cohort_n": len(left), "reference_n": len(right),
        "cohort_mean": float(np.mean(left)), "cohort_median": float(np.median(left)),
        "reference_mean": float(np.mean(right)), "reference_median": float(np.median(right)),
        "standardized_mean_difference": standardized, "mann_whitney_p": float(p_value),
    }


def analyze(input_path: Path, output: Path) -> dict[str, Any]:
    frame = add_streak_ids(pl.read_parquet(input_path))
    losses = frame.filter(~pl.col("won"))
    streak_losses = frame.filter(pl.col("loss_streak_length") >= 2)
    long_streak_losses = frame.filter(pl.col("loss_streak_length") >= 3)
    isolated_losses = frame.filter(pl.col("loss_streak_length") == 1)
    wins = frame.filter(pl.col("won"))
    quantiles = {
        feature: {
            "q25": float(frame[feature].quantile(0.25)),
            "q50": float(frame[feature].quantile(0.50)),
            "q75": float(frame[feature].quantile(0.75)),
        }
        for feature in FEATURES if frame[feature].null_count() < frame.height
    }
    streaks = []
    for (identifier,), part in streak_losses.group_by("loss_streak_id", maintain_order=True):
        start, end = part["window_start"].min(), part["window_start"].max()
        vol = part["btc_realized_volatility_60s_bps"]
        shock = part["btc_volatility_shock_30_vs_120"]
        streaks.append({
            "loss_streak_id": identifier, "length": part.height,
            "start": str(start), "end": str(end),
            "duration_hours": (end - start).total_seconds() / 3600,
            "sides": part.group_by("side").len().sort("side").to_dicts(),
            "folds": sorted(part["fold"].unique().to_list()),
            "mean_volatility_60s_bps": float(vol.mean()),
            "volatility_regime": (
                "calm" if float(vol.mean()) <= quantiles["btc_realized_volatility_60s_bps"]["q25"]
                else "volatile" if float(vol.mean()) >= quantiles["btc_realized_volatility_60s_bps"]["q75"]
                else "normal"
            ),
            "mean_volatility_shock_30_vs_120": float(shock.mean()),
            "mean_confidence": float(part["confidence"].mean()),
            "mean_share_cost": float(part["share_cost"].mean()),
            "mean_entry_second": float(part["seconds_elapsed"].mean()),
            "market_ids": part["market_id"].to_list(),
        })
    effects = {
        "streak_losses_vs_wins": sorted(
            (_effect(feature, streak_losses, wins) for feature in FEATURES),
            key=lambda row: abs(row.get("standardized_mean_difference", 0)), reverse=True,
        ),
        "streak_losses_vs_isolated_losses": sorted(
            (_effect(feature, streak_losses, isolated_losses) for feature in FEATURES),
            key=lambda row: abs(row.get("standardized_mean_difference", 0)), reverse=True,
        ),
        "long_streak_losses_vs_wins": sorted(
            (_effect(feature, long_streak_losses, wins) for feature in FEATURES),
            key=lambda row: abs(row.get("standardized_mean_difference", 0)), reverse=True,
        ),
    }
    counts = losses.group_by("loss_streak_length").agg(
        pl.col("loss_streak_id").n_unique().alias("streaks"), pl.len().alias("losses")
    ).sort("loss_streak_length")
    result = {
        "source": str(input_path), "trades": frame.height, "wins": wins.height,
        "losses": losses.height, "streak_loss_rows": streak_losses.height,
        "long_streak_loss_rows": long_streak_losses.height,
        "streak_distribution": counts.to_dicts(), "streaks": streaks,
        "quantiles": quantiles, "effects": effects,
        "interpretation_rule": "Effect sizes describe association at entry, not causal proof. Any derived veto must be trained out-of-fold and validated on different chronological periods.",
    }
    output.mkdir(parents=True, exist_ok=False)
    frame.write_parquet(output / "trade-ledger-with-streaks.parquet")
    streak_losses.write_parquet(output / "streak-losses.parquet")
    (output / "analysis.json").write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--input", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    analyze(args.input, args.output)
