"""Independent FAK/FOK capacity and stress replay of frozen OOS decisions."""

from __future__ import annotations

import argparse
import json
from datetime import date
from pathlib import Path

import joblib
import polars as pl

from .time_bucket_execution import day_books, levels
from .time_bucket_protocol import config
from .time_bucket_replay import LEDGER_SCHEMA, replay_rows
from .time_bucket_source_audit import checked_run, sha256, write_json
from .time_bucket_tournament import arms_at_freeze


def chosen_rows(predictions: pl.DataFrame, view: str, *, offset: int | None = None,
                general_market: bool = False) -> pl.DataFrame:
    flag = "general_market_diagnostic_admitted" if general_market else "admitted"
    selected = predictions.filter(pl.col(flag)).sort("decision_at", "market_id")
    if offset is not None:
        selected = selected.filter(pl.col("entry_offset") == offset)
    return selected.unique(["market_id", "bucket_start"] if view == "bucket_diagnostic" else ["market_id"],
                           keep="first", maintain_order=True)


def replay(run: Path) -> None:
    frozen = config(run)
    arms = {(arm["candidate"], arm["arm"]): arm for arm in arms_at_freeze(run)}
    prediction_manifest_path = run / "manifests/evaluation-predictions.json"
    predictions = json.loads(prediction_manifest_path.read_text())
    if predictions["status"] != "complete":
        raise ValueError("Freeze all OOS predictions before reading evaluation economics")
    identity = {"prediction_manifest": sha256(prediction_manifest_path),
                "source_books": sha256(run / "manifests/core-source-validation-polymarket_clob_l2.json"),
                "configuration": sha256(run / "inputs/tournament-freeze.json"),
                "replay_scope": sha256(run / "inputs/replay-scope.json"),
                "code": {name: sha256(Path(__file__).with_name(name)) for name in [
                    "time_bucket_evaluation.py", "time_bucket_replay.py", "time_bucket_execution.py", "time_bucket_exit.py",
                    "core_execution.py", "fok_retry_backtest.py"]}}
    manifest_path = run / "manifests/execution-replay.json"
    manifest = json.loads(manifest_path.read_text()) if manifest_path.exists() else {"identity": identity, "status": "in_progress", "outputs": []}
    if manifest["identity"] != identity:
        raise ValueError("Frozen replay semantics changed")
    done = {(r["candidate"], r["arm"], r["date"]): r for r in manifest["outputs"]}
    dates = sorted({record["date"] for record in predictions["days"]})
    for day in dates:
        records = [record for record in predictions["days"] if record["date"] == day]
        core = pl.read_parquet(run / "datasets/core" / f"{day}.parquet")
        index = day_books(run, date.fromisoformat(day), core["market_id"].unique().to_list())
        for record in records:
            key = (record["candidate"], record["arm"], day)
            if key in done:
                if sha256(Path(done[key]["path"])) != done[key]["sha256"]:
                    raise ValueError("Existing replay ledger changed")
                continue
            if sha256(Path(record["path"])) != record["sha256"]:
                raise ValueError("Prediction identity changed before execution replay")
            frame = pl.read_parquet(record["path"])
            arm = arms[key[:2]]
            fold = frame["fold"][0] if frame.height else next(f["name"] for f in frozen["folds"] if f["evaluation_start"] <= day < f["evaluation_end"])
            checkpoint = run / "checkpoints" / record["candidate"] / record["arm"] / fold
            fit = json.loads((checkpoint / "manifest.json").read_text())
            bundle = joblib.load(checkpoint / "model-bundle.joblib") if fit["status"] == "complete" else None
            pieces = []
            if bundle and bundle["policy"]:
                control = "learned" if bundle.get("exit") else "hold"
                for view in ["bucket_diagnostic", "sequential_policy"]:
                    attempts = chosen_rows(frame, view)
                    quantities = [5] if view == "bucket_diagnostic" else frozen["quantity_grid"]
                    scenarios = [{"name": "base"}] if view == "bucket_diagnostic" else frozen["scenarios"]
                    controls = [control, "hold", "fixed_180"] if bundle.get("exit") else [control]
                    for kind in ["FAK", "FOK"]:
                        for quantity in quantities:
                            for scenario in scenarios:
                                for entry_control in controls:
                                    ledger = replay_rows(attempts, index, frozen, quantity, kind, scenario,
                                        bundle["policy"]["maximum_share_cost"], exit_bundle=bundle.get("exit"),
                                        exit_policy=bundle.get("exit_policy"), exit_rows=core, control=entry_control)
                                    pieces.append(ledger.with_columns(pl.lit(view).alias("economic_view"),
                                        pl.lit("selected_wait_policy").alias("timing_control"),
                                        pl.lit("frozen_candidate_population").alias("population_control")))
                if len(arm["entry_offsets"]) > 1:
                    for offset in frozen["entry_offsets"]:
                        for kind in ["FAK", "FOK"]:
                            attempts = chosen_rows(frame, "sequential_policy", offset=offset)
                            ledger = replay_rows(attempts, index, frozen, 5, kind, {"name": "base"}, bundle["policy"]["maximum_share_cost"])
                            pieces.append(ledger.with_columns(pl.lit("sequential_policy").alias("economic_view"),
                                pl.lit(f"fixed_offset_{offset}").alias("timing_control"),
                                pl.lit("frozen_candidate_population").alias("population_control")))
                if arm.get("quiet_only"):
                    for kind in ["FAK", "FOK"]:
                        attempts = chosen_rows(frame, "sequential_policy", general_market=True)
                        ledger = replay_rows(attempts, index, frozen, 5, kind, {"name": "base"}, bundle["policy"]["maximum_share_cost"])
                        pieces.append(ledger.with_columns(pl.lit("sequential_policy").alias("economic_view"),
                            pl.lit("selected_wait_policy").alias("timing_control"),
                            pl.lit("general_market_diagnostic").alias("population_control")))
            ledger = pl.concat(pieces, how="vertical_relaxed") if pieces else pl.DataFrame(schema={**LEDGER_SCHEMA,
                "economic_view": pl.String, "timing_control": pl.String, "population_control": pl.String})
            ledger = ledger.with_columns(pl.lit(record["candidate"]).alias("candidate"), pl.lit(record["arm"]).alias("arm"),
                pl.lit(fold).alias("fold"), pl.lit(day).alias("date"), pl.lit(arm["primary"]).alias("primary_arm"),
                pl.lit(bundle["selected_quantity"] if bundle else None, dtype=pl.Float64).alias("calibration_selected_quantity"))
            target = run / "trades/evaluation" / record["candidate"] / record["arm"] / f"{day}.parquet"
            target.parent.mkdir(parents=True, exist_ok=True)
            if target.exists():
                raise ValueError("Unrecorded replay output cannot be overwritten")
            ledger.write_parquet(target, compression="zstd")
            result = {"candidate": record["candidate"], "arm": record["arm"], "date": day,
                      "path": str(target), "sha256": sha256(target), "rows": ledger.height,
                      "prediction_sha256": record["sha256"], "checkpoint_status": fit["status"]}
            manifest["outputs"].append(result)
            write_json(manifest_path, manifest)
        levels.cache_clear()
        print(json.dumps({"replayed_date": day, "arms": len(records)}), flush=True)
    manifest["status"] = "complete"
    write_json(manifest_path, manifest)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("run", type=Path)
    args = parser.parse_args()
    replay(checked_run(args.run))


if __name__ == "__main__":
    main()
