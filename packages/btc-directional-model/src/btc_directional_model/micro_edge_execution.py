"""Read existing books at omitted decision times through the established Q5 extractor."""

import json
import os
from datetime import timedelta
from pathlib import Path

import polars as pl

from .core_extract import configure_read_only_connection, database_connection, file_sha256
from .micro_edge_data import CACHE, END, PACKAGE, START
from .time_bucket_source_preparation import _write_json, _write_parquet
from .twap60_training_data import _query_capacity_frame


def snapshot_omitted_checkpoints():
    for line in (
        Path("/Users/gunnermadsen/development/polymarket-bot/.env.postgres")
        .read_text()
        .splitlines()
    ):
        if line and not line.startswith("#") and "=" in line:
            key, value = line.split("=", 1)
            os.environ.setdefault(key, value.strip().strip('"').strip("'"))
    source = PACKAGE / "sql/btc-current-orderbook-capacity-execution-source.sql"
    query = source.read_text().replace(
        "generate_series(30, 240, 5) offset_seconds",
        "generate_series(0, 295, 5) offset_seconds\n  WHERE offset_seconds < 30 OR offset_seconds > 240",
    )
    query = query.replace(
        "WHERE source.market_id = point.market_id",
        "WHERE source.sampled_at >= %(batch_start)s - interval '2 seconds'\n      AND source.sampled_at < %(batch_end)s\n      AND source.market_id = point.market_id",
    )
    import hashlib

    identity = {
        "query_sha256": hashlib.sha256(query.encode()).hexdigest(),
        "existing_source_sql_sha256": file_sha256(source),
        "seconds": [s for s in range(0, 300, 5) if s < 30 or s > 240],
        "read_only": True,
    }
    rows = []
    day = START
    while day < END:
        dest = CACHE / "omitted-execution" / f"{day.date()}.parquet"
        sidecar = dest.with_suffix(".json")
        if dest.exists() and sidecar.exists():
            saved = json.loads(sidecar.read_text())
            if saved["identity"] != identity or saved["sha256"] != file_sha256(dest):
                raise RuntimeError("execution checkpoint mismatch")
        else:
            connection = database_connection()
            configure_read_only_connection(connection)
            connection.execute("SET statement_timeout = '60s'")
            connection.execute("SET max_parallel_workers_per_gather = 0")
            try:
                frame = _query_capacity_frame(
                    connection,
                    query,
                    {"batch_start": day, "batch_end": day + timedelta(days=1)},
                    cursor_name="micro_edge_omitted_books",
                )
            finally:
                connection.close()
            _write_parquet(frame, dest)
            present = frame.filter(
                pl.col("up_ask_vwap_5").is_not_null() | pl.col("down_ask_vwap_5").is_not_null()
            )
            saved = {
                "day": str(day.date()),
                "path": str(dest),
                "sha256": file_sha256(dest),
                "rows": frame.height,
                "book_rows": present.height,
                "identity": identity,
            }
            _write_json(sidecar, saved)
        rows.append(saved)
        print("omitted books", day.date(), saved["book_rows"], flush=True)
        day += timedelta(days=1)
    _write_json(
        CACHE / "omitted-execution-manifest.json", {"partitions": rows, "identity": identity}
    )


if __name__ == "__main__":
    snapshot_omitted_checkpoints()


def enrich_execution(panel, books, features):
    """Recompute quantity-dependent context consistently at five shares for this run."""
    from .capacity_training import _attach_asymmetric_book_features
    from .micro_edge_data import attach_vwap5

    keys = ["market_id", "observed_at"]
    if books.is_empty():
        return panel.with_columns(
            *(pl.lit(None, dtype=pl.Float64).alias(c) for c in [*features, "fee_rate"]),
            pl.lit(False).alias("has_execution"),
        )
    books = books.unique(keys, keep="last")
    enriched = attach_vwap5(panel.select(keys), books)
    quantities = [5, 10, 15, 20, 25, 30, 40, 50, 75, 100, 125, 150, 175, 200]
    extra = [f"{side}_ask_vwap_{q}" for side in ("up", "down") for q in quantities if q != 5]
    enriched = enriched.join(books.select(*keys, *extra), on=keys, how="left", validate="1:1")
    # Larger ladders are optional context, with the same side-specific book validity.
    enriched = enriched.with_columns(
        *(
            pl.when(pl.col(f"{side}_ask_vwap_5").is_not_null())
            .then(pl.col(f"{side}_ask_vwap_{q}"))
            .otherwise(None)
            .alias(f"{side}_ask_vwap_{q}")
            for side in ("up", "down")
            for q in quantities
            if q != 5
        )
    )
    enriched = _attach_asymmetric_book_features(enriched, 5, 0.005)
    enriched = enriched.with_columns(
        *[
            (pl.col(f"up_ask_vwap_{q}") + pl.col(f"down_ask_vwap_{q}") - 1).alias(
                f"pm_vwap{q}_overround"
            )
            for q in (5, 50, 200)
        ],
        *[
            pl.col(f"{side}_ask_depth").log1p().alias(f"pm_{side}_depth_log")
            for side in ("up", "down")
        ],
        *[
            (pl.col(f"{side}_ask_vwap_{b}") - pl.col(f"{side}_ask_vwap_{a}")).alias(
                f"pm_{side}_slope_{a}_{b}"
            )
            for side in ("up", "down")
            for a, b in ((5, 25), (25, 100), (100, 200))
        ],
    )
    chosen = [c for c in [*features, "fee_rate"] if c in enriched.columns]
    panel = panel.drop([c for c in chosen if c in panel.columns]).join(
        enriched.select(*keys, *chosen), on=keys, how="left", validate="1:1"
    )
    return panel.with_columns(
        pl.any_horizontal(pl.col(c).is_finite().fill_null(False) for c in features).alias(
            "has_execution"
        )
    )
