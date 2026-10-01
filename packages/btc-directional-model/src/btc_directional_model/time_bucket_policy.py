"""Frozen calibration-only admission and first-attempt sequential selection."""

from __future__ import annotations

from itertools import product

import numpy as np
import polars as pl


def economic_metrics(frame: pl.DataFrame) -> dict:
    evidence = pl.col("evidence_known") & pl.col("net_pnl").is_finite() & pl.col("stress_pnl").is_finite()
    if "exit_evidence_known" in frame:
        evidence &= pl.col("exit_evidence_known")
    known = frame.filter(evidence) if frame.height else frame
    filled = known.filter(pl.col("filled_quantity") > 0) if known.height else known
    values = filled["stress_pnl"].to_numpy() if filled.height else np.array([])
    positive, negative = values[values > 0], values[values < 0]
    gain, loss = float(positive.sum()), float(-negative.sum())
    pf = gain / loss if loss else (None if not gain else float("inf"))
    recovery = float(-negative.mean() / positive.mean()) if len(negative) and len(positive) else (0.0 if len(positive) else None)
    return {"attempts": frame.height, "unknown_attempts": frame.height - known.height,
            "observed_entry_fills": frame.filter(pl.col("evidence_known") & (pl.col("filled_quantity") > 0)).height if frame.height else 0,
            "fills": filled.height, "markets": filled["market_id"].n_unique() if filled.height else 0,
            "net_pnl": float(filled["net_pnl"].sum()) if filled.height else 0.0,
            "stress_pnl": float(values.sum()), "stressed_profit_factor": pf,
            "recovery_ratio": recovery,
            "filled_shares": float(filled["filled_quantity"].sum()) if filled.height else 0.0,
            "requested_shares": float(known["requested_quantity"].sum()) if known.height else 0.0,
            "win_rate": float(np.mean(values > 0)) if len(values) else None,
            "average_stressed_win": float(positive.mean()) if len(positive) else None,
            "average_stressed_loss": float(negative.mean()) if len(negative) else None}


def intentions(frame: pl.DataFrame, policy: dict | None, frozen: dict) -> pl.DataFrame:
    """Admission uses decision-time fields only, never arrival fill or final outcome."""
    if policy is None:
        return frame.with_columns(pl.lit(False).alias("admitted"), pl.lit("policy_abstain").alias("admission_reason"),
                                  pl.lit(None, dtype=pl.String).alias("chosen_side"),
                                  pl.lit(None, dtype=pl.Float64).alias("expected_stressed_value_per_share"))
    probability = pl.col("probability_up")
    conservative = pl.col("conservative_probability_up")
    prediction_known = (probability.is_finite() & conservative.is_finite()).fill_null(False)
    if "expected_value_up" in frame and "expected_value_down" in frame:
        prediction_known &= (pl.col("expected_value_up").is_finite() & pl.col("expected_value_down").is_finite()).fill_null(False)
        choose_up = pl.col("expected_value_up") >= pl.col("expected_value_down")
    else:
        choose_up = probability >= 0.5
    result = frame.with_columns(pl.when(~prediction_known).then(pl.lit(None, dtype=pl.String))
                                .when(choose_up).then(pl.lit("up")).otherwise(pl.lit("down")).alias("chosen_side"))
    up = pl.col("chosen_side") == "up"
    price = pl.when(up).then(pl.col("up_decision_partial_vwap_5")).otherwise(pl.col("down_decision_partial_vwap_5"))
    limit = pl.when(up).then(pl.col("up_limit_5")).otherwise(pl.col("down_limit_5"))
    p = pl.when(up).then(probability).otherwise(1 - probability)
    cp = pl.when(up).then(conservative).otherwise(1 - conservative)
    price, limit, p, cp = [pl.when(prediction_known).then(value).otherwise(pl.lit(None, dtype=pl.Float64))
                           for value in [price, limit, p, cp]]
    fee = frozen["fees"]["rate_assumption"] * price * (1 - price)
    edge = cp - price - fee - frozen["book"]["reserve_per_share_per_leg"] - frozen["book"]["stress_extra_per_share_per_leg"]
    if "expected_value_up" in frame and "expected_value_down" in frame:
        edge = pl.when(prediction_known).then(
            pl.when(up).then(pl.col("expected_value_up")).otherwise(pl.col("expected_value_down")))
    result = result.with_columns(p.alias("chosen_probability"), price.alias("decision_share_cost"),
                                 edge.alias("expected_stressed_value_per_share"),
                                 (cp - price).alias("expected_gross_value_per_share"))
    reason = (pl.when(~prediction_known).then(pl.lit("unavailable_prediction"))
              .when(price.is_null() | limit.is_null()).then(pl.lit("unknown_decision_book"))
              .when(~pl.col("bucket_start").is_in(policy["bucket_mask"])).then(pl.lit("policy_bucket_disabled"))
              .when(p < policy["confidence"]).then(pl.lit("confidence_below_threshold"))
              .when(limit > policy["maximum_share_cost"]).then(pl.lit("decision_limit_above_price_cap"))
              .when(edge.is_null() | (edge < policy["minimum_stressed_edge"])).then(pl.lit("insufficient_stressed_value"))
              .otherwise(pl.lit("admit")))
    return result.with_columns(reason.alias("admission_reason")).with_columns(
        (pl.col("admission_reason") == "admit").alias("admitted"))


def selected_attempts(scored: pl.DataFrame, policy: dict | None, frozen: dict,
                      *, diagnostic: bool = False) -> pl.DataFrame:
    selected = intentions(scored, policy, frozen).filter(pl.col("admitted")).sort("decision_at", "market_id")
    # A missing arrival does not authorize a later retry as though the first order did not fill.
    keys = ["market_id", "bucket_start"] if diagnostic else ["market_id"]
    return selected.unique(keys, keep="first", maintain_order=True)


def cached_q5(attempts: pl.DataFrame, kind: str = "FAK") -> pl.DataFrame:
    side = pl.col("chosen_side") == "up"
    mapping = {"evidence_known": "evidence_known", "filled_quantity": "quantity_5",
               "share_price": "price_5", "net_pnl": "net_5", "stress_pnl": "stress_5"}
    columns = [pl.lit(5.0).alias("requested_quantity")]
    for target, suffix in mapping.items():
        columns.append(pl.when(side).then(pl.col(f"up_{kind.lower()}_{suffix}"))
                       .otherwise(pl.col(f"down_{kind.lower()}_{suffix}")).alias(target))
    return attempts.with_columns(columns)


def choose_policy(calibration: pl.DataFrame, frozen: dict) -> tuple[dict | None, list[dict]]:
    """Bounded Q5 FAK selection; the unchanged policy is also replayed independently as FOK."""
    # Complete execution labels are required for calibration comparisons, not for live admission.
    covered_markets = calibration.group_by("market_id").agg(
        (pl.col("up_fak_evidence_known") & pl.col("down_fak_evidence_known")).all().alias("known"))
    panel = calibration.join(covered_markets.filter(pl.col("known")).select("market_id"), on="market_id")
    days = panel["window_start"].dt.date().n_unique()
    baseline = {"policy": None, **economic_metrics(cached_q5(selected_attempts(panel, None, frozen))),
                "covered_calibration_days": days, "selection_kind": "FAK"}
    records = [baseline]
    p = frozen["policy_search"]
    for confidence, edge, cost, mask in product(p["confidence"], p["minimum_stressed_edge"],
                                               p["maximum_share_cost"], p["bucket_masks"]):
        policy = {"confidence": confidence, "minimum_stressed_edge": edge,
                  "maximum_share_cost": cost, "bucket_mask": mask}
        metrics = economic_metrics(cached_q5(selected_attempts(panel, policy, frozen)))
        records.append({"policy": policy, **metrics, "covered_calibration_days": days, "selection_kind": "FAK"})
    if len(records) > p["maximum_policies_per_fit"]:
        raise ValueError("Frozen admission search budget exceeded")

    def ordering(row: dict) -> tuple:
        pf = row["stressed_profit_factor"] or 0
        return (-row["stress_pnl"], -pf, abs(row["fills"] / max(days, 1) - 6.5),
                row["fills"], str(row["policy"]))

    records.sort(key=ordering)
    return records[0]["policy"], records
