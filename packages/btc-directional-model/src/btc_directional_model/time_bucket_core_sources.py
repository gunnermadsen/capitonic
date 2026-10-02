"""Validate frozen offline tick and CLOB archives without discovering new inputs."""

from __future__ import annotations

import argparse
import json
import math
import resource
import sys
import time
from collections import defaultdict
from datetime import UTC, date, datetime, timedelta
from functools import lru_cache
from pathlib import Path

import polars as pl

from .fok_retry_backtest import parse_asks
from .time_bucket_source_audit import checked_run, sha256, write_json

TICKS = "rtds_reference_price_ticks"
BOOKS = "polymarket_clob_l2"
KEYS = {
    TICKS: ["source", "dedup_key"],
    BOOKS: [
        "source", "market_id", "token_id", "connection_epoch", "ingest_sequence", "sampled_at",
    ],
}
TIMESTAMPS = {
    TICKS: ["source_timestamp", "envelope_timestamp", "received_at"],
    BOOKS: ["sampled_at", "source_timestamp", "provider_available_at", "received_at"],
}
DAY_COLUMN = {TICKS: "source_timestamp", BOOKS: "sampled_at"}
SCHEMA_VERSION = "btc-time-bucket-core-source-validation-v1"


class SourceConflict(ValueError):
    """Native identity cannot be reconciled without changing source evidence."""

    def __init__(self, message: str, facts: dict):
        super().__init__(message)
        self.facts = facts


@lru_cache(maxsize=4096)
def _level_facts(raw: str | None) -> tuple[str, float | None, float | None]:
    """Validate before calling the existing parser, which otherwise drops bad levels."""
    try:
        levels = json.loads(raw) if isinstance(raw, str) else raw
        if not isinstance(levels, list):
            return "invalid_json_shape", None, None
        if not levels:
            return "empty", None, None
        for level in levels:
            if not isinstance(level, (list, tuple)) or len(level) != 2:
                return "invalid_json_shape", None, None
            price, size = float(level[0]), float(level[1])
            if not (math.isfinite(price) and math.isfinite(size) and 0 < price < 1 and size > 0):
                return "invalid_level", None, None
        parsed = parse_asks(levels)
        return "ok", parsed[0][0], parsed[-1][0]
    except (ValueError, TypeError, IndexError, OverflowError):
        return "invalid_json", None, None


def _book_reason(row: dict) -> str:
    bid_status, _, best_bid = _level_facts(row["bids"])
    ask_status, best_ask, _ = _level_facts(row["asks"])
    if bid_status != "ok":
        return f"bids_{bid_status}"
    if ask_status != "ok":
        return f"asks_{ask_status}"
    if best_bid >= best_ask:
        return "crossed_or_locked"
    for name, actual in (("best_bid", best_bid), ("best_ask", best_ask)):
        stated = row[name]
        if stated is None or not math.isfinite(float(stated)) or not 0 < float(stated) < 1:
            return "invalid_best_quote"
        if abs(float(stated) - actual) > 1e-8:
            return "best_quote_level_mismatch"
    return "ok"


def _tick_partition_conflicts(frame: pl.DataFrame) -> int:
    # This verifies the native key's embedded millisecond, rather than assuming
    # identity groups cannot cross the source-date processing boundary.
    relevant = frame.filter(pl.col("source").is_in(["rtds_chainlink", "direct_binance"]))
    parts = pl.col("dedup_key").str.split_exact(":", 4)
    valid = (
        (parts.struct.field("field_0") == pl.col("source"))
        & (parts.struct.field("field_1") == pl.col("symbol"))
        & (
            parts.struct.field("field_2").cast(pl.Int64, strict=False)
            == pl.col("source_timestamp").dt.epoch("ms")
        )
        & parts.struct.field("field_4").is_not_null()
    )
    return relevant.filter(~valid.fill_null(False)).height


def canonical_core_rows(frame: pl.DataFrame, product: str) -> tuple[pl.DataFrame, dict]:
    """Return eligible native observations and source-quality counts, without imputation."""
    if product not in KEYS:
        raise ValueError(f"Unsupported product: {product}")
    keys = KEYS[product]
    time_columns = TIMESTAMPS[product]
    conflicts = ["price", "source_timestamp"] if product == TICKS else [
        "bids", "asks", "source_timestamp", "outcome",
    ]
    required = set(keys + time_columns + conflicts)
    required |= {"symbol", "integrity_status"} if product == TICKS else {
        "strategy_key", "capture_artifact_id", "best_bid", "best_ask",
    }
    missing = required - set(frame.columns)
    if missing:
        raise ValueError(f"{product}: missing required columns {sorted(missing)}")
    forbidden = {"official_outcome", "label_up", "net_pnl", "stress_pnl"} & set(frame.columns)
    if forbidden:
        raise ValueError(f"Source unexpectedly includes evaluation fields: {sorted(forbidden)}")
    native_groups = (
        frame.group_by(keys)
        .agg(pl.struct(conflicts).n_unique().alias("versions"))
    )
    native_conflicts = native_groups.filter(pl.col("versions") > 1)
    facts = {
        "raw_rows": frame.height,
        "unique_rows": native_groups.height,
        "duplicate_rows": frame.height - native_groups.height,
        "native_keys": keys,
        "identity_conflicts": native_conflicts.height,
        "timestamp_partition_conflicts": 0,
        "conflict_examples": native_conflicts.head(5).to_dicts(),
    }
    if product == TICKS:
        facts["timestamp_partition_conflicts"] = _tick_partition_conflicts(frame)
    if facts["identity_conflicts"] or facts["timestamp_partition_conflicts"]:
        raise SourceConflict(f"{product}: conflicting native identities or timestamp keys", facts)

    nonnull_keys = pl.all_horizontal(pl.col(name).is_not_null() for name in keys)
    if product == TICKS:
        price = pl.col("price").cast(pl.Float64)
        reason = (
            pl.when(~nonnull_keys).then(pl.lit("missing_native_identity"))
            .when(~pl.col("source").is_in(["rtds_chainlink", "direct_binance"]).fill_null(False))
            .then(pl.lit("unsupported_source"))
            .when(~(pl.col("symbol") == "BTCUSD").fill_null(False))
            .then(pl.lit("unsupported_symbol"))
            .when(~(pl.col("integrity_status") == "ok").fill_null(False))
            .then(pl.lit("integrity_not_ok"))
            .when(pl.col("source_timestamp").is_null() | pl.col("received_at").is_null())
            .then(pl.lit("missing_observation_time"))
            .when(~(price.is_finite() & (price > 0)).fill_null(False))
            .then(pl.lit("invalid_price"))
            .otherwise(pl.lit("ok"))
        )
    else:
        reason = (
            pl.when(~nonnull_keys).then(pl.lit("missing_native_identity"))
            .when(~(pl.col("source") == "polymarket_clob_market").fill_null(False))
            .then(pl.lit("unsupported_source"))
            .when(~(pl.col("strategy_key") == "polymarket_btc_five_minute_orderbooks").fill_null(False))
            .then(pl.lit("not_live_book_strategy"))
            .when(~pl.col("outcome").is_in(["up", "down"]).fill_null(False))
            .then(pl.lit("invalid_token_outcome"))
            .when(pl.any_horizontal(pl.col(name).is_null() for name in time_columns))
            .then(pl.lit("missing_observation_time"))
            .otherwise(pl.lit("ok"))
        )
    frame = frame.with_columns(
        pl.max_horizontal(time_columns).alias("available_at"), reason.alias("_reason")
    )
    if product == BOOKS:
        # Validate each distinct raw ladder pair once, retaining its original JSON.
        candidates = frame.filter(pl.col("_reason") == "ok")
        columns = ["bids", "asks", "best_bid", "best_ask"]
        pairs = candidates.select(columns).unique().with_columns(
            pl.struct(columns).map_elements(_book_reason, return_dtype=pl.String).alias("_book_reason")
        )
        frame = frame.join(pairs, on=columns, how="left", nulls_equal=True).with_columns(
            pl.when(pl.col("_reason") == "ok").then(pl.col("_book_reason"))
            .otherwise(pl.col("_reason")).alias("_reason")
        ).drop("_book_reason")
    # Null categorical evidence never satisfies a mandatory condition.
    frame = frame.with_columns(pl.col("_reason").fill_null("missing_required_evidence"))
    invalid = frame.filter(pl.col("_reason") != "ok")
    deduped = frame.with_columns((pl.col("_reason") == "ok").alias("_eligible")).sort(
        ["_eligible", "available_at"], descending=[True, False], nulls_last=True
    ).unique(keys, keep="first", maintain_order=True)
    eligible = deduped.filter(pl.col("_eligible")).drop("_reason", "_eligible")
    facts.update(
        unique_rows=deduped.height,
        duplicate_rows=frame.height - deduped.height,
        invalid_raw_rows=invalid.height,
        invalid_unique_rows=deduped.height - eligible.height,
        eligible_rows=eligible.height,
        invalid_reasons={r["_reason"]: r["len"] for r in invalid.group_by("_reason").len().to_dicts()},
        invalid_examples=invalid.select(*keys, "_reason").head(10).to_dicts(),
        raw_start=frame[DAY_COLUMN[product]].min(),
        raw_end=frame[DAY_COLUMN[product]].max(),
        source_start=eligible["source_timestamp"].min(),
        source_end=eligible["source_timestamp"].max(),
        available_start=eligible["available_at"].min(),
        available_end=eligible["available_at"].max(),
    )
    if eligible.height:
        latency = (eligible["available_at"] - eligible["source_timestamp"]).dt.total_microseconds() / 1e6
        facts["latency_seconds"] = {
            "min": latency.min(), "median": latency.median(),
            "p99": latency.quantile(0.99), "max": latency.max(),
        }
    facts["hour_counts"] = frame.group_by(
        pl.col(DAY_COLUMN[product]).dt.truncate("1h").alias("hour")
    ).agg(pl.len().alias("raw_rows")).join(
        eligible.group_by(pl.col(DAY_COLUMN[product]).dt.truncate("1h").alias("hour"))
        .agg(pl.len().alias("eligible_rows")),
        on="hour", how="left",
    ).with_columns(pl.col("eligible_rows").fill_null(0)).sort("hour").to_dicts()
    facts["source_counts"] = eligible.group_by("source").len().sort("source").to_dicts()
    if product == BOOKS:
        facts["eligible_without_capture_id"] = eligible["capture_artifact_id"].null_count()
    return eligible.sort("available_at"), facts


def _atomic_manifest(path: Path, value: dict) -> None:
    temporary = path.with_suffix(".json.pending")
    write_json(temporary, value)
    temporary.replace(path)


def _inventory(run: Path, product: str) -> tuple[dict, dict, dict[date, list[dict]]]:
    path = run / "inputs" / f"{product}-inventory.json"
    data = json.loads(path.read_text())
    if data.get("product") != product or not data.get("frozen_at"):
        raise ValueError(f"Missing frozen inventory contract: {path}")
    frozen = datetime.fromisoformat(data["frozen_at"]).timestamp()
    by_day = defaultdict(list)
    state = []
    for row in data["files"]:
        if not row.get("applicable") or not row["rows"]:
            continue
        source = Path(row["path"])
        stat = source.stat()
        if not row.get("sha256") or stat.st_size != row["bytes"] or stat.st_mtime > frozen:
            raise ValueError(f"Frozen source identity is no longer supported: {source}")
        bound = row["bounds"][DAY_COLUMN[product]]
        if bound["min"] is None or bound["max"] is None or bound.get("null_count", 0):
            raise ValueError(f"Incomplete partition timestamp bounds: {source}")
        first = datetime.fromisoformat(bound["min"]).astimezone(UTC).date()
        last = datetime.fromisoformat(bound["max"]).astimezone(UTC).date()
        current = first
        while current <= last:
            by_day[current].append(row)
            current += timedelta(days=1)
        state.append({"path": str(source), "bytes": stat.st_size, "mtime_ns": stat.st_mtime_ns})
    identity = {
        "inventory": str(path), "inventory_sha256": sha256(path),
        "implementation_sha256": sha256(Path(__file__)), "input_file_state": state,
        "native_keys": KEYS[product], "day_column": DAY_COLUMN[product],
        "availability": f"max({','.join(TIMESTAMPS[product])})",
        "duplicate_selection": "earliest demonstrated availability among valid native copies",
        "conflict_policy": "fail on differing native values, including raw JSON representations",
        "age_policy": "no decision-time freshness threshold; non-ok tick integrity excluded",
        "book_live_observation_proof": {
            "required": "polymarket_clob_market + live strategy + original four timestamps + native connection/sequence identity",
            "capture_artifact_id": "nullable metadata, preserved; not an observation-availability requirement",
            "writer_contract": "packages/market-data-ingester/src/strategies/polymarket/orderbook_snapshots.rs:2324-2444 SnapshotFact::new",
            "archive_contract": "polymarket-orderbook-snapshot-parquet-v1",
        } if product == BOOKS else None,
        "input_hash_policy": "reuse frozen SHA-256; verify size and pre-freeze mtime, then unchanged stat",
    }
    return data, identity, by_day


def validate_product(run: Path, product: str, deadline: float) -> dict:
    """Resume only matching source/config evidence and checksum-verified daily outputs."""
    data, identity, by_day = _inventory(run, product)
    manifest_path = run / "manifests" / f"core-source-validation-{product}.json"
    destination = run / "inputs/validated-core" / product
    destination.mkdir(parents=True, exist_ok=True)
    if manifest_path.exists():
        manifest = json.loads(manifest_path.read_text())
        if manifest["identity"] != identity:
            raise ValueError(f"Existing validation source/config evidence differs: {manifest_path}")
        if manifest["status"] == "source_conflict":
            raise ValueError(f"Unresolved recorded source conflict: {manifest_path}")
    else:
        manifest = {
            "schema_version": SCHEMA_VERSION, "product": product, "status": "running",
            "identity": identity, "input_refs": data["files"], "days": {},
            "created_at": datetime.now(UTC).isoformat(),
        }
        _atomic_manifest(manifest_path, manifest)
    for day, records in sorted(by_day.items()):
        if time.monotonic() >= deadline:
            manifest["status"] = "budget_exhausted"
            _atomic_manifest(manifest_path, manifest)
            return manifest
        target = destination / f"{day}.parquet"
        previous = manifest["days"].get(str(day))
        if previous:
            if not target.is_file() or sha256(target) != previous["output"]["sha256"]:
                raise ValueError(f"Existing normalized output changed: {target}")
            print(json.dumps({"product": product, "date": str(day), "status": "verified_resume"}), flush=True)
            continue
        if target.exists():
            raise ValueError(f"Unowned existing output; refusing overwrite: {target}")
        start = datetime.combine(day, datetime.min.time(), UTC)
        end = start + timedelta(days=1)
        frames = []
        observed_stats = {}
        for record in records:
            path = Path(record["path"])
            stat = path.stat()
            observed_stats[path] = (stat.st_size, stat.st_mtime_ns)
            source = pl.scan_parquet(path).filter(
                (pl.col(DAY_COLUMN[product]) >= start) & (pl.col(DAY_COLUMN[product]) < end)
            )
            if product == TICKS:
                source = source.select(pl.exclude("raw_payload"))
            frames.append(source.with_columns(pl.lit(str(path)).alias("input_path")).collect())
        frame = pl.concat(frames, how="diagonal_relaxed")
        try:
            eligible, facts = canonical_core_rows(frame, product)
        except SourceConflict as error:
            manifest.update(status="source_conflict", failure={"date": str(day), **error.facts})
            _atomic_manifest(manifest_path, manifest)
            raise
        for path, before in observed_stats.items():
            after = path.stat()
            if before != (after.st_size, after.st_mtime_ns):
                raise ValueError(f"Input changed during validation: {path}")
        temporary = target.with_suffix(".parquet.pending")
        eligible.write_parquet(temporary, compression="zstd", statistics=True)
        digest = sha256(temporary)
        temporary.rename(target)
        manifest["days"][str(day)] = {
            **facts, "input_paths": [r["path"] for r in records],
            "output": {"path": str(target), "sha256": digest, "rows": eligible.height},
        }
        peak = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
        manifest["peak_resident_bytes"] = peak if sys.platform == "darwin" else peak * 1024
        if manifest["peak_resident_bytes"] > 4 * 1024**3:
            manifest["status"] = "memory_budget_exceeded"
            _atomic_manifest(manifest_path, manifest)
            return manifest
        manifest["status"] = "running"
        manifest["updated_at"] = datetime.now(UTC).isoformat()
        _atomic_manifest(manifest_path, manifest)
        print(json.dumps({"product": product, "date": str(day), **{
            name: facts[name] for name in ["raw_rows", "duplicate_rows", "invalid_raw_rows", "eligible_rows"]
        }}), flush=True)
        del frames, frame, eligible
        _level_facts.cache_clear()
    manifest["totals"] = {
        name: sum(day[name] for day in manifest["days"].values())
        for name in ["raw_rows", "unique_rows", "duplicate_rows", "identity_conflicts",
                     "timestamp_partition_conflicts", "invalid_raw_rows", "invalid_unique_rows", "eligible_rows"]
    }
    manifest["status"] = "complete"
    manifest["date_start"] = str(min(by_day)) if by_day else None
    manifest["date_end"] = str(max(by_day)) if by_day else None
    manifest["updated_at"] = datetime.now(UTC).isoformat()
    _atomic_manifest(manifest_path, manifest)
    return manifest


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("run", type=Path)
    parser.add_argument("--product", choices=list(KEYS), action="append")
    parser.add_argument("--wall-seconds", type=int, default=3600)
    args = parser.parse_args()
    if not 1 <= args.wall_seconds <= 3600:
        parser.error("wall-seconds must be between 1 and the 3600-second source-audit budget")
    run = checked_run(args.run)
    deadline = time.monotonic() + args.wall_seconds
    for product in args.product or KEYS:
        result = validate_product(run, product, deadline)
        if result["status"] != "complete":
            raise SystemExit(f"Source validation stopped: {result['status']}")


if __name__ == "__main__":
    main()
