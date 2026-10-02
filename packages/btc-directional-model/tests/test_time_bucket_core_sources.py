import json
import time
from datetime import UTC, datetime, timedelta

import polars as pl
import pytest

from btc_directional_model import time_bucket_core_sources as core_sources
from btc_directional_model.time_bucket_core_sources import (
    BOOKS,
    TICKS,
    SourceConflict,
    canonical_core_rows,
)

NOW = datetime(2026, 8, 20, tzinfo=UTC)


def tick(**changes):
    return {
        "source": "rtds_chainlink", "symbol": "BTCUSD", "price": 100.0,
        "source_timestamp": NOW, "envelope_timestamp": NOW + timedelta(milliseconds=100),
        "received_at": NOW + timedelta(seconds=1), "integrity_status": "ok",
        "dedup_key": f"rtds_chainlink:BTCUSD:{int(NOW.timestamp() * 1000)}:-:100",
    } | changes


def book(**changes):
    return {
        "source": "polymarket_clob_market", "market_id": "market", "token_id": "token",
        "connection_epoch": "epoch", "ingest_sequence": 1, "sampled_at": NOW,
        "source_timestamp": NOW, "provider_available_at": NOW,
        "received_at": NOW + timedelta(milliseconds=100),
        "strategy_key": "polymarket_btc_five_minute_orderbooks", "capture_artifact_id": "capture",
        "outcome": "up", "bids": '[["0.4","10"]]', "asks": '[["0.6","20"]]',
        "best_bid": 0.4, "best_ask": 0.6,
    } | changes


def test_duplicate_tick_keeps_earliest_demonstrated_copy_and_source_identity():
    late = tick(received_at=NOW + timedelta(seconds=4))
    result, facts = canonical_core_rows(pl.DataFrame([late, tick()]), TICKS)
    assert result.height == 1
    assert result["available_at"].item() == NOW + timedelta(seconds=1)
    assert result["source"].item() == "rtds_chainlink"
    assert facts["duplicate_rows"] == 1


@pytest.mark.parametrize("change", [{"price": 101.0}, {"source_timestamp": NOW + timedelta(seconds=1)}])
def test_tick_native_value_conflict_blocks_source(change):
    with pytest.raises(SourceConflict):
        canonical_core_rows(pl.DataFrame([tick(), tick(**change)]), TICKS)


def test_tick_timestamp_key_rejects_cross_day_identity_ambiguity():
    with pytest.raises(SourceConflict, match="timestamp keys"):
        canonical_core_rows(pl.DataFrame([tick(source_timestamp=NOW + timedelta(days=1))]), TICKS)


@pytest.mark.parametrize("integrity", ["stale", "future", "clock_skew", None])
def test_tick_non_ok_integrity_is_ineligible(integrity):
    result, facts = canonical_core_rows(pl.DataFrame([tick(integrity_status=integrity)]), TICKS)
    assert result.is_empty()
    assert facts["invalid_reasons"] == {"integrity_not_ok": 1}


def test_tick_envelope_later_than_receipt_delays_availability():
    result, _ = canonical_core_rows(
        pl.DataFrame([tick(envelope_timestamp=NOW + timedelta(seconds=3))]), TICKS
    )
    assert result["available_at"].item() == NOW + timedelta(seconds=3)


def test_book_union_deduplicates_native_identity_with_late_receipt():
    rows = [book(received_at=NOW + timedelta(seconds=3)), book()]
    result, facts = canonical_core_rows(pl.DataFrame(rows), BOOKS)
    assert facts["duplicate_rows"] == 1
    assert result["available_at"].item() == NOW + timedelta(milliseconds=100)
    assert result["bids"].item() == rows[0]["bids"]


@pytest.mark.parametrize("change", [
    {"asks": '[["0.61","20"]]'},
    {"source_timestamp": NOW - timedelta(seconds=1)},
    {"asks": '[[0.6, 20]]'},
])
def test_book_conflicts_include_numerically_equivalent_json(change):
    with pytest.raises(SourceConflict):
        canonical_core_rows(pl.DataFrame([book(), book(**change)]), BOOKS)


@pytest.mark.parametrize("change,reason", [
    ({"bids": '[["0.7","10"]]', "best_bid": 0.7}, "crossed_or_locked"),
    ({"bids": "[]", "best_bid": None}, "bids_empty"),
    ({"asks": "[]", "best_ask": None}, "asks_empty"),
    ({"asks": '[["1","20"]]'}, "asks_invalid_level"),
    ({"asks": '[["0.6","NaN"]]'}, "asks_invalid_level"),
    ({"asks": json.dumps([{"price": "0.6", "size": "20"}])}, "asks_invalid_json_shape"),
    ({"strategy_key": None}, "not_live_book_strategy"),
    ({"provider_available_at": None}, "missing_observation_time"),
])
def test_invalid_and_empty_books_are_counted_without_imputation(change, reason):
    result, facts = canonical_core_rows(pl.DataFrame([book(**change)]), BOOKS)
    assert result.is_empty()
    assert facts["invalid_reasons"] == {reason: 1}


def test_book_provider_availability_is_preserved_and_limits_use():
    result, _ = canonical_core_rows(
        pl.DataFrame([book(provider_available_at=NOW + timedelta(seconds=3))]), BOOKS
    )
    assert result["available_at"].item() == NOW + timedelta(seconds=3)
    assert result["provider_available_at"].item() == result["available_at"].item()


def test_old_live_book_without_capture_id_retains_original_observation_proof():
    result, facts = canonical_core_rows(pl.DataFrame([book(capture_artifact_id=None)]), BOOKS)
    assert result.height == 1
    assert result["capture_artifact_id"].item() is None
    assert facts["eligible_without_capture_id"] == 1


def frozen_tick_run(tmp_path):
    run = tmp_path / "run"
    (run / "inputs").mkdir(parents=True)
    source = tmp_path / "ticks.parquet"
    pl.DataFrame([tick()]).write_parquet(source)
    inventory = {
        "product": TICKS,
        "frozen_at": datetime.now(UTC).isoformat(),
        "files": [{
            "path": str(source), "rows": 1, "bytes": source.stat().st_size,
            "sha256": core_sources.sha256(source), "applicable": True,
            "bounds": {"source_timestamp": {
                "min": NOW.isoformat(), "max": NOW.isoformat(), "null_count": 0,
            }},
        }],
    }
    (run / "inputs" / f"{TICKS}-inventory.json").write_text(json.dumps(inventory))
    return run, source


def test_resume_checks_output_hash_without_reprocessing(tmp_path, monkeypatch):
    run, _ = frozen_tick_run(tmp_path)
    first = core_sources.validate_product(run, TICKS, time.monotonic() + 60)

    def forbidden_recompute(*args):
        raise AssertionError("verified output must not be recomputed")

    monkeypatch.setattr(core_sources, "canonical_core_rows", forbidden_recompute)
    second = core_sources.validate_product(run, TICKS, time.monotonic() + 60)
    assert second["days"] == json.loads(json.dumps(first["days"], default=str))
    assert second["status"] == "complete"


def test_resume_rejects_changed_output_without_overwriting(tmp_path):
    run, _ = frozen_tick_run(tmp_path)
    result = core_sources.validate_product(run, TICKS, time.monotonic() + 60)
    output = run / "inputs/validated-core" / TICKS / f"{NOW.date()}.parquet"
    original = output.read_bytes()
    output.write_bytes(original + b"changed")
    with pytest.raises(ValueError, match="normalized output changed"):
        core_sources.validate_product(run, TICKS, time.monotonic() + 60)
    assert output.read_bytes() == original + b"changed"
    assert result["days"][str(NOW.date())]["output"]["sha256"] != core_sources.sha256(output)


def test_resume_rejects_different_frozen_inventory(tmp_path):
    run, _ = frozen_tick_run(tmp_path)
    core_sources.validate_product(run, TICKS, time.monotonic() + 60)
    inventory = run / "inputs" / f"{TICKS}-inventory.json"
    original = inventory.read_text()
    inventory.write_text(original + "\n")
    with pytest.raises(ValueError, match="source/config evidence differs"):
        core_sources.validate_product(run, TICKS, time.monotonic() + 60)
