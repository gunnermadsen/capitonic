from datetime import UTC, date, datetime

import pytest

from btc_directional_model.entry_disposition_diagnostics import (
    _best_worst,
    _daily,
    _economics,
)


def test_trade_economics_reconcile_positive_and_negative_pnl() -> None:
    rows = [
        {"window_start": datetime(2026, 7, 14, tzinfo=UTC), "market_id": "a",
         "net_pnl": 2.0, "stress_pnl": 1.95, "quantity": 5, "exit_type": "settlement",
         "side": "up", "official_outcome": "up", "share_cost": 0.4},
        {"window_start": datetime(2026, 7, 14, tzinfo=UTC), "market_id": "b",
         "net_pnl": -3.0, "stress_pnl": -3.1, "quantity": 5, "exit_type": "sell",
         "side": "up", "official_outcome": "down", "share_cost": 0.6},
    ]
    result = _economics(rows)
    assert result["net_pnl"] == -1.0
    assert result["stressed_pnl"] == pytest.approx(-1.15)
    assert result["profit_factor"] == pytest.approx(2 / 3)
    assert result["stressed_profit_factor"] == pytest.approx(1.95 / 3.1)
    assert result["wins_to_recover_mean_loss"] == pytest.approx(3.1 / 1.95)
    assert result["reserve_deduction"] == pytest.approx(0.075)
    assert result["completed_sales"] == 1


def test_daily_calendar_keeps_covered_quiet_and_disabled_days_distinct() -> None:
    statuses = {
        date(2026, 7, 13): {"status": "covered", "book_checkpoint_coverage": 0.99},
        date(2026, 7, 14): {"status": "policy_disabled", "book_checkpoint_coverage": 0.99},
    }
    result = _daily([], statuses)
    assert len(result) == 73
    assert result[0]["trades"] == 0 and result[0]["status"] == "covered"
    assert result[1]["trades"] == 0 and result[1]["status"] == "policy_disabled"
    assert result[-1]["day"] == date(2026, 9, 23)


def test_time_bucket_ranking_excludes_low_sample_extreme() -> None:
    slices = [
        {"bucket": "a", "trades": 3, "stressed_pnl": -100},
        {"bucket": "b", "trades": 10, "stressed_pnl": -4},
        {"bucket": "c", "trades": 12, "stressed_pnl": 6},
    ]
    ranked = _best_worst(slices, 10)
    assert ranked["worst"]["bucket"] == "b"
    assert ranked["best"]["bucket"] == "c"
