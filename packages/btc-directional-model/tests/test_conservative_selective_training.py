from datetime import UTC, datetime, timedelta

import polars as pl
import pytest

from btc_directional_model.conservative_selective_training import (
    PRICE_FEATURES,
    Policy,
    fit_model,
    replay,
    score,
    trade_metrics,
    wilson_lower,
)


def test_replay_uses_requested_vwap_quantity_without_changing_direction() -> None:
    frame = pl.DataFrame(
        {
            "market_id": ["m1"],
            "window_start": [datetime(2026, 1, 1, tzinfo=UTC)],
            "seconds_elapsed": [60],
            "official_outcome": ["up"],
            "probability_up": [0.9],
            "conservative_probability_up": [0.88],
            "up_ask_vwap_5": [0.40],
            "down_ask_vwap_5": [0.61],
            "up_ask_vwap_20": [0.45],
            "down_ask_vwap_20": [0.66],
        }
    )
    policy = Policy(0.80, 0.10, 0.55)

    q5 = replay(frame, policy, 0.005, 0.01, quantity=5)
    q20 = replay(frame, policy, 0.005, 0.01, quantity=20)

    assert q5["side"].item() == q20["side"].item() == "up"
    assert q5["share_cost"].item() == 0.40
    assert q20["share_cost"].item() == 0.45
    assert q20["net_pnl"].item() == pytest.approx(20 * (1 - 0.45 - 0.005))


def test_replay_takes_first_qualifying_row_per_market() -> None:
    start = datetime(2026, 1, 1, tzinfo=UTC)
    frame = pl.DataFrame(
        {
            "market_id": ["a", "a", "b"],
            "window_start": [start, start, start + timedelta(minutes=5)],
            "seconds_elapsed": [60, 65, 60],
            "probability_up": [0.91, 0.99, 0.08],
            "conservative_probability_up": [0.89, 0.97, 0.10],
            "up_ask_vwap_5": [0.70, 0.60, 0.55],
            "down_ask_vwap_5": [0.40, 0.40, 0.70],
            "official_outcome": ["up", "up", "down"],
        }
    )
    trades = replay(frame, Policy(0.88, 0.05, 0.80), 0.005, 0.01)
    assert trades["seconds_elapsed"].to_list() == [60, 60]
    assert trade_metrics(trades)["wins"] == 2


def test_wilson_lower_is_conservative() -> None:
    assert 0 < wilson_lower(9, 10) < 0.9
    assert wilson_lower(0, 0) == 0


def test_recovery_price_ceiling_rejects_expensive_entry() -> None:
    frame = pl.DataFrame(
        {
            "market_id": ["a"],
            "window_start": [datetime(2026, 1, 1, tzinfo=UTC)],
            "seconds_elapsed": [60],
            "probability_up": [0.99],
            "conservative_probability_up": [0.97],
            "up_ask_vwap_5": [0.66],
            "down_ask_vwap_5": [0.35],
            "official_outcome": ["up"],
        }
    )
    assert replay(frame, Policy(0.88, 0.03, 0.65), 0.005, 0.01).is_empty()


def test_small_fit_and_calibration_are_finite() -> None:
    rows = 20 * 5
    frame = pl.DataFrame(
        {
            "market_id": [f"m{i // 5}" for i in range(rows)],
            "official_outcome": ["up" if i // 5 % 2 else "down" for i in range(rows)],
            **{
                name: [float((i + j) % 11) for i in range(rows)]
                for j, name in enumerate(PRICE_FEATURES)
            },
        }
    )
    raw = {
        "model": {"learning_rate": 0.05, "max_iter": 5, "max_bins": 31},
        "calibration": {"reliability_bins": 5, "safety_floor": 0.02},
    }
    spec = {
        "max_leaf_nodes": 3,
        "min_samples_leaf": 2,
        "l2_regularization": 1.0,
        "include_aggregate_volume": False,
    }
    model = fit_model(frame.head(60), frame.tail(40), spec, raw, 7)
    scored = score(frame.tail(40), model, 0.02)
    assert scored["probability_up"].is_finite().all()
    assert scored["conservative_probability_up"].is_finite().all()
