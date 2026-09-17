"""Train one sparse, time-conditioned BTC directional model."""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import platform
import subprocess
import tomllib
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path
from typing import Any

import joblib
import numpy as np
import polars as pl
import sklearn
from sklearn.ensemble import HistGradientBoostingClassifier
from sklearn.linear_model import LogisticRegression
from sklearn.metrics import brier_score_loss, log_loss

SCHEMA_VERSION = "btc-conservative-selective-training-v1"

FEATURES = (
    "seconds_elapsed_scaled", "seconds_remaining_scaled",
    "btc_cross_venue_boundary_gap_bps", "btc_path_from_window_open_bps",
    "btc_return_1s_bps", "btc_return_5s_bps", "btc_return_15s_bps",
    "btc_return_30s_bps", "btc_return_60s_bps", "btc_return_90s_bps",
    "btc_return_120s_bps", "btc_return_180s_bps",
    "btc_realized_volatility_5s_bps", "btc_realized_volatility_15s_bps",
    "btc_realized_volatility_30s_bps", "btc_realized_volatility_60s_bps",
    "btc_realized_volatility_90s_bps", "btc_realized_volatility_120s_bps",
    "btc_realized_volatility_180s_bps", "btc_range_5s_bps", "btc_range_15s_bps",
    "btc_range_30s_bps", "btc_range_60s_bps", "btc_path_efficiency_30s",
    "btc_path_efficiency_60s", "btc_range_position_30s", "btc_range_position_60s",
    "btc_path_cross_count", "btc_boundary_cross_count", "btc_seconds_since_path_cross",
    "btc_seconds_since_boundary_cross", "btc_path_terminal_volatility_z",
    "btc_boundary_terminal_volatility_z", "btc_boundary_distance_velocity_5s_bps",
    "btc_boundary_momentum_alignment_5s", "btc_momentum_multihorizon_score",
    "btc_momentum_acceleration_5_vs_30", "btc_momentum_acceleration_15_vs_60",
    "btc_reversal_5_vs_30", "btc_path_max_favorable_excursion_bps",
    "btc_path_max_adverse_excursion_bps", "btc_path_pullback_from_favorable_extreme_bps",
    "btc_path_recovery_from_adverse_extreme_bps", "btc_seconds_since_path_high_scaled",
    "btc_seconds_since_path_low_scaled", "btc_volatility_shock_30_vs_120",
    "btc_volatility_shock_60_vs_180", "hour_sin", "hour_cos", "weekday_sin", "weekday_cos",
)


@dataclass(frozen=True)
class Policy:
    confidence: float
    minimum_edge: float
    maximum_share_cost: float


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def wilson_lower(wins: int, total: int, z: float = 1.96) -> float:
    if total == 0:
        return 0.0
    p = wins / total
    denominator = 1 + z * z / total
    center = p + z * z / (2 * total)
    spread = z * math.sqrt((p * (1 - p) + z * z / (4 * total)) / total)
    return (center - spread) / denominator


def trade_metrics(rows: pl.DataFrame) -> dict[str, Any]:
    if rows.is_empty():
        return {"trades": 0, "wins": 0, "losses": 0, "precision": None,
                "precision_wilson_lower": 0.0, "net_pnl": 0.0, "stress_pnl": 0.0,
                "profit_factor": None, "mean_win": None, "mean_loss_abs": None,
                "compensation_ratio": None, "maximum_drawdown": 0.0,
                "longest_losing_streak": 0}
    pnl = rows["net_pnl"].to_numpy()
    stress = rows["stress_pnl"].to_numpy()
    wins = int((pnl > 0).sum())
    losses = int((pnl <= 0).sum())
    positive = pnl[pnl > 0]
    negative = pnl[pnl <= 0]
    mean_win = float(positive.mean()) if len(positive) else None
    mean_loss = float(-negative.mean()) if len(negative) else None
    compensation = mean_loss / mean_win if mean_loss is not None and mean_win else None
    gross_win = float(positive.sum())
    gross_loss = float(-negative.sum())
    cumulative = np.cumsum(pnl)
    drawdown = np.maximum.accumulate(np.r_[0.0, cumulative])[-len(cumulative):] - cumulative
    longest = current = 0
    for value in pnl:
        current = current + 1 if value <= 0 else 0
        longest = max(longest, current)
    return {
        "trades": len(pnl), "wins": wins, "losses": losses,
        "precision": wins / len(pnl), "precision_wilson_lower": wilson_lower(wins, len(pnl)),
        "net_pnl": float(pnl.sum()), "stress_pnl": float(stress.sum()),
        "profit_factor": gross_win / gross_loss if gross_loss else None,
        "mean_win": mean_win, "mean_loss_abs": mean_loss,
        "compensation_ratio": compensation, "maximum_drawdown": float(drawdown.max()),
        "longest_losing_streak": longest,
        "average_entry_second": float(rows["seconds_elapsed"].mean()),
        "average_entry_price": float(rows["share_cost"].mean()),
    }


def replay(frame: pl.DataFrame, policy: Policy, reserve: float, stress: float) -> pl.DataFrame:
    eligible = (
        frame.with_columns(
            pl.when(pl.col("probability_up") >= 0.5).then(pl.lit("up")).otherwise(pl.lit("down")).alias("side"),
            pl.max_horizontal("probability_up", 1 - pl.col("probability_up")).alias("confidence"),
            pl.when(pl.col("probability_up") >= 0.5).then(pl.col("up_ask_vwap_5")).otherwise(pl.col("down_ask_vwap_5")).alias("share_cost"),
        )
        .with_columns((pl.col("confidence") - pl.col("share_cost") - reserve).alias("edge"))
        .filter(
            (pl.col("confidence") >= policy.confidence)
            & (pl.col("edge") >= policy.minimum_edge)
            & (pl.col("share_cost") <= policy.maximum_share_cost)
            & pl.col("share_cost").is_not_null()
        )
        .sort(["market_id", "seconds_elapsed"])
        .group_by("market_id", maintain_order=True).first()
        .sort("window_start")
    )
    return eligible.with_columns(
        (pl.col("side") == pl.col("official_outcome")).alias("won"),
        pl.when(pl.col("side") == pl.col("official_outcome"))
        .then(5 * (1 - pl.col("share_cost") - reserve))
        .otherwise(-5 * (pl.col("share_cost") + reserve)).alias("net_pnl"),
        pl.when(pl.col("side") == pl.col("official_outcome"))
        .then(5 * (1 - pl.col("share_cost") - reserve - stress))
        .otherwise(-5 * (pl.col("share_cost") + reserve + stress)).alias("stress_pnl"),
    )


def main(config_path: Path, output: Path) -> None:
    raw = tomllib.loads(config_path.read_text())
    if not raw["training"]["training_only"] or not raw["training"]["paper_only"]:
        raise RuntimeError("training must remain offline and paper-only")
    if raw["training"]["live_capital_allowed"]:
        raise RuntimeError("training cannot authorize live capital")
    panel = Path(raw["source"]["panel"])
    if sha256(panel) != raw["source"]["panel_sha256"]:
        raise RuntimeError("panel SHA-256 does not match config")
    required = ["market_id", "window_start", "official_outcome", "seconds_elapsed",
                "up_ask_vwap_5", "down_ask_vwap_5", *FEATURES]
    frame = (pl.scan_parquet(panel).select(required).filter(pl.col("has_core") if "has_core" in required else pl.lit(True)).collect())
    # Defensive finite-value normalization; HistGradientBoosting handles NaN as missing.
    frame = frame.with_columns([pl.col(c).cast(pl.Float64).fill_nan(None) for c in FEATURES])
    windows = {k: datetime.fromisoformat(v) for k, v in raw["windows"].items()}
    fit = frame.filter((pl.col("window_start") >= windows["fit_start"]) & (pl.col("window_start") < windows["fit_end"]))
    calibration = frame.filter((pl.col("window_start") >= windows["fit_end"]) & (pl.col("window_start") < windows["calibration_end"]))
    selection = frame.filter((pl.col("window_start") >= windows["calibration_end"]) & (pl.col("window_start") < windows["selection_end"]))
    sealed = frame.filter((pl.col("window_start") >= windows["selection_end"]) & (pl.col("window_start") < windows["sealed_end"]))
    confirmation = frame.filter((pl.col("window_start") >= windows["sealed_end"]) & (pl.col("window_start") < windows["confirmation_end"]))
    estimator = HistGradientBoostingClassifier(random_state=raw["training"]["random_seed"], **raw["model"])
    counts = fit.group_by("market_id").len().rename({"len": "market_rows"})
    weighted = fit.join(counts, on="market_id")
    estimator.fit(weighted.select(FEATURES).to_numpy(), (weighted["official_outcome"] == "up").cast(pl.Int8).to_numpy(), sample_weight=1 / weighted["market_rows"].to_numpy())
    calibration_raw = estimator.predict_proba(calibration.select(FEATURES).to_numpy())[:, 1]
    calibrator = LogisticRegression(C=1.0, random_state=raw["training"]["random_seed"])
    calibrator.fit(np.clip(calibration_raw, 1e-6, 1-1e-6).reshape(-1, 1), (calibration["official_outcome"] == "up").cast(pl.Int8).to_numpy())

    def score(part: pl.DataFrame) -> pl.DataFrame:
        raw_probability = estimator.predict_proba(part.select(FEATURES).to_numpy())[:, 1]
        probability = calibrator.predict_proba(np.clip(raw_probability, 1e-6, 1-1e-6).reshape(-1, 1))[:, 1]
        return part.with_columns(pl.Series("probability_up", probability))

    scored = {name: score(part) for name, part in (("selection", selection), ("sealed", sealed), ("confirmation", confirmation))}
    p = raw["policy"]
    candidates = []
    diagnostics = []
    for confidence in p["confidence_thresholds"]:
        for edge in p["minimum_edges"]:
            for maximum_cost in p["maximum_share_costs"]:
                policy = Policy(confidence, edge, maximum_cost)
                trades = replay(scored["selection"], policy, p["execution_reserve_per_share"], p["stress_slippage_per_share"])
                metrics = trade_metrics(trades)
                ratio = metrics["compensation_ratio"]
                qualifies = (metrics["trades"] >= p["minimum_selection_trades"] and metrics["precision"] >= p["minimum_selection_precision"] and metrics["stress_pnl"] > 0 and (ratio is None or ratio <= p["maximum_compensation_ratio"]))
                diagnostics.append({"policy": policy.__dict__, "qualifies": qualifies, **metrics})
                if qualifies:
                    candidates.append((metrics["precision_wilson_lower"], metrics["stress_pnl"] / metrics["trades"], -metrics["trades"], policy, metrics))
    output.mkdir(parents=True, exist_ok=False)
    diagnostics.sort(
        key=lambda row: (
            row["qualifies"],
            row["trades"] >= p["minimum_selection_trades"],
            row["precision_wilson_lower"],
            row["stress_pnl"] / max(row["trades"], 1),
        ),
        reverse=True,
    )
    (output / "policy-diagnostics.json").write_text(
        json.dumps(diagnostics, indent=2, sort_keys=True) + "\n"
    )
    policy = selection_metrics = None
    if candidates:
        _, _, _, policy, selection_metrics = max(candidates, key=lambda row: row[:3])
    metrics: dict[str, Any] = {
        "schema_version": SCHEMA_VERSION,
        "qualification_status": "qualified" if policy else "no_qualifying_policy",
        "policy": policy.__dict__ if policy else None,
        "windows": {},
        "source_sha256": sha256(panel),
        "best_policy_diagnostics": diagnostics[:20],
    }
    for name, part in scored.items():
        labels = (part["official_outcome"] == "up").cast(pl.Int8).to_numpy()
        probabilities = part["probability_up"].to_numpy()
        metrics["windows"][name] = {
            "markets": part["market_id"].n_unique(),
            "brier": float(brier_score_loss(labels, probabilities)),
            "log_loss": float(log_loss(labels, probabilities)),
        }
        if policy:
            trades = replay(
                part,
                policy,
                p["execution_reserve_per_share"],
                p["stress_slippage_per_share"],
            )
            trades.write_parquet(output / f"{name}-trades.parquet")
            metrics["windows"][name].update(trade_metrics(trades))
    if selection_metrics:
        metrics["windows"]["selection"].update(selection_metrics)
    artifact = {"schema_version": SCHEMA_VERSION, "features": FEATURES, "estimator": estimator, "calibrator": calibrator, "policy": policy, "source_sha256": metrics["source_sha256"]}
    joblib.dump(artifact, output / "model.joblib", compress=3)
    metrics["model_sha256"] = sha256(output / "model.joblib")
    metrics["runtime"] = {"python": platform.python_version(), "numpy": np.__version__, "polars": pl.__version__, "sklearn": sklearn.__version__}
    metrics["git"] = {"branch": subprocess.check_output(["git", "branch", "--show-current"], text=True).strip(), "commit": subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()}
    (output / "metrics.json").write_text(json.dumps(metrics, indent=2, sort_keys=True) + "\n")
    (output / "README.md").write_text(
        "# Conservative selective preliminary run\n\n"
        "Activity: training\n\n"
        "Domain/workflow: BTC directional model / conservative selective preliminary\n\n"
        f"Qualification: `{metrics['qualification_status']}`\n\n"
        f"Branch/commit: `{metrics['git']['branch']}` / `{metrics['git']['commit']}`\n\n"
        f"Config: `{config_path}`\n\n"
        "Entrypoint: `python -m btc_directional_model.conservative_selective_training`\n\n"
        f"Source SHA-256: `{metrics['source_sha256']}`\n\n"
        f"Model SHA-256: `{metrics['model_sha256']}`\n\n"
        "`model.joblib` is the unqualified fitted estimator bundle; `metrics.json` records "
        "coverage and qualification; `policy-diagnostics.json` records the bounded threshold "
        "search. Trade ledgers are emitted only when a policy qualifies.\n"
    )


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    main(args.config, args.output)
