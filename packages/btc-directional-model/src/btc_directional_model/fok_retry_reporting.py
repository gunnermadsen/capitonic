"""Produce the date-matched comparison for an exact FOK retry run."""

from __future__ import annotations

import argparse
import json
from datetime import UTC, datetime
from pathlib import Path

import polars as pl

from .conservative_selective_training import trade_metrics
from .fok_retry_backtest import QUANTITIES, sha256


def run(exact_results: Path, previous_trades: Path, output: Path) -> None:
    exact = json.loads(exact_results.read_text())
    start = datetime.fromisoformat(exact["source"]["start"]).astimezone(UTC)
    end = datetime.fromisoformat(exact["source"]["end"]).astimezone(UTC)
    rows = {}
    for quantity in QUANTITIES:
        prior_path = previous_trades / f"vwap-{quantity}.parquet"
        prior = pl.read_parquet(prior_path).filter(
            (pl.col("window_start") >= start) & (pl.col("window_start") < end)
        )
        prior_metrics = trade_metrics(prior)
        quantity_result = exact["quantities"][str(quantity)]
        one_shot = quantity_result["live_primary:one_shot"]
        continual = quantity_result["live_primary:continual"]
        rows[str(quantity)] = {
            "previous_training_rescoped": prior_metrics,
            "fixed_live_policy_assumed_fill": quantity_result["previous_assumed_fill"],
            "exact_fok_one_shot": one_shot,
            "exact_fok_continual": continual,
            "continual_minus_one_shot": {
                "filled_markets": continual["filled_markets"] - one_shot["filled_markets"],
                "eventual_fill_rate": continual["eventual_fill_rate"] - one_shot["eventual_fill_rate"],
                "stress_pnl": continual["trades"]["stress_pnl"] - one_shot["trades"]["stress_pnl"],
                "precision": (
                    continual["trades"]["precision"] - one_shot["trades"]["precision"]
                    if continual["trades"]["precision"] is not None
                    and one_shot["trades"]["precision"] is not None else None
                ),
            },
        }
    payload = {
        "schema_version": "btc-fok-retry-date-matched-comparison-v1",
        "coverage_start": start,
        "coverage_end_exclusive": end,
        "exact_results": str(exact_results),
        "exact_results_sha256": sha256(exact_results),
        "previous_training_trades": str(previous_trades),
        "comparison_notes": [
            "Previous training is re-scoped to the exact raw-book window.",
            "Previous training used its fold-selected policies; the matched assumed-fill baseline uses the frozen live-pilot policy.",
            "Exact one-shot and continual treatments use identical frozen predictions and live-pilot admission settings.",
            "VWAP quantities above five are execution-capacity counterfactuals and exceed the current live process target size.",
        ],
        "quantities": rows,
    }
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(payload, indent=2, sort_keys=True, default=str) + "\n")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--exact-results", type=Path, required=True)
    parser.add_argument("--previous-trades", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    arguments = parser.parse_args()
    run(arguments.exact_results, arguments.previous_trades, arguments.output)
