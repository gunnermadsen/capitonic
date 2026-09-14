"""Train historical time-slice specialists and route them as aggregate challengers."""

from __future__ import annotations

import argparse
import hashlib
import json
import platform
import shutil
import subprocess
import sys
import tomllib
from dataclasses import asdict
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

import joblib
import numpy as np
import polars as pl
import sklearn

from .core_extract import file_sha256
from .historical_signature_replay import _bucket_results, _screened
from .time_bucket_specialist_tournament import (
    KEY_COLUMNS,
    Policy,
    _economic_metrics,
    _fit_model,
    _load_panel,
    _opportunities,
    _predict,
    _prediction_frame,
    _predictive_metrics,
    _select_policy,
    _select_trades,
    _slice,
    _write_joblib,
    _write_json,
)

SCHEMA_VERSION = "btc-historical-aggregate-tournament-v1"
ARTIFACT_SCHEMA_VERSION = "btc-historical-aggregate-model-v1"
MODULE = "btc_directional_model.historical_aggregate_tournament"
if __name__ == "__main__":
    sys.modules[MODULE] = sys.modules[__name__]


def _parse_time(value: str) -> datetime:
    return datetime.fromisoformat(value)


def _resolve(root: Path, value: str) -> Path:
    path = Path(value)
    return path if path.is_absolute() else root / path


def _load_config(path: Path) -> tuple[Path, dict[str, Any]]:
    root = path.resolve().parents[1]
    with path.open("rb") as handle:
        raw = tomllib.load(handle)
    training = raw["training"]
    if not training.get("training_only") or not training.get("paper_only"):
        raise RuntimeError("aggregate tournament must remain training-only and paper-only")
    if training.get("live_capital_allowed"):
        raise RuntimeError("aggregate tournament cannot authorize live capital")
    execution = raw["execution"]
    if execution["fixed_primary_quantity"] != 5 or execution["quantities"] != [5]:
        raise RuntimeError("aggregate tournament execution must remain locked to VWAP5")
    buckets = raw["buckets"]
    if buckets["width_seconds"] != 10 or buckets["start_second"] != 60 or buckets["end_second"] != 239:
        raise RuntimeError("aggregate tournament buckets must remain frozen at 60-239 by 10 seconds")
    windows = raw["windows"]
    ordered = [
        _parse_time(windows[name])
        for name in ("source_start", "fit_end", "policy_end", "sealed_end", "confirmation_end")
    ]
    if ordered != sorted(ordered) or _parse_time(windows["policy_start"]) != ordered[1] or _parse_time(windows["sealed_start"]) != ordered[2] or _parse_time(windows["confirmation_start"]) != ordered[3]:
        raise RuntimeError("aggregate tournament windows must be contiguous and chronological")
    if raw["selection"]["use_post_cutoff_results_for_selection"]:
        raise RuntimeError("post-cutoff historical results cannot select donors")
    if raw["selection"]["minimum_historical_trades"] != 30:
        raise RuntimeError("historical donor screen is frozen at 30 trades")
    if _parse_time(raw["selection"]["selection_cutoff"]) != ordered[1]:
        raise RuntimeError("donor selection cutoff must equal fit_end")
    return root, raw


def _bucket_contracts(raw: dict[str, Any]) -> list[dict[str, Any]]:
    spec = raw["buckets"]
    return [
        {
            "name": f"seconds_{start}_{start + spec['width_seconds'] - 1}",
            "start_second": start,
            "end_second": start + spec["width_seconds"] - 1,
            "trade_sides": ["up", "down", "both"],
        }
        for start in range(spec["start_second"], spec["end_second"] + 1, spec["width_seconds"])
    ]


def _recipe_for(candidate: str, recipes: list[dict[str, Any]]) -> str | None:
    lowered = candidate.lower()
    for recipe in recipes:
        if any(token.lower() in lowered for token in recipe["match_tokens"]):
            return str(recipe["name"])
    return None


def _select_donors(
    historical: pl.DataFrame, raw: dict[str, Any], buckets: list[dict[str, Any]]
) -> tuple[dict[str, list[str]], dict[str, list[dict[str, Any]]]]:
    cutoff = _parse_time(raw["selection"]["selection_cutoff"])
    pre_cutoff = historical.filter(pl.col("window_start") < cutoff)
    results = _screened(_bucket_results(pre_cutoff)).filter(
        pl.col("bucket_width") == raw["selection"]["historical_bucket_width"]
    ).with_columns(
        (pl.col("positive_evidence_blocks") / pl.col("evidence_blocks")).alias("positive_block_rate"),
        (pl.col("stress_net_pnl") / pl.col("trades")).alias("stress_per_trade"),
    )
    recipes = raw["recipes"]
    limit = int(raw["selection"]["donors_per_bucket"])
    donor_map: dict[str, list[str]] = {}
    evidence: dict[str, list[dict[str, Any]]] = {}
    for bucket in buckets:
        ranked = results.filter(pl.col("bucket_start") == bucket["start_second"]).sort(
            [
                "positive_block_rate",
                "positive_evidence_blocks",
                "positive_calendar_months",
                "stress_per_trade",
                "trades",
            ],
            descending=True,
        )
        selected: list[str] = []
        selected_evidence: list[dict[str, Any]] = []
        for row in ranked.iter_rows(named=True):
            recipe = _recipe_for(str(row["candidate"]), recipes)
            if recipe is None or recipe in selected:
                continue
            selected.append(recipe)
            selected_evidence.append(
                {
                    "recipe": recipe,
                    "signature_id": row["signature_id"],
                    "historical_candidate": row["candidate"],
                    "trades": row["trades"],
                    "stress_net_pnl": row["stress_net_pnl"],
                    "profit_factor": row["profit_factor"],
                    "range_end": row["range_end"],
                }
            )
            if len(selected) == limit:
                break
        if not selected:
            raise RuntimeError(f"no reproducible pre-cutoff donor for {bucket['name']}")
        donor_map[bucket["name"]] = selected
        evidence[bucket["name"]] = selected_evidence
    return donor_map, evidence


def _feature_contracts(raw: dict[str, Any], manifest: dict[str, Any], donor_map: dict[str, list[str]]) -> dict[tuple[str, str], tuple[str, ...]]:
    groups = {name: tuple(values) for name, values in manifest["feature_groups"].items()}
    recipes = {row["name"]: row for row in raw["recipes"]}
    contracts: dict[tuple[str, str], tuple[str, ...]] = {}
    for donors in donor_map.values():
        for donor in donors:
            requested = tuple(recipes[donor]["feature_groups"])
            for mode in ("without_rtds_candles", "with_rtds_candles"):
                selected_groups = tuple(
                    group for group in requested if group != "candles" or mode == "with_rtds_candles"
                )
                contracts[(donor, mode)] = tuple(
                    dict.fromkeys(feature for group in selected_groups for feature in groups[group])
                )
    return contracts


def _required_columns(contracts: dict[tuple[str, str], tuple[str, ...]]) -> tuple[str, ...]:
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
                "up_ask_vwap_5",
                "down_ask_vwap_5",
                *(feature for features in contracts.values() for feature in features),
            )
        )
    )


def _aggregate_predictions(
    predictions: list[pl.DataFrame], kind: str, name: str
) -> pl.DataFrame:
    base = predictions[0].select(*KEY_COLUMNS, "label_up").sort(KEY_COLUMNS)
    joined = base
    probability_names: list[str] = []
    for index, frame in enumerate(predictions):
        column = f"probability_{index}"
        probability_names.append(column)
        joined = joined.join(
            frame.select(*KEY_COLUMNS, pl.col("probability").alias(column)),
            on=list(KEY_COLUMNS),
            how="inner",
            validate="1:1",
        )
    if kind == "leader" or len(probability_names) == 1:
        probability = pl.col(probability_names[0])
    elif kind == "mean":
        probability = pl.mean_horizontal(*probability_names)
    elif kind == "side_split":
        up_probability = pl.col(probability_names[0])
        down_probability = pl.col(probability_names[1])
        probability = (
            pl.when(up_probability - 0.5 >= 0.5 - down_probability)
            .then(up_probability)
            .otherwise(down_probability)
        )
    elif kind == "consensus":
        all_up = pl.all_horizontal(*(pl.col(column) >= 0.5 for column in probability_names))
        all_down = pl.all_horizontal(*(pl.col(column) < 0.5 for column in probability_names))
        probability = (
            pl.when(all_up | all_down)
            .then(pl.mean_horizontal(*probability_names))
            .otherwise(None)
        )
    else:
        raise RuntimeError(f"unknown aggregate kind: {kind}")
    return joined.select(*KEY_COLUMNS, "label_up", probability.alias("probability")).with_columns(
        pl.lit(name).alias("candidate")
    )


def _split_audit(panel: pl.DataFrame, windows: dict[str, datetime]) -> dict[str, Any]:
    specs = {
        "fit": (windows["source_start"], windows["fit_end"]),
        "policy": (windows["policy_start"], windows["policy_end"]),
        "sealed": (windows["sealed_start"], windows["sealed_end"]),
        "confirmation": (windows["confirmation_start"], windows["confirmation_end"]),
    }
    markets = {
        name: set(
            panel.filter(pl.col("window_start").is_between(start, end, closed="left"))[
                "market_id"
            ].to_list()
        )
        for name, (start, end) in specs.items()
    }
    overlaps = {
        f"{left}_vs_{right}": len(markets[left] & markets[right])
        for index, left in enumerate(specs)
        for right in list(specs)[index + 1 :]
    }
    if any(overlaps.values()):
        raise RuntimeError(f"market leakage across splits: {overlaps}")
    return {"markets": {name: len(values) for name, values in markets.items()}, "pairwise_market_overlap": overlaps}


def _period_bucket_metrics(
    frame: pl.DataFrame,
    predictions: pl.DataFrame,
    policy: Policy,
    raw: dict[str, Any],
) -> tuple[dict[str, Any], pl.DataFrame]:
    trades = _select_trades(_opportunities(frame, predictions, 5, raw), policy, raw)
    return _economic_metrics(trades, frame["market_id"].n_unique()), trades


def _available_predictive_metrics(frame: pl.DataFrame) -> dict[str, Any]:
    available = frame.filter(
        pl.col("probability").is_not_null() & pl.col("probability").is_finite()
    )
    return _predictive_metrics(available)


def _report(metrics: dict[str, Any]) -> str:
    lines = [
        "# Historical time-bucket aggregate tournament",
        "",
        "Four aggregate challengers were trained from reproducible historical donor families in frozen ten-second buckets. All PnL is five-share PnL.",
        "",
        "## Net challenger metrics",
        "",
        "| Challenger | RTDS | Period | PnL | Stress | PF | Trades | W/L | UP/DOWN | Coverage | Avg entry | Recovery | Max DD |",
        "|---|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|",
    ]
    for result in metrics["challengers"].values():
        for period in ("policy", "sealed", "confirmation"):
            row = result[period]
            pf = "—" if row["profit_factor"] is None else f"{row['profit_factor']:.3f}"
            recovery = "—" if row["recovery_wins_per_loss"] is None else f"{row['recovery_wins_per_loss']:.3f}"
            lines.append(
                f"| `{result['challenger']}` | {result['rtds_mode']} | {period} | {row['net_pnl']:.2f} | {row['stress_net_pnl']:.2f} | {pf} | {row['trades']} | {row['wins']}/{row['losses']} | {row['up_trades']}/{row['down_trades']} | {row['market_coverage']:.2%} | {row['average_entry_second'] or 0:.1f} | {recovery} | {row['maximum_drawdown']:.2f} |"
            )
    lines.extend(
        [
            "",
            "## Previous tournament comparison",
            "",
            "| Model | Period | PnL | Stress | PF | Trades | W/L |",
            "|---|---|---:|---:|---:|---:|---:|",
        ]
    )
    for period in ("sealed", "confirmation"):
        row = metrics["previous_tournament"]["metrics"][period]
        pf = "—" if row["profit_factor"] is None else f"{row['profit_factor']:.3f}"
        lines.append(
            f"| Previous bucket tournament | {period} | {row['net_pnl']:.2f} | {row['stress_net_pnl']:.2f} | {pf} | {row['trades']} | {row['wins']}/{row['losses']} |"
        )
    for challenger in metrics["challenger_names"]:
        lines.extend(
            [
                "",
                f"## {challenger}: per-bucket PnL",
                "",
                "| Seconds | RTDS | Period | Donors | PnL | Stress | PF | Trades | W/L | UP/DOWN | Coverage | Avg entry | Recovery |",
                "|---:|---|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|",
            ]
        )
        rows = [
            row
            for result in metrics["challengers"].values()
            if result["challenger"] == challenger
            for row in result["bucket_results"]
            if row["period"] in {"sealed", "confirmation"}
        ]
        for row in sorted(rows, key=lambda value: (value["bucket_start"], value["rtds_mode"], value["period"])):
            value = row["metrics"]
            pf = "—" if value["profit_factor"] is None else f"{value['profit_factor']:.3f}"
            recovery = "—" if value["recovery_wins_per_loss"] is None else f"{value['recovery_wins_per_loss']:.3f}"
            lines.append(
                f"| {row['bucket_start']}-{row['bucket_end']} | {row['rtds_mode']} | {row['period']} | {', '.join(row['donors'])} | {value['net_pnl']:.2f} | {value['stress_net_pnl']:.2f} | {pf} | {value['trades']} | {value['wins']}/{value['losses']} | {value['up_trades']}/{value['down_trades']} | {value['market_coverage']:.2%} | {value['average_entry_second'] or 0:.1f} | {recovery} |"
            )
    lines.extend(
        [
            "",
            "## Integrity and interpretation",
            "",
            "- Fit, policy, sealed, and confirmation markets have zero overlap.",
            "- Donor selection uses only historical trade evidence before August 26.",
            "- All variants use executable VWAP5, the same fees/reserve, one-cent-per-share stress, and at most one aggregate entry per market.",
            "- The source panel includes all existing core, Chainlink candle/oracle/RefPrice, Binance, Kraken, open-interest, and L2 feature groups; individual donor recipes retain their historical feature scope.",
            "- The panel has no pre-60-second observations, so the aggregate abstains before 60 seconds rather than inventing coverage.",
            "- September has been viewed in earlier research, so these are chronologically isolated but not epistemically fresh results.",
        ]
    )
    return "\n".join(lines) + "\n"


def run_tournament(config_path: Path, *, run_id: str | None = None, resume: bool = False) -> Path:
    root, raw = _load_config(config_path)
    run_id = run_id or datetime.now(UTC).strftime("%Y%m%dT%H%M%SZ")
    work = _resolve(root, raw["paths"]["runs"]) / run_id
    committed = _resolve(root, raw["paths"]["committed_results"]) / run_id
    checkpoints = work / "checkpoints"
    checkpoints.mkdir(parents=True, exist_ok=True)
    config_sha = file_sha256(config_path)
    panel_path = _resolve(root, raw["paths"]["panel"])
    manifest_path = _resolve(root, raw["paths"]["panel_manifest"])
    manifest = json.loads(manifest_path.read_text())
    if file_sha256(panel_path) != manifest["sha256"]:
        raise RuntimeError("full-coverage panel does not match its immutable manifest")
    historical_path = _resolve(root, raw["paths"]["historical_trades"])
    inventory_path = _resolve(root, raw["paths"]["historical_inventory"])
    historical = pl.read_parquet(historical_path)
    buckets = _bucket_contracts(raw)
    donor_map, donor_evidence = _select_donors(historical, raw, buckets)
    contracts = _feature_contracts(raw, manifest, donor_map)
    panel = _load_panel(panel_path, _required_columns(contracts))
    windows = {name: _parse_time(value) for name, value in raw["windows"].items()}
    split_audit = _split_audit(panel, windows)
    source_contract = {
        "panel": str(panel_path),
        "panel_sha256": manifest["sha256"],
        "panel_manifest": str(manifest_path),
        "panel_manifest_sha256": file_sha256(manifest_path),
        "historical_trades": str(historical_path),
        "historical_trades_sha256": file_sha256(historical_path),
        "historical_inventory": json.loads(inventory_path.read_text()),
        "historical_inventory_sha256": file_sha256(inventory_path),
        "available_feature_groups": manifest["feature_groups"],
        "read_only": True,
        "database_mutations": False,
        "new_sources": False,
        "new_ingesters": False,
        "new_tables": False,
        "new_schemas": False,
    }
    _write_json(work / "source-contract.json", source_contract)
    _write_json(work / "frozen-donor-map.json", {"donors": donor_map, "evidence": donor_evidence})
    seed = int(raw["training"]["random_seed"])
    models: dict[str, Any] = {}
    predictions: dict[tuple[str, str, str], pl.DataFrame] = {}
    needed = [
        (bucket, donor, mode)
        for bucket in buckets
        for donor in donor_map[bucket["name"]]
        for mode in ("without_rtds_candles", "with_rtds_candles")
    ]
    for index, (bucket, donor, mode) in enumerate(needed):
        identity = f"{bucket['name']}__{donor}__{mode}"
        model_path = checkpoints / f"{identity}.joblib"
        prediction_path = checkpoints / f"{identity}-predictions.parquet"
        print(f"specialist {index + 1}/{len(needed)}: {identity}", flush=True)
        if resume and model_path.is_file() and prediction_path.is_file():
            saved = joblib.load(model_path)
            if saved["config_sha256"] != config_sha or saved["panel_sha256"] != manifest["sha256"]:
                raise RuntimeError(f"checkpoint identity changed: {identity}")
            model = saved["model"]
            prediction = pl.read_parquet(prediction_path)
            print(f"checkpoint resume: {identity}", flush=True)
        else:
            fit = _slice(panel, bucket, windows["source_start"], windows["fit_end"])
            features = contracts[(donor, mode)]
            model = _fit_model(fit, features, raw, seed + index)
            evaluation = _slice(panel, bucket, windows["policy_start"], windows["confirmation_end"])
            prediction = _prediction_frame(evaluation, _predict(model, evaluation), identity)
            _write_joblib(
                model_path,
                {
                    "schema_version": ARTIFACT_SCHEMA_VERSION,
                    "config_sha256": config_sha,
                    "panel_sha256": manifest["sha256"],
                    "bucket": bucket,
                    "donor": donor,
                    "rtds_mode": mode,
                    "features": features,
                    "model": model,
                },
            )
            prediction.write_parquet(prediction_path, compression="zstd", statistics=True)
        models[identity] = model
        predictions[(bucket["name"], donor, mode)] = prediction
    challenger_results: dict[str, Any] = {}
    artifact_policies: dict[str, Any] = {}
    ledgers: dict[str, dict[str, pl.DataFrame]] = {}
    for challenger in raw["challengers"]:
        for mode in ("without_rtds_candles", "with_rtds_candles"):
            variant = f"{challenger['name']}__{mode}"
            bucket_results: list[dict[str, Any]] = []
            period_trades: dict[str, list[pl.DataFrame]] = {"policy": [], "sealed": [], "confirmation": []}
            artifact_policies[variant] = {}
            for bucket in buckets:
                donors = donor_map[bucket["name"]]
                member_predictions = [predictions[(bucket["name"], donor, mode)] for donor in donors]
                aggregate = _aggregate_predictions(member_predictions, challenger["kind"], variant)
                frames = {
                    "policy": _slice(panel, bucket, windows["policy_start"], windows["policy_end"]),
                    "sealed": _slice(panel, bucket, windows["sealed_start"], windows["sealed_end"]),
                    "confirmation": _slice(panel, bucket, windows["confirmation_start"], windows["confirmation_end"]),
                }
                aggregate_periods = {
                    period: aggregate.filter(
                        pl.col("window_start").is_between(
                            windows[f"{period}_start"], windows[f"{period}_end"], closed="left"
                        )
                    )
                    for period in frames
                }
                try:
                    policy, policy_metrics, policy_trades = _select_policy(
                        frames["policy"],
                        aggregate_periods["policy"],
                        ["up", "down", "both"],
                        raw,
                    )
                except RuntimeError as error:
                    if "no policy produced" not in str(error):
                        raise
                    policy = Policy("both", 5, 1.0, 1.0, 0.0)
                    policy_trades = _select_trades(
                        _opportunities(frames["policy"], aggregate_periods["policy"], 5, raw),
                        policy,
                        raw,
                    )
                    policy_metrics = {
                        "selection_score": None,
                        **_economic_metrics(
                            policy_trades, frames["policy"]["market_id"].n_unique()
                        ),
                    }
                artifact_policies[variant][bucket["name"]] = asdict(policy)
                metrics_by_period = {"policy": policy_metrics}
                trades_by_period = {"policy": policy_trades}
                for period in ("sealed", "confirmation"):
                    metrics_by_period[period], trades_by_period[period] = _period_bucket_metrics(
                        frames[period], aggregate_periods[period], policy, raw
                    )
                for period, value in metrics_by_period.items():
                    bucket_results.append(
                        {
                            "bucket": bucket["name"],
                            "bucket_start": bucket["start_second"],
                            "bucket_end": bucket["end_second"],
                            "rtds_mode": mode,
                            "period": period,
                            "donors": donors,
                            "policy": asdict(policy),
                            "predictive": _available_predictive_metrics(
                                aggregate_periods[period]
                            ),
                            "metrics": value,
                        }
                    )
                    period_trades[period].append(
                        trades_by_period[period].with_columns(
                            pl.lit(challenger["name"]).alias("challenger"),
                            pl.lit(mode).alias("rtds_mode"),
                            pl.lit(bucket["name"]).alias("bucket"),
                        )
                    )
            combined: dict[str, pl.DataFrame] = {}
            totals: dict[str, dict[str, Any]] = {}
            for period, frames in period_trades.items():
                combined[period] = (
                    pl.concat(frames, how="diagonal_relaxed")
                    .sort(["market_id", "seconds_elapsed"])
                    .group_by("market_id", maintain_order=True)
                    .first()
                    .sort(["window_start", "seconds_elapsed"])
                )
                total_markets = panel.filter(
                    pl.col("window_start").is_between(
                        windows[f"{period}_start"], windows[f"{period}_end"], closed="left"
                    )
                )["market_id"].n_unique()
                totals[period] = _economic_metrics(combined[period], total_markets)
            challenger_results[variant] = {
                "challenger": challenger["name"],
                "kind": challenger["kind"],
                "rtds_mode": mode,
                "donor_map": donor_map,
                "bucket_results": bucket_results,
                **totals,
            }
            ledgers[variant] = combined
            _write_json(checkpoints / f"{variant}-metrics.json", challenger_results[variant])
            for period, frame in combined.items():
                frame.write_parquet(checkpoints / f"{variant}-{period}-trades.parquet", compression="zstd", statistics=True)
            print(
                f"aggregate complete: {variant} sealed={totals['sealed']['net_pnl']:.2f} confirmation={totals['confirmation']['net_pnl']:.2f}",
                flush=True,
            )
    qualification = {
        variant: (
            "trained_evaluated_positive_sealed_and_confirmation"
            if result["sealed"]["stress_net_pnl"] > 0
            and (result["sealed"]["profit_factor"] or 0) > 1
            and result["confirmation"]["stress_net_pnl"] > 0
            and (result["confirmation"]["profit_factor"] or 0) > 1
            else "trained_evaluated_not_qualified"
        )
        for variant, result in challenger_results.items()
    }
    artifact = {
        "schema_version": ARTIFACT_SCHEMA_VERSION,
        "hypothesis": "historically successful model recipes retrained as ten-second specialists and folded into aggregate challengers",
        "models": models,
        "donor_map": donor_map,
        "aggregate_kinds": {row["name"]: row["kind"] for row in raw["challengers"]},
        "policies": artifact_policies,
        "default_action": "no_trade",
        "entry_seconds": [60, 239],
        "quantity": 5,
    }
    _write_joblib(work / "tournament.joblib", artifact)
    artifact_sha = file_sha256(work / "tournament.joblib")
    previous_metrics_path = _resolve(root, raw["paths"]["previous_tournament_metrics"])
    metrics = {
        "schema_version": SCHEMA_VERSION,
        "run_id": run_id,
        "source_commit": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip(),
        "artifact_sha256": artifact_sha,
        "hypothesis_preserved": True,
        "qualification": qualification,
        "windows": raw["windows"],
        "buckets": buckets,
        "execution": raw["execution"],
        "selection": raw["selection"],
        "donor_map": donor_map,
        "donor_evidence": donor_evidence,
        "challenger_names": [row["name"] for row in raw["challengers"]],
        "challengers": challenger_results,
        "integrity": {
            "chronological_market_splits": split_audit,
            "maximum_pairwise_market_overlap": max(split_audit["pairwise_market_overlap"].values()),
            "primary_execution_quantity_locked": 5,
            "post_cutoff_donor_selection": False,
        },
        "source_contract": source_contract,
        "previous_tournament": {
            "path": str(previous_metrics_path),
            "sha256": file_sha256(previous_metrics_path),
            "metrics": json.loads(previous_metrics_path.read_text())["composed"],
        },
        "versions": {
            "python": platform.python_version(),
            "numpy": np.__version__,
            "polars": pl.__version__,
            "sklearn": sklearn.__version__,
        },
        "limitations": [
            "The full-coverage executable panel begins at second 60, so earlier aggregate buckets abstain.",
            "September windows are chronologically isolated within this run but have been viewed in prior research.",
            "Archived entry prices are assumed executable at the recorded VWAP5 and do not model queue position.",
        ],
    }
    _write_json(work / "metrics.json", metrics)
    (work / "report.md").write_text(_report(metrics))
    for variant, periods in ledgers.items():
        for period, frame in periods.items():
            frame.write_parquet(work / f"{variant}-{period}-trades.parquet", compression="zstd", statistics=True)
    pl.DataFrame(
        [
            {
                "variant": variant,
                "challenger": result["challenger"],
                "rtds_mode": result["rtds_mode"],
                "qualification": qualification[variant],
                **{
                    f"{period}_{key}": value
                    for period in ("policy", "sealed", "confirmation")
                    for key, value in result[period].items()
                    if isinstance(value, (int, float)) or value is None
                },
            }
            for variant, result in challenger_results.items()
        ],
        infer_schema_length=None,
    ).write_parquet(work / "challenger-metrics.parquet", compression="zstd", statistics=True)
    pl.DataFrame(
        [
            {
                "variant": variant,
                "challenger": result["challenger"],
                **{key: value for key, value in row.items() if key not in {"metrics", "predictive", "policy", "donors"}},
                "donors": ",".join(row["donors"]),
                **{f"metric_{key}": value for key, value in row["metrics"].items()},
                **{f"predictive_{key}": value for key, value in row["predictive"].items()},
                **{f"policy_{key}": value for key, value in row["policy"].items()},
            }
            for variant, result in challenger_results.items()
            for row in result["bucket_results"]
        ],
        infer_schema_length=None,
    ).write_parquet(work / "bucket-metrics.parquet", compression="zstd", statistics=True)
    _write_json(
        work / "model-provenance.json",
        {
            "schema_version": "btc-model-provenance-v1",
            "model_artifact_sha256": artifact_sha,
            "artifact_path": "tournament.joblib",
            "producing_commit": metrics["source_commit"],
            "training_run_id": run_id,
            "source_identity": hashlib.sha256(json.dumps(source_contract, sort_keys=True, default=str).encode()).hexdigest(),
            "source_hashes": {
                "panel": manifest["sha256"],
                "historical_trades": source_contract["historical_trades_sha256"],
            },
            "configuration_sha256": config_sha,
            "qualification_status": qualification,
            "deployment_status": "not_deployed",
            "hypothesis_preserved": True,
        },
    )
    _write_json(
        work / "completion.json",
        {"complete": True, "run_id": run_id, "artifact_sha256": artifact_sha, "qualification": qualification},
    )
    if committed.exists() and not resume:
        shutil.rmtree(committed)
    shutil.copytree(work, committed, dirs_exist_ok=resume, ignore=shutil.ignore_patterns("checkpoints"))
    return committed


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--run-id")
    parser.add_argument("--resume-run")
    args = parser.parse_args()
    print(run_tournament(args.config, run_id=args.resume_run or args.run_id, resume=args.resume_run is not None))


if __name__ == "__main__":
    main()
