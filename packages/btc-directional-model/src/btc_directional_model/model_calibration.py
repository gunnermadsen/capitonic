"""Offline, frozen-predictor probability correction and chronological qualification.

No database writes, process updates, release actions, or runtime code changes.
"""

from __future__ import annotations

import argparse
import copy
import hashlib
import json
from pathlib import Path

import numpy as np
import polars as pl
from scipy.optimize import minimize_scalar
from scipy.special import expit, logit

from .runtime_export import canonical_json_bytes, write_immutable_directory


def corrected(probability: np.ndarray, slope: float, kind: str) -> np.ndarray:
    p = np.clip(np.asarray(probability, dtype=float), 1e-6, 1 - 1e-6)
    value = logit(p) if kind == "logit_temperature" else p - 0.5
    return expit(slope * value)


def fit_correction(probability: np.ndarray, outcome: np.ndarray, kind: str) -> float:
    """One positive parameter preserves direction and limits calibration capacity."""
    p = np.asarray(probability, dtype=float)
    y = np.asarray(outcome, dtype=float)
    if kind not in {"logit_temperature", "probability_sigmoid"}:
        raise ValueError("Unsupported correction")
    if len(p) != len(y) or len(p) < 50 or not np.isfinite(p).all():
        raise ValueError("At least 50 finite, independent market forecasts are required")
    if ((p <= 0) | (p >= 1)).any() or not np.isin(y, [0, 1]).all():
        raise ValueError("Invalid probability or official outcome")
    if len(np.unique(y)) != 2:
        raise ValueError("Calibration requires both official outcomes")

    def loss(slope: float) -> float:
        value = logit(p) if kind == "logit_temperature" else p - 0.5
        z = slope * value
        return float(np.mean(np.logaddexp(0.0, z) - y * z))

    result = minimize_scalar(loss, bounds=(0.05, 20.0), method="bounded")
    if not result.success:
        raise ValueError("Calibration optimizer failed")
    return float(result.x)


def independent_markets(frame: pl.DataFrame) -> pl.DataFrame:
    """One earliest forecast per market; duplicate processes are not extra samples."""
    required = {"market_id", "window_start", "feature_as_of", "probability_up", "outcome_up"}
    if not required.issubset(frame.columns):
        raise ValueError(f"Missing evidence columns: {sorted(required - set(frame.columns))}")
    for column in ("window_start", "feature_as_of"):
        if frame.schema[column] == pl.String:
            frame = frame.with_columns(pl.col(column).str.to_datetime(time_zone="UTC"))
    frame = frame.with_columns(pl.col("probability_up").cast(pl.Float64))
    if (
        frame.group_by("market_id")
        .agg(pl.col("outcome_up").n_unique())
        .filter(pl.col("outcome_up") != 1)
        .height
    ):
        raise ValueError("Conflicting official outcomes")
    return frame.sort(["feature_as_of", "market_id"]).unique("market_id", keep="first")


def quality(p: np.ndarray, y: np.ndarray) -> dict:
    p = np.clip(p, 1e-6, 1 - 1e-6)
    side = p >= 0.5
    confidence = np.maximum(p, 1 - p)
    actual = side == y.astype(bool)
    return {
        "markets": len(p),
        "brier": float(np.mean((p - y) ** 2)),
        "log_loss": float(np.mean(-y * np.log(p) - (1 - y) * np.log1p(-p))),
        "confidence": float(confidence.mean()),
        "accuracy": float(actual.mean()),
        "confidence_gap": float(confidence.mean() - actual.mean()),
    }


def qualify(p: np.ndarray, y: np.ndarray, q: np.ndarray) -> dict:
    """Paired market bootstrap; economic/runtime qualification remains separate."""
    before, after = quality(p, y), quality(q, y)
    delta = (q - y) ** 2 - (p - y) ** 2
    rng = np.random.default_rng(20261003)
    samples = np.array([rng.choice(delta, len(delta), replace=True).mean() for _ in range(2000)])
    interval = np.quantile(samples, [0.025, 0.975]).tolist()
    passed = (
        len(p) >= 50
        and interval[1] < 0
        and after["log_loss"] < before["log_loss"]
        and abs(after["confidence_gap"]) < abs(before["confidence_gap"])
    )
    return {
        "before": before,
        "after": after,
        "paired_brier_delta_interval": interval,
        "probability_passed": bool(passed),
        "deployment_qualified": False,
        "limitation": "Market bootstrap does not establish independence across nearby markets; "
        "policy replay and runtime parity must also pass before deployment.",
    }


def calibrated_payload(source: dict, key: str, slope: float, kind: str, provenance: dict) -> dict:
    """Embed a correction using existing math, preserving tree splits and old artifacts."""
    result = copy.deepcopy(source)
    definition = result["payoff_model"]["definition"]
    if kind == "logit_temperature":
        calibration = definition["calibration"]
        calibration["slope"] *= slope
        calibration["intercept"] *= slope
    elif kind == "probability_sigmoid":
        outcome = definition["outcome"]
        if outcome["output"] != "regression" or definition.get("temporal"):
            raise ValueError(
                "Probability sigmoid requires a frozen regression outcome without temporal heads"
            )
        if definition.get("admission"):
            raise ValueError(
                "Learned admission requires complete derived-feature replay before calibration export"
            )
        outcome["baseline"] = slope * (outcome["baseline"] - 0.5)
        for tree in outcome["trees"]:
            for node in tree["nodes"]:
                if node["kind"] == "leaf":
                    node["value"] *= slope
        outcome["output"] = "probability"
    else:
        raise ValueError("Unsupported correction")
    result["model_key"] = key
    result["provenance"]["calibration"] = dict(
        **provenance, method=kind, slope=slope, predictor_retrained=False
    )
    return result


def run(args: argparse.Namespace) -> dict:
    source_bytes = args.model.read_bytes()
    source = json.loads(source_bytes)
    holdout_source = pl.read_parquet(args.holdout)
    if "model_sha256" not in holdout_source.columns or set(
        holdout_source["model_sha256"].drop_nulls()
    ) != {hashlib.sha256(source_bytes).hexdigest()}:
        raise ValueError("Holdout must pin the exact source runtime model hash")
    fit = independent_markets(pl.read_parquet(args.fit))
    holdout = independent_markets(holdout_source)
    if fit["window_start"].max() >= holdout["window_start"].min():
        raise ValueError("Fit and holdout must be strictly chronological and market-disjoint")
    if set(fit["market_id"]) & set(holdout["market_id"]):
        raise ValueError("Market leakage between fitting and holdout")
    p = fit["probability_up"].to_numpy()
    y = fit["outcome_up"].cast(pl.Int8).to_numpy()
    slope = fit_correction(p, y, args.method)
    hp = holdout["probability_up"].to_numpy()
    hy = holdout["outcome_up"].cast(pl.Int8).to_numpy()
    result = qualify(hp, hy, corrected(hp, slope, args.method))
    provenance = {
        "source_model_key": source["model_key"],
        "source_model_sha256": hashlib.sha256(source_bytes).hexdigest(),
        "fit_sha256": hashlib.sha256(args.fit.read_bytes()).hexdigest(),
        "holdout_sha256": hashlib.sha256(args.holdout.read_bytes()).hexdigest(),
        "producing_commit": args.commit,
        "fit_markets": fit.height,
        "holdout_markets": holdout.height,
        "qualification": result,
    }
    payload = calibrated_payload(source, args.model_key, slope, args.method, provenance)
    # Diagnostic bundles are not admitted to the runtime catalog automatically.
    write_immutable_directory(
        args.output / args.model_key,
        {
            "model.json": canonical_json_bytes(payload),
            "calibration-result.json": canonical_json_bytes(dict(**provenance, slope=slope)),
        },
    )
    return dict(model_key=args.model_key, slope=slope, **result)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("model", "fit", "holdout", "output"):
        parser.add_argument(f"--{name}", type=Path, required=True)
    parser.add_argument("--model-key", required=True)
    parser.add_argument("--commit", required=True)
    parser.add_argument(
        "--method", choices=["logit_temperature", "probability_sigmoid"], required=True
    )
    args = parser.parse_args()
    canonical = Path("/Volumes/docker-data/capitonic-btc-directional-model").resolve()
    if not canonical.is_dir() or not args.output.resolve().is_relative_to(canonical):
        raise ValueError("Calibration output must be inside the available canonical SSD root")
    print(json.dumps(run(args), indent=2))


if __name__ == "__main__":
    main()
