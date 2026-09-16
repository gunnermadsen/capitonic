"""Tournament-local assembly of existing causal Parquet sources; no source mutations."""

from __future__ import annotations

import argparse
import json
import os
from datetime import UTC, datetime, timedelta
from pathlib import Path

import numpy as np
import polars as pl

from .core_extract import file_sha256
from .multivenue_early_entry_data import (
    CHAINLINK_FEATURES,
    KEY_COLUMNS,
    ORACLE_FEATURES,
    _mask_optional_features,
    _preserving_feature_join,
    attach_candle_context,
    attach_causal_oracle_rounds,
    attach_causal_refprice_features,
    derive_core_point_in_time_features,
    derive_oracle_point_in_time_features,
    prepare_causal_oracle_rounds,
)
from .time_bucket_source_preparation import _write_json, _write_parquet

ARCHIVE = Path(
    "/Volumes/docker-data/archives/polymarket-bot-worktrees/time-bucket-specialist-tournament"
)
OLD = "/Users/gunnermadsen/development/polymarket-bot/worktress/time-bucket-specialist-tournament/"
PACKAGE = Path(__file__).resolve().parents[2]
CACHE = PACKAGE / "data/btc-micro-edge-20260607-20260914"
START = datetime(2026, 6, 7, tzinfo=UTC)
END = datetime(2026, 9, 15, tzinfo=UTC)
BASE = ARCHIVE / "packages/btc-directional-model/data"


def resolve_source(value: str) -> Path:
    p = Path(value)
    if p.is_file():
        return p
    if value.startswith(OLD):
        p = ARCHIVE / value[len(OLD) :]
    if not p.is_file():
        raise FileNotFoundError(value)
    return p


def snapshot_last_day() -> None:
    """Use existing extractors/SQL for the last requested calendar day only."""
    from .twap60_training_data import DataPaths, _isolated_query_frame, extract_tournament_sources

    for line in (
        Path("/Users/gunnermadsen/development/polymarket-bot/.env.postgres")
        .read_text()
        .splitlines()
    ):
        if line and not line.startswith("#") and "=" in line:
            key, value = line.split("=", 1)
            os.environ.setdefault(key, value.strip().strip('"').strip("'"))
    dest = CACHE / "september14"
    paths = DataPaths(
        package_root=PACKAGE,
        cache=dest,
        core_features=dest / "unused.parquet",
        core_current_sql=PACKAGE / "sql/btc-twap60-core-current-source.sql",
        oracle_sql=PACKAGE / "sql/btc-core-oracle-source.sql",
        label_sql=PACKAGE / "sql/btc-twap60-label-source.sql",
        refprice_sql=PACKAGE / "sql/btc-twap60-refprice-source.sql",
        candle_sql=PACKAGE / "sql/btc-twap60-candle-source.sql",
        execution_sql=PACKAGE / "sql/btc-current-orderbook-capacity-execution-source.sql",
    )
    day = datetime(2026, 9, 14, tzinfo=UTC)
    extract_tournament_sources(paths, range_start=day, range_end=END, current_start=day)
    for name, sql in [
        ("open-interest", "btc-binance-five-minute-open-interest-source.sql"),
        ("binance-prints", "btc-binance-trade-print-source.sql"),
        ("spot-l2", "btc-middle-strategy-spot-l2-source.sql"),
    ]:
        p = dest / f"{name}.parquet"
        if p.exists():
            continue
        source = _isolated_query_frame(
            (PACKAGE / "sql" / sql).read_text(),
            {
                "batch_start": day,
                "batch_end": END,
                "range_start": day,
                "range_end": END,
                "history_minutes": 120,
                "open_interest_symbol": "BTCUSDT",
            },
            cursor_name="micro_edge_" + name.replace("-", "_"),
        )
        _write_parquet(source, p)
        print(name, source.height, flush=True)


def attach_vwap5(panel: pl.DataFrame, books: pl.DataFrame) -> pl.DataFrame:
    """Preserve prediction rows; independently validate each side's five-share evidence."""
    columns = ["fee_rate", "quality_flags"] + [
        f"{side}_{suffix}"
        for side in ("up", "down")
        for suffix in ("provider_received_at", "ask_vwap_5", "ask_depth", "best_ask")
    ]
    if books.is_empty():
        return panel.with_columns(
            *(
                pl.lit(None, dtype=pl.Float64).alias(c)
                for c in [
                    "fee_rate",
                    "up_ask_vwap_5",
                    "down_ask_vwap_5",
                    "pm_up_book_age_seconds",
                    "pm_down_book_age_seconds",
                ]
            )
        )
    books = books.unique(["market_id", "observed_at"], keep="last")
    frame = panel.join(
        books.select("market_id", "observed_at", *columns),
        on=["market_id", "observed_at"],
        how="left",
        validate="1:1",
    )
    for side in ("up", "down"):
        age = (
            pl.col("observed_at") - pl.col(f"{side}_provider_received_at")
        ).dt.total_microseconds() / 1e6
        valid = (
            ((pl.col("quality_flags").fill_null(63) & 63) == 0)
            & age.is_between(0, 2)
            & (pl.col(f"{side}_ask_vwap_5").is_between(0, 1, closed="none"))
            & (pl.col(f"{side}_ask_depth") >= 5)
        )
        frame = frame.with_columns(
            pl.when(valid)
            .then(pl.col(f"{side}_ask_vwap_5"))
            .otherwise(None)
            .alias(f"{side}_ask_vwap_5"),
            pl.when(valid).then(age).otherwise(None).alias(f"pm_{side}_book_age_seconds"),
        )
    return frame


def causal_twap(panel: pl.DataFrame, source: pl.DataFrame) -> pl.DataFrame:
    """Past-only piecewise-average sensors; never completed settlement targets."""
    names = ["twap30_margin_bps", "twap60_margin_bps"]
    if source.height < 2:
        return panel.with_columns(*(pl.lit(None, dtype=pl.Float64).alias(n) for n in names))
    path = source.filter(
        pl.col("price").is_finite()
        & (pl.col("price") > 0)
        & (pl.col("source_timestamp") <= pl.col("provider_available_at"))
    ).sort(["provider_available_at", "source_timestamp", "archive_row_number"])
    path = (
        path.filter(pl.col("source_timestamp") == pl.col("source_timestamp").cum_max())
        .unique("source_timestamp", keep="first")
        .sort("source_timestamp")
    )
    if path.height < 2:
        return panel.with_columns(*(pl.lit(None, dtype=pl.Float64).alias(n) for n in names))
    src = path["source_timestamp"].to_numpy().astype("datetime64[us]").astype("int64")
    avail = path["provider_available_at"].to_numpy().astype("datetime64[us]").astype("int64")
    price = path["price"].to_numpy()
    dec = panel["observed_at"].to_numpy().astype("datetime64[us]").astype("int64")
    opening = panel["window_start"].to_numpy().astype("datetime64[us]").astype("int64")
    idx = np.searchsorted(avail, dec, side="left") - 1
    safe = np.maximum(idx, 0)
    cumulative = np.r_[0.0, np.cumsum(price[:-1] * np.diff(src))]

    def integral(points):
        k = np.minimum(np.searchsorted(src, points, side="right") - 1, idx)
        j = np.maximum(k, 0)
        return np.where(k >= 0, cumulative[j] + price[j] * (points - src[j]), np.nan)

    base = (integral(opening) - integral(opening - 60_000_000)) / 60_000_000
    valid = (idx >= 0) & (dec - src[safe] > 0) & (dec - src[safe] <= 5_000_000) & (base > 0)
    for seconds, name in zip((30, 60), names, strict=True):
        value = (integral(dec) - integral(dec - seconds * 1_000_000)) / (seconds * 1_000_000)
        with np.errstate(invalid="ignore", divide="ignore"):
            margin = np.log(value / base) * 10000
        panel = panel.with_columns(
            pl.Series(name, np.where(valid & np.isfinite(margin), margin, np.nan)).fill_nan(None)
        )
    return panel


def prepare(*, rebuild_stale: bool = False) -> None:
    CACHE.mkdir(parents=True, exist_ok=True)
    manifest = json.loads(
        (BASE / "btc-time-bucket-early-source-20260321-20260914/source-manifest.json").read_text()
    )
    groups = {
        g: {
            Path(r["path"]).stem: {**r, "path": str(resolve_source(r["path"]))}
            for r in rows
            if "2026-06-07" <= Path(r["path"]).stem < "2026-09-14"
        }
        for g, rows in manifest["partitions"].items()
    }
    tail_root = BASE / "btc-full-coverage-vwap-admission-tail-20260827-20260914"
    tail_manifest = tail_root / "source-manifest.json"
    tail = json.loads(tail_manifest.read_text())
    # The early manifest retains a truncated August 27 partition. Prefer the
    # existing fuller source independently per group, preserving rare RefPrice rows.
    for group, records in tail["partitions"].items():
        for record in records:
            key = Path(record["path"]).stem
            prior = groups[group].get(key)
            if prior is None or record["rows"] > prior["rows"]:
                groups[group][key] = {**record, "path": str(tail_root / record["path"])}
    last = json.loads((CACHE / "september14/source-manifest.json").read_text())
    for g, rows in last["partitions"].items():
        for r in rows:
            groups[g][Path(r["path"]).stem] = {**r, "path": str(CACHE / "september14" / r["path"])}
    # Existing panel supplies all previously engineered optional dimensions on exact causal keys.
    base_path = (
        BASE / "btc-full-coverage-vwap-admission-20260321-20260914/vwap-admission-panel.parquet"
    )
    base_manifest = json.loads(
        base_path.with_name("vwap-admission-panel-manifest.json").read_text()
    )
    base_groups = base_manifest["feature_groups"]
    optional_groups = ["open_interest", "binance_prints", "kraken", "spot_l2", "kraken_l2"]
    optional = list(dict.fromkeys(c for g in optional_groups for c in base_groups[g]))
    extra_execution = [
        c
        for c in base_groups["execution"]
        if c
        not in [
            "up_ask_vwap_5",
            "down_ask_vwap_5",
            "pm_up_book_age_seconds",
            "pm_down_book_age_seconds",
        ]
    ]
    optional = list(dict.fromkeys([c for c in optional if c != "has_kraken_l2"] + extra_execution))
    base_scan = pl.scan_parquet(base_path)
    identity = {
        "source_manifest": file_sha256(
            BASE / "btc-time-bucket-early-source-20260321-20260914/source-manifest.json"
        ),
        "base_panel": file_sha256(base_path),
        "tail_source_manifest": file_sha256(tail_manifest),
        "last_day_manifest": file_sha256(CACHE / "september14/source-manifest.json"),
        "preparation_code": file_sha256(Path(__file__)),
        "enrichment_code": file_sha256(Path(__file__).with_name("micro_edge_sources.py")),
    }
    from .micro_edge_sources import OptionalSources

    sources = OptionalSources(BASE, CACHE)
    coverage = []
    partitions = []
    feature_groups = {}
    day = START
    while day < END:
        key = day.date().isoformat()
        dest = CACHE / "daily" / f"{key}.parquet"
        meta = dest.with_suffix(".json")
        if dest.exists() and meta.exists():
            saved = json.loads(meta.read_text())
            if saved["sha256"] != file_sha256(dest):
                raise RuntimeError("corrupt daily checkpoint " + key)
            if saved["identity"] == identity:
                coverage.extend(saved["coverage"])
                partitions.append(saved)
                feature_groups = saved["feature_groups"]
                day += timedelta(days=1)
                continue
            if not rebuild_stale:
                raise RuntimeError(
                    "stale daily checkpoint " + key + "; explicitly rebuild derived data"
                )

        def load(g, key=key):
            r = groups[g].get(key)
            if r is None:
                return pl.DataFrame()
            p = Path(r["path"])
            if file_sha256(p) != r["sha256"]:
                raise RuntimeError("source hash mismatch " + str(p))
            return pl.read_parquet(p)

        raw = (
            load("core_current")
            .unique(["market_id", "observed_at"], keep="last")
            .sort(["market_id", "seconds_elapsed"])
        )
        if raw.is_empty():
            day += timedelta(days=1)
            continue
        # Earliest raw observation supplies market opening; require second zero for path meaning.
        starts = raw.filter(pl.col("seconds_elapsed") == 0).select(
            "market_id", pl.col("btc_open").alias("opening_boundary")
        )
        raw = raw.drop("opening_boundary").join(starts, on="market_id", how="inner")
        oracle = load("oracle")
        core = derive_core_point_in_time_features(
            raw
            if oracle.is_empty()
            else attach_causal_oracle_rounds(raw, prepare_causal_oracle_rounds(oracle))
        )
        if not oracle.is_empty():
            core = derive_oracle_point_in_time_features(core)
            core = _mask_optional_features(
                core,
                tuple(c for c in ORACLE_FEATURES if c != "early_oracle_eligible"),
                "oracle_model_eligible",
            )
        panel = core.filter(
            (pl.col("seconds_elapsed") % 5 == 0) & pl.col("seconds_elapsed").is_between(0, 295)
        )
        panel = panel.with_columns(
            (pl.col("seconds_elapsed") / 300.0).alias("seconds_elapsed_scaled"),
            ((300 - pl.col("seconds_elapsed")) / 300.0).alias("seconds_remaining_scaled"),
        )
        ref = load("refprice")
        if not ref.is_empty():
            panel = attach_causal_refprice_features(panel, ref)
            panel = _mask_optional_features(
                panel,
                tuple(c for c in panel.columns if c.startswith("chainlink_ref_")),
                "refprice_causal_eligible",
            )
        panel = causal_twap(panel, ref)
        candles = load("candles")
        if not candles.is_empty():
            panel = _preserving_feature_join(
                panel,
                attach_candle_context(panel, candles.unique("close_timestamp", keep="last")),
                tuple(CHAINLINK_FEATURES),
            )
        panel = attach_vwap5(panel, load("execution"))
        prior = (
            base_scan.filter(
                pl.col("window_start").is_between(day, day + timedelta(days=1), closed="left")
            )
            .select(*KEY_COLUMNS, *optional)
            .collect()
        )
        if prior.height:
            panel = panel.join(prior, on=list(KEY_COLUMNS), how="left", validate="1:1")
        else:
            panel = panel.with_columns(*(pl.lit(None, dtype=pl.Float64).alias(c) for c in optional))
        panel = sources.attach(panel, day)
        feature_groups = {**base_groups, "twap": ["twap30_margin_bps", "twap60_margin_bps"]}
        feature_groups["execution"] = list(
            dict.fromkeys(
                extra_execution
                + [
                    "up_ask_vwap_5",
                    "down_ask_vwap_5",
                    "pm_up_book_age_seconds",
                    "pm_down_book_age_seconds",
                    "pm_vwap5_overround",
                    "pm_depth_imbalance",
                    "pm_up_depth_log",
                    "pm_down_depth_log",
                ]
            )
        )
        panel = panel.with_columns(
            (pl.col("up_ask_vwap_5") + pl.col("down_ask_vwap_5") - 1).alias("pm_vwap5_overround")
        )
        # Preserve available larger-depth measurements as input context, never as a fill requirement.
        if "up_ask_depth" in panel.columns:
            panel = panel.with_columns(
                (
                    (pl.col("up_ask_depth") - pl.col("down_ask_depth"))
                    / (pl.col("up_ask_depth") + pl.col("down_ask_depth"))
                ).alias("pm_depth_imbalance"),
                pl.col("up_ask_depth").log1p().alias("pm_up_depth_log"),
                pl.col("down_ask_depth").log1p().alias("pm_down_depth_log"),
            )
        for g, cols in feature_groups.items():
            cols = [c for c in cols if not c.startswith("has_")]
            feature_groups[g] = cols
            for c in cols:
                if c not in panel.columns:
                    panel = panel.with_columns(pl.lit(None, dtype=pl.Float64).alias(c))
            if g == "kraken_l2":
                # Existing updater does not enforce an age bound itself.
                panel = panel.with_columns(
                    *(
                        pl.when(pl.col("kraken_l2_age_seconds").is_between(0, 2))
                        .then(pl.col(c))
                        .otherwise(None)
                        .alias(c)
                        for c in cols
                    )
                )
            panel = panel.with_columns(
                pl.any_horizontal(pl.col(c).is_finite().fill_null(False) for c in cols).alias(
                    "has_" + g
                )
            )
        panel = panel.with_columns(
            (pl.col("window_start") >= datetime(2026, 8, 14, tzinfo=UTC))
            .cast(pl.Float64)
            .alias("settlement_regime")
        )
        keep = list(
            dict.fromkeys(
                [
                    *KEY_COLUMNS,
                    "window_end",
                    "label_up",
                    "fee_rate",
                    "settlement_regime",
                    "btc_close",
                    *["has_" + g for g in feature_groups],
                    *[c for cols in feature_groups.values() for c in cols],
                ]
            )
        )
        panel = panel.select(keep).sort(["window_start", "market_id", "seconds_elapsed"])
        rows = []
        for g in feature_groups:
            valid = panel.filter("has_" + g)
            rows.append(
                {
                    "day": key,
                    "group": g,
                    "rows": valid.height,
                    "markets": valid["market_id"].n_unique(),
                    "total_rows": panel.height,
                    "total_markets": panel["market_id"].n_unique(),
                }
            )
        _write_parquet(panel, dest)
        saved = {
            "day": key,
            "path": str(dest),
            "sha256": file_sha256(dest),
            "rows": panel.height,
            "markets": panel["market_id"].n_unique(),
            "identity": identity,
            "coverage": rows,
            "feature_groups": feature_groups,
        }
        _write_json(meta, saved)
        partitions.append(saved)
        coverage.extend(rows)
        print("prepared", key, panel.height, flush=True)
        day += timedelta(days=1)
    _write_parquet(pl.DataFrame(coverage), CACHE / "coverage.parquet")
    _write_json(
        CACHE / "panel-manifest.json",
        {
            "source_start": START.isoformat(),
            "source_end_exclusive": END.isoformat(),
            "partitions": partitions,
            "feature_groups": feature_groups,
            "identity": identity,
            "selected_raw_partitions": groups,
            "read_only_sources": True,
            "database_mutations": False,
            "new_sources": False,
        },
    )


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--snapshot", action="store_true")
    parser.add_argument("--rebuild-stale", action="store_true")
    args = parser.parse_args()
    snapshot_last_day() if args.snapshot else prepare(rebuild_stale=args.rebuild_stale)
