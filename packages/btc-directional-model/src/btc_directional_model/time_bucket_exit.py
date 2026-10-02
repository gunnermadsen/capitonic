"""Offline sell-versus-settlement targets and causal partial-exit decisions."""

from __future__ import annotations

import argparse
import json
from datetime import date
from pathlib import Path

import numpy as np
import polars as pl

from .time_bucket_execution import day_books, levels, order, settlement
from .time_bucket_protocol import config
from .time_bucket_source_audit import checked_run, sha256, write_json
from .time_bucket_training import regress


def labels(query: pl.DataFrame, index: dict, frozen: dict) -> pl.DataFrame:
    output = []
    for row in query.select("point_id", "market_id", "decision_at", "official_outcome").iter_rows(named=True):
        for side in ["up", "down"]:
            for kind in ["FAK", "FOK"]:
                sale = order(index, row["market_id"], side, row["decision_at"], 5, kind, frozen, action="sell")
                advantage = None
                if sale["filled_quantity"] > 0:
                    advantage = (sale["share_price"] - sale["fee_per_share"] - sale["reserve_per_share"]
                                 - sale["stress_extra_per_share"] - int(side == row["official_outcome"]))
                output.append({"point_id": row["point_id"], "side_is_up": float(side == "up"),
                               "order_type": kind, "evidence_known": sale["evidence_known"],
                               "filled_quantity": sale["filled_quantity"], "sell_advantage_label": advantage})
    return pl.DataFrame(output, schema={"point_id": pl.String, "side_is_up": pl.Float64, "order_type": pl.String,
                                       "evidence_known": pl.Boolean, "filled_quantity": pl.Float64,
                                       "sell_advantage_label": pl.Float64})


def build(run: Path) -> None:
    frozen = config(run)
    path = run / "manifests/exit-labels.json"
    identity = {"code": {name: sha256(Path(__file__).with_name(name)) for name in [
        "time_bucket_exit.py", "time_bucket_execution.py", "time_bucket_protocol.py", "core_execution.py", "fok_retry_backtest.py"]},
        "core_panel_manifest": sha256(run / "manifests/core-panels.json"),
        "source_audit": sha256(run / "manifests/core-source-validation-polymarket_clob_l2.json"),
        "configuration": sha256(run / "inputs/tournament-freeze.json")}
    record = json.loads(path.read_text()) if path.exists() else {"days": [], "status": "in_progress",
        "identity": identity}
    if record.get("identity") != identity:
        raise ValueError("Existing exit surfaces need explicit implementation compatibility review")
    done = {x["date"]: x for x in record["days"]}
    for core in sorted((run / "datasets/core").glob("*.parquet")):
        if core.stem in done:
            if sha256(Path(done[core.stem]["path"])) != done[core.stem]["sha256"]:
                raise ValueError("Existing exit labels changed")
            continue
        query = pl.read_parquet(core, columns=["point_id", "market_id", "decision_at", "official_outcome"])
        index = day_books(run, date.fromisoformat(core.stem), query["market_id"].unique().to_list())
        result = labels(query, index, frozen)
        target = run / "datasets/exit-labels" / core.name
        target.parent.mkdir(parents=True, exist_ok=True)
        result.write_parquet(target, compression="zstd")
        record["days"].append({"date": core.stem, "path": str(target), "sha256": sha256(target),
                               "rows": result.height, "known": result["evidence_known"].sum()})
        write_json(path, record)
        levels.cache_clear()
        print(json.dumps(record["days"][-1]), flush=True)
    record["status"] = "complete"
    write_json(path, record)


def fit_exit(train: pl.DataFrame, target_rows: pl.DataFrame, features: list[str], frozen: dict) -> tuple:
    joined = train.join(target_rows.filter((pl.col("order_type") == "FAK") & pl.col("sell_advantage_label").is_not_null()),
                        on="point_id", how="inner", validate="1:m")
    if joined["market_id"].n_unique() < frozen["calibration"]["minimum_fit_markets"]:
        raise ValueError("Insufficient causal sell targets for required exit checkpoint")
    head_features = features + ["side_is_up"]
    return regress(joined, head_features, "sell_advantage_label", frozen), head_features


def sell_policy(buy: dict, subsequent: pl.DataFrame, side: str, won: bool, index: dict,
                bundle: tuple | None, policy: dict | None, frozen: dict, kind: str,
                scenario: dict, *, control: str = "learned") -> dict:
    if buy["filled_quantity"] <= 0 or control == "hold":
        return {**settlement(buy, won), "sales": [], "exit_evidence_known": True}
    if control == "learned" and (bundle is None or policy is None):
        return {**settlement(buy, won), "sales": [], "exit_evidence_known": True}
    selected = subsequent.sort("decision_at")
    if control == "fixed_180":
        selected = selected.filter(pl.col("seconds_elapsed") >= 180).head(1)
    else:
        model, features = bundle
        selected = selected.with_columns(pl.lit(float(side == "up")).alias("side_is_up"))
        finite = np.isfinite(selected.select(features).to_numpy()).all(axis=1)
        selected = selected.filter(pl.Series(finite))
        if selected.height:
            selected = selected.with_columns(pl.Series("sell_advantage", model.predict(selected.select(features).to_numpy())))
            selected = selected.filter(pl.col("sell_advantage") >= policy["minimum_advantage"]).head(1)
    # A single conservative post-entry order; unsold shares remain held to settlement.
    sales = []
    known = True
    for row in selected.select("market_id", "decision_at").iter_rows(named=True):
        fraction = 1.0 if control == "fixed_180" else policy["fraction"]
        sale = order(index, row["market_id"], side, row["decision_at"], buy["filled_quantity"] * fraction,
                     kind, frozen, action="sell", scenario=scenario)
        known = sale["evidence_known"]
        sales.append(sale)
    economics = settlement(buy, won, sales)
    if not known:
        # Unknown execution cannot be valued as a proven hold-to-close result.
        economics = {"net_pnl": None, "stress_pnl": None, "residual_quantity": None}
    return {**economics, "sales": sales, "exit_evidence_known": known}


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("run", type=Path)
    args = parser.parse_args()
    build(checked_run(args.run))


if __name__ == "__main__":
    main()
