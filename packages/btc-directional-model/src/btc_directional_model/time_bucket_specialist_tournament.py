"""Train a composed challenger from historically successful time-bucket specialists."""

from __future__ import annotations

import argparse
import itertools
import json
import math
import platform
import shutil
import subprocess
import sys
import tomllib
from dataclasses import asdict, dataclass, replace
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

import joblib
import numpy as np
import polars as pl
import sklearn
from scipy.special import logit
from sklearn.ensemble import HistGradientBoostingRegressor
from sklearn.linear_model import LogisticRegression
from sklearn.metrics import brier_score_loss, log_loss, roc_auc_score

from . import latent_twap_tournament as latent
from .binance_latent_twap_context import OI_FEATURES
from .chainlink_oi_features import (
    CHAINLINK_CANDLE_FEATURES,
    _derive_open_interest_source_features,
    _prepare_open_interest,
)
from .core_extract import file_sha256
from .twap60_training_data import attach_candle_context

SCHEMA_VERSION = "btc-time-bucket-specialist-tournament-v1"
ARTIFACT_SCHEMA_VERSION = "btc-time-bucket-specialist-model-v1"
MODULE = "btc_directional_model.time_bucket_specialist_tournament"
if __name__ == "__main__":
    sys.modules[MODULE] = sys.modules[__name__]

FORBIDDEN_FEATURE_TOKENS = (
    "official_outcome",
    "official_label",
    "label_up",
    "final_price",
    "resolution",
    "ask_vwap",
    "share_cost",
    "net_pnl",
)
SAFE_CAUSAL_TWAP_FEATURES = (
    "twap30_margin_bps",
    "twap60_margin_bps",
)
EARLY_FEATURE_GROUPS = {
    "latent_twap": (
        *SAFE_CAUSAL_TWAP_FEATURES,
        "refprice_margin_bps",
        "sensor_source_age_seconds",
        "seconds_elapsed",
    ),
    "latent_refprice": (
        "refprice_margin_bps",
        "sensor_source_age_seconds",
        "seconds_elapsed",
    ),
    "latent_open_interest": tuple(
        feature for feature in OI_FEATURES if "path_agreement" not in feature
    ),
}
KEY_COLUMNS = ("market_id", "window_start", "observed_at", "seconds_elapsed")


@dataclass(frozen=True)
class BucketModel:
    features: tuple[str, ...]
    estimator: HistGradientBoostingRegressor
    calibrator: LogisticRegression | None
    neutralized_columns: tuple[int, ...]


@dataclass(frozen=True)
class Policy:
    side: str
    quantity: int
    minimum_edge: float
    minimum_confidence: float
    maximum_share_cost: float


BucketModel.__module__ = MODULE
Policy.__module__ = MODULE


def _write_json(path: Path, payload: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(json.dumps(payload, indent=2, sort_keys=True, default=str) + "\n")
    temporary.replace(path)


def _write_joblib(path: Path, payload: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + ".tmp")
    joblib.dump(payload, temporary, compress=3)
    temporary.replace(path)


def _parse_time(value: str) -> datetime:
    return datetime.fromisoformat(value)


def _load_config(path: Path) -> tuple[Path, dict[str, Any]]:
    root = path.resolve().parents[1]
    with path.open("rb") as handle:
        raw = tomllib.load(handle)
    if not raw["training"].get("training_only") or not raw["training"].get("paper_only"):
        raise RuntimeError("time-bucket tournament must remain training-only and paper-only")
    if raw["training"].get("live_capital_allowed"):
        raise RuntimeError("time-bucket tournament cannot authorize live capital")
    names = [row["name"] for row in raw["buckets"]]
    if len(names) != len(set(names)):
        raise RuntimeError("bucket names must be unique")
    ordered = sorted(raw["buckets"], key=lambda row: int(row["start_second"]))
    for previous, current in itertools.pairwise(ordered):
        if int(previous["end_second"]) >= int(current["start_second"]):
            raise RuntimeError("training buckets overlap")
    known = set(names)
    if any(row["bucket"] not in known for row in raw["candidates"]):
        raise RuntimeError("candidate references an unknown bucket")
    return root, raw


def _resolve(root: Path, value: str) -> Path:
    path = Path(value)
    return path if path.is_absolute() else root / path


def _source_contract(root: Path, raw: dict[str, Any]) -> dict[str, Any]:
    paths = raw["paths"]
    resolved = {
        name: _resolve(root, paths[name])
        for name in (
            "training_panel",
            "training_manifest",
            "evaluation_panel",
            "evaluation_manifest",
            "early_config",
            "early_source_cache",
            "early_source_manifest",
            "early_source_frame",
            "early_label_audit",
            "early_context_manifest",
            "early_open_interest",
        )
    }
    manifests = {
        "training": json.loads(resolved["training_manifest"].read_text()),
        "evaluation": json.loads(resolved["evaluation_manifest"].read_text()),
    }
    for kind in ("training", "evaluation"):
        panel = resolved[f"{kind}_panel"]
        digest = file_sha256(panel)
        if digest != manifests[kind]["sha256"]:
            raise RuntimeError(f"{kind} panel does not match its immutable manifest")
    early_context = json.loads(resolved["early_context_manifest"].read_text())
    expected_oi_sha = early_context["sources"]["open_interest"]["partitions"][0]["sha256"]
    if file_sha256(resolved["early_open_interest"]) != expected_oi_sha:
        raise RuntimeError("early open-interest partition does not match its immutable manifest")
    return {
        "paths": {name: str(path) for name, path in resolved.items()},
        "sha256": {
            "training_panel": manifests["training"]["sha256"],
            "training_manifest": file_sha256(resolved["training_manifest"]),
            "evaluation_panel": manifests["evaluation"]["sha256"],
            "evaluation_manifest": file_sha256(resolved["evaluation_manifest"]),
            "early_config": file_sha256(resolved["early_config"]),
            "early_source_manifest": file_sha256(resolved["early_source_manifest"]),
            "early_source_frame": file_sha256(resolved["early_source_frame"]),
            "early_label_audit": file_sha256(resolved["early_label_audit"]),
            "early_context_manifest": file_sha256(resolved["early_context_manifest"]),
            "early_open_interest": file_sha256(resolved["early_open_interest"]),
        },
        "feature_groups": manifests["training"]["feature_groups"],
        "training_range": [
            manifests["training"]["range_start"],
            manifests["training"]["range_end"],
        ],
        "evaluation_range": [
            manifests["evaluation"]["range_start"],
            manifests["evaluation"]["range_end"],
        ],
        "read_only": True,
        "database_mutations": False,
        "new_sources": False,
        "new_ingesters": False,
        "new_tables": False,
        "new_schemas": False,
    }


def _feature_contracts(raw: dict[str, Any], source: dict[str, Any]) -> list[dict[str, Any]]:
    groups = {
        **{name: tuple(values) for name, values in source["feature_groups"].items()},
        **EARLY_FEATURE_GROUPS,
    }
    buckets = _bucket_map(raw)
    contracts: list[dict[str, Any]] = []
    for row in raw["candidates"]:
        requested = tuple(row["feature_groups"])
        for rtds_mode in ("without_rtds_candles", "with_rtds_candles"):
            selected_groups = tuple(
                group
                for group in requested
                if group != "candles" or rtds_mode == "with_rtds_candles"
            )
            features = tuple(
                dict.fromkeys(feature for group in selected_groups for feature in groups[group])
            )
            forbidden = [
                feature
                for feature in features
                if any(token in feature.lower() for token in FORBIDDEN_FEATURE_TOKENS)
            ]
            if forbidden:
                raise RuntimeError(f"forbidden inference features in {row['name']}: {forbidden}")
            contracts.append(
                {
                    "name": f"{row['name']}__{rtds_mode}",
                    "candidate": row["name"],
                    "historical_model": row["historical_model"],
                    "bucket": row["bucket"],
                    "dataset": (
                        "early_causal_twap"
                        if int(buckets[row["bucket"]]["start_second"]) < 60
                        else "full_history_panel"
                    ),
                    "rtds_mode": rtds_mode,
                    "feature_groups": selected_groups,
                    "features": features,
                }
            )
    return contracts


def _required_columns(contracts: list[dict[str, Any]], raw: dict[str, Any]) -> tuple[str, ...]:
    quantities = tuple(raw["execution"]["quantities"]) + tuple(
        raw["execution"]["capacity_quantities"]
    )
    return tuple(
        dict.fromkeys(
            (
                *KEY_COLUMNS,
                "label_up",
                "bridge_probability_target",
                "label_weight",
                "fee_rate",
                "pm_up_book_age_seconds",
                "pm_down_book_age_seconds",
                *(f"up_ask_vwap_{quantity}" for quantity in quantities),
                *(f"down_ask_vwap_{quantity}" for quantity in quantities),
                *(feature for contract in contracts for feature in contract["features"]),
            )
        )
    )


def _load_panel(path: Path, columns: tuple[str, ...]) -> pl.DataFrame:
    scan = pl.scan_parquet(path)
    available = set(scan.collect_schema().names())
    selected = [name for name in columns if name in available]
    frame = scan.select(selected).collect()
    for name in columns:
        if name not in frame.columns:
            frame = frame.with_columns(pl.lit(None, dtype=pl.Float64).alias(name))
    return frame.with_columns(
        pl.col("window_start").cast(pl.Datetime(time_zone="UTC")),
        pl.col("observed_at").cast(pl.Datetime(time_zone="UTC")),
    )


def _complete_columns(frame: pl.DataFrame, columns: tuple[str, ...]) -> pl.DataFrame:
    for name in columns:
        if name not in frame.columns:
            frame = frame.with_columns(pl.lit(None, dtype=pl.Float64).alias(name))
    return frame.with_columns(
        pl.col("window_start").cast(pl.Datetime(time_zone="UTC")),
        pl.col("observed_at").cast(pl.Datetime(time_zone="UTC")),
    )


def _attach_early_open_interest(frame: pl.DataFrame, source: pl.DataFrame) -> pl.DataFrame:
    features = EARLY_FEATURE_GROUPS["latent_open_interest"]
    interest = _derive_open_interest_source_features(_prepare_open_interest(source))
    original = tuple(frame.columns)
    joined = (
        frame.with_row_index("_early_row")
        .sort("observed_at")
        .join_asof(
            interest.select("available_at", "source_timestamp", *features),
            left_on="observed_at",
            right_on="available_at",
            strategy="backward",
            allow_exact_matches=False,
        )
    )
    age_us = (pl.col("observed_at") - pl.col("source_timestamp")).dt.total_microseconds()
    eligible = (
        pl.col("available_at").is_not_null()
        & (pl.col("available_at") < pl.col("observed_at"))
        & pl.col("source_timestamp").is_not_null()
        & (pl.col("source_timestamp") < pl.col("observed_at"))
        & (age_us > 0)
        & (age_us <= 300_000_000)
    )
    return (
        joined.with_columns(
            *(pl.when(eligible).then(pl.col(name)).otherwise(None).alias(name) for name in features)
        )
        .sort("_early_row")
        .select(*original, *features)
    )


def _load_or_build_early_panel(
    root: Path,
    raw: dict[str, Any],
    source: dict[str, Any],
    columns: tuple[str, ...],
    windows: dict[str, datetime],
    work: Path,
) -> pl.DataFrame:
    panel_path = work / "early-causal-panel.parquet"
    manifest_path = work / "early-causal-panel-manifest.json"
    if panel_path.is_file() and manifest_path.is_file():
        manifest = json.loads(manifest_path.read_text())
        if file_sha256(panel_path) != manifest["sha256"]:
            raise RuntimeError("early causal panel checkpoint changed")
        print("checkpoint resume: early causal panel", flush=True)
        return _load_panel(panel_path, columns)

    early_config_path = _resolve(root, raw["paths"]["early_config"])
    with early_config_path.open("rb") as handle:
        early_raw = tomllib.load(handle)
    periods = early_raw["periods"]
    early_paths = early_raw["paths"]
    base_config = latent.TournamentConfig(
        source_path=early_config_path,
        package_root=root,
        raw=early_raw,
        profile=str(early_raw["training"]["profile"]),
        model_family=str(early_raw["training"]["model_family"]),
        random_seed=int(early_raw["training"]["random_seed"]),
        freeze_at=_parse_time(early_raw["training"]["freeze_at"]),
        historical_start=_parse_time(periods["historical_fit_start"]),
        historical_end=_parse_time(periods["historical_fit_end"]),
        calibration_start=_parse_time(periods["calibration_start"]),
        calibration_end=_parse_time(periods["calibration_end"]),
        development_start=_parse_time(periods["development_start"]),
        development_end=_parse_time(periods["development_end"]),
        prospective_start=_parse_time(periods["prospective_start"]),
        folds=tuple(
            latent.Fold(
                str(row["name"]),
                _parse_time(row["test_start"]),
                _parse_time(row["test_end"]),
            )
            for row in early_raw["folds"]
        ),
        source_cache=_resolve(root, early_paths["source_cache"]),
        source_frame=_resolve(root, early_paths["source_frame"]),
        source_manifest=_resolve(root, early_paths["source_manifest"]),
        label_audit=_resolve(root, early_paths["label_audit"]),
        comparator_metrics=_resolve(root, early_paths["corrected_comparator_metrics"]),
        runs=_resolve(root, early_paths["runs"]),
        committed_results=_resolve(root, early_paths["committed_results"]),
    )
    freeze_at = min(base_config.freeze_at, windows["confirmation_end"])
    early_config = replace(
        base_config,
        historical_start=windows["source_start"],
        development_start=windows["policy_start"],
        development_end=freeze_at,
        prospective_start=freeze_at,
        source_cache=Path(source["paths"]["early_source_cache"]),
        source_frame=Path(source["paths"]["early_source_frame"]),
        source_manifest=Path(source["paths"]["early_source_manifest"]),
        label_audit=Path(source["paths"]["early_label_audit"]),
    )
    print("build: causal 30-150 second TWAP/refprice sensor frame", flush=True)
    frame, base_manifest = latent.build_training_frame(early_config)
    candles = latent._load_source_partitions(
        early_config.source_cache,
        "candles",
        (
            "open_timestamp",
            "close_timestamp",
            "available_at",
            "open_price",
            "high_price",
            "low_price",
            "close_price",
        ),
    ).unique(subset=["close_timestamp"], keep="last")
    candle_features = attach_candle_context(frame, candles).select(
        *KEY_COLUMNS, *CHAINLINK_CANDLE_FEATURES
    )
    frame = frame.join(candle_features, on=list(KEY_COLUMNS), how="left", validate="1:1")
    frame = _attach_early_open_interest(
        frame, pl.read_parquet(Path(source["paths"]["early_open_interest"]))
    ).with_columns(
        pl.col("label_up").cast(pl.Float64).alias("bridge_probability_target"),
        pl.lit(1.0).alias("label_weight"),
        (
            (pl.col("observed_at") - pl.col("up_provider_received_at"))
            .dt.total_microseconds()
            .cast(pl.Float64)
            / 1_000_000.0
        ).alias("pm_up_book_age_seconds"),
        (
            (pl.col("observed_at") - pl.col("down_provider_received_at"))
            .dt.total_microseconds()
            .cast(pl.Float64)
            / 1_000_000.0
        ).alias("pm_down_book_age_seconds"),
    )
    frame = _complete_columns(frame, columns)
    temporary = panel_path.with_suffix(".parquet.tmp")
    frame.write_parquet(temporary, compression="zstd", statistics=True)
    temporary.replace(panel_path)
    manifest = {
        "schema_version": "btc-time-bucket-early-causal-panel-v1",
        "sha256": file_sha256(panel_path),
        "rows": frame.height,
        "markets": frame["market_id"].n_unique(),
        "range_start": frame["window_start"].min(),
        "range_end": frame["window_start"].max(),
        "base_manifest": base_manifest,
        "causal_twap_features": list(SAFE_CAUSAL_TWAP_FEATURES),
        "settlement_labels_are_supervision_only": True,
        "read_only_sources": True,
        "database_mutations": False,
    }
    _write_json(manifest_path, manifest)
    print(f"checkpoint complete: early causal panel ({frame.height:,} rows)", flush=True)
    return frame


def _bucket_map(raw: dict[str, Any]) -> dict[str, dict[str, Any]]:
    return {row["name"]: row for row in raw["buckets"]}


def _slice(
    frame: pl.DataFrame, bucket: dict[str, Any], start: datetime, end: datetime
) -> pl.DataFrame:
    return frame.filter(
        pl.col("window_start").is_between(start, end, closed="left")
        & pl.col("seconds_elapsed").is_between(
            int(bucket["start_second"]), int(bucket["end_second"]), closed="both"
        )
    )


def _matrix(
    frame: pl.DataFrame, features: tuple[str, ...], excluded: tuple[int, ...] = ()
) -> np.ndarray:
    matrix = np.column_stack(
        [
            frame[name].cast(pl.Float64).fill_nan(None).fill_null(float("nan")).to_numpy()
            for name in features
        ]
    )
    return np.delete(matrix, excluded, axis=1) if excluded else matrix


def _neutralized(matrix: np.ndarray) -> tuple[int, ...]:
    result = []
    for index in range(matrix.shape[1]):
        finite = matrix[np.isfinite(matrix[:, index]), index]
        if len(finite) < 250 or float(finite.min()) == float(finite.max()):
            result.append(index)
    return tuple(result)


def _fit_model(
    frame: pl.DataFrame, features: tuple[str, ...], raw: dict[str, Any], seed: int
) -> BucketModel:
    markets = frame.select("market_id", "window_start").unique("market_id").sort("window_start")
    if markets.height < 250:
        raise RuntimeError(f"insufficient training markets: {markets.height}")
    boundary_index = max(
        1, int(markets.height * (1.0 - float(raw["model"]["calibration_fraction"])))
    )
    boundary = markets["window_start"][boundary_index]
    fit = frame.filter(pl.col("window_start") < boundary)
    calibration = frame.filter(pl.col("window_start") >= boundary)
    matrix = _matrix(fit, features)
    excluded = _neutralized(matrix)
    matrix = np.delete(matrix, excluded, axis=1) if excluded else matrix
    if not matrix.shape[1]:
        raise RuntimeError("candidate has no usable inference features")
    spec = raw["model"]
    target_name = (
        "bridge_probability_target"
        if fit["bridge_probability_target"].is_not_null().any()
        else "label_up"
    )
    target = fit[target_name].fill_null(fit["label_up"]).to_numpy().astype(float)
    weights = fit["label_weight"].fill_null(1.0).to_numpy().astype(float)
    estimator = HistGradientBoostingRegressor(
        loss="squared_error",
        learning_rate=float(spec["learning_rate"]),
        max_iter=int(spec["max_iter"]),
        max_leaf_nodes=int(spec["max_leaf_nodes"]),
        min_samples_leaf=int(spec["min_samples_leaf"]),
        l2_regularization=float(spec["l2_regularization"]),
        max_bins=int(spec["max_bins"]),
        early_stopping=False,
        random_state=seed,
    ).fit(matrix, target, sample_weight=weights)
    raw_probability = np.clip(
        estimator.predict(_matrix(calibration, features, excluded)), 1e-6, 1 - 1e-6
    )
    calibrator: LogisticRegression | None = None
    if calibration["label_up"].n_unique() == 2 and np.ptp(raw_probability) > 1e-9:
        calibrator = LogisticRegression(C=1.0, solver="lbfgs", max_iter=500, random_state=seed).fit(
            logit(raw_probability).reshape(-1, 1), calibration["label_up"].to_numpy()
        )
    return BucketModel(features, estimator, calibrator, excluded)


def _predict(model: BucketModel, frame: pl.DataFrame) -> np.ndarray:
    probability = np.clip(
        model.estimator.predict(_matrix(frame, model.features, model.neutralized_columns)),
        1e-6,
        1 - 1e-6,
    )
    if model.calibrator is not None:
        probability = model.calibrator.predict_proba(logit(probability).reshape(-1, 1))[:, 1]
    return probability


def _prediction_frame(frame: pl.DataFrame, probability: np.ndarray, name: str) -> pl.DataFrame:
    return frame.select(*KEY_COLUMNS, "label_up").with_columns(
        pl.Series("probability", probability), pl.lit(name).alias("candidate")
    )


def _predictive_metrics(frame: pl.DataFrame) -> dict[str, Any]:
    if frame.is_empty():
        return {"rows": 0, "markets": 0, "brier": None, "log_loss": None, "roc_auc": None}
    labels = frame["label_up"].to_numpy().astype(int)
    probability = frame["probability"].to_numpy().astype(float)
    return {
        "rows": frame.height,
        "markets": frame["market_id"].n_unique(),
        "brier": float(brier_score_loss(labels, probability)),
        "log_loss": float(log_loss(labels, probability, labels=[0, 1])),
        "roc_auc": float(roc_auc_score(labels, probability))
        if len(np.unique(labels)) == 2
        else None,
    }


def _opportunities(
    frame: pl.DataFrame,
    predictions: pl.DataFrame,
    quantity: int,
    raw: dict[str, Any],
) -> pl.DataFrame:
    freshness = float(raw["execution"]["freshness_seconds"])
    reserve = float(raw["execution"]["execution_reserve_per_share"])
    up = f"up_ask_vwap_{quantity}"
    down = f"down_ask_vwap_{quantity}"
    return (
        predictions.join(
            frame.select(
                *KEY_COLUMNS,
                "fee_rate",
                "pm_up_book_age_seconds",
                "pm_down_book_age_seconds",
                up,
                down,
            ),
            on=list(KEY_COLUMNS),
            how="left",
            validate="1:1",
        )
        .with_columns(
            pl.when(pl.col("probability") >= 0.5)
            .then(pl.lit("up"))
            .otherwise(pl.lit("down"))
            .alias("side"),
            pl.max_horizontal("probability", 1.0 - pl.col("probability")).alias(
                "selected_probability"
            ),
        )
        .with_columns(
            pl.when(pl.col("side") == "up")
            .then(pl.col(up))
            .otherwise(pl.col(down))
            .alias("share_cost"),
            pl.when(pl.col("side") == "up")
            .then(pl.col("pm_up_book_age_seconds"))
            .otherwise(pl.col("pm_down_book_age_seconds"))
            .alias("book_age_seconds"),
        )
        .filter(pl.col("book_age_seconds").fill_null(math.inf) <= freshness)
        .with_columns(
            (
                pl.col("fee_rate").fill_null(0.0)
                * pl.col("share_cost")
                * (1.0 - pl.col("share_cost"))
            ).alias("fee_per_share")
        )
        .with_columns(
            (
                pl.col("selected_probability")
                - pl.col("share_cost")
                - pl.col("fee_per_share")
                - reserve
            ).alias("expected_edge")
        )
    )


def _select_trades(
    opportunities: pl.DataFrame, policy: Policy, raw: dict[str, Any]
) -> pl.DataFrame:
    reserve = float(raw["execution"]["execution_reserve_per_share"])
    stress = float(raw["execution"]["stress_slippage_per_share"])
    selected = (
        opportunities.filter(
            pl.col("share_cost").is_not_null()
            & pl.col("share_cost").is_finite()
            & (pl.col("share_cost") > 0)
            & (pl.col("share_cost") <= policy.maximum_share_cost)
            & (pl.col("selected_probability") >= policy.minimum_confidence)
            & (pl.col("expected_edge") >= policy.minimum_edge)
            & (pl.lit(policy.side == "both") | (pl.col("side") == policy.side))
        )
        .sort(["market_id", "seconds_elapsed"])
        .group_by("market_id", maintain_order=True)
        .first()
    )
    won = ((pl.col("side") == "up") & (pl.col("label_up") == 1)) | (
        (pl.col("side") == "down") & (pl.col("label_up") == 0)
    )
    return selected.with_columns(won.alias("won")).with_columns(
        pl.when(pl.col("won"))
        .then(1.0 - pl.col("share_cost"))
        .otherwise(-pl.col("share_cost"))
        .sub(pl.col("fee_per_share") + reserve)
        .mul(policy.quantity)
        .alias("net_pnl"),
        pl.when(pl.col("won"))
        .then(1.0 - pl.col("share_cost") - stress)
        .otherwise(-(pl.col("share_cost") + stress))
        .sub(pl.col("fee_per_share") + reserve)
        .mul(policy.quantity)
        .alias("stress_net_pnl"),
    )


def _economic_metrics(trades: pl.DataFrame, total_markets: int) -> dict[str, Any]:
    if trades.is_empty():
        return {
            "trades": 0,
            "wins": 0,
            "losses": 0,
            "win_rate": None,
            "net_pnl": 0.0,
            "stress_net_pnl": 0.0,
            "profit_factor": None,
            "recovery_wins_per_loss": None,
            "market_coverage": 0.0,
            "average_entry_second": None,
            "average_share_cost": None,
            "maximum_drawdown": 0.0,
        }
    pnl = trades["net_pnl"].to_numpy().astype(float)
    wins = pnl[pnl > 0]
    losses = -pnl[pnl < 0]
    cumulative = np.cumsum(pnl)
    drawdown = np.maximum.accumulate(np.r_[0.0, cumulative])[1:] - cumulative
    return {
        "trades": trades.height,
        "wins": len(wins),
        "losses": len(losses),
        "win_rate": float(len(wins) / trades.height),
        "net_pnl": float(pnl.sum()),
        "stress_net_pnl": float(trades["stress_net_pnl"].sum()),
        "profit_factor": float(wins.sum() / losses.sum()) if losses.sum() else None,
        "recovery_wins_per_loss": float(losses.mean() / wins.mean())
        if len(wins) and len(losses)
        else None,
        "market_coverage": float(trades["market_id"].n_unique() / max(total_markets, 1)),
        "average_entry_second": float(trades["seconds_elapsed"].mean()),
        "average_share_cost": float(trades["share_cost"].mean()),
        "maximum_drawdown": float(drawdown.max(initial=0.0)),
    }


def _policy_score(metrics: dict[str, Any]) -> float:
    if not metrics["trades"]:
        return -math.inf
    return (
        metrics["stress_net_pnl"]
        - 0.25 * metrics["maximum_drawdown"]
        + 0.05 * math.sqrt(metrics["trades"])
    )


def _select_policy(
    frame: pl.DataFrame,
    predictions: pl.DataFrame,
    sides: list[str],
    raw: dict[str, Any],
) -> tuple[Policy, dict[str, Any], pl.DataFrame]:
    total_markets = frame["market_id"].n_unique()
    minimum_trades = int(raw["execution"]["minimum_policy_trades"])
    best: tuple[float, Policy, dict[str, Any], pl.DataFrame] | None = None
    for quantity in raw["execution"]["quantities"]:
        opportunities = _opportunities(frame, predictions, int(quantity), raw)
        for side in sides:
            for edge in raw["execution"]["minimum_edges"]:
                for confidence in raw["execution"]["minimum_confidences"]:
                    for cost in raw["execution"]["maximum_share_costs"]:
                        policy = Policy(
                            side, int(quantity), float(edge), float(confidence), float(cost)
                        )
                        trades = _select_trades(opportunities, policy, raw)
                        metrics = _economic_metrics(trades, total_markets)
                        if metrics["trades"] < minimum_trades:
                            continue
                        score = _policy_score(metrics)
                        if best is None or score > best[0]:
                            best = (score, policy, metrics, trades)
    if best is None:
        raise RuntimeError("no policy produced the minimum development trade count")
    score, policy, metrics, trades = best
    return policy, {"selection_score": score, **metrics}, trades


def _capacity(
    frame: pl.DataFrame,
    predictions: pl.DataFrame,
    policy: Policy,
    raw: dict[str, Any],
) -> dict[str, Any]:
    result = {}
    for quantity in (*raw["execution"]["quantities"], *raw["execution"]["capacity_quantities"]):
        candidate = Policy(
            policy.side,
            int(quantity),
            policy.minimum_edge,
            policy.minimum_confidence,
            policy.maximum_share_cost,
        )
        trades = _select_trades(
            _opportunities(frame, predictions, int(quantity), raw), candidate, raw
        )
        result[str(quantity)] = _economic_metrics(trades, frame["market_id"].n_unique())
    return result


def _five_second_metrics(trades: pl.DataFrame, total_markets: int) -> list[dict[str, Any]]:
    if trades.is_empty():
        return []
    return [
        {"entry_second": int(second), **_economic_metrics(part, total_markets)}
        for (second,), part in trades.group_by("seconds_elapsed")
    ]


def _report(metrics: dict[str, Any]) -> str:
    lines = [
        "# BTC Five-Minute Time-Bucket Specialist Tournament",
        "",
        f"Run: `{metrics['run_id']}`",
        f"Qualification: **{metrics['qualification']}**",
        "",
        "## Hypothesis",
        "",
        "Historically successful model families are retrained only for the time slices where they showed edge, then the winning bucket descendants are routed together as one challenger.",
        "",
        "## Composed sealed result",
        "",
        "| PnL | Stress PnL | PF | Trades | Wins/Losses | Coverage | Avg entry | Recovery wins/loss | Max DD |",
        "|---:|---:|---:|---:|---:|---:|---:|---:|---:|",
    ]
    overall = metrics["composed"]["sealed"]
    lines.append(
        f"| {overall['net_pnl']:.2f} | {overall['stress_net_pnl']:.2f} | "
        f"{overall['profit_factor'] or 0:.3f} | {overall['trades']} | "
        f"{overall['wins']}/{overall['losses']} | {overall['market_coverage']:.2%} | "
        f"{overall['average_entry_second'] or 0:.1f} | "
        f"{overall['recovery_wins_per_loss'] or 0:.3f} | {overall['maximum_drawdown']:.2f} |"
    )
    lines.extend(
        [
            "",
            "## Bucket winners",
            "",
            "| Bucket | Winner | RTDS | Side | VWAP | PnL | Stress | PF | Trades | W/L | Coverage | Avg entry | Recovery |",
            "|---|---|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|",
        ]
    )
    for bucket, winner in metrics["winners"].items():
        sealed = winner["sealed"]
        policy = winner["policy"]
        lines.append(
            f"| {bucket} | {winner['candidate']} | {winner['rtds_mode']} | {policy['side']} | "
            f"{policy['quantity']} | {sealed['net_pnl']:.2f} | {sealed['stress_net_pnl']:.2f} | "
            f"{sealed['profit_factor'] or 0:.3f} | {sealed['trades']} | {sealed['wins']}/{sealed['losses']} | "
            f"{sealed['market_coverage']:.2%} | {sealed['average_entry_second'] or 0:.1f} | "
            f"{sealed['recovery_wins_per_loss'] or 0:.3f} |"
        )
    lines.extend(
        [
            "",
            "## Integrity",
            "",
            "- Training, policy fitting, sealed testing, and confirmation windows are chronological and disjoint.",
            "- Settlement labels, outcomes, and execution prices are supervision/evaluation only; the early specialist uses only causal TWAP/refprice state available at its decision timestamp.",
            "- RTDS candle and RTDS-free variants are paired on identical rows, splits, policies, and seeds.",
            "- Economic replay is chronological and admits at most one entry per market.",
            "- Inputs are immutable existing Parquet artifacts; no database, ingester, source, table, schema, runtime, or deployment was changed.",
            "- The sealed interval has been observed by prior research and is chronological but not epistemically fresh.",
        ]
    )
    return "\n".join(lines) + "\n"


def run_tournament(config_path: Path, *, run_id: str | None = None, force: bool = False) -> Path:
    root, raw = _load_config(config_path)
    source = _source_contract(root, raw)
    contracts = _feature_contracts(raw, source)
    config_sha = file_sha256(config_path)
    run_id = run_id or datetime.now(UTC).strftime("%Y%m%dT%H%M%SZ")
    work = _resolve(root, raw["paths"]["runs"]) / run_id
    committed = _resolve(root, raw["paths"]["committed_results"]) / run_id
    if committed.exists() and not force:
        raise RuntimeError(f"committed result already exists: {committed}")
    checkpoints = work / "checkpoints"
    checkpoints.mkdir(parents=True, exist_ok=True)
    _write_json(work / "source-contract.json", source)
    _write_json(work / "candidate-contract.json", {row["name"]: row for row in contracts})
    columns = _required_columns(contracts, raw)
    training = _load_panel(Path(source["paths"]["training_panel"]), columns)
    evaluation = _load_panel(Path(source["paths"]["evaluation_panel"]), columns)
    buckets = _bucket_map(raw)
    windows = {name: _parse_time(value) for name, value in raw["windows"].items()}
    early = _load_or_build_early_panel(root, raw, source, columns, windows, work)
    seed = int(raw["training"]["random_seed"])
    results: dict[str, Any] = {}
    sealed_trade_frames: dict[str, pl.DataFrame] = {}
    artifact_models: dict[str, Any] = {}
    for index, contract in enumerate(contracts):
        name = contract["name"]
        dataset = early if contract["dataset"] == "early_causal_twap" else evaluation
        fit_dataset = early if contract["dataset"] == "early_causal_twap" else training
        checkpoint = checkpoints / f"{name}.joblib"
        prediction_checkpoint = checkpoints / f"{name}-predictions.parquet"
        bucket = buckets[contract["bucket"]]
        print(f"candidate {index + 1}/{len(contracts)}: {name}", flush=True)
        if checkpoint.is_file() and prediction_checkpoint.is_file() and not force:
            saved = joblib.load(checkpoint)
            if saved["config_sha256"] != config_sha or saved["source_sha256"] != source["sha256"]:
                raise RuntimeError(f"checkpoint identity changed for {name}")
            model = saved["model"]
            prediction_all = pl.read_parquet(prediction_checkpoint)
            print(f"checkpoint resume: {name}", flush=True)
        else:
            fit = _slice(fit_dataset, bucket, windows["source_start"], windows["fit_end"])
            model = _fit_model(fit, tuple(contract["features"]), raw, seed + index)
            evaluation_slice = _slice(
                dataset, bucket, windows["policy_start"], windows["confirmation_end"]
            )
            prediction_all = _prediction_frame(
                evaluation_slice, _predict(model, evaluation_slice), name
            )
            _write_joblib(
                checkpoint,
                {
                    "schema_version": ARTIFACT_SCHEMA_VERSION,
                    "config_sha256": config_sha,
                    "source_sha256": source["sha256"],
                    "contract": contract,
                    "model": model,
                },
            )
            prediction_all.write_parquet(prediction_checkpoint, compression="zstd")
        policy_frame = _slice(dataset, bucket, windows["policy_start"], windows["policy_end"])
        sealed_frame = _slice(dataset, bucket, windows["sealed_start"], windows["sealed_end"])
        confirmation_frame = _slice(
            dataset, bucket, windows["confirmation_start"], windows["confirmation_end"]
        )
        policy_predictions = prediction_all.filter(
            pl.col("window_start").is_between(
                windows["policy_start"], windows["policy_end"], closed="left"
            )
        )
        sealed_predictions = prediction_all.filter(
            pl.col("window_start").is_between(
                windows["sealed_start"], windows["sealed_end"], closed="left"
            )
        )
        confirmation_predictions = prediction_all.filter(
            pl.col("window_start").is_between(
                windows["confirmation_start"], windows["confirmation_end"], closed="left"
            )
        )
        policy, development_metrics, development_trades = _select_policy(
            policy_frame, policy_predictions, list(bucket["trade_sides"]), raw
        )
        sealed_trades = _select_trades(
            _opportunities(sealed_frame, sealed_predictions, policy.quantity, raw), policy, raw
        )
        confirmation_trades = _select_trades(
            _opportunities(confirmation_frame, confirmation_predictions, policy.quantity, raw),
            policy,
            raw,
        )
        sealed_metrics = _economic_metrics(sealed_trades, sealed_frame["market_id"].n_unique())
        confirmation_metrics = _economic_metrics(
            confirmation_trades, confirmation_frame["market_id"].n_unique()
        )
        results[name] = {
            **contract,
            "policy": asdict(policy),
            "development": development_metrics,
            "sealed": sealed_metrics,
            "confirmation": confirmation_metrics,
            "predictive": {
                "development": _predictive_metrics(policy_predictions),
                "sealed": _predictive_metrics(sealed_predictions),
                "confirmation": _predictive_metrics(confirmation_predictions),
            },
            "sealed_five_second": _five_second_metrics(
                sealed_trades, sealed_frame["market_id"].n_unique()
            ),
            "sealed_capacity": _capacity(sealed_frame, sealed_predictions, policy, raw),
        }
        artifact_models[name] = model
        sealed_trade_frames[name] = sealed_trades.with_columns(
            pl.lit(name).alias("candidate"),
            pl.lit(contract["bucket"]).alias("bucket"),
            pl.lit(contract["rtds_mode"]).alias("rtds_mode"),
        )
        development_trades.write_parquet(
            checkpoints / f"{name}-development-trades.parquet", compression="zstd"
        )
        sealed_trade_frames[name].write_parquet(
            checkpoints / f"{name}-sealed-trades.parquet", compression="zstd"
        )
        _write_json(checkpoints / f"{name}-metrics.json", results[name])
        print(
            f"checkpoint complete: {name} "
            f"sealed_pnl={sealed_metrics['net_pnl']:.2f} trades={sealed_metrics['trades']}",
            flush=True,
        )
    winners: dict[str, Any] = {}
    winner_trade_frames = []
    for bucket_name in buckets:
        rows = [value for value in results.values() if value["bucket"] == bucket_name]
        winner = max(
            rows,
            key=lambda row: (
                _policy_score(row["development"]),
                row["rtds_mode"] == "without_rtds_candles",
            ),
        )
        winners[bucket_name] = winner
        winner_trade_frames.append(sealed_trade_frames[winner["name"]])
    composed_trades = (
        pl.concat(winner_trade_frames, how="diagonal_relaxed")
        .sort(["market_id", "seconds_elapsed"])
        .group_by("market_id", maintain_order=True)
        .first()
        .sort(["window_start", "seconds_elapsed"])
    )
    sealed_markets = evaluation.filter(
        pl.col("window_start").is_between(
            windows["sealed_start"], windows["sealed_end"], closed="left"
        )
    )["market_id"].n_unique()
    composed_metrics = _economic_metrics(composed_trades, sealed_markets)
    rtds_winners = sum(winner["rtds_mode"] == "with_rtds_candles" for winner in winners.values())
    qualification = (
        "trained_evaluated_positive_sealed"
        if composed_metrics["stress_net_pnl"] > 0 and (composed_metrics["profit_factor"] or 0) > 1
        else "trained_evaluated_not_qualified"
    )
    artifact = {
        "schema_version": ARTIFACT_SCHEMA_VERSION,
        "hypothesis": "historically informed models retrained as time-local specialists and routed into one challenger",
        "models": artifact_models,
        "winners": {
            bucket: {
                "candidate": winner["name"],
                "policy": winner["policy"],
                "start_second": buckets[bucket]["start_second"],
                "end_second": buckets[bucket]["end_second"],
            }
            for bucket, winner in winners.items()
        },
        "default_action": "no_trade",
    }
    _write_joblib(work / "tournament.joblib", artifact)
    artifact_sha = file_sha256(work / "tournament.joblib")
    metrics = {
        "schema_version": SCHEMA_VERSION,
        "run_id": run_id,
        "source_commit": subprocess.check_output(
            ["git", "rev-parse", "HEAD"], cwd=root, text=True
        ).strip(),
        "artifact_sha256": artifact_sha,
        "qualification": qualification,
        "hypothesis_preserved": True,
        "source_contract": source,
        "windows": raw["windows"],
        "buckets": raw["buckets"],
        "candidate_results": results,
        "winners": winners,
        "composed": {"sealed": composed_metrics},
        "rtds": {
            "paired_variants_per_candidate": True,
            "bucket_winners_using_candles": rtds_winners,
            "bucket_winners_without_candles": len(winners) - rtds_winners,
        },
        "versions": {
            "python": platform.python_version(),
            "numpy": np.__version__,
            "polars": pl.__version__,
            "sklearn": sklearn.__version__,
        },
        "limitations": [
            "The chronological sealed interval has been observed by prior research and is not epistemically fresh.",
            "Archived executable order-book coverage is incomplete and varies by date and VWAP quantity.",
            "Projected PnL assumes recorded ask VWAP was fillable and does not model queue position.",
        ],
    }
    _write_json(work / "metrics.json", metrics)
    (work / "report.md").write_text(_report(metrics))
    composed_trades.write_parquet(work / "composed-sealed-trades.parquet", compression="zstd")
    _write_json(
        work / "completion.json",
        {
            "complete": True,
            "run_id": run_id,
            "artifact_sha256": artifact_sha,
            "qualification": qualification,
        },
    )
    if committed.exists():
        shutil.rmtree(committed)
    shutil.copytree(work, committed, ignore=shutil.ignore_patterns("checkpoints"))
    return committed


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--run-id")
    parser.add_argument("--force", action="store_true")
    args = parser.parse_args()
    print(run_tournament(args.config, run_id=args.run_id, force=args.force))


if __name__ == "__main__":
    main()
