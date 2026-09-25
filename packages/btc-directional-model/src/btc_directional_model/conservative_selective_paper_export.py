"""Export an unchanged conservative estimator for an exploratory paper process."""

from __future__ import annotations

import argparse
import hashlib
import re
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


def _validate_unqualified_policy(bundle: dict, quantity: int) -> None:
    if bundle.get("policy") is not None:
        raise ValueError("expected the diagnostic artifact without a qualified final policy")
    quantity_policies = bundle.get("quantity_policies")
    if quantity_policies is not None:
        policy = quantity_policies.get(str(quantity), quantity_policies.get(quantity))
        if policy is not None:
            raise ValueError(
                f"expected quantity {quantity} without a qualified final policy"
            )


def export_candidate(
    source: Path,
    panel: Path,
    output: Path,
    *,
    source_sha256: str,
    model_key: str,
    source_training_run: str,
    producing_commit: str,
    historical_qualification: str = "failed",
    policy: dict[str, float | int] | None = None,
    quantity: int = 5,
) -> Path:
    """Export one identified estimator under the shared conservative adapter."""
    if re.fullmatch(r"[0-9a-f]{64}", source_sha256) is None:
        raise ValueError("source SHA-256 must be a lowercase digest")
    if re.fullmatch(r"[a-z0-9](?:[a-z0-9-]{0,126}[a-z0-9])?", model_key) is None:
        raise ValueError("model key must contain only lowercase letters, digits, and hyphens")
    if file_sha256(source) != source_sha256:
        raise ValueError("conservative training artifact identity changed")
    bundle = joblib.load(source)
    if not isinstance(bundle, dict):
        raise TypeError("conservative training artifact must be a mapping")
    _validate_unqualified_policy(bundle, quantity)
    frozen_policy = dict(POLICY if policy is None else policy)
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
        "policy": frozen_policy,
    }
    payload = _base_model(model_key, FEATURE_SCHEMA, runtime_names, source_sha256, {
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
        source_training_run=source_training_run,
        producing_commit=producing_commit,
        historical_qualification=historical_qualification,
        paper_policy_basis="fixed exploratory policy; not selected or qualified by training",
        frozen_paper_policy=frozen_policy,
        qualified_trade_size=quantity,
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
        accepted = bool(
            cost is not None
            and 0 < cost <= frozen_policy["maximum_share_cost"]
            and conservative >= frozen_policy["minimum_confidence"]
            and conservative
            - cost
            - frozen_policy["execution_reserve_per_share"]
            - frozen_policy["stress_slippage_per_share"]
            >= frozen_policy["minimum_stressed_edge"]
        )
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
        "model_key": model_key,
        "feature_schema_sha256": payload["features"]["schema_sha256"],
        "vectors": vectors,
    })
    manifest = {
        "schema_version": "capitonic-btc-directional-runtime-manifest-v1",
        "model_key": model_key,
        "model_file": "model.json",
        "model_sha256": hashlib.sha256(model_bytes).hexdigest(),
        "golden_vectors_file": "golden-vectors.json",
        "golden_vectors_sha256": hashlib.sha256(golden_bytes).hexdigest(),
        "feature_schema_version": FEATURE_SCHEMA,
        "feature_schema_sha256": payload["features"]["schema_sha256"],
        "source_freeze_manifest_sha256": source_sha256,
        "source_training_model_sha256": source_sha256,
        "deployment_scope": "paper_only",
        "production_qualified": False,
        "live_capital_allowed": False,
    }
    destination = output / model_key
    write_immutable_directory(destination, {
        "model.json": model_bytes,
        "manifest.json": canonical_json_bytes(manifest),
        "golden-vectors.json": golden_bytes,
    })
    return destination


def export(source: Path, panel: Path, output: Path) -> Path:
    """Preserve the original deterministic paper export entry point."""
    return export_candidate(
        source,
        panel,
        output,
        source_sha256=SOURCE_SHA256,
        model_key=MODEL_KEY,
        source_training_run="20260917T171105Z",
        producing_commit="7e89a2169fee834ed342fc12392b3952642b98e6",
    )


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--source", required=True, type=Path)
    parser.add_argument("--panel", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--source-sha256", default=SOURCE_SHA256)
    parser.add_argument("--model-key", default=MODEL_KEY)
    parser.add_argument("--source-training-run", default="20260917T171105Z")
    parser.add_argument("--historical-qualification", default="failed")
    parser.add_argument(
        "--producing-commit",
        default="7e89a2169fee834ed342fc12392b3952642b98e6",
    )
    arguments = parser.parse_args()
    print(
        export_candidate(
            arguments.source,
            arguments.panel,
            arguments.output,
            source_sha256=arguments.source_sha256,
            model_key=arguments.model_key,
            source_training_run=arguments.source_training_run,
            producing_commit=arguments.producing_commit,
            historical_qualification=arguments.historical_qualification,
        )
    )
