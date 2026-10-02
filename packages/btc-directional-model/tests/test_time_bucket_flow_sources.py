import hashlib
import json
from datetime import UTC, datetime, timedelta
from decimal import Decimal

import polars as pl
import pytest

from btc_directional_model import time_bucket_flow_sources as flow

NOW = datetime(2026, 8, 20, tzinfo=UTC)
CANDLES = "binance_spot_one_second_ohlcv"
TRADES = "binance_spot_aggregate_trades"
SPOT_L2 = "binance_spot_l2_features"
FUTURES_L2 = "binance_futures_l2_features"
SNAPSHOTS = "binance_spot_l2_snapshots"
OI = "binance_futures_open_interest"


def candle(**changes):
    return {
        "source": "binance_spot",
        "symbol": "BTCUSDT",
        "open_timestamp": NOW,
        "close_timestamp": NOW + timedelta(milliseconds=999),
        "provider_available_at": NOW + timedelta(seconds=1, milliseconds=20),
        "received_at": NOW + timedelta(seconds=1, milliseconds=40),
        "open_price": "100",
        "high_price": "102",
        "low_price": "99",
        "close_price": "101",
        "base_volume": "2",
        "quote_volume": "201",
        "taker_buy_base_volume": "1",
        "taker_buy_quote_volume": "100",
        "trade_count": 2,
    } | changes


def trade(**changes):
    return {
        "source": "binance_spot",
        "symbol": "BTCUSDT",
        "aggregate_trade_id": 5,
        "trade_timestamp": NOW,
        "provider_available_at": NOW + timedelta(milliseconds=10),
        "received_at": NOW + timedelta(milliseconds=20),
        "price": "100.1234567890",
        "quantity": "2.0000000001",
        "first_trade_id": 50,
        "last_trade_id": 51,
        "buyer_maker": True,
        "best_match": True,
    } | changes


def l2(product=SPOT_L2, **changes):
    row = {name: "0" for name in flow.L2_VALUES}
    row.update(
        {
            "midpoint": "100",
            "microprice": "100.001",
            "symbol": "BTCUSDT",
            "second_start": NOW,
            "source_event_timestamp": NOW + timedelta(milliseconds=700),
            "provider_received_at": NOW + timedelta(milliseconds=800),
            "available_at": NOW + timedelta(milliseconds=900),
            "source_update_id": 500,
            "feature_schema_version": "binance-spot-btcusdt-l2-one-second-features-v1"
            if product == SPOT_L2
            else "binance-btcusdt-l2-one-second-features-v1",
            "quality_status": "qualified",
            "artifact_id": "original-provider-capture",
        }
    )
    for side in ("bid", "ask"):
        for depth in (5, 10, 20):
            row[f"{side}_depth_{depth}"] = str(depth)
        row[f"{side}_depth_concentration_20"] = "0.25"
    return row | changes


def snapshot(**changes):
    policy = {
        "sample_depth": 2,
        "sample_interval_ms": 1000,
        "symbol": "BTCUSDT",
        "source": "binance_spot_diff_depth",
        "selection": "latest_contiguous_update_at_or_after_interval",
    }
    encoded = json.dumps(policy, separators=(",", ":"), sort_keys=True)
    return {
        "source": "binance_spot",
        "symbol": "BTCUSDT",
        "source_timestamp": NOW,
        "source_update_id": 123,
        "received_at": NOW - timedelta(milliseconds=100),
        "connection_epoch": "original-epoch",
        "sample_depth": 2,
        "bids": '[["99.0","2.000"],["98.00","3"]]',
        "asks": '[["100","4"],["101","5"]]',
        "sampling_policy": encoded,
        "sampling_policy_sha256": hashlib.sha256(encoded.encode()).hexdigest(),
    } | changes


def interest(**changes):
    return {
        "source": "binance_usd_m_futures",
        "symbol": "BTCUSDT",
        "source_timestamp": NOW,
        "period_seconds": "300",
        "sum_open_interest": "100",
        "sum_open_interest_value": "10000.123456789012345678",
        "provider_available_at": None,
        "received_at": NOW + timedelta(days=1),
    } | changes


def test_closed_candle_waits_for_original_receipt_and_rejects_backfill_copy():
    good = candle()
    result, facts = flow.canonical_rows(
        pl.DataFrame(
            [
                good,
                candle(provider_available_at=None, received_at=NOW + timedelta(days=10)),
            ]
        ),
        CANDLES,
    )
    assert result.height == 1
    assert result["available_at"].item() == good["received_at"]
    assert result["open_price"].item() == Decimal(100)
    assert facts["duplicate_rows"] == 1
    assert facts["unproven_raw_rows"] == 1


@pytest.mark.parametrize(
    "change",
    [
        {"high_price": "98"},
        {"close_timestamp": NOW},
        {"taker_buy_base_volume": "3"},
        {"trade_count": 0},
        {"close_price": "NaN"},
        {"base_volume": "-1"},
        {"provider_available_at": NOW},
        {"trade_count": 1.5},
    ],
)
def test_invalid_candles_are_excluded_without_repair(change):
    result, facts = flow.canonical_rows(pl.DataFrame([candle(**change)]), CANDLES)
    assert result.is_empty()
    assert facts["invalid_raw_rows"] == 1


def test_prints_preserve_actual_decimal_values_and_do_not_admit_receipt_only_backfills():
    result, facts = flow.canonical_rows(
        pl.DataFrame(
            [
                trade(),
                trade(aggregate_trade_id=6, provider_available_at=None),
            ]
        ),
        TRADES,
    )
    assert result.height == 1
    assert result["price"].item() == Decimal("100.1234567890")
    assert result["quantity"].item() == Decimal("2.0000000001")
    assert result["buyer_maker"].item() is True
    assert result["aggregate_trade_id"].item() == 5
    assert facts["unproven_raw_rows"] == 1


@pytest.mark.parametrize(
    "product,row,changes",
    [
        (TRADES, trade, {"quantity": "3"}),
        (CANDLES, candle, {"close_price": "100"}),
        (SPOT_L2, l2, {"source_update_id": 501}),
        (OI, interest, {"sum_open_interest": "101"}),
        (SNAPSHOTS, snapshot, {"bids": '[["99","3"],["98","3"]]'}),
    ],
)
def test_conflicting_native_identity_blocks_day(product, row, changes):
    with pytest.raises(ValueError, match="conflicting native identities"):
        flow.canonical_rows(pl.DataFrame([row(), row(**changes)]), product)


def test_l2_union_deduplicates_exact_facts_and_waits_for_second_close():
    row = l2()
    strings = {k: (str(v) if isinstance(v, datetime) else v) for k, v in row.items()}
    frame = pl.concat([pl.DataFrame([row]), pl.DataFrame([strings])], how="diagonal_relaxed")
    result, facts = flow.canonical_rows(frame, SPOT_L2)
    assert result.height == 1
    assert result["available_at"].item() == NOW + timedelta(seconds=1)
    assert result["stored_available_at"].item() == NOW + timedelta(milliseconds=900)
    assert result["provider_received_at"].item() == NOW + timedelta(milliseconds=800)
    assert facts["duplicate_rows"] == 1
    wrong, _ = flow.canonical_rows(pl.DataFrame([l2(FUTURES_L2)]), SPOT_L2)
    assert wrong.is_empty()


@pytest.mark.parametrize(
    "change",
    [
        {"provider_received_at": None},
        {"quality_status": "unqualified"},
        {"imbalance_20": "1.01"},
        {"bid_depth_10": "1"},
        {"midpoint": "Infinity"},
        {"available_at": NOW},
        {"artifact_id": None},
    ],
)
def test_l2_invalid_or_unproven_rows_are_not_repaired(change):
    result, _ = flow.canonical_rows(pl.DataFrame([l2(**change)]), SPOT_L2)
    assert result.is_empty()


def test_snapshot_preserves_all_levels_and_handles_clock_lead():
    result, _ = flow.canonical_rows(pl.DataFrame([snapshot()]), SNAPSHOTS)
    assert result["available_at"].item() == NOW
    assert len(json.loads(result["bids"].item())) == 2
    assert Decimal(json.loads(result["asks"].item())[1][0]) == Decimal(101)
    assert result["connection_epoch"].item() == "original-epoch"
    crossed, _ = flow.canonical_rows(
        pl.DataFrame([snapshot(asks='[["98","4"],["101","5"]]')]), SNAPSHOTS
    )
    assert crossed.is_empty()


def test_open_interest_does_not_move_backfill_to_its_economic_timestamp():
    result, facts = flow.canonical_rows(pl.DataFrame([interest()]), OI)
    assert result["available_at"].item() == NOW + timedelta(days=1)
    assert result["sum_open_interest_value"].item() == Decimal("10000.123456789012345678")
    assert facts["delayed_eligible_rows"] == 1
    assert facts["freshness_applied"] is False


def test_null_or_naive_native_time_and_multi_day_inputs_fail_closed():
    for row in [trade(trade_timestamp=None), trade(trade_timestamp=NOW.replace(tzinfo=None))]:
        with pytest.raises(ValueError, match="timestamp"):
            flow.canonical_rows(pl.DataFrame([row]), TRADES)
    with pytest.raises(ValueError, match="one native UTC day"):
        flow.canonical_rows(
            pl.DataFrame([trade(), trade(trade_timestamp=NOW + timedelta(days=1))]), TRADES
        )


def fixture_run(tmp_path, product=CANDLES, rows=None):
    run = tmp_path / "run"
    (run / "inputs").mkdir(parents=True)
    (run / "manifests").mkdir()
    source = tmp_path / "source.parquet"
    pl.DataFrame(rows or [candle()]).write_parquet(source, row_group_size=1)
    coordinate = flow._spec(product)[0]
    frame = pl.read_parquet(source)
    values = frame[coordinate]
    entry = {
        "path": str(source),
        "bytes": source.stat().st_size,
        "rows": frame.height,
        "frozen": True,
        "sha256": "already-frozen-source-hash",
        "timestamp_statistics": {coordinate: {"min": str(values.min()), "max": str(values.max())}},
    }
    freeze = {
        "complete": True,
        "frozen_at": str(datetime.now(UTC) + timedelta(seconds=5)),
        "groups": [{"product_key": flow.PRODUCTS[product][0], "files": [entry]}],
    }
    (run / "inputs/optional-source-freeze.json").write_text(json.dumps(freeze))
    (run / "manifests/optional-source-causal-contract.json").write_text(
        json.dumps(
            {
                "classification_complete": True,
                "products": [],
            }
        )
    )
    return run, source, entry


def test_daily_audit_resumes_verified_outputs_and_recovers_manifest_interruption(
    tmp_path, monkeypatch
):
    run, source, _ = fixture_run(tmp_path)
    report = flow.audit_product(run, CANDLES)
    assert report["status"] == "completed"
    target = run / "inputs/validated-flow" / CANDLES / "2026-08-20.parquet"
    assert pl.read_parquet(target)["source_timestamp"].item() == NOW
    original = target.read_bytes()
    source.unlink()  # A verified completed output must not rescan or rehash its original archive.
    report_path = run / "manifests" / f"flow-source-validation-{CANDLES}.json"
    report_path.unlink()
    flow.audit_product(run, CANDLES)
    monkeypatch.setattr(flow, "read_day", lambda *a: pytest.fail("Unexpected source reread"))
    flow.audit_product(run, CANDLES)
    assert target.read_bytes() == original
    coverage = pl.read_parquet(run / "metrics/flow-source-hourly-coverage.parquet")
    assert coverage["eligible_rows"].sum() == 1


def test_resume_rejects_changed_output_without_overwriting_it(tmp_path):
    run, _, _ = fixture_run(tmp_path)
    flow.audit_product(run, CANDLES)
    target = run / "inputs/validated-flow" / CANDLES / "2026-08-20.parquet"
    pl.DataFrame({"tampered": [1]}).write_parquet(target)
    changed = target.read_bytes()
    with pytest.raises(ValueError, match="lacks validator evidence"):
        flow.audit_product(run, CANDLES)
    assert target.read_bytes() == changed


def test_frozen_read_selects_one_day_and_stops_at_memory_bound(tmp_path, monkeypatch):
    rows = [interest(), interest(source_timestamp=NOW + timedelta(days=1))]
    _, _, f = fixture_run(tmp_path, OI, rows)
    f.update(file_index=0, group=OI)
    result = flow.read_day(
        [f], "source_timestamp", NOW.date(), datetime.now(UTC) + timedelta(seconds=5)
    )
    assert result.height == 1
    monkeypatch.setattr(flow, "MAX_DAY_BYTES", 1)
    with pytest.raises(MemoryError, match="bounded day"):
        flow.read_day([f], "source_timestamp", NOW.date(), datetime.now(UTC) + timedelta(seconds=5))


def test_cross_day_aggregate_id_regression_blocks_next_output(tmp_path):
    later = NOW + timedelta(days=1)
    rows = [trade(), trade(trade_timestamp=later, provider_available_at=later, received_at=later)]
    run, _, _ = fixture_run(tmp_path, TRADES, rows)
    with pytest.raises(ValueError, match="native-ID ranges overlap"):
        flow.audit_product(run, TRADES)
    assert not (run / "inputs/validated-flow" / TRADES / "2026-08-21.parquet").exists()


def test_union_audit_handles_typed_and_string_drains_without_rehashing(tmp_path):
    run, source, entry = fixture_run(tmp_path, SPOT_L2, [l2()])
    verified = tmp_path / "verified.parquet"
    strings = {
        k: str(v).replace("+00:00", "+00") if isinstance(v, datetime) else str(v)
        for k, v in l2().items()
    }
    pl.DataFrame([strings]).write_parquet(verified)
    freeze_path = run / "inputs/optional-source-freeze.json"
    freeze = json.loads(freeze_path.read_text())
    second = dict(entry, path=str(verified), bytes=verified.stat().st_size)
    freeze["groups"].append({"product_key": "binance_spot_l2_features_verified", "files": [second]})
    freeze_path.write_text(json.dumps(freeze))
    report = flow.audit_product(run, SPOT_L2)
    assert report["daily"][0]["facts"]["duplicate_rows"] == 1
    output = pl.read_parquet(run / "inputs/validated-flow" / SPOT_L2 / "2026-08-20.parquet")
    assert output.height == 1
    assert output["available_at"].item() == NOW + timedelta(seconds=1)
    assert source.exists()


def test_optional_oi_numeric_null_is_allowed_but_nan_is_not():
    for value, expected in [(None, 1), ("NaN", 0), ("-1", 0), ("100", 1)]:
        result, _ = flow.canonical_rows(pl.DataFrame([interest(cmc_circulating_supply=value)]), OI)
        assert result.height == expected


def test_existing_partial_evidence_is_preserved(tmp_path):
    path = tmp_path / "manifest.json"
    partial = tmp_path / "manifest.json.partial"
    partial.write_text("interrupted evidence")
    with pytest.raises(ValueError, match="requires inspection"):
        flow._atomic_json(path, {"test": True})
    assert partial.read_text() == "interrupted evidence"


def test_trade_dictionary_encoding_preserves_complete_output_and_facts():
    frame = pl.DataFrame([trade(), trade(aggregate_trade_id=6)])
    frame = frame.with_columns(
        pl.lit("capture-original").alias("capture_artifact_id"),
        pl.lit(TRADES).alias("strategy_key"),
    )
    original, original_facts = flow.canonical_rows(frame, TRADES)
    compact = flow.compact_trade_lineage(frame)
    decoded, compact_facts = flow.canonical_rows(compact, TRADES)
    assert decoded.equals(original)
    assert decoded.schema == original.schema
    assert original_facts == compact_facts


def test_cross_batch_trade_conflict_survives_dictionary_encoding(tmp_path):
    rows = [trade() for _ in range(16_384)] + [trade(quantity="3")]
    _, _, entry = fixture_run(tmp_path, TRADES, rows)
    entry.update(file_index=0, group=TRADES)
    frame = flow.read_day(
        [entry], "trade_timestamp", NOW.date(), datetime.now(UTC) + timedelta(seconds=10)
    )
    with pytest.raises(ValueError, match="conflicting native identities"):
        flow.canonical_rows(frame, TRADES)


def test_compatible_code_requires_explicit_frozen_input_and_test_evidence():
    implementation = {
        "validator_sha256": "new-code",
        "source_freeze_sha256": "same-freeze",
        "causal_contract_sha256": "same-causal-contract",
    }
    report = {
        "binding": "old-binding",
        "implementation_compatibility": [
            {
                "kind": "lossless_trade_lineage_encoding",
                "preserved_binding": "old-binding",
                "semantics_unchanged": True,
                "previous_validator_sha256": "old-code",
                "focused_tests": {"status": "passed"},
                **implementation,
            }
        ],
    }
    assert flow.accepted_binding(report, "new-binding", implementation) == "old-binding"
    with pytest.raises(ValueError, match="incompatible"):
        flow.accepted_binding(
            report, "new-binding", implementation | {"source_freeze_sha256": "changed"}
        )
    with pytest.raises(ValueError, match="incompatible"):
        flow.accepted_binding({"binding": "old-binding"}, "new-binding", implementation)


@pytest.mark.parametrize("digest", ["ff" * 32, "Ab" * 32, "not-hex", None])
def test_payload_digest_memory_encoding_is_lossless_including_non_hex(digest):
    frame = pl.DataFrame(
        [trade(payload_sha256=digest), trade(aggregate_trade_id=6, payload_sha256="ab" * 32)]
    )
    expected, facts = flow.canonical_rows(frame, TRADES)
    actual, compact_facts = flow.canonical_rows(flow.compact_trade_lineage(frame), TRADES)
    assert actual.equals(expected)
    assert actual.schema == expected.schema
    assert facts == compact_facts


def test_backfilled_digest_remains_excluded_after_memory_encoding():
    frame = pl.DataFrame([trade(payload_sha256="ff" * 32, provider_available_at=None)])
    actual, facts = flow.canonical_rows(flow.compact_trade_lineage(frame), TRADES)
    assert actual.is_empty()
    assert facts["unproven_raw_rows"] == 1
    assert actual.schema["payload_sha256"] == pl.String
