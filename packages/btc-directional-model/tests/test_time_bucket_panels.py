from datetime import UTC, datetime, timedelta

import polars as pl

from btc_directional_model.time_bucket_panels import points, price_states


def test_price_state_respects_receipt_and_does_not_regress_on_late_old_tick():
    start = datetime(2026, 8, 21, tzinfo=UTC)
    raw = pl.DataFrame({
        "source_timestamp": [start, start + timedelta(seconds=4), start],
        "available_at": [start + timedelta(seconds=1), start + timedelta(seconds=4), start + timedelta(seconds=5)],
        "price": [100.0, 104.0, 100.0],
    })
    q = pl.DataFrame({"decision_at": [start, start + timedelta(seconds=2), start + timedelta(seconds=6)],
                      "window_start": [start] * 3})
    output = price_states(q, raw, "rtds_chainlink", 5)
    assert output["rtds_chainlink_price"].to_list() == [None, 100.0, 104.0]
    assert output["rtds_chainlink_return_180_bps"].null_count() == 3
    assert output["rtds_chainlink_path_bps"].null_count() == 3


def test_grid_preserves_thirteen_buckets_and_wait_observations():
    start = datetime(2026, 8, 21, tzinfo=UTC)
    population = pl.DataFrame({"market_id": ["m"], "window_start": [start]})
    frame = points(population, {"bucket_starts": list(range(0, 260, 20)), "entry_offsets": [0, 9, 19], "additional_exit_seconds": [279, 299]})
    assert frame.height == 41
    assert frame.filter(pl.col("seconds_elapsed") < 260)["bucket_start"].n_unique() == 13
    assert frame["point_id"].n_unique() == 41
