"""Export the unchanged conservative estimator for an explicitly exploratory paper process."""

from __future__ import annotations

import argparse
import hashlib
from pathlib import Path

import joblib
import numpy as np
import polars as pl

from .core_extract import file_sha256
from .payoff_runtime_export import _base_model, _histogram
from .router_bucket_export import contract as bucket_contract
from .runtime_export import canonical_json_bytes, write_immutable_directory

SOURCE_SHA256 = "88744a1cc1fb0af824a902212906944ea7873433e672111d38a740afc3e01481"
MODEL_KEY = "btc-5m-conservative-selective-paper-20260917"
FEATURE_SCHEMA = "btc-5m-payoff-aware-conservative-price-v1"
POLICY = {
    "start_second": 30,
    "end_second": 210,
    "minimum_confidence": 0.88,
    "minimum_stressed_edge": 0.10,
    "maximum_share_cost": 0.55,
    "execution_reserve_per_share": 0.005,
    "stress_slippage_per_share": 0.010,
}


def export(source: Path, panel: Path, output: Path) -> Path:
    if file_sha256(source) != SOURCE_SHA256:
        raise ValueError("conservative training artifact identity changed")
    bundle = joblib.load(source)
    if bundle["policy"] is not None:
        raise ValueError("expected the diagnostic artifact without a qualified final policy")
    names = list(bundle["features"])
    runtime_names = names + ["up_ask_vwap_5", "down_ask_vwap_5"]
    contract = bucket_contract(names)
    contract["adapter"] = "conservative_selective"
    calibrator = bundle["calibrator"]
    penalty = max(float(bundle["calibration_ece"]), 0.02)
    definition = {
        "contract": contract,
        "outcome": _histogram(bundle["estimator"], names, runtime_names, "probability"),
        "calibration": {
            "slope": float(calibrator.coef_[0, 0]),
            "intercept": float(calibrator.intercept_[0]),
            "reliability_penalty": penalty,
        },
        "policy": POLICY,
    }
    payload = _base_model(MODEL_KEY, FEATURE_SCHEMA, runtime_names, SOURCE_SHA256, {
        "kind": "unified",
        "definition": definition,
        "prediction_policy": {
            "type": "first_confidence_crossing",
            "minimum_seconds_after_open": 30,
            "maximum_seconds_after_open": 210,
            "cadence_seconds": 5,
            "early_end_second": None,
            "early_cadence_seconds": None,
            "late_start_second": None,
        },
    })
    payload["provenance"].update(
        source_training_run="20260917T171105Z",
        producing_commit="7e89a2169fee834ed342fc12392b3952642b98e6",
        historical_qualification="failed",
        paper_policy_basis="fixed exploratory policy; not selected or qualified by training",
        frozen_paper_policy=POLICY,
        export_is_training=False,
    )
    sample = (
        pl.scan_parquet(panel)
        .filter(pl.col("seconds_elapsed").is_between(30, 210)
                & pl.col("up_ask_vwap_5").is_not_null()
                & pl.col("down_ask_vwap_5").is_not_null())
        .select(["seconds_elapsed", *runtime_names])
        .head(256)
        .collect()
    )
    if sample.height < 16:
        raise ValueError("insufficient authentic execution reference rows")
    raw = np.clip(bundle["estimator"].predict_proba(sample.select(names).to_numpy())[:, 1], 1e-6, 1 - 1e-6)
    probabilities = calibrator.predict_proba(np.log(raw / (1 - raw)).reshape(-1, 1))[:, 1]
    vectors = []
    for index, (row, probability) in enumerate(zip(sample.to_dicts(), probabilities, strict=True)):
        p = float(probability)
        side = "up" if p >= 0.5 else "down"
        confidence = max(p, 1 - p)
        conservative = max(0.5, confidence - penalty)
        cost = row[f"{side}_ask_vwap_5"]
        accepted = bool(cost is not None and 0 < cost <= POLICY["maximum_share_cost"]
                        and conservative >= POLICY["minimum_confidence"]
                        and conservative - cost - 0.015 >= POLICY["minimum_stressed_edge"])
        vectors.append({
            "id": f"conservative-{index}",
            "seconds_elapsed": int(row["seconds_elapsed"]),
            "source": {"accepted": accepted},
            "feature_values": [float(row[name]) if row[name] is not None and np.isfinite(row[name]) else None for name in runtime_names],
            "expected": {
                "probability_up": p,
                "confidence": confidence,
                "raw_logit": float(np.log(p / (1 - p))),
                "action": side if accepted else "no_trade",
            },
        })
    model_bytes = canonical_json_bytes(payload)
    golden_bytes = canonical_json_bytes({
        "schema_version": "capitonic-btc-payoff-aware-golden-vectors-v1",
        "model_key": MODEL_KEY,
        "feature_schema_sha256": payload["features"]["schema_sha256"],
        "vectors": vectors,
    })
    manifest = {
        "schema_version": "capitonic-btc-directional-runtime-manifest-v1",
        "model_key": MODEL_KEY,
        "model_file": "model.json",
        "model_sha256": hashlib.sha256(model_bytes).hexdigest(),
        "golden_vectors_file": "golden-vectors.json",
        "golden_vectors_sha256": hashlib.sha256(golden_bytes).hexdigest(),
        "feature_schema_version": FEATURE_SCHEMA,
        "feature_schema_sha256": payload["features"]["schema_sha256"],
        "source_freeze_manifest_sha256": SOURCE_SHA256,
        "source_training_model_sha256": SOURCE_SHA256,
        "deployment_scope": "paper_only",
        "production_qualified": False,
        "live_capital_allowed": False,
    }
    destination = output / MODEL_KEY
    write_immutable_directory(destination, {
        "model.json": model_bytes,
        "manifest.json": canonical_json_bytes(manifest),
        "golden-vectors.json": golden_bytes,
    })
    return destination


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--source", required=True, type=Path)
    parser.add_argument("--panel", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    arguments = parser.parse_args()
    print(export(arguments.source, arguments.panel, arguments.output))
