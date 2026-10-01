"""Causal offline diagnostics for two immutable native historical controls.

This module never fits or selects. Native grids, abstention and quantity policies
remain separate from the current tournament's challenger qualification.
"""

from __future__ import annotations

import argparse
import ast
import hashlib
import inspect
import json
import subprocess
import tomllib
from dataclasses import replace
from datetime import UTC, date, datetime, timedelta
from pathlib import Path

import joblib
import numpy as np
import polars as pl

from . import time_bucket_specialist_tournament as specialist
from .conservative_selective_training import score as capacity_score
from .core_features import derive_core_point_in_time_features
from .fok_retry_backtest import walk
from .time_bucket_evaluation import chosen_rows
from .time_bucket_execution import at, day_books, levels
from .time_bucket_historical import CANDLE_TIMES, CANDLE_VALUES, KEYS, PRODUCT, reconstruct_native
from .time_bucket_labels import label_contract, prediction_features_only
from .time_bucket_protocol import config, split_rows
from .time_bucket_replay import LEDGER_SCHEMA, replay_rows
from .time_bucket_source_audit import checked_run, sha256, write_json

CAPACITY = "conservative_selective_locked_capacity"
TERMINAL = "time_bucket_price_control_terminal_without_rtds"
EVIDENCE_SHA256 = "0c3124c232a4481e45a0eefd4595c71fc18863aa651887efa5d9209575aac8b2"
ARTIFACTS = {
    CAPACITY: "65d41630a2211cf2979e5fa952d34fb494bbc85c7027bc100d35b96bdbe86453",
    TERMINAL: "cdfd94dc40fe2809b24a4fd8a7cd920fd9e31ab3146bc7320a9d31afb644e618",
}
CUTOFFS = {
    CAPACITY: datetime(2026, 9, 22, tzinfo=UTC),
    TERMINAL: datetime(2026, 9, 1, tzinfo=UTC),
}
OWNER_HASHES = {
    capacity_score: "cb53f073f35e3b42a3dd77b95c19b447dfabc0bbed7d75bcc0f56c7bae3d81cf",
    specialist._matrix: "7471973f25bf6500da25ad72fe9ffab8b3ac005d8015d1eb0a928a7b1f67f243",
    specialist._predict: "e43d308de4e4e70eb1bdc73694cb8f31e9daf7f00eef2d00bd738936bc04395a",
    specialist._opportunities: "c17b8995b45c5e81dea94c0c340b972deeed9e55983f52d4824561ac8c0fb178",
    specialist._select_trades: "5a02fcf1b09eb393766943693467fc8395d3d965f520f25e37321d47d4c41a20",
    derive_core_point_in_time_features: "a5d33bbfb9bd4db5d4c58cd7ffff19273ed0048022c7bc19614b296031b62ab0",
}
BOOK_COLUMNS = [f"{side}_{name}" for side in ("up", "down")
                for name in ("vwap_5", "book_age_seconds")]


def cutoff(name: str, labels: dict) -> datetime:
    if labels.get("seconds_after_close") != 1800:
        raise ValueError("Historical controls require the authorized label contract")
    end = CUTOFFS[name] + timedelta(seconds=1800)
    for row in labels.get("verified_later_availability", []):
        if not row.get("evidence") or not row.get("semantics_verified"):
            raise ValueError("Unverified historical label override")
        stamp = datetime.fromisoformat(row["available_at"])
        if stamp.tzinfo is None:
            raise ValueError("Historical label availability requires a timezone")
        end = max(end, stamp.astimezone(UTC))
    return end


def load_controls(run: Path) -> dict:
    evidence_path = run / "manifests/historical-conditional-pathways.json"
    if sha256(evidence_path) != EVIDENCE_SHA256:
        raise ValueError("Historical conditional evidence changed")
    evidence = json.loads(evidence_path.read_text())
    inventory = json.loads((run / "inputs/historical-artifact-freeze.json").read_text())
    for function, expected in OWNER_HASHES.items():
        syntax = ast.parse(inspect.getsource(function)).body[0]
        if hashlib.sha256(ast.dump(syntax, include_attributes=False).encode()).hexdigest() != expected:
            raise ValueError(f"Native historical owner changed: {function.__name__}")
    controls = {}
    for name, expected in ARTIFACTS.items():
        record = next(r for r in evidence["pathways"] if r["pathway"] == name)
        path = Path(record["artifact"]["path"])
        if sum(r["sha256"] == expected and r["path"] == str(path) for r in inventory) != 1:
            raise ValueError("Historical control is absent from the immutable freeze")
        for ref in [record["artifact"], *record["metadata_evidence"]]:
            if sha256(Path(ref["path"])) != ref["sha256"]:
                raise ValueError("Historical artifact or provenance changed")
        if sha256(path) != expected:
            raise ValueError("Unsupported historical artifact")
        payload = joblib.load(path)
        features = record["features"]
        prediction_features_only(features)
        config_path = next(Path(r["path"]) for r in record["metadata_evidence"]
                           if r["path"].endswith(".toml"))
        original = subprocess.check_output(
            ["git", "show", f"{record['producing_commit']}:packages/btc-directional-model/configs/{config_path.name}"],
            cwd=Path(__file__).parents[2],
        )
        if hashlib.sha256(original).hexdigest() != record["config_sha256"]:
            raise ValueError("Historical configuration identity mismatch")
        raw = tomllib.loads(original.decode())
        if name == CAPACITY:
            if (list(payload["features"]) != features
                    or payload["source_sha256"] != record["source_sha256"]
                    or payload["quantity_policies"] != record["native_quantity_policies"]
                    or any(p is not None for p in payload["quantity_policies"].values())):
                raise ValueError("The immutable capacity control must retain all abstaining policies")
            quantities = sorted(int(q) for q in payload["quantity_policies"])
        else:
            if (list(payload["model"].features) != features
                    or payload["model"].neutralized_columns
                    or payload["contract"]["name"] != record["name"]
                    or payload["config_sha256"] != record["config_sha256"]
                    or payload["source_sha256"] != record["source_sha256"]):
                raise ValueError("The immutable terminal feature contract changed")
            # Thresholds come from the hashed original checkpoint metrics, never this run.
            policy = specialist.Policy(**record["policy"])
            if policy.quantity != 5 or policy.side != "both":
                raise ValueError("Unsupported native terminal policy")
            payload["policy"] = policy
            quantities = list(dict.fromkeys([*raw["execution"]["quantities"],
                                             *raw["execution"]["capacity_quantities"]]))
        controls[name] = {"payload": payload, "record": record, "raw": raw,
                          "features": features, "quantities": quantities,
                          "cutoff": cutoff(name, label_contract(run))}
    return controls


def schedule(panel: pl.DataFrame, frozen: dict, fold: dict) -> pl.DataFrame:
    """Add native decision times without changing any frozen current-grid identity."""
    eligible = split_rows(panel, frozen, fold, "evaluation")["market_id"].unique()
    current = panel.filter(pl.col("seconds_elapsed") < 260).select(*KEYS).with_columns(
        pl.col("market_id").is_in(eligible.implode()).alias("evaluation_clock_eligible"),
        pl.lit("tournament_grid").alias("decision_grid"),
    )
    markets = current.select("market_id", "window_start", "window_end", "official_outcome",
                             "label_up", "role", "role_fold", "evaluation_clock_eligible").unique()
    native = markets.join(pl.DataFrame({"seconds_elapsed": list(range(60, 241, 5))}), how="cross")
    native = native.with_columns(
        (pl.col("window_start") + pl.duration(seconds=pl.col("seconds_elapsed"))).alias("decision_at"),
        (pl.col("seconds_elapsed") // 20 * 20).alias("bucket_start"),
        (pl.col("seconds_elapsed") % 20).alias("entry_offset"),
        (pl.col("market_id") + pl.lit("@") + pl.col("seconds_elapsed").cast(pl.String)).alias("point_id"),
        pl.lit("native_grid").alias("decision_grid"),
    ).select(current.columns)
    return pl.concat([current, native], how="vertical_relaxed")


def decision_books(rows: pl.DataFrame, index: dict, frozen: dict, quantities: list[int]) -> pl.DataFrame:
    """Adapt actual causal ladders to native VWAP inputs without quantity interpolation."""
    output = []
    for row in rows.select("point_id", "market_id", "decision_at").unique().iter_rows(named=True):
        values = {"point_id": row["point_id"]}
        for side in ("up", "down"):
            book = at(index, row["market_id"], side, row["decision_at"], frozen["book"]["max_age_seconds"])
            values[f"{side}_book_age_seconds"] = ((row["decision_at"] - book.source_timestamp).total_seconds()
                                                    if book else None)
            for q in quantities:
                result = walk(levels(book.asks), q) if book else None
                values[f"{side}_vwap_{q}"] = result[0] if result else None
        output.append(values)
    schema = {"point_id": pl.String, **{f"{side}_{field}": pl.Float64 for side in ("up", "down")
               for field in ["book_age_seconds", *(f"vwap_{q}" for q in quantities)]}}
    return pl.DataFrame(output, schema=schema)


def score_control(name: str, rows: pl.DataFrame, native: pl.DataFrame, control: dict) -> pl.DataFrame:
    """Score finite causal native features; retain every unsupported/unknown row."""
    features = control["features"]
    prediction_features_only(features)
    joined = rows.join(native, on="point_id", how="left", validate="1:1").with_columns(
        (pl.col("decision_at") > control["cutoff"]).alias("historical_cutoff_eligible"),
        ((pl.col("seconds_elapsed") % 5 == 0)
         & pl.col("seconds_elapsed").is_between(60, 240)).alias("native_grid_eligible"),
        (pl.col("seconds_elapsed").is_between(210, 239) if name == TERMINAL
         else pl.lit(True)).alias("native_window_eligible"),
        pl.all_horizontal(pl.col(c).is_finite().fill_null(False) for c in features).alias("feature_eligible"),
    )
    eligible = joined.filter(pl.all_horizontal(pl.col(c) for c in ["historical_cutoff_eligible",
        "native_grid_eligible", "native_window_eligible", "feature_eligible", "evaluation_clock_eligible"]))
    probabilities = eligible.select("point_id").head(0).with_columns(
        pl.lit(None, dtype=pl.Float64).alias("probability_up"),
        pl.lit(None, dtype=pl.Float64).alias("conservative_probability_up"),
    )
    if eligible.height:
        payload = control["payload"]
        # Only declared feature columns enter either unchanged native scorer.
        frame = eligible.select("point_id", *features)
        if name == CAPACITY:
            model = (payload["estimator"], payload["calibrator"], tuple(features), payload["calibration_ece"])
            probabilities = capacity_score(frame, model, control["raw"]["calibration"]["safety_floor"]).select(
                "point_id", "probability_up", "conservative_probability_up")
        else:
            p = specialist._predict(payload["model"], frame)
            probabilities = frame.select("point_id").with_columns(
                pl.Series("probability_up", p),
                pl.lit(None, dtype=pl.Float64).alias("conservative_probability_up"))
        values = probabilities["probability_up"].to_numpy()
        if not np.isfinite(values).all() or np.any((values < 0) | (values > 1)):
            raise ValueError("Invalid native control probabilities")
    return joined.join(probabilities, on="point_id", how="left", validate="1:1").with_columns(
        (1 - pl.col("probability_up")).alias("probability_down"),
        pl.when(pl.col("probability_up").is_null()).then(pl.lit(None, dtype=pl.String))
        .when(pl.col("probability_up") >= 0.5).then(pl.lit("up")).otherwise(pl.lit("down")).alias("chosen_side"),
        pl.lit("historical_" + name).alias("candidate"),
        pl.lit("historical_reference_only").alias("qualification_class"),
    )


def admit_control(name: str, scored: pl.DataFrame, control: dict, frozen: dict, quantity: int = 5) -> pl.DataFrame:
    """Apply unchanged policies independently for bucket and sequential diagnostics."""
    if quantity not in control["quantities"]:
        raise ValueError("Quantity has no native policy/capacity contract; do not interpolate")
    eligible = scored.filter(pl.col("probability_up").is_not_null())
    admitted, bucket_admitted, executable = [], [], []
    if name == TERMINAL and eligible.height:
        frame = eligible.with_columns(
            pl.col("decision_at").alias("observed_at"),
            pl.lit(frozen["fees"]["rate_assumption"]).alias("fee_rate"),
            *(pl.col(f"{side}_vwap_{quantity}").alias(f"{side}_ask_vwap_{quantity}") for side in ("up", "down")),
            *(pl.col(f"{side}_book_age_seconds").alias(f"pm_{side}_book_age_seconds") for side in ("up", "down")),
        )
        predictions = frame.select(*specialist.KEY_COLUMNS, "label_up", pl.col("probability_up").alias("probability"))
        opportunities = specialist._opportunities(frame, predictions, quantity, control["raw"])
        keys = frame.select("point_id", "market_id", "seconds_elapsed", "bucket_start")
        opportunities = opportunities.join(keys, on=["market_id", "seconds_elapsed"], validate="1:1")
        executable = opportunities.filter(pl.col("share_cost").is_finite() & (pl.col("share_cost") > 0))["point_id"].to_list()
        policy = replace(control["payload"]["policy"], quantity=quantity)
        admitted = specialist._select_trades(opportunities, policy, control["raw"])["point_id"].to_list()
        for part in opportunities.partition_by("bucket_start"):
            bucket_admitted.extend(specialist._select_trades(part, policy, control["raw"])["point_id"].to_list())
    reason = (pl.when(~pl.col("evaluation_clock_eligible")).then(pl.lit("purged_boundary_market"))
        .when(~pl.col("historical_cutoff_eligible")).then(pl.lit("historical_fit_calibration_policy_cutoff"))
        .when(~pl.col("native_grid_eligible")).then(pl.lit("outside_native_five_second_grid"))
        .when(~pl.col("native_window_eligible")).then(pl.lit("outside_native_terminal_210_239"))
        .when(~pl.col("feature_eligible")).then(pl.lit("missing_causal_native_complete_prefix_or_features")))
    if name == CAPACITY:
        reason = reason.otherwise(pl.lit("native_quantity_policy_abstains"))
    else:
        reason = (reason.when(~pl.col("point_id").is_in(executable)).then(pl.lit("missing_native_quantity_decision_book"))
            .when(pl.col("point_id").is_in(admitted)).then(pl.lit("admitted"))
            .otherwise(pl.lit("native_frozen_policy_rejected_or_later_attempt")))
    return scored.with_columns(pl.col("point_id").is_in(admitted).alias("admitted"),
        pl.col("point_id").is_in(bucket_admitted).alias("bucket_diagnostic_admitted"),
        pl.lit(quantity).alias("native_policy_quantity"), reason.alias("admission_reason"))


def replay_control(name: str, predictions: pl.DataFrame, index: dict, control: dict,
                   frozen: dict, quantity: int = 5, grid: str = "tournament_grid") -> pl.DataFrame:
    pieces = []
    for view in ("bucket_diagnostic", "sequential_policy"):
        attempts = chosen_rows(predictions, view)
        if attempts.is_empty():
            continue
        if name == CAPACITY:
            raise ValueError("An abstaining immutable capacity policy cannot admit a trade")
        for kind in ("FAK", "FOK"):
            scenarios = [{"name": "base"}] if view == "bucket_diagnostic" else frozen["scenarios"]
            for scenario in scenarios:
                pieces.append(replay_rows(attempts, index, frozen, quantity, kind, scenario,
                    control["payload"]["policy"].maximum_share_cost).with_columns(pl.lit(view).alias("economic_view")))
    result = pl.concat(pieces, how="vertical_relaxed") if pieces else pl.DataFrame(
        schema={**LEDGER_SCHEMA, "economic_view": pl.String})
    return result.with_columns(pl.lit("historical_" + name).alias("candidate"),
        pl.lit("historical_reference_only").alias("qualification_class"), pl.lit(grid).alias("decision_grid"))


def _verified_file(ref: dict) -> Path:
    path = Path(ref["path"])
    if sha256(path) != ref["sha256"]:
        raise ValueError(f"Historical control input/output checksum mismatch: {path}")
    return path


def execute(run: Path) -> None:
    frozen, controls = config(run), load_controls(run)
    names = ["core-panels.json", f"flow-source-validation-{PRODUCT}.json",
             "core-source-validation-polymarket_clob_l2.json", "evaluation-predictions.json"]
    manifests = {n: json.loads((run / "manifests" / n).read_text()) for n in names}
    if any(manifests[n]["status"] not in {"complete", "completed"} for n in names):
        raise ValueError("Complete source validation and frozen current OOS predictions before historical replay")
    identity = {"configuration": sha256(run / "inputs/tournament-freeze.json"),
        "historical_freeze": sha256(run / "inputs/historical-artifact-freeze.json"),
        "label_contract": sha256(run / "inputs/label-availability-contract.json"), "evidence": EVIDENCE_SHA256,
        "manifests": {n: sha256(run / "manifests" / n) for n in names},
        "code": {n: sha256(Path(__file__).with_name(n)) for n in [
            "time_bucket_historical_controls.py", "time_bucket_historical.py", "time_bucket_execution.py",
            "time_bucket_evaluation.py", "time_bucket_protocol.py", "time_bucket_replay.py",
            "time_bucket_exit.py", "fok_retry_backtest.py", "core_execution.py"]}}
    path = run / "manifests/historical-controls.json"
    report = json.loads(path.read_text()) if path.exists() else {
        "identity": identity, "status": "in_progress", "qualification_class": "historical_reference_only",
        "days": [], "quantity_contract": {n: {"native": c["quantities"],
            "unsupported_current_grid": sorted(set(frozen["quantity_grid"]) - set(c["quantities"]))}
            for n, c in controls.items()}}
    if report["identity"] != identity:
        raise ValueError("Historical control checkpoint identity changed")
    done = {r["date"]: r for r in report["days"]}
    candle_files = {r["date"]: r for r in manifests[f"flow-source-validation-{PRODUCT}.json"]["daily"]}
    book_files = manifests["core-source-validation-polymarket_clob_l2.json"]["days"]
    for record in manifests["core-panels.json"]["days"]:
        day = record["date"]
        fold = next((f for f in frozen["folds"] if f["evaluation_start"] <= day < f["evaluation_end"]), None)
        if fold is None:
            continue
        if day in done:
            for output in done[day]["outputs"]:
                _verified_file(output)
            continue
        panel = pl.read_parquet(_verified_file(record), columns=KEYS)
        rows = schedule(panel, frozen, fold)
        candles = []
        for source_day in [date.fromisoformat(day) - timedelta(days=1), date.fromisoformat(day)]:
            key = str(source_day)
            source = candle_files.get(key)
            if source:
                source_path = _verified_file({"path": source["output_path"], "sha256": source["output_sha256"]})
                candles.append(pl.read_parquet(source_path, columns=["source", "symbol", *CANDLE_TIMES, *CANDLE_VALUES]))
            if key in book_files:
                _verified_file(book_files[key]["output"])
            elif (run / "inputs/validated-core/polymarket_clob_l2" / f"{key}.parquet").exists():
                raise ValueError("Unmanifested native replay book input")
        index = day_books(run, date.fromisoformat(day), rows["market_id"].unique().to_list())
        features = list(dict.fromkeys(f for c in controls.values() for f in c["features"]))
        unique = rows.unique("point_id")
        native = reconstruct_native(unique, pl.concat(candles) if candles else pl.DataFrame(), features)
        quantities = sorted({q for c in controls.values() for q in c["quantities"]})
        books = decision_books(unique, index, frozen, quantities)
        outputs, rejections = [], []
        for name, control in controls.items():
            ledger_parts = []
            for grid in ["tournament_grid", "native_grid"]:
                grid_rows = rows.filter(pl.col("decision_grid") == grid).join(books, on="point_id", validate="1:1")
                feature_native = native.select("point_id", *control["features"], "prefix_available_at",
                    "native_first_open_timestamp", "native_last_open_timestamp", "native_prefix_rows")
                scored = score_control(name, grid_rows, feature_native, control)
                for q in [5] if grid == "tournament_grid" else control["quantities"]:
                    admitted = admit_control(name, scored, control, frozen, q)
                    target = run / "predictions/historical-controls" / name / grid / f"q{q}" / f"{day}.parquet"
                    if target.exists():
                        raise ValueError("Unrecorded historical prediction output cannot be overwritten")
                    target.parent.mkdir(parents=True, exist_ok=True)
                    admitted.write_parquet(target, compression="zstd")
                    outputs.append({"path": str(target), "sha256": sha256(target), "rows": admitted.height})
                    rejections.extend(admitted.group_by("bucket_start", "admission_reason").len().with_columns(
                        pl.lit(name).alias("control"), pl.lit(grid).alias("decision_grid"), pl.lit(q).alias("quantity")).to_dicts())
                    ledger_parts.append(replay_control(name, admitted, index, control, frozen, q, grid))
            ledger = pl.concat(ledger_parts, how="vertical_relaxed")
            target = run / "trades/historical-controls" / name / f"{day}.parquet"
            if target.exists():
                raise ValueError("Unrecorded historical ledger cannot be overwritten")
            target.parent.mkdir(parents=True, exist_ok=True)
            ledger.write_parquet(target, compression="zstd")
            outputs.append({"path": str(target), "sha256": sha256(target), "rows": ledger.height})
        report["days"].append({"date": day, "outputs": outputs, "rejections": rejections})
        write_json(path, report)
        levels.cache_clear()
        print(json.dumps({"historical_controls_date": day, "outputs": len(outputs)}), flush=True)
    report["status"] = "complete_with_explicit_native_range_and_policy_exclusions"
    write_json(path, report)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("run", type=Path)
    args = parser.parse_args()
    execute(checked_run(args.run))


if __name__ == "__main__":
    main()
