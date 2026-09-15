"""Optional source enrichment for this tournament, using existing feature builders."""

import json
from datetime import timedelta
from pathlib import Path

import polars as pl

from .chainlink_oi_features import BINANCE_OI_FEATURES, _attach_open_interest_features
from .kraken_l2_training_data import (
    KRAKEN_L2_FEATURES,
    _raw_files,
    attach_kraken_l2,
    build_kraken_l2_features,
)
from .middle_market_tournament import (
    TRADE_PRINT_FEATURES,
    _derive_trade_print_features,
    _join_trade_print_features,
)
from .multivenue_early_entry_data import KEY_COLUMNS, KRAKEN_FEATURES, _attach_kraken
from .spot_l2_chainlink_features import L2_FEATURES, join_qualified_l2

HISTORY = Path(
    "/Volumes/docker-data/polymarket-bot/model-training-archive/btc-early-entry-economic-tournament/20260902/data"
)


def merge_features(panel, enriched, columns):
    columns = [c for c in columns if c in enriched.columns]
    joined = panel.join(
        enriched.select(*KEY_COLUMNS, *columns),
        on=list(KEY_COLUMNS),
        how="left",
        suffix="_new",
        validate="1:1",
    )
    for c in columns:
        if c in panel.columns:
            joined = joined.with_columns(pl.coalesce(pl.col(c + "_new"), pl.col(c)).alias(c)).drop(
                c + "_new"
            )
    return joined


class OptionalSources:
    def __init__(self, base, cache):
        self.base, self.cache = base, cache
        oi_paths = [
            base / "btc-time-bucket-early-source-20260321-20260914/open-interest.parquet",
            cache / "september14/open-interest.parquet",
        ]
        self.oi = pl.concat(
            [pl.read_parquet(p) for p in oi_paths if p.exists()], how="diagonal_relaxed"
        ).unique("source_timestamp", keep="last")
        self.kraken_paths = [
            HISTORY / "btc-multivenue-early-entry-20260321-20260828/kraken-features.parquet",
            base
            / "btc-full-coverage-vwap-admission-tail-20260827-20260914/kraken-features.parquet",
        ]
        self.l2_cache = HISTORY / "btc-hybrid-payoff-admission-20260321-20260827"
        m = json.loads((self.l2_cache / "kraken-l2-manifest.json").read_text())
        self.l2 = {
            r["day"]: {**r, "path": str(self.l2_cache / "kraken-l2-daily" / Path(r["path"]).name)}
            for r in m["partitions"]
        }
        self.spot_root = HISTORY / "btc-middle-strategy-tournament-20260321-20260828"
        self.print_roots = [
            HISTORY / "btc-multivenue-early-entry-20260321-20260828/binance-prints",
            base / "btc-full-coverage-vwap-admission-tail-20260827-20260914/binance-prints",
        ]

    def attach(self, panel, day):
        key = day.date().isoformat()
        bare = lambda cols: panel.drop([c for c in cols if c in panel.columns])
        oi = self.oi.filter(
            pl.col("source_timestamp").is_between(
                day - timedelta(hours=2), day + timedelta(days=1), closed="left"
            )
        )
        if oi.height:
            x = _attach_open_interest_features(bare(BINANCE_OI_FEATURES), oi, max_age_seconds=600)
            panel = merge_features(panel, x, BINANCE_OI_FEATURES)
        prints = []
        for root in self.print_roots:
            for d in [day - timedelta(days=1), day]:
                p = root / f"{d.date()}.parquet"
                if p.exists():
                    x = pl.read_parquet(p)
                    if x.height:
                        prints.append(x)
        if key == "2026-09-14" and (self.cache / "september14/binance-prints.parquet").exists():
            x = pl.read_parquet(self.cache / "september14/binance-prints.parquet")
            if x.height:
                prints.append(x)
        if prints:
            x = _join_trade_print_features(
                bare(TRADE_PRINT_FEATURES),
                _derive_trade_print_features(pl.concat(prints, how="diagonal_relaxed").unique()),
            )
            panel = merge_features(panel, x, TRADE_PRINT_FEATURES)
        ks = []
        for p in self.kraken_paths:
            x = (
                pl.scan_parquet(p)
                .filter(
                    pl.col("kraken_available_at").is_between(
                        day - timedelta(minutes=3), day + timedelta(days=1), closed="left"
                    )
                )
                .collect()
            )
            if x.height:
                ks.append(x)
        if ks:
            x = _attach_kraken(
                bare(KRAKEN_FEATURES),
                pl.concat(ks, how="diagonal_relaxed").unique("kraken_available_at", keep="last"),
            )
            panel = merge_features(panel, x, KRAKEN_FEATURES)
        spot = self.spot_root / "spot-l2" / f"{key}.parquet"
        if key == "2026-09-14":
            spot = self.cache / "september14/spot-l2.parquet"
        if spot.exists():
            source = pl.read_parquet(spot)
            if source.height:
                x = join_qualified_l2(
                    panel.select(*KEY_COLUMNS, "btc_close"),
                    source.unique("second_start", keep="last").sort("available_at"),
                )
                panel = merge_features(panel, x, L2_FEATURES)
        record = self.l2.get(key)
        raw = Path("/Volumes/docker-data/kraken-data/spot-l2/cryptohftdata/kraken_spot")
        if (record is None or not Path(record["path"]).exists()) and _raw_files(
            raw, day, day + timedelta(days=1)
        ):
            _, m = build_kraken_l2_features(
                raw_root=raw, cache=self.cache / "kraken-l2", start=day, end=day + timedelta(days=1)
            )
            record = m["partitions"][0]
        if record and Path(record["path"]).exists():
            x = attach_kraken_l2(panel.select(*KEY_COLUMNS, "btc_close"), {"partitions": [record]})
            panel = merge_features(panel, x, KRAKEN_L2_FEATURES)
        return panel
