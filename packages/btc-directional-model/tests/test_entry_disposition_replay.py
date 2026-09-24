from datetime import UTC, datetime, timedelta

from btc_directional_model.entry_disposition_replay import (
    Book,
    _bid_walk,
    _buy,
    _dispose,
    _sell,
)


def _book(at: datetime, asks: str, bids: str) -> Book:
    return Book(at, at, at, at, asks, bids)


def test_buy_rejects_full_order_when_arrival_depth_falls() -> None:
    at = datetime(2026, 9, 20, tzinfo=UTC)
    decision = _book(at, '[["0.40","100"]]', '[["0.35","100"]]')
    arrival_at = at + timedelta(milliseconds=100)
    arrival = _book(arrival_at, '[["0.40","4"]]', '[["0.35","100"]]')
    index = {("market", "up"): ([at, arrival_at], [decision, arrival]),
             ("market", "down"): ([at], [decision])}
    assert _buy(index, "market", "up", at, 0.55)["reason"] == "arrival_fok_rejected"


def test_sell_uses_causal_bid_limit_and_does_not_partially_fill() -> None:
    at = datetime(2026, 9, 20, tzinfo=UTC)
    decision = _book(at, '[["0.40","100"]]', '[["0.35","100"]]')
    arrival_at = at + timedelta(milliseconds=100)
    arrival = _book(arrival_at, '[["0.40","100"]]', '[["0.34","100"]]')
    index = {("market", "up"): ([at, arrival_at], [decision, arrival])}
    assert _sell(index, "market", "up", at)["reason"] == "sell_fok_rejected"
    assert _bid_walk('[["0.35","2"],["0.34","4"]]', 5)[0] == 0.344


def test_fixed_exit_waits_five_seconds_and_stops_before_220() -> None:
    start = datetime(2026, 9, 20, tzinfo=UTC)
    book = _book(start, '[["0.40","100"]]', '[["0.60","100"]]')
    index = {("market", "up"): ([start], [book])}
    trade = {"market_id": "market", "window_start": start, "official_outcome": "up",
             "seconds_elapsed": 210, "side": "up", "share_cost": 0.40, "quantity": 5}
    rows = [
        {"market_id": "market", "window_start": start, "seconds_elapsed": second,
         "up_bids": '[["0.60","100"]]'}
        for second in (210, 215, 220)
    ]
    held, events = _dispose(trade, rows, index, None, "fixed")
    assert held["exit_type"] == "settlement"
    assert all(event["second"] == 215 for event in events)
