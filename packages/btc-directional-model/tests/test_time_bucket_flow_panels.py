from datetime import UTC, datetime, timedelta

import polars as pl
import pytest

from btc_directional_model.time_bucket_flow_panels import (
    CANDLES,
    FEATURES,
    FIELDS,
    OI,
    PRINTS,
    SNAPSHOTS,
    flow_states,
)
from btc_directional_model.time_bucket_flow_sources import L2_VALUES

START = datetime(2026, 8, 20, tzinfo=UTC)
FROZEN = {"flow_max_age_seconds": 5, "open_interest_max_age_seconds": 600}


def query(seconds):
    return pl.DataFrame({"decision_at": [START + timedelta(seconds=t) for t in seconds]})


def candle(second, **changes):
    opened = START + timedelta(seconds=second)
    return {"source": "binance_spot", "symbol": "BTCUSDT", "product": CANDLES,
            "source_timestamp": opened, "open_timestamp": opened,
            "close_timestamp": opened + timedelta(milliseconds=999),
            "available_at": opened + timedelta(seconds=1),
            "open_price": 100 + second, "high_price": 102 + second,
            "low_price": 99 + second, "close_price": 101 + second,
            "base_volume": 2.0, "quote_volume": 200.0, "taker_buy_base_volume": 1.5,
            "taker_buy_quote_volume": 150.0, "trade_count": 2} | changes


def trade(second, price=100.0, **changes):
    source = START + timedelta(seconds=second)
    return {"source": "binance_spot", "symbol": "BTCUSDT", "product": PRINTS,
            "source_timestamp": source, "trade_timestamp": source,
            "aggregate_trade_id": second, "available_at": source,
            "price": price, "quantity": 2.0, "buyer_maker": False} | changes


def test_print_receipt_and_freshness_are_causal_and_missing_is_not_filled():
    raw = pl.DataFrame([trade(0), trade(5, 110, available_at=START + timedelta(seconds=7))])
    result = flow_states(query([5, 6, 7, 11]), raw, PRINTS, FROZEN)
    assert result[f"{PRINTS}_price"].to_list() == [100.0, None, 110.0, None]
    assert result[f"{PRINTS}_return_5s_bps"][0] == 0.0
    assert result[f"{PRINTS}_return_5s_bps"][2] == pytest.approx(1000)
    assert all("volume" not in x and "quiet" not in x for x in FEATURES[PRINTS])


def test_late_older_print_does_not_erase_a_newer_known_price():
    raw = pl.DataFrame([trade(3, 103), trade(1, 101, available_at=START + timedelta(seconds=4))])
    result = flow_states(query([4]), raw, PRINTS, FROZEN)
    assert result[f"{PRINTS}_price"].item() == 103


def test_completed_contiguous_candles_produce_only_known_window_features():
    raw = pl.DataFrame([candle(s) for s in range(60)])
    result = flow_states(query([59.5, 60]), raw, CANDLES, FROZEN)
    assert result[f"{CANDLES}_return_bps_60s"][0] is None
    assert result[f"{CANDLES}_return_bps_60s"][1] == pytest.approx(6000)
    assert result[f"{CANDLES}_base_volume_60s"][1] == 120
    assert result[f"{CANDLES}_taker_imbalance"][1] == 0.5


@pytest.mark.parametrize("missing", [True, False])
def test_candle_window_rejects_gaps_and_late_constituents(missing):
    rows = [candle(s) for s in range(60)]
    if missing:
        rows.pop(45)
    else:
        rows[45]["available_at"] = START + timedelta(seconds=61)
    result = flow_states(query([60]), pl.DataFrame(rows), CANDLES, FROZEN)
    assert result[f"{CANDLES}_close_price"].item() == 160
    assert result[f"{CANDLES}_base_volume_30s"].item() is None
    assert result[f"{CANDLES}_return_bps_60s"].item() is None


def test_l2_values_wait_for_validated_completed_second_without_recomputation():
    product = "binance_futures_l2_features"
    row = {"symbol": "BTCUSDT", "product": product, "second_start": START,
           "source_timestamp": START, "available_at": START + timedelta(seconds=1),
           "stored_available_at": START + timedelta(milliseconds=200),
           **{name: float(i + 1) for i, name in enumerate(L2_VALUES)}}
    result = flow_states(query([0.9, 1]), pl.DataFrame([row]), product, FROZEN)
    assert result[f"{product}_midpoint"].to_list() == [None, 1.0]
    assert result[f"{product}_stored_available_at"][1] == row["stored_available_at"]
    assert FIELDS[product] == L2_VALUES


def test_open_interest_prior_period_requires_known_original_availability_and_age():
    rows = [{"source": "binance_usd_m_futures", "symbol": "BTCUSDT", "product": OI,
             "source_timestamp": START + timedelta(seconds=s), "period_seconds": 300,
             "available_at": START + timedelta(seconds=s + 2),
             "sum_open_interest": value, "sum_open_interest_value": value * 100}
            for s, value in [(0, 100.0), (300, 110.0)]]
    result = flow_states(query([301, 302, 601]), pl.DataFrame(rows), OI, FROZEN)
    assert result[f"{OI}_change_5m"].to_list() == [None, 10.0, None]
    rows[0]["available_at"] = START + timedelta(seconds=303)
    result = flow_states(query([302]), pl.DataFrame(rows), OI, FROZEN)
    assert result[f"{OI}_change_5m"].item() is None


def test_snapshot_observed_levels_support_depth_and_imbalance():
    raw = pl.DataFrame([{"source": "binance_spot", "symbol": "BTCUSDT", "product": SNAPSHOTS,
                         "source_timestamp": START, "available_at": START + timedelta(seconds=1),
                         "source_update_id": 4, "sampling_policy_sha256": "policy",
                         "bids": '[["100","2"],["99","4"]]', "asks": '[["102","2"]]'}])
    result = flow_states(query([1]), raw, SNAPSHOTS, FROZEN)
    assert result[f"{SNAPSHOTS}_midpoint"].item() == 101
    assert result[f"{SNAPSHOTS}_bid_depth"].item() == 6
    assert result[f"{SNAPSHOTS}_imbalance"].item() == 0.5
    assert '"source_update_id": 4' in result[f"{SNAPSHOTS}_native_identity"].item()


def test_product_feature_groups_remain_separate_with_no_missing_value_fill():
    left = flow_states(query([0]), pl.DataFrame([trade(0)]), PRINTS, FROZEN)
    right = flow_states(query([0]), pl.DataFrame(), CANDLES, FROZEN)
    joined = left.hstack(right)
    assert joined.height == 1
    assert joined[f"{PRINTS}_price"].item() == 100
    assert joined.select(FEATURES[CANDLES]).null_count().row(0) == (1,) * len(FEATURES[CANDLES])
    populated = flow_states(query([1]), pl.DataFrame([candle(0)]), CANDLES, FROZEN)
    assert right.schema == populated.schema
