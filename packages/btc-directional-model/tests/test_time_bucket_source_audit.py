from datetime import UTC, datetime, timedelta

import polars as pl
import pytest

from btc_directional_model.time_bucket_source_audit import canonical_rows

PRODUCT = "chainlink_btcusd_reference_prices"
SOURCE = datetime(2026, 8, 20, tzinfo=UTC)


def direct_row(**changes):
    row = {
        "feed_id": "btc-usd",
        "report_sha256": "report",
        "source_timestamp": SOURCE,
        "valid_from_timestamp": SOURCE - timedelta(seconds=1),
        "provider_available_at": None,
        "received_at": SOURCE + timedelta(seconds=2),
        "price": 100.0,
        "strategy_key": "chainlink_btcusd_reference_price",
        "capture_artifact_id": "live-capture",
        "backfill_artifact_id": None,
    }
    return row | changes


def test_backfill_receipt_never_establishes_source_time_availability():
    frame = pl.DataFrame(
        [
            direct_row(
                strategy_key="chainlink_btcusd_reference_ticks_backfill",
                capture_artifact_id=None,
                backfill_artifact_id="backfill",
                received_at=SOURCE + timedelta(days=7),
            )
        ]
    )
    result, facts = canonical_rows(frame, PRODUCT)
    assert result.is_empty()
    assert facts["unproven_raw_rows"] == 1


def test_live_report_uses_receipt_and_preserves_proven_copy_over_backfill():
    frame = pl.DataFrame(
        [
            direct_row(
                strategy_key="chainlink_btcusd_reference_ticks_backfill",
                capture_artifact_id=None,
                backfill_artifact_id="backfill",
                received_at=SOURCE + timedelta(seconds=1),
            ),
            direct_row(),
        ]
    )
    result, facts = canonical_rows(frame, PRODUCT)
    assert result.height == 1
    assert result["available_at"].item() == SOURCE + timedelta(seconds=2)
    assert facts["duplicate_rows"] == 1


def test_conflicting_native_report_stops_source_checkpoint():
    with pytest.raises(ValueError, match="conflicting native identities"):
        canonical_rows(pl.DataFrame([direct_row(), direct_row(price=101.0)]), PRODUCT)


def test_pmdata_archive_overlap_deduplicates_native_report_without_relabeling():
    row = direct_row(
        provider_available_at=SOURCE + timedelta(seconds=1),
        strategy_key="pmdata",
        capture_artifact_id=None,
        backfill_artifact_id="archive",
    )
    result, facts = canonical_rows(
        pl.DataFrame([row, row]), "pmdata_chainlink_btcusd_reference_prices"
    )
    assert result.height == 1
    assert result["available_at"].item() == SOURCE + timedelta(seconds=2)
    assert facts["duplicate_rows"] == 1


def test_twap_windows_remain_distinct_native_observations():
    base = {
        "source_timestamp": SOURCE,
        "provider_received_at": SOURCE + timedelta(seconds=2),
        "valid_from_timestamp": SOURCE - timedelta(seconds=1),
        "twap_price": 100.0,
    }
    result, facts = canonical_rows(
        pl.DataFrame([base | {"window_seconds": 30}, base | {"window_seconds": 60}]),
        "pmdata_chainlink_btcusd_twap",
    )
    assert result.height == 2
    assert facts["duplicate_rows"] == 0
    assert set(result["window_seconds"]) == {30, 60}


def test_rtds_twap_waits_for_publication_and_receipt():
    result, _ = canonical_rows(
        pl.DataFrame(
            [
                {
                    "source_timestamp": SOURCE,
                    "published_at": SOURCE + timedelta(seconds=3),
                    "received_at": SOURCE + timedelta(seconds=2),
                    "window_seconds": 60,
                    "twap_price": 100.0,
                }
            ]
        ),
        "polymarket_chainlink_btcusd_twap",
    )
    assert result["available_at"].item() == SOURCE + timedelta(seconds=3)


def test_invalid_valid_from_is_excluded_without_repair():
    result, facts = canonical_rows(
        pl.DataFrame([direct_row(valid_from_timestamp=SOURCE + timedelta(seconds=1))]),
        PRODUCT,
    )
    assert result.is_empty()
    assert facts["invalid_raw_rows"] == 1
