"""Descriptive matched comparisons; never used to choose outer-fold policies."""

from datetime import UTC, datetime, timedelta

import numpy as np
import polars as pl

from .micro_edge_data import END, START
from .micro_edge_evaluation import metrics, select
from .micro_edge_models import recipes
from .time_bucket_source_preparation import _write_json, _write_parquet

EVAL_START = datetime(2026, 7, 5, tzinfo=UTC)


def paired_interval(a, b, days):
    def daily(t):
        sums = (
            t.with_columns(pl.col("window_start").dt.date().alias("day"))
            .group_by("day")
            .agg(pl.col("stress_net_pnl").sum())
        )
        values = dict(zip(sums["day"].to_list(), sums["stress_net_pnl"].to_list(), strict=True))
        return np.array([values.get(day, 0.0) for day in days])

    delta = daily(a) - daily(b)
    rng = np.random.default_rng(20260915)
    values = np.array([rng.choice(delta, len(delta), replace=True).sum() for _ in range(500)])
    return [float(delta.sum()), *np.quantile(values, [0.025, 0.975]).tolist()]


def diagnostic_report(frame, output, pooled, ledgers):
    evaluation = frame.filter(pl.col("window_start") >= EVAL_START)
    days = [(EVAL_START + timedelta(days=i)).date() for i in range((END - EVAL_START).days)]
    nmarkets = evaluation["market_id"].n_unique()
    # Day/source/bucket counts include missing cells, not just tradable ledgers.
    coverage = []
    for source in [c for c in frame.columns if c.startswith("has_")]:
        data = (
            frame.with_columns(
                pl.col("window_start").dt.date().alias("day"),
                (pl.col("seconds_elapsed") // 10 * 10).alias("bucket"),
            )
            .group_by("day", "bucket")
            .agg(
                pl.len().alias("total_rows"),
                pl.col("market_id").n_unique().alias("total_markets"),
                pl.col(source).sum().alias("present_rows"),
                pl.col("market_id").filter(pl.col(source)).n_unique().alias("present_markets"),
            )
            .with_columns(pl.lit(source[4:]).alias("source"))
        )
        coverage.append(data)
    _write_parquet(pl.concat(coverage), output / "coverage-day-source-bucket.parquet")
    coverage_summary = []
    for source in [c for c in frame.columns if c.startswith("has_")]:
        present = frame.filter(pl.col(source))
        coverage_summary.append(
            {
                "source": source[4:],
                "rows": present.height,
                "markets": present["market_id"].n_unique(),
                "first_decision": str(present["observed_at"].min()),
                "last_decision": str(present["observed_at"].max()),
            }
        )
    _write_json(
        output / "source-coverage-summary.json",
        {
            "start": str(START),
            "end_exclusive": str(END),
            "rows": frame.height,
            "markets": frame["market_id"].n_unique(),
            "sources": coverage_summary,
            "labels": "polymarket.btc_interval_markets official_outcome; validation_status valid",
            "label_embargo_minutes": 5,
        },
    )
    # Five-second cells and genuine next-observed-book latency diagnostic.
    five = []
    latency = []
    attribution = []
    baseline_opps = {
        r.name: pl.read_parquet(output / "opportunities" / f"{r.name}.parquet").filter(
            pl.col("window_start") >= EVAL_START
        )
        for r in recipes()
        if r.baseline
    }
    comparisons = []
    matched = []
    for name, ledger in ledgers.items():
        opp = pl.read_parquet(output / "opportunities" / f"{name}.parquet").filter(
            pl.col("window_start") >= EVAL_START
        )
        for second in range(0, 300, 5):
            part = select(
                opp.filter(pl.col("seconds_elapsed") == second), (0.5, 0.01, 0.95, "both")
            )
            five.append({"name": name, "second": second, **metrics(part, nmarkets, len(days))})
        # Compare equal trade counts selected by confidence, never realized PnL.
        # This is an evaluation-set descriptive curve, not a prospective policy.
        unrestricted = select(opp, (0.5, 0.01, 0.95, "both"))
        for count in (100, 250, 500, 1000, 2000):
            if unrestricted.height < count:
                continue
            top = unrestricted.sort(
                ["selected_probability", "window_start"], descending=[True, False]
            ).head(count)
            matched.append(
                {"name": name, "target_trades": count, **metrics(top, nmarkets, len(days))}
            )
        for bucket in range(0, 300, 10):
            part = unrestricted.filter(pl.col("bucket") == bucket)
            attribution.append(
                {"name": name, "bucket": bucket, **metrics(part, nmarkets, len(days))}
            )
        for base, baseopp in baseline_opps.items():
            if name == base:
                continue
            # Both ledgers evaluated on identical markets with authentic execution support.
            common = (
                opp.select("market_id")
                .unique()
                .join(baseopp.select("market_id").unique(), on="market_id")
            )
            a = ledger.join(common, on="market_id")
            b = ledgers[base].join(common, on="market_id")
            delta, lo, hi = paired_interval(a, b, days)
            comparisons.append(
                {
                    "name": name,
                    "baseline": base,
                    "common_executable_markets": common.height,
                    "new_trades": a.height,
                    "baseline_trades": b.height,
                    "stress_delta": delta,
                    "paired_day_95_low": lo,
                    "paired_day_95_high": hi,
                }
            )
        later = evaluation.select(
            "market_id",
            "seconds_elapsed",
            "up_ask_vwap_5",
            "down_ask_vwap_5",
            "fee_rate",
            "pm_up_book_age_seconds",
            "pm_down_book_age_seconds",
        ).with_columns((pl.col("seconds_elapsed") - 5).alias("seconds_elapsed"))
        delayed = ledger.join(later, on=["market_id", "seconds_elapsed"], how="inner")
        delayed = delayed.with_columns(
            pl.when(pl.col("side") == "up")
            .then(pl.col("up_ask_vwap_5"))
            .otherwise(pl.col("down_ask_vwap_5"))
            .alias("_cost"),
            pl.when(pl.col("side") == "up")
            .then(pl.col("pm_up_book_age_seconds"))
            .otherwise(pl.col("pm_down_book_age_seconds"))
            .alias("_age"),
        ).filter(
            pl.col("_cost").is_finite()
            & pl.col("_age").is_between(0, 2)
            & pl.col("fee_rate").is_finite()
        )
        original = delayed.select(ledger.columns)
        delayed = delayed.with_columns(
            (
                5
                * (
                    pl.col("won").cast(pl.Float64)
                    - pl.col("_cost")
                    - pl.col("fee_rate") * pl.col("_cost") * (1 - pl.col("_cost"))
                    - 0.005
                )
            ).alias("net_pnl"),
            pl.col("_cost").alias("share_cost"),
        ).with_columns((pl.col("net_pnl") - 0.05).alias("stress_net_pnl"))
        latency.append(
            {
                "name": name,
                "original_trades": ledger.height,
                "later_book_supported_trades": delayed.height,
                "original_same_rows_pnl": float(original["net_pnl"].sum()),
                "five_second_delayed_pnl": float(delayed["net_pnl"].sum()),
                "five_second_delayed_stress_pnl": float(delayed["stress_net_pnl"].sum()),
            }
        )
    from .micro_edge_reporting import rank_scored

    full = rank_scored(pooled)
    incremental = []
    for name in ledgers:
        without = rank_scored(pooled.filter(pl.col("candidate") != name))
        new = full.join(without.select("market_id"), on="market_id", how="anti")
        paired = full.join(
            without.select(
                "market_id",
                pl.col("candidate").alias("other_candidate"),
                pl.col("seconds_elapsed").alias("other_second"),
            ),
            on="market_id",
        )
        changed = paired.filter(
            (pl.col("candidate") != pl.col("other_candidate"))
            | (pl.col("seconds_elapsed") != pl.col("other_second"))
        )
        incremental.append(
            {
                "name": name,
                "router_pnl_delta_when_included": float(
                    full["net_pnl"].sum() - without["net_pnl"].sum()
                ),
                "incremental_markets": new.height,
                "displaced_trades": changed.height,
                "displaced_later_trades": changed.filter(
                    pl.col("other_second") > pl.col("seconds_elapsed")
                ).height,
            }
        )
    for filename, rows in [
        ("five-second-metrics", five),
        ("matched-frequency", matched),
        ("matched-baselines", comparisons),
        ("unrestricted-bucket-attribution", attribution),
        ("latency-diagnostic", latency),
        ("router-incremental", incremental),
    ]:
        _write_parquet(pl.DataFrame(rows), output / f"{filename}.parquet")
    _write_json(
        output / "diagnostics.json",
        {
            "matched_baselines": comparisons,
            "latency": latency,
            "router_incremental": incremental,
            "matched_frequency_selection": "highest OOF confidence on evaluation set; descriptive only",
            "router_incremental_method": "remove one candidate while holding earlier-fold scores and policies fixed",
            "baseline_comparison": "same executable markets, same chronological folds, refitted historical features/estimator; new common OOF calibration/admission, official labels, VWAP5; not an exact reproduction of old target/selection workflow",
        },
    )


def cutover_refit_diagnostic(frame, output, identity):
    """Post-cutover-only transfer diagnostic; never replaces a full-range final model."""
    import subprocess
    from pathlib import Path

    import joblib

    from .core_extract import file_sha256
    from .micro_edge_data import CACHE
    from .micro_edge_evaluation import opportunities
    from .micro_edge_models import (
        feature_names,
        fit_estimator,
        fit_indices,
        market_weights,
        matrix,
        predict_estimator,
    )
    from .micro_edge_tournament import dump

    identity = {**identity, "diagnostic_code_sha256": file_sha256(Path(__file__))}
    recipe = next(r for r in recipes() if r.name == "all_dimensions__no_rtds")
    manifest = __import__("json").loads((CACHE / "panel-manifest.json").read_text())
    features = feature_names(recipe, manifest["feature_groups"])
    # August 14 is reported separately because exact transition timing is unverified.
    f = frame.filter(pl.col("window_start") >= datetime(2026, 8, 15, tzinfo=UTC))
    x = matrix(f, features)
    y = f["label_up"].to_numpy()
    parts = []
    start = datetime(2026, 8, 23, tzinfo=UTC)
    while start < END:
        end = min(start + timedelta(days=7), END)
        train = fit_indices(f, start)
        test = np.flatnonzero(
            (f["window_start"].to_numpy() >= np.datetime64(start.replace(tzinfo=None), "us"))
            & (f["window_start"].to_numpy() < np.datetime64(end.replace(tzinfo=None), "us"))
        )
        path = output / "cutover-diagnostic" / f"{start.date()}.joblib"
        meta = path.with_suffix(".json")
        if path.exists() and meta.exists():
            saved = __import__("json").loads(meta.read_text())
            if saved["identity"] != identity or saved["sha256"] != file_sha256(path):
                raise RuntimeError("cutover diagnostic checkpoint mismatch")
            model = joblib.load(path)
        else:
            model = fit_estimator(
                x[train], y[train], market_weights(f.select("market_id")[train]), "tree"
            )
            dump(model, path)
            _write_json(
                meta,
                {
                    "identity": identity,
                    "sha256": file_sha256(path),
                    "producing_commit": subprocess.check_output(
                        ["git", "rev-parse", "HEAD"], text=True
                    ).strip(),
                    "artifact_path": str(path),
                    "qualification_status": "cutover_diagnostic_only_not_deployed",
                    "fit_start": str(f["window_start"][train].min()),
                    "fit_last_end": str(f["window_end"][train].max()),
                    "evaluation_start": str(start),
                },
            )
        p = predict_estimator(model, x[test])
        parts.append(opportunities(f[test], p, "post_cutover_only_diagnostic"))
        start = end
    post = pl.concat(parts)
    pooled = pl.read_parquet(output / "opportunities" / "all_dimensions__no_rtds.parquet").filter(
        pl.col("window_start") >= datetime(2026, 8, 23, tzinfo=UTC)
    )
    # Compare raw probabilities for both fits: calibration is not the transfer variable.
    raw = pl.read_parquet(output / "predictions" / "all_dimensions__no_rtds.parquet").filter(
        pl.col("window_start") >= datetime(2026, 8, 23, tzinfo=UTC)
    )
    matched_frame = frame.join(
        raw.select("row_id", "raw_probability"), on="row_id", how="inner", validate="1:1"
    )
    pooled = opportunities(
        matched_frame, matched_frame["raw_probability"].to_numpy(), "pooled_raw_probability_control"
    )
    rows = []
    for name, opp in [("pooled_full_range", pooled), ("post_cutover_only", post)]:
        t = select(opp, (0.5, 0.01, 0.95, "both"))
        rows.append({"name": name, **metrics(t, matched_frame["market_id"].n_unique(), 23)})
        _write_parquet(t, output / "cutover-diagnostic" / f"{name}-trades.parquet")
    _write_json(
        output / "cutover-diagnostic" / "metrics.json",
        {
            "purpose": "descriptive transfer diagnostic, no selection into router; full-range models unchanged",
            "models": rows,
        },
    )
