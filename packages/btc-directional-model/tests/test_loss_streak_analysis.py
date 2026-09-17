from datetime import UTC, datetime, timedelta

import polars as pl

from btc_directional_model.loss_streak_analysis import add_streak_ids


def test_add_streak_ids_marks_consecutive_losses() -> None:
    start = datetime(2026, 1, 1, tzinfo=UTC)
    frame = pl.DataFrame({
        "window_start": [start + timedelta(minutes=5 * i) for i in range(7)],
        "won": [False, False, True, False, True, False, False],
    })
    result = add_streak_ids(frame)
    assert result["loss_streak_length"].to_list() == [2, 2, 0, 1, 0, 2, 2]
    assert result["loss_streak_id"].to_list() == [1, 1, None, 2, None, 3, 3]
