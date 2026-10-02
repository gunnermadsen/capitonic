"""Offline tournament report and native-quantity robustness figures from fixed results."""

from __future__ import annotations

import argparse
import json
import math
from pathlib import Path

import plotly.graph_objects as go
import polars as pl
from plotly.subplots import make_subplots

from .time_bucket_protocol import config
from .time_bucket_source_audit import checked_run, sha256, write_json
from .time_bucket_tournament import arms_at_freeze


def display(value) -> str:
    if value is None:
        return "unavailable"
    if isinstance(value, float):
        return f"{value:,.4f}" if math.isfinite(value) else "∞" if value > 0 else "−∞"
    return str(value).replace("|", "\\|").replace("\n", " ")


def table(rows: list[dict], fields: list[str]) -> str:
    if not rows:
        return "No comparable records; no result fabricated.\n"
    return "| " + " | ".join(fields) + " |\n| " + " | ".join(["---"] * len(fields)) + " |\n" + "\n".join(
        "| " + " | ".join(display(row.get(name)) for name in fields) + " |" for row in rows) + "\n"


def diagnostic_figure(run: Path, arm: dict, dimensions: pl.DataFrame, folds: pl.DataFrame,
                      buckets: pl.DataFrame, daily: pl.DataFrame) -> dict:
    candidate = arm["candidate"]
    figure = make_subplots(rows=4, cols=2, subplot_titles=[
        "Native quantity · net / stressed PnL", "Native quantity · filled shares",
        "Native quantity · stressed profit factor", "Native quantity · recovery ratio",
        "Frozen folds · stressed Q5 FAK PnL", "Independent bucket counterfactual · stressed Q5 PnL",
        "Observed daily Q5 FAK PnL", "Observed cumulative Q5 FAK PnL"])
    if dimensions.width:
        capacity = dimensions.filter((pl.col("candidate") == candidate) & (pl.col("arm") == "primary")
            & (pl.col("economic_view") == "sequential_policy") & (pl.col("scenario") == "base")
            & (pl.col("timing_control") == "selected_wait_policy") & (pl.col("population_control") == "frozen_candidate_population")
            & (pl.col("control") == ("learned" if arm["head"] == "direction_and_exit" else "hold")))
        for kind in ["FAK", "FOK"]:
            rows = capacity.filter(pl.col("order_type") == kind).sort("requested_quantity")
            for metric, r, c in [("net_pnl", 1, 1), ("stress_pnl", 1, 1), ("filled_shares", 1, 2),
                                  ("stressed_profit_factor", 2, 1), ("recovery_ratio", 2, 2)]:
                figure.add_trace(go.Scatter(x=rows["requested_quantity"].to_list(),
                    y=[value if value is None or math.isfinite(value) else None for value in rows[metric]],
                    mode="lines+markers", name=f"{kind} {metric}", connectgaps=False), row=r, col=c)
    local = folds.filter((pl.col("candidate") == candidate) & (pl.col("arm") == "primary"))
    any_predictions = local["predictions"].sum() > 0
    figure.add_trace(go.Bar(x=local["fold"].to_list(),
        y=[row["stress_pnl"] if row["predictions"] else None for row in local.to_dicts()], name="Q5 FAK folds"), row=3, col=1)
    for kind in ["FAK", "FOK"]:
        local = buckets.filter((pl.col("candidate") == candidate) & (pl.col("arm") == "primary")
                               & (pl.col("order_type") == kind)).sort("bucket_start")
        figure.add_trace(go.Scatter(x=local["bucket_start"].to_list(), y=local["stress_pnl"].to_list() if any_predictions else [None] * local.height,
                                   mode="lines+markers", name=f"{kind} independent bucket"), row=3, col=2)
    local = daily.filter((pl.col("candidate") == candidate) & (pl.col("arm") == "primary")).sort("date")
    dates, data = [], []
    by_day = {row["date"]: row for row in local.to_dicts()}
    from datetime import date, timedelta
    if by_day:
        day, end = date.fromisoformat(min(by_day)), date.fromisoformat(max(by_day))
        while day <= end:
            dates.append(str(day))
            data.append(by_day.get(str(day)))
            day += timedelta(days=1)
    for metric in ["net_pnl", "stress_pnl"]:
        values = [row[metric] if row and row["observed_hours"] else None for row in data]
        figure.add_trace(go.Bar(x=dates, y=values, name=f"Daily {metric}"), row=4, col=1)
        running, curve = 0., []
        for value in values:
            if value is not None:
                running += value
            curve.append(running if value is not None else None)
        figure.add_trace(go.Scatter(x=dates, y=curve, name=f"Cumulative {metric}", connectgaps=False), row=4, col=2)
    availability = "Observed OOS predictions" if any_predictions else "No OOS predictions: performance unavailable"
    figure.update_layout(height=1500, width=1250, template="plotly_white", title={"text":
        f"{candidate} · offline backtest · conditional label availability<br><sup>{availability}. Bucket counterfactuals are separate; missing dates are gaps; ∞ ratios retained in tables.</sup>"},
        legend={"orientation": "h", "y": -.08}, margin={"t": 110, "b": 140})
    target = run / "diagnostics/robustness" / f"{candidate}.html"
    target.parent.mkdir(parents=True, exist_ok=True)
    if target.exists():
        raise ValueError("Unrecorded robustness chart cannot be overwritten")
    figure.write_html(target, include_plotlyjs=True, config={"displaylogo": False,
        "toImageButtonOptions": {"format": "svg", "filename": candidate}})
    return {"candidate": candidate, "path": str(target), "sha256": sha256(target)}


def build(run: Path) -> None:
    frozen, arms = config(run), arms_at_freeze(run)
    prerequisites = {name: run / "manifests" / name for name in ["tournament-results.json", "final-models.json"]}
    prerequisite_data = {name: json.loads(path.read_text()) for name, path in prerequisites.items()}
    if any(value["status"] not in ["complete", "complete_with_explicit_support_outcomes"] for value in prerequisite_data.values()):
        raise ValueError("Complete fixed evaluation and final-fit outcomes before reporting")
    for output in prerequisite_data["tournament-results.json"]["outputs"]:
        if sha256(Path(output["path"])) != output["sha256"]:
            raise ValueError("Recorded tournament result changed")
    identity = {"prerequisites": {name: sha256(path) for name, path in prerequisites.items()},
                "implementation": sha256(Path(__file__))}
    manifest_path = run / "manifests/tournament-report.json"
    if manifest_path.exists():
        prior = json.loads(manifest_path.read_text())
        if prior["identity"] != identity:
            raise ValueError("Report inputs changed")
        for artifact in prior["outputs"]:
            if sha256(Path(artifact["path"])) != artifact["sha256"]:
                raise ValueError("Report output changed")
        return
    names = ["arm-overview", "fold-results", "daily-results", "capacity-stress-controls", "bucket-results",
             "side-calibration", "reference-activity-states", "candidate-ranking", "neighbor-stability"]
    data = {name: pl.read_parquet(run / "metrics" / f"{name}.parquet") for name in names}
    decisions = json.loads((run / "metrics/qualification.json").read_text())
    final = {item["candidate"]: item for item in prerequisite_data["final-models.json"]["candidates"]}
    lines = ["# BTC time-bucket tournament\n", f"Run: `{run.name}`. Eight challengers;38declared arms; four walk-forward folds plus final holdout.\n",
        "**All results and qualification are conditional on the authorized official-label availability assumption: market close+30 minutes, or later verified availability. Historical availability is not independently proven. This assumption says nothing about settlement, cash availability or live receipt.**\n",
        "This report describes offline backtests only. No artifact is deployed, activated, admitted or authorized for runtime use. Evaluation and holdout never entered new fitting, calibration, feature selection or policy tuning. Negative and insufficient-support outcomes are retained.\n",
        "## Calendar and source scope\n",
        f"Full frozen history: {frozen['range_start']} to {frozen['range_end_exclusive']} exclusive. Previously used dates remain eligible; this run's protected roles remain excluded.\n",
        table(frozen["folds"], ["name", "calibration_start", "evaluation_start", "evaluation_end"]),
        "Core, product-specific and exact four-product matched tracks use model-specific complete cases. Direct Chainlink RefPrice, PMData RefPrice, PMData TWAP30/60 and RTDS TWAP30/60 remain distinct. The15nonempty mandatory-source ablations share exact matched point identities. Missing sources are never imputed or relabeled.\n",
        "Detailed validated source coverage, gaps, causality, conflicts and deduplication: `manifests/source-checkpoint-verification.json`, source validation manifests and `diagnostics/source-coverage/`. Training-only feature evidence: `diagnostics/training-exploration-findings.md` and the quiet-study manifest/report.\n",
        "## Qualification and primary ordering\n",
        "Q5 sequential FAK is the primary ranking. FOK independently replays the unchanged selected policy. Counterfactual bucket PnL is never added to sequential-policy income. Gates include held-out economics, fold/day diversity, neighboring quantity/time stability and qualification on fully covered UTC days. PF1.25,recovery0.75 and5–8fills/day are targets, not gates.\n",
        table(data["candidate-ranking"].to_dicts(), ["rank", "candidate", "predictions", "attempts", "fills", "fill_ratio", "net_pnl", "stress_pnl", "stressed_profit_factor", "recovery_ratio", "qualified"]),
        "Candidates without an actual OOS prediction remain in the required-candidate report below and are excluded from the distinct evaluated-candidate chart count.\n"]
    outputs = []
    prepared = [json.loads((run / "manifests" / name).read_text()) for name in ["complete-case-panels.json", "quiet-complete-case-panels.json"]]
    next_steps = {
        "conservative_selective_refresh": "Retain the refresh as the offline reference; resolve coverage or execution failures before any separately authorized prospective validation.",
        "time_bucket_conditional_selector": "Use the frozen bucket and neighbor evidence to decide whether timing warrants a separately preregistered experiment; do not choose a new mask from this holdout.",
        "refprice_twap_consensus_residual": "Require longer causal overlap and newest-date coverage of the distinct products before another all-four qualification claim.",
        "cross_venue_flow": "Prioritize the documented native-product coverage limitations; preserve venue and product identities in any later experiment.",
        "reversal_exhaustion_within_bucket_wait": "Compare the frozen wait controls and neighboring buckets in the report before proposing any new timing hypothesis.",
        "quiet_explorer": "Obtain sufficient causal known-quiet support for a separately authorized run; actual-incumbent complementarity still requires actual incumbent evidence.",
        "two_sided_buy_sell": "Use the hold/fixed180/learned controls and unknown-exit counts to determine whether selective exits merit future validation.",
        "payoff_recovery_aware_selector": "Use payoff, loss-size and recovery evidence to assess future value-head work; negative held-out economics does not authorize retuning."}
    for arm in [item for item in arms if item["primary"]]:
        candidate = arm["candidate"]
        summary = data["arm-overview"].filter((pl.col("candidate") == candidate) & (pl.col("arm") == "primary")).row(0, named=True)
        decision = decisions["candidates_and_arms"][candidate + "/primary"]
        dates = sorted({day["date"] for manifest in prepared for day in manifest["days"]
            if any(record["candidate"] == candidate and record["arm"] == "primary" and record["rows"] for record in day["artifacts"])})
        local_arms = [item for item in arms if item["candidate"] == candidate]
        lines += [f"## {candidate}\n", f"Hypothesis: {arm.get('hypothesis', 'See frozen candidate-feature contract')}.\n",
            f"Nonempty primary complete-case calendar: {dates[0] + ' through ' + dates[-1] if dates else 'none'}; {len(dates)}observed dates. All gaps remain visible in coverage artifacts.\n",
            f"Declared arms: {', '.join(item['arm'] for item in local_arms)}. Exact features: {', '.join(arm['features'])}.\n",
            table([summary], ["predictions", "admissions", "attempts", "unknown_attempts", "fills", "fill_ratio", "partial_fills", "filled_notional", "unfilled_shares", "mean_execution_price", "direction_accuracy", "admitted_direction_accuracy", "brier_score", "ece"]),
            f"Qualification: **{'conditionally qualified' if decision['qualified'] else 'not qualified'}**. Failed gates: {', '.join(decision['failed_gates']) or 'none'}. Final artifact status: {final[candidate]['status']}.\n",
            table([summary], ["net_pnl", "stress_pnl", "stressed_profit_factor", "recovery_ratio", "average_stressed_win", "average_stressed_loss", "fully_covered_days", "best_day", "worst_day", "best_day_deleted_stress", "worst_day_deleted_stress", "bootstrap_low", "bootstrap_high"])]
        for name, fields in [("fold-results", ["fold", "predictions", "fills", "net_pnl", "stress_pnl", "stressed_profit_factor", "recovery_ratio"]),
            ("side-calibration", ["side", "predictions", "direction_accuracy", "brier_score", "ece", "actual_0_predicted_0", "actual_0_predicted_1", "actual_1_predicted_0", "actual_1_predicted_1", "filled_fills", "filled_stress_pnl"]),
            ("reference-activity-states", ["reference_state", "attempts", "fills", "unknown_attempts", "net_pnl", "stress_pnl"]),
            ("daily-results", ["date", "fully_covered", "observed_hours", "fills", "filled_notional", "net_pnl", "stress_pnl"]),
            ("bucket-results", ["bucket_start", "order_type", "attempts", "fills", "net_pnl", "stress_pnl"]),
            ("neighbor-stability", ["axis", "neighbor", "fills", "stress_pnl", "pass"])]:
            records = data[name].filter((pl.col("candidate") == candidate) & (pl.col("arm") == "primary")).to_dicts() if data[name].width else []
            lines += [f"{name}:\n", table(records, fields)]
        dimensions = data["capacity-stress-controls"]
        if dimensions.width:
            local = dimensions.filter((pl.col("candidate") == candidate) & (pl.col("arm") == "primary")
                & (pl.col("economic_view") == "sequential_policy") & (pl.col("requested_quantity") == 5))
            lines += ["Q5 execution, stress and mandatory controls:\n", table(local.to_dicts(),
                ["order_type", "scenario", "control", "timing_control", "population_control", "fills", "fill_ratio", "net_pnl", "stress_pnl", "stressed_profit_factor", "recovery_ratio"])]
        chart = diagnostic_figure(run, arm, dimensions, data["fold-results"], data["bucket-results"], data["daily-results"])
        outputs.append(chart)
        lines += [f"[Capacity and robustness curves](../diagnostics/robustness/{candidate}.html). Native Q-grid values and every scenario: `metrics/capacity-stress-controls.parquet`.\n",
            f"Strength/limitation evidence: {summary['predictions']}OOS predictions, {summary['fills']}valued fills, {summary['unvalued_entry_fills']}unvalued entry fills. Economics with missing executions are known-subset totals, not proof of complete profitability.\n",
            f"Recommended next step (not executed): {next_steps[candidate]}\n"]
    lines += ["## Source ablations and candidate arms\n",
        "The matched15arms support comparisons on their shared declared population. Broader arms retain their own calendars; aggregate differences across different coverage are not isolated causal feature effects.\n",
        table(data["arm-overview"].to_dicts(), ["candidate", "arm", "predictions", "fills", "stress_pnl", "stressed_profit_factor", "recovery_ratio", "qualified"]),
        "## Refresh composition and historical references\n",
        "Actual incumbent trade activity remains unavailable. All quiet/activity comparisons above refer to **simulated refresh activity**. Missing reference observations never establish quiet, and no actual paper/live complementarity is proven.\n",
        "```json\n" + json.dumps(decisions["simulated_refresh_composition"], indent=2) + "\n```\n",
        "Immutable historical diagnostics and exact feature/cutoff exclusions are recorded in `manifests/historical-native-diagnostics.json`, `manifests/historical-controls.json`, `manifests/historical-conditional-pathways.json` and the historical compatibility manifests. Historical diagnostics are not additional challengers or qualification evidence for new fits.\n",
        "[Detailed economic interpretation and model-specific next investigations](../diagnostics/tournament-interpretation.md) reports useful and failed feature families, matched ablations, execution controls and descriptive profitable buckets without revising the experiment.\n",
        "## Interpretation and immutable evidence\n",
        "A return to market close is an outcome-valued backtest, not evidence of settlement or reusable cash at that timestamp. Filled notional measures entry usage; it does not prove cash turnover. Unknown or partial coverage cannot count as a quiet period. Calibration selects policies and quantities; evaluation only measures them.\n",
        "Top-ceil(30%×distinct evaluated candidates) chronological activity/PnL/burst figures and ranking evidence: `diagnostics/activity/charts-manifest.json`. All38arms,13buckets, native quantities, both execution modes, controls and frozen stresses remain in SSD Parquet evidence. Unsupported arms retain explicit checkpoint and prediction rejection records.\n",
        "Run status and requirement checklist: `manifests/run-status.md`, `manifests/requirements.md`. Artifact/checkpoint identities: independent/quiet fold manifests, final-models.json, and final provenance inventory. No merge, push, application image, service operation, database operation or runtime admission is part of this run.\n"]
    report = run / "metrics/tournament-report.md"
    if report.exists():
        raise ValueError("Unrecorded report cannot be overwritten")
    report.write_text("\n".join(lines))
    outputs.append({"path": str(report), "sha256": sha256(report)})
    write_json(manifest_path, {"identity": identity, "status": "complete", "outputs": outputs,
        "conditional_on_label_timing_assumption": True, "deployment_authorized": False})


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("run", type=Path)
    args = parser.parse_args()
    build(checked_run(args.run))


if __name__ == "__main__":
    main()
