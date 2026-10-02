from datetime import UTC, datetime, timedelta

import polars as pl

from btc_directional_model.time_bucket_policy import (
    cached_q5,
    economic_metrics,
    intentions,
    selected_attempts,
)


def fixture():
    start = datetime(2026, 8, 1, tzinfo=UTC)
    frame = pl.DataFrame({
        "market_id": ["m", "m"], "decision_at": [start, start + timedelta(seconds=20)],
        "bucket_start": [0, 20], "probability_up": [.9, .9], "conservative_probability_up": [.88, .88],
        "up_decision_partial_vwap_5": [.5, .5], "down_decision_partial_vwap_5": [.5, .5],
        "up_limit_5": [.5, .5], "down_limit_5": [.5, .5],
        "up_fak_evidence_known": [False, True], "down_fak_evidence_known": [False, True],
        "up_fak_quantity_5": [None, 5.], "down_fak_quantity_5": [None, 5.],
        "up_fak_price_5": [None, .5], "down_fak_price_5": [None, .5],
        "up_fak_net_5": [None, 2.], "down_fak_net_5": [None, -3.],
        "up_fak_stress_5": [None, 1.9], "down_fak_stress_5": [None, -3.1],
    })
    frozen = {"fees": {"rate_assumption": .07}, "book": {"reserve_per_share_per_leg": .005,
                "stress_extra_per_share_per_leg": .01}}
    policy = {"confidence": .7, "minimum_stressed_edge": .01,
              "maximum_share_cost": .7, "bucket_mask": [0, 20]}
    return frame, frozen, policy


def test_unknown_first_attempt_does_not_become_later_profitable_fill():
    frame, frozen, policy = fixture()
    attempt = cached_q5(selected_attempts(frame, policy, frozen))
    assert attempt.height == 1
    assert not attempt["evidence_known"][0]
    assert attempt["net_pnl"][0] is None
    assert selected_attempts(frame, policy, frozen, diagnostic=True).height == 2


def test_admission_cannot_use_future_execution_or_outcome():
    frame, frozen, policy = fixture()
    expected = intentions(frame, policy, frozen)["admitted"].to_list()
    changed = frame.with_columns(pl.lit(False).alias("up_fak_evidence_known"),
                                 pl.lit(-100.).alias("up_fak_stress_5"))
    assert intentions(changed, policy, frozen)["admitted"].to_list() == expected
    capped = frame.with_columns(pl.lit(.71).alias("up_limit_5"))
    assert not any(intentions(capped, policy, frozen)["admitted"])


def test_unknown_exit_preserves_entry_fill_without_inventing_economics():
    frame = pl.DataFrame({"market_id": ["m"], "evidence_known": [True], "exit_evidence_known": [False],
                          "filled_quantity": [5.], "requested_quantity": [5.],
                          "net_pnl": pl.Series([None], dtype=pl.Float64),
                          "stress_pnl": pl.Series([None], dtype=pl.Float64)})
    result = economic_metrics(frame)
    assert result["unknown_attempts"] == 1
    assert result["observed_entry_fills"] == 1
    assert result["fills"] == 0
