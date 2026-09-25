"""Qualification and chronological activity report for the entry/disposition tournament."""

from __future__ import annotations

import argparse
import hashlib
import json
import math
from datetime import date, timedelta
from pathlib import Path

import plotly.graph_objects as go
import polars as pl
from plotly.subplots import make_subplots

from .conservative_selective_training import expected_calibration_error, trade_metrics

CANDIDATES = ("buy_hold", "buy_sell", "champion_specialist")
START = date(2026, 7, 13)
END = date(2026, 9, 24)


def _sha(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _ledger(root: Path, name: str) -> pl.DataFrame:
    path = root / "backtests" / f"{name}.parquet"
    return pl.read_parquet(path).sort("window_start") if path.exists() else pl.DataFrame()


def _ece(root: Path) -> dict:
    paths = sorted((root / "predictions").glob("buy-fold-*.parquet"))
    if not paths:
        raise RuntimeError("no genuine out-of-fold buy predictions")
    frame = pl.scan_parquet(paths).select("market_id", "official_outcome", "probability_up",
                                          "admission_probability", "won").collect()
    direction = (frame["official_outcome"] == "up").cast(pl.Int8).to_numpy()
    return {
        "direction_ece": expected_calibration_error(direction, frame["probability_up"].to_numpy(), 10),
        "admission_ece": expected_calibration_error(frame["won"].to_numpy(), frame["admission_probability"].to_numpy(), 10),
        "rows": frame.height, "markets": frame["market_id"].n_unique(),
    }


def _side_metrics(frame: pl.DataFrame) -> dict:
    if frame.is_empty():
        return {"up": trade_metrics(pl.DataFrame()), "down": trade_metrics(pl.DataFrame())}
    return {side: trade_metrics(frame.filter(pl.col("side") == side)) for side in ("up", "down")}


def _activity(frame: pl.DataFrame, folds: list[dict], name: str, coverage: dict) -> list[dict]:
    by_day = {}
    if not frame.is_empty():
        daily = frame.with_columns(pl.col("window_start").dt.date().alias("day")).group_by("day").agg(
            pl.len().alias("count"), pl.col("net_pnl").sum().alias("net_pnl"),
            pl.col("stress_pnl").sum().alias("stress_pnl"))
        by_day = {row["day"]: row for row in daily.to_dicts()}
    result = []
    day = START
    while day < END:
        fold = next((row for row in folds if row["test_start"][:10] <= day.isoformat() < row["test_end"][:10]), None)
        if fold is None:
            status = "untested"
        elif name in ("buy_hold", "buy_sell") and fold["policy"] is None:
            status = "policy_disabled"
        elif day not in coverage:
            status = "missing_source_coverage"
        elif coverage[day]["markets"] < 280 or coverage[day]["both_book_rows"] / coverage[day]["rows"] < 0.95:
            status = "partial_source_coverage"
        else:
            status = "covered"
        row = by_day.get(day, {})
        result.append({"candidate": name, "day": day, "status": status,
                       "book_checkpoint_coverage": (coverage[day]["both_book_rows"] / coverage[day]["rows"]) if day in coverage else None,
                       "trade_count": row.get("count", 0), "net_pnl": row.get("net_pnl", 0.0),
                       "stress_pnl": row.get("stress_pnl", 0.0)})
        day += timedelta(days=1)
    return result


def _bursts(frame: pl.DataFrame, daily: list[dict]) -> list[dict]:
    if frame.is_empty():
        return []
    entries = frame.sort("window_start").to_dicts()
    chunks = []
    current = []
    prior_at = None
    for row in entries:
        at = row["window_start"] + timedelta(seconds=int(row["seconds_elapsed"]))
        if prior_at is not None and at - prior_at > timedelta(minutes=30):
            chunks.append(current)
            current = []
        current.append((at, row))
        prior_at = at
    if current:
        chunks.append(current)
    statuses = {row["day"]: row["status"] for row in daily}
    result = []
    previous_end = None
    for group in chunks:
        beginning, ending = group[0][0], group[-1][0]
        quiet = None
        if previous_end is not None:
            cursor = previous_end.date()
            verified = True
            while cursor <= beginning.date():
                if statuses.get(cursor) != "covered":
                    verified = False
                    break
                cursor += timedelta(days=1)
            if verified:
                quiet = (beginning - previous_end).total_seconds() / 60.0
        stress = sum(row["stress_pnl"] for _, row in group)
        result.append({"start": beginning.isoformat(), "end": ending.isoformat(),
                       "trades": len(group), "preceding_verified_quiet_minutes": quiet,
                       "net_pnl": sum(row["net_pnl"] for _, row in group),
                       "stress_pnl": stress,
                       "material": stress >= 10.0 or stress <= -10.0})
        previous_end = ending
    return result


def _chart(path: Path, name: str, daily: list[dict], bursts: list[dict], folds: list[dict]) -> None:
    dates = [row["day"] for row in daily]
    figure = make_subplots(rows=2, cols=1, shared_xaxes=True, vertical_spacing=0.09,
                           subplot_titles=("Completed buys per UTC day", "Daily PnL ($, five shares)"))
    figure.add_trace(go.Bar(x=dates, y=[row["trade_count"] for row in daily], name="Trades"), row=1, col=1)
    figure.add_trace(go.Scatter(x=dates, y=[row["net_pnl"] for row in daily], name="Net PnL", mode="lines+markers"), row=2, col=1)
    figure.add_trace(go.Scatter(x=dates, y=[row["stress_pnl"] for row in daily], name="Stressed PnL", mode="lines+markers"), row=2, col=1)
    for row in daily:
        if row["status"] != "covered":
            color = "rgba(255,165,0,0.20)" if row["status"] == "policy_disabled" else "rgba(200,40,40,0.20)"
            figure.add_vrect(x0=row["day"], x1=row["day"] + timedelta(days=1), fillcolor=color,
                             line_width=0, row="all", col=1)
    for fold in folds:
        figure.add_vline(x=fold["test_start"][:10], line_dash="dot", line_color="gray", row="all", col=1)
    for burst in bursts:
        if burst["material"]:
            figure.add_annotation(x=burst["start"][:10], y=burst["stress_pnl"],
                                  text=f"{burst['trades']} trades / ${burst['stress_pnl']:.0f}",
                                  showarrow=True, row=2, col=1)
    figure.update_layout(title=f"{name}: July 13–September 23, 2026", height=800,
                         barmode="overlay", legend={"orientation": "h"})
    figure.update_xaxes(title_text="UTC date", row=2, col=1)
    rows = ["<h2>Contiguous activity bursts (at most 30 minutes between entries)</h2>",
            "<table border='1'><tr><th>Start UTC</th><th>End UTC</th><th>Trades</th><th>Preceding verified quiet (min)</th><th>Net PnL</th><th>Stressed PnL</th></tr>"]
    for burst in bursts:
        quiet = "unknown" if burst["preceding_verified_quiet_minutes"] is None else f"{burst['preceding_verified_quiet_minutes']:.0f}"
        rows.append(f"<tr><td>{burst['start']}</td><td>{burst['end']}</td><td>{burst['trades']}</td><td>{quiet}</td><td>{burst['net_pnl']:.2f}</td><td>{burst['stress_pnl']:.2f}</td></tr>")
    rows.append("</table><p>Orange: policy disabled. Red: untested, missing, or below 95% causal book checkpoint coverage (or fewer than 280 markets). Zero bars on covered days mean no admitted trades. Fold boundaries are dotted.</p>")
    path.write_text("<html><body>" + figure.to_html(full_html=False, include_plotlyjs="cdn") + "\n".join(rows) + "</body></html>")


def _checks(metrics: dict, ece: float, side: dict) -> dict:
    sides_pass = all(side[s]["trades"] == 0 or side[s]["stress_pnl"] > 0 for s in ("up", "down"))
    return {
        "minimum_60_trades": metrics["trades"] >= 60,
        "minimum_8_represented_weeks": metrics["weeks"] >= 8,
        "precision_above_0_78": metrics["precision"] is not None and metrics["precision"] > 0.78,
        "wilson_above_0_68": metrics["wilson_lower"] > 0.68,
        "stressed_profit_factor_above_1_5": metrics["stressed_profit_factor"] is not None and metrics["stressed_profit_factor"] > 1.5,
        "loss_recovery_at_most_2_wins": metrics["compensation_ratio"] is not None and metrics["compensation_ratio"] <= 2,
        "losing_streak_below_3": metrics["longest_losing_streak"] < 3,
        "positive_stressed_pnl": metrics["stress_pnl"] > 0,
        "positive_month_fraction_at_least_0_70": metrics["positive_month_fraction"] is not None and metrics["positive_month_fraction"] >= 0.70,
        "best_day_below_0_25_positive_pnl": metrics["best_day_positive_pnl_share"] is not None and metrics["best_day_positive_pnl_share"] < 0.25,
        "ece_below_0_07": ece < 0.07,
        "no_negative_side": sides_pass,
    }


def run(root: Path) -> None:
    replay = json.loads((root / "metrics" / "replay.json").read_text())
    folds = json.loads((root / "metrics" / "training-folds.json").read_text())
    coverage = {date.fromisoformat(row["day"]): row for row in json.loads(
        (root / "datasets" / "causal-book-panel-coverage.json").read_text()) if row["status"] == "written"}
    predictions = _ece(root)
    ledgers = {name: _ledger(root, name) for name in ("champion", *CANDIDATES, "fixed_exit")}
    buy, sell = ledgers["buy_hold"], ledgers["buy_sell"]
    paired_columns = ("market_id", "window_start", "side", "seconds_elapsed", "share_cost", "quantity")
    if buy.height != sell.height or not buy.select(paired_columns).equals(sell.select(paired_columns)):
        raise RuntimeError("buy/sell candidate changed the buy ledger")
    activities, bursts, results = [], {}, {}
    for name in ("champion", *CANDIDATES):
        frame = ledgers[name]
        metrics = trade_metrics(frame)
        side = _side_metrics(frame)
        daily = _activity(frame, folds, name, coverage)
        burst = _bursts(frame, daily)
        activities.extend(daily)
        bursts[name] = burst
        _chart(root / "diagnostics" / f"trade-activity-{name}.html", name, daily, burst, folds)
        checks = _checks(metrics, predictions["admission_ece"] if name != "champion" else 0.0, side)
        results[name] = {
            "metrics": metrics, "side_metrics": side, "checks": checks,
            "hard_gates_passed": all(checks.values()) if name != "champion" else None,
            "average_buys_per_calendar_day": metrics["trades"] / (END - START).days,
            "full_coverage_days": sum(row["status"] == "covered" for row in daily),
            "average_buys_per_full_coverage_day": sum(row["trade_count"] for row in daily if row["status"] == "covered") / max(sum(row["status"] == "covered" for row in daily), 1),
            "partial_coverage_days": sum(row["status"] == "partial_source_coverage" for row in daily),
            "days_with_buys": sum(row["trade_count"] > 0 for row in daily),
            "chart": f"diagnostics/trade-activity-{name}.html",
        }
    pl.DataFrame(activities).write_parquet(root / "metrics" / "daily-activity.parquet")
    (root / "metrics" / "bursts.json").write_text(json.dumps(bursts, indent=2) + "\n")
    sell_rows = sell.filter(pl.col("exit_type") == "sell") if not sell.is_empty() else pl.DataFrame()
    sell_comparison = {
        "matched_buys": buy.height == sell.height,
        "sold_positions": sell_rows.height,
        "stressed_pnl_delta": results["buy_sell"]["metrics"]["stress_pnl"] - results["buy_hold"]["metrics"]["stress_pnl"],
        "profit_factor_no_worse": (results["buy_sell"]["metrics"]["stressed_profit_factor"] or 0) >= (results["buy_hold"]["metrics"]["stressed_profit_factor"] or 0),
        "drawdown_at_most_10_pct_worse": results["buy_sell"]["metrics"]["maximum_drawdown"] <= 1.1 * results["buy_hold"]["metrics"]["maximum_drawdown"],
        "losing_streak_not_amplified": results["buy_sell"]["metrics"]["longest_losing_streak"] <= results["buy_hold"]["metrics"]["longest_losing_streak"],
        "unfilled_sell_intent_fraction": replay["sell_intents_unfilled"] / replay["sell_intents"] if replay["sell_intents"] else 0,
        "worse_exit_slippage_delta": (results["buy_sell"]["metrics"]["stress_pnl"] - 0.02 * 5 * sell_rows.height) - results["buy_hold"]["metrics"]["stress_pnl"],
        "fixed_exit_stressed_pnl": trade_metrics(ledgers["fixed_exit"])["stress_pnl"],
    }
    sell_comparison["incremental_gates_passed"] = (
        sell_comparison["stressed_pnl_delta"] > 0 and sell_comparison["profit_factor_no_worse"]
        and sell_comparison["drawdown_at_most_10_pct_worse"]
        and sell_comparison["losing_streak_not_amplified"]
        and sell_comparison["unfilled_sell_intent_fraction"] < 0.05
        and sell_comparison["worse_exit_slippage_delta"] > 0
    )
    results["buy_sell"]["hard_gates_passed"] &= sell_comparison["incremental_gates_passed"]
    champion_losses = set(ledgers["champion"].filter(pl.col("net_pnl") <= 0)["market_id"].to_list()) if not ledgers["champion"].is_empty() else set()
    buy_losses = set(buy.filter(pl.col("net_pnl") <= 0)["market_id"].to_list()) if not buy.is_empty() else set()
    behavior = {"champion_losses_rejected_by_buy": len(champion_losses - buy_losses),
                "new_buy_losses_not_in_champion": len(buy_losses - champion_losses)}
    capacity_path = root / "metrics" / "capacity.json"
    capacity = json.loads(capacity_path.read_text()) if capacity_path.exists() else None
    if capacity is not None:
        for name in ("buy_hold", "buy_sell"):
            adverse = capacity["adverse"]["quantities"]["5"][name]
            results[name]["checks"]["adverse_q5_positive_stressed_pnl"] = adverse["stress_pnl"] > 0
            results[name]["hard_gates_passed"] &= adverse["stress_pnl"] > 0
    ranking = sorted(CANDIDATES, key=lambda name: (
        results[name]["hard_gates_passed"], results[name]["metrics"]["wilson_lower"],
        -(results[name]["metrics"]["compensation_ratio"] or math.inf),
        -results[name]["metrics"]["maximum_drawdown"],
        -results[name]["metrics"]["longest_losing_streak"],
        results[name]["metrics"]["stressed_profit_factor"] or 0,
        results[name]["metrics"]["stress_pnl"] / max(results[name]["metrics"]["trades"], 1),
    ), reverse=True)
    winner = ranking[0] if results[ranking[0]]["hard_gates_passed"] else None
    report = {"schema_version": "entry-disposition-tournament-v1", "date_range": [START.isoformat(), END.isoformat()],
              "prediction_calibration": predictions, "entries": results, "sell_comparison": sell_comparison,
              "buy_vs_champion_loss_behavior": behavior,
              "displaced_champion_trades": replay["displaced_champion_trades"],
              "displacement_stress_pnl_delta": replay["displacement_stress_pnl_delta"],
              "ranking": ranking, "charted_candidates": list(CANDIDATES), "qualified_winner": winner,
              "quantity_sensitivity": ({scenario: {quantity: {
                  "eligible": row["eligible"], "filled": row["filled"],
                  "buy_hold_stress_pnl": row["buy_hold"]["stress_pnl"],
                  "buy_sell_stress_pnl": row["buy_sell"]["stress_pnl"],
                  "sell_stress_delta": row["sell_stress_delta"]}
                  for quantity, row in capacity[scenario]["quantities"].items()}
                  for scenario in ("primary", "adverse")} if capacity else None),
              "limitations": ["Frozen champion artifact was trained on overlapping historical dates; its backtest is descriptive and requires prospective paper comparison.",
                              "Archived snapshots cannot observe every intra-sample book change; FOK replay uses causal as-of books, 150 ms latency and 80% displayed-depth haircut. Only days with at least 280 markets and 95% paired book checkpoints count as full coverage.",
                              "Q5 is the common primary size; the fourteen-quantity sensitivity freezes the Q5 decision time and side, without quantity-specific policy selection or retries."]}
    (root / "metrics" / "qualification.json").write_text(json.dumps(report, indent=2, default=str) + "\n")
    status = "completed"
    (root / "manifests" / "run-status.md").write_text(
        f"# Entry disposition tournament\n\n- Status: {status}\n- Training: completed\n- Evaluation: completed at Q5 through 2026-09-23\n"
        f"- Qualification: {'passed historical gates for ' + winner if winner else 'failed; no winner'}\n"
        "- Deployment authorization: none\n- Prospective paper comparison: required\n"
        f"- Quantity sensitivity: {'14 sizes under primary and adverse FOK assumptions' if capacity else 'pending'}\n"
        "- Source database mutations: none; orderbook drain was copy-only\n"
        "- See `metrics/qualification.json` and `diagnostics/trade-activity-*.html` for evidence.\n")
    manifest = {"run_id": root.name, "status": status, "branch": "training/entry-disposition-tournament",
                "starting_commit": "fda43e794492381f5f28dd17be8d456a10260404",
                "source_panel": "/Volumes/docker-data/capitonic-btc-directional-model/conservative-selective-vwap-capacity-20260923T011753Z/datasets/vwap-admission-panel-20260321-20260921.parquet",
                "source_panel_sha256": "33990603cc3621cf3b3e6d17fa15ffc611d619cf4e6e761a153b78d36ddecd31",
                "extended_panel_sha256": _sha(root / "datasets" / "directional-panel-20260321-20260923.parquet"),
                "champion_artifact_sha256": replay["champion_source_sha256"],
                "book_drain_job": "894dd296-0479-4225-af14-2e08141ee60d",
                "book_panel_coverage_sha256": _sha(root / "datasets" / "causal-book-panel-coverage.json"),
                "capacity_sha256": _sha(capacity_path) if capacity is not None else None,
                "training_command": "python -m btc_directional_model.entry_disposition_training",
                "evaluation_command": "python -m btc_directional_model.entry_disposition_replay",
                "evaluation_interval": [START.isoformat(), END.isoformat()],
                "folds": [{"test_start": row["test_start"], "test_end": row["test_end"],
                           "fit_end": row["direction_fit_end"], "policy": row.get("policy")}
                          for row in folds],
                "qualification": "passed" if winner else "failed", "winner": winner,
                "deployment_authorization": False,
                "ranking": ranking, "charts": {name: results[name]["chart"] for name in CANDIDATES},
                "known_limitations": report["limitations"],
                "outputs": {str(path.relative_to(root)): _sha(path) for path in sorted((root / "metrics").glob("*")) if path.is_file()}}
    (root / "manifests" / "run.json").write_text(json.dumps(manifest, indent=2, default=str) + "\n")
    print(json.dumps({"ranking": ranking, "qualified_winner": winner,
                      "trades": {name: results[name]["metrics"]["trades"] for name in results}}), flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--run-dir", type=Path, required=True)
    args = parser.parse_args()
    run(args.run_dir)
