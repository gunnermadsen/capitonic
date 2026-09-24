"""Exact frozen-decision quantity and latency sensitivity for matched buy policies."""

from __future__ import annotations

import argparse
import json
from collections import defaultdict
from datetime import date, timedelta
from pathlib import Path

import joblib
import polars as pl

from .conservative_selective_training import trade_metrics
from .entry_disposition_data import _books
from .entry_disposition_replay import _book_index, _buy, _dispose, _hold
from .fok_retry_backtest import QUANTITIES

SCENARIOS = (("primary", 150, 0.80), ("adverse", 300, 0.65))


def run(root: Path, canonical: Path, verified: Path) -> None:
    entries = pl.read_parquet(root / "backtests" / "buy_hold.parquet")
    folds = {row["fold"]: row for row in json.loads((root / "metrics" / "training-folds.json").read_text())}
    groups: dict[date, list[dict]] = defaultdict(list)
    for row in entries.to_dicts():
        groups[row["window_start"].date()].append(row)
    ledgers: dict[tuple[str, int, str], list[dict]] = defaultdict(list)
    counts: dict[tuple[str, int], dict] = defaultdict(lambda: {"decisions": 0, "eligible": 0, "filled": 0})
    parity_misses = []
    for day, trades in sorted(groups.items()):
        causal = pl.read_parquet(root / "datasets" / "causal-book-panel" / f"date={day.isoformat()}.parquet")
        by_market = {str(key[0] if isinstance(key, tuple) else key): group.sort("seconds_elapsed")
                     for key, group in causal.partition_by("market_id", as_dict=True).items()}
        raw, _ = _books(day, canonical, verified)
        books = _book_index(raw)
        for entry in trades:
            market = str(entry["market_id"])
            rows = by_market[market].to_dicts()
            decision = next(row for row in rows if row["seconds_elapsed"] == entry["seconds_elapsed"])
            fold = folds[int(entry["fold"])]
            policy = fold["policy"]
            if policy is None:
                raise RuntimeError("completed buy on policy-disabled fold")
            sell_path = root / "models" / f"sell-{fold['test_start'][:10]}.joblib"
            sell_model = joblib.load(sell_path) if sell_path.exists() else None
            at = entry["window_start"] + timedelta(seconds=int(entry["seconds_elapsed"]))
            for scenario, latency, haircut in SCENARIOS:
                for quantity in QUANTITIES:
                    key = (scenario, quantity)
                    counts[key]["decisions"] += 1
                    vwap = decision[f"{entry['side']}_ask_vwap_{quantity}"]
                    if vwap is None or vwap > policy["maximum_share_cost"] or (
                        entry["confidence"] - vwap - 0.005 - 0.010 < policy["minimum_stressed_edge"]
                    ):
                        continue
                    counts[key]["eligible"] += 1
                    result = _buy(books, market, entry["side"], at, policy["maximum_share_cost"],
                                  latency_ms=latency, haircut=haircut, quantity=quantity)
                    if not result["filled"]:
                        continue
                    counts[key]["filled"] += 1
                    current = {**entry, "quantity": quantity, "share_cost": result["share_cost"]}
                    held = _hold(current)
                    disposed, _ = _dispose(current, rows, books, sell_model, "model", quantity=quantity,
                                           latency_ms=latency, haircut=haircut)
                    ledgers[(scenario, quantity, "buy_hold")].append(held)
                    ledgers[(scenario, quantity, "buy_sell")].append(disposed)
                    if scenario == "primary" and quantity == 5 and abs(entry["share_cost"] - result["share_cost"]) > 1e-9:
                        parity_misses.append(market)
        print(json.dumps({"day": day.isoformat(), "frozen_entries": len(trades)}), flush=True)
    if parity_misses:
        raise RuntimeError(f"Q5 replay changed {len(parity_misses)} filled prices")
    summary = {}
    for scenario, latency, haircut in SCENARIOS:
        summary[scenario] = {"latency_ms": latency, "depth_haircut": haircut, "quantities": {}}
        for quantity in QUANTITIES:
            by_name = {}
            for name in ("buy_hold", "buy_sell"):
                rows = ledgers[(scenario, quantity, name)]
                frame = pl.DataFrame(rows).sort("window_start") if rows else pl.DataFrame()
                by_name[name] = trade_metrics(frame)
                if rows:
                    frame.write_parquet(root / "backtests" / f"capacity-{scenario}-{name}-q{quantity}.parquet")
            summary[scenario]["quantities"][str(quantity)] = {
                **counts[(scenario, quantity)], "buy_hold": by_name["buy_hold"],
                "buy_sell": by_name["buy_sell"],
                "sell_stress_delta": by_name["buy_sell"]["stress_pnl"] - by_name["buy_hold"]["stress_pnl"],
            }
    summary["scope"] = "frozen Q5 entry decision time and side; quantity-specific economics and full-order FOK fills; no quantity-specific retry or policy reselection"
    summary["q5_primary_parity"] = len(ledgers[("primary", 5, "buy_hold")]) == entries.height and not parity_misses
    (root / "metrics" / "capacity.json").write_text(json.dumps(summary, indent=2, default=str) + "\n")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--run-dir", type=Path, required=True)
    parser.add_argument("--canonical-books", type=Path, required=True)
    parser.add_argument("--verified-books", type=Path, required=True)
    args = parser.parse_args()
    run(args.run_dir, args.canonical_books, args.verified_books)
