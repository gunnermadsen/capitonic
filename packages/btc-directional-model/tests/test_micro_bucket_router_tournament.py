from __future__ import annotations

from pathlib import Path

import polars as pl
import pytest

from btc_directional_model import time_bucket_specialist_tournament as base
from btc_directional_model.micro_bucket_router_tournament import (
    _attach_confirmation_execution,
    _blend_predictions,
    _layer_qualified,
    _router_bucket_metrics,
    _select_challenger,
)

CONFIG = (
    Path(__file__).parents[1]
    / "configs"
    / "btc-5m-micro-bucket-router-tournament-20260321-20260901.toml"
)
AMALGAMATED_CONFIG = (
    Path(__file__).parents[1]
    / "configs"
    / "btc-5m-amalgamated-bucket-tournament-20260321-20260901.toml"
)


def test_micro_bucket_hypothesis_and_split_contract_are_frozen() -> None:
    _, raw = base._load_config(CONFIG)
    windows = raw["windows"]
    assert windows["fit_end"] == windows["transition_start"]
    assert windows["transition_end"] == windows["policy_start"]
    assert windows["policy_end"] == windows["sealed_start"]
    assert windows["sealed_end"] == windows["confirmation_start"]
    assert raw["router"]["default_action"] == "no_trade"
    assert {(row["start_second"], row["end_second"]) for row in raw["buckets"]} >= {
        (40, 44),
        (45, 49),
        (60, 64),
        (65, 69),
        (70, 74),
    }
    assert all("prior_candidate" in row for row in raw["candidates"])


def test_prior_weighted_blend_and_agreement_preserve_keys() -> None:
    keys = {
        "market_id": ["a", "b"],
        "window_start": [1, 2],
        "observed_at": [1, 2],
        "seconds_elapsed": [40, 40],
        "label_up": [1, 0],
    }
    predictions = {
        "one": pl.DataFrame(keys).with_columns(pl.Series("probability", [0.8, 0.7])),
        "two": pl.DataFrame(keys).with_columns(pl.Series("probability", [0.6, 0.3])),
    }
    weighted = _blend_predictions(
        ["one", "two"], predictions, {"one": 3.0, "two": 1.0}, "weighted", agreement=False
    )
    assert weighted["probability"].to_list() == pytest.approx([0.75, 0.6])
    agreement = _blend_predictions(
        ["one", "two"], predictions, {"one": 1.0, "two": 1.0}, "agreement", agreement=True
    )
    assert agreement["market_id"].to_list() == ["a"]


def test_confirmation_execution_replaces_only_available_economics() -> None:
    raw = {
        "execution": {"quantities": [5], "capacity_quantities": []},
    }
    frame = pl.DataFrame(
        {
            "market_id": ["a"],
            "window_start": [1],
            "observed_at": [2],
            "seconds_elapsed": [1],
            "fee_rate": [0.0],
            "pm_up_book_age_seconds": [None],
            "pm_down_book_age_seconds": [None],
            "up_ask_vwap_5": [None],
            "down_ask_vwap_5": [None],
        },
        schema_overrides={
            "window_start": pl.Datetime,
            "observed_at": pl.Datetime,
            "pm_up_book_age_seconds": pl.Float64,
            "pm_down_book_age_seconds": pl.Float64,
            "up_ask_vwap_5": pl.Float64,
            "down_ask_vwap_5": pl.Float64,
        },
    )
    execution = pl.DataFrame(
        {
            "market_id": ["a"],
            "window_start": [1],
            "observed_at": [2],
            "seconds_elapsed": [1],
            "fee_rate": [0.01],
            "up_provider_received_at": [1],
            "down_provider_received_at": [1],
            "up_ask_vwap_5": [0.55],
            "down_ask_vwap_5": [0.46],
        },
        schema_overrides={
            "window_start": pl.Datetime,
            "observed_at": pl.Datetime,
            "up_provider_received_at": pl.Datetime,
            "down_provider_received_at": pl.Datetime,
        },
    )
    attached = _attach_confirmation_execution(frame, execution, raw)
    assert attached["up_ask_vwap_5"].item() == 0.55
    assert attached["down_ask_vwap_5"].item() == 0.46
    assert attached["fee_rate"].item() == 0.01


def test_layer_qualification_requires_both_development_and_design() -> None:
    raw = {
        "router": {
            "minimum_development_trades": 20,
            "minimum_design_trades": 20,
            "minimum_profit_factor": 1.0,
            "require_positive_development_stress": True,
            "require_positive_design_stress": True,
        }
    }
    layer = {
        "development": {"trades": 25, "profit_factor": 1.2, "stress_net_pnl": 10.0},
        "design": {"trades": 25, "profit_factor": 1.1, "stress_net_pnl": 5.0},
    }
    assert _layer_qualified(layer, raw)
    layer["design"]["stress_net_pnl"] = -1.0
    assert not _layer_qualified(layer, raw)


def test_challenger_selection_excludes_historical_benchmarks() -> None:
    routers = {
        "champion_replay": {"confirmation": {"stress_net_pnl": 20.0, "net_pnl": 30.0}},
        "prior_composed_replay": {"confirmation": {"stress_net_pnl": 100.0, "net_pnl": 120.0}},
        "new_router": {"confirmation": {"stress_net_pnl": 40.0, "net_pnl": 50.0}},
    }
    assert _select_challenger(routers, ("new_router",)) == "new_router"


def test_amalgamated_contract_preserves_august_and_qualifies_at_q5() -> None:
    _, raw = base._load_config(AMALGAMATED_CONFIG)
    assert raw["windows"] == {
        "source_start": "2026-03-21T00:00:00Z",
        "fit_end": "2026-08-14T00:00:00Z",
        "transition_start": "2026-08-14T00:00:00Z",
        "transition_end": "2026-08-15T00:00:00Z",
        "policy_start": "2026-08-15T00:00:00Z",
        "policy_end": "2026-08-20T00:00:00Z",
        "sealed_start": "2026-08-20T00:00:00Z",
        "sealed_end": "2026-08-26T00:00:00Z",
        "confirmation_start": "2026-08-26T00:00:00Z",
        "confirmation_end": "2026-09-01T00:00:00Z",
    }
    assert raw["execution"]["quantities"] == [5]
    assert 50 in raw["execution"]["capacity_quantities"]
    assert raw["router"]["selection_period"] == "design"
    assert not raw["router"]["require_qualified_layers_for_evaluation"]
    assert raw["router"]["default_action"] == "no_trade"
    assert any(row["form"] == "prior_weighted" for row in raw["router_definitions"])
    assert any("reservation_before_second" in row for row in raw["router_definitions"])


def test_layer_qualification_applies_loss_and_recovery_limits() -> None:
    raw = {
        "router": {
            "minimum_development_trades": 20,
            "minimum_design_trades": 20,
            "minimum_profit_factor": 1.15,
            "maximum_loss_rate": 0.40,
            "maximum_recovery_wins_per_loss": 2.5,
            "require_positive_development_stress": True,
            "require_positive_design_stress": True,
        }
    }
    layer = {
        "development": {
            "trades": 30,
            "profit_factor": 1.2,
            "stress_net_pnl": 10.0,
            "win_rate": 0.65,
            "recovery_wins_per_loss": 2.0,
        },
        "design": {
            "trades": 30,
            "profit_factor": 1.2,
            "stress_net_pnl": 10.0,
            "win_rate": 0.65,
            "recovery_wins_per_loss": 2.0,
        },
    }
    assert _layer_qualified(layer, raw)
    layer["design"]["win_rate"] = 0.55
    assert not _layer_qualified(layer, raw)


def test_router_bucket_metrics_include_zero_trade_buckets() -> None:
    trades = pl.DataFrame(
        {
            "seconds_elapsed": [62],
            "market_id": ["m"],
            "side": ["up"],
            "won": [True],
            "net_pnl": [1.0],
            "stress_net_pnl": [0.9],
            "share_cost": [0.5],
            "selected_probability": [0.7],
            "expected_edge": [0.1],
        }
    )
    buckets = {
        "early": {"start_second": 60, "end_second": 74},
        "late": {"start_second": 185, "end_second": 209},
    }
    result = _router_bucket_metrics(trades, buckets, 10)
    assert result["early"]["net_pnl"] == 1.0
    assert result["late"]["trades"] == 0


def test_challenger_selection_uses_design_when_requested() -> None:
    routers = {
        "stable": {
            "design": {"stress_net_pnl": 10.0, "net_pnl": 12.0},
            "confirmation": {"stress_net_pnl": 1.0, "net_pnl": 2.0},
        },
        "lucky": {
            "design": {"stress_net_pnl": 2.0, "net_pnl": 3.0},
            "confirmation": {"stress_net_pnl": 100.0, "net_pnl": 110.0},
        },
    }
    assert _select_challenger(routers, ("stable", "lucky"), "design") == "stable"
