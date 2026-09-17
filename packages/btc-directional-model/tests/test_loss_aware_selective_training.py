import polars as pl

from btc_directional_model.conservative_selective_training import Policy
from btc_directional_model.loss_aware_selective_training import (
    CombinedPolicy,
    _rejection_economics,
)


def test_combined_policy_keeps_directional_and_risk_thresholds() -> None:
    policy = CombinedPolicy(Policy(0.8, 0.05, 0.65), 0.25)
    assert policy.directional.maximum_share_cost == 0.65
    assert policy.maximum_loss_probability == 0.25


def test_rejection_economics_compares_avoided_losses_with_rejected_wins() -> None:
    baseline = pl.DataFrame(
        {"market_id": ["kept", "avoided", "forfeited"], "stress_pnl": [2.0, -4.0, 1.5]}
    )
    new = pl.DataFrame({"market_id": ["kept"], "stress_pnl": [2.0]})

    result = _rejection_economics(new, baseline)

    assert result == {
        "baseline_trades_rejected": 2,
        "loss_dollars_avoided": 4.0,
        "win_dollars_rejected": 1.5,
        "net_rejection_value": 2.5,
    }
