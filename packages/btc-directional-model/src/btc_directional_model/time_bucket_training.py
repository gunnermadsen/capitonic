"""Explicit complete-case fold fitting through the maintained selective trainer."""

from __future__ import annotations

import json
from datetime import UTC, datetime
from pathlib import Path
from time import monotonic

import joblib
import numpy as np
import polars as pl
from sklearn.ensemble import HistGradientBoostingRegressor

from .conservative_selective_training import fit_model, score, weights
from .time_bucket_candidates import complete_cases, derived_features, reference_features
from .time_bucket_policy import choose_policy
from .time_bucket_protocol import config, split_rows
from .time_bucket_source_audit import sha256, write_json

IDENTITY = ["point_id", "market_id", "window_start", "window_end", "decision_at", "seconds_elapsed",
            "bucket_start", "entry_offset", "role", "role_fold", "official_outcome", "label_up"]
DECISION = [f"{s}_{n}" for s in ["up", "down"] for n in [
    "decision_partial_vwap_5", "limit_5", "book_sha256", "vwap_grid",
    "fak_evidence_known", "fak_reason", "fak_quantity_5", "fak_price_5", "fak_net_5", "fak_stress_5",
    "fok_evidence_known", "fok_reason", "fok_quantity_5", "fok_price_5", "fok_net_5", "fok_stress_5"]]


def read_day(run: Path, day: str, layers: list[str]) -> pl.DataFrame:
    core = pl.scan_parquet(run / "datasets/core" / f"{day}.parquet")
    for layer in layers:
        path = run / "datasets" / layer / f"{day}.parquet"
        other = pl.scan_parquet(path)
        columns = [c for c in other.collect_schema().names() if c not in IDENTITY or c == "point_id"]
        core = core.join(other.select(columns), on="point_id", how="left", validate="1:1")
    return derived_features(core.collect(engine="streaming"))


def load_arm(run: Path, arm: dict, *, include_exit_points: bool = False) -> pl.DataFrame:
    prepared_manifest = run / "manifests" / ("quiet-complete-case-panels.json" if arm.get("quiet_only") else "complete-case-panels.json")
    if prepared_manifest.exists():
        prepared = json.loads(prepared_manifest.read_text())
        if prepared["status"] != "complete" or prepared["identity"]["features"] != sha256(run / "inputs/candidate-feature-freeze.json"):
            raise ValueError("Prepared model-specific panel identity is not complete/current")
        records = [item for day in prepared["days"] for item in day["artifacts"]
                   if item["candidate"] == arm["candidate"] and item["arm"] == ("post_entry_observations" if include_exit_points else arm["arm"])]
        if not records:
            raise ValueError("Declared candidate panel is absent")
        for item in records:
            if sha256(Path(item["path"])) != item["sha256"]:
                raise ValueError("Prepared complete-case artifact changed")
        return pl.scan_parquet([item["path"] for item in records]).collect(engine="streaming").sort("decision_at", "market_id")
    required = list(arm["features"])
    for product in arm["matched_products"]:
        required.extend(reference_features([product]))
    layers = ["refprice-twap", "flow"]
    if arm.get("quiet_only"):
        layers.append("simulated-refresh")
    columns = list(dict.fromkeys(IDENTITY + DECISION + required))
    frames = []
    for path in sorted((run / "datasets/core").glob("*.parquet")):
        frame = read_day(run, path.stem, layers)
        eligible_clock = pl.col("entry_offset").is_in(arm["entry_offsets"]) & (pl.col("seconds_elapsed") < 260)
        if include_exit_points:
            eligible_clock = pl.lit(True)
        frame = complete_cases(frame.filter(eligible_clock), arm).select(columns)
        if frame.height:
            frames.append(frame)
    if not frames:
        return frame.head(0)
    return pl.concat(frames, how="vertical_relaxed").sort("decision_at", "market_id")


class FitBudget:
    """One-process durable fit accounting, bounded even after interruption."""

    def __init__(self, run: Path, frozen: dict):
        self.path = run / "manifests/fit-budget.json"
        self.frozen = frozen
        self.state = json.loads(self.path.read_text()) if self.path.exists() else {
            "fits_reserved": [], "elapsed_training_seconds": 0.0, "active_fit": None}
        self.started = monotonic()
        if self.state["active_fit"]:
            raise ValueError("Interrupted fit requires explicit status reconciliation before resuming")

    def reserve(self, identity: str, heads: int = 1) -> None:
        if any(x["identity"] == identity for x in self.state["fits_reserved"]):
            raise ValueError("Cannot silently refit an existing identity")
        resources = self.frozen["resources"]
        used = sum(x["heads"] for x in self.state["fits_reserved"])
        if used + heads > resources["maximum_training_fits_including_reference_and_arms"]:
            raise ValueError("Frozen fit budget exhausted")
        if self.elapsed() >= resources["training_wall_hours"] * 3600:
            raise ValueError("Frozen training wall-time budget exhausted")
        self.state["fits_reserved"].append({"identity": identity, "heads": heads, "reserved_at": datetime.now(UTC).isoformat()})
        self.state["active_fit"] = identity
        self.save()

    def elapsed(self) -> float:
        return self.state["elapsed_training_seconds"] + monotonic() - self.started

    def save(self) -> None:
        self.state["elapsed_training_seconds"] = self.elapsed()
        self.started = monotonic()
        write_json(self.path, self.state)

    def finish(self) -> None:
        self.state["active_fit"] = None
        self.save()


def support(frame: pl.DataFrame, minimum: int) -> bool:
    return frame["market_id"].n_unique() >= minimum and frame["label_up"].n_unique() == 2


def regress(frame: pl.DataFrame, features: list[str], label: str, frozen: dict):
    matrix = frame.select(features).to_numpy()
    if not np.isfinite(matrix).all() or not np.isfinite(frame[label].to_numpy()).all():
        raise ValueError("No imputation or implicit missing-value routing is permitted")
    model = HistGradientBoostingRegressor(**{k: v for k, v in frozen["model"].items()
        if k != "hyperparameter_settings_per_fit"}, random_state=frozen["seed"])
    model.fit(matrix, frame[label].to_numpy(), sample_weight=weights(frame))
    return model


def value_head(frame: pl.DataFrame, features: list[str], frozen: dict) -> tuple:
    rows = []
    for side in ["up", "down"]:
        rows.append(frame.filter(pl.col(f"{side}_fak_evidence_known") & (pl.col(f"{side}_fak_quantity_5") > 0))
                    .select("market_id", *features, pl.lit(float(side == "up")).alias("side_is_up"),
                            (pl.col(f"{side}_fak_stress_5") / pl.col(f"{side}_fak_quantity_5")).alias("value_label")))
    long = pl.concat(rows)
    if long["market_id"].n_unique() < frozen["calibration"]["minimum_fit_markets"]:
        raise ValueError("Insufficient executed value labels for the required value head")
    head_features = features + ["side_is_up"]
    return regress(long, head_features, "value_label", frozen), head_features


def score_bundle(frame: pl.DataFrame, bundle: dict, frozen: dict) -> pl.DataFrame:
    if frame.is_empty():
        return frame.with_columns(pl.lit(None, dtype=pl.Float64).alias("probability_up"),
                                  pl.lit(None, dtype=pl.Float64).alias("conservative_probability_up"))
    result = score(frame, bundle["direction"], frozen["calibration"]["safety_probability"])
    if bundle.get("value"):
        model, features = bundle["value"]
        for side in ["up", "down"]:
            matrix = result.with_columns(pl.lit(float(side == "up")).alias("side_is_up")).select(features).to_numpy()
            result = result.with_columns(pl.Series(f"expected_value_{side}", model.predict(matrix)))
    return result


def fit_fold(run: Path, panel: pl.DataFrame, arm: dict, fold: dict, budget: FitBudget,
             *, exit_panel: pl.DataFrame | None = None) -> dict:
    from .time_bucket_exit import fit_exit
    from .time_bucket_reference import label_contract
    from .time_bucket_replay import calibration_replay, choose_exit, select_quantity

    frozen = config(run)
    label_contract(run)
    eligibility = json.loads((run / "manifests/training-eligibility.json").read_text())
    if arm["candidate"] not in eligibility.get("accepted_candidates", []):
        raise ValueError("Candidate-specific training checkpoint dependencies are not accepted")
    target = run / "checkpoints" / arm["candidate"] / arm["arm"] / fold["name"]
    manifest_path = target / "manifest.json"
    panel_names = ["core-panels.json", "refprice-twap-panels.json", "flow-panels.json"]
    panel_names.append("quiet-complete-case-panels.json" if arm.get("quiet_only") else "complete-case-panels.json")
    if arm.get("quiet_only"):
        panel_names.append("simulated-refresh-panels.json")
    if arm["head"] == "direction_and_exit":
        panel_names.append("exit-labels.json")
    identity = {"candidate": arm["candidate"], "arm": arm["arm"], "fold": fold,
                "features": arm["features"], "configuration_sha256": sha256(run / "inputs/tournament-freeze.json"),
                "feature_freeze_sha256": sha256(run / "inputs/candidate-feature-freeze.json"),
                "panel_manifests": {name: sha256(run / "manifests" / name) for name in panel_names}}
    if manifest_path.exists():
        record = json.loads(manifest_path.read_text())
        if any(record.get(key) != value for key, value in identity.items()):
            raise ValueError("Existing fit differs from the current frozen input/feature/fold identity")
        for entry in record.get("artifacts", []):
            if sha256(Path(entry["path"])) != entry["sha256"]:
                raise ValueError("Checkpoint artifact identity changed")
        predictions = record.get("calibration_predictions")
        if predictions and sha256(Path(predictions["path"])) != predictions["sha256"]:
            raise ValueError("Calibration predictions changed since checkpoint")
        return record
    train, calibration = [split_rows(panel, frozen, fold, role) for role in ["training", "calibration"]]
    target.mkdir(parents=True, exist_ok=True)
    record = {**identity,
              "train_markets": train["market_id"].n_unique(), "train_rows": train.height,
              "calibration_markets": calibration["market_id"].n_unique(), "calibration_rows": calibration.height,
              "status": "insufficient_complete_case_support", "artifacts": []}
    if not support(train, frozen["calibration"]["minimum_fit_markets"]) or not support(calibration, frozen["calibration"]["minimum_calibration_markets"]):
        write_json(manifest_path, record)
        return record
    for frame in [train, calibration]:
        if not np.isfinite(frame.select(arm["features"]).to_numpy()).all():
            raise ValueError("Complete-case contract was violated before fitting")
    # Chronology guards are independent of the row-selection implementation.
    if train.filter(pl.col("role") != "training").height or calibration.filter(pl.col("role") != "calibration").height:
        raise ValueError("Evaluation or holdout observations entered fitting/calibration")
    budget.reserve(f"{arm['candidate']}/{arm['arm']}/{fold['name']}", 2 if arm["head"] in {"direction_and_value", "direction_and_exit"} else 1)
    direction = fit_model(train, calibration, frozen["model"], frozen, frozen["seed"], features=tuple(arm["features"]))
    bundle = {"direction": direction, "value": None, "exit": None}
    if arm["head"] == "direction_and_value":
        bundle["value"] = value_head(train, arm["features"], frozen)
    if arm["head"] == "direction_and_exit":
        if exit_panel is None:
            raise ValueError("Buy/sell candidate requires separate causal post-entry training observations")
        exit_train = split_rows(exit_panel, frozen, fold, "training")
        target_rows = pl.scan_parquet(run / "datasets/exit-labels/*.parquet").filter(
            (pl.col("order_type") == "FAK") & pl.col("sell_advantage_label").is_not_null()
        ).join(exit_train.lazy().select("point_id"), on="point_id", how="inner").collect(engine="streaming")
        bundle["exit"] = fit_exit(exit_train, target_rows, arm["features"], frozen)
    calibrated = score_bundle(calibration, bundle, frozen)
    policy, search = choose_policy(calibrated, frozen)
    bundle["policy"] = policy
    bundle["exit_policy"] = None
    exit_calibration = None
    if bundle["exit"]:
        exit_calibration = split_rows(exit_panel, frozen, fold, "calibration")
        selected_exit, exit_search = choose_exit(run, calibrated, policy, frozen, bundle["exit"], exit_calibration)
        bundle["exit_policy"] = selected_exit
        exit_policy_path = target / "exit-policy.json"
        write_json(exit_policy_path, {"selected": selected_exit, "calibration_search": exit_search})
        record["artifacts"].append({"kind": "exit-policy", "path": str(exit_policy_path), "sha256": sha256(exit_policy_path)})
    quantity_search = calibration_replay(run, calibrated, policy, frozen, frozen["quantity_grid"],
        exit_bundle=bundle["exit"], exit_policy=bundle["exit_policy"], exit_rows=exit_calibration,
        control="learned" if bundle["exit"] else "hold")
    bundle["selected_quantity"] = select_quantity(quantity_search, frozen)
    quantity_path = target / "quantity-selection.json"
    write_json(quantity_path, {"selected": bundle["selected_quantity"], "calibration_search": quantity_search})
    record["artifacts"].append({"kind": "quantity-selection", "path": str(quantity_path), "sha256": sha256(quantity_path)})
    for name, artifact in [("direction", direction[0]), ("probability-calibration", direction[1]),
                           ("model-bundle", bundle)]:
        path = target / f"{name}.joblib"
        joblib.dump(artifact, path, compress=3)
        record["artifacts"].append({"kind": name, "path": str(path), "sha256": sha256(path)})
    if bundle["value"]:
        path = target / "admission-value.joblib"
        joblib.dump(bundle["value"], path, compress=3)
        record["artifacts"].append({"kind": "admission-value", "path": str(path), "sha256": sha256(path)})
    if bundle["exit"]:
        path = target / "exit-sell.joblib"
        joblib.dump(bundle["exit"], path, compress=3)
        record["artifacts"].append({"kind": "exit-sell", "path": str(path), "sha256": sha256(path)})
    if arm.get("quiet_only"):
        path = target / "quiet-state.json"
        write_json(path, {"reference": "simulated refresh activity", "rule": frozen["reference"],
                          "source_panel_manifest": identity["panel_manifests"]["simulated-refresh-panels.json"],
                          "actual_incumbent_comparison_available": False})
        record["artifacts"].append({"kind": "quiet-state", "path": str(path), "sha256": sha256(path)})
    policy_path = target / "policy.json"
    write_json(policy_path, {"selected": policy, "selection": "calibration-only Q5 FAK; unchanged FOK control", "search": search})
    record["artifacts"].append({"kind": "admission-policy", "path": str(policy_path), "sha256": sha256(policy_path)})
    calibration_path = run / "predictions/calibration" / arm["candidate"] / arm["arm"] / f"{fold['name']}.parquet"
    calibration_path.parent.mkdir(parents=True, exist_ok=True)
    calibrated.write_parquet(calibration_path, compression="zstd")
    record.update(status="complete", calibration_ece=direction[3],
                  calibration_predictions={"path": str(calibration_path), "sha256": sha256(calibration_path)},
                  fit_latest_market_end=str(train["window_end"].max()), calibration_latest_market_end=str(calibration["window_end"].max()))
    write_json(manifest_path, record)
    budget.finish()
    return record
