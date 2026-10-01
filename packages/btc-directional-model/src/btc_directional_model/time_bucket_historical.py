"""Immutable historical diagnostics using native feature math and current offline replay.

This module never fits, calibrates, selects, or qualifies a challenger. The supported
reference keeps its original five-second observation grid and Q5 bucket policies.
"""

from __future__ import annotations

import argparse
import ast
import hashlib
import inspect
import json
from datetime import UTC, date, datetime, timedelta
from pathlib import Path

import joblib
import numpy as np
import polars as pl

from .core_features import derive_core_point_in_time_features
from .micro_edge_evaluation import apply_policies, opportunities, select
from .micro_edge_models import calibrate, matrix, predict_estimator
from .time_bucket_evaluation import chosen_rows
from .time_bucket_execution import day_books, levels
from .time_bucket_labels import label_contract, prediction_features_only
from .time_bucket_protocol import config, split_rows
from .time_bucket_replay import LEDGER_SCHEMA, replay_rows
from .time_bucket_source_audit import checked_run, sha256, write_json

NAME = "reversal_exhaustion__no_rtds"
CANDIDATE = "historical_" + NAME
ARTIFACT_SHA256 = "e89d5910a4374aa596b903c3cc6458430b6cd96af524d5cd47c5ae6556b61aa7"
SIDECAR_SHA256 = "712077a4ded89273c60158dc109b002c7b1a02a7fa6b6617a6e086856604f46e"
PRODUCING_COMMIT = "0c7d50ce4b8b247d0435a0ac5a32adbeff171b2e"
TRAINING_END = datetime(2026, 9, 15, tzinfo=UTC)
PRODUCT = "binance_spot_one_second_ohlcv"
NATIVE_FUNCTIONS = {
    derive_core_point_in_time_features: "6a233b71911c5a14bfe5e76d1df26c2fd019e5f77a29b4306e3981d5a6696d95",
    matrix: "19b948f54cccd5bf819a896ce114972116bd60f5c50711c58e649c04ffa80fb2",
    predict_estimator: "8337790ceb51af8e1222ed7721dd501a52eb96bb30b8d3816f1bcfcd94ee2cd3",
    calibrate: "914c9c4ac4ee1ab210205bad2d314c47016f2b03a5485e4f688e67269fedea40",
    opportunities: "8e7d6149736237a8741ebb6c95173605c77024a47484f7e0e16160786e60dc07",
    select: "360877fc0da222d4b4aa5067cb0f28e3a62813c1ff676964d4829cfb2c32577c",
    apply_policies: "a50dc2bd951d7b11ab1c265a35593032d07b3ec8bc5efb6ba3a0ec6c5ca5ddc6",
}
CANDLE_VALUES = {
    "open_price": "btc_open",
    "high_price": "btc_high",
    "low_price": "btc_low",
    "close_price": "btc_close",
    "base_volume": "btc_base_volume",
    "quote_volume": "btc_quote_volume",
    "taker_buy_base_volume": "btc_taker_buy_base_volume",
    "taker_buy_quote_volume": "btc_taker_buy_quote_volume",
    "trade_count": "trade_count",
}
CANDLE_TIMES = [
    "open_timestamp",
    "close_timestamp",
    "source_timestamp",
    "received_at",
    "provider_available_at",
    "available_at",
]
KEYS = [
    "point_id",
    "market_id",
    "window_start",
    "window_end",
    "decision_at",
    "seconds_elapsed",
    "bucket_start",
    "entry_offset",
    "official_outcome",
    "label_up",
    "role",
    "role_fold",
]


def historical_cutoff(labels: dict) -> datetime:
    """Preserve the final fit/calibration/policy boundary and later proven label times."""
    if labels.get("seconds_after_close") != 1800:
        raise ValueError("Historical diagnostics require the authorized label contract")
    cutoff = TRAINING_END + timedelta(seconds=labels["seconds_after_close"])
    # Without a proven original fit-market list, every verified later override is
    # conservatively considered: no override can move this boundary earlier.
    for record in labels.get("verified_later_availability", []):
        if not record.get("evidence") or not record.get("semantics_verified"):
            raise ValueError("Unverified historical label availability override")
        timestamp = datetime.fromisoformat(record["available_at"])
        if timestamp.tzinfo is None:
            raise ValueError("Historical label availability requires an explicit timezone")
        cutoff = max(cutoff, timestamp.astimezone(UTC))
    return cutoff


def load_reference(run: Path) -> tuple[dict, dict]:
    inventory = json.loads((run / "inputs/historical-artifact-freeze.json").read_text())
    matches = [r for r in inventory if r["sha256"] == ARTIFACT_SHA256]
    if len(matches) != 1:
        raise ValueError("Exact supported immutable reference is absent from the source freeze")
    path = Path(matches[0]["path"])
    if sha256(path) != ARTIFACT_SHA256 or sha256(path.with_suffix(".json")) != SIDECAR_SHA256:
        raise ValueError("Historical artifact or cutoff provenance changed")
    for function, expected in NATIVE_FUNCTIONS.items():
        syntax = ast.dump(ast.parse(inspect.getsource(function)), include_attributes=False)
        if hashlib.sha256(syntax.encode()).hexdigest() != expected:
            raise ValueError(f"Native historical semantics changed: {function.__name__}")
    sidecar = json.loads(path.with_suffix(".json").read_text())
    payload = joblib.load(path)
    if (
        payload["recipe"]["name"] != NAME
        or payload["recipe"]["kind"] != "tree"
        or payload["recipe"]["rtds"]
        or payload["base_model"] is not None
        or payload["quantity"] != 5
        or datetime.fromisoformat(payload["training_end_exclusive"]) != TRAINING_END
        or sidecar["producing_commit"] != PRODUCING_COMMIT
        or payload["source_identity"] != sidecar["source_identity"]
    ):
        raise ValueError("Unsupported native historical model contract")
    prediction_features_only(payload["feature_names"])
    labels = label_contract(run)
    return payload, {
        "candidate": CANDIDATE,
        "artifact": str(path),
        "artifact_sha256": ARTIFACT_SHA256,
        "sidecar_sha256": SIDECAR_SHA256,
        "producing_commit": PRODUCING_COMMIT,
        "training_start": payload["training_start"],
        "fit_calibration_policy_end_exclusive": TRAINING_END,
        "strictly_after": historical_cutoff(labels),
        "label_contract": labels,
        "native_quantity": 5,
        "native_decision_step_seconds": 5,
        "native_policy_bucket_seconds": 10,
        "features": payload["feature_names"],
        "native_function_hashes": {f.__name__: h for f, h in NATIVE_FUNCTIONS.items()},
        "clock": "observed_at=open_timestamp+1s; second0 open_price is the opening boundary",
        "availability": "Every original candle in the complete market prefix is known by decision",
        "scope": "Historical partial-range offline diagnostic, never a new challenger or qualification",
        "quantity_grid": "Counterfactual sizing under unchanged native Q5 admission; no selection",
        "fees": "Current frozen offline fee assumption; native admission keeps its original 0.005 reserve",
        "cutoff_evidence": "Original micro_edge_tournament.py at producing commit: final fit uses pre-END panel; final calibration and policies use pre-END OOF predictions/opportunities",
    }


def reconstruct_native(
    rows: pl.DataFrame, candles: pl.DataFrame, features: list[str]
) -> pl.DataFrame:
    """Reconstruct only exact native closed-second prefixes, without interpolation."""
    prediction_features_only(features)
    empty = (
        rows.select("point_id")
        .head(0)
        .with_columns(
            *(pl.lit(None, dtype=pl.Float64).alias(c) for c in features),
            pl.lit(None, dtype=pl.Datetime("us", "UTC")).alias("prefix_available_at"),
            pl.lit(None, dtype=pl.Datetime("us", "UTC")).alias("native_first_open_timestamp"),
            pl.lit(None, dtype=pl.Datetime("us", "UTC")).alias("native_last_open_timestamp"),
            pl.lit(None, dtype=pl.Int64).alias("native_prefix_rows"),
        )
    )
    if rows.is_empty() or candles.is_empty():
        return empty
    keys = ["source", "symbol", "open_timestamp"]
    if candles.select(keys).n_unique() != candles.height:
        raise ValueError("Validated native candles contain duplicate identities")
    if candles.filter(
        (pl.col("source") != "binance_spot") | (pl.col("symbol") != "BTCUSDT")
    ).height:
        raise ValueError("Historical core cannot substitute another source or symbol")
    if candles.filter(
        pl.col("available_at").is_null()
        | (pl.col("available_at") < pl.max_horizontal(CANDLE_TIMES[:-1]))
        | (pl.col("available_at") < pl.col("open_timestamp") + pl.duration(seconds=1))
    ).height:
        raise ValueError(
            "Validated candle availability precedes an original timestamp or candle close"
        )
    observed = candles.with_columns(
        (pl.col("open_timestamp") + pl.duration(seconds=1)).alias("observed_at")
    ).with_columns(pl.col("observed_at").dt.truncate("5m").alias("window_start"))
    markets = rows.select("market_id", "window_start").unique()
    raw = observed.join(markets, on="window_start", how="inner").rename(CANDLE_VALUES)
    # Match the original offline extractor's Float64 numeric representation;
    # validated archives retain Decimal prices/volumes instead of SQL doubles.
    raw = raw.with_columns(pl.col(name).cast(pl.Float64) for name in CANDLE_VALUES.values() if name != "trade_count")
    raw = (
        raw.with_columns(
            (pl.col("observed_at") - pl.col("window_start"))
            .dt.total_seconds()
            .alias("seconds_elapsed")
        )
        .filter(pl.col("seconds_elapsed").is_between(0, 259))
        .sort("market_id", "seconds_elapsed")
    )
    if raw.is_empty():
        return empty
    starts = raw.filter(pl.col("seconds_elapsed") == 0).select(
        "market_id",
        pl.col("btc_open").alias("opening_boundary"),
        pl.col("open_timestamp").alias("native_first_open_timestamp"),
    )
    raw = raw.join(starts, on="market_id", how="inner", validate="m:1").with_columns(
        pl.col("available_at").cum_max().over("market_id").alias("prefix_available_at"),
        pl.col("seconds_elapsed")
        .cum_count()
        .over("market_id")
        .cast(pl.Int64)
        .alias("native_prefix_rows"),
        pl.col("open_timestamp").alias("native_last_open_timestamp"),
        (pl.col("window_start") >= datetime(2026, 8, 14, tzinfo=UTC))
        .cast(pl.Float64)
        .alias("settlement_regime"),
    )
    if raw.is_empty():
        return empty
    native = derive_core_point_in_time_features(raw)
    missing = set(features) - set(native.columns)
    if missing:
        raise ValueError(f"Unsupported historical native feature contracts: {sorted(missing)}")
    trace = [
        "prefix_available_at",
        "native_first_open_timestamp",
        "native_last_open_timestamp",
        "native_prefix_rows",
    ]
    native = native.filter(pl.col("native_prefix_rows") == pl.col("seconds_elapsed") + 1)
    joined = rows.select("point_id", "market_id", "seconds_elapsed", "decision_at").join(
        native.select("market_id", "seconds_elapsed", *features, *trace),
        on=["market_id", "seconds_elapsed"],
        how="inner",
        validate="m:1",
    )
    return joined.filter(pl.col("prefix_available_at") <= pl.col("decision_at")).select(
        "point_id", *(pl.col(c).cast(pl.Float64) for c in features), *trace
    )


def score_reference(
    rows: pl.DataFrame, native: pl.DataFrame, payload: dict, cutoff: datetime, frozen: dict
) -> pl.DataFrame:
    """Retain all scheduled rows, including missing, in-sample and off-grid rejections."""
    features = payload["feature_names"]
    base = rows.select(
        *KEYS,
        "evaluation_clock_eligible",
        "up_vwap_5",
        "down_vwap_5",
        "up_book_age_seconds",
        "down_book_age_seconds",
    )
    joined = base.join(native, on="point_id", how="left", validate="1:1").with_columns(
        (pl.col("decision_at") > cutoff).alias("historical_cutoff_eligible"),
        (pl.col("seconds_elapsed") % 5 == 0).alias("native_grid_eligible"),
        pl.all_horizontal(pl.col(c).is_finite().fill_null(False) for c in features).alias(
            "feature_eligible"
        ),
    )
    eligible = joined.filter(
        pl.col("historical_cutoff_eligible")
        & pl.col("native_grid_eligible")
        & pl.col("evaluation_clock_eligible")
        & pl.col("feature_eligible")
    )
    predictions = (
        eligible.select("point_id")
        .head(0)
        .with_columns(pl.lit(None, dtype=pl.Float64).alias("probability_up"))
    )
    admitted_ids: list[str] = []
    executable_ids: list[str] = []
    if eligible.height:
        p = calibrate(
            np.clip(
                predict_estimator(payload["estimator"], matrix(eligible, features)), 1e-5, 1 - 1e-5
            ),
            payload["calibrator"],
            eligible["settlement_regime"].to_numpy(),
        )
        if not np.isfinite(p).all() or np.any((p < 0) | (p > 1)):
            raise ValueError("Invalid immutable historical probabilities")
        predictions = eligible.select("point_id").with_columns(pl.Series("probability_up", p))
        policy_input = eligible.with_columns(
            pl.col("decision_at").alias("observed_at"),
            pl.col("up_vwap_5").alias("up_ask_vwap_5"),
            pl.col("down_vwap_5").alias("down_ask_vwap_5"),
            pl.col("up_book_age_seconds").alias("pm_up_book_age_seconds"),
            pl.col("down_book_age_seconds").alias("pm_down_book_age_seconds"),
            pl.lit(frozen["fees"]["rate_assumption"]).alias("fee_rate"),
        )
        # The unchanged owner computes retrospective columns, but its fixed policy
        # reads only probability, decision cost, age, side and bucket. No fit runs.
        opportunities_frame = opportunities(policy_input, p, NAME)
        point_keys = eligible.select("point_id", "market_id", "seconds_elapsed")
        executable_ids = opportunities_frame.join(point_keys, on=["market_id", "seconds_elapsed"])[
            "point_id"
        ].to_list()
        admitted_ids = (
            apply_policies(opportunities_frame, payload["bucket_policies"], True)
            .join(point_keys, on=["market_id", "seconds_elapsed"])["point_id"]
            .to_list()
        )
    return joined.join(predictions, on="point_id", how="left", validate="1:1").with_columns(
        (1 - pl.col("probability_up")).alias("probability_down"),
        pl.when(pl.col("probability_up").is_null())
        .then(pl.lit(None, dtype=pl.String))
        .when(pl.col("probability_up") >= 0.5)
        .then(pl.lit("up"))
        .otherwise(pl.lit("down"))
        .alias("chosen_side"),
        pl.col("point_id").is_in(admitted_ids).alias("admitted"),
        pl.when(~pl.col("evaluation_clock_eligible"))
        .then(pl.lit("purged_boundary_market"))
        .when(~pl.col("historical_cutoff_eligible"))
        .then(pl.lit("historical_fit_calibration_policy_cutoff"))
        .when(~pl.col("native_grid_eligible"))
        .then(pl.lit("outside_native_five_second_grid"))
        .when(~pl.col("feature_eligible"))
        .then(pl.lit("missing_causal_native_complete_prefix_or_features"))
        .when(~pl.col("point_id").is_in(executable_ids))
        .then(pl.lit("missing_native_q5_decision_book"))
        .when(pl.col("point_id").is_in(admitted_ids))
        .then(pl.lit("admitted"))
        .otherwise(pl.lit("native_frozen_policy_rejected_or_later_attempt"))
        .alias("admission_reason"),
        pl.lit(CANDIDATE).alias("candidate"),
        pl.lit("historical_reference_only").alias("qualification_class"),
    )


def replay_reference(
    predictions: pl.DataFrame, index: dict, payload: dict, frozen: dict
) -> pl.DataFrame:
    """Reuse existing selection and FAK/FOK ledgers; never retry an unknown first attempt."""
    pieces = []
    for view in ["bucket_diagnostic", "sequential_policy"]:
        attempts = chosen_rows(predictions, view).with_columns(
            (pl.col("seconds_elapsed") // 10 * 10).alias("native_bucket")
        )
        for (bucket,), rows in attempts.partition_by("native_bucket", as_dict=True).items():
            max_price = payload["bucket_policies"][bucket][2]
            for quantity in [5] if view == "bucket_diagnostic" else frozen["quantity_grid"]:
                for kind in ["FAK", "FOK"]:
                    for scenario in (
                        [{"name": "base"}] if view == "bucket_diagnostic" else frozen["scenarios"]
                    ):
                        pieces.append(
                            replay_rows(
                                rows, index, frozen, quantity, kind, scenario, max_price
                            ).with_columns(pl.lit(view).alias("economic_view"))
                        )
    schema = {**LEDGER_SCHEMA, "economic_view": pl.String}
    result = pl.concat(pieces, how="vertical_relaxed") if pieces else pl.DataFrame(schema=schema)
    return result.with_columns(
        pl.lit(CANDIDATE).alias("candidate"),
        pl.lit("historical_reference_only").alias("qualification_class"),
    )


def execute(run: Path) -> None:
    """Explicit offline entry point; primary dispatches after current model selection ends."""
    frozen = config(run)
    if (
        json.loads((run / "manifests/evaluation-predictions.json").read_text())["status"]
        != "complete"
    ):
        raise ValueError("Freeze all current challenger predictions before historical diagnostics")
    payload, contract = load_reference(run)
    manifest_names = [
        "core-panels.json",
        f"flow-source-validation-{PRODUCT}.json",
        "core-source-validation-polymarket_clob_l2.json",
        "evaluation-predictions.json",
    ]
    manifests = {n: json.loads((run / "manifests" / n).read_text()) for n in manifest_names}
    if (
        manifests["core-panels.json"]["status"] != "complete"
        or manifests[f"flow-source-validation-{PRODUCT}.json"]["status"] != "completed"
    ):
        raise ValueError("Historical reconstruction requires completed validated source panels")
    identity = {
        "contract": {
            **contract,
            "strictly_after": contract["strictly_after"].isoformat(),
            "fit_calibration_policy_end_exclusive": TRAINING_END.isoformat(),
        },
        "configuration": sha256(run / "inputs/tournament-freeze.json"),
        "historical_freeze": sha256(run / "inputs/historical-artifact-freeze.json"),
        "manifests": {n: sha256(run / "manifests" / n) for n in manifest_names},
        "code": {
            n: sha256(Path(__file__).with_name(n))
            for n in [
                "time_bucket_historical.py",
                "time_bucket_execution.py",
                "time_bucket_replay.py",
                "time_bucket_evaluation.py",
                "fok_retry_backtest.py",
                "core_execution.py",
            ]
        },
    }
    path = run / "manifests/historical-native-diagnostics.json"
    report = (
        json.loads(path.read_text())
        if path.exists()
        else {"identity": identity, "status": "in_progress", "days": []}
    )
    if report["identity"] != identity:
        raise ValueError("Historical diagnostic checkpoint identity changed")
    done = {r["date"]: r for r in report["days"]}
    candle_files = {
        r["date"]: r for r in manifests[f"flow-source-validation-{PRODUCT}.json"]["daily"]
    }
    for record in manifests["core-panels.json"]["days"]:
        day = record["date"]
        fold = next(
            (f for f in frozen["folds"] if f["evaluation_start"] <= day < f["evaluation_end"]), None
        )
        if fold is None:
            continue
        if day in done:
            if any(sha256(Path(r["path"])) != r["sha256"] for r in done[day]["outputs"]):
                raise ValueError("Historical diagnostic output checksum mismatch")
            continue
        if sha256(Path(record["path"])) != record["sha256"]:
            raise ValueError("Core panel checksum mismatch")
        panel = pl.read_parquet(record["path"])
        ids = split_rows(panel, frozen, fold, "evaluation")["point_id"].implode()
        rows = panel.filter(pl.col("seconds_elapsed") < 260).with_columns(
            pl.col("point_id").is_in(ids).alias("evaluation_clock_eligible")
        )
        candles = []
        for source_day in [date.fromisoformat(day) - timedelta(days=1), date.fromisoformat(day)]:
            source = candle_files.get(str(source_day))
            if source:
                if sha256(Path(source["output_path"])) != source["output_sha256"]:
                    raise ValueError("Validated candle checksum mismatch")
                candles.append(
                    pl.read_parquet(
                        source["output_path"],
                        columns=["source", "symbol", *CANDLE_TIMES, *CANDLE_VALUES],
                    )
                )
        native = reconstruct_native(
            rows, pl.concat(candles) if candles else pl.DataFrame(), payload["feature_names"]
        )
        predictions = score_reference(rows, native, payload, contract["strictly_after"], frozen)
        index = day_books(run, date.fromisoformat(day), rows["market_id"].unique().to_list())
        ledger = replay_reference(predictions, index, payload, frozen)
        outputs = []
        for directory, frame in [("predictions", predictions), ("trades", ledger)]:
            target = run / directory / "historical-native" / NAME / f"{day}.parquet"
            if target.exists():
                raise ValueError("Unrecorded historical output must not be overwritten")
            target.parent.mkdir(parents=True, exist_ok=True)
            frame.write_parquet(target, compression="zstd")
            outputs.append({"path": str(target), "sha256": sha256(target), "rows": frame.height})
        report["days"].append(
            {
                "date": day,
                "outputs": outputs,
                "rejections": predictions.group_by("bucket_start", "admission_reason")
                .len()
                .to_dicts(),
            }
        )
        write_json(path, report)
        levels.cache_clear()
        print(
            json.dumps({"historical_reference_date": day, "predictions": predictions.height}),
            flush=True,
        )
    report["status"] = "complete_with_explicit_partial_range_exclusions"
    write_json(path, report)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("run", type=Path)
    args = parser.parse_args()
    execute(checked_run(args.run))


if __name__ == "__main__":
    main()
