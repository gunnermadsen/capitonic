"""Complete model-by-bucket and router numerical reports from saved OOF evidence."""

import json
from datetime import UTC, datetime, timedelta

import joblib
import numpy as np
import polars as pl
from sklearn.metrics import brier_score_loss, log_loss

from .micro_edge_data import END, START
from .micro_edge_evaluation import apply_policies, bootstrap_daily, metrics, router, select
from .micro_edge_models import recipes
from .time_bucket_source_preparation import _write_json, _write_parquet

EVAL_START = datetime(2026, 7, 5, tzinfo=UTC)
CUTOVER = datetime(2026, 8, 14, tzinfo=UTC)


def table(rows, title="Candidate"):
    header = f"| {title} | RTDS | PnL | Stress | PF | Trades | W/L | Win % | Coverage % | Avg sec | Confidence | Wins/loss compensation | Max DD |"
    out = [header, "|" + "---|" * 13]

    def n(x, d=2):
        return "—" if x is None else f"{x:.{d}f}"

    for r in rows:
        out.append(
            f"| {r['name']} | {r.get('rtds', '—')} | {n(r['net_pnl'])} | {n(r['stress_net_pnl'])} | {n(r['profit_factor'], 3)} | {r['trades']} | {r['wins']}/{r['losses']} | {n((r['win_rate'] or 0) * 100)} | {n(r['market_coverage'] * 100)} | {n(r['average_entry_second'], 1)} | {n(r['average_confidence'], 3)} | {n(r['recovery_wins_per_loss'], 3)} | {n(r['maximum_drawdown'])} |"
        )
    return "\n".join(out)


def report(frame, output, identity):
    evaluation = frame.filter(pl.col("window_start") >= EVAL_START)
    nmarkets = evaluation["market_id"].n_unique()
    ndays = (END - EVAL_START).days
    days = [(EVAL_START + timedelta(days=i)).date() for i in range(ndays)]
    rows = []
    bucket_rows = []
    regime_rows = []
    calibration_rows = []
    fold_rows = []
    source_rows = []
    final_rows = []
    unrestricted_rows = []
    all_admitted = []
    ledgers = {}
    prediction_summary = []
    coverage = pl.concat(
        [
            frame.with_columns(pl.col("window_start").dt.date().cast(pl.String).alias("day"))
            .group_by("day")
            .agg(
                pl.col(c).sum().alias("rows"),
                pl.col("market_id").filter(pl.col(c)).n_unique().alias("markets"),
                pl.len().alias("total_rows"),
                pl.col("market_id").n_unique().alias("total_markets"),
            )
            .with_columns(pl.lit(c[4:]).alias("group"))
            for c in frame.columns
            if c.startswith("has_")
        ]
    )
    _write_parquet(coverage, output / "coverage.parquet")
    for recipe in recipes():
        name = recipe.name
        ledger = pl.read_parquet(output / "trades" / f"{name}.parquet")
        ledgers[name] = ledger
        opp = pl.read_parquet(output / "opportunities" / f"{name}.parquet").filter(
            pl.col("window_start") >= EVAL_START
        )
        predictions = pl.read_parquet(output / "predictions" / f"{name}.parquet").filter(
            pl.col("window_start") >= EVAL_START
        )
        m = metrics(ledger, nmarkets, ndays, opp["market_id"].n_unique())
        m.update(
            name=name,
            rtds="Yes" if recipe.rtds else "No",
            stress_pnl_95_interval=bootstrap_daily(ledger, days),
        )
        rows.append(m)
        preds = predictions["probability"].to_numpy()
        labels = predictions["label_up"].to_numpy()
        valid = np.isfinite(preds)
        pred = {
            "name": name,
            "rows": int(valid.sum()),
            "brier": float(brier_score_loss(labels[valid], preds[valid])) if valid.any() else None,
            "log_loss": float(log_loss(labels[valid], preds[valid], labels=[0, 1]))
            if valid.any()
            else None,
        }
        prediction_summary.append(pred)
        m.update(
            prediction_rows=int(valid.sum()),
            executable_rows=opp.height,
            executable_markets=opp["market_id"].n_unique(),
            brier=pred["brier"],
            log_loss=pred["log_loss"],
            baseline=recipe.baseline is not None,
        )
        for low in np.arange(0, 1, 0.1):
            ix = valid & (preds >= low) & (preds < low + 0.1)
            calibration_rows.append(
                {
                    "name": name,
                    "bin_start": float(low),
                    "rows": int(ix.sum()),
                    "mean_probability": float(preds[ix].mean()) if ix.any() else None,
                    "up_rate": float(labels[ix].mean()) if ix.any() else None,
                }
            )
        finalmeta = json.loads((output / "models" / f"{name}.json").read_text())
        final = joblib.load(output / "models" / f"{name}.joblib")
        retrospective = apply_policies(opp, final["bucket_policies"])
        fm = metrics(retrospective, nmarkets, ndays)
        fm.update(name=name, rtds=m["rtds"], allowlist=finalmeta["bucket_allowlist"])
        final_rows.append(fm)
        unrestricted = select(opp, (0.5, 0.01, 0.95, "both"))
        um = metrics(unrestricted, nmarkets, ndays)
        um.update(name=name, rtds=m["rtds"])
        unrestricted_rows.append(um)
        for bucket in range(0, 300, 10):
            support = evaluation.filter((pl.col("seconds_elapsed") // 10 * 10) == bucket)
            part = opp.filter(pl.col("bucket") == bucket)
            independent = select(part, (0.5, 0.01, 0.95, "both"))
            bm = metrics(
                independent, support["market_id"].n_unique(), ndays, part["market_id"].n_unique()
            )
            bm.update(
                name=name,
                rtds=m["rtds"],
                bucket=bucket,
                available_rows=part.height,
                predictive_rows=support.height,
                retrospective_edge=bool(bm["net_pnl"] > 0 and bm["stress_net_pnl"] > 0),
                in_final_allowlist=bucket in finalmeta["bucket_allowlist"],
                evidence="unavailable"
                if part.is_empty()
                else "limited"
                if bm["trades"] < 100
                else "larger_sample",
            )
            bucket_rows.append(bm)
        for label, start, end in [
            ("pre_cutover", EVAL_START, CUTOVER),
            ("transition_day", CUTOVER, CUTOVER + timedelta(days=1)),
            ("post_cutover", CUTOVER + timedelta(days=1), END),
            ("september", datetime(2026, 9, 1, tzinfo=UTC), END),
        ]:
            part = ledger.filter(pl.col("window_start").is_between(start, end, closed="left"))
            support = evaluation.filter(
                pl.col("window_start").is_between(start, end, closed="left")
            )
            rm = metrics(part, support["market_id"].n_unique(), (end - start).days)
            rm.update(name=name, period=label, rtds=m["rtds"])
            regime_rows.append(rm)
        # Source-presence comparisons retain all eligible dates and reveal support differences.
        for group in (
            "refprice",
            "kraken",
            "spot_l2",
            "kraken_l2",
            "open_interest",
            "binance_prints",
            "candles",
        ):
            keys = evaluation.select("market_id", "observed_at", "has_" + group)
            joined = ledger.join(keys, on=["market_id", "observed_at"], how="left", validate="1:1")
            for present in (True, False):
                part = joined.filter(pl.col("has_" + group) == present)
                sm = metrics(part, nmarkets, ndays)
                sm.update(name=name, group=group, present=present)
                source_rows.append(sm)
        checkpoints = sorted((output / "checkpoints" / name).glob("*/admitted-buckets.parquet"))
        all_admitted.extend(pl.read_parquet(p) for p in checkpoints)
        detail = json.loads((output / "metrics" / f"{name}.json").read_text())
        for f in detail["folds"]:
            fold_rows.append({"name": name, **{k: v for k, v in f.items() if k != "policies"}})
    for row in rows:
        post = next(
            r for r in regime_rows if r["name"] == row["name"] and r["period"] == "post_cutover"
        )
        row["post_cutover_stress_pnl_per_day"] = post["stress_net_pnl"] / 31
    rows.sort(
        key=lambda r: (
            r["post_cutover_stress_pnl_per_day"],
            r["stress_net_pnl"],
            r["profit_factor"] or 0,
        ),
        reverse=True,
    )
    pooled = pl.concat(all_admitted).sort(["window_start", "market_id", "seconds_elapsed"])
    baseline_names = [r.name for r in recipes() if r.baseline]
    baseline_router = router(pooled.filter(pl.col("candidate").is_in(baseline_names)))
    pooled = pooled.filter(~pl.col("candidate").is_in(baseline_names))
    greedy = router(pooled)
    reserved = []
    for fold in sorted(pooled["fold"].unique().to_list()):
        current = pooled.filter(pl.col("fold") == fold)
        history = pooled.filter(
            pl.col("window_end") + pl.duration(minutes=5) < datetime.fromisoformat(fold)
        )
        if history.is_empty():
            reserved.append(router(current))
            continue
        means = history.group_by("bucket").agg(pl.col("stress_net_pnl").mean().alias("value"))
        values = dict(zip(means["bucket"].to_list(), means["value"].to_list(), strict=True))
        limits = {
            b: max([v for k, v in values.items() if k > b] + [0.0]) for b in range(0, 300, 10)
        }
        current = current.with_columns(
            pl.col("bucket").replace_strict(limits, return_dtype=pl.Float64).alias("_later_value")
        )
        current = current.filter(
            (5 * (pl.col("expected_edge") - 0.01) >= pl.col("_later_value"))
            | (pl.col("_later_value") <= 0)
        ).drop("_later_value")
        reserved.append(router(current))
    reservation = pl.concat(reserved) if reserved else greedy.head(0)
    router_rows = []
    router_buckets = []
    for name, ledger in [
        ("eligible_first", greedy),
        ("opportunity_reservation", reservation),
        ("historical_baseline_refits", baseline_router),
    ]:
        _write_parquet(ledger, output / f"router-{name}.parquet")
        m = metrics(ledger, nmarkets, ndays)
        m.update(name=name, stress_pnl_95_interval=bootstrap_daily(ledger, days))
        router_rows.append(m)
        for bucket in range(0, 300, 10):
            bm = metrics(ledger.filter(pl.col("bucket") == bucket), nmarkets, ndays)
            bm.update(name=name, bucket=bucket)
            router_buckets.append(bm)
    # Duplicate decisions and Pareto comparisons are diagnostics, not retrospective rerouting.
    duplicate_rows = []
    for i, a in enumerate(rows):
        ka = set(
            zip(
                ledgers[a["name"]]["market_id"].to_list(),
                ledgers[a["name"]]["seconds_elapsed"].to_list(),
                ledgers[a["name"]]["side"].to_list(),
                strict=True,
            )
        )
        for b in rows[i + 1 :]:
            kb = set(
                zip(
                    ledgers[b["name"]]["market_id"].to_list(),
                    ledgers[b["name"]]["seconds_elapsed"].to_list(),
                    ledgers[b["name"]]["side"].to_list(),
                    strict=True,
                )
            )
            union = ka | kb
            ratio = len(ka & kb) / len(union) if union else 1.0
            if ratio >= 0.98:
                duplicate_rows.append({"a": a["name"], "b": b["name"], "decision_jaccard": ratio})
    pareto = []
    for a in rows:
        if not a["trades"]:
            continue
        vec = lambda r: (
            r["stress_net_pnl"],
            r["profit_factor"] or 0,
            r["trades"],
            r["win_rate"] or 0,
            -(r["recovery_wins_per_loss"] if r["recovery_wins_per_loss"] is not None else 1e9),
        )
        va = vec(a)
        if not any(
            all(x >= y for x, y in zip(vec(b), va)) and any(x > y for x, y in zip(vec(b), va))
            for b in rows
            if b != a
        ):
            pareto.append(a["name"])
    for filename, data in [
        ("model-metrics", rows),
        ("bucket-metrics", bucket_rows),
        ("regime-metrics", regime_rows),
        ("fold-metrics", fold_rows),
        ("source-metrics", source_rows),
        ("calibration", calibration_rows),
        ("router-bucket-metrics", router_buckets),
        ("predictive-metrics", prediction_summary),
    ]:
        # Nested CI values remain in metrics.json; numerical tables stay ordinary Parquet.
        _write_parquet(
            pl.DataFrame(
                [{k: v for k, v in r.items() if not isinstance(v, (dict, list))} for r in data]
            ),
            output / f"{filename}.parquet",
        )
    result = {
        "run_id": output.name,
        "source_commit": identity["source_commit"],
        "source_start": START.isoformat(),
        "source_end_exclusive": END.isoformat(),
        "oof_evaluation_start": EVAL_START.isoformat(),
        "models": rows,
        "routers": router_rows,
        "pareto_models": pareto,
        "duplicates": duplicate_rows,
        "final_allowlist_retrospective": final_rows,
        "unrestricted_control": unrestricted_rows,
        "predictive": prediction_summary,
        "training_only": True,
        "deployed": False,
    }
    _write_json(output / "metrics.json", result)
    text = [
        "# BTC micro-edge model tournament",
        "",
        f"Run: `{output.name}`. Five shares per trade. RTDS means candle features were included in that model.",
        "",
        f"Source: {START.date()} through September 14 inclusive. Causal evaluation: {EVAL_START.date()}–September 14. June 7–20 initializes models; June 21–July 4 initializes policy/calibration. Final models refit all usable source dates.",
        "",
        "## Chronological model comparison",
        "",
        table(rows),
        "",
        "## Routers",
        "",
        table(router_rows, "Router"),
        "",
        "## Interpretation",
        "",
        "These are stitched chronological evaluations with policies and bucket allowlists selected only from earlier out-of-fold evidence. Individual model results allow at most one trade per market across their admitted buckets. Independent bucket tables permit one trade per market per bucket and cannot be summed into portfolio PnL.",
        "",
        "Final-model allowlist results are retrospective selection-conditioned diagnostics, not new untouched tests. Repeated tournaments reuse the full range without permanent sealing.",
        "",
        "Compensation = mean absolute losing trade / mean winning trade. PF = gross positive net trade PnL / absolute negative net trade PnL. Undefined ratios are shown as —, including zero-loss samples. Fees and 0.005/share reserve are included; stress subtracts an additional 0.01/share.",
        "",
        "## Final multi-bucket allowlists (retrospective selection)",
        "",
    ]
    for r in final_rows:
        text.append(
            f"- `{r['name']}`: "
            + (
                ", ".join(f"{b}–{b + 9}" for b in r["allowlist"])
                or "none selected; all cells retained in diagnostics"
            )
        )
    text += [
        "",
        "## Pareto contenders",
        "",
        ", ".join(f"`{n}`" for n in pareto),
        "",
        "## Detailed evidence",
        "",
        "See bucket-report.md for every model/bucket, regime-report.md for cutover and September splits, and Parquet tables for folds, calibration, source presence, predictions, opportunities and trades. Per-model models/*.json records full-range fitting counts, artifact SHA-256, producing commit and deployment status.",
        "",
        "## Limits",
        "",
        "- Cutover indicator uses the supplied August 14 date; transition-day results are shown separately. Official resolved labels are preserved. No synthetic TWAP labels replace official pre-cutover outcomes.",
        "- Optional source gaps remain missing; no global complete-case filtering. RefPrice/TWAP inputs cannot be manufactured beyond authentic source coverage. Kraken L2 uses incremental flow, not a reconstructed full book.",
        "- Book snapshots assume the recorded five-share ask was fillable; no queue-position guarantee. Performance is simulated, not live income.",
        "- Runtime export and deployment were not performed. These model artifacts are training outputs and require later UMR compatibility/parity work before runtime use.",
    ]
    (output / "report.md").write_text("\n".join(text) + "\n")
    btext = [
        "# Independent ten-second bucket results",
        "",
        "Fixed diagnostic admission: confidence ≥0.50, expected net edge ≥0.01/share, maximum share cost 0.95, either side. This prevents choosing each diagnostic cell’s thresholds from its own evaluation outcomes. The separately trained allowlists may differ.",
    ]
    for r in recipes():
        part = [
            {**x, "name": f"{x['bucket']}–{x['bucket'] + 9}"}
            for x in bucket_rows
            if x["name"] == r.name
        ]
        btext += ["", f"## {r.name}", "", table(part, "Seconds")]
    (output / "bucket-report.md").write_text("\n".join(btext) + "\n")
    rtext = ["# Model results by settlement regime and month"]
    for period in ("pre_cutover", "transition_day", "post_cutover", "september"):
        rtext += ["", f"## {period}", "", table([r for r in regime_rows if r["period"] == period])]
    (output / "regime-report.md").write_text("\n".join(rtext) + "\n")
    from .micro_edge_diagnostics import cutover_refit_diagnostic, diagnostic_report

    diagnostic_report(frame, output, pooled, ledgers)
    cutover_refit_diagnostic(frame, output, identity)
    print("REPORT COMPLETE", output, flush=True)
