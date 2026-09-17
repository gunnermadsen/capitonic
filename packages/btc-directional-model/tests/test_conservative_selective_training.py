from datetime import UTC, datetime, timedelta

import polars as pl

from btc_directional_model.conservative_selective_training import (
    PRICE_FEATURES,
    Policy,
    fit_model,
    replay,
    score,
    trade_metrics,
    wilson_lower,
)


def test_replay_takes_first_qualifying_row_per_market() -> None:
    start = datetime(2026, 1, 1, tzinfo=UTC)
    frame = pl.DataFrame({
        "market_id": ["a", "a", "b"],
        "window_start": [start, start, start + timedelta(minutes=5)],
        "seconds_elapsed": [60, 65, 60],
        "probability_up": [0.91, 0.99, 0.08],
        "conservative_probability_up": [0.89, 0.97, 0.10],
        "up_ask_vwap_5": [0.70, 0.60, 0.55],
        "down_ask_vwap_5": [0.40, 0.40, 0.70],
        "official_outcome": ["up", "up", "down"],
    })
    trades = replay(frame, Policy(0.88, 0.05, 0.80), 0.005, 0.01)
    assert trades["seconds_elapsed"].to_list() == [60, 60]
    assert trade_metrics(trades)["wins"] == 2


def test_wilson_lower_is_conservative() -> None:
    assert 0 < wilson_lower(9, 10) < 0.9
    assert wilson_lower(0, 0) == 0


def test_recovery_price_ceiling_rejects_expensive_entry() -> None:
    frame = pl.DataFrame({
        "market_id": ["a"], "window_start": [datetime(2026, 1, 1, tzinfo=UTC)],
        "seconds_elapsed": [60],
        "probability_up": [0.99], "conservative_probability_up": [0.97],
        "up_ask_vwap_5": [0.66], "down_ask_vwap_5": [0.35],
        "official_outcome": ["up"],
    })
    assert replay(frame, Policy(0.88, 0.03, 0.65), 0.005, 0.01).is_empty()


def test_small_fit_and_calibration_are_finite() -> None:
    rows = 20 * 5
    frame = pl.DataFrame({
        "market_id": [f"m{i // 5}" for i in range(rows)],
        "official_outcome": ["up" if i // 5 % 2 else "down" for i in range(rows)],
        **{name: [float((i + j) % 11) for i in range(rows)] for j, name in enumerate(PRICE_FEATURES)},
    })
    raw = {
        "model": {"learning_rate": 0.05, "max_iter": 5, "max_bins": 31},
        "calibration": {"reliability_bins": 5, "safety_floor": 0.02},
    }
    spec = {"max_leaf_nodes": 3, "min_samples_leaf": 2,
            "l2_regularization": 1.0, "include_aggregate_volume": False}
    model = fit_model(frame.head(60), frame.tail(40), spec, raw, 7)
    scored = score(frame.tail(40), model, 0.02)
    assert scored["probability_up"].is_finite().all()
    assert scored["conservative_probability_up"].is_finite().all()
