"""Join six independently validated offline flow products at causal decision times."""

from __future__ import annotations

import argparse
import json
from datetime import UTC, date, datetime, timedelta
from pathlib import Path

import numpy as np
import polars as pl

from .time_bucket_flow_sources import CANDLE_VALUES, L2_VALUES, PRODUCTS, _spec
from .time_bucket_panels import finite, points
from .time_bucket_protocol import config
from .time_bucket_source_audit import checked_run, sha256, write_json

CANDLES = "binance_spot_one_second_ohlcv"
PRINTS = "binance_spot_aggregate_trades"
SNAPSHOTS = "binance_spot_l2_snapshots"
OI = "binance_futures_open_interest"
FIELDS = {
    CANDLES: [*CANDLE_VALUES, "trade_count", "taker_imbalance"] + [
        f"{field}_{horizon}s" for horizon in (30, 60)
        for field in ("return_bps", "base_volume", "quote_volume")
    ],
    PRINTS: ["price", "quantity", "buyer_maker", "return_5s_bps", "return_30s_bps"],
    SNAPSHOTS: ["midpoint", "spread_bps", "bid_depth", "ask_depth", "imbalance"],
    "binance_spot_l2_features": list(L2_VALUES),
    "binance_futures_l2_features": list(L2_VALUES),
    OI: ["sum_open_interest", "sum_open_interest_value", "change_5m", "value_change_5m"],
}
FEATURES = {product: [f"{product}_{field}" for field in fields] for product, fields in FIELDS.items()}
TIME = pl.Datetime("us", "UTC")
TRACE_FIELDS = ["source_timestamp", "available_at", "received_at", "provider_available_at",
                "provider_received_at", "stored_available_at"]
HISTORY_TRACE_FIELDS = {
    CANDLES: [f"window_{h}s_{field}" for h in (30, 60)
              for field in ("start", "end", "max_available_at")],
    PRINTS: [f"return_{h}s_{field}" for h in (5, 30)
             for field in ("source_timestamp", "available_at")],
    OI: ["previous_5m_source_timestamp", "previous_5m_available_at"],
}


def _micros(series: pl.Series) -> np.ndarray:
    return series.cast(TIME).cast(pl.Int64).to_numpy()


def _lookup(raw: pl.DataFrame, query: np.ndarray, age: float) -> tuple[np.ndarray, np.ndarray]:
    """Latest source state already available, without letting a late older row erase it."""
    source = _micros(raw["source_timestamp"])
    available = _micros(raw["available_at"])
    monotonic = source >= np.maximum.accumulate(source)
    indices = np.flatnonzero(monotonic)
    positions = np.searchsorted(available[indices], query, side="right") - 1
    safe = indices[np.maximum(positions, 0)]
    elapsed = (query - source[safe]) / 1_000_000
    known = (positions >= 0) & (elapsed >= 0) & (elapsed <= age)
    return safe, known


def _number(raw: pl.DataFrame, name: str) -> np.ndarray:
    return raw[name].cast(pl.Float64).to_numpy()


def _time_trace(name: str, values: np.ndarray, known: np.ndarray) -> pl.Series:
    return pl.Series(name, values, dtype=pl.Int64).cast(TIME).set(
        pl.Series(~known), None
    )


def _rolling_candles(
    raw: pl.DataFrame, indices: np.ndarray, known: np.ndarray, query: np.ndarray,
) -> tuple[dict, dict]:
    chronological = raw.with_row_index("_available_index").sort("open_timestamp")
    order = chronological["_available_index"].to_numpy()
    reverse = np.empty(raw.height, dtype=np.int64)
    reverse[order] = np.arange(raw.height)
    current = reverse[indices]
    times = _micros(chronological["open_timestamp"])
    availability = _micros(chronological["available_at"])
    features, traces = {}, {}
    for horizon in (30, 60):
        valid = known & (current >= horizon - 1)
        begin = np.maximum(current - horizon + 1, 0)
        window_available = np.zeros(raw.height, dtype=np.int64)
        if raw.height >= horizon:
            windows = np.lib.stride_tricks.sliding_window_view(availability, horizon)
            window_available[horizon - 1:] = np.max(windows, axis=1)
        valid &= (times[current] - times[begin]) == (horizon - 1) * 1_000_000
        # Unique native one-second opens plus exact count/endpoints establish contiguity.
        valid &= window_available[current] <= query
        value = (_number(chronological, "close_price")[current]
                 / _number(chronological, "open_price")[begin] - 1) * 10_000
        features[f"return_bps_{horizon}s"] = np.where(valid, value, np.nan)
        for field in ("base_volume", "quote_volume"):
            cumulative = np.r_[0.0, np.cumsum(_number(chronological, field))]
            total = cumulative[current + 1] - cumulative[begin]
            features[f"{field}_{horizon}s"] = np.where(valid, total, np.nan)
        traces[f"window_{horizon}s_start"] = (times[begin], valid)
        traces[f"window_{horizon}s_end"] = (times[current] + 1_000_000, valid)
        traces[f"window_{horizon}s_max_available_at"] = (window_available[current], valid)
    return features, traces


def _snapshot_features(raw: pl.DataFrame) -> pl.DataFrame:
    def observed(row: dict) -> dict:
        bids, asks = json.loads(row["bids"]), json.loads(row["asks"])
        bid, ask = float(bids[0][0]), float(asks[0][0])
        bid_depth = sum(float(level[1]) for level in bids)
        ask_depth = sum(float(level[1]) for level in asks)
        midpoint = (bid + ask) / 2
        return {"midpoint": midpoint, "spread_bps": (ask - bid) / midpoint * 10_000,
                "bid_depth": bid_depth, "ask_depth": ask_depth,
                "imbalance": (bid_depth - ask_depth) / (bid_depth + ask_depth)}

    return raw.select("bids", "asks").with_columns(
        pl.struct("bids", "asks").map_elements(
            observed, return_dtype=pl.Struct({name: pl.Float64 for name in FIELDS[SNAPSHOTS]})
        ).alias("_features")
    ).unnest("_features")


def flow_states(query: pl.DataFrame, raw: pl.DataFrame, product: str, frozen: dict) -> pl.DataFrame:
    """Preserve every decision; absent product evidence leaves only that product null."""
    if product not in PRODUCTS:
        raise ValueError(f"Unsupported flow product: {product}")
    n = query.height
    result = pl.DataFrame({name: pl.Series([None] * n, dtype=pl.Float64) for name in FEATURES[product]})
    metadata = {
        f"{product}_{name}": pl.Series([None] * n, dtype=TIME)
        for name in TRACE_FIELDS + HISTORY_TRACE_FIELDS.get(product, [])
    }
    for name in ["product", "source", "native_identity", "validated_path", "frozen_file_index"]:
        metadata[f"{product}_{name}"] = pl.Series([None] * n, dtype=pl.String)
    if not raw.height:
        return result.with_columns(pl.Series(name, value) for name, value in metadata.items())
    keys = _spec(product)[1]
    required = {"source_timestamp", "available_at", *keys}
    if required - set(raw.columns):
        raise ValueError(f"{product}: normalized source lacks {sorted(required - set(raw.columns))}")
    if raw["source_timestamp"].null_count() or raw["available_at"].null_count():
        raise ValueError("Validated source timestamps must not be missing")
    if "product" in raw.columns and raw.filter(pl.col("product") != product).height:
        raise ValueError("Validated source product identity mismatch")
    if raw.select(pl.struct(keys).n_unique()).item() != raw.height:
        raise ValueError("Validated source native identities must be unique")
    raw = raw.sort(list(dict.fromkeys(["available_at", "source_timestamp", *keys])))
    q = _micros(query["decision_at"])
    age = frozen["open_interest_max_age_seconds"] if product == OI else frozen["flow_max_age_seconds"]
    index, known = _lookup(raw, q, age)
    selected = raw[index]
    values, traces = {}, {}
    if product == CANDLES:
        for name in CANDLE_VALUES + ["trade_count"]:
            values[name] = np.where(known, _number(raw, name)[index], np.nan)
        quote = values["quote_volume"]
        values["taker_imbalance"] = np.divide(
            2 * values["taker_buy_quote_volume"] - quote, quote,
            out=np.full(n, np.nan), where=quote > 0,
        )
        windows, traces = _rolling_candles(raw, index, known, q)
        values.update(windows)
    elif product == PRINTS:
        for name in ("price", "quantity", "buyer_maker"):
            values[name] = np.where(known, _number(raw, name)[index], np.nan)
        for lag in (5, 30):
            previous, previous_known = _lookup(raw, q - lag * 1_000_000, age)
            valid = known & previous_known
            values[f"return_{lag}s_bps"] = np.where(
                valid, (values["price"] / _number(raw, "price")[previous] - 1) * 10_000, np.nan
            )
            traces[f"return_{lag}s_source_timestamp"] = (_micros(raw["source_timestamp"])[previous], valid)
            traces[f"return_{lag}s_available_at"] = (_micros(raw["available_at"])[previous], valid)
    elif product == SNAPSHOTS:
        measured = _snapshot_features(selected)
        values = {name: np.where(known, _number(measured, name), np.nan) for name in FIELDS[product]}
    elif product == OI:
        for name in ("sum_open_interest", "sum_open_interest_value"):
            values[name] = np.where(known, _number(raw, name)[index], np.nan)
        times = _micros(raw["source_timestamp"])
        chronological = np.argsort(times)
        ordered = times[chronological]
        target = times[index] - 300 * 1_000_000
        previous = np.searchsorted(ordered, target)
        safe = chronological[np.minimum(previous, raw.height - 1)]
        valid = known & (previous < raw.height) & (times[safe] == target)
        valid &= (_micros(raw["available_at"])[safe] <= q) & (q - times[safe] <= age * 1_000_000)
        for name, change in [("sum_open_interest", "change_5m"), ("sum_open_interest_value", "value_change_5m")]:
            values[change] = np.where(valid, values[name] - _number(raw, name)[safe], np.nan)
        traces["previous_5m_source_timestamp"] = (times[safe], valid)
        traces["previous_5m_available_at"] = (_micros(raw["available_at"])[safe], valid)
    else:
        values = {name: np.where(known, _number(raw, name)[index], np.nan) for name in L2_VALUES}
    result = pl.DataFrame({f"{product}_{name}": value for name, value in values.items()})
    for name in TRACE_FIELDS:
        if name in selected.columns:
            metadata[f"{product}_{name}"] = selected[name].cast(TIME).set(pl.Series(~known), None)
    for source, target in [("product", "product"), ("source", "source"),
                           ("_validated_path", "validated_path"), ("_frozen_file_index", "frozen_file_index")]:
        if source in selected.columns:
            metadata[f"{product}_{target}"] = selected[source].cast(pl.String).set(pl.Series(~known), None)
    native = [json.dumps(row, sort_keys=True, default=str) if valid else None
              for row, valid in zip(selected.select(keys).to_dicts(), known, strict=True)]
    metadata[f"{product}_native_identity"] = pl.Series(native, dtype=pl.String)
    for name, (timestamps, valid) in traces.items():
        metadata[f"{product}_{name}"] = _time_trace(f"{product}_{name}", timestamps, valid)
    return finite(result.with_columns(pl.Series(name, value) for name, value in metadata.items()))


def _source_day(run: Path, audit: dict, product: str, day: date, verified: set[str]) -> pl.DataFrame:
    frames = []
    start = datetime.combine(day, datetime.min.time(), UTC) - timedelta(seconds=601)
    end = start + timedelta(days=1, seconds=601)
    for record in audit["daily"]:
        if record["date"] not in {str(day - timedelta(days=1)), str(day)}:
            continue
        path = Path(record["output_path"])
        expected = run / "inputs/validated-flow" / product / f"{record['date']}.parquet"
        if path.resolve() != expected.resolve():
            raise ValueError("Flow input lies outside its validated product directory")
        if str(path) not in verified:
            if sha256(path) != record["output_sha256"]:
                raise ValueError(f"Validated flow source changed: {path}")
            verified.add(str(path))
        frames.append(pl.scan_parquet(path).filter(
            (pl.col("source_timestamp") >= start) & (pl.col("source_timestamp") < end)
            & (pl.col("available_at") < end)
        ).with_columns(pl.lit(str(path)).alias("_validated_path")).collect(engine="streaming"))
    return pl.concat(frames, how="diagonal_relaxed") if frames else pl.DataFrame()


def build(run: Path) -> None:
    frozen = config(run)
    audits, audit_hashes = {}, {}
    for product in PRODUCTS:
        path = run / "manifests" / f"flow-source-validation-{product}.json"
        audit = json.loads(path.read_text())
        if audit["status"] != "completed":
            raise ValueError(f"Flow source checkpoint incomplete: {product}")
        audits[product], audit_hashes[product] = audit, sha256(path)
    identity = {"configuration_sha256": sha256(run / "inputs/tournament-freeze.json"),
                "market_identity_sha256": sha256(run / "inputs/market-identities.parquet"),
                "implementation_sha256": sha256(Path(__file__)), "source_audit_sha256": audit_hashes}
    manifest_path = run / "manifests/flow-panels.json"
    if manifest_path.exists():
        manifest = json.loads(manifest_path.read_text())
        if manifest["identity"] != identity:
            raise ValueError("Flow-panel source/configuration evidence changed; refusing overwrite")
    else:
        manifest = {"identity": identity, "status": "in_progress", "days": [], "features": FEATURES,
                    "semantics": "Independent product joins; original availability <= decision; no imputation; contiguous known candles only; no print-volume/quiet inference."}
    population = pl.read_parquet(run / "inputs/market-identities.parquet", columns=["market_id", "window_start", "window_end"])
    done = {record["date"]: record for record in manifest["days"]}
    verified: set[str] = set()
    destination = run / "datasets/flow"
    destination.mkdir(parents=True, exist_ok=True)
    for day in sorted(population["window_start"].dt.date().unique().to_list()):
        output = destination / f"{day}.parquet"
        if str(day) in done:
            if sha256(output) != done[str(day)]["sha256"]:
                raise ValueError(f"Existing flow panel changed: {output}")
            continue
        if output.exists():
            raise ValueError(f"Unowned existing flow panel: {output}")
        query = points(population.filter(pl.col("window_start").dt.date() == day), frozen)
        result = query.select("point_id", "market_id", "decision_at", "window_start")
        coverage = {}
        for product in PRODUCTS:
            raw = _source_day(run, audits[product], product, day, verified)
            state = flow_states(query, raw, product, frozen)
            coverage[product] = state[f"{product}_available_at"].drop_nulls().len()
            result = result.hstack(state)
        temporary = output.with_suffix(".parquet.pending")
        result.write_parquet(temporary, compression="zstd")
        digest = sha256(temporary)
        temporary.rename(output)
        record = {"date": str(day), "path": str(output), "sha256": digest,
                  "rows": result.height, "causal_product_rows": coverage}
        manifest["days"].append(record)
        write_json(manifest_path, manifest)
        print(json.dumps(record), flush=True)
    manifest["status"] = "complete"
    write_json(manifest_path, manifest)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("run", type=Path)
    args = parser.parse_args()
    build(checked_run(args.run))


if __name__ == "__main__":
    main()
