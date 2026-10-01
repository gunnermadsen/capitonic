"""Synthetic native-feature and immutable-reference replay contract checks."""

import math
from datetime import UTC, datetime, timedelta

import polars as pl
import pytest

from btc_directional_model.time_bucket_execution import ObservedBook
from btc_directional_model.time_bucket_historical import (
    TRAINING_END,
    historical_cutoff,
    reconstruct_native,
    replay_reference,
    score_reference,
)

START = datetime(2026, 9, 16, tzinfo=UTC)
FEATURES = ["btc_cross_venue_boundary_gap_bps", "btc_return_180s_bps", "settlement_regime"]
FROZEN = {
    "fees": {"rate_assumption": 0.07},
    "quantity_grid": [1, 5, 10, 200],
    "scenarios": [{"name": "base"}],
    "book": {
        "arrival_ms": 150,
        "max_age_seconds": 2,
        "depth_haircut": 0.8,
        "participation_cap": 0.25,
        "reserve_per_share_per_leg": 0.005,
        "stress_extra_per_share_per_leg": 0.01,
    },
}


def schedule(seconds=None):
    seconds = (
        seconds if seconds is not None else [b + o for b in range(0, 260, 20) for o in [0, 9, 19]]
    )
    return pl.DataFrame(
        [
            {
                "point_id": f"m:{s}",
                "market_id": "m",
                "window_start": START,
                "window_end": START + timedelta(seconds=300),
                "decision_at": START + timedelta(seconds=s),
                "seconds_elapsed": s,
                "bucket_start": s // 20 * 20,
                "entry_offset": s % 20,
                "official_outcome": "up",
                "label_up": 1,
                "role": "holdout",
                "role_fold": "holdout",
                "evaluation_clock_eligible": True,
                "up_vwap_5": 0.4,
                "down_vwap_5": 0.6,
                "up_book_age_seconds": 0.1,
                "down_book_age_seconds": 0.1,
            }
            for s in seconds
        ]
    )


def candles():
    rows = []
    for second in range(300):
        opened = START + timedelta(seconds=second - 1)
        rows.append(
            {
                "source": "binance_spot",
                "symbol": "BTCUSDT",
                "open_timestamp": opened,
                "close_timestamp": opened + timedelta(milliseconds=999),
                "source_timestamp": opened,
                "received_at": opened + timedelta(seconds=1),
                "provider_available_at": opened + timedelta(milliseconds=999),
                "available_at": opened + timedelta(seconds=1),
                "open_price": 100.0 + second,
                "high_price": 102.0 + second,
                "low_price": 99.0 + second,
                "close_price": 101.0 + second,
                "base_volume": 2.0,
                "quote_volume": 200.0,
                "taker_buy_base_volume": 1.0,
                "taker_buy_quote_volume": 100.0,
                "trade_count": 3,
            }
        )
    return pl.DataFrame(rows)


def payload():
    # Native constant estimator is deterministic; no fit or real artifact is loaded.
    return {
        "feature_names": FEATURES,
        "estimator": {"constant": 0.9},
        "calibrator": None,
        "bucket_policies": {b: (0.5, 0.01, 0.95, "both") for b in range(0, 300, 10)},
    }


def scored(seconds=None):
    rows = schedule(seconds)
    return score_reference(
        rows,
        reconstruct_native(rows, candles(), FEATURES),
        payload(),
        START - timedelta(days=1),
        FROZEN,
    )


def test_native_closed_second_clock_anchor_and_future_independence():
    rows = schedule([180])
    result = reconstruct_native(rows, candles(), FEATURES)
    assert result["btc_cross_venue_boundary_gap_bps"].item() == pytest.approx(
        math.log(281 / 100) * 10000
    )
    assert result["btc_return_180s_bps"].item() == pytest.approx(math.log(281 / 101) * 10000)
    assert result["native_first_open_timestamp"].item() == START - timedelta(seconds=1)
    assert result["native_last_open_timestamp"].item() == START + timedelta(seconds=179)
    assert result["native_prefix_rows"].item() == 181
    changed = candles().with_columns(
        pl.when(pl.col("open_timestamp") >= START + timedelta(seconds=180))
        .then(999999.0)
        .otherwise(pl.col("close_price"))
        .alias("close_price")
    )
    assert result.equals(reconstruct_native(rows, changed, FEATURES))


def test_gap_and_late_constituent_are_not_filled_or_shifted():
    rows = schedule([180, 200])
    missing = candles().filter(pl.col("open_timestamp") != START + timedelta(seconds=19))
    assert reconstruct_native(rows, missing, FEATURES).is_empty()
    late = candles().with_columns(
        pl.when(pl.col("open_timestamp") == START + timedelta(seconds=19))
        .then(START + timedelta(seconds=181))
        .otherwise(pl.col("available_at"))
        .alias("available_at")
    )
    result = reconstruct_native(rows, late, FEATURES)
    assert result["point_id"].to_list() == ["m:200"]
    assert result["native_prefix_rows"].item() == 201
    absent = score_reference(
        rows,
        reconstruct_native(rows, missing, FEATURES),
        payload(),
        START - timedelta(days=1),
        FROZEN,
    )
    assert absent["probability_up"].null_count() == 2
    assert absent["chosen_side"].null_count() == 2
    assert not absent["admitted"].any()


@pytest.mark.parametrize("violation", ["duplicate", "source", "future_receipt"])
def test_invalid_native_identity_or_availability_fails_closed(violation):
    raw = candles()
    if violation == "duplicate":
        raw = pl.concat([raw, raw.head(1)])
    elif violation == "source":
        raw = raw.with_columns(pl.lit("rtds_direct_binance").alias("source"))
    else:
        raw = raw.with_columns(
            (pl.col("available_at") + pl.duration(seconds=1)).alias("received_at")
        )
    with pytest.raises(ValueError):
        reconstruct_native(schedule([180]), raw, FEATURES)


def test_unsupported_native_features_are_not_substituted():
    with pytest.raises(ValueError, match="Unsupported historical native feature"):
        reconstruct_native(schedule([180]), candles(), [*FEATURES, "oracle_round_freshness"])


def test_cutoff_uses_authorized_label_delay_and_never_ignores_later_proof():
    contract = {"seconds_after_close": 1800}
    assert historical_cutoff(contract) == TRAINING_END + timedelta(minutes=30)
    contract["verified_later_availability"] = [
        {
            "market_id": "m",
            "available_at": START.isoformat(),
            "evidence": "synthetic-proof",
            "semantics_verified": True,
        }
    ]
    assert historical_cutoff(contract) == START
    rows = schedule([180, 200])
    result = score_reference(
        rows,
        reconstruct_native(rows, candles(), FEATURES),
        payload(),
        START + timedelta(seconds=180),
        FROZEN,
    )
    assert result["admission_reason"].to_list() == [
        "historical_fit_calibration_policy_cutoff",
        "admitted",
    ]
    assert result["probability_up"].to_list() == [None, 0.9]


def test_all_thirteen_buckets_remain_with_native_grid_and_complete_case_exclusions():
    result = scored()
    assert result.height == 39
    assert result["bucket_start"].unique().sort().to_list() == list(range(0, 260, 20))
    off_grid = result.filter(pl.col("entry_offset") != 0)
    assert off_grid.height == 26
    assert off_grid["admission_reason"].unique().to_list() == ["outside_native_five_second_grid"]
    assert off_grid["probability_up"].null_count() == 26
    early = result.filter((pl.col("seconds_elapsed") < 180) & (pl.col("entry_offset") == 0))
    assert early["admission_reason"].unique().to_list() == [
        "missing_causal_native_complete_prefix_or_features"
    ]
    assert result.schema["probability_up"] == pl.Float64
    assert result.schema["chosen_side"] == pl.String


def test_fixed_policy_does_not_use_outcomes_and_purge_blocks_predictions():
    rows = schedule([180, 200])
    native = reconstruct_native(rows, candles(), FEATURES)
    first = score_reference(rows, native, payload(), START - timedelta(days=1), FROZEN)
    flipped = rows.with_columns(
        pl.lit(0).alias("label_up"), pl.lit("down").alias("official_outcome")
    )
    second = score_reference(flipped, native, payload(), START - timedelta(days=1), FROZEN)
    assert first.select("probability_up", "admitted", "chosen_side").equals(
        second.select("probability_up", "admitted", "chosen_side")
    )
    purged = rows.with_columns(pl.lit(False).alias("evaluation_clock_eligible"))
    result = score_reference(purged, native, payload(), START - timedelta(days=1), FROZEN)
    assert result["probability_up"].null_count() == 2
    assert result["admission_reason"].unique().to_list() == ["purged_boundary_market"]


def books(second):
    t = START + timedelta(seconds=second)
    return {
        ("m", side): (
            [t],
            [
                ObservedBook(
                    available_at=t,
                    source_timestamp=t,
                    received_at=t,
                    sampled_at=t,
                    provider_available_at=t,
                    bids="[[0.39,30]]",
                    asks="[[0.4,30]]",
                    token_id=side,
                    book_sha256="synthetic-" + side,
                )
            ],
        )
        for side in ["up", "down"]
    }


def test_sequential_replay_does_not_retry_an_unknown_first_attempt():
    result = replay_reference(scored([180, 200]), books(200), payload(), FROZEN)
    seq = result.filter(pl.col("economic_view") == "sequential_policy")
    assert seq.height == 2 * len(FROZEN["quantity_grid"])
    assert seq["point_id"].unique().to_list() == ["m:180"]
    assert seq["net_pnl"].null_count() == seq.height
    assert set(seq["order_type"]) == {"FAK", "FOK"}
    diagnostic = result.filter(
        (pl.col("economic_view") == "bucket_diagnostic") & (pl.col("point_id") == "m:200")
    )
    assert diagnostic.height == 2
    assert diagnostic["filled_quantity"].to_list() == [5.0, 5.0]


def test_independent_fak_partial_and_fok_zero_share_the_existing_engine():
    result = replay_reference(scored([180]), books(180), payload(), FROZEN)
    capacity = result.filter(
        (pl.col("economic_view") == "sequential_policy") & (pl.col("requested_quantity") == 10)
    )
    assert dict(zip(capacity["order_type"], capacity["filled_quantity"], strict=True)) == {
        "FAK": 7.5,
        "FOK": 0.0,
    }
    assert capacity.filter(pl.col("order_type") == "FAK")["fee_per_share"].item() == pytest.approx(
        0.07 * 0.4 * 0.6
    )
