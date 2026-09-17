import polars as pl

from btc_directional_model.conservative_selective_training import (
    Policy,
    replay,
    trade_metrics,
    wilson_lower,
)


def test_replay_takes_first_qualifying_row_per_market() -> None:
    frame = pl.DataFrame({
        "market_id": ["a", "a", "b"],
        "window_start": [1, 1, 2],
        "seconds_elapsed": [60, 65, 60],
        "probability_up": [0.91, 0.99, 0.08],
        "up_ask_vwap_5": [0.70, 0.60, 0.55],
        "down_ask_vwap_5": [0.40, 0.40, 0.70],
        "official_outcome": ["up", "up", "down"],
    })
    trades = replay(frame, Policy(0.90, 0.05, 0.80), 0.005, 0.01)
    assert trades["seconds_elapsed"].to_list() == [60, 60]
    assert trade_metrics(trades)["wins"] == 2


def test_wilson_lower_is_conservative() -> None:
    assert 0 < wilson_lower(9, 10) < 0.9
    assert wilson_lower(0, 0) == 0
