"""Validate the frozen optional flow sources without changing evaluation semantics.

Run with POLARS_MAX_THREADS=2 and OPENBLAS_NUM_THREADS=1. Source archives are
never discovered or rehashed here. Only one economic UTC day is held at a time.
Freshness and feature joins belong to the tournament, not this source validator.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from collections import defaultdict
from datetime import UTC, date, datetime, timedelta
from decimal import Decimal, InvalidOperation
from pathlib import Path

import polars as pl
import pyarrow.parquet as pq

from .time_bucket_source_audit import checked_run, sha256

VERSION = "btc-time-bucket-flow-source-validation-v1"
MAX_DAY_BYTES = 512 * 1024 * 1024
UTC_TIME = pl.Datetime("us", "UTC")
PRODUCTS = {
    "binance_spot_one_second_ohlcv": ("binance_spot_one_second_ohlcv",),
    "binance_spot_aggregate_trades": ("binance_spot_aggregate_trades",),
    "binance_spot_l2_snapshots": ("binance_spot_l2_snapshots",),
    "binance_spot_l2_features": (
        "binance_spot_l2_features_canonical",
        "binance_spot_l2_features_verified",
    ),
    "binance_futures_l2_features": (
        "binance_futures_l2_features_canonical",
        "binance_futures_l2_features_verified",
    ),
    "binance_futures_open_interest": ("binance_futures_open_interest",),
}
L2_VALUES = (
    ["midpoint", "microprice", "spread_bps"]
    + [f"{side}_depth_{depth}" for depth in (5, 10, 20) for side in ("bid", "ask")]
    + [f"imbalance_{depth}" for depth in (5, 10, 20)]
    + [
        f"{side}_{field}"
        for side in ("bid", "ask")
        for field in (
            "depth_slope_20",
            "depth_concentration_20",
            "quote_replenishment_1s",
            "quote_churn_1s",
        )
    ]
    + [
        f"{field}_{h}s"
        for h in (1, 5, 15, 30, 60)
        for field in (
            "midpoint_change_bps",
            "spread_bps_delta",
            "depth_20_change_bps",
            "imbalance_20_delta",
        )
    ]
)
CANDLE_VALUES = [
    "open_price",
    "high_price",
    "low_price",
    "close_price",
    "base_volume",
    "quote_volume",
    "taker_buy_base_volume",
    "taker_buy_quote_volume",
]
TIME_COLUMNS = {
    "open_timestamp",
    "close_timestamp",
    "trade_timestamp",
    "source_timestamp",
    "second_start",
    "source_event_timestamp",
    "provider_received_at",
    "received_at",
    "provider_available_at",
    "available_at",
    "ingested_at",
}


def _spec(product: str) -> tuple[str, list[str], list[str], list[str], list[str]]:
    if product not in PRODUCTS:
        raise ValueError(f"Unsupported frozen flow product: {product}")
    if product.endswith("one_second_ohlcv"):
        return (
            "open_timestamp",
            ["source", "symbol", "open_timestamp"],
            CANDLE_VALUES,
            ["trade_count"],
            ["close_timestamp", "provider_available_at", "received_at"],
        )
    if product.endswith("aggregate_trades"):
        return (
            "trade_timestamp",
            ["source", "symbol", "aggregate_trade_id"],
            ["price", "quantity"],
            ["aggregate_trade_id", "first_trade_id", "last_trade_id"],
            ["provider_available_at", "received_at", "buyer_maker", "best_match"],
        )
    if product.endswith("l2_snapshots"):
        return (
            "source_timestamp",
            ["source", "symbol", "source_timestamp", "source_update_id", "sampling_policy_sha256"],
            [],
            ["source_update_id", "sample_depth"],
            ["received_at", "bids", "asks", "sampling_policy", "connection_epoch"],
        )
    if product.endswith("l2_features"):
        return (
            "second_start",
            ["symbol", "second_start"],
            L2_VALUES,
            ["source_update_id"],
            [
                "source_event_timestamp",
                "provider_received_at",
                "available_at",
                "feature_schema_version",
                "quality_status",
                "artifact_id",
            ],
        )
    return (
        "source_timestamp",
        ["source", "symbol", "source_timestamp", "period_seconds"],
        ["sum_open_interest", "sum_open_interest_value"],
        ["period_seconds"],
        ["provider_available_at", "received_at"],
    )


def _time(expr: pl.Expr, dtype: pl.DataType) -> pl.Expr:
    if dtype == pl.String:
        # Both native drains (+00) and Arrow-exported ISO UTC timestamps are supported.
        return pl.coalesce(
            expr.str.to_datetime("%Y-%m-%d %H:%M:%S%.f%#z", time_unit="us", strict=False),
            expr.str.to_datetime("%Y-%m-%dT%H:%M:%S%.f%#z", time_unit="us", strict=False),
        ).cast(UTC_TIME)
    if isinstance(dtype, pl.Datetime) and dtype.time_zone is None:
        raise ValueError("Naive timestamps do not prove UTC source availability")
    if dtype != pl.Null and not isinstance(dtype, pl.Datetime):
        raise ValueError(f"Unexpected native timestamp type: {dtype}")
    return expr.cast(UTC_TIME)


def _times(frame: pl.DataFrame) -> pl.DataFrame:
    return frame.with_columns(
        *(
            _time(pl.col(n), dtype).alias(n)
            for n, dtype in frame.schema.items()
            if n in TIME_COLUMNS
        )
    )


def _decimal_text(value: Decimal) -> str:
    text = format(value, "f")
    return text.rstrip("0").rstrip(".") if "." in text else text


def _snapshot(value: dict) -> dict:
    """Keep all original price levels; normalize decimal spelling for conflict checks."""
    try:
        books = [json.loads(value[side], parse_float=Decimal) for side in ("bids", "asks")]
        depth = int(value["sample_depth"])
        canonical = []
        for side, levels in zip(("bids", "asks"), books, strict=True):
            if not isinstance(levels, list) or len(levels) != depth or depth <= 0:
                raise ValueError("Incomplete sampled book")
            parsed = []
            for level in levels:
                if len(level) != 2:
                    raise ValueError("Malformed level")
                price, quantity = (Decimal(str(x)) for x in level)
                if not price.is_finite() or not quantity.is_finite() or price <= 0 or quantity <= 0:
                    raise ValueError("Invalid level")
                parsed.append((price, quantity))
            prices = [p for p, _ in parsed]
            if prices != sorted(set(prices), reverse=(side == "bids")):
                raise ValueError("Unordered or duplicate price levels")
            canonical.append([[_decimal_text(p), _decimal_text(q)] for p, q in parsed])
        if Decimal(canonical[0][0][0]) >= Decimal(canonical[1][0][0]):
            raise ValueError("Crossed book")
        policy = json.loads(value["sampling_policy"])
        encoded = json.dumps(policy, sort_keys=True, separators=(",", ":")).encode()
        if hashlib.sha256(encoded).hexdigest() != value["sampling_policy_sha256"]:
            raise ValueError("Sampling policy hash conflict")
        if policy.get("sample_depth") != depth or policy.get("symbol") != "BTCUSDT":
            raise ValueError("Wrong sampling policy")
        return {
            "bids": json.dumps(canonical[0], separators=(",", ":")),
            "asks": json.dumps(canonical[1], separators=(",", ":")),
            "sane": True,
        }
    except (ValueError, TypeError, KeyError, InvalidOperation, OverflowError):
        return {"bids": value.get("bids"), "asks": value.get("asks"), "sane": False}


def canonical_rows(frame: pl.DataFrame, product: str) -> tuple[pl.DataFrame, dict]:
    """Normalize a single native UTC day, reject conflicts, retain demonstrated receipts."""
    coordinate, keys, numbers, integers, extra = _spec(product)
    required = set(keys + numbers + integers + extra + [coordinate])
    missing = required - set(frame.columns)
    if missing:
        raise ValueError(f"{product}: missing native columns {sorted(missing)}")
    frame = _times(frame)
    if frame[coordinate].null_count():
        raise ValueError(f"{product}: missing/unparseable native event timestamp")
    if frame[coordinate].dt.date().n_unique() > 1:
        raise ValueError("Row validation must be bounded to one native UTC day")
    extra_numbers = [n for n in ["cmc_circulating_supply"] if n in frame.columns]
    scale = 18 if product.endswith("open_interest") else 10
    optional_invalid = pl.lit(False)
    for n in extra_numbers:
        optional_invalid |= (
            pl.col(n).is_not_null() & pl.col(n).cast(pl.Decimal(38, scale), strict=False).is_null()
        )
    frame = frame.with_columns(optional_invalid.alias("_invalid_optional_number"))
    frame = frame.with_columns(
        *(pl.col(n).cast(pl.Decimal(38, scale), strict=False) for n in numbers + extra_numbers),
        # String conversion prevents truncating fractional floating point identifiers/counts.
        *(pl.col(n).cast(pl.String).cast(pl.Int64, strict=False) for n in integers),
    )
    if "_frozen_file_index" not in frame.columns:
        frame = frame.with_columns(pl.lit(0, dtype=pl.UInt32).alias("_frozen_file_index"))
    native = pl.all_horizontal(pl.col(n).is_not_null() for n in keys)
    sane = native & (pl.col("symbol") == "BTCUSDT") & ~pl.col("_invalid_optional_number")
    sane &= pl.all_horizontal(pl.col(n).is_not_null() for n in numbers + integers)
    if numbers:
        sane &= pl.all_horizontal(pl.col(n).cast(pl.Float64).is_finite() for n in numbers)
    sane &= pl.all_horizontal(pl.col(n) >= 0 for n in integers)
    factual = list(dict.fromkeys(numbers + integers + [coordinate]))
    if "source" in frame.columns:
        expected = "binance_usd_m_futures" if product.endswith("open_interest") else "binance_spot"
        sane &= pl.col("source") == expected
    if product.endswith("one_second_ohlcv"):
        sane &= pl.all_horizontal(pl.col(n) > 0 for n in CANDLE_VALUES[:4])
        sane &= pl.all_horizontal(pl.col(n) >= 0 for n in CANDLE_VALUES[4:])
        sane &= pl.col("open_timestamp") == pl.col("open_timestamp").dt.truncate("1s")
        sane &= pl.col("close_timestamp") == pl.col("open_timestamp") + pl.duration(
            milliseconds=999
        )
        sane &= pl.col("high_price") >= pl.max_horizontal("open_price", "close_price")
        sane &= pl.col("low_price") <= pl.min_horizontal("open_price", "close_price")
        sane &= pl.col("high_price") >= pl.col("low_price")
        sane &= pl.col("taker_buy_base_volume") <= pl.col("base_volume")
        sane &= pl.col("taker_buy_quote_volume") <= pl.col("quote_volume")
        sane &= (pl.col("base_volume") == 0) == (pl.col("quote_volume") == 0)
        sane &= (pl.col("trade_count") == 0) == (pl.col("base_volume") == 0)
        sane &= pl.col("provider_available_at").is_null() | (
            pl.col("provider_available_at") >= pl.col("close_timestamp")
        )
        proven = pl.col("provider_available_at").is_not_null() & pl.col("received_at").is_not_null()
        available = pl.max_horizontal(
            "close_timestamp",
            "provider_available_at",
            "received_at",
            pl.col("open_timestamp") + pl.duration(seconds=1),
        )
        factual += ["close_timestamp"]
    elif product.endswith("aggregate_trades"):
        frame = frame.with_columns(
            *(
                pl.col(n)
                .cast(pl.String)
                .str.to_lowercase()
                .replace_strict(
                    {"true": True, "false": False},
                    default=None,
                    return_dtype=pl.Boolean,
                )
                .alias(n)
                for n in ["buyer_maker", "best_match"]
            )
        )
        sane &= (pl.col("price") > 0) & (pl.col("quantity") >= 0)
        sane &= pl.col("last_trade_id") >= pl.col("first_trade_id")
        sane &= pl.col("buyer_maker").is_not_null() & pl.col("best_match").is_not_null()
        sane &= pl.col("provider_available_at").is_null() | (
            pl.col("provider_available_at") >= pl.col("trade_timestamp")
        )
        proven = pl.col("provider_available_at").is_not_null() & pl.col("received_at").is_not_null()
        available = pl.max_horizontal("trade_timestamp", "provider_available_at", "received_at")
        factual += ["buyer_maker", "best_match"]
    elif product.endswith("l2_snapshots"):
        book = pl.struct(
            ["bids", "asks", "sample_depth", "sampling_policy", "sampling_policy_sha256"]
        ).map_elements(
            _snapshot,
            return_dtype=pl.Struct({"bids": pl.String, "asks": pl.String, "sane": pl.Boolean}),
        )
        frame = frame.with_columns(book.alias("_book")).with_columns(
            pl.col("_book").struct.field("bids").alias("bids"),
            pl.col("_book").struct.field("asks").alias("asks"),
        )
        sane &= pl.col("_book").struct.field("sane") & pl.col("connection_epoch").is_not_null()
        proven = pl.col("received_at").is_not_null()
        available = pl.max_horizontal("source_timestamp", "received_at")
        factual += ["bids", "asks"]
    elif product.endswith("l2_features"):
        schema = (
            "binance-spot-btcusdt-l2-one-second-features-v1"
            if product.startswith("binance_spot")
            else "binance-btcusdt-l2-one-second-features-v1"
        )
        sane &= (pl.col("feature_schema_version") == schema) & (
            pl.col("quality_status") == "qualified"
        )
        sane &= (pl.col("midpoint") > 0) & (pl.col("microprice") > 0) & (pl.col("spread_bps") >= 0)
        sane &= pl.col("second_start") == pl.col("second_start").dt.truncate("1s")
        for side in ("bid", "ask"):
            sane &= pl.col(f"{side}_depth_5") > 0
            sane &= pl.col(f"{side}_depth_10") >= pl.col(f"{side}_depth_5")
            sane &= pl.col(f"{side}_depth_20") >= pl.col(f"{side}_depth_10")
            sane &= pl.col(f"{side}_depth_slope_20") >= 0
            sane &= pl.col(f"{side}_depth_concentration_20").is_between(0, 1)
            sane &= pl.col(f"{side}_quote_replenishment_1s") >= 0
            sane &= pl.col(f"{side}_quote_churn_1s") >= 0
        sane &= pl.all_horizontal(pl.col(f"imbalance_{n}").is_between(-1, 1) for n in (5, 10, 20))
        sane &= pl.col("available_at") >= pl.max_horizontal(
            "source_event_timestamp", "provider_received_at"
        )
        sane &= pl.col("available_at").dt.truncate("1s") == pl.col("second_start")
        proven = pl.all_horizontal(
            pl.col(n).is_not_null()
            for n in (
                "source_event_timestamp",
                "provider_received_at",
                "available_at",
                "artifact_id",
            )
        )
        available = pl.max_horizontal(
            "available_at",
            "provider_received_at",
            "source_event_timestamp",
            pl.col("second_start") + pl.duration(seconds=1),
        )
        factual += ["source_event_timestamp", "feature_schema_version"]
        frame = frame.with_columns(pl.col("available_at").alias("stored_available_at"))
    else:
        sane &= pl.col("period_seconds") == 300
        sane &= pl.col("source_timestamp") == pl.col("source_timestamp").dt.truncate("5m")
        sane &= pl.all_horizontal(pl.col(n) >= 0 for n in numbers)
        for n in extra_numbers:
            sane &= pl.col(n).is_null() | (
                pl.col(n).cast(pl.Float64).is_finite() & (pl.col(n) >= 0)
            )
        proven = pl.col("received_at").is_not_null()
        available = pl.max_horizontal("source_timestamp", "received_at", "provider_available_at")
        factual += extra_numbers
    frame = frame.with_columns(
        native.alias("_native"),
        sane.fill_null(False).alias("_sane"),
        proven.fill_null(False).alias("_proven"),
        available.alias("available_at"),
    )
    factual = [n for n in dict.fromkeys(factual) if n not in keys]
    conflicts = (
        frame.filter(pl.col("_native"))
        .group_by(keys)
        .agg(pl.struct(factual).n_unique().alias("variants"))
        .filter(pl.col("variants") > 1)
        .height
    )
    if conflicts:
        raise ValueError(f"{product}: {conflicts} conflicting native identities")
    dedup = (
        frame.filter(pl.col("_native"))
        .sort(
            ["_sane", "_proven", "available_at", "_frozen_file_index"],
            descending=[True, True, False, False],
        )
        .unique(keys, keep="first", maintain_order=True)
    )
    eligible = dedup.filter(pl.col("_sane") & pl.col("_proven"))
    facts = {
        "raw_rows": frame.height,
        "unique_native_rows": dedup.height,
        "duplicate_rows": frame.filter(pl.col("_native")).height - dedup.height,
        "invalid_raw_rows": frame.filter(~pl.col("_sane")).height,
        "unproven_raw_rows": frame.filter(~pl.col("_proven")).height,
        "eligible_rows": eligible.height,
        "identity_conflicts": conflicts,
        "native_keys": keys,
        "delayed_eligible_rows": eligible.filter(
            pl.col("available_at")
            > pl.col(coordinate)
            + pl.duration(seconds=600 if product.endswith("open_interest") else 5)
        ).height,
        "freshness_applied": False,
    }
    if product.endswith("aggregate_trades"):
        ids = frame.filter(pl.col("_native"))["aggregate_trade_id"]
        facts.update(native_id_min=ids.min(), native_id_max=ids.max())
    if coordinate != "source_timestamp":
        eligible = eligible.with_columns(pl.col(coordinate).alias("source_timestamp"))
    result = eligible.drop(
        [
            n
            for n in ["_native", "_sane", "_proven", "_book", "_invalid_optional_number"]
            if n in eligible.columns
        ]
    )
    # Storage/output contracts remain strings; categorical encoding is memory-only.
    encoded = [name for name, dtype in result.schema.items() if dtype == pl.Categorical]
    if encoded:
        result = result.with_columns(pl.col(encoded).cast(pl.String))
    if "_payload_sha256_hex" in result.columns:
        # Filter before casting: arbitrary digest bytes are not valid UTF-8. Preserve
        # non-hex spellings and nulls exactly instead of tightening admission rules.
        result = result.with_row_index("_lineage_restore_order")
        encoded = result.filter("_payload_sha256_hex").with_columns(
            pl.col("payload_sha256").bin.encode("hex")
        )
        original = result.filter(~pl.col("_payload_sha256_hex")).with_columns(
            pl.col("payload_sha256").cast(pl.String)
        )
        result = (
            pl.concat([encoded, original])
            .sort("_lineage_restore_order")
            .drop("_lineage_restore_order", "_payload_sha256_hex")
        )
    return result.with_columns(pl.lit(product).alias("product")).sort("available_at"), facts


def _parse(value: str | datetime) -> datetime:
    result = datetime.fromisoformat(value) if isinstance(value, str) else value
    if result.tzinfo is None:
        raise ValueError("Frozen source timestamp lacks timezone")
    return result.astimezone(UTC)


def frozen_days(files: list[dict], coordinate: str) -> dict[date, list[dict]]:
    days = defaultdict(list)
    for f in files:
        if not f["rows"]:
            continue
        bounds = f.get("timestamp_statistics", {}).get(coordinate, {})
        if not bounds.get("min") or not bounds.get("max"):
            raise ValueError(f"Missing frozen timestamp bounds: {f['path']}")
        day, last = _parse(bounds["min"]).date(), _parse(bounds["max"]).date()
        while day <= last:
            days[day].append(f)
            day += timedelta(days=1)
    return dict(sorted(days.items()))


def compact_trade_lineage(frame: pl.DataFrame) -> pl.DataFrame:
    """Losslessly dictionary-encode repeated capture strings, retaining every value."""
    names = [
        n
        for n in ("source", "symbol", "strategy_key", "capture_artifact_id", "_frozen_group")
        if n in frame.columns
    ]
    frame = frame.with_columns(pl.col(names).cast(pl.Categorical))
    if "payload_sha256" in frame.columns:
        digest = pl.col("payload_sha256")
        is_hex = digest.str.contains(r"^[0-9a-f]{64}$").fill_null(False)
        frame = frame.with_columns(
            is_hex.alias("_payload_sha256_hex"),
            pl.when(is_hex)
            .then(digest.str.decode("hex", strict=False))
            .otherwise(digest.cast(pl.Binary))
            .alias("payload_sha256"),
        )
    return frame


def read_day(files: list[dict], coordinate: str, day: date, frozen_at: datetime) -> pl.DataFrame:
    frames, size = [], 0
    for f in files:
        path = Path(f["path"])
        before = path.stat()
        if before.st_size != f["bytes"] or before.st_mtime > frozen_at.timestamp():
            raise ValueError(f"Frozen source identity changed; refusing rehash: {path}")
        parquet = pq.ParquetFile(path)
        if parquet.metadata.num_rows != f["rows"]:
            raise ValueError(f"Frozen row count changed: {path}")
        index = parquet.schema_arrow.get_field_index(coordinate)
        groups = []
        for i in range(parquet.num_row_groups):
            stats = parquet.metadata.row_group(i).column(index).statistics
            if not stats or not stats.has_min_max or stats.null_count:
                raise ValueError(f"Unassignable event timestamps in {path}, row group {i}")
            if _parse(stats.min).date() <= day <= _parse(stats.max).date():
                groups.append(i)
        columns = [n for n in parquet.schema_arrow.names if n != "source_payload"]
        for batch in parquet.iter_batches(
            batch_size=16_384, row_groups=groups, columns=columns, use_threads=False
        ):
            frame = _times(pl.from_arrow(batch))
            if frame[coordinate].null_count():
                raise ValueError(f"Invalid native timestamp in {path}")
            frame = frame.filter(pl.col(coordinate).dt.date() == day)
            if frame.height:
                frame = frame.with_columns(
                    pl.lit(f["file_index"], dtype=pl.UInt32).alias("_frozen_file_index"),
                    pl.lit(f["group"]).alias("_frozen_group"),
                )
                if coordinate == "trade_timestamp":
                    frame = compact_trade_lineage(frame)
                size += frame.estimated_size()
                if size > MAX_DAY_BYTES:
                    raise MemoryError(f"{day}: bounded day exceeds {MAX_DAY_BYTES} bytes; stop")
                frames.append(frame)
        after = path.stat()
        if (before.st_size, before.st_mtime_ns) != (after.st_size, after.st_mtime_ns):
            raise ValueError(f"Frozen source changed during read: {path}")
    if not frames:
        raise ValueError(f"Frozen bounds yielded no rows on {day}")
    return pl.concat(frames, how="diagonal_relaxed")


def _atomic_json(path: Path, value: dict) -> None:
    temp = path.with_suffix(".json.partial")
    if temp.exists():
        raise ValueError(f"Unfinished manifest requires inspection: {temp}")
    try:
        with temp.open("x") as out:
            out.write(json.dumps(value, indent=2, sort_keys=True, default=str) + "\n")
        temp.replace(path)
    finally:
        temp.unlink(missing_ok=True)


def _parquet(frame: pl.DataFrame, path: Path, evidence: dict) -> None:
    temp = path.with_suffix(".parquet.partial")
    if temp.exists():
        raise ValueError(f"Unfinished output requires inspection: {temp}")
    metadata = {
        b"flow_source_validation": json.dumps(evidence, sort_keys=True, default=str).encode()
    }
    try:
        pq.write_table(
            frame.to_arrow().replace_schema_metadata(metadata),
            temp,
            compression="zstd",
            write_statistics=True,
            row_group_size=65_536,
        )
        temp.replace(path)
    finally:
        temp.unlink(missing_ok=True)


def _evidence(path: Path) -> dict:
    raw = (pq.ParquetFile(path).schema_arrow.metadata or {}).get(b"flow_source_validation")
    if raw is None:
        raise ValueError(f"Output lacks validator evidence: {path}")
    return json.loads(raw)


def _coverage(
    raw: pl.DataFrame, eligible: pl.DataFrame, coordinate: str, product: str
) -> list[dict]:
    counts = raw.group_by(pl.col(coordinate).dt.truncate("1h").alias("hour")).len(name="raw_rows")
    good = eligible.group_by(pl.col("source_timestamp").dt.truncate("1h").alias("hour")).len(
        name="eligible_rows"
    )
    return (
        counts.join(good, on="hour", how="left")
        .with_columns(
            pl.col("eligible_rows").fill_null(0),
            pl.lit(product).alias("product"),
        )
        .sort("hour")
        .to_dicts()
    )


def _hourly(run: Path, binding: str) -> None:
    rows = []
    for product in PRODUCTS:
        path = run / "manifests" / f"flow-source-validation-{product}.json"
        if not path.exists():
            continue
        report = json.loads(path.read_text())
        if report["binding"] != binding:
            raise ValueError(f"Incompatible flow validation evidence: {path}")
        for day in report["daily"]:
            rows.extend(day["coverage"])
    if not rows:
        return
    frame = (
        pl.DataFrame(rows)
        .with_columns(
            pl.col("hour").str.to_datetime(time_zone="UTC", strict=True).cast(UTC_TIME),
            pl.col("raw_rows", "eligible_rows").cast(pl.UInt64),
        )
        .sort("product", "hour")
    )
    path = run / "metrics/flow-source-hourly-coverage.parquet"
    path.parent.mkdir(exist_ok=True)
    if path.exists() and _evidence(path).get("binding") != binding:
        raise ValueError("Existing hourly evidence belongs to another validation contract")
    _parquet(frame, path, {"binding": binding, "row_counts_do_not_prove_continuity": True})


def accepted_binding(report: dict, current: str, implementation: dict) -> str:
    """Accept a different implementation only with explicit frozen-input parity evidence."""
    if report.get("binding") == current:
        return current
    for proof in report.get("implementation_compatibility", []):
        if (
            proof.get("kind") == "lossless_trade_lineage_encoding"
            and proof.get("preserved_binding") == report.get("binding")
            and proof.get("semantics_unchanged") is True
            and all(proof.get(k) == v for k, v in implementation.items())
            and proof.get("previous_validator_sha256")
            and proof.get("focused_tests", {}).get("status") == "passed"
        ):
            return report["binding"]
    raise ValueError("Existing validation manifest is incompatible; no evidence overwritten")


def audit_product(run: Path, product: str) -> dict:
    """Execute only after the caller has acquired the tournament's heavy-I/O slot."""
    _spec(product)
    freeze_bytes = (run / "inputs/optional-source-freeze.json").read_bytes()
    contract_bytes = (run / "manifests/optional-source-causal-contract.json").read_bytes()
    freeze, contract = json.loads(freeze_bytes), json.loads(contract_bytes)
    if not freeze.get("complete") or not contract.get("classification_complete"):
        raise ValueError("Optional source freeze/causal classifications are incomplete")
    # Manifest and maintained-code fingerprints, not a second hashing of source archives.
    code_bytes = Path(__file__).read_bytes()
    implementation = {
        "validator_sha256": hashlib.sha256(code_bytes).hexdigest(),
        "source_freeze_sha256": hashlib.sha256(freeze_bytes).hexdigest(),
        "causal_contract_sha256": hashlib.sha256(contract_bytes).hexdigest(),
    }
    binding = hashlib.sha256(freeze_bytes + contract_bytes + code_bytes).hexdigest()
    groups = {g["product_key"]: g for g in freeze["groups"]}
    files = []
    for group in PRODUCTS[product]:
        for f in groups[group]["files"]:
            if not f.get("frozen") or not f.get("sha256"):
                raise ValueError(f"Unfrozen optional source: {f['path']}")
            files.append(dict(f, file_index=len(files), group=group))
    coordinate = _spec(product)[0]
    days = frozen_days(files, coordinate)
    report_path = run / "manifests" / f"flow-source-validation-{product}.json"
    report = {
        "version": VERSION,
        "binding": binding,
        "product": product,
        "status": "running",
        "freeze_time": freeze["frozen_at"],
        "native_coordinate": coordinate,
        "source_files": files,
        "daily": [],
        "freshness_applied": False,
        "excluded_or_external_contracts": [
            {"product_key": g["product_key"], "classification": g["classification"]}
            for g in contract["products"]
            if g["product_key"] not in set(sum(PRODUCTS.values(), ()))
        ],
        "coverage_interpretation": "Counts do not establish capture continuity or zero activity.",
    }
    if report_path.exists():
        report = json.loads(report_path.read_text())
        if report.get("product") != product:
            raise ValueError("Existing validation manifest belongs to another product")
        binding = accepted_binding(report, binding, implementation)
        if report.get("error"):
            previous = {
                "error": report["error"],
                "binding": report["binding"],
                "completed_days": len(report["daily"]),
            }
            failures = report.setdefault("prior_failures", [])
            if previous not in failures:
                failures.append(previous)
    report["executing_implementation"] = implementation
    destination = run / "inputs/validated-flow" / product
    destination.mkdir(parents=True, exist_ok=True)
    previous_max = None
    try:
        for day, day_files in days.items():
            target = destination / f"{day}.parquet"
            old = next((d for d in report["daily"] if d["date"] == str(day)), None)
            if target.exists():
                evidence = _evidence(target)
                if (
                    evidence.get("binding") != binding
                    or evidence.get("product") != product
                    or evidence.get("date") != str(day)
                ):
                    raise ValueError(f"Incompatible preexisting output: {target}")
                digest = sha256(target)
                if old and old["output_sha256"] != digest:
                    raise ValueError(f"Validated output changed: {target}")
                if pq.ParquetFile(target).metadata.num_rows != evidence["facts"]["eligible_rows"]:
                    raise ValueError(f"Validated output row count changed: {target}")
                record = {**evidence, "output_path": str(target), "output_sha256": digest}
            else:
                if old:
                    raise ValueError(f"Recorded immutable output is missing: {target}")
                raw = read_day(day_files, coordinate, day, _parse(freeze["frozen_at"]))
                eligible, facts = canonical_rows(raw, product)
                evidence = {
                    "binding": binding,
                    "product": product,
                    "date": str(day),
                    "source_file_indices": [f["file_index"] for f in day_files],
                    "producing_validator_sha256": implementation["validator_sha256"],
                    "facts": facts,
                    "coverage": _coverage(raw, eligible, coordinate, product),
                }
                if (
                    product.endswith("aggregate_trades")
                    and previous_max is not None
                    and facts["native_id_min"] is not None
                    and facts["native_id_min"] <= previous_max
                ):
                    raise ValueError(
                        "Aggregate trade native-ID ranges overlap/regress across UTC days"
                    )
                _parquet(eligible, target, evidence)
                record = {**evidence, "output_path": str(target), "output_sha256": sha256(target)}
                del raw, eligible
            if product.endswith("aggregate_trades"):
                low, high = record["facts"]["native_id_min"], record["facts"]["native_id_max"]
                if previous_max is not None and low is not None and low <= previous_max:
                    raise ValueError(
                        "Aggregate trade native-ID ranges overlap/regress across UTC days"
                    )
                previous_max = high if high is not None else previous_max
            if old is None:
                # A fully published compatible Parquet footer recovers an interrupted manifest write.
                report["daily"].append(json.loads(json.dumps(record, default=str)))
            report.update(status="running", error=None)
            _atomic_json(report_path, report)
            print(json.dumps({"product": product, "date": str(day), **record["facts"]}), flush=True)
        report["status"] = "completed"
        _atomic_json(report_path, report)
        _hourly(run, binding)
    except Exception as exc:
        report.update(status="failed", error=f"{type(exc).__name__}: {exc}")
        _atomic_json(report_path, report)
        raise
    return report


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("run", type=Path)
    parser.add_argument("--product", choices=PRODUCTS)
    args = parser.parse_args()
    if pl.thread_pool_size() > 2 or int(os.environ.get("OPENBLAS_NUM_THREADS", "1")) > 2:
        raise ValueError("Use POLARS_MAX_THREADS=2 OPENBLAS_NUM_THREADS=1 for the frozen budget")
    run = checked_run(args.run)
    for product in [args.product] if args.product else PRODUCTS:
        audit_product(run, product)


if __name__ == "__main__":
    main()
