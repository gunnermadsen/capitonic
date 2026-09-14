from __future__ import annotations

import polars as pl

from btc_directional_model.historical_aggregate_tournament import (
    _aggregate_predictions,
    _available_predictive_metrics,
    _bucket_contracts,
    _recipe_for,
)


def test_bucket_contracts_freeze_ten_second_ranges() -> None:
    raw = {"buckets": {"start_second": 60, "end_second": 239, "width_seconds": 10}}
    buckets = _bucket_contracts(raw)
    assert len(buckets) == 18
    assert buckets[0]["start_second"] == 60
    assert buckets[-1]["end_second"] == 239


def test_recipe_mapping_preserves_declared_priority() -> None:
    recipes = [
        {"name": "fair", "match_tokens": ["fair_value"]},
        {"name": "cross", "match_tokens": ["crossvenue"]},
    ]
    assert _recipe_for("specialist_distilled_fair_value", recipes) == "fair"
    assert _recipe_for("crossvenue_middle_specialist", recipes) == "cross"


def test_consensus_abstains_when_member_directions_disagree() -> None:
    base = pl.DataFrame(
        {
            "market_id": ["a", "b"],
            "window_start": ["2026-08-26T00:00:00Z", "2026-08-26T00:05:00Z"],
            "observed_at": ["2026-08-26T00:01:00Z", "2026-08-26T00:06:00Z"],
            "seconds_elapsed": [60, 60],
            "label_up": [1, 0],
        }
    ).with_columns(
        pl.col("window_start").str.to_datetime(time_zone="UTC"),
        pl.col("observed_at").str.to_datetime(time_zone="UTC"),
    )
    left = base.with_columns(pl.Series("probability", [0.8, 0.8]), pl.lit("left").alias("candidate"))
    right = base.with_columns(pl.Series("probability", [0.7, 0.2]), pl.lit("right").alias("candidate"))
    result = _aggregate_predictions([left, right], "consensus", "aggregate")
    assert result["probability"].to_list() == [0.75, None]
    metrics = _available_predictive_metrics(result)
    assert metrics["rows"] == 1
