from datetime import UTC, datetime, timedelta

import pytest

from btc_directional_model.fok_retry_backtest import Book, fok_attempt, parse_asks, select_book, walk


def book(at: datetime, asks: tuple[tuple[float, float], ...]) -> Book:
    return Book(received_at=at, source_timestamp=at, asks=asks, token_id="token")


def test_walk_returns_vwap_and_worst_ask() -> None:
    result = walk(((0.40, 3.0), (0.45, 4.0)), 5)
    assert result == pytest.approx(((3 * 0.40 + 2 * 0.45) / 5, 0.45, 7.0))


def test_fok_rejects_atomically_when_arrival_depth_is_insufficient() -> None:
    at = datetime(2026, 1, 1, tzinfo=UTC)
    result = fok_attempt(
        book(at, ((0.40, 10.0),)), book(at, ((0.40, 5.0),)),
        quantity=5, max_price=0.55, haircut=0.80, participation=1.0,
    )
    assert result["filled"] is False
    assert result["reason"] == "insufficient_arrival_depth"


def test_fok_applies_runtime_participation_cap() -> None:
    at = datetime(2026, 1, 1, tzinfo=UTC)
    result = fok_attempt(
        book(at, ((0.40, 10.0),)), book(at, ((0.40, 10.0),)),
        quantity=5, max_price=0.55, haircut=0.80, participation=0.25,
    )
    assert result["filled"] is False
    assert result["reason"] == "arrival_depth_participation_exceeded"


def test_select_book_is_causal_and_fresh() -> None:
    at = datetime(2026, 1, 1, tzinfo=UTC)
    books = [book(at, ((0.4, 10.0),)), book(at + timedelta(seconds=1), ((0.5, 10.0),))]
    assert select_book(books, at + timedelta(milliseconds=500), 2000) == books[0]
    assert select_book(books, at + timedelta(seconds=3, milliseconds=1), 2000) is None


def test_parse_asks_accepts_archive_json() -> None:
    assert parse_asks('[["0.45", "2.5"], ["0.40", "3"]]') == ((0.4, 3.0), (0.45, 2.5))
