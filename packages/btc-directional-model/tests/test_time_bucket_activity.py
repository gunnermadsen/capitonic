from datetime import UTC, datetime, timedelta

import polars as pl

from btc_directional_model.time_bucket_activity import activity_states


def fixture():
    start = datetime(2026, 8, 1, tzinfo=UTC)
    rows = []
    for market in range(6):
        for bucket in range(0, 260, 20):
            at = start + timedelta(seconds=market * 300 + bucket + 19)
            rows.append({"point_id": f"{market}@{bucket}", "market_id": str(market), "decision_at": at,
                         "evidence_available_at": at + timedelta(milliseconds=150), "reference_known": True,
                         "filled_quantity": 0., "selected_attempt": False, "probability_up": .6})
    query = pl.DataFrame({"decision_at": [start + timedelta(seconds=1800)]})
    frozen = {"reference": {"quiet_seconds": 1800}, "bucket_starts": list(range(0, 260, 20)), "regular_entry_offset": 19}
    return query, pl.DataFrame(rows), frozen


def test_complete_no_fills_is_quiet_but_missing_decision_is_unknown():
    query, reference, frozen = fixture()
    assert activity_states(query, reference, frozen)["reference_quiet"][0] == 1
    assert activity_states(query, reference.slice(1), frozen)["reference_quiet"][0] is None
    unknown = reference.with_columns(pl.when(pl.col("point_id") == "1@20").then(False)
                                    .otherwise(pl.col("reference_known")).alias("reference_known"))
    assert activity_states(query, unknown, frozen)["reference_quiet"][0] is None


def test_simulated_fill_changes_quiet_only_after_observed_arrival():
    query, reference, frozen = fixture()
    changed = reference.with_columns(pl.when(pl.col("point_id") == "5@240").then(2.)
                                     .otherwise(pl.col("filled_quantity")).alias("filled_quantity"),
                                     (pl.col("point_id") == "5@240").alias("selected_attempt"))
    result = activity_states(query, changed, frozen)
    assert result["reference_quiet"][0] == 0
    assert result["reference_fills_1800"][0] == 1
    future = changed.with_columns(pl.when(pl.col("point_id") == "5@240").then(query["decision_at"][0])
                                  .otherwise(pl.col("evidence_available_at")).alias("evidence_available_at"))
    assert activity_states(query, future, frozen)["reference_quiet"][0] is None
