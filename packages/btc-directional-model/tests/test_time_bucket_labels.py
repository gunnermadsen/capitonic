from datetime import UTC, datetime, timedelta

import polars as pl
import pytest

from btc_directional_model.time_bucket_labels import (
    eligible_labels,
    prediction_features_only,
    with_label_availability,
)
from btc_directional_model.time_bucket_protocol import split_rows
from btc_directional_model.time_bucket_reference import reference_splits


def test_assumed_label_use_is_not_before_thirty_minutes_and_later_evidence_wins():
    close = datetime(2026, 8, 1, 12, tzinfo=UTC)
    panel = pl.DataFrame({"market_id": ["assumed", "late", "earlier"], "window_end": [close] * 3})
    contract = {"seconds_after_close": 1800, "verified_later_availability": [
        {"market_id": "late", "available_at": (close + timedelta(hours=2)).isoformat(),
         "evidence": "synthetic verified receipt", "semantics_verified": True},
        {"market_id": "earlier", "available_at": (close + timedelta(minutes=1)).isoformat(),
         "evidence": "synthetic verified receipt", "semantics_verified": True}]}
    eligible = with_label_availability(panel, contract)
    assert eligible_labels(eligible, close + timedelta(minutes=29)).is_empty()
    assert eligible_labels(eligible, close + timedelta(minutes=30))["market_id"].to_list() == ["assumed", "earlier"]
    assert eligible_labels(eligible, close + timedelta(hours=2)).height == 3


def test_later_label_timing_preserves_global_split_exclusions():
    start = datetime(2026, 8, 1, tzinfo=UTC)
    frame = pl.DataFrame({"market_id": [str(i) for i in range(20)],
        "window_start": [start + timedelta(days=i, hours=1) for i in range(20)],
        "window_end": [start + timedelta(days=i, hours=1, minutes=5) for i in range(20)],
        "role": ["evaluation" if i == 6 else "holdout" if i == 7 else "training" for i in range(20)],
        "role_fold": [None] * 20})
    contract = {"seconds_after_close": 1800, "verified_later_availability": [
        {"market_id": "2", "available_at": (start + timedelta(days=30)).isoformat(),
         "evidence": "synthetic delayed resolution", "semantics_verified": True}]}
    panel = with_label_availability(frame, contract)
    frozen = {"purge_seconds": 1800, "reference": {"calibration_days": 2, "initial_fit_min_days": 5}}
    train, cal = reference_splits(panel, (start + timedelta(days=15)).date(), frozen)
    assert not set(train["market_id"]) & {"2", "6", "7", "15", "16", "17", "18", "19"}
    assert not set(cal["market_id"]) & {"2", "6", "7"}
    fold = {"calibration_start": "2026-08-14"}
    outer = eligible_labels(split_rows(panel, frozen, fold, "training"), start + timedelta(days=13))
    assert not set(outer["market_id"]) & {"2", "6", "7"}


def test_unproven_override_and_retrospective_features_fail_closed():
    frame = pl.DataFrame({"market_id": ["x"], "window_end": [datetime(2026, 8, 1, tzinfo=UTC)]})
    with pytest.raises(ValueError, match="verified evidence"):
        with_label_availability(frame, {"seconds_after_close": 1800, "verified_later_availability": [
            {"market_id": "x", "available_at": "2026-08-01T03:00:00+00:00"}]})
    for feature in ["official_outcome", "label_up", "label_use_at", "up_fak_net_5", "stress_pnl"]:
        with pytest.raises(ValueError, match="Retrospective"):
            prediction_features_only(["rtds_chainlink_return_30_bps", feature])
    prediction_features_only(["rtds_chainlink_return_30_bps", "reference_fills_1800"])
