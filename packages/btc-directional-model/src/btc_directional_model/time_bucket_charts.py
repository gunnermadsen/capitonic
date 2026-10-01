"""Offline trade-activity figures from the primary-owned final ranking and ledgers.

Uses the existing training Python's Plotly dependency. HTML embeds its plotting
runtime, requires no network, and supports SVG export from the figure toolbar.
"""

from __future__ import annotations

import argparse
import html
import json
import math
from datetime import UTC, datetime, timedelta
from pathlib import Path

import plotly
import plotly.graph_objects as go
import polars as pl
from plotly.subplots import make_subplots

from .time_bucket_protocol import boundary, config
from .time_bucket_source_audit import checked_run, sha256, write_json

STATUS_COLORS = {
    "fully_covered": "#ffffff",
    "no_trade_covered": "#d4eddf",
    "partial_coverage": "#ffe1a3",
    "missing_source": "#f2b5b5",
    "untested": "#d9dde3",
    "policy_disabled": "#c9dcf7",
}
STATUS_LABELS = {
    "fully_covered": "Covered",
    "no_trade_covered": "Covered · zero trades",
    "partial_coverage": "Partial coverage",
    "missing_source": "Missing source",
    "untested": "Untested",
    "policy_disabled": "Policy disabled",
}


def _utc(value: object) -> datetime:
    timestamp = datetime.fromisoformat(value) if isinstance(value, str) else value
    if not isinstance(timestamp, datetime) or timestamp.tzinfo is None:
        raise ValueError("Activity timestamps must establish their UTC timezone")
    return timestamp.astimezone(UTC)


def _number(value: object, *, nullable: bool = False) -> float | None:
    if value is None and nullable:
        return None
    if value is None or not math.isfinite(float(value)):
        raise ValueError("Activity metrics must be finite, with explicit nulls for unknown values")
    return float(value)


def selected_candidates(ranking: pl.DataFrame, fraction: float) -> list[str]:
    """Consume the supplied rank, never recompute economic ordering or qualification."""
    required = {"candidate", "rank", "stress_pnl", "stressed_profit_factor", "recovery_ratio"}
    if required - set(ranking.columns):
        raise ValueError("Candidate ranking lacks required primary-owned columns")
    if ranking["candidate"].null_count() or ranking["candidate"].n_unique() != ranking.height:
        raise ValueError("Ranking must contain each distinct evaluated candidate exactly once")
    if ranking["rank"].null_count() or ranking["rank"].n_unique() != ranking.height:
        raise ValueError("Primary ranking must contain unique nonnull ranks")
    if any(float(rank) != int(rank) or rank < 1 for rank in ranking["rank"]):
        raise ValueError("Ranks must be positive integers")
    if not 0 < fraction <= 1:
        raise ValueError("Invalid frozen chart fraction")
    count = max(1, math.ceil(fraction * ranking.height)) if ranking.height else 0
    return ranking.sort("rank")["candidate"].head(count).to_list()


def calendar(hourly: pl.DataFrame, candidate: str, frozen: dict) -> tuple[list[dict], int]:
    start = min(boundary(fold["evaluation_start"]) for fold in frozen["folds"])
    end = max(boundary(fold["evaluation_end"]) for fold in frozen["folds"])
    observations = {}
    for row in hourly.filter(pl.col("candidate") == candidate).iter_rows(named=True):
        timestamp = _utc(row["hour"])
        if timestamp.minute or timestamp.second or timestamp.microsecond or not start <= timestamp < end:
            raise ValueError("Hourly activity must use aligned UTC hours inside the complete evaluated calendar")
        if timestamp in observations or row["coverage_status"] not in STATUS_COLORS:
            raise ValueError("Duplicate activity hour or unknown coverage status")
        row = dict(row, hour=timestamp)
        for field in ("filled_trades", "net_pnl", "stress_pnl"):
            row[field] = _number(row[field], nullable=True)
        if row["filled_trades"] is not None and (row["filled_trades"] < 0 or row["filled_trades"] % 1):
            raise ValueError("Filled-trade counts must be nonnegative integers")
        if row["coverage_status"] in {"fully_covered", "no_trade_covered"} and any(
            row[field] is None for field in ("filled_trades", "net_pnl", "stress_pnl")
        ):
            raise ValueError("Covered hours require observed counts and economics")
        if row["coverage_status"] == "no_trade_covered" and any(row[field] != 0 for field in ("filled_trades", "net_pnl", "stress_pnl")):
            raise ValueError("Covered no-trade hours cannot contain fills or nonzero PnL")
        if row["coverage_status"] in {"missing_source", "untested"}:
            # Upstream zero placeholders are never drawn as observed zero activity.
            if any(row[field] not in (None, 0) for field in ("filled_trades", "net_pnl", "stress_pnl")):
                raise ValueError("Unknown activity coverage cannot carry known nonzero economics")
            row.update(filled_trades=None, net_pnl=None, stress_pnl=None)
        if row["coverage_status"] == "policy_disabled" and any(row[field] not in (None, 0) for field in ("filled_trades", "net_pnl", "stress_pnl")):
            raise ValueError("Policy-disabled hours cannot contain simulated fills")
        observations[timestamp] = row
    rows, missing = [], 0
    timestamp = start
    while timestamp < end:
        row = observations.get(timestamp)
        if row is None:
            missing += 1
            row = {"hour": timestamp, "coverage_status": "untested", "filled_trades": None,
                   "net_pnl": None, "stress_pnl": None}
        rows.append(row)
        timestamp += timedelta(hours=1)
    return rows, missing


def _bursts(frame: pl.DataFrame, candidate: str, start: datetime, end: datetime) -> list[dict]:
    rows = []
    for source in frame.filter(pl.col("candidate") == candidate).iter_rows(named=True):
        row = dict(source)
        row["start"], row["end"] = _utc(row["start"]), _utc(row["end"])
        if not start <= row["start"] <= row["end"] < end:
            raise ValueError("Burst lies outside the evaluated calendar")
        row["trade_count"] = _number(row["trade_count"])
        for field in ("net_pnl", "stress_pnl"):
            row[field] = _number(row[field], nullable=True)
        if row["trade_count"] < 1 or row["trade_count"] % 1:
            raise ValueError("Burst trade counts must be positive integers")
        row["preceding_verified_quiet_minutes"] = _number(row["preceding_verified_quiet_minutes"], nullable=True)
        if row["preceding_verified_quiet_minutes"] is not None and row["preceding_verified_quiet_minutes"] < 0:
            raise ValueError("Verified quiet duration cannot be negative")
        rows.append(row)
    return sorted(rows, key=lambda row: (row["start"], row["end"]))


def render(path: Path, candidate: str, hourly: pl.DataFrame, bursts: pl.DataFrame,
           frozen: dict, *, synthetic: bool = False) -> dict:
    rows, missing = calendar(hourly, candidate, frozen)
    start, end = rows[0]["hour"], rows[-1]["hour"] + timedelta(hours=1)
    activity = _bursts(bursts, candidate, start, end)
    title = ("SYNTHETIC VERIFICATION · " if synthetic else "") + f"{candidate} · offline backtest"
    figure = make_subplots(rows=3, cols=1, shared_xaxes=True, vertical_spacing=.075,
        subplot_titles=("Filled trades per UTC hour", "Hourly PnL · USD", "Observed cumulative PnL · USD"))
    hours = [row["hour"] for row in rows]
    statuses = [STATUS_LABELS[row["coverage_status"]] for row in rows]
    figure.add_trace(go.Bar(x=hours, y=[row["filled_trades"] for row in rows], width=3_600_000,
        name="Offline backtest · fills", marker_color="#344f7b", customdata=statuses,
        hovertemplate="%{x|%Y-%m-%d %H:%M UTC}<br>%{y} filled trades<br>%{customdata}<extra></extra>"), row=1, col=1)
    quiet = [row["hour"] for row in rows if row["coverage_status"] == "no_trade_covered"]
    figure.add_trace(go.Scatter(x=quiet, y=[0] * len(quiet), mode="markers", marker={"size": 3, "color": "#35875a"},
        showlegend=False, hovertemplate="%{x|%Y-%m-%d %H:%M UTC}<br>Covered, zero fills<extra></extra>"), row=1, col=1)
    for field, label, color in [("net_pnl", "Net", "#207b9e"), ("stress_pnl", "Stressed", "#b45309")]:
        values = [row[field] for row in rows]
        figure.add_trace(go.Bar(x=hours, y=values, width=3_600_000, opacity=.62, marker_color=color,
            name=f"Offline backtest · {label}", legendgroup=field,
            hovertemplate="%{x|%Y-%m-%d %H:%M UTC}<br>$%{y:.2f}<extra></extra>"), row=2, col=1)
        cumulative, total = [], 0.0
        for value in values:
            if value is not None:
                total += value
            cumulative.append(None if value is None else total)
        figure.add_trace(go.Scatter(x=hours, y=cumulative, mode="lines", line={"color": color, "width": 2},
            name=f"Offline backtest · cumulative {label}", legendgroup=field, showlegend=False, connectgaps=False,
            hovertemplate="%{x|%Y-%m-%d %H:%M UTC}<br>Observed cumulative $%{y:.2f}<extra></extra>"), row=3, col=1)
    # Merge contiguous equal statuses to keep full-calendar SVG output compact.
    first = 0
    for i in range(1, len(rows) + 1):
        if i < len(rows) and rows[i]["coverage_status"] == rows[first]["coverage_status"]:
            continue
        status = rows[first]["coverage_status"]
        if status != "fully_covered":
            figure.add_vrect(x0=hours[first], x1=hours[i - 1] + timedelta(hours=1),
                fillcolor=STATUS_COLORS[status], opacity=.30, line_width=0, layer="below", row="all", col=1)
        first = i
    for fold in frozen["folds"]:
        for field in ("evaluation_start", "evaluation_end"):
            figure.add_vline(x=boundary(fold[field]), line_width=1, line_dash="dot", line_color="#576375", row="all", col=1)
        figure.add_annotation(x=boundary(fold["evaluation_start"]), y=1.045, xref="x", yref="paper",
            text=html.escape(fold["name"]), showarrow=False, xanchor="left", font={"size": 10, "color": "#445166"})
    burst_rows, material = [], 0
    maximum_count = max((row["filled_trades"] or 0 for row in rows), default=0)
    for number, burst in enumerate(activity, 1):
        significant = burst["stress_pnl"] is not None and (burst["stress_pnl"] >= frozen["charts"]["positive_burst_annotation_usd"]
                       or burst["stress_pnl"] <= frozen["charts"]["negative_burst_annotation_usd"])
        color = "#6b7280" if burst["stress_pnl"] is None else "#21734b" if burst["stress_pnl"] >= 0 else "#a33434"
        if significant:
            material += 1
            figure.add_annotation(x=burst["start"], y=maximum_count * 1.06 + 1 + (number % 3) * .8,
                text=f"#{number}", showarrow=True, arrowhead=0, ax=0, ay=-14,
                font={"size": 9, "color": color}, row=1, col=1)
        quiet = burst["preceding_verified_quiet_minutes"]
        quiet_text = "unknown" if quiet is None else f"{quiet:,.0f} min"
        net_text = "unknown" if burst["net_pnl"] is None else f'${burst["net_pnl"]:,.2f}'
        stress_text = "unknown" if burst["stress_pnl"] is None else f'${burst["stress_pnl"]:,.2f}'
        burst_rows.append(f'<tr class="{"material" if significant else ""}"><td>#{number}</td>'
            f'<td>{burst["start"]:%m-%d %H:%M}<br><small>to {burst["end"]:%m-%d %H:%M}</small></td>'
            f'<td>{int(burst["trade_count"])}</td><td>{quiet_text}</td>'
            f'<td>{net_text}</td><td style="color:{color}">{stress_text}</td></tr>')
    figure.update_layout(title={"text": html.escape(title), "font": {"size": 19}}, height=930,
        template="plotly_white", font={"family": "Arial, sans-serif", "size": 11}, barmode="overlay",
        margin={"l": 66, "r": 18, "t": 130, "b": 55}, hovermode="x unified",
        legend={"orientation": "h", "y": 1.16, "x": 0},
        paper_bgcolor="white", plot_bgcolor="white")
    figure.update_xaxes(range=[start, end], tickformat="%b %d", showgrid=True, gridcolor="#edf0f3")
    figure.update_xaxes(title_text="UTC · complete evaluated calendar", row=3, col=1)
    figure.update_yaxes(rangemode="tozero", row=1, col=1)
    plot = figure.to_html(full_html=False, include_plotlyjs=True, div_id="activity-figure",
        config={"displaylogo": False, "responsive": True, "toImageButtonOptions": {
            "format": "svg", "filename": "offline-trade-activity", "width": 1350, "height": 930}})
    legend = "".join(f'<span><i style="background:{STATUS_COLORS[name]}"></i>{STATUS_LABELS[name]}</span>' for name in STATUS_COLORS)
    document = f'''<!doctype html><html lang="en"><head><meta charset="utf-8"><title>{html.escape(title)}</title>
<style>body{{margin:0;padding:22px;background:#f3f5f8;color:#1c2a3c;font:13px Arial,sans-serif}}
.layout{{display:grid;grid-template-columns:minmax(680px,2.1fr) minmax(390px,1fr);gap:18px}}
.plot,.aside{{background:white;border:1px solid #dce2ea;border-radius:8px;padding:12px}}
.legend{{display:flex;flex-wrap:wrap;gap:13px;margin-bottom:10px}}.legend span{{white-space:nowrap}}
i{{display:inline-block;width:12px;height:12px;border:1px solid #aab3bf;margin-right:5px;vertical-align:middle}}
h2{{font-size:17px;margin:4px 0 8px}}p{{line-height:1.5}}.table-scroll{{max-height:830px;overflow:auto}}
table{{width:100%;border-collapse:collapse;font-size:11px}}th{{background:#edf1f6;position:sticky;top:0;text-align:left}}
td,th{{padding:8px 5px;border-bottom:1px solid #e6eaf0}}.material{{background:#f5f8fc;font-weight:bold}}
small{{font-weight:normal;color:#596779}}.note{{color:#46566e;font-size:12px;margin:12px 0 0}}
@media(max-width:1100px){{.layout{{grid-template-columns:1fr}}.table-scroll{{max-height:none}}}}
@media print{{.layout{{grid-template-columns:2fr 1fr}}.table-scroll{{max-height:none;overflow:visible}}body{{padding:0}}}}
</style></head><body><div class="legend">{legend}</div><div class="layout"><section class="plot">{plot}</section>
<aside class="aside"><h2>Chronological activity bursts · UTC</h2><p>Offline backtest only. Material stressed bursts are numbered on the chart; all bursts remain listed.</p>
<div class="table-scroll"><table><thead><tr><th>ID</th><th>Start / end</th><th>Trades</th><th>Prior verified quiet</th><th>Net</th><th>Stressed</th></tr></thead>
<tbody>{''.join(burst_rows) or '<tr><td colspan="6">No recorded activity bursts.</td></tr>'}</tbody></table></div></aside></div>
<p class="note">Consistent hourly cadence across the full evaluated range. Shading distinguishes coverage and policy state; gaps never count as verified quiet.
Cumulative lines sum observed PnL, including reported partial-coverage observations, and break across unknown intervals; they are not complete-calendar profit claims.
Fold and holdout starts/ends are dotted. This chart contains no actual paper/live or incumbent trade series. SVG export is available from the figure toolbar.</p>
</body></html>'''
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(document)
    return {"candidate": candidate, "path": str(path), "sha256": sha256(path), "hour_count": len(rows),
            "absent_input_hours_marked_untested": missing, "bursts": len(activity), "material_bursts": material,
            "coverage_hours": {status: sum(row["coverage_status"] == status for row in rows) for status in STATUS_COLORS},
            "synthetic": synthetic}


def build(run: Path) -> None:
    frozen = config(run)
    inputs = {name: run / "metrics" / filename for name, filename in {
        "ranking": "candidate-ranking.parquet", "hourly": "activity-hourly.parquet", "bursts": "activity-bursts.parquet"}.items()}
    ranking, hourly, bursts = [pl.read_parquet(inputs[name]) for name in ("ranking", "hourly", "bursts")]
    selected = selected_candidates(ranking, frozen["charts"]["fraction_distinct_candidates"])
    target = run / "diagnostics/activity"
    identity = {"inputs": {name: {"path": str(path), "sha256": sha256(path)} for name, path in inputs.items()},
                "configuration_sha256": sha256(run / "inputs/tournament-freeze.json"),
                "implementation_sha256": sha256(Path(__file__)), "plotly_version": plotly.__version__}
    manifest_path = target / "charts-manifest.json"
    if manifest_path.exists():
        existing = json.loads(manifest_path.read_text())
        if existing["identity"] != identity:
            raise ValueError("Activity chart evidence changed; refusing to overwrite existing figures")
        for artifact in existing["charts"]:
            if sha256(Path(artifact["path"])) != artifact["sha256"]:
                raise ValueError("Existing chart identity changed")
        return
    charts, limitations = [], []
    for candidate in selected:
        recorded = hourly.filter(pl.col("candidate") == candidate)
        comparable = recorded.filter(pl.col("coverage_status").is_in(["fully_covered", "no_trade_covered", "partial_coverage", "policy_disabled"])
            & pl.col("filled_trades").is_not_null() & pl.col("net_pnl").is_not_null() & pl.col("stress_pnl").is_not_null())
        if not comparable.height:
            limitations.append({"candidate": candidate, "reason": "No comparable activity ledger; no timeline fabricated"})
            continue
        slug = "".join(character if character.isalnum() or character in "-_" else "_" for character in candidate)
        output = target / f"{slug}.html"
        if output.exists():
            raise ValueError("Unrecorded existing chart cannot be overwritten")
        charts.append(render(output, candidate, hourly, bursts, frozen))
    write_json(manifest_path, {"identity": identity, "status": "complete" if not limitations else "partial_missing_ledgers",
        "ranking_order": ranking.sort("rank")["candidate"].to_list(), "ranking_rule": frozen["charts"]["ranking"],
        "distinct_candidates": ranking.height, "selected": selected, "charts": charts, "limitations": limitations,
        "scope": "Offline backtest only; primary-owned ranks; no qualification or policy changes"})


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("run", type=Path)
    args = parser.parse_args()
    build(checked_run(args.run))


if __name__ == "__main__":
    main()
