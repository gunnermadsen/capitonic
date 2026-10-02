"""Freeze SSD market identities and inspect time-bucket source availability offline."""

from __future__ import annotations

import argparse
import json
from datetime import UTC, datetime, timedelta
from pathlib import Path

import polars as pl
import pyarrow.parquet as pq

from .time_bucket_source_audit import ARCHIVE, PRODUCTS, SSD, checked_run, sha256, write_json

CORE_ROOTS = (
    SSD
    / "btc-5m-signal-attribution-early-entry-20260321-20260905-20260901T010530Z/inputs/shared-data/btc-signal-attribution-early-entry-20260321-20260905/core_current",
    SSD
    / "btc-5m-time-bucket-specialist-tournament-20260321-20260914-20260914T171438Z/inputs/shared-data/btc-full-coverage-vwap-admission-tail-20260827-20260914/core_current",
    SSD
    / "conservative-selective-vwap-capacity-20260923T011753Z/inputs/sep13-sep21-source/core_current",
    SSD / "entry-disposition-tournament-20260924T225644Z/inputs/sep20-sep23-source/core_current",
)


def markets(run: Path) -> pl.DataFrame:
    records, frames = [], []
    columns = ["market_id", "window_start", "window_end", "official_outcome"]
    for root in CORE_ROOTS:
        for path in sorted(root.glob("*.parquet")):
            frame = pl.read_parquet(path, columns=columns)
            records.append({"path": str(path), "sha256": sha256(path), "rows": frame.height})
            if frame.height:
                frames.append(frame.unique())
    merged = pl.concat(frames, how="diagonal_relaxed").unique().sort("window_start")
    bad = merged.group_by("market_id").len().filter(pl.col("len") != 1)
    if bad.height:
        raise ValueError(f"{bad.height} conflicting market identities/outcomes")
    if merged.filter(
        (pl.col("window_end") - pl.col("window_start")) != pl.duration(minutes=5)
    ).height:
        raise ValueError("Non-five-minute market in source population")
    merged.write_parquet(run / "inputs/market-identities.parquet")
    write_json(
        run / "inputs/market-source-inventory.json",
        {
            "files": records,
            "markets": merged.height,
            "start": merged["window_start"].min(),
            "end": merged["window_end"].max(),
            "source_role": "market identity/timing/outcome only; old derived feature timing not admitted",
            "output_sha256": sha256(run / "inputs/market-identities.parquet"),
        },
    )
    return merged


def audit_joins(run: Path, population: pl.DataFrame) -> None:
    """Report causal source age at fixed bucket ends; no model threshold selection."""
    points = (
        population.select("market_id", "window_start")
        .join(pl.DataFrame({"bucket_start": list(range(0, 260, 20))}), how="cross")
        .with_columns(
            (pl.col("window_start") + pl.duration(seconds=pl.col("bucket_start") + 19)).alias(
                "decision_at"
            )
        )
        .sort("decision_at")
    )
    result = points
    evidence = {}
    for product in PRODUCTS:
        paths = sorted((run / "inputs/validated-sources" / product).glob("*.parquet"))
        if not paths:
            raise ValueError(f"Missing completed source validation: {product}")
        is_twap = product.endswith("twap")
        columns = ["source_timestamp", "available_at"] + (["window_seconds"] if is_twap else [])
        source = pl.scan_parquet(paths).select(columns).collect(engine="streaming")
        product_counts = {}
        for window in [30, 60] if is_twap else [0]:
            selected = source.filter(pl.col("window_seconds") == window) if window else source
            suffix = f"{product}_{window}" if window else product
            selected = (
                selected.select("source_timestamp", "available_at").unique().sort("available_at")
            )
            joined = points.join_asof(
                selected, left_on="decision_at", right_on="available_at", strategy="backward"
            )
            joined = joined.with_columns(
                (pl.col("decision_at") - pl.col("source_timestamp"))
                .dt.total_microseconds()
                .truediv(1e6)
                .alias(f"{suffix}_source_age_seconds"),
                (pl.col("decision_at") - pl.col("available_at"))
                .dt.total_microseconds()
                .truediv(1e6)
                .alias(f"{suffix}_receipt_age_seconds"),
            )
            product_counts[str(window)] = {
                "raw_backward_matches": joined["source_timestamp"].drop_nulls().len(),
                **{
                    f"fresh_within_{age}s": joined.filter(
                        pl.col(f"{suffix}_source_age_seconds") <= age
                    ).height
                    for age in [2, 5, 10]
                },
            }
            result = result.with_columns(
                joined[f"{suffix}_source_age_seconds"], joined[f"{suffix}_receipt_age_seconds"]
            )
        evidence[product] = product_counts
    result.write_parquet(run / "metrics/mandatory-product-market-bucket-coverage.parquet")
    write_json(
        run / "manifests/mandatory-product-join-audit.json",
        {
            "decision_count": points.height,
            "market_count": population.height,
            "bucket_start_seconds": list(range(0, 260, 20)),
            "audit_time": "bucket end (start+19 seconds); diagnostic convention, policy timing not selected",
            "freshness": "2/5/10 second diagnostics, no model threshold selected and no missing value imputation",
            "products": evidence,
        },
    )
    summarize_joins(run)


def joined_coverage_by_day_bucket(points: pl.DataFrame) -> pl.DataFrame:
    """Count source availability only, without examining outcomes or selecting cutoffs."""
    records = []
    points = points.with_columns(pl.col("decision_at").dt.date().alias("date"))
    for age in [2, 5, 10]:
        masks = {}
        for product in PRODUCTS:
            windows = [30, 60] if product.endswith("twap") else [None]
            columns = [
                f"{product}_{window}_source_age_seconds"
                if window
                else f"{product}_source_age_seconds"
                for window in windows
            ]
            masks[product] = pl.all_horizontal(
                pl.col(column).is_between(0, age).fill_null(False) for column in columns
            )
        masks["all_four_products"] = pl.all_horizontal(masks.values())
        for product, eligible in masks.items():
            records.append(
                points.group_by("date", "bucket_start")
                .agg(
                    pl.len().alias("observed_market_decisions"),
                    eligible.sum().alias("causal_fresh_decisions"),
                )
                .with_columns(
                    pl.lit(product).alias("product"),
                    pl.lit(age).alias("diagnostic_age_seconds"),
                )
            )
    return pl.concat(records).sort("product", "diagnostic_age_seconds", "date", "bucket_start")


def summarize_joins(run: Path) -> None:
    source = run / "metrics/mandatory-product-market-bucket-coverage.parquet"
    coverage = joined_coverage_by_day_bucket(pl.read_parquet(source))
    output = run / "metrics/mandatory-product-day-bucket-coverage.parquet"
    coverage.write_parquet(output, compression="zstd")
    summary = coverage.group_by("product", "diagnostic_age_seconds").agg(
        pl.col("observed_market_decisions").sum(),
        pl.col("causal_fresh_decisions").sum(),
        pl.col("date").filter(pl.col("causal_fresh_decisions") > 0).min().alias("first_date"),
        pl.col("date").filter(pl.col("causal_fresh_decisions") > 0).max().alias("last_date"),
        pl.col("date").filter(pl.col("causal_fresh_decisions") > 0).n_unique().alias("dates"),
        pl.col("bucket_start").filter(pl.col("causal_fresh_decisions") > 0).n_unique().alias("buckets"),
    )
    write_json(
        run / "manifests/mandatory-product-day-bucket-coverage.json",
        {
            "source_sha256": sha256(source),
            "output": str(output),
            "output_sha256": sha256(output),
            "scope": "Coverage only. TWAP products require both 30/60 windows; all-four requires all six observations. No model freshness threshold selected.",
            "missing_dates": "Absent market dates are unknown coverage, not zero activity.",
            "summary": summary.sort("product", "diagnostic_age_seconds").to_dicts(),
        },
    )


def timestamp_inventory(
    run: Path, product: str, roots: list[Path], timestamp: str, minimum: datetime | None = None
) -> None:
    """Footer inventory and hashes; no price-pattern or outcome analysis."""
    records = []
    for root in roots:
        for path in sorted(root.rglob("*.parquet")):
            meta = pq.ParquetFile(path).metadata
            if timestamp not in meta.schema.names:
                records.append(
                    {"path": str(path), "rows": meta.num_rows, "missing_timestamp": timestamp}
                )
                continue
            bounds = {}
            for name in [timestamp, "provider_available_at", "received_at"]:
                if name not in meta.schema.names:
                    continue
                idx = meta.schema.names.index(name)
                stats = [
                    meta.row_group(g).column(idx).statistics for g in range(meta.num_row_groups)
                ]
                minima = [s.min for s in stats if s and s.has_min_max]
                maxima = [s.max for s in stats if s and s.has_min_max]
                bounds[name] = {
                    "min": min(minima) if minima else None,
                    "max": max(maxima) if maxima else None,
                    "null_count": sum(s.null_count for s in stats if s),
                }
            end = bounds.get(timestamp, {}).get("max")
            applicable = bool(meta.num_rows and (minimum is None or end is None or end >= minimum))
            record = {
                "path": str(path),
                "rows": meta.num_rows,
                "bytes": path.stat().st_size,
                "bounds": bounds,
                "applicable": applicable,
            }
            if applicable:
                record["sha256"] = sha256(path)
            records.append(record)
    write_json(
        run / "inputs" / f"{product}-inventory.json",
        {
            "product": product,
            "files": records,
            "minimum_applicable_time": minimum,
            "frozen_at": datetime.now(UTC).isoformat(),
        },
    )
    print(
        json.dumps(
            {
                "product": product,
                "files": len(records),
                "applicable_files": sum(bool(r.get("applicable")) for r in records),
            }
        ),
        flush=True,
    )


def freeze_reference_inventory(run: Path) -> None:
    """Hash inventoried optional sources once, preserving all product identities."""
    cached = {}
    excluded = set()
    for inventory in (run / "inputs").glob("*-inventory.json"):
        payload = json.loads(inventory.read_text())
        rows = payload if isinstance(payload, list) else payload.get("files", [])
        for row in rows:
            if row.get("sha256"):
                cached[row["path"]] = row["sha256"]
            elif row.get("applicable") is False:
                excluded.add(row["path"])
    locations = json.loads((run / "manifests/optional-source-location-map.json").read_text())
    groups = []
    for group in locations["groups"]:
        files = []
        for record in group["files"]:
            path = Path(record["path"])
            if str(path) in excluded:
                continue
            before = path.stat()
            if before.st_size != record["bytes"]:
                raise ValueError(f"Source changed since inventory: {path}")
            if str(path) not in cached:
                cached[str(path)] = sha256(path)
            after = path.stat()
            if (before.st_size, before.st_mtime_ns) != (after.st_size, after.st_mtime_ns):
                raise ValueError(f"Source changed during freeze: {path}")
            files.append({**record, "sha256": cached[str(path)], "frozen": True})
        groups.append(
            {
                **group,
                "files": files,
                "frozen": True,
                "note": "Hash freeze only; causal admission and source completeness pending.",
            }
        )
        print(json.dumps({"hashed_product": group["product_key"], "files": len(files)}), flush=True)
        write_json(
            run / "inputs/optional-source-freeze.json", {"groups": groups, "complete": False}
        )
    historical = json.loads((run / "manifests/historical-model-compatibility.json").read_text())
    artifacts = []
    for entry in historical["entries"]:
        for record in entry.get("artifacts", []):
            path = Path(record["path"])
            if not path.is_file():
                continue
            digest = cached.get(str(path)) or sha256(path)
            cached[str(path)] = digest
            if record.get("sha256") and digest != record["sha256"]:
                raise ValueError(f"Historical artifact hash mismatch: {path}")
            artifacts.append(
                {
                    "pathway": entry["pathway"],
                    "path": str(path),
                    "sha256": digest,
                    "role": "historical input; replay eligibility pending",
                }
            )
    write_json(run / "inputs/historical-artifact-freeze.json", artifacts)
    write_json(
        run / "inputs/optional-source-freeze.json",
        {
            "groups": groups,
            "complete": True,
            "unavailable_categories": locations["unavailable_categories"],
            "frozen_at": datetime.now(UTC).isoformat(),
        },
    )


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("run", type=Path)
    parser.add_argument("--joins-only", action="store_true")
    parser.add_argument("--freeze-references-only", action="store_true")
    parser.add_argument("--summarize-joins-only", action="store_true")
    args = parser.parse_args()
    run = checked_run(args.run)
    if args.summarize_joins_only:
        summarize_joins(run)
        return
    if args.freeze_references_only:
        freeze_reference_inventory(run)
        return
    population = markets(run)
    audit_joins(run, population)
    if not args.joins_only:
        start = population["window_start"].min() - timedelta(days=1)
        for product, roots, timestamp in [
            (
                "binance_spot_one_second_candles",
                [ARCHIVE / "binance-one-second-ohlcv/verified-chunks-v1"],
                "open_timestamp",
            ),
            (
                "rtds_reference_price_ticks",
                [ARCHIVE / "reference-price-ticks/verified-chunks-v1"],
                "source_timestamp",
            ),
            (
                "polymarket_clob_l2",
                [
                    ARCHIVE / "orderbook-drain/canonical-v1",
                    ARCHIVE / "orderbook-drain/verified-chunks-v1",
                ],
                "sampled_at",
            ),
        ]:
            timestamp_inventory(run, product, roots, timestamp, start)


if __name__ == "__main__":
    main()
