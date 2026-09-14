"""Prepare immutable full-coverage inputs for the time-bucket specialist tournament."""

from __future__ import annotations

import argparse
import json
import tomllib
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

import polars as pl

from .core_extract import file_sha256
from .multivenue_early_entry_data import load_data_config
from .vwap_curve_admission_tournament import build_vwap_panel


def _resolve(root: Path, value: str) -> Path:
    path = Path(value)
    return path if path.is_absolute() else root / path


def _write_json(path: Path, payload: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(json.dumps(payload, indent=2, sort_keys=True, default=str) + "\n")
    temporary.replace(path)


def _write_parquet(frame: pl.DataFrame, path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + ".tmp")
    frame.write_parquet(temporary, compression="zstd", statistics=True)
    temporary.replace(path)


def _absolute_partitions(cache: Path, manifest: dict[str, Any]) -> dict[str, list[dict[str, Any]]]:
    return {
        group: [
            {**row, "path": str(cache / row["path"])}
            for row in rows
        ]
        for group, rows in manifest["partitions"].items()
    }


def _compose_early_manifest(
    base_cache: Path,
    tail_cache: Path,
    destination: Path,
    tail_start: datetime,
    range_end: datetime,
) -> dict[str, Any]:
    base = json.loads((base_cache / "source-manifest.json").read_text())
    tail = json.loads((tail_cache / "source-manifest.json").read_text())
    base_groups = _absolute_partitions(base_cache, base)
    tail_groups = _absolute_partitions(tail_cache, tail)
    cutoff = tail_start.date().isoformat()
    groups: dict[str, list[dict[str, Any]]] = {}
    for name in base_groups:
        older = [row for row in base_groups[name] if Path(row["path"]).stem < cutoff]
        newer = [row for row in tail_groups[name] if Path(row["path"]).stem >= cutoff]
        groups[name] = older + newer
        for row in groups[name]:
            path = Path(row["path"])
            if not path.is_file() or file_sha256(path) != row["sha256"]:
                raise RuntimeError(f"immutable source partition changed: {path}")
    payload = {
        "contract": {
            "schema_version": "btc-time-bucket-full-coverage-source-v1",
            "range_start": base["contract"]["range_start"],
            "range_end": range_end.isoformat(),
            "tail_start": tail_start.isoformat(),
            "read_only": True,
            "database_mutations": False,
            "new_sources": False,
        },
        "partitions": groups,
        "base_manifest_sha256": file_sha256(base_cache / "source-manifest.json"),
        "tail_manifest_sha256": file_sha256(tail_cache / "source-manifest.json"),
    }
    _write_json(destination / "source-manifest.json", payload)
    return payload


def _tail_labels(source_cache: Path, tail_start: datetime) -> pl.DataFrame:
    manifest = json.loads((source_cache / "source-manifest.json").read_text())
    paths = [
        source_cache / row["path"]
        for row in manifest["partitions"]["labels"]
        if Path(row["path"]).stem >= tail_start.date().isoformat()
    ]
    labels = pl.concat([pl.read_parquet(path) for path in paths], how="diagonal_relaxed")
    valid = (
        pl.col("twap_open_price").is_not_null()
        & pl.col("twap_close_price").is_not_null()
        & (pl.col("twap_open_effective_timestamp_rows") == 1)
        & (pl.col("twap_close_effective_timestamp_rows") == 1)
        & (pl.col("twap_open_valid_from_timestamp") <= pl.col("twap_open_source_timestamp"))
        & (pl.col("twap_close_valid_from_timestamp") <= pl.col("twap_close_source_timestamp"))
    )
    result = labels.filter(valid & (pl.col("window_start") >= tail_start)).with_columns(
        (pl.col("twap_close_price") >= pl.col("twap_open_price")).cast(pl.Int8).alias("label_up"),
        pl.lit(1.0).alias("base_label_weight"),
        pl.lit("authentic_official_twap60").alias("label_source"),
        (pl.col("twap_close_price") / pl.col("twap_open_price"))
        .log()
        .mul(10_000.0)
        .alias("target_margin_bps"),
        pl.col("twap_open_price").alias("opening_twap60"),
    )
    disagreement = result.filter(
        pl.col("label_up") != (pl.col("official_outcome") == "up").cast(pl.Int8)
    )
    if disagreement.height:
        raise RuntimeError(f"{disagreement.height} September TWAP labels disagree with outcomes")
    return result


def _compose_label_audit(
    base_path: Path, tail_cache: Path, destination: Path, tail_start: datetime
) -> dict[str, Any]:
    base = pl.read_parquet(base_path).filter(pl.col("window_start") < tail_start)
    tail = _tail_labels(tail_cache, tail_start)
    schema = base.schema
    for name, dtype in schema.items():
        if name not in tail.columns:
            tail = tail.with_columns(pl.lit(None, dtype=dtype).alias(name))
    tail = tail.select(*(pl.col(name).cast(dtype) for name, dtype in schema.items()))
    combined = (
        pl.concat((base, tail), how="vertical")
        .unique(subset=["market_id"], keep="last")
        .sort(["window_start", "market_id"])
    )
    _write_parquet(combined, destination)
    return {
        "path": str(destination),
        "sha256": file_sha256(destination),
        "rows": combined.height,
        "markets": combined["market_id"].n_unique(),
        "range_start": combined["window_start"].min(),
        "range_end": combined["window_start"].max(),
        "tail_markets": tail["market_id"].n_unique(),
    }


def _compose_open_interest(
    base_path: Path, tail_path: Path, destination: Path
) -> dict[str, Any]:
    combined = (
        pl.concat(
            (pl.read_parquet(base_path), pl.read_parquet(tail_path)),
            how="diagonal_relaxed",
        )
        .unique(subset=["source_timestamp"], keep="last")
        .sort("source_timestamp")
    )
    _write_parquet(combined, destination)
    return {
        "path": str(destination),
        "sha256": file_sha256(destination),
        "rows": combined.height,
        "range_start": combined["source_timestamp"].min(),
        "range_end": combined["source_timestamp"].max(),
    }


def prepare_sources(
    data_config_path: Path, tournament_config_path: Path, *, force_tail: bool = False
) -> Path:
    data_config = load_data_config(data_config_path)
    panel, panel_manifest = build_vwap_panel(data_config, force_tail=force_tail)
    del panel

    root = tournament_config_path.resolve().parents[1]
    with tournament_config_path.open("rb") as handle:
        raw = tomllib.load(handle)
    paths = raw["paths"]
    windows = raw["windows"]
    destination = _resolve(root, paths["early_source_cache"])
    destination.mkdir(parents=True, exist_ok=True)
    base_cache = _resolve(root, paths["early_base_source_cache"])
    tail_cache = data_config.package_root / data_config.raw["paths"]["tail_cache"]
    tail_start = datetime(2026, 8, 28, tzinfo=UTC)
    range_end = datetime.fromisoformat(windows["confirmation_end"])
    early_manifest = _compose_early_manifest(
        base_cache, tail_cache, destination, tail_start, range_end
    )
    labels = _compose_label_audit(
        _resolve(root, paths["early_base_label_audit"]),
        tail_cache,
        destination / "label-audit.parquet",
        tail_start,
    )
    interest = _compose_open_interest(
        _resolve(root, paths["early_base_open_interest"]),
        tail_cache / "open-interest.parquet",
        destination / "open-interest.parquet",
    )
    result = {
        "schema_version": "btc-time-bucket-source-preparation-v1",
        "created_at": datetime.now(UTC).isoformat(),
        "full_vwap_panel": panel_manifest,
        "early_source_manifest_sha256": file_sha256(destination / "source-manifest.json"),
        "early_source_partitions": {
            name: len(rows) for name, rows in early_manifest["partitions"].items()
        },
        "early_labels": labels,
        "open_interest": interest,
        "fixed_execution_quantity": 5,
        "read_only_sources": True,
        "database_mutations": False,
        "new_sources": False,
        "new_ingesters": False,
        "new_tables": False,
        "new_schemas": False,
    }
    _write_json(destination / "preparation-manifest.json", result)
    return destination / "preparation-manifest.json"


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--data-config", type=Path, required=True)
    parser.add_argument("--tournament-config", type=Path, required=True)
    parser.add_argument("--force-tail", action="store_true")
    arguments = parser.parse_args()
    print(
        prepare_sources(
            arguments.data_config,
            arguments.tournament_config,
            force_tail=arguments.force_tail,
        )
    )


if __name__ == "__main__":
    main()
