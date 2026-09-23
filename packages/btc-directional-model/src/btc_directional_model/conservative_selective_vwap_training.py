"""Clean locked-model retraining with independent VWAP-capacity replay."""

from __future__ import annotations

import argparse
import json
import platform
import subprocess
import tomllib
from dataclasses import asdict
from datetime import timedelta
from pathlib import Path

import joblib
import numpy as np
import polars as pl
import sklearn

from .conservative_selective_training import (
    KEYS,
    PRICE_FEATURES,
    VOLUME_FEATURES,
    fit_model,
    parse,
    probability_metrics,
    qualification,
    replay,
    score,
    select_policy,
    sha256,
    trade_metrics,
)

SCHEMA_VERSION = "btc-conservative-selective-vwap-capacity-training-v1"


def _git() -> dict[str, object]:
    return {
        "branch": subprocess.check_output(["git", "branch", "--show-current"], text=True).strip(),
        "commit": subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip(),
        "dirty": bool(subprocess.check_output(["git", "status", "--porcelain"], text=True).strip()),
    }


def run(config: Path, output: Path) -> None:
    raw = tomllib.loads(config.read_text())
    training = raw["training"]
    if (
        not training["training_only"]
        or not training["paper_only"]
        or training["live_capital_allowed"]
    ):
        raise RuntimeError("run must remain offline, training-only and paper-only")
    if len(raw["models"]) != 1 or raw["models"][0]["name"] != training["locked_model_name"]:
        raise RuntimeError("exactly one locked prior model specification is required")
    quantities = tuple(raw["execution"]["vwap_quantities"])
    panel = Path(raw["source"]["panel"])
    vwap_columns = tuple(
        column
        for quantity in quantities
        for column in (f"up_ask_vwap_{quantity}", f"down_ask_vwap_{quantity}")
    )
    columns = list(dict.fromkeys((*KEYS, *vwap_columns, *PRICE_FEATURES, *VOLUME_FEATURES)))
    frame = pl.read_parquet(panel, columns=columns).with_columns(
        pl.col(column).cast(pl.Float64).fill_nan(None)
        for column in (*PRICE_FEATURES, *VOLUME_FEATURES, *vwap_columns)
    )
    source = raw["source"]
    audit = {
        "panel": str(panel),
        "sha256": sha256(panel),
        "rows": frame.height,
        "markets": frame["market_id"].n_unique(),
        "range_start": frame["window_start"].min(),
        "range_end": frame["window_start"].max(),
        "vwap_coverage": {
            str(quantity): {
                "rows": frame.drop_nulls(
                    [f"up_ask_vwap_{quantity}", f"down_ask_vwap_{quantity}"]
                ).height,
                "markets": frame.drop_nulls(
                    [f"up_ask_vwap_{quantity}", f"down_ask_vwap_{quantity}"]
                )["market_id"].n_unique(),
            }
            for quantity in quantities
        },
        "predictive_features": list(PRICE_FEATURES),
        "vwap_role": "execution_economics_only",
    }
    if audit["sha256"] != source["panel_sha256"]:
        raise RuntimeError("training panel identity does not match the frozen config")
    if (
        audit["range_start"] < parse(source["source_start"])
        or frame.filter(pl.col("window_start") >= parse(source["source_end"])).height
    ):
        raise RuntimeError(f"panel coverage is outside the frozen source interval: {audit}")

    output.mkdir(parents=True, exist_ok=True)
    for directory in ("models", "predictions", "trades", "metrics", "manifests"):
        (output / directory).mkdir(exist_ok=True)
    (output / "manifests" / "readiness.json").write_text(
        json.dumps(audit, indent=2, sort_keys=True, default=str) + "\n"
    )

    source_start = parse(source["source_start"])
    source_end = parse(source["source_end"])
    calibration_days = training["calibration_days"]
    selection_days = training["selection_days"]
    test_days = training["test_days"]
    cursor = source_start + timedelta(
        days=training["minimum_fit_days"] + calibration_days + selection_days
    )
    starts = []
    while cursor < source_end:
        starts.append(cursor)
        cursor += timedelta(days=test_days)

    spec = raw["models"][0]
    seed = training["random_seed"]
    predictions = []
    trades: dict[int, list[pl.DataFrame]] = {quantity: [] for quantity in quantities}
    folds = []
    for index, test_start in enumerate(starts):
        test_end = min(test_start + timedelta(days=test_days), source_end)
        fit_end = test_start - timedelta(days=calibration_days + selection_days)
        calibration_end = test_start - timedelta(days=selection_days)
        fit = frame.filter(
            (pl.col("window_start") >= source_start) & (pl.col("window_start") < fit_end)
        )
        calibration = frame.filter(
            (pl.col("window_start") >= fit_end) & (pl.col("window_start") < calibration_end)
        )
        selection = frame.filter(
            (pl.col("window_start") >= calibration_end) & (pl.col("window_start") < test_start)
        )
        test = frame.filter(
            (pl.col("window_start") >= test_start) & (pl.col("window_start") < test_end)
        )
        model = fit_model(fit, calibration, spec, raw, seed)
        scored_selection = score(selection, model, raw["calibration"]["safety_floor"])
        scored_test = score(test, model, raw["calibration"]["safety_floor"]).with_columns(
            pl.lit(index).alias("fold")
        )
        predictions.append(scored_test)
        policies = {}
        for quantity in quantities:
            policy, search = select_policy(scored_selection, raw, quantity)
            policies[str(quantity)] = {
                "selected": asdict(policy) if policy else None,
                "search_top": search[:5],
            }
            if policy:
                executed = replay(
                    scored_test,
                    policy,
                    raw["policy"]["execution_reserve_per_share"],
                    raw["policy"]["stress_slippage_per_share"],
                    quantity,
                )
                if not executed.is_empty():
                    trades[quantity].append(executed.with_columns(pl.lit(index).alias("fold")))
        folds.append(
            {
                "fold": index,
                "fit_end": fit_end,
                "calibration_end": calibration_end,
                "test_start": test_start,
                "test_end": test_end,
                "calibration_ece": model[3],
                "quantity_policies": policies,
            }
        )

    out_of_sample = pl.concat(predictions)
    out_of_sample.write_parquet(output / "predictions" / "walk-forward.parquet", compression="zstd")
    quantity_results = {}
    for quantity in quantities:
        executed = pl.concat(trades[quantity]) if trades[quantity] else pl.DataFrame()
        if not executed.is_empty():
            executed.write_parquet(
                output / "trades" / f"vwap-{quantity}.parquet", compression="zstd"
            )
        metrics = {
            "quantity": quantity,
            "probability": probability_metrics(out_of_sample, raw),
            "trades": trade_metrics(executed),
        }
        metrics["qualification"] = qualification(metrics, raw)
        quantity_results[str(quantity)] = metrics

    final_fit_end = source_end - timedelta(days=calibration_days + selection_days)
    final_calibration_end = source_end - timedelta(days=selection_days)
    final_fit = frame.filter(pl.col("window_start") < final_fit_end)
    final_calibration = frame.filter(
        (pl.col("window_start") >= final_fit_end) & (pl.col("window_start") < final_calibration_end)
    )
    final_selection = frame.filter(
        (pl.col("window_start") >= final_calibration_end) & (pl.col("window_start") < source_end)
    )
    final_model = fit_model(final_fit, final_calibration, spec, raw, seed)
    scored_final_selection = score(final_selection, final_model, raw["calibration"]["safety_floor"])
    final_policies = {
        str(quantity): select_policy(scored_final_selection, raw, quantity)[0]
        for quantity in quantities
    }
    bundle = {
        "schema_version": SCHEMA_VERSION,
        "spec": spec,
        "features": final_model[2],
        "estimator": final_model[0],
        "calibrator": final_model[1],
        "calibration_ece": final_model[3],
        "quantity_policies": final_policies,
        "source_sha256": audit["sha256"],
    }
    model_path = output / "models" / "locked-model.joblib"
    joblib.dump(bundle, model_path, compress=3)
    results = {
        "schema_version": SCHEMA_VERSION,
        "status": "completed_not_deployment_qualified",
        "clean_run": True,
        "prior_results_used_for_training_or_selection": False,
        "model_parameter_lock": {
            "baseline_run": training["baseline_run"],
            "baseline_model_sha256": training["baseline_model_sha256"],
            "locked_spec": spec,
            "model": raw["model"],
            "calibration": raw["calibration"],
            "policy": raw["policy"],
            "training_windows": {
                key: training[key]
                for key in (
                    "minimum_fit_days",
                    "calibration_days",
                    "selection_days",
                    "test_days",
                )
            },
            "seeds": [seed, *training["stability_seeds"][1:]],
        },
        "readiness": audit,
        "folds": folds,
        "walk_forward_probability": probability_metrics(out_of_sample, raw),
        "quantities": quantity_results,
        "final_fit": {
            "fit_end": final_fit_end,
            "calibration_end": final_calibration_end,
            "selection_end": source_end,
            "policies": {
                quantity: asdict(policy) if policy else None
                for quantity, policy in final_policies.items()
            },
        },
        "git": _git(),
        "runtime": {
            "python": platform.python_version(),
            "numpy": np.__version__,
            "polars": pl.__version__,
            "sklearn": sklearn.__version__,
        },
        "config_sha256": sha256(config),
        "model_artifact": model_path.name,
        "model_sha256": sha256(model_path),
    }
    (output / "metrics" / "results.json").write_text(
        json.dumps(results, indent=2, sort_keys=True, default=str) + "\n"
    )
    (output / "README.md").write_text(
        "# Conservative selective VWAP-capacity training\n\n"
        f"Run commit: `{results['git']['commit']}` on `{results['git']['branch']}`.\n\n"
        f"Source panel: `{panel}` (`{audit['sha256']}`).\n\n"
        "`inputs/` contains the read-only daily source extracts; `datasets/` the combined "
        "training panel; `models/` the locked-spec diagnostic bundle; `predictions/` the "
        "walk-forward predictions; `trades/` quantity-specific replays; `metrics/` results; "
        "`manifests/` source/readiness identities; `diagnostics/` and `logs/` are reserved "
        "for run diagnostics. VWAP is execution economics only and is not a predictive feature.\n"
    )


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    arguments = parser.parse_args()
    run(arguments.config, arguments.output)
