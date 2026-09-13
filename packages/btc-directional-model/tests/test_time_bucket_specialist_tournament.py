from __future__ import annotations

from pathlib import Path

import polars as pl

from btc_directional_model.time_bucket_specialist_tournament import (
    Policy,
    _economic_metrics,
    _feature_contracts,
    _load_config,
    _select_trades,
)

CONFIG = (
    Path(__file__).parents[1]
    / "configs"
    / "btc-5m-time-bucket-specialist-tournament-20260321-20260901.toml"
)


def test_hypothesis_config_is_training_only_and_bucket_disjoint() -> None:
    _, raw = _load_config(CONFIG)
    assert raw["training"]["training_only"] is True
    assert raw["training"]["paper_only"] is True
    assert raw["training"]["live_capital_allowed"] is False
    assert raw["windows"]["fit_end"] == raw["windows"]["policy_start"]
    assert raw["windows"]["policy_end"] == raw["windows"]["sealed_start"]
    assert raw["windows"]["sealed_end"] == raw["windows"]["confirmation_start"]
    assert {row["historical_model"] for row in raw["candidates"]}
    assert {row["bucket"] for row in raw["candidates"]} <= {row["name"] for row in raw["buckets"]}


def test_sequential_gate_uses_first_eligible_entry_once_per_market() -> None:
    frame = pl.DataFrame(
        {
            "market_id": ["a", "a", "b"],
            "window_start": [1, 1, 2],
            "seconds_elapsed": [40, 45, 40],
            "side": ["up", "up", "down"],
            "label_up": [1, 1, 1],
            "share_cost": [0.60, 0.55, 0.40],
            "selected_probability": [0.80, 0.90, 0.80],
            "expected_edge": [0.19, 0.34, 0.39],
            "fee_per_share": [0.0, 0.0, 0.0],
        }
    )
    policy = Policy("both", 5, 0.0, 0.5, 0.95)
    trades = _select_trades(
        frame,
        policy,
        {"execution": {"execution_reserve_per_share": 0.005, "stress_slippage_per_share": 0.01}},
    )
    assert trades.height == 2
    assert trades.filter(pl.col("market_id") == "a")["seconds_elapsed"].item() == 40
    metrics = _economic_metrics(trades, 2)
    assert metrics["trades"] == 2
    assert metrics["wins"] == 1
    assert metrics["losses"] == 1


def test_candidate_roster_contains_rtds_and_rtds_free_training_pairs() -> None:
    _, raw = _load_config(CONFIG)
    assert any("candles" in row["feature_groups"] for row in raw["candidates"])
    requested = {
        group
        for row in raw["candidates"]
        for group in row["feature_groups"]
        if not group.startswith("latent_")
    }
    source = {"feature_groups": {group: [f"{group}_feature"] for group in requested}}
    contracts = _feature_contracts(raw, source)
    for candidate in raw["candidates"]:
        pair = [row for row in contracts if row["candidate"] == candidate["name"]]
        assert {row["rtds_mode"] for row in pair} == {
            "with_rtds_candles",
            "without_rtds_candles",
        }
    assert raw["execution"]["quantities"] == [5, 10, 20, 30, 50]
