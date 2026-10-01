"""Tournament ledgers keep bucket counterfactuals separate from sequential income."""

from __future__ import annotations

import json
from pathlib import Path

import polars as pl

from .time_bucket_execution import day_books, levels, order, settlement
from .time_bucket_exit import sell_policy
from .time_bucket_policy import economic_metrics, selected_attempts

LEDGER_SCHEMA = {
    "point_id": pl.String, "market_id": pl.String, "decision_at": pl.Datetime("us", "UTC"),
    "bucket_start": pl.Int64, "chosen_side": pl.String, "order_type": pl.String,
    "scenario": pl.String, "control": pl.String, "requested_quantity": pl.Float64,
    "filled_quantity": pl.Float64, "fill_ratio": pl.Float64, "share_price": pl.Float64,
    "fee_per_share": pl.Float64, "reserve_per_share": pl.Float64, "filled_notional": pl.Float64,
    "net_pnl": pl.Float64, "stress_pnl": pl.Float64, "residual_quantity": pl.Float64,
    "evidence_known": pl.Boolean, "exit_evidence_known": pl.Boolean, "reason": pl.String,
    "arrival_at": pl.Datetime("us", "UTC"), "decision_book_sha256": pl.String,
    "arrival_book_sha256": pl.String, "sales_json": pl.String,
}


def replay_rows(attempts: pl.DataFrame, index: dict, frozen: dict, quantity: float, kind: str,
                scenario: dict, max_price: float, *, exit_bundle: tuple | None = None,
                exit_policy: dict | None = None, exit_rows: pl.DataFrame | None = None,
                control: str = "hold") -> pl.DataFrame:
    records = []
    for row in attempts.iter_rows(named=True):
        buy = order(index, row["market_id"], row["chosen_side"], row["decision_at"], quantity,
                    kind, frozen, max_price=max_price, scenario=scenario)
        won = row["chosen_side"] == row["official_outcome"]
        economics = {**settlement(buy, won), "exit_evidence_known": True, "sales": []}
        if buy["evidence_known"] and buy["filled_quantity"] > 0 and control != "hold":
            if exit_rows is None:
                raise ValueError("Sell replay requires the causal post-entry feature observations")
            subsequent = exit_rows.filter((pl.col("market_id") == row["market_id"])
                                          & (pl.col("decision_at") > buy["arrival_at"]))
            economics = sell_policy(buy, subsequent, row["chosen_side"], won, index,
                                    exit_bundle, exit_policy, frozen, kind, scenario, control=control)
        if not buy["evidence_known"]:
            economics.update(net_pnl=None, stress_pnl=None, residual_quantity=None)
        record = {name: buy.get(name) for name in LEDGER_SCHEMA}
        record.update(point_id=row["point_id"], market_id=row["market_id"], decision_at=row["decision_at"],
                      bucket_start=row["bucket_start"], chosen_side=row["chosen_side"],
                      order_type=kind, scenario=scenario["name"], control=control,
                      net_pnl=economics["net_pnl"], stress_pnl=economics["stress_pnl"],
                      residual_quantity=economics["residual_quantity"], exit_evidence_known=economics["exit_evidence_known"],
                      sales_json=json.dumps(economics["sales"], default=str, sort_keys=True))
        records.append(record)
    return pl.DataFrame(records, schema=LEDGER_SCHEMA)


def calibration_replay(run: Path, scored: pl.DataFrame, policy: dict | None, frozen: dict,
                        quantities: list[float], *, exit_bundle: tuple | None = None,
                        exit_policy: dict | None = None, exit_rows: pl.DataFrame | None = None,
                        control: str = "hold") -> list[dict]:
    attempts = selected_attempts(scored, policy, frozen)
    by_quantity: dict[float, list[pl.DataFrame]] = {q: [] for q in quantities}
    for day_key, rows in attempts.with_columns(pl.col("decision_at").dt.date().alias("date")).partition_by("date", as_dict=True).items():
        day = day_key[0]
        index = day_books(run, day, rows["market_id"].unique().to_list())
        for q in quantities:
            result = replay_rows(rows, index, frozen, q, "FAK", {"name": "base"}, policy["maximum_share_cost"],
                                 exit_bundle=exit_bundle, exit_policy=exit_policy, exit_rows=exit_rows, control=control)
            by_quantity[q].append(result)
        levels.cache_clear()
    records = []
    for q, frames in by_quantity.items():
        result = pl.concat(frames) if frames else pl.DataFrame(schema=LEDGER_SCHEMA)
        records.append({"quantity": q, **economic_metrics(result)})
    return records


def select_quantity(records: list[dict], frozen: dict) -> float:
    # A tie keeps the principal Q5 anchor, then the smaller neighboring order.
    ordered = sorted(records, key=lambda row: (-row["stress_pnl"], -(row["stressed_profit_factor"] or 0),
                                               abs(row["quantity"] - frozen["primary_quantity"]), row["quantity"]))
    return ordered[0]["quantity"]


def choose_exit(run: Path, scored: pl.DataFrame, entry_policy: dict | None, frozen: dict,
                head: tuple, exit_rows: pl.DataFrame) -> tuple[dict | None, list[dict]]:
    feature_contract = json.loads((run / "inputs/candidate-feature-freeze.json").read_text())
    contract = feature_contract["exit_selection"]
    choices = [None] + [{"minimum_advantage": threshold, "fraction": fraction}
                       for threshold in contract["thresholds"] for fraction in contract["fractions"]]
    if len(choices) > contract["maximum_exit_policies_after_entry_selection"]:
        raise ValueError("Frozen sell-policy budget exceeded")
    attempts = selected_attempts(scored, entry_policy, frozen)
    collected: dict[int, list[pl.DataFrame]] = {i: [] for i in range(len(choices))}
    for key, rows in attempts.with_columns(pl.col("decision_at").dt.date().alias("date")).partition_by("date", as_dict=True).items():
        index = day_books(run, key[0], rows["market_id"].unique().to_list())
        for i, choice in enumerate(choices):
            collected[i].append(replay_rows(rows, index, frozen, 5, "FAK", {"name": "base"},
                                entry_policy["maximum_share_cost"], exit_bundle=head, exit_policy=choice,
                                exit_rows=exit_rows, control="hold" if choice is None else "learned"))
        levels.cache_clear()
    records = []
    for i, frames in collected.items():
        result = pl.concat(frames) if frames else pl.DataFrame(schema=LEDGER_SCHEMA)
        sales = sum(len(json.loads(value)) for value in result["sales_json"]) if result.height else 0
        records.append({"policy": choices[i], "sell_attempts": sales, **economic_metrics(result)})
    hold_unknown = records[0]["unknown_attempts"]
    for record in records:
        record["selection_eligible"] = record["unknown_attempts"] == hold_unknown
    records.sort(key=lambda row: (-row["stress_pnl"], -(row["stressed_profit_factor"] or 0),
                                 row["sell_attempts"], str(row["policy"])))
    return next(row["policy"] for row in records if row["selection_eligible"]), records
