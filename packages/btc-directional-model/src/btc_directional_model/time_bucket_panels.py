"""Source-specific causal price states and offline market/bucket execution panels."""

from __future__ import annotations

import argparse
import json
from datetime import date, timedelta
from pathlib import Path

import numpy as np
import polars as pl

from .fok_retry_backtest import walk
from .time_bucket_execution import at, day_books, levels, order, settlement
from .time_bucket_protocol import config
from .time_bucket_source_audit import PRODUCTS, checked_run, sha256, write_json

LAGS = [0, 1, 5, 10, 15, 20, 25, 30, 45, 60, 90, 120, 180]
PRICE_FIELDS = ["path_bps", "return_1_bps", "return_5_bps", "return_15_bps", "return_30_bps",
                "return_60_bps", "return_120_bps", "return_180_bps", "volatility_30_bps",
                "volatility_120_bps", "range_30_bps", "range_120_bps", "efficiency_30",
                "acceleration_5_30", "reversal_5_30", "volatility_ratio_30_120",
                "range_position_30", "source_age_seconds", "latency_seconds"]
TIME_FIELDS = ["seconds_elapsed_scaled", "hour_sin", "hour_cos", "weekday_sin", "weekday_cos"]
BOOK_FIELDS = [f"{side}_{name}" for side in ["up", "down"] for name in
               ["best_bid", "best_ask", "spread", "log_ask_depth", "imbalance", "book_age_seconds",
                "decision_partial_vwap_5"]]
CAPACITY_FIELDS = [f"{side}_{name}" for side in ["up", "down"] for name in
                   ["vwap_5", "limit_5", "capacity_slope_5_15", "capacity_slope_5_50", "capacity_slope_5_200"]]


def finite(frame: pl.DataFrame) -> pl.DataFrame:
    names = [name for name, dtype in frame.schema.items() if dtype in (pl.Float32, pl.Float64)]
    return frame.with_columns(pl.when(pl.col(name).is_finite()).then(pl.col(name))
                              .otherwise(None).alias(name) for name in names)


def points(population: pl.DataFrame, frozen: dict) -> pl.DataFrame:
    seconds = sorted({b + offset for b in frozen["bucket_starts"] for offset in frozen["entry_offsets"]}
                     | set(frozen["additional_exit_seconds"]))
    result = population.join(pl.DataFrame({"seconds_elapsed": seconds}), how="cross")
    return result.with_columns(
        (pl.col("window_start") + pl.duration(seconds=pl.col("seconds_elapsed"))).alias("decision_at"),
        (pl.col("seconds_elapsed") // 20 * 20).alias("bucket_start"),
        (pl.col("seconds_elapsed") % 20).alias("entry_offset"),
        (pl.col("market_id") + pl.lit("@") + pl.col("seconds_elapsed").cast(pl.String)).alias("point_id"),
        (pl.col("seconds_elapsed") / 300).alias("seconds_elapsed_scaled"),
        (pl.col("window_start").dt.hour() * (2 * np.pi / 24)).sin().alias("hour_sin"),
        (pl.col("window_start").dt.hour() * (2 * np.pi / 24)).cos().alias("hour_cos"),
        (pl.col("window_start").dt.weekday() * (2 * np.pi / 7)).sin().alias("weekday_sin"),
        (pl.col("window_start").dt.weekday() * (2 * np.pi / 7)).cos().alias("weekday_cos"),
    ).sort("decision_at", "market_id")


def daily_source(directory: Path, day: date, columns: list[str], source: str | None = None) -> pl.DataFrame:
    paths = [directory / f"{d}.parquet" for d in [day - timedelta(days=1), day]]
    frames = []
    for path in paths:
        if path.exists():
            frame = pl.scan_parquet(path)
            if source:
                frame = frame.filter(pl.col("source") == source)
            frames.append(frame.select(columns).collect(engine="streaming"))
    return pl.concat(frames, how="diagonal_relaxed") if frames else pl.DataFrame()


def price_states(query: pl.DataFrame, raw: pl.DataFrame, prefix: str, age: float) -> pl.DataFrame:
    """As-of observed prices, never filled candles or source-time-only joins."""
    n = query.height
    values = np.full((n, len(LAGS)), np.nan)
    opening = np.full(n, np.nan)
    source_age, latency = np.full(n, np.nan), np.full(n, np.nan)
    receipt_age = np.full(n, np.nan)
    missing_time = np.iinfo(np.int64).min
    source_trace = np.full((n, len(LAGS) + 1), missing_time, dtype=np.int64)
    availability_trace = source_trace.copy()
    if raw.height:
        raw = raw.select("source_timestamp", "available_at", pl.col("price").cast(pl.Float64))
        raw = raw.sort("available_at", "source_timestamp")
        # A late old observation must not erase a newer already observed price.
        raw = raw.filter(pl.col("source_timestamp") >= pl.col("source_timestamp").cum_max())
        availability = raw["available_at"].cast(pl.Int64).to_numpy()
        timestamps = raw["source_timestamp"].cast(pl.Int64).to_numpy()
        prices = raw["price"].to_numpy()
        q = query["decision_at"].cast(pl.Int64).to_numpy()

        def lookup(times: np.ndarray) -> tuple:
            idx = np.searchsorted(availability, times, side="right") - 1
            safe = np.maximum(idx, 0)
            delta = (times - timestamps[safe]) / 1e6
            known = (idx >= 0) & (delta >= 0) & (delta <= age)
            return np.where(known, prices[safe], np.nan), delta, safe, known

        for i, lag in enumerate(LAGS):
            value, _, idx, known = lookup(q - lag * 1_000_000)
            values[:, i] = value
            source_trace[:, i] = np.where(known, timestamps[idx], missing_time)
            availability_trace[:, i] = np.where(known, availability[idx], missing_time)
        opening, _, idx, known = lookup(query["window_start"].cast(pl.Int64).to_numpy())
        source_trace[:, -1] = np.where(known, timestamps[idx], missing_time)
        availability_trace[:, -1] = np.where(known, availability[idx], missing_time)
        _, delta, idx, known = lookup(q)
        source_age = np.where(known, delta, np.nan)
        latency = np.where(known, (availability[idx] - timestamps[idx]) / 1e6, np.nan)
        receipt_age = np.where(known, (q - availability[idx]) / 1e6, np.nan)
    p = {lag: values[:, i] for i, lag in enumerate(LAGS)}
    features = {"price": p[0], "path_bps": (p[0] / opening - 1) * 10000,
                "source_age_seconds": source_age, "receipt_age_seconds": receipt_age,
                "latency_seconds": latency}
    for lag in [1, 5, 15, 30, 60, 120, 180]:
        features[f"return_{lag}_bps"] = (p[0] / p[lag] - 1) * 10000
    for horizon, sample_lags in [(30, [30, 25, 20, 15, 10, 5, 0]), (120, [120, 90, 60, 30, 0])]:
        samples = np.column_stack([p[lag] for lag in sample_lags])
        returns = np.diff(np.log(samples), axis=1) * 10000
        features[f"volatility_{horizon}_bps"] = np.std(returns, axis=1)
        features[f"range_{horizon}_bps"] = (np.max(samples, axis=1) / np.min(samples, axis=1) - 1) * 10000
        if horizon == 30:
            movement = np.sum(np.abs(np.diff(samples, axis=1)), axis=1)
            features["efficiency_30"] = np.abs(samples[:, -1] - samples[:, 0]) / np.maximum(movement, 1e-12)
            spread = np.max(samples, axis=1) - np.min(samples, axis=1)
            features["range_position_30"] = np.where(spread == 0, 0.5, (p[0] - np.min(samples, axis=1)) / np.maximum(spread, 1e-12))
    r5, r30 = features["return_5_bps"], features["return_30_bps"]
    features["acceleration_5_30"] = r5 - r30 / 6
    features["reversal_5_30"] = -r5 * np.sign(r30)
    features["volatility_ratio_30_120"] = features["volatility_30_bps"] / np.maximum(features["volatility_120_bps"], 1e-9)
    result = pl.DataFrame({f"{prefix}_{name}": value for name, value in features.items()})
    for name, trace in [("source_times", source_trace), ("available_times", availability_trace)]:
        provenance = pl.Series(f"{prefix}_{name}", trace.tolist(), dtype=pl.List(pl.Int64))
        provenance = provenance.list.eval(
            pl.when(pl.element() == missing_time).then(None).otherwise(pl.element())
        ).cast(pl.List(pl.Datetime("us", "UTC")))
        result = result.with_columns(provenance)
    return finite(result)


def book_states(query: pl.DataFrame, index: dict, frozen: dict) -> pl.DataFrame:
    rows = []
    numeric = BOOK_FIELDS + CAPACITY_FIELDS + [f"{side}_{name}" for side in ["up", "down"] for name in
                            ["fak_quantity_5", "fak_price_5", "fak_net_5", "fak_stress_5",
                             "fok_quantity_5", "fok_price_5", "fok_net_5", "fok_stress_5"]]
    for row in query.select("market_id", "decision_at", "official_outcome").iter_rows(named=True):
        state = dict.fromkeys(numeric)
        for side in ["up", "down"]:
            state[f"{side}_book_sha256"] = None
            for name in ["available_at", "source_timestamp", "received_at", "sampled_at", "provider_available_at"]:
                state[f"{side}_book_{name}"] = None
            state[f"{side}_vwap_grid"] = [None] * len(frozen["quantity_grid"])
            book = at(index, row["market_id"], side, row["decision_at"], frozen["book"]["max_age_seconds"])
            if book:
                state[f"{side}_book_sha256"] = book.book_sha256
                for name in ["available_at", "source_timestamp", "received_at", "sampled_at", "provider_available_at"]:
                    state[f"{side}_book_{name}"] = getattr(book, name)
                asks, bids = levels(book.asks), levels(book.bids)
                ask_depth, bid_depth = sum(q for _, q in asks), sum(q for _, q in bids)
                grid = [walk(asks, q) for q in frozen["quantity_grid"]]
                prices = {q: None if v is None else v[0] for q, v in zip(frozen["quantity_grid"], grid, strict=True)}
                partial = walk(asks, min(5, ask_depth))
                state.update({f"{side}_best_ask": asks[0][0], f"{side}_best_bid": bids[-1][0],
                              f"{side}_spread": asks[0][0] - bids[-1][0],
                              f"{side}_log_ask_depth": np.log1p(ask_depth),
                              f"{side}_imbalance": (bid_depth - ask_depth) / (bid_depth + ask_depth),
                              f"{side}_book_age_seconds": (row["decision_at"] - book.source_timestamp).total_seconds(),
                              f"{side}_decision_partial_vwap_5": partial[0], f"{side}_limit_5": partial[1],
                              f"{side}_vwap_5": prices[5], f"{side}_vwap_grid": list(prices.values())})
                for q in [15, 50, 200]:
                    state[f"{side}_capacity_slope_5_{q}"] = None if prices[q] is None or prices[5] is None else prices[q] - prices[5]
            for kind in ["FAK", "FOK"]:
                execution = order(index, row["market_id"], side, row["decision_at"], 5, kind, frozen)
                key = f"{side}_{kind.lower()}"
                state[f"{key}_evidence_known"] = execution["evidence_known"]
                state[f"{key}_reason"] = execution["reason"]
                if execution["evidence_known"]:
                    state[f"{key}_quantity_5"] = execution["filled_quantity"]
                    state[f"{key}_price_5"] = execution["share_price"]
                    economics = settlement(execution, side == row["official_outcome"])
                    state[f"{key}_net_5"] = economics["net_pnl"]
                    state[f"{key}_stress_5"] = economics["stress_pnl"]
        rows.append(state)
    schema = {name: pl.Float64 for name in numeric}
    for side in ["up", "down"]:
        schema[f"{side}_book_sha256"] = pl.String
        for name in ["available_at", "source_timestamp", "received_at", "sampled_at", "provider_available_at"]:
            schema[f"{side}_book_{name}"] = pl.Datetime("us", "UTC")
        schema[f"{side}_vwap_grid"] = pl.List(pl.Float64)
        for kind in ["fak", "fok"]:
            schema[f"{side}_{kind}_evidence_known"] = pl.Boolean
            schema[f"{side}_{kind}_reason"] = pl.String
    return pl.DataFrame(rows, schema=schema)


def core_day(run: Path, population: pl.DataFrame, day: date, frozen: dict) -> dict:
    query = points(population.filter(pl.col("window_start").dt.date() == day), frozen)
    tick_root = run / "inputs/validated-core/rtds_reference_price_ticks"
    columns = ["source_timestamp", "available_at", "price"]
    for source in ["rtds_chainlink", "direct_binance"]:
        raw = daily_source(tick_root, day, columns, source)
        query = query.hstack(price_states(query, raw, source, frozen["price_max_age_seconds"]))
    index = day_books(run, day, query["market_id"].unique().to_list())
    query = query.hstack(book_states(query, index, frozen))
    query = query.with_columns((pl.col("official_outcome") == "up").cast(pl.Int8).alias("label_up"))
    output = run / "datasets/core" / f"{day}.parquet"
    output.parent.mkdir(parents=True, exist_ok=True)
    query.write_parquet(output, compression="zstd")
    levels.cache_clear()
    return {"date": str(day), "path": str(output), "sha256": sha256(output), "rows": query.height,
            "rtds_price_present": query["rtds_chainlink_price"].drop_nulls().len(),
            "up_execution_known": query["up_fak_evidence_known"].sum(),
            "down_execution_known": query["down_fak_evidence_known"].sum()}


def reference_day(run: Path, query: pl.DataFrame, day: date, frozen: dict) -> dict:
    result = query.select("point_id", "market_id", "decision_at", "window_start")
    for product in PRODUCTS:
        path = run / "inputs/validated-sources" / product
        twap = product.endswith("twap")
        columns = ["source_timestamp", "available_at", "twap_price" if twap else "price"] + (["window_seconds"] if twap else [])
        raw = daily_source(path, day, columns)
        for window in [30, 60] if twap else [None]:
            selected = raw
            prefix = f"{product}_{window}" if window else product
            if twap and raw.height:
                selected = raw.filter(pl.col("window_seconds") == window).rename({"twap_price": "price"})
            state = price_states(query, selected, prefix, frozen["price_max_age_seconds"])
            keep = [f"{prefix}_{k}" for k in ["price", "path_bps", "return_30_bps", "source_age_seconds", "receipt_age_seconds", "latency_seconds", "source_times", "available_times"]]
            result = result.hstack(state.select(keep))
    output = run / "datasets/refprice-twap" / f"{day}.parquet"
    output.parent.mkdir(parents=True, exist_ok=True)
    result.write_parquet(output, compression="zstd")
    return {"date": str(day), "path": str(output), "sha256": sha256(output), "rows": result.height}


def build(run: Path, layer: str) -> None:
    frozen = config(run)
    population = pl.read_parquet(run / "inputs/market-identities.parquet").join(
        pl.read_parquet(run / "inputs/chronological-market-roles.parquet"),
        on=["market_id", "window_start", "window_end"], how="inner", validate="1:1")
    manifest_path = run / "manifests" / f"{layer}-panels.json"
    sources = ([f"core-source-validation-{p}.json" for p in ["rtds_reference_price_ticks", "polymarket_clob_l2"]]
               if layer == "core" else [f"{p}-audit.json" for p in PRODUCTS])
    identity = {
        "code": {name: sha256(Path(__file__).with_name(name)) for name in [
            "time_bucket_panels.py", "time_bucket_execution.py", "time_bucket_protocol.py", "core_execution.py", "fok_retry_backtest.py"]},
        "inputs": {name: sha256(run / "inputs" / name) for name in [
            "market-identities.parquet", "chronological-market-roles.parquet", "tournament-freeze.json"]},
        "sources": {name: sha256(run / "manifests" / name) for name in sources},
    }
    manifest = json.loads(manifest_path.read_text()) if manifest_path.exists() else {
        "layer": layer, "configuration_sha256": sha256(run / "inputs/tournament-freeze.json"),
        "implementation_sha256": sha256(Path(__file__)), "identity": identity, "days": [], "status": "in_progress"}
    if manifest.get("identity") != identity:
        raise ValueError("Panel implementation changed; existing outputs need explicit compatibility review")
    done = {x["date"]: x for x in manifest["days"]}
    for day in sorted(population["window_start"].dt.date().unique().to_list()):
        if str(day) in done:
            if sha256(Path(done[str(day)]["path"])) != done[str(day)]["sha256"]:
                raise ValueError("Existing panel checksum mismatch")
            continue
        if layer == "core":
            record = core_day(run, population, day, frozen)
        else:
            query = points(population.filter(pl.col("window_start").dt.date() == day), frozen)
            record = reference_day(run, query, day, frozen)
        manifest["days"].append(record)
        write_json(manifest_path, manifest)
        print(json.dumps(record), flush=True)
    manifest["status"] = "complete"
    write_json(manifest_path, manifest)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("run", type=Path)
    parser.add_argument("--layer", choices=["core", "refprice-twap"], required=True)
    args = parser.parse_args()
    run = checked_run(args.run)
    if args.layer == "core":
        for product in ["rtds_reference_price_ticks", "polymarket_clob_l2"]:
            audit = json.loads((run / "manifests" / f"core-source-validation-{product}.json").read_text())
            if audit.get("status") != "complete":
                raise ValueError(f"Source audit is not complete: {product}")
    else:
        status = json.loads((run / "manifests/run.json").read_text())
        if status["checkpoints"]["source_freeze"]["status"] != "accepted":
            raise ValueError("Mandatory reference source checkpoint is not accepted")
        for product in PRODUCTS:
            report = json.loads((run / "manifests" / f"{product}-audit.json").read_text())
            if not report.get("outputs") or sum(x["rows"] for x in report["outputs"]) <= 0:
                raise ValueError(f"Mandatory validated source output missing: {product}")
            if any(x["identity_conflicts"] for x in report["daily"]):
                raise ValueError(f"Unresolved native source conflicts: {product}")
    build(run, args.layer)


if __name__ == "__main__":
    main()
