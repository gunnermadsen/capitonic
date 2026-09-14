"""Replay archived model trade ledgers into comparable entry-time buckets.

This is a read-only evidence analysis.  It does not fit models, modify source
artifacts, or infer decisions at timestamps absent from an archived ledger.
"""

from __future__ import annotations

import argparse
import json
from collections import Counter
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

import polars as pl

BUCKET_WIDTHS = (5, 10, 15, 30)
REPLAY_OUTPUT_PREFIX = "btc-5m-historical-signature-replay-"
STANDARD_LEDGER_DIRECTORIES = {
    "ledgers",
    "development-ledgers",
    "holdout-ledgers",
    "evaluation-ledgers",
}
EXCLUDED_NAME_PARTS = (
    "prediction",
    "decision",
    "opportunity",
    "abstention",
    "transition",
    "slice-matrix",
    "candidate-metrics",
    "historical-fold",
)
EXPLICIT_Q5_PNL_COLUMNS = (
    "fee_adjusted_pnl_5",
    "reserve_adjusted_pnl_5",
)
EXPLICIT_Q5_STRESS_COLUMNS = ("stressed_pnl_5",)
COST_COLUMNS = ("share_cost", "selected_cost_5", "contract_cost")
PROBABILITY_COLUMNS = (
    "model_probability_up",
    "directional_family_probability_up",
    "probability_up",
    "selected_probability_up",
)


def _write_json(path: Path, payload: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(json.dumps(payload, indent=2, sort_keys=True, default=str) + "\n")
    temporary.replace(path)


def _training_relative(path: Path) -> Path | None:
    positions = [index for index, part in enumerate(path.parts) if part == "training-results"]
    if not positions:
        return None
    index = positions[-1]
    if len(path.parts) <= index + 2:
        return None
    return Path(*path.parts[index + 1 :])


def _canonical_files(
    local_root: Path, archive_root: Path
) -> tuple[dict[Path, Path], dict[str, Any]]:
    selected: dict[Path, Path] = {}
    origins: dict[Path, str] = {}
    counts: Counter[str] = Counter()
    for origin, root in (("archive", archive_root), ("local", local_root)):
        if not root.is_dir():
            continue
        for path in root.rglob("*"):
            if not path.is_file() or path.suffix not in {".parquet", ".json", ".joblib"}:
                continue
            relative = _training_relative(path)
            if relative is None:
                if root.name == "training-results":
                    relative = path.relative_to(root)
                else:
                    continue
            if relative.parts[0].startswith(REPLAY_OUTPUT_PREFIX):
                continue
            counts[f"{origin}_files_seen"] += 1
            if relative in selected:
                counts["duplicate_relative_paths"] += 1
            selected[relative] = path
            origins[relative] = origin
    return selected, {
        **counts,
        "canonical_files": len(selected),
        "local_preferred_on_duplicate": True,
        "origin_counts": dict(Counter(origins.values())),
    }


def _looks_like_trade_ledger(relative: Path) -> bool:
    name = relative.name.lower()
    full_name = relative.as_posix().lower()
    if relative.suffix != ".parquet" or any(
        part in full_name for part in EXCLUDED_NAME_PARTS
    ):
        return False
    parents = {part.lower() for part in relative.parts}
    return (
        "trade" in name
        or "evaluation-ledger" in name
        or bool(parents & STANDARD_LEDGER_DIRECTORIES)
    )


def _evidence_role(relative: Path) -> str:
    lowered = "/".join(relative.parts).lower()
    for role in (
        "confirmation",
        "prospective",
        "sealed",
        "heldout",
        "holdout",
        "evaluation",
        "test",
        "development",
        "oof",
    ):
        if role in lowered:
            return role
    if relative.name.startswith("router-trades-"):
        return "router_replay"
    return "unspecified"


def _default_candidate(relative: Path) -> str:
    remainder = list(relative.parts[2:-1])
    context = [part for part in remainder if part.lower() not in STANDARD_LEDGER_DIRECTORIES]
    stem = relative.stem
    for suffix in (
        "-evaluation-ledger",
        "-development-trades",
        "-heldout-trades",
        "-sealed-trades",
        "-prospective-trades",
        "-test-trades",
        "-trades",
        "_trades",
    ):
        if stem.endswith(suffix):
            stem = stem[: -len(suffix)]
            break
    if stem.startswith("router-trades-"):
        stem = stem.removeprefix("router-trades-")
    return ":".join((*context, stem)) if context else stem


def _normalized_side(frame: pl.DataFrame) -> pl.Expr | None:
    columns = set(frame.columns)
    if "side" in columns:
        value = pl.col("side").cast(pl.String).str.to_lowercase()
        return (
            pl.when(value.is_in(["up", "yes", "1", "true"]))
            .then(pl.lit("up"))
            .when(value.is_in(["down", "no", "0", "false"]))
            .then(pl.lit("down"))
            .otherwise(None)
        )
    probability = next((name for name in PROBABILITY_COLUMNS if name in columns), None)
    if probability:
        return pl.when(pl.col(probability) >= 0.5).then(pl.lit("up")).otherwise(pl.lit("down"))
    if "direction_correct" in columns and "label_up" in columns:
        predicted_up = (
            pl.when(pl.col("direction_correct"))
            .then(pl.col("label_up"))
            .otherwise(1 - pl.col("label_up"))
        )
        return pl.when(predicted_up == 1).then(pl.lit("up")).otherwise(pl.lit("down"))
    return None


def _normalize_ledger(path: Path, relative: Path) -> tuple[pl.DataFrame | None, str | None]:
    schema = pl.scan_parquet(path).collect_schema()
    columns = set(schema.names())
    required = {"market_id", "seconds_elapsed"}
    if not required <= columns:
        return None, "missing market_id or seconds_elapsed"
    wanted = required | {
        "window_start",
        "observed_at",
        "candidate",
        "strategy_candidate",
        "label_up",
        "direction_correct",
        "side",
        "fee_per_share",
        "selected_fee_per_share",
        *EXPLICIT_Q5_PNL_COLUMNS,
        *EXPLICIT_Q5_STRESS_COLUMNS,
        *COST_COLUMNS,
        *PROBABILITY_COLUMNS,
    }
    frame = pl.read_parquet(path, columns=sorted(wanted & columns))
    if frame.is_empty():
        return None, "empty ledger"
    side = _normalized_side(frame)
    if side is None:
        return None, "direction unavailable"
    cost_column = next((name for name in COST_COLUMNS if name in columns), None)
    pnl_column = next((name for name in EXPLICIT_Q5_PNL_COLUMNS if name in columns), None)
    if pnl_column is None and cost_column is None:
        return None, "five-share economics unavailable"
    if "window_start" in columns:
        window_start = pl.col("window_start").cast(pl.Datetime("us", "UTC"), strict=False)
    elif "observed_at" in columns:
        window_start = pl.col("observed_at").cast(
            pl.Datetime("us", "UTC"), strict=False
        ) - pl.duration(seconds=pl.col("seconds_elapsed"))
    else:
        return None, "timestamp unavailable"
    frame = frame.with_columns(side.alias("side"), window_start.alias("window_start"))
    if "direction_correct" in columns:
        correct = pl.col("direction_correct").cast(pl.Boolean, strict=False)
    elif "label_up" in columns:
        correct = (
            pl.when(pl.col("side") == "up")
            .then(pl.col("label_up") == 1)
            .otherwise(pl.col("label_up") == 0)
        )
    else:
        return None, "outcome unavailable"
    frame = frame.with_columns(correct.alias("direction_correct"))
    if cost_column:
        fee_column = next(
            (name for name in ("fee_per_share", "selected_fee_per_share") if name in columns), None
        )
        fee = (
            pl.col(fee_column).cast(pl.Float64, strict=False).fill_null(0.0)
            if fee_column
            else pl.lit(0.0)
        )
        cost = pl.col(cost_column).cast(pl.Float64, strict=False)
        pnl = 5.0 * (
            pl.when(pl.col("direction_correct"))
            .then(1.0 - cost)
            .otherwise(-cost)
            - fee
        )
        economic_basis = f"reconstructed_q5_at_archived_entry_price:{cost_column}"
        stress = pnl - 0.05
    else:
        pnl = pl.col(pnl_column).cast(pl.Float64, strict=False)
        economic_basis = f"archived_explicit_q5:{pnl_column}"
        stress_column = next(
            (name for name in EXPLICIT_Q5_STRESS_COLUMNS if name in columns), None
        )
        stress = (
            pl.col(stress_column).cast(pl.Float64, strict=False)
            if stress_column
            else pnl - 0.05
        )
    candidate = (
        pl.col("candidate").cast(pl.String).fill_null(_default_candidate(relative))
        if "candidate" in columns
        else pl.lit(_default_candidate(relative))
    )
    if "strategy_candidate" in columns:
        candidate = pl.concat_str(
            pl.col("strategy_candidate").cast(pl.String).fill_null("unknown_strategy"),
            candidate,
            separator="|",
        )
    cost = pl.col(cost_column).cast(pl.Float64, strict=False) if cost_column else pl.lit(None)
    normalized = frame.select(
        pl.lit(relative.parts[0]).alias("tournament"),
        pl.lit(relative.parts[1]).alias("run_id"),
        candidate.alias("candidate"),
        pl.lit(_evidence_role(relative)).alias("evidence_role"),
        pl.lit(relative.as_posix()).alias("source_artifact"),
        pl.lit(economic_basis).alias("economic_basis"),
        pl.col("market_id").cast(pl.String),
        pl.col("window_start"),
        pl.col("seconds_elapsed").cast(pl.Int16, strict=False),
        pl.col("side"),
        pl.col("direction_correct"),
        cost.alias("share_cost"),
        pnl.alias("net_pnl"),
        stress.alias("stress_net_pnl"),
    ).filter(
        pl.col("window_start").is_not_null()
        & pl.col("seconds_elapsed").is_between(0, 299)
        & pl.col("side").is_not_null()
        & pl.col("direction_correct").is_not_null()
        & pl.col("net_pnl").is_finite()
        & pl.col("net_pnl").is_between(-5.1, 5.1)
    )
    if normalized.is_empty():
        return None, "no complete replay rows"
    return normalized, None


def _metrics(frame: pl.DataFrame) -> dict[str, Any]:
    pnl = frame["net_pnl"]
    wins = pnl.filter(pnl > 0)
    losses = -pnl.filter(pnl < 0)
    gross_profit = float(wins.sum())
    gross_loss = float(losses.sum())
    return {
        "trades": frame.height,
        "wins": len(wins),
        "losses": len(losses),
        "up_trades": frame.filter(pl.col("side") == "up").height,
        "down_trades": frame.filter(pl.col("side") == "down").height,
        "net_pnl": float(pnl.sum()),
        "stress_net_pnl": float(frame["stress_net_pnl"].sum()),
        "gross_profit": gross_profit,
        "gross_loss": gross_loss,
        "profit_factor": gross_profit / gross_loss if gross_loss else None,
        "average_entry_second": float(frame["seconds_elapsed"].mean()),
        "average_share_cost": (
            float(frame["share_cost"].drop_nulls().mean())
            if frame["share_cost"].null_count() < frame.height
            else None
        ),
        "recovery_wins_per_loss": (
            float(losses.mean() / wins.mean()) if len(wins) and len(losses) else None
        ),
        "range_start": frame["window_start"].min(),
        "range_end": frame["window_start"].max(),
        "days": frame["window_start"].dt.date().n_unique(),
    }


def _bucket_results(trades: pl.DataFrame) -> pl.DataFrame:
    rows: list[dict[str, Any]] = []
    signatures = trades.partition_by(
        ["tournament", "run_id", "candidate"], maintain_order=True, as_dict=True
    )
    for identity, frame in signatures.items():
        tournament, run_id, candidate = identity
        for width in BUCKET_WIDTHS:
            bucketed = frame.with_columns(
                ((pl.col("seconds_elapsed") // width) * width).alias("bucket_start"),
                pl.col("window_start").dt.strftime("%Y-%m").alias("month"),
            )
            for bucket, part in bucketed.partition_by("bucket_start", as_dict=True).items():
                bucket_start = int(bucket[0] if isinstance(bucket, tuple) else bucket)
                metrics = _metrics(part)
                block_metrics = [
                    _metrics(block)
                    for block in part.partition_by("source_artifact", maintain_order=True)
                ]
                month_metrics = [
                    _metrics(month) for month in part.partition_by("month", maintain_order=True)
                ]
                rows.append(
                    {
                        "tournament": tournament,
                        "run_id": run_id,
                        "candidate": candidate,
                        "signature_id": f"{tournament}/{run_id}/{candidate}",
                        "bucket_width": width,
                        "bucket_start": bucket_start,
                        "bucket_end": bucket_start + width - 1,
                        **metrics,
                        "evidence_blocks": len(block_metrics),
                        "positive_evidence_blocks": sum(
                            value["stress_net_pnl"] > 0 for value in block_metrics
                        ),
                        "calendar_months": len(month_metrics),
                        "positive_calendar_months": sum(
                            value["stress_net_pnl"] > 0 for value in month_metrics
                        ),
                        "source_artifacts": part["source_artifact"].n_unique(),
                        "economic_bases": ",".join(sorted(part["economic_basis"].unique())),
                    }
                )
    return pl.DataFrame(rows).sort(
        ["bucket_width", "bucket_start", "stress_net_pnl"], descending=[False, False, True]
    )


def _screened(frame: pl.DataFrame) -> pl.DataFrame:
    return frame.filter(
        (pl.col("trades") >= 30)
        & (pl.col("net_pnl") > 0)
        & (pl.col("stress_net_pnl") > 0)
        & (pl.col("profit_factor") > 1.0)
        & (
            (pl.col("positive_evidence_blocks") >= 2)
            | (pl.col("positive_calendar_months") >= 2)
            | (pl.col("trades") >= 100)
        )
    )


def _retrospective_aggregates(
    trades: pl.DataFrame, results: pl.DataFrame
) -> tuple[pl.DataFrame, pl.DataFrame]:
    selections: list[dict[str, Any]] = []
    aggregate_rows: list[dict[str, Any]] = []
    eligible = _screened(results).with_columns(
        (pl.col("positive_evidence_blocks") / pl.col("evidence_blocks")).alias(
            "positive_block_rate"
        ),
        (pl.col("stress_net_pnl") / pl.col("trades")).alias("stress_per_trade"),
    )
    for width in BUCKET_WIDTHS:
        choices = eligible.filter(pl.col("bucket_width") == width)
        selected: list[pl.DataFrame] = []
        for bucket, part in choices.partition_by("bucket_start", as_dict=True).items():
            bucket_start = int(bucket[0] if isinstance(bucket, tuple) else bucket)
            winner = part.sort(
                [
                    "positive_block_rate",
                    "positive_evidence_blocks",
                    "positive_calendar_months",
                    "stress_per_trade",
                    "trades",
                ],
                descending=True,
            ).row(0, named=True)
            selections.append(winner)
            selected.append(
                trades.filter(
                    (pl.col("tournament") == winner["tournament"])
                    & (pl.col("run_id") == winner["run_id"])
                    & (pl.col("candidate") == winner["candidate"])
                    & pl.col("seconds_elapsed").is_between(bucket_start, bucket_start + width - 1)
                )
            )
        if not selected:
            continue
        replay = (
            pl.concat(selected, how="vertical_relaxed")
            .sort(["window_start", "market_id", "seconds_elapsed"])
            .unique(subset=["market_id", "window_start"], keep="first", maintain_order=True)
        )
        aggregate_rows.append(
            {
                "bucket_width": width,
                "selected_bucket_signatures": len(selected),
                **_metrics(replay),
                "interpretation": "retrospective in-sample feasibility sketch; not a trained or qualified challenger",
            }
        )
    return pl.DataFrame(aggregate_rows), pl.DataFrame(selections)


def _latest_bucketed_summary(local_root: Path) -> pl.DataFrame:
    candidates = sorted(
        local_root.glob("btc-5m-time-bucket-specialist-tournament-20260321-20260914/*/metrics.json")
    )
    if not candidates:
        return pl.DataFrame()
    metrics = json.loads(candidates[-1].read_text())
    rows: list[dict[str, Any]] = []
    for variant, result in metrics["candidate_results"].items():
        for period in ("development", "sealed", "confirmation"):
            rows.append(
                {
                    "variant": variant,
                    "historical_model": result["historical_model"],
                    "assigned_bucket": result["bucket"],
                    "rtds_mode": result["rtds_mode"],
                    "period": period,
                    **result[period],
                }
            )
        for value in result["sealed_five_second"]:
            rows.append(
                {
                    "variant": variant,
                    "historical_model": result["historical_model"],
                    "assigned_bucket": f"{value['entry_second']}-{value['entry_second'] + 4}",
                    "rtds_mode": result["rtds_mode"],
                    "period": "sealed_five_second",
                    **value,
                }
            )
    return pl.DataFrame(rows, infer_schema_length=None)


def _format_metric(value: float | None, digits: int = 2) -> str:
    return "—" if value is None else f"{value:.{digits}f}"


def _report(
    inventory: dict[str, Any],
    results: pl.DataFrame,
    aggregates: pl.DataFrame,
    selections: pl.DataFrame,
    latest: pl.DataFrame,
) -> str:
    screened = _screened(results)
    lines = [
        "# BTC five-minute historical model signature replay",
        "",
        "This is a read-only replay of archived model outputs. No model was trained, and no missing prediction was reconstructed.",
        "",
        "## Inventory",
        "",
        f"- Canonical archived files examined: **{inventory['canonical_files']}**",
        f"- Unique training-result runs found: **{inventory['training_runs']}**",
        f"- Candidate Parquet ledgers inspected: **{inventory['candidate_ledgers']}**",
        f"- Replayable ledgers: **{inventory['replayable_ledgers']}**",
        f"- Unique replayable model signatures: **{inventory['replayable_signatures']}**",
        f"- Normalized archived trades: **{inventory['normalized_trade_rows']}**",
        f"- Unavailable ledgers retained in inventory: **{inventory['unavailable_ledgers']}**",
        "",
        "## Feasibility",
        "",
        "A signature below is descriptive evidence, not a promotion gate or a new standard. The screening view retains at least 30 trades, positive raw and stressed PnL, PF above one, and either two positive blocks/months or 100 trades.",
        "Within each bucket, the feasibility sketch prefers broader positive evidence support, then stressed PnL per trade and sample size; it does not simply choose the largest PnL.",
        "",
        "| Bucket width | Replay cells | Positive stressed cells | Screened cells | Covered bucket starts |",
        "|---:|---:|---:|---:|---:|",
    ]
    for width in BUCKET_WIDTHS:
        part = results.filter(pl.col("bucket_width") == width)
        stable = screened.filter(pl.col("bucket_width") == width)
        lines.append(
            f"| {width}s | {part.height} | {part.filter(pl.col('stress_net_pnl') > 0).height} | "
            f"{stable.height} | {stable['bucket_start'].n_unique()} |"
        )
    lines += [
        "",
        "## Retrospective aggregate feasibility sketches",
        "",
        "These sketches select signatures and replay them on the same historical evidence. They are intentionally optimistic and are not tournament results.",
        "",
        "| Granularity | Bucket signatures | Trades | PnL | Stress PnL | PF | W/L | UP/DOWN | Avg entry | Date range |",
        "|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|",
    ]
    for row in aggregates.iter_rows(named=True):
        lines.append(
            f"| {row['bucket_width']}s | {row['selected_bucket_signatures']} | {row['trades']} | "
            f"{row['net_pnl']:.2f} | {row['stress_net_pnl']:.2f} | "
            f"{_format_metric(row['profit_factor'], 3)} | {row['wins']}/{row['losses']} | "
            f"{row['up_trades']}/{row['down_trades']} | {row['average_entry_second']:.1f} | "
            f"{row['range_start']} – {row['range_end']} |"
        )
    lines += [
        "",
        "## Selected screened historical signature per 10-second bucket",
        "",
        "| Seconds | Model signature | Trades | PnL | Stress | PF | W/L | UP/DOWN | Positive blocks | Positive months | Range |",
        "|---:|---|---:|---:|---:|---:|---:|---:|---:|---:|---|",
    ]
    selected_10 = {
        row["bucket_start"]: row
        for row in selections.filter(pl.col("bucket_width") == 10)
        .sort("bucket_start")
        .iter_rows(named=True)
    }
    for bucket_start in range(0, 240, 10):
        row = selected_10.get(bucket_start)
        if row is None:
            lines.append(
                f"| {bucket_start}-{bucket_start + 9} | — | 0 | — | — | — | — | — | — | — | No signature passed the descriptive screen |"
            )
            continue
        lines.append(
            f"| {row['bucket_start']}-{row['bucket_end']} | `{row['signature_id']}` | "
            f"{row['trades']} | {row['net_pnl']:.2f} | {row['stress_net_pnl']:.2f} | "
            f"{_format_metric(row['profit_factor'], 3)} | {row['wins']}/{row['losses']} | "
            f"{row['up_trades']}/{row['down_trades']} | "
            f"{row['positive_evidence_blocks']}/{row['evidence_blocks']} | "
            f"{row['positive_calendar_months']}/{row['calendar_months']} | "
            f"{row['range_start']} – {row['range_end']} |"
        )
    lines += [
        "",
        "## Included latest bucket-screening results",
        "",
        "The September fixed-VWAP5 screening artifact is included as supporting evidence, not mislabeled as an aggregate tournament.",
        "",
        "| Variant | Assigned bucket | RTDS | Period | Trades | PnL | Stress | PF | W/L |",
        "|---|---|---|---|---:|---:|---:|---:|---:|",
    ]
    latest_display = latest.filter(
        pl.col("period").is_in(["sealed", "confirmation"]) & (pl.col("trades") > 0)
    ).sort(["period", "stress_net_pnl"], descending=[False, True])
    for row in latest_display.iter_rows(named=True):
        lines.append(
            f"| `{row['variant']}` | {row['assigned_bucket']} | {row['rtds_mode']} | "
            f"{row['period']} | {row['trades']} | {row['net_pnl']:.2f} | "
            f"{row['stress_net_pnl']:.2f} | {_format_metric(row['profit_factor'], 3)} | "
            f"{row['wins']}/{row['losses']} |"
        )
    lines += [
        "",
        "## Interpretation limits",
        "",
        "- Signatures preserve archived outputs, but some archives contain only selected trades rather than every prediction timestamp.",
        "- Evidence windows differ across historical runs. Cross-signature PnL is descriptive and not a common-window tournament comparison.",
        "- Every replayed trade is standardized to five shares. When only an archived entry price is present, Q5 PnL is reconstructed at that archived price; this removes archived quantity scaling but cannot recreate a missing VWAP5 book snapshot.",
        "- The aggregate sketches select and evaluate on the same historical evidence; they demonstrate mechanical feasibility only.",
        "- Multiple archived copies of the same relative artifact are deduplicated, preferring the current worktree copy.",
        "- Missing direction, timestamp, or auditable five-share economics causes a ledger to remain inventoried but excluded from PnL replay.",
    ]
    return "\n".join(lines) + "\n"


def run_replay(local_root: Path, archive_root: Path, output: Path) -> Path:
    files, inventory = _canonical_files(local_root, archive_root)
    training_runs = {(path.parts[0], path.parts[1]) for path in files if len(path.parts) >= 2}
    candidate_paths = {
        relative: path for relative, path in files.items() if _looks_like_trade_ledger(relative)
    }
    frames: list[pl.DataFrame] = []
    unavailable: list[dict[str, str]] = []
    for index, (relative, path) in enumerate(sorted(candidate_paths.items()), start=1):
        print(f"ledger {index}/{len(candidate_paths)}: {relative}", flush=True)
        try:
            frame, reason = _normalize_ledger(path, relative)
        except (OSError, TypeError, ValueError, pl.exceptions.PolarsError) as error:
            frame, reason = None, f"read error: {type(error).__name__}: {error}"
        if frame is None:
            unavailable.append({"artifact": relative.as_posix(), "reason": reason or "unknown"})
        else:
            frames.append(frame)
    if not frames:
        raise RuntimeError("no replayable historical trade ledgers found")
    trades = pl.concat(frames, how="diagonal_relaxed", rechunk=True)
    trades = trades.with_columns(
        pl.concat_str("tournament", "run_id", "candidate", separator="/").alias("signature_id")
    ).unique(
        subset=["signature_id", "market_id", "window_start"],
        keep="first",
        maintain_order=True,
    )
    results = _bucket_results(trades)
    aggregates, selections = _retrospective_aggregates(trades, results)
    latest = _latest_bucketed_summary(local_root)
    inventory.update(
        {
            "schema_version": "btc-historical-signature-replay-inventory-v1",
            "created_at": datetime.now(UTC).isoformat(),
            "read_only": True,
            "models_trained": 0,
            "database_mutations": False,
            "training_runs": len(training_runs),
            "candidate_ledgers": len(candidate_paths),
            "replayable_ledgers": len(frames),
            "unavailable_ledgers": len(unavailable),
            "replayable_signatures": trades["signature_id"].n_unique(),
            "normalized_trade_rows": trades.height,
            "unavailable": unavailable,
        }
    )
    output.mkdir(parents=True, exist_ok=True)
    trades.write_parquet(output / "normalized-trades.parquet", compression="zstd", statistics=True)
    results.write_parquet(output / "bucket-results.parquet", compression="zstd", statistics=True)
    _screened(results).write_parquet(
        output / "screened-signatures.parquet", compression="zstd", statistics=True
    )
    aggregates.write_parquet(
        output / "retrospective-aggregate-sketches.parquet", compression="zstd", statistics=True
    )
    selections.write_parquet(
        output / "retrospective-aggregate-selections.parquet", compression="zstd", statistics=True
    )
    if not latest.is_empty():
        latest.write_parquet(
            output / "latest-bucket-screening-summary.parquet",
            compression="zstd",
            statistics=True,
        )
    _write_json(output / "inventory.json", inventory)
    (output / "report.md").write_text(_report(inventory, results, aggregates, selections, latest))
    _write_json(
        output / "completion.json",
        {
            "complete": True,
            "created_at": inventory["created_at"],
            "models_trained": 0,
            "replayable_signatures": inventory["replayable_signatures"],
            "normalized_trade_rows": inventory["normalized_trade_rows"],
        },
    )
    return output


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--local-root", type=Path, default=Path("training-results"))
    parser.add_argument(
        "--archive-root",
        type=Path,
        default=Path("/Volumes/docker-data/polymarket-bot/model-training-archive"),
    )
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    print(run_replay(args.local_root, args.archive_root, args.output))


if __name__ == "__main__":
    main()
