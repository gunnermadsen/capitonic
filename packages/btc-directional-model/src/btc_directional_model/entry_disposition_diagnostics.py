"""Trade-ledger diagnostics and full-calendar charts for the entry tournament."""

from __future__ import annotations

import argparse
import json
import math
from collections import defaultdict
from datetime import date, timedelta
from pathlib import Path
from statistics import median, pstdev
from typing import Any

import plotly.graph_objects as go
import polars as pl
from plotly.subplots import make_subplots

MODELS = ("champion", "buy_hold", "buy_sell", "champion_specialist", "fixed_exit")
LABELS = {
    "champion": "Frozen champion",
    "buy_hold": "New buy and hold",
    "buy_sell": "New buy and sell",
    "champion_specialist": "Champion plus specialist",
    "fixed_exit": "Fixed exit control",
}
FIRST_DAY = date(2026, 7, 13)
END_DAY = date(2026, 9, 24)


def _profit_factor(values: list[float]) -> float | None:
    gain = sum(value for value in values if value > 0)
    loss = -sum(value for value in values if value < 0)
    return gain / loss if loss > 0 else None


def _drawdown(values: list[float]) -> float:
    balance = peak = maximum = 0.0
    for value in values:
        balance += value
        peak = max(peak, balance)
        maximum = max(maximum, peak - balance)
    return maximum


def _economics(rows: list[dict[str, Any]]) -> dict[str, Any]:
    ordered = sorted(rows, key=lambda row: (row["window_start"], row["market_id"]))
    net = [float(row["net_pnl"]) for row in ordered]
    stress = [float(row["stress_pnl"]) for row in ordered]
    positive = [value for value in net if value > 0]
    negative = [value for value in net if value < 0]
    stressed_positive = [value for value in stress if value > 0]
    stressed_negative = [value for value in stress if value < 0]
    reserve = sum(row["quantity"] * 0.005 * (2 if row["exit_type"] == "sell" else 1) for row in ordered)
    stress_deduction = sum(row["net_pnl"] - row["stress_pnl"] for row in ordered)
    return {
        "trades": len(ordered),
        "profitable_trades": len(positive), "losing_trades": len(negative),
        "realized_positive_rate": len(positive) / len(ordered) if ordered else None,
        "direction_correct": sum(row["side"] == row["official_outcome"] for row in ordered),
        "direction_accuracy": sum(row["side"] == row["official_outcome"] for row in ordered) / len(ordered) if ordered else None,
        "gross_profit": sum(positive), "gross_loss_abs": -sum(negative), "net_pnl": sum(net),
        "stressed_gross_profit": sum(stressed_positive),
        "stressed_gross_loss_abs": -sum(stressed_negative), "stressed_pnl": sum(stress),
        "profit_factor": _profit_factor(net), "stressed_profit_factor": _profit_factor(stress),
        "expectancy_per_trade": sum(net) / len(ordered) if ordered else None,
        "stressed_expectancy_per_trade": sum(stress) / len(ordered) if ordered else None,
        "median_trade_pnl": median(net) if net else None,
        "median_stressed_trade_pnl": median(stress) if stress else None,
        "mean_win": sum(positive) / len(positive) if positive else None,
        "mean_loss_abs": -sum(negative) / len(negative) if negative else None,
        "stressed_mean_win": sum(stressed_positive) / len(stressed_positive) if stressed_positive else None,
        "stressed_mean_loss_abs": -sum(stressed_negative) / len(stressed_negative) if stressed_negative else None,
        "wins_to_recover_mean_loss": ((-sum(stressed_negative) / len(stressed_negative)) / (sum(stressed_positive) / len(stressed_positive))
                                      if stressed_positive and stressed_negative else None),
        "max_drawdown": _drawdown(net), "stressed_max_drawdown": _drawdown(stress),
        "largest_win": max(net) if net else None, "largest_loss": min(net) if net else None,
        "largest_stressed_win": max(stress) if stress else None,
        "largest_stressed_loss": min(stress) if stress else None,
        "entry_notional": sum(row["quantity"] * row["share_cost"] for row in ordered),
        "average_share_cost": sum(row["share_cost"] for row in ordered) / len(ordered) if ordered else None,
        "reserve_deduction": reserve,
        "pnl_before_reserve": sum(net) + reserve,
        "stress_deduction": stress_deduction,
        "completed_sales": sum(row["exit_type"] == "sell" for row in ordered),
    }


def _group(rows: list[dict], dimension: str) -> list[dict]:
    groups: dict[str, list[dict]] = defaultdict(list)
    for row in rows:
        if dimension == "entry_age_seconds":
            bucket = (int(row["seconds_elapsed"]) // 30) * 30
            key = f"{bucket:03d}-{bucket + 29:03d}"
        elif dimension == "utc_hour":
            key = f"{row['window_start'].hour:02d}:00-{row['window_start'].hour:02d}:59"
        elif dimension == "utc_weekday":
            key = f"{row['window_start'].weekday()}-{row['window_start'].strftime('%A')}"
        elif dimension == "utc_month":
            key = row["window_start"].strftime("%Y-%m")
        elif dimension == "side":
            key = row["side"]
        elif dimension == "entry_origin":
            key = row["entry"]
        elif dimension == "fold":
            key = str(row["fold"])
        else:
            raise ValueError(dimension)
        groups[key].append(row)
    return [{"bucket": key, **_economics(group)} for key, group in sorted(groups.items())]


def _age_month_side(rows: list[dict]) -> list[dict]:
    groups: dict[tuple[str, str, str], list[dict]] = defaultdict(list)
    for row in rows:
        age = (int(row["seconds_elapsed"]) // 30) * 30
        key = (f"{age:03d}-{age + 29:03d}", row["window_start"].strftime("%Y-%m"), row["side"])
        groups[key].append(row)
    return [{"entry_age_seconds": age, "utc_month": month, "side": side,
             **_economics(group)} for (age, month, side), group in sorted(groups.items())]


def _best_worst(slices: list[dict], minimum: int) -> dict:
    supported = [row for row in slices if row["trades"] >= minimum]
    cohort = supported or slices
    if not cohort:
        return {"minimum_trades": minimum, "supported_bucket_count": 0, "best": None, "worst": None}
    return {
        "minimum_trades": minimum, "supported_bucket_count": len(supported),
        "best": max(cohort, key=lambda row: (row["stressed_pnl"], row["trades"])),
        "worst": min(cohort, key=lambda row: (row["stressed_pnl"], -row["trades"])),
        "fell_back_to_small_samples": not bool(supported),
    }


def _daily(rows: list[dict], status_rows: dict[date, dict]) -> list[dict]:
    trades_by_day: dict[date, list[dict]] = defaultdict(list)
    for row in rows:
        trades_by_day[row["window_start"].date()].append(row)
    result = []
    day = FIRST_DAY
    cumulative_net = cumulative_stress = 0.0
    while day < END_DAY:
        trades = trades_by_day[day]
        metrics = _economics(trades)
        cumulative_net += metrics["net_pnl"]
        cumulative_stress += metrics["stressed_pnl"]
        status = status_rows.get(day, {})
        result.append({
            "day": day, "status": status.get("status", "untested"),
            "book_checkpoint_coverage": status.get("book_checkpoint_coverage"),
            "trades": metrics["trades"], "positive_trades": metrics["profitable_trades"],
            "negative_trades": metrics["losing_trades"],
            "gross_profit": metrics["gross_profit"], "gross_loss_abs": metrics["gross_loss_abs"],
            "net_pnl": metrics["net_pnl"], "stressed_pnl": metrics["stressed_pnl"],
            "cumulative_net_pnl": cumulative_net,
            "cumulative_stressed_pnl": cumulative_stress,
        })
        day += timedelta(days=1)
    return result


def _daily_summary(daily: list[dict]) -> dict:
    traded = [row for row in daily if row["trades"]]
    full = [row for row in daily if row["status"] == "covered"]
    full_traded = [row for row in full if row["trades"]]
    ranked = sorted(traded, key=lambda row: row["stressed_pnl"])
    ranked_full = sorted(full_traded, key=lambda row: row["stressed_pnl"])
    return {
        "calendar_days": len(daily), "full_coverage_active_days": len(full),
        "partial_coverage_days": sum(row["status"] == "partial_source_coverage" for row in daily),
        "policy_disabled_days": sum(row["status"] == "policy_disabled" for row in daily),
        "trading_days": len(traded), "full_coverage_trading_days": len(full_traded),
        "full_coverage_no_trade_days": len(full) - len(full_traded),
        "positive_stressed_days": sum(row["stressed_pnl"] > 0 for row in traded),
        "negative_stressed_days": sum(row["stressed_pnl"] < 0 for row in traded),
        "mean_stressed_pnl_per_calendar_day": sum(row["stressed_pnl"] for row in daily) / len(daily),
        "mean_stressed_pnl_per_full_coverage_day": sum(row["stressed_pnl"] for row in full) / len(full) if full else None,
        "std_stressed_pnl_per_full_coverage_day": pstdev(row["stressed_pnl"] for row in full) if full else None,
        "best_trading_days": list(reversed(ranked[-5:])),
        "worst_trading_days": ranked[:5],
        "best_full_coverage_days": list(reversed(ranked_full[-5:])),
        "worst_full_coverage_days": ranked_full[:5],
        "daily_max_drawdown": _drawdown([row["net_pnl"] for row in daily]),
        "daily_stressed_max_drawdown": _drawdown([row["stressed_pnl"] for row in daily]),
    }


def _coverage_by_time(root: Path, policy_active: set[date]) -> dict:
    paths = sorted((root / "datasets" / "causal-book-panel").glob("date=*.parquet"))
    frame = pl.scan_parquet(paths, hive_partitioning=False).select(
        pl.col("window_start").dt.date().alias("day"),
        pl.col("window_start").dt.hour().alias("hour"),
        (pl.col("seconds_elapsed") // 30 * 30).alias("age"),
        (pl.col("up_book_available_at").is_not_null()
         & pl.col("down_book_available_at").is_not_null()).alias("paired"),
    ).collect()
    def summarize(source: pl.DataFrame) -> dict:
        result = {}
        for dimension, column in (("utc_hour", "hour"), ("entry_age_seconds", "age")):
            grouped = source.group_by(column).agg(
                pl.len().alias("checkpoints"), pl.col("paired").sum().alias("paired_checkpoints"),
                pl.col("day").n_unique().alias("days"),
            ).sort(column)
            result[dimension] = {str(row[column]): {
                "checkpoints": row["checkpoints"],
                "paired_checkpoints": row["paired_checkpoints"],
                "paired_fraction": row["paired_checkpoints"] / row["checkpoints"],
                "days": row["days"],
            } for row in grouped.to_dicts()}
        return result
    return {"all_calendar_days": summarize(frame),
            "buy_policy_active_days": summarize(frame.filter(pl.col("day").is_in(sorted(policy_active))))}


def _chart(root: Path, name: str, daily: list[dict], folds: list[dict]) -> Path:
    dates = [row["day"] for row in daily]
    colors = {"covered": "#149e7a", "partial_source_coverage": "#d96c5f",
              "policy_disabled": "#e4a930", "missing_source_coverage": "#666666", "untested": "#666666"}
    figure = make_subplots(rows=3, cols=1, shared_xaxes=True, vertical_spacing=0.07,
                           subplot_titles=("Filled trades by day", "Daily net and stressed PnL ($)",
                                           "Cumulative net and stressed PnL ($)"),
                           row_heights=[0.20, 0.38, 0.42])
    hover = [f"{row['day']}<br>{row['status']}<br>paired book checkpoints: "
             f"{row['book_checkpoint_coverage']:.1%}" if row["book_checkpoint_coverage"] is not None
             else f"{row['day']}<br>{row['status']}" for row in daily]
    figure.add_trace(go.Bar(x=dates, y=[row["trades"] for row in daily],
                            marker_color=[colors.get(row["status"], "#666666") for row in daily],
                            text=hover, hovertemplate="%{text}<br>trades: %{y}<extra></extra>", name="Trades"), row=1, col=1)
    figure.add_trace(go.Bar(x=dates, y=[row["stressed_pnl"] for row in daily],
                            marker_color=["#149e7a" if row["stressed_pnl"] >= 0 else "#cf5148" for row in daily],
                            text=hover, hovertemplate="%{text}<br>stressed PnL: $%{y:.2f}<extra></extra>",
                            name="Daily stressed PnL"), row=2, col=1)
    figure.add_trace(go.Scatter(x=dates, y=[row["net_pnl"] for row in daily], mode="lines+markers",
                                line={"color": "#2254a2", "width": 1.5}, marker={"size": 4},
                                name="Daily net PnL"), row=2, col=1)
    figure.add_trace(go.Scatter(x=dates, y=[row["cumulative_stressed_pnl"] for row in daily],
                                mode="lines+markers", line={"color": "#c44b44", "width": 2},
                                marker={"size": 4}, name="Cumulative stressed PnL"), row=3, col=1)
    figure.add_trace(go.Scatter(x=dates, y=[row["cumulative_net_pnl"] for row in daily],
                                mode="lines", line={"color": "#2254a2", "width": 1.5},
                                name="Cumulative net PnL"), row=3, col=1)
    for row in daily:
        if row["status"] != "covered":
            shade = "rgba(228,169,48,0.15)" if row["status"] == "policy_disabled" else "rgba(207,81,72,0.12)"
            figure.add_vrect(x0=row["day"], x1=row["day"] + timedelta(days=1),
                             fillcolor=shade, line_width=0, row="all", col=1)
    for fold in folds:
        figure.add_vline(x=fold["test_start"][:10], line_color="#aaa", line_dash="dot", row="all", col=1)
    figure.update_layout(title=f"{LABELS[name]} — every UTC day, July 13–September 23, 2026",
                         height=1050, barmode="overlay", legend={"orientation": "h"})
    figure.update_xaxes(title_text="UTC date", dtick="D7", tickformat="%b %d", row=3, col=1)
    header = ("<h2>Daily ledger and coverage</h2><p>Green bars: full coverage. Red bars or shading: partial/missing "
              "coverage. Orange shading: buy policy disabled. A zero on a full-coverage active day is a verified "
              "no-trade day. PnL on partial days reflects observed fills only. Dotted lines mark test folds. "
              "No permanently sealed holdout was used.</p>")
    lines = [header, "<table border='1' cellpadding='4'><tr><th>UTC day</th><th>Status</th><th>Paired books</th><th>Trades</th><th>Wins</th><th>Losses</th><th>Gross profit</th><th>Gross loss</th><th>Net PnL</th><th>Stressed PnL</th><th>Cumulative stressed</th></tr>"]
    for row in daily:
        coverage = "unknown" if row["book_checkpoint_coverage"] is None else f"{row['book_checkpoint_coverage']:.1%}"
        lines.append("<tr>" + "".join(f"<td>{item}</td>" for item in (
            row["day"], row["status"], coverage, row["trades"], row["positive_trades"],
            row["negative_trades"], f"{row['gross_profit']:.2f}", f"{row['gross_loss_abs']:.2f}",
            f"{row['net_pnl']:.2f}", f"{row['stressed_pnl']:.2f}",
            f"{row['cumulative_stressed_pnl']:.2f}")) + "</tr>")
    lines.append("</table>")
    path = root / "diagnostics" / f"daily-performance-{name}.html"
    path.write_text("<html><body>" + figure.to_html(full_html=False, include_plotlyjs=True) + "\n".join(lines) + "</body></html>")
    return path


def _comparison_chart(root: Path, daily_by_model: dict[str, list[dict]]) -> Path:
    names = list(MODELS)
    days = [row["day"] for row in daily_by_model[names[0]]]
    z = [[row["stressed_pnl"] if row["status"] not in ("policy_disabled", "untested") else None
          for row in daily_by_model[name]] for name in names]
    text = [[f"{row['day']}<br>{row['status']}<br>{row['trades']} trades<br>stressed PnL ${row['stressed_pnl']:.2f}"
             for row in daily_by_model[name]] for name in names]
    figure = go.Figure(go.Heatmap(x=days, y=[LABELS[name] for name in names], z=z, text=text,
                                   hovertemplate="%{text}<extra></extra>", colorscale="RdYlGn", zmid=0,
                                   colorbar={"title": "Stressed $"}))
    figure.update_layout(title="Daily stressed PnL across all tournament entries and fixed-exit control",
                         height=450, xaxis={"dtick": "D7", "tickformat": "%b %d", "title": "UTC date"})
    path = root / "diagnostics" / "daily-performance-comparison.html"
    path.write_text(figure.to_html(full_html=True, include_plotlyjs=True))
    return path


def _slice_chart(root: Path, slices: dict[str, dict], dimension: str, filename: str) -> Path:
    buckets = sorted({row["bucket"] for name in MODELS[:-1] for row in slices[name][dimension]})
    figure = go.Figure()
    for name in MODELS[:-1]:
        by_bucket = {row["bucket"]: row for row in slices[name][dimension]}
        figure.add_trace(go.Bar(x=buckets,
                                y=[by_bucket.get(bucket, {}).get("stressed_pnl", 0) for bucket in buckets],
                                customdata=[[by_bucket.get(bucket, {}).get("trades", 0),
                                             by_bucket.get(bucket, {}).get("stressed_expectancy_per_trade")]
                                            for bucket in buckets],
                                hovertemplate="%{x}<br>stressed PnL: $%{y:.2f}<br>trades: %{customdata[0]}<br>stressed per trade: %{customdata[1]:.2f}<extra></extra>",
                                name=LABELS[name]))
    figure.update_layout(title=f"Stressed PnL by {dimension.replace('_', ' ')} — descriptive observed fills",
                         barmode="group", height=650, xaxis_title=dimension.replace("_", " "),
                         yaxis_title="Stressed PnL ($)")
    path = root / "diagnostics" / filename
    path.write_text(figure.to_html(full_html=True, include_plotlyjs=True))
    return path


def _matched_decomposition(ledgers: dict[str, list[dict]], root: Path) -> dict:
    champion = {row["market_id"]: row for row in ledgers["champion"]}
    buy = {row["market_id"]: row for row in ledgers["buy_hold"]}
    composed = ledgers["champion_specialist"]
    specialist = [row for row in composed if row["entry"] == "specialist"]
    specialist_only = [row for row in specialist if row["market_id"] not in champion]
    displaced = pl.read_parquet(root / "backtests" / "specialist-displacement.parquet").to_dicts()
    sale = [row for row in ledgers["buy_sell"] if row["exit_type"] == "sell"]
    sale_detail = []
    for row in sale:
        held = buy[row["market_id"]]
        sale_detail.append({"market_id": row["market_id"], "window_start": row["window_start"].isoformat(),
                            "entry_second": row["seconds_elapsed"], "exit_second": row["exit_second"],
                            "side": row["side"], "entry_cost": row["share_cost"],
                            "sale_price": row.get("sale_price"), "hold_stress_pnl": held["stress_pnl"],
                            "sell_stress_pnl": row["stress_pnl"],
                            "incremental_stress_pnl": row["stress_pnl"] - held["stress_pnl"]})
    return {
        "champion_specialist": {
            "champion_branch": _economics([row for row in composed if row["entry"] == "champion"]),
            "specialist_branch": _economics(specialist),
            "specialist_on_no_champion_market": _economics(specialist_only),
            "displaced_champion_count": len(displaced),
            "displacement_stress_delta": sum(row["specialist_stress_pnl"] - row["champion_stress_pnl"] for row in displaced),
            "decomposition_delta": sum(row["stress_pnl"] for row in specialist_only)
                                   + sum(row["specialist_stress_pnl"] - row["champion_stress_pnl"] for row in displaced),
        },
        "buy_vs_champion": {
            "shared_market_count": len(champion.keys() & buy.keys()),
            "champion_only": _economics([row for market, row in champion.items() if market not in buy]),
            "buy_only": _economics([row for market, row in buy.items() if market not in champion]),
            "shared_opposite_side_count": sum(champion[market]["side"] != buy[market]["side"]
                                             for market in champion.keys() & buy.keys()),
        },
        "learned_sell": {"completed_sales": len(sale), "sales": sale_detail,
                         "matched_stress_delta": sum(row["stress_pnl"] for row in ledgers["buy_sell"])
                                                 - sum(row["stress_pnl"] for row in ledgers["buy_hold"])},
    }


def analyze(root: Path) -> dict:
    activity = pl.read_parquet(root / "metrics" / "daily-activity.parquet")
    folds = json.loads((root / "metrics" / "training-folds.json").read_text())
    status = {name: {row["day"]: row for row in activity.filter(pl.col("candidate") == (
        "buy_hold" if name == "fixed_exit" else name)).to_dicts()} for name in MODELS}
    policy_active = {day for day, row in status["buy_hold"].items() if row["status"] != "policy_disabled"}
    ledgers = {name: pl.read_parquet(root / "backtests" / f"{name}.parquet").to_dicts() for name in MODELS}
    detailed = {}
    slices = {}
    daily_by_model = {}
    daily_rows = []
    slice_rows = []
    for name in MODELS:
        rows = ledgers[name]
        daily = _daily(rows, status[name])
        daily_by_model[name] = daily
        daily_rows.extend({"model": name, **row} for row in daily)
        dimensions = {key: _group(rows, key) for key in (
            "entry_age_seconds", "utc_hour", "utc_weekday", "utc_month", "side", "entry_origin", "fold")}
        slices[name] = dimensions
        for dimension, buckets in dimensions.items():
            slice_rows.extend({"model": name, "dimension": dimension, **row} for row in buckets)
            if not math.isclose(sum(row["stressed_pnl"] for row in buckets),
                                sum(row["stress_pnl"] for row in rows), abs_tol=1e-7):
                raise RuntimeError(f"{name} {dimension} does not reconcile to the ledger")
        detailed[name] = {
            "economics": _economics(rows), "days": _daily_summary(daily),
            "entry_age_by_month_and_side": _age_month_side(rows),
            "best_worst_entry_age_by_stressed_pnl": _best_worst(dimensions["entry_age_seconds"], 10),
            "best_worst_utc_hour_by_stressed_pnl": _best_worst(dimensions["utc_hour"], 5),
            "slices": dimensions,
            "daily_chart": str(_chart(root, name, daily, folds).relative_to(root)),
        }
        if not math.isclose(daily[-1]["cumulative_stressed_pnl"],
                            detailed[name]["economics"]["stressed_pnl"], abs_tol=1e-7):
            raise RuntimeError(f"{name} daily ledger does not reconcile")
    coverage = _coverage_by_time(root, policy_active)
    pl.DataFrame(daily_rows).write_parquet(root / "metrics" / "daily-economics.parquet")
    pl.DataFrame(slice_rows, infer_schema_length=None).write_parquet(root / "metrics" / "time-slices.parquet")
    charts = {"comparison": str(_comparison_chart(root, daily_by_model).relative_to(root)),
              "entry_age": str(_slice_chart(root, slices, "entry_age_seconds", "entry-age-performance.html").relative_to(root)),
              "utc_hour": str(_slice_chart(root, slices, "utc_hour", "utc-hour-performance.html").relative_to(root))}
    result = {
        "schema_version": "entry-disposition-deep-dive-v1",
        "date_range": [FIRST_DAY.isoformat(), (END_DAY - timedelta(days=1)).isoformat()],
        "ranking_basis": "Descriptive stressed PnL for observed fills; entry-age ranks require at least 10 trades and UTC-hour ranks at least five where available.",
        "models": detailed, "source_coverage": coverage,
        "matched_comparisons": _matched_decomposition(ledgers, root),
        "charts": charts,
        "limits": ["Timing slices are exploratory and were identified after reading the evaluation ledgers.",
                   "A positive bucket or hour is not a deployable policy; source coverage, policy selection, and small sample sizes differ by slice.",
                   "Historical champion comparisons overlap the frozen champion training period."]}
    (root / "metrics" / "deep-dive.json").write_text(json.dumps(result, indent=2, sort_keys=True, default=str) + "\n")
    print(json.dumps({"models": {name: {"trades": value["economics"]["trades"],
                                    "stress_pnl": value["economics"]["stressed_pnl"],
                                    "best_age": value["best_worst_entry_age_by_stressed_pnl"]["best"]["bucket"],
                                    "worst_age": value["best_worst_entry_age_by_stressed_pnl"]["worst"]["bucket"]}
                                  for name, value in detailed.items()}}, default=str), flush=True)
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--run-dir", type=Path, required=True)
    arguments = parser.parse_args()
    analyze(arguments.run_dir)
