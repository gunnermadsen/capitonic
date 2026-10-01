import json
from datetime import UTC, datetime

import polars as pl
import pytest

from btc_directional_model.time_bucket_execution import book_index, order, settlement


def fixture():
    when = datetime(2026, 8, 23, 12, tzinfo=UTC)
    frame = pl.DataFrame([{
        "market_id": "m", "outcome": side, "token_id": side, "book_sha256": "fixture",
        "available_at": when, "source_timestamp": when, "sampled_at": when,
        "provider_available_at": when, "received_at": when, "ingest_sequence": 1,
        "bids": json.dumps([["0.49", "8"]]), "asks": json.dumps([["0.5", "8"]]),
    } for side in ["up", "down"]])
    frozen = {"book": {"arrival_ms": 150, "max_age_seconds": 2,
                         "depth_haircut": 0.8, "participation_cap": 0.25,
                         "reserve_per_share_per_leg": 0.005,
                         "stress_extra_per_share_per_leg": 0.01},
              "fees": {"rate_assumption": 0.07}}
    return when, book_index(frame), frozen


def test_fak_partial_does_not_become_fok_fill_and_charges_fee():
    when, books, frozen = fixture()
    fak = order(books, "m", "up", when, 5, "FAK", frozen)
    fok = order(books, "m", "up", when, 5, "FOK", frozen)
    assert fak["filled_quantity"] == 2
    assert fak["fee_per_share"] == pytest.approx(0.0175)
    assert fok["filled_quantity"] == 0 and fok["evidence_known"]
    assert settlement(fak, True)["net_pnl"] == pytest.approx(0.955)


def test_missing_arrival_is_unknown_not_a_known_no_trade():
    when, books, frozen = fixture()
    result = order(books, "m", "up", when, 5, "FAK", frozen, scenario={"arrival_ms": 3000})
    assert not result["evidence_known"]
    assert result["reason"] == "unknown_arrival_book"


def test_partial_sale_settles_only_residual_and_charges_both_legs():
    when, books, frozen = fixture()
    buy = order(books, "m", "up", when, 5, "FAK", frozen)
    sale = order(books, "m", "up", when, 2, "FAK", frozen,
                 action="sell", scenario={"participation_cap": 0.125})
    result = settlement(buy, True, [sale])
    assert result["residual_quantity"] == 1
    expected = -2 * (0.5 + 0.0175 + 0.005) + 0.49 - 0.07 * 0.49 * 0.51 - 0.005 + 1
    assert result["net_pnl"] == pytest.approx(expected)
    assert result["stress_pnl"] == pytest.approx(expected - 0.03)
