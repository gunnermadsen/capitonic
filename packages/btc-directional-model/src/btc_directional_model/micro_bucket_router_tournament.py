"""Confirm time-local specialists and route only development-qualified bucket layers."""

from __future__ import annotations

import argparse
import hashlib
import json
import platform
import shutil
import subprocess
from dataclasses import asdict
from datetime import UTC, datetime, timedelta
from pathlib import Path
from typing import Any

import joblib
import numpy as np
import polars as pl
import sklearn

from . import time_bucket_specialist_tournament as base
from .core_extract import file_sha256
from .twap60_training_data import _isolated_query_frame

SCHEMA_VERSION = "btc-micro-bucket-router-tournament-v1"
ARTIFACT_SCHEMA_VERSION = "btc-micro-bucket-router-model-v1"
ECONOMIC_COLUMNS = (
    "fee_rate",
    "pm_up_book_age_seconds",
    "pm_down_book_age_seconds",
)


def _write_json(path: Path, payload: Any) -> None:
    base._write_json(path, payload)


def _source_contract(root: Path, raw: dict[str, Any]) -> dict[str, Any]:
    contract = base._source_contract(root, raw)
    extra_paths = {
        name: base._resolve(root, raw["paths"][name])
        for name in (
            "confirmation_execution_sql",
            "prior_metrics",
            "prior_artifact",
            "historic_results_root",
        )
    }
    for name in ("confirmation_execution_sql", "prior_metrics", "prior_artifact"):
        if not extra_paths[name].is_file():
            raise FileNotFoundError(extra_paths[name])
    if not extra_paths["historic_results_root"].is_dir():
        raise FileNotFoundError(extra_paths["historic_results_root"])
    contract["paths"].update({name: str(path) for name, path in extra_paths.items()})
    contract["sha256"].update(
        {
            name: file_sha256(extra_paths[name])
            for name in ("confirmation_execution_sql", "prior_metrics", "prior_artifact")
        }
    )
    return contract


def _historic_inventory(root: Path) -> dict[str, Any]:
    tournaments: dict[str, dict[str, int]] = {}
    parquet_files = 0
    total_bytes = 0
    for path in root.rglob("*.parquet"):
        if "training-results" not in path.parts:
            continue
        parquet_files += 1
        size = path.stat().st_size
        total_bytes += size
        index = path.parts.index("training-results")
        name = path.parts[index + 1] if index + 1 < len(path.parts) else "unknown"
        row = tournaments.setdefault(name, {"parquet_files": 0, "bytes": 0})
        row["parquet_files"] += 1
        row["bytes"] += size
    return {
        "root": str(root),
        "tournaments": dict(sorted(tournaments.items())),
        "tournament_count": len(tournaments),
        "parquet_files": parquet_files,
        "bytes": total_bytes,
        "role": "historic roster and prior-weight evidence",
        "read_only": True,
    }


def _confirmation_execution(
    sql_path: Path,
    start: datetime,
    end: datetime,
    work: Path,
) -> tuple[pl.DataFrame, dict[str, Any]]:
    cache = work / "confirmation-execution"
    cache.mkdir(parents=True, exist_ok=True)
    manifest_path = cache / "manifest.json"
    partial_path = cache / "manifest.partial.json"
    contract = {
        "schema_version": "btc-current-orderbook-capacity-execution-cache-v1",
        "range_start": start.isoformat(),
        "range_end": end.isoformat(),
        "query_sha256": file_sha256(sql_path),
        "read_only": True,
        "database_mutations": False,
        "new_tables": False,
        "new_schemas": False,
        "new_ingesters": False,
        "new_sources": False,
    }
    records: list[dict[str, Any]] = []
    checkpoint = manifest_path if manifest_path.is_file() else partial_path
    if checkpoint.is_file():
        saved = json.loads(checkpoint.read_text())
        if saved["contract"] != contract:
            raise RuntimeError("confirmation execution checkpoint contract changed")
        records = saved["partitions"]
        for row in records:
            path = cache / row["path"]
            if not path.is_file() or file_sha256(path) != row["sha256"]:
                raise RuntimeError(f"confirmation execution partition changed: {path}")
    completed = {Path(row["path"]).stem for row in records}
    sql = sql_path.read_text()
    day = start
    while day < end:
        day_end = min(day + timedelta(days=1), end)
        stem = day.date().isoformat()
        if stem not in completed:
            print(f"extract confirmation execution: {stem}", flush=True)
            frame = _isolated_query_frame(
                sql,
                {"batch_start": day, "batch_end": day_end},
                cursor_name=f"micro_bucket_execution_{day:%Y%m%d}",
                capacity=True,
            )
            destination = cache / f"{stem}.parquet"
            temporary = destination.with_suffix(".parquet.tmp")
            frame.write_parquet(temporary, compression="zstd", statistics=True)
            temporary.replace(destination)
            records.append(
                {
                    "path": destination.name,
                    "rows": frame.height,
                    "markets": frame["market_id"].n_unique(),
                    "sha256": file_sha256(destination),
                }
            )
            _write_json(partial_path, {"contract": contract, "partitions": records})
        day = day_end
    payload = {"contract": contract, "partitions": records}
    _write_json(manifest_path, payload)
    partial_path.unlink(missing_ok=True)
    frames = [pl.read_parquet(cache / row["path"]) for row in records]
    return pl.concat(frames, how="vertical_relaxed", rechunk=True), payload


def _attach_confirmation_execution(
    frame: pl.DataFrame,
    execution: pl.DataFrame,
    raw: dict[str, Any],
) -> pl.DataFrame:
    quantities = tuple(raw["execution"]["quantities"]) + tuple(
        raw["execution"]["capacity_quantities"]
    )
    vwaps = tuple(
        f"{side}_ask_vwap_{quantity}" for side in ("up", "down") for quantity in quantities
    )
    selected = execution.select(
        *base.KEY_COLUMNS,
        "fee_rate",
        "up_provider_received_at",
        "down_provider_received_at",
        *vwaps,
    ).unique(subset=list(base.KEY_COLUMNS), keep="last")
    joined = frame.join(
        selected,
        on=list(base.KEY_COLUMNS),
        how="left",
        suffix="_confirmation",
        validate="m:1",
    )
    replaceable = ("fee_rate", *vwaps)
    joined = joined.with_columns(
        *(
            pl.coalesce(pl.col(f"{name}_confirmation"), pl.col(name)).alias(name)
            for name in replaceable
        ),
        pl.when(pl.col("up_provider_received_at").is_not_null())
        .then(
            (pl.col("observed_at") - pl.col("up_provider_received_at")).dt.total_microseconds()
            / 1_000_000.0
        )
        .otherwise(pl.col("pm_up_book_age_seconds"))
        .alias("pm_up_book_age_seconds"),
        pl.when(pl.col("down_provider_received_at").is_not_null())
        .then(
            (pl.col("observed_at") - pl.col("down_provider_received_at")).dt.total_microseconds()
            / 1_000_000.0
        )
        .otherwise(pl.col("pm_down_book_age_seconds"))
        .alias("pm_down_book_age_seconds"),
    )
    return joined.drop(
        "up_provider_received_at",
        "down_provider_received_at",
        *(f"{name}_confirmation" for name in replaceable),
        strict=False,
    )


def _prior_weight(
    prior: dict[str, Any], prior_candidate: str, rtds_mode: str, floor: float
) -> float:
    name = f"{prior_candidate}__{rtds_mode}"
    row = prior["candidate_results"].get(name)
    if not row:
        return floor
    development = float(row["development"]["stress_net_pnl"])
    design = float(row["sealed"]["stress_net_pnl"])
    return max(floor, max(0.0, development) + max(0.0, design))


def _blend_predictions(
    members: list[str],
    predictions: dict[str, pl.DataFrame],
    weights: dict[str, float],
    name: str,
    *,
    agreement: bool,
) -> pl.DataFrame:
    joined: pl.DataFrame | None = None
    probability_columns = []
    for index, member in enumerate(members):
        column = f"p_{index}"
        probability_columns.append(column)
        selected = predictions[member].select(
            *base.KEY_COLUMNS,
            "label_up",
            pl.col("probability").alias(column),
        )
        if joined is None:
            joined = selected
        else:
            joined = joined.join(
                selected.drop("label_up"),
                on=list(base.KEY_COLUMNS),
                how="inner",
                validate="1:1",
            )
    if joined is None:
        raise RuntimeError("blend has no members")
    denominator = sum(weights[member] for member in members)
    probability = (
        sum(
            pl.col(column) * weights[member]
            for member, column in zip(members, probability_columns, strict=True)
        )
        / denominator
    )
    if agreement:
        joined = joined.filter(
            pl.all_horizontal(pl.col(column) >= 0.5 for column in probability_columns)
            | pl.all_horizontal(pl.col(column) < 0.5 for column in probability_columns)
        )
    return joined.with_columns(
        probability.alias("probability"), pl.lit(name).alias("candidate")
    ).select(*base.KEY_COLUMNS, "label_up", "probability", "candidate")


def _period_result(
    frame: pl.DataFrame,
    predictions: pl.DataFrame,
    policy: base.Policy,
    raw: dict[str, Any],
) -> tuple[dict[str, Any], pl.DataFrame]:
    trades = base._select_trades(
        base._opportunities(frame, predictions, policy.quantity, raw), policy, raw
    )
    return base._economic_metrics(trades, frame["market_id"].n_unique()), trades


def _layer_qualified(layer: dict[str, Any], raw: dict[str, Any]) -> bool:
    spec = raw["router"]
    development = layer["development"]
    design = layer["design"]
    maximum_loss_rate = spec.get("maximum_loss_rate")
    maximum_recovery = spec.get("maximum_recovery_wins_per_loss")
    return (
        development["trades"] >= int(spec["minimum_development_trades"])
        and design["trades"] >= int(spec["minimum_design_trades"])
        and (development["profit_factor"] or 0) >= float(spec["minimum_profit_factor"])
        and (design["profit_factor"] or 0) >= float(spec["minimum_profit_factor"])
        and (not spec["require_positive_development_stress"] or development["stress_net_pnl"] > 0)
        and (not spec["require_positive_design_stress"] or design["stress_net_pnl"] > 0)
        and (
            maximum_loss_rate is None
            or (1.0 - float(design["win_rate"] or 0.0)) <= float(maximum_loss_rate)
        )
        and (
            maximum_recovery is None
            or float(design["recovery_wins_per_loss"] or 0.0) <= float(maximum_recovery)
        )
    )


def _compose_trades(parts: list[pl.DataFrame]) -> pl.DataFrame:
    if not parts:
        return pl.DataFrame()
    return (
        pl.concat(parts, how="diagonal_relaxed")
        .sort(["market_id", "seconds_elapsed"])
        .group_by("market_id", maintain_order=True)
        .first()
        .sort(["window_start", "seconds_elapsed"])
    )


def _select_challenger(
    routers: dict[str, dict[str, dict[str, Any]]],
    challenger_names: tuple[str, ...],
    period: str = "confirmation",
) -> str:
    if not challenger_names:
        raise RuntimeError("no newly trained routers are eligible for selection")
    return max(
        challenger_names,
        key=lambda name: (
            routers[name][period]["stress_net_pnl"],
            routers[name][period]["net_pnl"],
        ),
    )


def _router_metrics(
    trades: pl.DataFrame,
    total_markets: int,
    common_end: datetime,
    common_markets: int,
) -> dict[str, Any]:
    return {
        "confirmation": base._economic_metrics(trades, total_markets),
        "confirmation_common_window": base._economic_metrics(
            trades.filter(pl.col("window_start") < common_end) if not trades.is_empty() else trades,
            common_markets,
        ),
    }


def _router_bucket_metrics(
    trades: pl.DataFrame,
    buckets: dict[str, dict[str, Any]],
    total_markets: int,
) -> dict[str, dict[str, Any]]:
    return {
        name: base._economic_metrics(
            trades.filter(
                pl.col("seconds_elapsed").is_between(
                    int(bucket["start_second"]),
                    int(bucket["end_second"]),
                    closed="both",
                )
            )
            if not trades.is_empty()
            else trades,
            total_markets,
        )
        for name, bucket in buckets.items()
    }


def _reserved_parts(
    names: list[str],
    trades: dict[str, dict[str, pl.DataFrame]],
    layers: dict[str, Any],
    period: str,
    router: dict[str, Any],
) -> list[pl.DataFrame]:
    parts = []
    cutoff = router.get("reservation_before_second")
    confidence_delta = float(router.get("reservation_confidence_delta", 0.0))
    edge_delta = float(router.get("reservation_edge_delta", 0.0))
    for name in names:
        value = trades[name][period]
        if cutoff is not None and int(layers[name]["policy"]["quantity"]) == 5:
            threshold = int(cutoff)
            policy = layers[name]["policy"]
            value = value.filter(
                (pl.col("seconds_elapsed") >= threshold)
                | (
                    (
                        pl.col("selected_probability")
                        >= float(policy["minimum_confidence"]) + confidence_delta
                    )
                    & (pl.col("expected_edge") >= float(policy["minimum_edge"]) + edge_delta)
                )
            )
        parts.append(value)
    return parts


def _report(metrics: dict[str, Any]) -> str:
    common = metrics["confirmation_common_window"]
    common_range = f"{common['start'][:10]} to {common['end_exclusive'][:10]} (end exclusive)"
    lines = [
        "# BTC Five-Minute Micro-Bucket Router Tournament",
        "",
        f"Run: `{metrics['run_id']}`",
        f"Qualification: **{metrics['qualification']}**",
        f"Selected router: **{metrics['selected_router']}**",
        f"Selection evidence: **{metrics.get('selection_period', 'confirmation')}**",
        "",
        "## Frozen hypothesis",
        "",
        "Historically successful model families are retrained inside the time slices where they showed edge; individual, prior-weighted, and agreement-gated descendants are then folded into a no-trade-by-default router.",
        "",
        "## Router comparison — chronological confirmation (not globally blind)",
        "",
        "| Router | PnL | Stress | PF | Trades | UP/DOWN | W/L | Coverage | Avg entry | Recovery | Max DD |",
        "|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|",
    ]
    for name, row in metrics["routers"].items():
        value = row["confirmation"]
        lines.append(
            f"| {name} | {value['net_pnl']:.2f} | {value['stress_net_pnl']:.2f} | "
            f"{value['profit_factor'] or 0:.3f} | {value['trades']} | "
            f"{value['up_trades']}/{value['down_trades']} | {value['wins']}/{value['losses']} | "
            f"{value['market_coverage']:.2%} | {value['average_entry_second'] or 0:.1f} | "
            f"{value['recovery_wins_per_loss'] or 0:.3f} | {value['maximum_drawdown']:.2f} |"
        )
    lines.extend(
        [
            "",
            f"## Common-window comparison — {common_range}",
            "",
            "| Router | PnL | Stress | PF | Trades | W/L | Coverage | Max DD |",
            "|---|---:|---:|---:|---:|---:|---:|---:|",
        ]
    )
    for name, row in metrics["routers"].items():
        value = row["confirmation_common_window"]
        lines.append(
            f"| {name} | {value['net_pnl']:.2f} | {value['stress_net_pnl']:.2f} | "
            f"{value['profit_factor'] or 0:.3f} | {value['trades']} | "
            f"{value['wins']}/{value['losses']} | {value['market_coverage']:.2%} | "
            f"{value['maximum_drawdown']:.2f} |"
        )
    if metrics.get("router_bucket_metrics"):
        for router_name, bucket_rows in metrics["router_bucket_metrics"].items():
            lines.extend(
                [
                    "",
                    f"## {router_name} — confirmation PnL by bucket",
                    "",
                    "| Bucket | Seconds | PnL | Stress | PF | Trades | UP/DOWN | W/L | Win rate | Coverage | Avg entry | Confidence | Recovery | Max DD |",
                    "|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|",
                ]
            )
            bucket_map = {row["name"]: row for row in metrics["buckets"]}
            for bucket_name, value in bucket_rows.items():
                bucket = bucket_map[bucket_name]
                lines.append(
                    f"| {bucket_name} | {bucket['start_second']}-{bucket['end_second']} | "
                    f"{value['net_pnl']:.2f} | {value['stress_net_pnl']:.2f} | "
                    f"{value['profit_factor'] or 0:.3f} | {value['trades']} | "
                    f"{value['up_trades']}/{value['down_trades']} | "
                    f"{value['wins']}/{value['losses']} | {value['win_rate'] or 0:.2%} | "
                    f"{value['market_coverage']:.2%} | {value['average_entry_second'] or 0:.1f} | "
                    f"{value['average_confidence'] or 0:.3f} | "
                    f"{value['recovery_wins_per_loss'] or 0:.3f} | "
                    f"{value['maximum_drawdown']:.2f} |"
                )
    lines.extend(
        [
            "",
            "## Qualified bucket layers",
            "",
            "| Bucket | Layer | Form | RTDS | Side | VWAP | Dev stress | Design stress | Confirm stress | Trades | PF |",
            "|---|---|---|---|---|---:|---:|---:|---:|---:|---:|",
        ]
    )
    for bucket, layers in metrics["qualified_layers"].items():
        for layer in layers:
            confirmation = layer["confirmation"]
            lines.append(
                f"| {bucket} | {layer['name']} | {layer['form']} | {layer['rtds_mode']} | "
                f"{layer['policy']['side']} | {layer['policy']['quantity']} | "
                f"{layer['development']['stress_net_pnl']:.2f} | "
                f"{layer['design']['stress_net_pnl']:.2f} | "
                f"{confirmation['stress_net_pnl']:.2f} | {confirmation['trades']} | "
                f"{confirmation['profit_factor'] or 0:.3f} |"
            )
    lines.extend(
        [
            "",
            "## Integrity and limitations",
            "",
            "- Model fitting ends before the August 14 settlement cutover; August 14 is transition-audit-only.",
            "- Policy fitting, observed design validation, and untouched confirmation are chronological and disjoint.",
            "- August 20–26 is design evidence, not relabeled as an unseen test.",
            "- Confirmation execution is a read-only Parquet snapshot of the established current orderbook table.",
            f"- Early causal feature coverage ends at {common['end_exclusive']} (exclusive); all-router comparisons therefore include the exact common window above.",
            "- No database row, table, schema, source, ingester, runtime model, trading process, or image was changed.",
            "- VWAP replay assumes the recorded ask ladder was fillable and does not model queue position.",
            "- Tournament qualification uses VWAP5 only; larger quantities are post-selection capacity evidence.",
        ]
    )
    return "\n".join(lines) + "\n"


def run_tournament(config_path: Path, *, run_id: str | None = None) -> Path:
    root, raw = base._load_config(config_path)
    source = _source_contract(root, raw)
    contracts = base._feature_contracts(raw, source)
    run_id = run_id or datetime.now(UTC).strftime("%Y%m%dT%H%M%SZ")
    work = base._resolve(root, raw["paths"]["runs"]) / run_id
    committed = base._resolve(root, raw["paths"]["committed_results"]) / run_id
    if committed.exists():
        raise FileExistsError(committed)
    work.mkdir(parents=True, exist_ok=True)
    checkpoints = work / "checkpoints"
    checkpoints.mkdir(exist_ok=True)
    _write_json(work / "source-contract.json", source)
    inventory = _historic_inventory(Path(source["paths"]["historic_results_root"]))
    _write_json(work / "historic-results-inventory.json", inventory)
    prior_artifact = joblib.load(Path(source["paths"]["prior_artifact"]))
    benchmark_features = tuple(
        dict.fromkeys(
            feature for model in prior_artifact["models"].values() for feature in model.features
        )
    )
    columns = base._required_columns([*contracts, {"features": benchmark_features}], raw)
    training = base._load_panel(Path(source["paths"]["training_panel"]), columns)
    evaluation = base._load_panel(Path(source["paths"]["evaluation_panel"]), columns)
    windows = {name: base._parse_time(value) for name, value in raw["windows"].items()}
    execution, execution_manifest = _confirmation_execution(
        Path(source["paths"]["confirmation_execution_sql"]),
        windows["confirmation_start"],
        windows["confirmation_end"],
        work,
    )
    evaluation = _attach_confirmation_execution(evaluation, execution, raw)
    early = base._load_or_build_early_panel(root, raw, source, columns, windows, work)
    early = _attach_confirmation_execution(early, execution, raw)
    prior = json.loads(Path(source["paths"]["prior_metrics"]).read_text())
    config_rows = {row["name"]: row for row in raw["candidates"]}
    buckets = base._bucket_map(raw)
    seed = int(raw["training"]["random_seed"])
    config_sha = file_sha256(config_path)
    implementation_sha = file_sha256(Path(__file__))
    predictions: dict[str, pl.DataFrame] = {}
    results: dict[str, Any] = {}
    trades: dict[str, dict[str, pl.DataFrame]] = {}
    models: dict[str, Any] = {}
    for index, contract in enumerate(contracts):
        name = contract["name"]
        bucket = buckets[contract["bucket"]]
        dataset = early if contract["dataset"] == "early_causal_twap" else evaluation
        fit_dataset = early if contract["dataset"] == "early_causal_twap" else training
        checkpoint = checkpoints / f"{name}.joblib"
        prediction_path = checkpoints / f"{name}-predictions.parquet"
        print(f"candidate {index + 1}/{len(contracts)}: {name}", flush=True)
        if checkpoint.is_file() and prediction_path.is_file():
            saved = joblib.load(checkpoint)
            if (
                saved["config_sha256"] != config_sha
                or saved["implementation_sha256"] != implementation_sha
                or saved["source_sha256"] != source["sha256"]
            ):
                raise RuntimeError(f"checkpoint identity changed for {name}")
            model = saved["model"]
            prediction_all = pl.read_parquet(prediction_path)
        else:
            fit = base._slice(fit_dataset, bucket, windows["source_start"], windows["fit_end"])
            model = base._fit_model(fit, tuple(contract["features"]), raw, seed + index)
            scoring = base._slice(
                dataset, bucket, windows["policy_start"], windows["confirmation_end"]
            )
            prediction_all = base._prediction_frame(scoring, base._predict(model, scoring), name)
            base._write_joblib(
                checkpoint,
                {
                    "config_sha256": config_sha,
                    "implementation_sha256": implementation_sha,
                    "source_sha256": source["sha256"],
                    "model": model,
                    "contract": contract,
                },
            )
            prediction_all.write_parquet(prediction_path, compression="zstd")
        period_frames = {
            "development": base._slice(
                dataset, bucket, windows["policy_start"], windows["policy_end"]
            ),
            "design": base._slice(dataset, bucket, windows["sealed_start"], windows["sealed_end"]),
            "confirmation": base._slice(
                dataset, bucket, windows["confirmation_start"], windows["confirmation_end"]
            ),
        }
        period_predictions = {
            period: prediction_all.filter(
                pl.col("window_start").is_between(
                    frame["window_start"].min(),
                    frame["window_start"].max() + timedelta(minutes=5),
                    closed="left",
                )
            )
            if not frame.is_empty()
            else prediction_all.head(0)
            for period, frame in period_frames.items()
        }
        policy, development_metrics, development_trades = base._select_policy(
            period_frames["development"],
            period_predictions["development"],
            list(bucket["trade_sides"]),
            raw,
        )
        design_metrics, design_trades = _period_result(
            period_frames["design"], period_predictions["design"], policy, raw
        )
        confirmation_metrics, confirmation_trades = _period_result(
            period_frames["confirmation"], period_predictions["confirmation"], policy, raw
        )
        results[name] = {
            **contract,
            "form": "individual",
            "policy": asdict(policy),
            "development": development_metrics,
            "design": design_metrics,
            "confirmation": confirmation_metrics,
            "historical_folds": base._historical_fold_metrics(
                fit_dataset,
                bucket,
                tuple(contract["features"]),
                raw,
                seed + index * 10,
                checkpoints,
                contract["dataset"],
            ),
            "predictive": {
                period: base._predictive_metrics(value)
                for period, value in period_predictions.items()
            },
            "confirmation_capacity": base._capacity(
                period_frames["confirmation"],
                period_predictions["confirmation"],
                policy,
                raw,
            ),
        }
        predictions[name] = prediction_all
        trades[name] = {
            "development": development_trades,
            "design": design_trades,
            "confirmation": confirmation_trades,
        }
        models[name] = model
        _write_json(checkpoints / f"{name}-metrics.json", results[name])

    layers: dict[str, Any] = dict(results)
    for bucket_name, bucket in buckets.items():
        for rtds_mode in ("without_rtds_candles", "with_rtds_candles"):
            members = [
                row["name"]
                for row in results.values()
                if row["bucket"] == bucket_name and row["rtds_mode"] == rtds_mode
            ]
            if len(members) < 2:
                continue
            weights = {
                member: _prior_weight(
                    prior,
                    config_rows[results[member]["candidate"]]["prior_candidate"],
                    rtds_mode,
                    float(raw["router"]["prior_weight_floor"]),
                )
                for member in members
            }
            for form, agreement in (("prior_weighted", False), ("agreement", True)):
                name = f"{bucket_name}__{form}__{rtds_mode}"
                prediction_all = _blend_predictions(
                    members, predictions, weights, name, agreement=agreement
                )
                dataset = early if int(bucket["start_second"]) < 60 else evaluation
                period_frames = {
                    "development": base._slice(
                        dataset, bucket, windows["policy_start"], windows["policy_end"]
                    ),
                    "design": base._slice(
                        dataset, bucket, windows["sealed_start"], windows["sealed_end"]
                    ),
                    "confirmation": base._slice(
                        dataset,
                        bucket,
                        windows["confirmation_start"],
                        windows["confirmation_end"],
                    ),
                }
                period_predictions = {
                    period: prediction_all.join(
                        frame.select(*base.KEY_COLUMNS),
                        on=list(base.KEY_COLUMNS),
                        how="inner",
                        validate="1:1",
                    )
                    for period, frame in period_frames.items()
                }
                try:
                    policy, development_metrics, development_trades = base._select_policy(
                        period_frames["development"],
                        period_predictions["development"],
                        list(bucket["trade_sides"]),
                        raw,
                    )
                except RuntimeError:
                    continue
                design_metrics, design_trades = _period_result(
                    period_frames["design"], period_predictions["design"], policy, raw
                )
                confirmation_metrics, confirmation_trades = _period_result(
                    period_frames["confirmation"],
                    period_predictions["confirmation"],
                    policy,
                    raw,
                )
                layers[name] = {
                    "name": name,
                    "candidate": name,
                    "historical_model": "folded_bucket_layer",
                    "bucket": bucket_name,
                    "dataset": results[members[0]]["dataset"],
                    "rtds_mode": rtds_mode,
                    "form": form,
                    "members": members,
                    "weights": weights,
                    "policy": asdict(policy),
                    "development": development_metrics,
                    "design": design_metrics,
                    "confirmation": confirmation_metrics,
                    "predictive": {
                        period: base._predictive_metrics(value)
                        for period, value in period_predictions.items()
                    },
                }
                predictions[name] = prediction_all
                trades[name] = {
                    "development": development_trades,
                    "design": design_trades,
                    "confirmation": confirmation_trades,
                }

    qualified = {
        bucket: [
            row for row in layers.values() if row["bucket"] == bucket and _layer_qualified(row, raw)
        ]
        for bucket in buckets
    }
    selected_layers: dict[tuple[str, str, str], dict[str, Any]] = {}
    for bucket, rows in qualified.items():
        for mode in ("without_rtds_candles", "with_rtds_candles"):
            for form in ("individual", "prior_weighted", "agreement", "best"):
                eligible = [
                    row
                    for row in rows
                    if row["rtds_mode"] == mode and (form == "best" or row["form"] == form)
                ]
                if eligible:
                    selected_layers[(bucket, mode, form)] = max(
                        eligible,
                        key=lambda row: (
                            base._policy_score(row["design"]),
                            base._policy_score(row["development"]),
                        ),
                    )

    champion_name = raw["router"]["champion_variant"]
    champion_model = prior_artifact["models"][champion_name]
    champion_prior = prior["candidate_results"][champion_name]
    champion_policy = base.Policy(**champion_prior["policy"])
    champion_bucket = {"name": "champion_60_74", "start_second": 60, "end_second": 74}
    champion_frame = base._slice(
        evaluation,
        champion_bucket,
        windows["confirmation_start"],
        windows["confirmation_end"],
    )
    champion_predictions = base._prediction_frame(
        champion_frame, base._predict(champion_model, champion_frame), champion_name
    )
    _, champion_trades = _period_result(champion_frame, champion_predictions, champion_policy, raw)

    def chosen(bucket: str, mode: str, form: str) -> dict[str, Any] | None:
        return selected_layers.get((bucket, mode, form))

    primary = ("early_40_44", "early_60_64", "early_65_69", "early_70_74")
    optional = ("middle_110_119", "middle_150_169", "late_185_209")

    def layer_names(bucket_names: tuple[str, ...], mode: str, form: str) -> list[str]:
        selected = [chosen(bucket, mode, form) for bucket in bucket_names]
        return [row["name"] for row in selected if row is not None]

    def parts(names: list[str]) -> list[pl.DataFrame]:
        return [trades[name]["confirmation"] for name in names]

    router_specs = raw.get("router_definitions")
    if router_specs:
        router_layer_names = {}
        router_spec_by_name = {}
        for router in router_specs:
            router_name = router["name"]
            mode = router.get("rtds_mode", "without_rtds_candles")
            form = router.get("form", "best")
            selected = []
            for bucket_name in router["buckets"]:
                eligible = [
                    row
                    for row in qualified[bucket_name]
                    if (mode == "any" or row["rtds_mode"] == mode)
                    and (form == "best" or row["form"] == form)
                ]
                if eligible:
                    selected.append(
                        max(
                            eligible,
                            key=lambda row: (
                                base._policy_score(row["design"]),
                                base._policy_score(row["development"]),
                            ),
                        )["name"]
                    )
            router_layer_names[router_name] = selected
            router_spec_by_name[router_name] = router
    else:
        router_layer_names = {
            "early_micro_individual": layer_names(primary, "without_rtds_candles", "individual"),
            "early_micro_prior_weighted": layer_names(
                primary, "without_rtds_candles", "prior_weighted"
            ),
            "early_micro_agreement": layer_names(primary, "without_rtds_candles", "agreement"),
            "later_rtds": layer_names(optional, "with_rtds_candles", "best"),
            "full_hybrid": layer_names(primary, "without_rtds_candles", "best")
            + layer_names(optional, "with_rtds_candles", "best"),
            "full_rtds_free": layer_names((*primary, *optional), "without_rtds_candles", "best"),
        }
        router_spec_by_name = {name: {} for name in router_layer_names}

    router_period_trades = {
        name: {
            period: _compose_trades(
                _reserved_parts(names, trades, layers, period, router_spec_by_name[name])
            )
            for period in ("development", "design", "confirmation")
        }
        for name, names in router_layer_names.items()
    }
    router_trades = {
        "champion_replay": champion_trades,
        **{name: periods["confirmation"] for name, periods in router_period_trades.items()},
    }
    prior_parts = []
    for bucket_name, winner in prior_artifact["winners"].items():
        prior_frame = base._slice(
            early if int(winner["start_second"]) < 60 else evaluation,
            {
                "name": bucket_name,
                "start_second": winner["start_second"],
                "end_second": winner["end_second"],
            },
            windows["confirmation_start"],
            windows["confirmation_end"],
        )
        model = prior_artifact["models"][winner["candidate"]]
        prediction = base._prediction_frame(
            prior_frame, base._predict(model, prior_frame), winner["candidate"]
        )
        _, replay = _period_result(prior_frame, prediction, base.Policy(**winner["policy"]), raw)
        prior_parts.append(replay)
    router_trades["prior_composed_replay"] = _compose_trades(prior_parts)

    total_markets = evaluation.filter(
        pl.col("window_start").is_between(
            windows["confirmation_start"], windows["confirmation_end"], closed="left"
        )
    )["market_id"].n_unique()
    common_end = min(
        windows["confirmation_end"],
        early["window_start"].max() + timedelta(minutes=5),
    )
    common_markets = evaluation.filter(
        pl.col("window_start").is_between(windows["confirmation_start"], common_end, closed="left")
    )["market_id"].n_unique()
    routers = {
        name: _router_metrics(value, total_markets, common_end, common_markets)
        for name, value in router_trades.items()
    }
    period_ranges = {
        "development": (windows["policy_start"], windows["policy_end"]),
        "design": (windows["sealed_start"], windows["sealed_end"]),
    }
    for name, period_trades in router_period_trades.items():
        for period, (start, end) in period_ranges.items():
            period_markets = evaluation.filter(
                pl.col("window_start").is_between(start, end, closed="left")
            )["market_id"].n_unique()
            routers[name][period] = base._economic_metrics(period_trades[period], period_markets)
    selection_period = raw["router"].get("selection_period", "confirmation")
    selected_router = _select_challenger(routers, tuple(router_layer_names), selection_period)
    winner = routers[selected_router]["confirmation"]
    benchmark_name = raw["router"].get("qualification_benchmark", "champion_replay")
    champion = routers[benchmark_name]["confirmation"]
    qualification = (
        "trained_evaluated_outperformed_champion"
        if winner["stress_net_pnl"] > champion["stress_net_pnl"]
        and winner["net_pnl"] > 0
        and winner["stress_net_pnl"] > 0
        and (winner["profit_factor"] or 0) > 1
        else "trained_evaluated_did_not_outperform_champion"
    )
    artifact = {
        "schema_version": ARTIFACT_SCHEMA_VERSION,
        "hypothesis": "historic model strengths specialized by time slice and folded into a no-trade-default router",
        "models": models,
        "layers": {
            name: {
                key: value
                for key, value in row.items()
                if key in {"bucket", "form", "rtds_mode", "members", "weights", "policy"}
            }
            for name, row in layers.items()
            if _layer_qualified(row, raw)
        },
        "routers": router_layer_names,
        "router_definitions": router_spec_by_name,
        "selected_router": selected_router,
        "selection_period": selection_period,
        "default_action": "no_trade",
        "paper_only": True,
        "live_capital_allowed": False,
    }
    base._write_joblib(work / "tournament.joblib", artifact)
    artifact_sha = file_sha256(work / "tournament.joblib")
    source_identity = hashlib.sha256(
        json.dumps(source["sha256"], sort_keys=True).encode()
    ).hexdigest()
    metrics = {
        "schema_version": SCHEMA_VERSION,
        "run_id": run_id,
        "source_commit": subprocess.check_output(
            ["git", "rev-parse", "HEAD"], cwd=root, text=True
        ).strip(),
        "artifact_sha256": artifact_sha,
        "qualification": qualification,
        "selected_router": selected_router,
        "hypothesis_preserved": True,
        "windows": raw["windows"],
        "buckets": raw["buckets"],
        "source_contract": source,
        "historic_inventory": inventory,
        "confirmation_execution": execution_manifest,
        "confirmation_common_window": {
            "start": windows["confirmation_start"].isoformat(),
            "end_exclusive": common_end.isoformat(),
            "markets": common_markets,
        },
        "candidate_results": results,
        "bucket_layers": layers,
        "qualified_layers": qualified,
        "routers": routers,
        "router_bucket_metrics": {
            name: _router_bucket_metrics(value, buckets, total_markets)
            for name, value in router_trades.items()
        },
        "versions": {
            "python": platform.python_version(),
            "numpy": np.__version__,
            "polars": pl.__version__,
            "sklearn": sklearn.__version__,
        },
        "limitations": [
            f"The derived early causal feature panel ends at {common_end.isoformat()} (exclusive).",
            "Later feature coverage extends through 2026-09-01 and is compared separately from the common window.",
            "Projected economics assume recorded ask-ladder VWAP was fillable and omit queue position.",
        ],
    }
    _write_json(work / "metrics.json", metrics)
    (work / "report.md").write_text(_report(metrics))
    for name, value in router_trades.items():
        value.write_parquet(work / f"router-trades-{name}.parquet", compression="zstd")
    candidate_rows = []
    for row in layers.values():
        candidate_rows.append(
            {
                "name": row["name"],
                "bucket": row["bucket"],
                "form": row["form"],
                "rtds_mode": row["rtds_mode"],
                "qualified": _layer_qualified(row, raw),
                **{
                    f"{period}_{key}": value
                    for period in ("development", "design", "confirmation")
                    for key, value in row[period].items()
                    if isinstance(value, (int, float)) or value is None
                },
            }
        )
    pl.DataFrame(candidate_rows, infer_schema_length=None).write_parquet(
        work / "bucket-layer-metrics.parquet", compression="zstd"
    )
    pl.DataFrame(
        [
            {
                "router": name,
                **{
                    f"{period}_{key}": value
                    for period, values in row.items()
                    for key, value in values.items()
                },
            }
            for name, row in routers.items()
        ],
        infer_schema_length=None,
    ).write_parquet(work / "router-metrics.parquet", compression="zstd")
    pl.DataFrame(
        [
            {"router": router, "bucket": bucket, **value}
            for router, bucket_rows in metrics["router_bucket_metrics"].items()
            for bucket, value in bucket_rows.items()
        ],
        infer_schema_length=None,
    ).write_parquet(work / "router-bucket-metrics.parquet", compression="zstd")
    _write_json(
        work / "model-provenance.json",
        {
            "schema_version": "btc-model-provenance-v1",
            "model_artifact_sha256": artifact_sha,
            "artifact_path": "tournament.joblib",
            "producing_commit": metrics["source_commit"],
            "training_run_id": run_id,
            "source_identity": source_identity,
            "source_hashes": source["sha256"],
            "configuration_sha256": config_sha,
            "qualification_status": qualification,
            "deployment_status": "not_deployed_training_only",
            "hypothesis_preserved": True,
            "selected_router": selected_router,
        },
    )
    (work / "model-family.sha256").write_text(artifact_sha + "\n")
    _write_json(
        work / "completion.json",
        {
            "complete": True,
            "run_id": run_id,
            "artifact_sha256": artifact_sha,
            "qualification": qualification,
        },
    )
    committed.parent.mkdir(parents=True, exist_ok=True)
    shutil.copytree(work, committed, ignore=shutil.ignore_patterns("checkpoints"))
    return committed


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--run-id")
    arguments = parser.parse_args()
    print(run_tournament(arguments.config, run_id=arguments.run_id))


if __name__ == "__main__":
    main()
