from datetime import UTC, datetime, timedelta

import polars as pl

from btc_directional_model.time_bucket_protocol import chronological_roles, split_rows


def test_later_fit_never_reuses_prior_evaluation_or_calibration_markets():
    points = [datetime(2026, 8, d, 12, tzinfo=UTC) for d in range(20, 28)]
    population = pl.DataFrame({"market_id": [str(d) for d in range(20, 28)],
                               "window_start": points,
                               "window_end": [p + timedelta(minutes=5) for p in points]})
    frozen = {"purge_seconds": 1800, "folds": [
        {"name": "fold_1", "calibration_start": "2026-08-22", "evaluation_start": "2026-08-23", "evaluation_end": "2026-08-24"},
        {"name": "fold_2", "calibration_start": "2026-08-25", "evaluation_start": "2026-08-26", "evaluation_end": "2026-08-27"}]}
    roles = chronological_roles(population, frozen)
    later = split_rows(roles, frozen, frozen["folds"][1], "training")
    assert later["market_id"].to_list() == ["20", "21", "24"]
    assert split_rows(roles, frozen, frozen["folds"][1], "calibration")["market_id"].to_list() == ["25"]
    assert split_rows(roles, frozen, frozen["folds"][1], "evaluation")["market_id"].to_list() == ["26"]
