import json
from pathlib import Path

import pytest

from btc_directional_model.conservative_selective_paper_export import (
    _validate_unqualified_policy,
)
from btc_directional_model.core_extract import file_sha256

PAPER_KEY = "btc-5m-conservative-selective-vwap-capacity-q5-paper-20260924"
LIVE_KEY = (
    "btc-5m-conservative-selective-vwap-capacity-q5-"
    "development-live-pilot-20260924-v1"
)


def test_capacity_artifact_requires_an_unqualified_quantity_policy() -> None:
    _validate_unqualified_policy({"quantity_policies": {"5": None}}, 5)

    with pytest.raises(ValueError, match="quantity 5"):
        _validate_unqualified_policy(
            {"quantity_policies": {"5": {"minimum_confidence": 0.88}}},
            5,
        )


def test_checked_in_capacity_packages_preserve_behavior_across_scopes() -> None:
    runtime_root = Path(__file__).parent.parent / "runtime-models"
    paper_dir = runtime_root / PAPER_KEY
    live_dir = runtime_root / LIVE_KEY
    paper_model = json.loads((paper_dir / "model.json").read_text())
    live_model = json.loads((live_dir / "model.json").read_text())
    paper_manifest = json.loads((paper_dir / "manifest.json").read_text())
    live_manifest = json.loads((live_dir / "manifest.json").read_text())
    paper_vectors = json.loads((paper_dir / "golden-vectors.json").read_text())
    live_vectors = json.loads((live_dir / "golden-vectors.json").read_text())

    assert paper_manifest["model_sha256"] == file_sha256(paper_dir / "model.json")
    assert live_manifest["model_sha256"] == file_sha256(live_dir / "model.json")
    assert paper_manifest["source_training_model_sha256"] == (
        "65d41630a2211cf2979e5fa952d34fb494bbc85c7027bc100d35b96bdbe86453"
    )
    assert paper_model["payoff_model"]["definition"]["contract"]["inputs"] == [
        {
            "lookback_seconds": 301,
            "maximum_age_ms": 5000,
            "product": "binance_spot_btcusdt_one_second_ohlcv",
            "required": True,
            "semantics": "binance_closed_seconds_prewindow_open_v1",
            "slot": "btc_seconds",
        },
        {
            "lookback_seconds": 2,
            "maximum_age_ms": 2000,
            "product": "polymarket_btc_five_minute_orderbooks",
            "required": True,
            "semantics": "causal_vwap_five_shares_v1",
            "slot": "execution_book",
        },
    ]
    assert paper_model["payoff_model"]["definition"]["contract"][
        "qualified_trade_size"
    ] == 5.0

    assert paper_model.pop("model_key") == PAPER_KEY
    assert live_model.pop("model_key") == LIVE_KEY
    assert paper_model.pop("deployment") == {
        "scope": "paper_only",
        "production_qualified": False,
        "live_capital_allowed": False,
    }
    assert live_model.pop("deployment") == {
        "scope": "development_live_pilot",
        "production_qualified": False,
        "live_capital_allowed": True,
    }
    assert live_model == paper_model
    assert paper_vectors["vectors"] == live_vectors["vectors"]

