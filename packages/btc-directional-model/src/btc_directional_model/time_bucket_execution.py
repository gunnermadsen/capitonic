"""Offline arrival-book replay using the maintained ladder walker and fee formula."""

from __future__ import annotations

import bisect
from dataclasses import dataclass
from datetime import date, datetime, timedelta
from functools import lru_cache
from pathlib import Path

import polars as pl

from .core_execution import taker_fee_per_share
from .fok_retry_backtest import parse_asks, walk


@lru_cache(maxsize=32768)
def levels(value: str) -> tuple[tuple[float, float], ...]:
    return parse_asks(value)


@dataclass(frozen=True)
class ObservedBook:
    available_at: datetime
    source_timestamp: datetime
    received_at: datetime
    sampled_at: datetime
    provider_available_at: datetime
    bids: str
    asks: str
    token_id: str
    book_sha256: str


def book_index(frame: pl.DataFrame) -> dict:
    index: dict = {}
    for key, rows in frame.partition_by(["market_id", "outcome"], as_dict=True).items():
        rows = rows.sort("available_at", "sampled_at", "ingest_sequence")
        books = [ObservedBook(**r) for r in rows.select(list(ObservedBook.__dataclass_fields__)).to_dicts()]
        index[key] = ([r.available_at for r in books], books)
    return index


def day_books(run: Path, day: date, markets: list[str]) -> dict:
    """Retain valid previous-day observations around a UTC midnight decision."""
    directory = run / "inputs/validated-core/polymarket_clob_l2"
    paths = [directory / f"{d}.parquet" for d in [day - timedelta(days=1), day]]
    paths = [path for path in paths if path.exists()]
    if not paths:
        return {}
    columns = list(ObservedBook.__dataclass_fields__) + ["market_id", "outcome", "ingest_sequence"]
    frame = pl.scan_parquet(paths).filter(pl.col("market_id").is_in(markets)).select(columns).collect(engine="streaming")
    return book_index(frame)


def at(index: dict, market: str, side: str, when: datetime, age: float) -> ObservedBook | None:
    entry = index.get((market, side))
    if entry is None:
        return None
    times, books = entry
    i = bisect.bisect_right(times, when) - 1
    if i < 0:
        return None
    book = books[i]
    for name in ["available_at", "source_timestamp", "received_at", "sampled_at", "provider_available_at"]:
        elapsed = (when - getattr(book, name)).total_seconds()
        if elapsed < 0 or elapsed > age:
            return None
    return book


def bid_walk(bids: tuple, quantity: float, haircut: float = 1.0, floor: float = 0.0) -> tuple | None:
    # A complementary-price ask walk preserves the existing partial-depth math.
    inverse = tuple(sorted((1 - price, size) for price, size in bids))
    result = walk(inverse, quantity, haircut=haircut, limit=1 - floor)
    return None if result is None else (1 - result[0], 1 - result[1], result[2])


def order(
    index: dict, market: str, side: str, decision_at: datetime, quantity: float,
    order_type: str, frozen: dict, *, max_price: float = 1.0,
    action: str = "buy", scenario: dict | None = None,
) -> dict:
    parameters = {**frozen["book"], **(scenario or {})}
    result = {"requested_quantity": quantity, "filled_quantity": 0.0,
              "fill_ratio": 0.0, "share_price": None, "fee_per_share": None,
              "reason": "unknown_decision_book", "evidence_known": False,
              "order_type": order_type, "action": action, "decision_at": decision_at}
    if order_type not in {"FAK", "FOK"} or quantity <= 0:
        raise ValueError("Unsupported offline order")
    age = parameters["max_age_seconds"]
    decision = at(index, market, side, decision_at, age)
    if decision is None:
        return result
    result.update(decision_book_sha256=decision.book_sha256, token_id=decision.token_id,
                  decision_book_available_at=decision.available_at)
    ladder = levels(decision.asks if action == "buy" else decision.bids)
    if not ladder:
        return result
    decision_walk = walk(ladder, quantity) if action == "buy" else bid_walk(ladder, quantity)
    # FAK can admit a partial visible amount even when the entire request lacks depth.
    if decision_walk is None:
        visible = sum(size for _, size in ladder)
        if visible <= 0:
            return result
        decision_walk = walk(ladder, min(quantity, visible)) if action == "buy" else bid_walk(ladder, min(quantity, visible))
    limit = min(max_price, decision_walk[1]) if action == "buy" else decision_walk[1]
    arrival_time = decision_at + timedelta(milliseconds=parameters["arrival_ms"])
    result.update(arrival_at=arrival_time, limit_price=limit, decision_vwap=decision_walk[0])
    arrival = at(index, market, side, arrival_time, age)
    opposite = at(index, market, "down" if side == "up" else "up", arrival_time, age)
    if arrival is None or opposite is None:
        result["reason"] = "unknown_arrival_book"
        return result
    result["evidence_known"] = True
    result.update(arrival_book_sha256=arrival.book_sha256, arrival_book_available_at=arrival.available_at)
    ladder = levels(arrival.asks if action == "buy" else arrival.bids)
    adverse = parameters.get("adverse_price_move", 0.0)
    shifted = tuple((price + adverse if action == "buy" else price - adverse, size)
                    for price, size in ladder)
    permitted = tuple((p, q) for p, q in shifted if 0 < p < 1 and
                      (p <= limit + 1e-12 if action == "buy" else p >= limit - 1e-12))
    displayed = sum(size for _, size in permitted)
    # Haircut and participation are separate ceilings on the original displayed size.
    capacity = min(displayed * parameters["depth_haircut"], displayed * parameters["participation_cap"])
    capacity *= parameters.get("partial_fill_multiplier", 1.0)
    filled = min(quantity, capacity)
    if order_type == "FOK" and filled + 1e-9 < quantity:
        result["reason"] = "fok_insufficient_permitted_depth"
        return result
    if filled <= 1e-9:
        result["reason"] = "no_immediate_fill"
        return result
    fill = walk(permitted, filled, haircut=parameters["depth_haircut"], limit=limit) if action == "buy" else bid_walk(permitted, filled, parameters["depth_haircut"], limit)
    if fill is None:
        raise ValueError("Permitted quantity and ladder walk disagree")
    remaining, total_fee = filled, 0.0
    for price, size in sorted(permitted, reverse=action != "buy"):
        taken = min(remaining, size * parameters["depth_haircut"])
        total_fee += taken * taker_fee_per_share(
            frozen["fees"]["rate_assumption"] * parameters.get("fee_multiplier", 1.0), price)
        remaining -= taken
        if remaining <= 1e-9:
            break
    fee = total_fee / filled
    result.update(filled_quantity=filled, fill_ratio=filled / quantity, share_price=fill[0],
                  fee_per_share=fee, reason="filled" if filled + 1e-9 >= quantity else "partial_fill",
                  reserve_per_share=parameters["reserve_per_share_per_leg"],
                  stress_extra_per_share=parameters["stress_extra_per_share_per_leg"],
                  filled_notional=filled * fill[0], displayed_depth=displayed)
    return result


def settlement(buy: dict, won: bool, sales: list[dict] | None = None) -> dict:
    """Charge every executed leg and settle exactly the unsold residual shares."""
    quantity = buy["filled_quantity"]
    if quantity <= 0:
        return {"net_pnl": 0.0, "stress_pnl": 0.0, "residual_quantity": 0.0}
    net = -quantity * (buy["share_price"] + buy["fee_per_share"] + buy["reserve_per_share"])
    stress_cost = quantity * buy["stress_extra_per_share"]
    remaining = quantity
    for sale in sales or []:
        sold = sale["filled_quantity"]
        if sold > remaining + 1e-9:
            raise ValueError("Offline exit cannot sell more shares than held")
        if sold > 0:
            net += sold * (sale["share_price"] - sale["fee_per_share"] - sale["reserve_per_share"])
            stress_cost += sold * sale["stress_extra_per_share"]
        remaining -= sold
    net += remaining * int(won)
    return {"net_pnl": net, "stress_pnl": net - stress_cost, "residual_quantity": remaining}
