"""Offline, provenance-preserving source checkpoint for the time-bucket tournament."""

from __future__ import annotations

import argparse
import hashlib
import json
from collections import defaultdict
from datetime import UTC, datetime, timedelta
from pathlib import Path

import polars as pl
import pyarrow.parquet as pq

SSD = Path("/Volumes/docker-data/capitonic-btc-directional-model")
ARCHIVE = Path("/Volumes/docker-data/polymarket-bot")
PRODUCTS = {
    "chainlink_btcusd_reference_prices": ("chainlink-reference-price-drain/canonical-v1/direct",),
    "pmdata_chainlink_btcusd_reference_prices": (
        "chainlink-reference-price-drain/canonical-v1/pmdata",
        "pmdata-chainlink-reference-prices/verified-chunks-v1",
    ),
    "pmdata_chainlink_btcusd_twap": ("pmdata-chainlink-twap/verified-chunks-v1",),
    "polymarket_chainlink_btcusd_twap": ("polymarket-chainlink-twap/verified-chunks-v1",),
}


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(4 * 1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def write_json(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, default=str) + "\n")


def checked_run(path: Path) -> Path:
    path = path.resolve(strict=True)
    if not path.is_relative_to(SSD.resolve()) or path == SSD.resolve():
        raise ValueError("All generated outputs must be inside a distinct canonical SSD run")
    manifest = json.loads((path / "manifests/run.json").read_text())
    if manifest["branch"] != "training/btc-time-bucket-tournament":
        raise ValueError("Run belongs to another tournament")
    return path


def canonical_rows(frame: pl.DataFrame, product: str) -> tuple[pl.DataFrame, dict]:
    """Validate native identity; retain only demonstrated historical availability."""
    ref = product.endswith("reference_prices")
    direct = product == "chainlink_btcusd_reference_prices"
    keys = (
        ["feed_id", "source_timestamp", "report_sha256"]
        if ref
        else ["source_timestamp", "window_seconds"]
    )
    price = "price" if ref else "twap_price"
    values = [price]
    for name in ["valid_from_timestamp", "expires_at", "symbol", "full_accuracy_value"]:
        if name in frame.columns:
            values.append(name)
    identity_conflicts = (
        frame.group_by(keys)
        .agg(*(pl.col(name).drop_nulls().n_unique().alias(name) for name in values))
        .filter(pl.any_horizontal(pl.col(name) > 1 for name in values))
        .height
    )
    if identity_conflicts:
        raise ValueError(f"{product}: {identity_conflicts} conflicting native identities")
    valid = pl.col(price).is_not_null() & (pl.col(price) > 0)
    if "valid_from_timestamp" in frame.columns:
        valid &= pl.col("valid_from_timestamp").is_null() | (
            pl.col("valid_from_timestamp") <= pl.col("source_timestamp")
        )
    if ref:
        timestamps = ["source_timestamp", "received_at", "provider_available_at"]
        if direct:
            proven = (
                (pl.col("strategy_key") == "chainlink_btcusd_reference_price")
                & pl.col("capture_artifact_id").is_not_null()
                & pl.col("backfill_artifact_id").is_null()
                & pl.col("received_at").is_not_null()
            )
        else:
            proven = pl.col("provider_available_at").is_not_null()
    elif product == "pmdata_chainlink_btcusd_twap":
        timestamps = ["source_timestamp", "provider_received_at"]
        proven = pl.col("provider_received_at").is_not_null()
    else:
        timestamps = ["source_timestamp", "published_at", "received_at"]
        proven = pl.col("published_at").is_not_null() & pl.col("received_at").is_not_null()
    if not ref:
        valid &= pl.col("window_seconds").is_in([30, 60])
    available = pl.max_horizontal(timestamps).alias("available_at")
    frame = frame.with_columns(
        available,
        valid.fill_null(False).alias("_sane"),
        proven.fill_null(False).alias("_proven"),
    )
    raw = frame.height
    invalid = frame.filter(~pl.col("_sane").fill_null(False)).height
    unproven = frame.filter(~pl.col("_proven").fill_null(False)).height
    # Earliest demonstrated availability of the same native report is conservative.
    deduped = frame.sort(
        ["_sane", "_proven", "available_at"], descending=[True, True, False]
    ).unique(keys, keep="first", maintain_order=True)
    eligible = deduped.filter(pl.col("_sane") & pl.col("_proven"))
    facts = {
        "raw_rows": raw,
        "unique_rows": deduped.height,
        "duplicate_rows": raw - deduped.height,
        "identity_conflicts": identity_conflicts,
        "invalid_raw_rows": invalid,
        "unproven_raw_rows": unproven,
        "causally_eligible_rows": eligible.height,
        "source_min": frame["source_timestamp"].min(),
        "source_max": frame["source_timestamp"].max(),
        "eligible_source_min": eligible["source_timestamp"].min(),
        "eligible_source_max": eligible["source_timestamp"].max(),
        "price_min": str(frame[price].min()),
        "price_max": str(frame[price].max()),
        "native_keys": keys,
    }
    if eligible.height:
        latency = (
            eligible["available_at"] - eligible["source_timestamp"]
        ).dt.total_microseconds() / 1e6
        facts.update(
            latency_median_seconds=latency.median(),
            latency_p99_seconds=latency.quantile(0.99),
            latency_max_seconds=latency.max(),
        )
    return eligible.drop("_sane", "_proven").sort("available_at"), facts


def file_inventory(product: str) -> list[dict]:
    inventory = []
    for relative in PRODUCTS[product]:
        for path in sorted((ARCHIVE / relative).rglob("*.parquet")):
            before = path.stat()
            meta = pq.ParquetFile(path).metadata
            column = meta.schema.names.index("source_timestamp")
            minimum, maximum = [], []
            for group in range(meta.num_row_groups):
                stats = meta.row_group(group).column(column).statistics
                if stats and stats.has_min_max:
                    minimum.append(stats.min)
                    maximum.append(stats.max)
            digest = sha256(path)
            after = path.stat()
            if (before.st_size, before.st_mtime_ns) != (after.st_size, after.st_mtime_ns):
                raise ValueError(f"Source changed during freeze: {path}")
            sidecar = path.with_suffix(".manifest.json")
            if sidecar.is_file():
                old = json.loads(sidecar.read_text())
                if old.get("file_sha256") and old["file_sha256"] != digest:
                    raise ValueError(f"Archive checksum mismatch: {path}")
            inventory.append(
                {
                    "product": product,
                    "path": str(path),
                    "sha256": digest,
                    "bytes": before.st_size,
                    "rows": meta.num_rows,
                    "source_min": min(minimum) if minimum else None,
                    "source_max": max(maximum) if maximum else None,
                }
            )
    return inventory


def audit_product(run: Path, product: str) -> dict:
    destination = run / "inputs/validated-sources" / product
    destination.mkdir(parents=True, exist_ok=True)
    inventory = file_inventory(product)
    write_json(run / "inputs" / f"{product}-inventory.json", inventory)
    daily_files = defaultdict(list)
    for row in inventory:
        if not row["rows"]:
            continue
        if row["source_min"] is None or row["source_max"] is None:
            raise ValueError(f"Missing timestamp footer statistics: {row['path']}")
        day = row["source_min"].date()
        end = row["source_max"].date()
        while day <= end:
            daily_files[day].append(row["path"])
            day += timedelta(days=1)
    summaries, coverage, outputs = [], [], []
    for day, files in sorted(daily_files.items()):
        frames = [
            pl.read_parquet(p).filter(pl.col("source_timestamp").dt.date() == day) for p in files
        ]
        frame = pl.concat(frames, how="diagonal_relaxed")
        if not frame.height:
            continue
        monotonic = sum(not f["source_timestamp"].is_sorted() for f in frames if f.height)
        eligible, facts = canonical_rows(frame, product)
        facts.update(product=product, date=str(day), nonmonotonic_input_partitions=monotonic)
        summaries.append(facts)
        hours = frame.group_by(pl.col("source_timestamp").dt.truncate("1h").alias("hour")).agg(
            pl.len().alias("raw_rows")
        )
        good = eligible.group_by(pl.col("source_timestamp").dt.truncate("1h").alias("hour")).agg(
            pl.len().alias("eligible_rows")
        )
        coverage.append(
            hours.join(good, on="hour", how="left").with_columns(pl.lit(product).alias("product"))
        )
        target = destination / f"{day}.parquet"
        eligible.write_parquet(target, compression="zstd")
        outputs.append({"path": str(target), "sha256": sha256(target), "rows": eligible.height})
        print(
            json.dumps(
                {
                    "product": product,
                    "date": str(day),
                    "raw": frame.height,
                    "eligible": eligible.height,
                    "duplicates": facts["duplicate_rows"],
                }
            ),
            flush=True,
        )
    report = {
        "product": product,
        "inventory_files": len(inventory),
        "raw_rows": sum(r["rows"] for r in inventory),
        "daily": summaries,
        "outputs": outputs,
    }
    write_json(run / "manifests" / f"{product}-audit.json", report)
    if coverage:
        pl.concat(coverage).sort("hour").write_parquet(
            run / "metrics" / f"{product}-hourly-coverage.parquet"
        )
    return report


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("run", type=Path)
    parser.add_argument("--product", choices=PRODUCTS)
    args = parser.parse_args()
    run = checked_run(args.run)
    products = [args.product] if args.product else list(PRODUCTS)
    for product in products:
        try:
            audit_product(run, product)
        except Exception as error:
            write_json(
                run / "manifests/source-audit-failure.json",
                {
                    "product": product,
                    "error": str(error),
                    "at": datetime.now(UTC).isoformat(),
                    "checkpoint_accepted": False,
                },
            )
            raise


if __name__ == "__main__":
    main()
