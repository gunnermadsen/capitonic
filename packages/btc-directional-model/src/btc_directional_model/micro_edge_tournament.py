"""Resumable full-range model-first tournament; training-only entry point."""

from __future__ import annotations

import argparse
import gc
import json
import subprocess
import time
from dataclasses import asdict
from datetime import UTC, datetime, timedelta
from pathlib import Path

import joblib
import numpy as np
import polars as pl
from sklearn.metrics import brier_score_loss, log_loss
from threadpoolctl import threadpool_limits

from .core_extract import file_sha256
from .micro_edge_data import CACHE, END, START
from .micro_edge_evaluation import (
    apply_policies,
    fit_policies,
    metrics,
    opportunities,
)
from .micro_edge_models import (
    calibrate,
    feature_names,
    fit_calibrator,
    fit_estimator,
    fit_indices,
    market_weights,
    matrix,
    predict_estimator,
    recipes,
)
from .time_bucket_source_preparation import _write_json, _write_parquet

PACKAGE = Path(__file__).resolve().parents[2]
RUN = "20260915T030000Z"
OUTPUT = PACKAGE / "runs/btc-micro-edge-20260607-20260914" / RUN
OOF_START = datetime(2026, 6, 21, tzinfo=UTC)
EVAL_START = datetime(2026, 7, 5, tzinfo=UTC)


def dump(payload, path):
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_suffix(path.suffix + ".tmp")
    joblib.dump(payload, tmp, compress=3)
    tmp.replace(path)


def load_panel():
    manifest = json.loads((CACHE / "panel-manifest.json").read_text())
    for r in manifest["partitions"]:
        if file_sha256(Path(r["path"])) != r["sha256"]:
            raise RuntimeError("panel checkpoint corrupt")
    from .micro_edge_execution import enrich_execution

    execution_manifest = json.loads((CACHE / "omitted-execution-manifest.json").read_text())
    extras = {r["day"]: r for r in execution_manifest["partitions"]}
    daily = []
    for record in manifest["partitions"]:
        day = record["day"]
        extra = extras[day]
        if file_sha256(Path(extra["path"])) != extra["sha256"]:
            raise RuntimeError("omitted execution input mismatch")
        original = manifest["selected_raw_partitions"]["execution"][day]
        if file_sha256(Path(original["path"])) != original["sha256"]:
            raise RuntimeError("execution source input mismatch")
        books = pl.concat(
            [pl.read_parquet(original["path"]), pl.read_parquet(extra["path"])],
            how="diagonal_relaxed",
        )
        part = enrich_execution(
            pl.read_parquet(record["path"]), books, manifest["feature_groups"]["execution"]
        )
        daily.append(
            part.with_columns(
                *[pl.col(c).cast(pl.Float32) for c, t in part.schema.items() if t == pl.Float64]
            )
        )
    frame = pl.concat(daily, how="diagonal_relaxed").sort(
        ["window_start", "market_id", "seconds_elapsed"]
    )
    frame = frame.with_columns(
        *[pl.col(c).cast(pl.Float32) for c, t in frame.schema.items() if t == pl.Float64]
    )
    frame = frame.with_columns(
        (
            pl.col("btc_cross_venue_boundary_gap_bps")
            / (pl.col("btc_realized_volatility_60s_bps").abs() + 0.1)
        )
        .clip(-50, 50)
        .alias("normalized_boundary_gap"),
        (pl.col("twap60_margin_bps") / (pl.col("btc_realized_volatility_60s_bps").abs() + 0.1))
        .clip(-50, 50)
        .alias("normalized_twap_gap"),
    ).with_row_index("row_id")
    if frame.select("market_id", "observed_at").n_unique() != frame.height:
        raise RuntimeError("duplicate causal keys")
    if frame.filter(
        (pl.col("observed_at") < pl.col("window_start"))
        | (pl.col("observed_at") >= pl.col("window_end"))
    ).height:
        raise RuntimeError("invalid timestamp")
    return frame, manifest


def code_identity():
    return {
        p.name: file_sha256(p)
        for p in Path(__file__).parent.glob("micro_edge_*.py")
        if p.name not in ("micro_edge_reporting.py", "micro_edge_diagnostics.py")
    }


def identity_matches(saved, active):
    return saved == active or saved in active.get("compatible_prior_identities", [])


def raw_predict(model, x, cost, fee):
    p = predict_estimator(model, x)
    if model["kind"] == "payoff":
        p = p + cost + fee + 0.005
    return np.clip(p, 1e-5, 1 - 1e-5)


def base_oof(frame, name, output):
    paths = sorted((output / "checkpoints" / name).glob("*/predictions.parquet"))
    d = pl.concat([pl.read_parquet(p).select("row_id", "probability") for p in paths])
    joined = frame.select("row_id").join(
        d, on="row_id", how="left", validate="1:1", maintain_order="left"
    )
    return joined["probability"].to_numpy()


def run_model(recipe, frame, manifest, identity, output):
    folder = output / "checkpoints" / recipe.name
    features = feature_names(recipe, manifest["feature_groups"])
    baseline = recipe.baseline
    parameters = baseline["estimator_parameters"] if baseline else None
    if baseline:
        frame = frame.filter(
            pl.col("seconds_elapsed").is_between(baseline["start_second"], baseline["end_second"])
        )
    x = matrix(frame, features)
    basep = None
    if recipe.kind in ("loss", "wait"):
        basep = base_oof(
            frame, "all_dimensions" + ("__rtds" if recipe.rtds else "__no_rtds"), output
        )
        x = np.column_stack((x, basep))
        features = features + ["base_probability_oof"]
    y = frame["label_up"].to_numpy()
    cost = frame["up_ask_vwap_5"].to_numpy()
    fee = frame["fee_rate"].to_numpy() * cost * (1 - cost)
    target = y.astype(float)
    kind = recipe.kind
    if kind == "normalized":
        kind = "tree"
    if recipe.kind == "payoff":
        target = y - cost - fee - 0.005
    if recipe.kind == "wait":
        # Supervision uses later executable prices only within this ten-second bucket.
        up = np.where(np.isfinite(basep), basep >= 0.5, True)
        current = np.where(up, cost, frame["down_ask_vwap_5"].to_numpy())
        next_up = np.r_[cost[1:], np.nan]
        next_down = np.r_[frame["down_ask_vwap_5"].to_numpy()[1:], np.nan]
        nextcost = np.where(up, next_up, next_down)
        same = np.r_[
            (frame["market_id"].to_numpy()[:-1] == frame["market_id"].to_numpy()[1:])
            & (
                frame["seconds_elapsed"].to_numpy()[:-1] // 10
                == frame["seconds_elapsed"].to_numpy()[1:] // 10
            ),
            False,
        ]
        # Buying now beats waiting when next cost (including fee) is higher.
        rate = frame["fee_rate"].to_numpy()
        target = (
            nextcost + rate * nextcost * (1 - nextcost) - current - rate * current * (1 - current)
        )
        target = np.where(same, target, 0.0)
        target = np.where(
            np.isfinite(current) & np.isfinite(rate) & np.isfinite(basep), target, np.nan
        )
    loss_weights = np.ones(len(frame))
    if recipe.kind == "loss":
        chosen_cost = np.where(basep >= 0.5, cost, frame["down_ask_vwap_5"].to_numpy())
        wrong = (basep >= 0.5) != (y == 1)
        loss_weights += np.where(
            np.isfinite(basep) & np.isfinite(chosen_cost) & wrong, chosen_cost, 0.0
        )
    windows = []
    boundary = OOF_START
    while boundary < END:
        windows.append((boundary, min(boundary + timedelta(days=7), END)))
        boundary += timedelta(days=7)
    history_predictions = []
    trades = []
    all_opps = []
    fold_metrics = []
    start_clock = time.monotonic()
    for start, end in windows:
        part = folder / start.strftime("%Y%m%d")
        cp = part / "model.joblib"
        pp = part / "predictions.parquet"
        sidecar = part / "completion.json"
        train = fit_indices(frame, start)
        train = train[np.isfinite(target[train])]
        test = np.flatnonzero(
            (frame["window_start"].to_numpy() >= np.datetime64(start.replace(tzinfo=None), "us"))
            & (frame["window_start"].to_numpy() < np.datetime64(end.replace(tzinfo=None), "us"))
        )
        if not len(test):
            continue
        if cp.exists() and pp.exists() and sidecar.exists():
            saved = json.loads(sidecar.read_text())
            if (
                not identity_matches(saved["identity"], identity)
                or file_sha256(cp) != saved["model_sha256"]
                or file_sha256(pp) != saved["prediction_sha256"]
            ):
                raise RuntimeError("checkpoint identity mismatch " + str(part))
            predictions = pl.read_parquet(pp)
            model = joblib.load(cp)
        else:
            weights = market_weights(frame.select("market_id")[train]) * loss_weights[train]
            model = fit_estimator(
                x[train],
                target[train],
                weights,
                "tree" if recipe.kind == "loss" else kind,
                parameters=parameters,
            )
            raw = (
                raw_predict(model, x[test], cost[test], fee[test])
                if recipe.kind != "wait"
                else basep[test]
            )
            c = None
            if history_predictions and recipe.kind != "wait":
                previous = pl.concat(history_predictions).filter(
                    pl.col("window_end") + pl.duration(minutes=5) < start
                )
                c = fit_calibrator(
                    previous["raw_probability"].to_numpy(),
                    previous["label_up"].to_numpy(),
                    previous["settlement_regime"].to_numpy()
                    if recipe.kind == "normalized"
                    else None,
                )
            prob = (
                calibrate(raw, c, frame["settlement_regime"][test].to_numpy())
                if recipe.kind != "wait"
                else raw
            )
            gate = (
                predict_estimator(model, x[test]) >= 0
                if recipe.kind == "wait"
                else np.ones(len(test), dtype=bool)
            )
            predictions = frame.select(
                "row_id", "window_start", "window_end", "label_up", "settlement_regime"
            )[test].with_columns(
                pl.Series("raw_probability", raw),
                pl.Series("probability", prob),
                pl.Series("gate", gate),
            )
            dump(model, cp)
            _write_parquet(predictions, pp)
            _write_json(
                sidecar,
                {
                    "identity": identity,
                    "model_sha256": file_sha256(cp),
                    "prediction_sha256": file_sha256(pp),
                    "train_rows": len(train),
                    "train_markets": frame["market_id"][train].n_unique(),
                    "train_start": str(frame["window_start"][train].min()),
                    "train_last_end": str(frame["window_end"][train].max()),
                    "evaluation_start": start.isoformat(),
                    "evaluation_end": end.isoformat(),
                },
            )
        opp = opportunities(
            frame.select(
                "market_id",
                "window_start",
                "window_end",
                "observed_at",
                "seconds_elapsed",
                "label_up",
                "settlement_regime",
                "up_ask_vwap_5",
                "down_ask_vwap_5",
                "pm_up_book_age_seconds",
                "pm_down_book_age_seconds",
                "fee_rate",
            )[test],
            predictions["probability"].to_numpy(),
            recipe.name,
            predictions["gate"].to_numpy(),
        )
        if start >= EVAL_START:
            hist = (
                pl.concat(all_opps).filter(pl.col("window_end") + pl.duration(minutes=5) < start)
                if all_opps
                else opp.head(0)
            )
            policies = fit_policies(hist)
            selected = apply_policies(opp, policies)
            # Preserve bucket-independent opportunities for honest subsequent router evaluation.
            independent = apply_policies(opp, policies, True).with_columns(
                pl.lit(start.isoformat()).alias("fold")
            )
            _write_parquet(independent, part / "admitted-buckets.parquet")
            selected = selected.with_columns(pl.lit(start.isoformat()).alias("fold"))
            trades.append(selected)
            m = metrics(
                selected,
                frame["market_id"][test].n_unique(),
                (end - start).total_seconds() / 86400,
                opp["market_id"].n_unique(),
            )
            finite = np.isfinite(predictions["probability"].to_numpy())
            m.update(
                {
                    "fold": start.isoformat(),
                    "policies": policies,
                    "brier": float(
                        brier_score_loss(
                            y[test][finite], predictions["probability"].to_numpy()[finite]
                        )
                    )
                    if finite.any()
                    else None,
                    "log_loss": float(
                        log_loss(
                            y[test][finite],
                            np.clip(predictions["probability"].to_numpy()[finite], 1e-5, 1 - 1e-5),
                            labels=[0, 1],
                        )
                    )
                    if finite.any()
                    else None,
                }
            )
            fold_metrics.append(m)
        all_opps.append(opp)
        history_predictions.append(predictions)
        print(
            recipe.name,
            start.date(),
            "train",
            len(train),
            "test",
            len(test),
            "elapsed",
            round(time.monotonic() - start_clock, 1),
            flush=True,
        )
    historical = pl.concat(all_opps)
    predictions = pl.concat(history_predictions)
    ledger = pl.concat(trades) if trades else historical.head(0)
    _write_parquet(historical, output / "opportunities" / f"{recipe.name}.parquet")
    _write_parquet(ledger, output / "trades" / f"{recipe.name}.parquet")
    _write_parquet(predictions, output / "predictions" / f"{recipe.name}.parquet")
    _write_json(
        output / "metrics" / f"{recipe.name}.json",
        {
            "recipe": asdict(recipe),
            "folds": fold_metrics,
            "elapsed_seconds": time.monotonic() - start_clock,
        },
    )
    # Refit on the entire resolved agreed range. Past OOF evidence calibrates final predictions.
    final = output / "models" / f"{recipe.name}.joblib"
    finalmeta = final.with_suffix(".json")
    if final.exists() and finalmeta.exists():
        saved = json.loads(finalmeta.read_text())
        if (
            not identity_matches(saved["source_identity"], identity)
            or file_sha256(final) != saved["model_artifact_sha256"]
        ):
            raise RuntimeError("final artifact checkpoint mismatch " + recipe.name)
    else:
        ix = np.flatnonzero(np.isfinite(target))
        weights = market_weights(frame.select("market_id")[ix]) * loss_weights[ix]
        estimator = fit_estimator(
            x[ix],
            target[ix],
            weights,
            "tree" if recipe.kind == "loss" else kind,
            parameters=parameters,
        )
        c = (
            fit_calibrator(
                predictions["raw_probability"].to_numpy(),
                predictions["label_up"].to_numpy(),
                predictions["settlement_regime"].to_numpy()
                if recipe.kind == "normalized"
                else None,
            )
            if recipe.kind != "wait"
            else None
        )
        policies = fit_policies(historical)
        payload = {
            "recipe": asdict(recipe),
            "feature_names": features,
            "estimator": estimator,
            "calibrator": c,
            "bucket_policies": policies,
            "quantity": 5,
            "source_identity": identity,
            "training_start": START.isoformat(),
            "training_end_exclusive": END.isoformat(),
            "base_model": ("all_dimensions" + ("__rtds" if recipe.rtds else "__no_rtds"))
            if basep is not None
            else None,
        }
        dump(payload, final)
        # Reload reference predictions to verify serialization parity.
        loaded = joblib.load(final)
        sample = x[ix[-100:]]
        np.testing.assert_allclose(
            predict_estimator(estimator, sample),
            predict_estimator(loaded["estimator"], sample),
            rtol=0,
            atol=0,
        )
        _write_json(
            finalmeta,
            {
                "artifact_path": str(final),
                "model_artifact_sha256": file_sha256(final),
                "producing_commit": identity["source_commit"],
                "training_run_id": RUN,
                "source_identity": identity,
                "qualification_status": "trained_evaluated_training_only",
                "deployment_status": "not_deployed",
                "fit_rows": len(ix),
                "fit_markets": frame["market_id"][ix].n_unique(),
                "fit_first": str(frame["window_start"][ix].min()),
                "fit_last": str(frame["window_start"][ix].max()),
                "bucket_allowlist": sorted(policies),
                "rtds": recipe.rtds,
                "serialized_prediction_parity": True,
            },
        )
    del x
    gc.collect()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--models", nargs="*")
    parser.add_argument("--report-only", action="store_true")
    args = parser.parse_args()
    OUTPUT.mkdir(parents=True, exist_ok=True)
    frame, manifest = load_panel()
    identity = {
        "code": {
            **code_identity(),
            "baseline_registry": file_sha256(PACKAGE / "config/btc-micro-edge-baselines.json"),
        },
        "panel_manifest": file_sha256(CACHE / "panel-manifest.json"),
        "execution_manifest": file_sha256(CACHE / "omitted-execution-manifest.json"),
        "source_commit": subprocess.check_output(
            ["git", "rev-parse", "HEAD"], cwd=PACKAGE, text=True
        ).strip(),
    }
    frozen = OUTPUT / "run-identity.json"
    if frozen.exists():
        old = json.loads(frozen.read_text())
        # Result-only commits do not invalidate unchanged fitting/data code.
        if (
            old["code"] != identity["code"]
            or old["panel_manifest"] != identity["panel_manifest"]
            or old["execution_manifest"] != identity["execution_manifest"]
        ):
            raise RuntimeError("run inputs changed; checkpoint reuse rejected")
        identity = old
    else:
        _write_json(frozen, identity)
    selected = [r for r in recipes() if not args.models or r.name in args.models]
    _write_json(OUTPUT / "candidate-registry.json", [asdict(r) for r in recipes()])
    if not args.report_only:
        with threadpool_limits(limits=2):
            for recipe in selected:
                run_model(recipe, frame, manifest, identity, OUTPUT)
    if all((OUTPUT / "models" / f"{r.name}.joblib").exists() for r in recipes()):
        from .micro_edge_reporting import report

        report(frame, OUTPUT, identity)


if __name__ == "__main__":
    main()
