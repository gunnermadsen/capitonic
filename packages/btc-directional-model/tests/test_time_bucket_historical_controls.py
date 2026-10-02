"""Synthetic immutable-control scoring and replay checks; no real model fit."""

from datetime import UTC, datetime, timedelta
from types import SimpleNamespace

import numpy as np
import polars as pl
import pytest

from btc_directional_model import time_bucket_historical_controls as controls
from btc_directional_model.conservative_selective_training import PRICE_FEATURES
from btc_directional_model.continuous_edge_training import CORE_FEATURES
from btc_directional_model.time_bucket_execution import ObservedBook

START = datetime(2026, 9, 23, tzinfo=UTC)
FEATURES = ["btc_return_1s_bps"]
FROZEN = {
    "fees": {"rate_assumption": 0.07},
    "quantity_grid": [1, 5, 10, 15, 200],
    "scenarios": [{"name": "base"}],
    "purge_seconds": 1800,
    "book": {"arrival_ms": 150, "max_age_seconds": 2, "depth_haircut": 0.8,
             "participation_cap": 0.25, "reserve_per_share_per_leg": 0.005,
             "stress_extra_per_share_per_leg": 0.01},
}


class Classifier:
    def predict_proba(self, matrix):
        return np.tile([0.1, 0.9], (len(matrix), 1))


class Regressor:
    def predict(self, matrix):
        return np.full(len(matrix), 0.9)


def fixture(name=controls.TERMINAL):
    raw = {"execution": {"freshness_seconds": 2, "execution_reserve_per_share": 0.005,
                          "stress_slippage_per_share": 0.01}, "calibration": {"safety_floor": 0.02}}
    model = SimpleNamespace(features=FEATURES, estimator=Regressor(), calibrator=None,
                            neutralized_columns=())
    payload = {"model": model, "policy": controls.specialist.Policy("both", 5, 0.02, 0.75, 0.9)}
    if name == controls.CAPACITY:
        payload = {"estimator": Classifier(), "calibrator": Classifier(), "calibration_ece": 0.03,
                   "quantity_policies": {"5": None, "10": None}}
    return {"features": FEATURES, "raw": raw, "payload": payload, "quantities": [5, 10],
            "cutoff": START - timedelta(hours=1)}


def rows(seconds=None):
    seconds = seconds if seconds is not None else [b + o for b in range(0, 260, 20) for o in (0, 9, 19)]
    return pl.DataFrame([{"point_id": f"m@{s}", "market_id": "m", "window_start": START,
        "window_end": START + timedelta(seconds=300), "decision_at": START + timedelta(seconds=s),
        "seconds_elapsed": s, "bucket_start": s // 20 * 20, "entry_offset": s % 20,
        "official_outcome": "up", "label_up": 1, "role": "holdout", "role_fold": "holdout",
        "evaluation_clock_eligible": True, "decision_grid": "tournament_grid",
        "up_vwap_5": 0.4, "down_vwap_5": 0.6, "up_vwap_10": 0.4, "down_vwap_10": 0.6,
        "up_book_age_seconds": 0.1, "down_book_age_seconds": 0.1} for s in seconds])


def native(frame):
    return frame.select("point_id").with_columns(pl.lit(0.5).alias(FEATURES[0]))


def admitted(seconds=None, name=controls.TERMINAL):
    frame = rows(seconds)
    control = fixture(name)
    scored = controls.score_control(name, frame, native(frame), control)
    return controls.admit_control(name, scored, control, FROZEN)


def test_all13_current_buckets_retained_with_typed_missing_probabilities():
    result = admitted()
    assert result.height == 39
    assert result["bucket_start"].n_unique() == 13
    assert result["probability_up"].dtype == pl.Float64
    assert result.filter(pl.col("probability_up").is_not_null())["seconds_elapsed"].to_list() == [220]
    assert result.filter(pl.col("admitted"))["seconds_elapsed"].to_list() == [220]
    assert result.filter(pl.col("seconds_elapsed") == 200)["admission_reason"].item() == "outside_native_terminal_210_239"
    assert result.filter(pl.col("seconds_elapsed") == 219)["admission_reason"].item() == "outside_native_five_second_grid"


def test_cutoff_purge_and_missing_features_block_before_native_predict(monkeypatch):
    frame = rows([210, 215, 220, 225])
    control = fixture()
    control["cutoff"] = START + timedelta(seconds=210)
    frame = frame.with_columns(pl.when(pl.col("seconds_elapsed") == 215).then(False)
                               .otherwise(True).alias("evaluation_clock_eligible"))
    features = native(frame).filter(pl.col("point_id") != "m@220")
    calls = []
    original = controls.specialist._predict

    def predict(model, values):
        calls.extend(values["point_id"].to_list())
        assert set(values.columns) == {"point_id", *FEATURES}
        return original(model, values)

    monkeypatch.setattr(controls.specialist, "_predict", predict)
    scored = controls.score_control(controls.TERMINAL, frame, features, control)
    result = controls.admit_control(controls.TERMINAL, scored, control, FROZEN)
    assert calls == ["m@225"]
    assert result["admission_reason"].to_list()[:3] == ["historical_fit_calibration_policy_cutoff",
        "purged_boundary_market", "missing_causal_native_complete_prefix_or_features"]


def test_all_none_capacity_policy_remains_abstaining_with_scored_probabilities():
    result = admitted([60, 180, 240], controls.CAPACITY)
    assert result["probability_up"].to_list() == pytest.approx([0.9] * 3)
    assert result["conservative_probability_up"].to_list() == pytest.approx([0.87] * 3)
    assert result["admitted"].sum() == result["bucket_diagnostic_admitted"].sum() == 0
    assert result["admission_reason"].unique().to_list() == ["native_quantity_policy_abstains"]
    ledger = controls.replay_control(controls.CAPACITY, result, {}, fixture(controls.CAPACITY), FROZEN)
    assert ledger.is_empty()
    assert ledger.schema["net_pnl"] == pl.Float64


def test_native_policy_uses_exact_quantity_book_and_no_interpolation():
    frame = rows([210])
    control = fixture()
    frame = frame.with_columns(pl.lit(0.89).alias("up_vwap_10"))
    scored = controls.score_control(controls.TERMINAL, frame, native(frame), control)
    assert controls.admit_control(controls.TERMINAL, scored, control, FROZEN, 5)["admitted"].sum() == 1
    assert controls.admit_control(controls.TERMINAL, scored, control, FROZEN, 10)["admitted"].sum() == 0
    with pytest.raises(ValueError, match="do not interpolate"):
        controls.admit_control(controls.TERMINAL, scored, control, FROZEN, 15)


def test_bucket_first_attempt_independent_of_sequential_and_labels():
    frame = rows([210, 215, 220, 225])
    control = fixture()
    scored = controls.score_control(controls.TERMINAL, frame, native(frame), control)
    first = controls.admit_control(controls.TERMINAL, scored, control, FROZEN)
    flipped = scored.with_columns(pl.lit(0).alias("label_up"), pl.lit("down").alias("official_outcome"))
    second = controls.admit_control(controls.TERMINAL, flipped, control, FROZEN)
    assert first.select("admitted", "bucket_diagnostic_admitted", "admission_reason").equals(
        second.select("admitted", "bucket_diagnostic_admitted", "admission_reason"))
    assert first.filter(pl.col("admitted"))["seconds_elapsed"].to_list() == [210]
    assert first.filter(pl.col("bucket_diagnostic_admitted"))["seconds_elapsed"].to_list() == [210, 220]


def book(when, side, quantity=100):
    return ObservedBook(when, when, when, when, when, '[["0.3","100"]]',
                        f'[["0.4","{quantity}"]]', side, f"{when}:{side}:{quantity}")


def test_unknown_first_attempt_never_retries_and_fak_fok_independent():
    result = admitted([210, 215, 220])
    # The first attempt has no opposite arrival book. A later healthy opportunity
    # must not replace it in deployable sequential-policy economics.
    when = START + timedelta(seconds=210)
    later = START + timedelta(seconds=220)
    index = {("m", "up"): ([when, later], [book(when, "up"), book(later, "up", 8)]),
             ("m", "down"): ([later], [book(later, "down")])}
    ledger = controls.replay_control(controls.TERMINAL, result, index, fixture(), FROZEN)
    sequential = ledger.filter(pl.col("economic_view") == "sequential_policy")
    assert sequential.height == 2
    assert sequential["decision_at"].unique().to_list() == [when]
    assert sequential["evidence_known"].sum() == 0
    assert sequential["net_pnl"].null_count() == 2
    bucket_later = ledger.filter((pl.col("economic_view") == "bucket_diagnostic")
                                & (pl.col("decision_at") == later))
    assert set(bucket_later["order_type"]) == {"FAK", "FOK"}
    assert bucket_later.filter(pl.col("order_type") == "FAK")["filled_quantity"].item() == 2
    assert bucket_later.filter(pl.col("order_type") == "FOK")["filled_quantity"].item() == 0


def test_later_label_override_delays_cutoff_and_rejects_unproven_override():
    labels = {"seconds_after_close": 1800}
    assert controls.cutoff(controls.TERMINAL, labels) == datetime(2026, 9, 1, 0, 30, tzinfo=UTC)
    labels["verified_later_availability"] = [{"available_at": START.isoformat(), "evidence": "synthetic",
                                              "semantics_verified": True}]
    assert controls.cutoff(controls.TERMINAL, labels) == START
    labels["verified_later_availability"][0]["semantics_verified"] = False
    with pytest.raises(ValueError, match="Unverified"):
        controls.cutoff(controls.TERMINAL, labels)


def test_schedule_retains_frozen39_and_adds_exact_native37():
    frame = rows()
    fold = {"name": "holdout", "evaluation_start": "2026-09-22", "evaluation_end": "2026-09-24"}
    result = controls.schedule(frame, FROZEN, fold)
    current = result.filter(pl.col("decision_grid") == "tournament_grid")
    native_rows = result.filter(pl.col("decision_grid") == "native_grid")
    assert current.height == 39 and current["bucket_start"].n_unique() == 13
    assert native_rows["seconds_elapsed"].sort().to_list() == list(range(60, 241, 5))
    assert native_rows["evaluation_clock_eligible"].all()


def test_causal_books_preserve_missing_and_exact_ladder_quantity():
    frame = rows([210, 215])
    when = START + timedelta(seconds=210)
    index = {("m", "up"): ([when], [book(when, "up", 8)])}
    result = controls.decision_books(frame, index, FROZEN, [5, 10]).sort("point_id")
    assert result["up_vwap_5"][0] == 0.4
    assert result["up_vwap_10"].null_count() == 2
    assert result["down_vwap_5"].null_count() == 2
    assert result["up_vwap_5"][1] is None


def test_changed_provenance_is_rejected_before_deserialization(monkeypatch):
    monkeypatch.setattr(controls, "sha256", lambda _: "changed")

    def forbidden(*args, **kwargs):
        raise AssertionError("Unverified model must not be loaded")

    monkeypatch.setattr(controls.joblib, "load", forbidden)
    with pytest.raises(ValueError, match="conditional evidence changed"):
        controls.load_controls(controls.Path("synthetic-unused"))


def test_complete_native_ohlcv_supports_both_declared_feature_contracts():
    values = []
    for second in range(241):
        opened = START + timedelta(seconds=second - 1)
        values.append({"source": "binance_spot", "symbol": "BTCUSDT",
            "open_timestamp": opened, "close_timestamp": opened + timedelta(milliseconds=999),
            "source_timestamp": opened, "received_at": opened + timedelta(seconds=1),
            "provider_available_at": opened + timedelta(milliseconds=999),
            "available_at": opened + timedelta(seconds=1),
            "open_price": 100.0 + second, "close_price": 101.0 + second,
            "high_price": 102.0 + second, "low_price": 99.0 + second,
            "base_volume": 2.0, "quote_volume": 200.0, "trade_count": 3,
            "taker_buy_base_volume": 1.0, "taker_buy_quote_volume": 100.0})
    candles = pl.DataFrame(values)
    features = list(dict.fromkeys([*PRICE_FEATURES, *CORE_FEATURES]))
    result = controls.reconstruct_native(rows([220]), candles, features)
    assert result.height == 1
    assert result.select(pl.all_horizontal(pl.col(c).is_finite() for c in features)).item()
    late = candles.with_columns(pl.when(pl.col("open_timestamp") == START + timedelta(seconds=20))
        .then(START + timedelta(seconds=221)).otherwise(pl.col("available_at")).alias("available_at"))
    assert controls.reconstruct_native(rows([220]), late, features).is_empty()
