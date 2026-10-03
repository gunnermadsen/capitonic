import copy

import numpy as np
import polars as pl
import pytest
from scipy.special import expit

from btc_directional_model.model_calibration import (
    calibrated_payload,
    corrected,
    fit_correction,
    independent_markets,
    qualify,
)


def test_temperature_improves_overconfident_probabilities_without_direction_change():
    rng = np.random.default_rng(41)
    p = expit(rng.normal(size=4000) * 3)
    y = rng.binomial(1, corrected(p, 0.4, "logit_temperature"))
    slope = fit_correction(p[:2000], y[:2000], "logit_temperature")
    q = corrected(p[2000:], slope, "logit_temperature")
    assert 0.3 < slope < 0.5
    assert np.array_equal(p[2000:] >= 0.5, q >= 0.5)
    result = qualify(p[2000:], y[2000:], q)
    assert result["probability_passed"]
    assert not result["deployment_qualified"]


def test_artifact_correction_preserves_original_and_tree_splits():
    source = {
        "model_key": "old",
        "provenance": {},
        "payoff_model": {
            "definition": {
                "outcome": {
                    "output": "regression",
                    "baseline": 0.6,
                    "trees": [
                        {
                            "nodes": [
                                {"kind": "split", "threshold": 2, "left": 1, "right": 2},
                                {"kind": "leaf", "value": -0.1},
                                {"kind": "leaf", "value": 0.2},
                            ]
                        }
                    ],
                },
                "temporal": [],
                "admission": None,
            }
        },
    }
    original = copy.deepcopy(source)
    candidate = calibrated_payload(source, "new", 2, "probability_sigmoid", {})
    before = source["payoff_model"]["definition"]["outcome"]
    after = candidate["payoff_model"]["definition"]["outcome"]
    assert source == original
    assert before["trees"][0]["nodes"][0] == after["trees"][0]["nodes"][0]
    for index in [1, 2]:
        raw = before["baseline"] + before["trees"][0]["nodes"][index]["value"]
        new = expit(after["baseline"] + after["trees"][0]["nodes"][index]["value"])
        assert new == pytest.approx(expit(2 * (raw - 0.5)))


def test_duplicate_processes_do_not_increase_market_support():
    frame = pl.DataFrame(
        {
            "market_id": ["a", "a"],
            "window_start": ["2026-09-01T00:00:00Z"] * 2,
            "feature_as_of": ["2026-09-01T00:01:00Z"] * 2,
            "probability_up": [0.9, 0.9],
            "outcome_up": [True, True],
        }
    )
    assert independent_markets(frame).height == 1
    with pytest.raises(ValueError, match="Conflicting"):
        independent_markets(frame.with_columns(pl.Series("outcome_up", [True, False])))


def test_existing_temperature_is_composed_and_policy_unchanged():
    source = {
        "model_key": "old",
        "provenance": {},
        "payoff_model": {
            "definition": {
                "calibration": {"slope": 1.4, "intercept": -0.05, "reliability_penalty": 0.02},
                "policy": {"minimum_confidence": 0.88},
            }
        },
    }
    new = calibrated_payload(source, "new", 0.5, "logit_temperature", {})
    definition = new["payoff_model"]["definition"]
    assert definition["calibration"]["slope"] == 0.7
    assert definition["calibration"]["intercept"] == -0.025
    assert definition["calibration"]["reliability_penalty"] == 0.02
    assert definition["policy"] == source["payoff_model"]["definition"]["policy"]


def test_high_confidence_fit_does_not_flatten_optimizer_objective():
    probability = np.full(100, 0.95)
    outcomes = np.r_[np.ones(70), np.zeros(30)]
    slope = fit_correction(probability, outcomes, "logit_temperature")
    assert corrected(np.array([0.95]), slope, "logit_temperature")[0] == pytest.approx(
        0.7, abs=1e-5
    )
