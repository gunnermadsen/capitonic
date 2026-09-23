"""Exact causal FOK replay with continued inference after definitive rejection."""

from __future__ import annotations

import argparse
import bisect
import hashlib
import json
import subprocess
import tomllib
from collections import defaultdict
from dataclasses import dataclass
from datetime import UTC, datetime, timedelta
from decimal import Decimal
from pathlib import Path
from typing import Any

import polars as pl

from .conservative_selective_training import trade_metrics
from .core_extract import database_connection


QUANTITIES = (5, 10, 15, 20, 25, 30, 40, 50, 75, 100, 125, 150, 175, 200)


@dataclass(frozen=True)
class Scenario:
    name: str
    latency_ms: int
    depth_haircut: float


@dataclass(frozen=True)
class Book:
    received_at: datetime
    source_timestamp: datetime
    asks: tuple[tuple[float, float], ...]
    token_id: str


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def parse_asks(value: Any) -> tuple[tuple[float, float], ...]:
    if isinstance(value, str):
        value = json.loads(value)
    if not value:
        return ()
    levels = []
    for level in value:
        price, size = float(level[0]), float(level[1])
        if 0 < price <= 1 and size > 0:
            levels.append((price, size))
    return tuple(sorted(levels))


def walk(asks: tuple[tuple[float, float], ...], quantity: float, haircut: float = 1.0,
         limit: float | None = None) -> tuple[float, float, float] | None:
    remaining = quantity
    notional = 0.0
    worst = 0.0
    displayed = 0.0
    for price, size in asks:
        if limit is not None and price > limit + 1e-12:
            continue
        displayed += size
        available = size * haircut
        fill = min(remaining, available)
        if fill > 0:
            notional += fill * price
            worst = price
            remaining -= fill
        if remaining <= 1e-9:
            return notional / quantity, worst, displayed
    return None


def select_book(books: list[Book], at: datetime, max_age_ms: int) -> Book | None:
    if not books:
        return None
    timestamps = [book.received_at for book in books]
    index = bisect.bisect_right(timestamps, at) - 1
    if index < 0:
        return None
    book = books[index]
    maximum_age = timedelta(milliseconds=max_age_ms)
    if book.received_at > at or at - book.received_at > maximum_age:
        return None
    if book.source_timestamp > at or at - book.source_timestamp > maximum_age:
        return None
    if book.received_at - book.source_timestamp > maximum_age:
        return None
    return book


def fok_attempt(decision_book: Book | None, arrival_book: Book | None, quantity: int,
                max_price: float, haircut: float, participation: float) -> dict[str, Any]:
    if decision_book is None:
        return {"filled": False, "reason": "missing_decision_book"}
    decision = walk(decision_book.asks, quantity)
    if decision is None:
        return {"filled": False, "reason": "insufficient_decision_depth"}
    decision_vwap, limit_price, _ = decision
    if limit_price > max_price + 1e-12:
        return {
            "filled": False, "reason": "marketable_limit_above_maximum",
            "decision_vwap": decision_vwap, "limit_price": limit_price,
        }
    if arrival_book is None:
        return {
            "filled": False, "reason": "missing_arrival_book",
            "decision_vwap": decision_vwap, "limit_price": limit_price,
        }
    displayed = sum(size for price, size in arrival_book.asks if price <= limit_price + 1e-12)
    arrival = walk(arrival_book.asks, quantity, haircut=haircut, limit=limit_price)
    if arrival is not None and quantity > displayed * participation + 1e-9:
        return {
            "filled": False, "reason": "arrival_depth_participation_exceeded",
            "decision_vwap": decision_vwap, "limit_price": limit_price,
            "displayed_size_at_limit": displayed,
        }
    if arrival is None:
        return {
            "filled": False, "reason": "insufficient_arrival_depth",
            "decision_vwap": decision_vwap, "limit_price": limit_price,
            "displayed_size_at_limit": displayed,
        }
    arrival_vwap, _, _ = arrival
    return {
        "filled": True, "reason": "filled", "decision_vwap": decision_vwap,
        "arrival_vwap": arrival_vwap, "limit_price": limit_price,
        "displayed_size_at_limit": displayed,
    }


def _archive_day(root: Path, day: datetime, market_ids: list[str]) -> pl.DataFrame:
    paths = sorted((root / f"date={day.date().isoformat()}").glob("hour=*.parquet"))
    if not paths:
        return pl.DataFrame()
    return (
        pl.scan_parquet(paths, hive_partitioning=False)
        .filter(pl.col("market_id").is_in(market_ids))
        .select("received_at", "source_timestamp", "market_id", "token_id", "outcome", "asks")
        .collect(engine="streaming")
    )


def _database_day(day: datetime, market_ids: list[str]) -> pl.DataFrame:
    connection = database_connection()
    try:
        rows = connection.execute(
            """
            SELECT received_at, source_timestamp, market_id, token_id, outcome, asks
            FROM polymarket.btc_five_minute_orderbook_snapshots
            WHERE window_start >= %s AND window_start < %s
              AND market_id = ANY(%s)
            ORDER BY received_at, ingest_sequence
            """,
            (day, day + timedelta(days=1), market_ids),
        ).fetchall()
    finally:
        connection.close()
    return pl.DataFrame(
        rows,
        schema=("received_at", "source_timestamp", "market_id", "token_id", "outcome", "asks"),
        orient="row",
        infer_schema_length=None,
    ) if rows else pl.DataFrame()


def _books(frame: pl.DataFrame) -> dict[tuple[str, str], list[Book]]:
    result: dict[tuple[str, str], list[Book]] = defaultdict(list)
    for row in frame.iter_rows(named=True):
        outcome = str(row["outcome"]).lower()
        if outcome not in {"up", "down"}:
            continue
        result[(str(row["market_id"]), outcome)].append(
            Book(
                received_at=row["received_at"], source_timestamp=row["source_timestamp"],
                asks=parse_asks(row["asks"]), token_id=str(row["token_id"]),
            )
        )
    for rows in result.values():
        rows.sort(key=lambda row: row.received_at)
    return result


def _eligible(row: dict[str, Any], quantity: int, confidence_floor: float,
              minimum_edge: float, maximum_cost: float, reserve: float,
              stress: float) -> tuple[bool, str, float, float]:
    side = "up" if row["probability_up"] >= 0.5 else "down"
    confidence = (
        row["conservative_probability_up"] if side == "up"
        else 1 - row["conservative_probability_up"]
    )
    cost = row.get(f"{side}_ask_vwap_{quantity}")
    edge = confidence - cost - reserve - stress if cost is not None else float("-inf")
    return (
        cost is not None and confidence >= confidence_floor
        and cost <= maximum_cost and edge >= minimum_edge,
        side, confidence, edge,
    )


def _metrics(trades: list[dict[str, Any]]) -> dict[str, Any]:
    if not trades:
        return trade_metrics(pl.DataFrame())
    return trade_metrics(pl.DataFrame(trades))


def run(config_path: Path, output: Path) -> None:
    raw = tomllib.loads(config_path.read_text())
    source = raw["source"]
    execution = raw["execution"]
    policy = raw["policy"]
    start = datetime.fromisoformat(source["start"]).astimezone(UTC)
    end = datetime.fromisoformat(source["end"]).astimezone(UTC)
    predictions_path = Path(source["predictions"])
    if sha256(predictions_path) != source["predictions_sha256"]:
        raise RuntimeError("prediction artifact identity changed")
    predictions = (
        pl.read_parquet(predictions_path)
        .filter(
            (pl.col("window_start") >= start) & (pl.col("window_start") < end)
            & pl.col("seconds_elapsed").is_between(policy["start_second"], policy["end_second"])
        )
        .sort(["window_start", "market_id", "seconds_elapsed"])
    )
    scenarios = [Scenario(**row) for row in raw["scenarios"]]
    output.mkdir(parents=True, exist_ok=True)
    for name in ("inputs", "datasets", "predictions", "trades", "metrics", "diagnostics", "manifests", "logs"):
        (output / name).mkdir(exist_ok=True)

    all_events: list[dict[str, Any]] = []
    all_trades: list[dict[str, Any]] = []
    previous_trades: list[dict[str, Any]] = []
    day = start
    coverage = []
    archive_root = Path(source["archive_root"])
    database_start = datetime.fromisoformat(source["database_start"]).astimezone(UTC)
    while day < end:
        day_predictions = predictions.filter(
            (pl.col("window_start") >= day) & (pl.col("window_start") < day + timedelta(days=1))
        )
        market_ids = day_predictions["market_id"].unique().to_list()
        frame = (
            _archive_day(archive_root, day, market_ids)
            if day < database_start else _database_day(day, market_ids)
        )
        books = _books(frame)
        coverage.append({
            "day": day.date().isoformat(), "source": "archive" if day < database_start else "database",
            "snapshots": frame.height, "markets": len({key[0] for key in books}),
            "prediction_markets": len(market_ids),
        })
        groups = day_predictions.partition_by("market_id", as_dict=True, maintain_order=True)
        for quantity in QUANTITIES:
            for market_key, market_frame in groups.items():
                market_id = str(market_key[0] if isinstance(market_key, tuple) else market_key)
                rows = market_frame.to_dicts()
                previous = None
                for row in rows:
                    eligible, side, confidence, edge = _eligible(
                        row, quantity, policy["minimum_confidence"],
                        policy["minimum_stressed_edge"], policy["maximum_share_cost"],
                        policy["execution_reserve_per_share"], policy["stress_slippage_per_share"],
                    )
                    if eligible:
                        cost = row[f"{side}_ask_vwap_{quantity}"]
                        won = side == row["official_outcome"]
                        previous = {
                            "mode": "previous_assumed_fill", "scenario": "historical",
                            "quantity": quantity, "market_id": market_id,
                            "window_start": row["window_start"], "seconds_elapsed": row["seconds_elapsed"],
                            "side": side, "official_outcome": row["official_outcome"], "won": won,
                            "share_cost": cost,
                            "net_pnl": quantity * ((1 - cost - policy["execution_reserve_per_share"]) if won else -(cost + policy["execution_reserve_per_share"])),
                            "stress_pnl": quantity * ((1 - cost - policy["execution_reserve_per_share"] - policy["stress_slippage_per_share"]) if won else -(cost + policy["execution_reserve_per_share"] + policy["stress_slippage_per_share"])),
                        }
                        break
                if previous:
                    previous_trades.append(previous)

                for scenario in scenarios:
                    for mode in ("one_shot", "continual"):
                        attempt_number = 0
                        for row in rows:
                            eligible, side, confidence, edge = _eligible(
                                row, quantity, policy["minimum_confidence"],
                                policy["minimum_stressed_edge"], policy["maximum_share_cost"],
                                policy["execution_reserve_per_share"], policy["stress_slippage_per_share"],
                            )
                            if not eligible:
                                continue
                            attempt_number += 1
                            observed_at = row["window_start"] + timedelta(seconds=row["seconds_elapsed"])
                            arrival_at = observed_at + timedelta(milliseconds=scenario.latency_ms)
                            selected_books = books.get((market_id, side), [])
                            exit_books = books.get((market_id, "down" if side == "up" else "up"), [])
                            decision_book = select_book(selected_books, observed_at, execution["max_book_age_ms"])
                            arrival_book = select_book(selected_books, arrival_at, execution["max_book_age_ms"])
                            exit_book = select_book(exit_books, arrival_at, execution["max_book_age_ms"])
                            result = fok_attempt(
                                decision_book, arrival_book, quantity, execution["maximum_limit_price"],
                                scenario.depth_haircut, execution["maximum_depth_participation"],
                            )
                            if result["filled"] and exit_book is None:
                                result = {**result, "filled": False, "reason": "missing_exit_book"}
                            event = {
                                "mode": mode, "scenario": scenario.name, "quantity": quantity,
                                "market_id": market_id, "window_start": row["window_start"],
                                "seconds_elapsed": row["seconds_elapsed"], "attempt": attempt_number,
                                "side": side, "confidence": confidence, "stressed_edge": edge,
                                "filled": result["filled"], "reason": result["reason"],
                                "decision_vwap": result.get("decision_vwap"),
                                "arrival_vwap": result.get("arrival_vwap"),
                                "limit_price": result.get("limit_price"),
                                "displayed_size_at_limit": result.get("displayed_size_at_limit"),
                            }
                            all_events.append(event)
                            if result["filled"]:
                                cost = result["arrival_vwap"]
                                won = side == row["official_outcome"]
                                all_trades.append({
                                    **event, "official_outcome": row["official_outcome"], "won": won,
                                    "share_cost": cost,
                                    "net_pnl": quantity * ((1 - cost - policy["execution_reserve_per_share"]) if won else -(cost + policy["execution_reserve_per_share"])),
                                    "stress_pnl": quantity * ((1 - cost - policy["execution_reserve_per_share"] - policy["stress_slippage_per_share"]) if won else -(cost + policy["execution_reserve_per_share"] + policy["stress_slippage_per_share"])),
                                })
                                break
                            if mode == "one_shot":
                                break
        day += timedelta(days=1)

    events = pl.DataFrame(all_events)
    trades = pl.DataFrame(all_trades) if all_trades else pl.DataFrame()
    previous = pl.DataFrame(previous_trades) if previous_trades else pl.DataFrame()
    events.write_parquet(output / "datasets" / "fok-attempts.parquet", compression="zstd")
    if not trades.is_empty():
        trades.write_parquet(output / "trades" / "exact-fok-trades.parquet", compression="zstd")
    if not previous.is_empty():
        previous.write_parquet(output / "trades" / "previous-assumed-fill.parquet", compression="zstd")
    results: dict[str, Any] = {
        "schema_version": "btc-fok-retry-backtest-v1", "coverage": coverage,
        "source": {**source, "prediction_rows": predictions.height, "prediction_markets": predictions["market_id"].n_unique()},
        "execution": execution, "policy": policy, "scenarios": raw["scenarios"], "quantities": {},
    }
    for quantity in QUANTITIES:
        quantity_results = {"previous_assumed_fill": _metrics([row for row in previous_trades if row["quantity"] == quantity])}
        for scenario in scenarios:
            for mode in ("one_shot", "continual"):
                selected_events = [row for row in all_events if row["quantity"] == quantity and row["scenario"] == scenario.name and row["mode"] == mode]
                selected_trades = [row for row in all_trades if row["quantity"] == quantity and row["scenario"] == scenario.name and row["mode"] == mode]
                attempted_markets = len({row["market_id"] for row in selected_events})
                filled_markets = len({row["market_id"] for row in selected_trades})
                recovered = len({row["market_id"] for row in selected_trades if row["attempt"] > 1})
                quantity_results[f"{scenario.name}:{mode}"] = {
                    "attempts": len(selected_events), "attempted_markets": attempted_markets,
                    "filled_markets": filled_markets,
                    "eventual_fill_rate": filled_markets / attempted_markets if attempted_markets else None,
                    "recovered_after_rejection": recovered,
                    "expired_without_fill": attempted_markets - filled_markets,
                    "attempts_per_fill": len(selected_events) / filled_markets if filled_markets else None,
                    "reject_reasons": dict(pl.DataFrame(selected_events).group_by("reason").len().iter_rows()) if selected_events else {},
                    "trades": _metrics(selected_trades),
                }
        results["quantities"][str(quantity)] = quantity_results
    git = {
        "branch": subprocess.check_output(["git", "branch", "--show-current"], text=True).strip(),
        "commit": subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip(),
        "dirty": bool(subprocess.check_output(["git", "status", "--porcelain"], text=True).strip()),
    }
    results["git"] = git
    (output / "metrics" / "results.json").write_text(json.dumps(results, indent=2, sort_keys=True, default=str) + "\n")
    (output / "README.md").write_text(
        "# Exact FOK retry backtest\n\n"
        f"Coverage: `{start.isoformat()}` through `{end.isoformat()}`.\n\n"
        f"Predictions: `{predictions_path}` (`{source['predictions_sha256']}`).\n\n"
        "The model and admission policy are frozen. Raw causal orderbook ladders determine decision limits and arrival FOK fills. "
        "One-shot and continual-inference modes are matched on identical decisions and coverage.\n"
    )


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    arguments = parser.parse_args()
    run(arguments.config, arguments.output)
