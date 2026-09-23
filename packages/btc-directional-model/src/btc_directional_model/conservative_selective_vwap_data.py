"""Build a clean conservative-selective panel extension with native CLOB VWAP."""

from __future__ import annotations

import argparse
import json
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

import polars as pl

from .core_extract import file_sha256
from .core_features import derive_core_point_in_time_features
from .conservative_selective_training import PRICE_FEATURES, VOLUME_FEATURES
from .multivenue_early_entry_data import ENTRY_SECONDS, _attach_execution_60_240
from .twap60_training_data import DataPaths, extract_tournament_sources, load_source_group


def _utc(value: str) -> datetime:
    return datetime.fromisoformat(value).astimezone(UTC)


def _write_json(path: Path, payload: dict[str, Any]) -> None:
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(json.dumps(payload, indent=2, sort_keys=True, default=str) + "\n")
    temporary.replace(path)


def _tail_panel(paths: DataPaths, start: datetime) -> pl.DataFrame:
    raw = load_source_group(paths, "core_current").sort(["market_id", "seconds_elapsed"])
    raw = raw.unique(subset=["market_id", "observed_at"], keep="last", maintain_order=True)
    complete = (
        raw.group_by("market_id")
        .agg(
            pl.len().alias("rows"),
            pl.col("seconds_elapsed").n_unique().alias("seconds"),
            pl.col("seconds_elapsed").min().alias("minimum"),
            pl.col("seconds_elapsed").max().alias("maximum"),
        )
        .filter(
            (pl.col("rows") == 300)
            & (pl.col("seconds") == 300)
            & (pl.col("minimum") == 0)
            & (pl.col("maximum") == 299)
        )
        .select("market_id")
    )
    boundaries = raw.group_by("market_id", maintain_order=True).agg(
        pl.first("btc_open").alias("opening_boundary")
    )
    core = (
        raw.drop("opening_boundary")
        .join(complete, on="market_id", how="inner")
        .join(boundaries, on="market_id", how="inner", validate="m:1")
    )
    panel = (
        derive_core_point_in_time_features(core)
        .filter(pl.col("seconds_elapsed").is_in(ENTRY_SECONDS))
        .with_columns(
            (pl.col("seconds_elapsed") / 300.0).alias("seconds_elapsed_scaled"),
            ((300 - pl.col("seconds_elapsed")) / 300.0).alias("seconds_remaining_scaled"),
        )
    )
    return _attach_execution_60_240(
        panel,
        load_source_group(paths, "execution"),
        freshness=2,
    ).filter(pl.col("window_start") >= start)


def build(
    *,
    package_root: Path,
    base_panel: Path,
    source_cache: Path,
    destination: Path,
    manifest_path: Path,
    validation_start: datetime,
    append_start: datetime,
    range_end: datetime,
) -> dict[str, Any]:
    paths = DataPaths(
        package_root=package_root,
        cache=source_cache,
        core_features=source_cache / "unused-core-features.parquet",
        core_current_sql=package_root / "sql/btc-twap60-core-current-source.sql",
        oracle_sql=package_root / "sql/btc-core-oracle-source.sql",
        label_sql=package_root / "sql/btc-twap60-label-source.sql",
        refprice_sql=package_root / "sql/btc-twap60-refprice-source.sql",
        candle_sql=package_root / "sql/btc-twap60-candle-source.sql",
        execution_sql=package_root / "sql/btc-current-orderbook-capacity-execution-source.sql",
    )
    extract_tournament_sources(
        paths,
        range_start=validation_start,
        range_end=range_end,
        current_start=validation_start,
    )
    base = pl.read_parquet(base_panel)
    rebuilt = _tail_panel(paths, validation_start)
    features = [name for name in (*PRICE_FEATURES, *VOLUME_FEATURES) if name in base.columns]
    overlap_base = base.filter(
        (pl.col("window_start") >= validation_start) & (pl.col("window_start") < append_start)
    ).select("market_id", "observed_at", *features)
    overlap_new = rebuilt.filter(pl.col("window_start") < append_start).select(
        "market_id", "observed_at", *features
    )
    overlap = overlap_base.join(
        overlap_new,
        on=["market_id", "observed_at"],
        how="inner",
        suffix="_rebuilt",
        validate="1:1",
    )
    mismatches = {}
    for name in features:
        rebuilt_name = f"{name}_rebuilt"
        null_disagreement = overlap.filter(
            pl.col(name).is_null() != pl.col(rebuilt_name).is_null()
        ).height
        delta = overlap.select(
            (pl.col(name) - pl.col(rebuilt_name)).abs().max()
        ).item()
        if null_disagreement or (delta is not None and delta > 1e-9):
            mismatches[name] = {
                "maximum_absolute_delta": delta,
                "null_disagreement_rows": null_disagreement,
            }
    if overlap.height == 0 or mismatches:
        raise RuntimeError(
            f"feature parity failed: overlap_rows={overlap.height}, mismatches={mismatches}"
        )

    tail = rebuilt.filter(pl.col("window_start") >= append_start)
    for name, dtype in base.schema.items():
        if name not in tail.columns:
            tail = tail.with_columns(pl.lit(None, dtype=dtype).alias(name))
    tail = tail.select(*(pl.col(name).cast(dtype) for name, dtype in base.schema.items()))
    combined = pl.concat(
        (base.filter(pl.col("window_start") < append_start), tail),
        how="vertical",
    ).sort(["window_start", "market_id", "seconds_elapsed"])
    duplicates = combined.select("market_id", "observed_at").is_duplicated().sum()
    if duplicates:
        raise RuntimeError(f"combined panel has {duplicates} duplicate checkpoints")
    destination.parent.mkdir(parents=True, exist_ok=True)
    combined.write_parquet(destination, compression="zstd", statistics=True)
    quantities = (5, 10, 15, 20, 25, 30, 40, 50, 75, 100, 125, 150, 175, 200)
    coverage = {}
    for quantity in quantities:
        valid = combined.drop_nulls([f"up_ask_vwap_{quantity}", f"down_ask_vwap_{quantity}"])
        coverage[str(quantity)] = {
            "rows": valid.height,
            "markets": valid["market_id"].n_unique(),
        }
    manifest = {
        "schema_version": "btc-conservative-selective-vwap-panel-v1",
        "base_panel": str(base_panel),
        "base_sha256": file_sha256(base_panel),
        "source_cache": str(source_cache),
        "source_manifest_sha256": file_sha256(source_cache / "source-manifest.json"),
        "validation_start": validation_start,
        "append_start": append_start,
        "range_start": combined["window_start"].min(),
        "range_end_exclusive": range_end,
        "rows": combined.height,
        "markets": combined["market_id"].n_unique(),
        "tail_rows": tail.height,
        "tail_markets": tail["market_id"].n_unique(),
        "overlap_parity_rows": overlap.height,
        "feature_parity_tolerance": 1e-9,
        "vwap_quantities": list(quantities),
        "vwap_coverage": coverage,
        "predictive_features_include_vwap": False,
        "database_mutations": False,
        "sha256": file_sha256(destination),
    }
    _write_json(manifest_path, manifest)
    return manifest


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--package-root", type=Path, required=True)
    parser.add_argument("--base-panel", type=Path, required=True)
    parser.add_argument("--source-cache", type=Path, required=True)
    parser.add_argument("--destination", type=Path, required=True)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--validation-start", default="2026-09-13T00:00:00Z")
    parser.add_argument("--append-start", default="2026-09-14T00:00:00Z")
    parser.add_argument("--range-end", default="2026-09-22T00:00:00Z")
    arguments = parser.parse_args()
    build(
        package_root=arguments.package_root.resolve(),
        base_panel=arguments.base_panel,
        source_cache=arguments.source_cache,
        destination=arguments.destination,
        manifest_path=arguments.manifest,
        validation_start=_utc(arguments.validation_start),
        append_start=_utc(arguments.append_start),
        range_end=_utc(arguments.range_end),
    )
