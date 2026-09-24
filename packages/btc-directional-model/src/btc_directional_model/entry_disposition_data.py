"""Build causal checkpoint and book evidence for entry/disposition training."""

from __future__ import annotations

import argparse
import json
from datetime import date, timedelta
from pathlib import Path

import polars as pl

from .conservative_selective_training import PRICE_FEATURES, VOLUME_FEATURES
from .core_features import derive_core_point_in_time_features
from .fok_retry_backtest import QUANTITIES, parse_asks, walk

CHECKPOINTS = tuple(range(30, 220, 5))


def _core_path(day: date, sources: list[Path]) -> Path | None:
    for root in sources:
        path = root / f"{day.isoformat()}.parquet"
        if path.is_file():
            return path
    return None


def _books(day: date, canonical: Path, verified: Path) -> tuple[pl.DataFrame, str]:
    if day < date(2026, 8, 14):
        paths = sorted((canonical / f"date={day.isoformat()}").glob("hour=*.parquet"))
        source = "canonical-hourly"
    else:
        paths = sorted((verified / f"year={day.year:04d}/month={day.month:02d}/day={day.day:02d}").glob("*.parquet"))
        source = "verified-daily"
    if not paths:
        return pl.DataFrame(), source
    frame = pl.scan_parquet(paths, hive_partitioning=False).select(
        "market_id", "outcome", "sampled_at", "source_timestamp", "received_at", "asks", "bids", "token_id"
    ).collect(engine="streaming")
    return frame, source


def _side_join(points: pl.DataFrame, books: pl.DataFrame, side: str) -> pl.DataFrame:
    source = (
        books.filter(pl.col("outcome") == side)
        .with_columns(pl.max_horizontal("sampled_at", "source_timestamp", "received_at").alias("available_at"))
        .sort("available_at")
        .select("market_id", "available_at", "asks", "bids", "token_id")
    )
    joined = points.sort("observed_at").join_asof(
        source, left_on="observed_at", right_on="available_at", by="market_id", strategy="backward",
        tolerance="2s", check_sortedness=False,
    )
    return joined.rename({
        "available_at": f"{side}_book_available_at", "asks": f"{side}_asks",
        "bids": f"{side}_bids", "token_id": f"{side}_token_id",
    })


def _vwap(value: str | None, quantities: tuple[int, ...]) -> list[float | None]:
    asks = parse_asks(value)
    return [None if (result := walk(asks, amount)) is None else result[0] for amount in quantities]


def build_day(day: date, core_sources: list[Path], canonical: Path, verified: Path, output: Path) -> dict:
    core_path = _core_path(day, core_sources)
    if core_path is None:
        return {"day": day.isoformat(), "status": "missing_core"}
    core = pl.read_parquet(core_path).sort(["market_id", "seconds_elapsed"])
    complete = core.group_by("market_id").agg(
        pl.len().alias("rows"), pl.col("seconds_elapsed").n_unique().alias("seconds"),
        pl.col("seconds_elapsed").min().alias("minimum"), pl.col("seconds_elapsed").max().alias("maximum"),
    ).filter((pl.col("rows") == 300) & (pl.col("seconds") == 300) & (pl.col("minimum") == 0) & (pl.col("maximum") == 299)).select("market_id")
    core = core.unique(subset=["market_id", "observed_at"], keep="last", maintain_order=True).join(complete, on="market_id", how="inner").sort(["market_id", "seconds_elapsed"])
    if core.is_empty():
        return {"day": day.isoformat(), "status": "no_complete_core"}
    core = core.with_columns(pl.first("btc_open").over("market_id").alias("opening_boundary"))
    features = derive_core_point_in_time_features(core).filter(pl.col("seconds_elapsed").is_in(CHECKPOINTS))
    columns = ["market_id", "window_start", "official_outcome", "observed_at", "seconds_elapsed", *PRICE_FEATURES, *VOLUME_FEATURES]
    points = features.select(columns)
    books, source = _books(day, canonical, verified)
    if books.is_empty():
        return {"day": day.isoformat(), "status": "missing_books", "markets": points["market_id"].n_unique(), "book_source": source}
    points = _side_join(points, books, "up")
    points = _side_join(points, books, "down")
    for side in ("up", "down"):
        values = [_vwap(value, QUANTITIES) for value in points[f"{side}_asks"].to_list()]
        points = points.with_columns(
            pl.Series(f"{side}_ask_vwap_{quantity}", [row[index] for row in values], dtype=pl.Float64)
            for index, quantity in enumerate(QUANTITIES)
        )
    output.parent.mkdir(parents=True, exist_ok=True)
    points.sort(["window_start", "market_id", "seconds_elapsed"]).write_parquet(output, compression="zstd")
    valid = points.drop_nulls(["up_asks", "down_asks"])
    return {
        "day": day.isoformat(), "status": "written", "core_source": str(core_path), "book_source": source,
        "book_snapshots": books.height, "rows": points.height, "markets": points["market_id"].n_unique(),
        "both_book_rows": valid.height, "both_book_markets": valid["market_id"].n_unique(),
        "output": str(output),
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--start", type=date.fromisoformat, required=True)
    parser.add_argument("--end", type=date.fromisoformat, required=True)
    parser.add_argument("--core-source", type=Path, action="append", required=True)
    parser.add_argument("--canonical-books", type=Path, required=True)
    parser.add_argument("--verified-books", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    day = args.start
    manifest = []
    while day < args.end:
        record = build_day(day, args.core_source, args.canonical_books, args.verified_books,
                           args.output / f"date={day.isoformat()}.parquet")
        manifest.append(record)
        print(json.dumps(record, default=str), flush=True)
        day += timedelta(days=1)
    (args.output.parent / "causal-book-panel-coverage.json").write_text(json.dumps(manifest, indent=2) + "\n")


if __name__ == "__main__":
    main()
