"""Known quiet states from chronological OOS simulated refresh activity only."""

from __future__ import annotations

import numpy as np
import polars as pl

from .time_bucket_candidates import QUIET_FIELDS


def activity_states(query: pl.DataFrame, reference: pl.DataFrame, frozen: dict) -> pl.DataFrame:
    """An absent model, market, prediction, or execution observation is never quiet."""
    n = query.height
    columns = {name: np.full(n, np.nan) for name in QUIET_FIELDS}
    columns["reference_availability_known"] = np.zeros(n, dtype=bool)
    if reference.is_empty():
        return pl.DataFrame(columns).with_columns(pl.col(pl.Float64).fill_nan(None))
    if reference["point_id"].n_unique() != reference.height:
        raise ValueError("Duplicate reference decisions invalidate activity counts")
    reference = reference.sort("decision_at")
    decision = reference["decision_at"].cast(pl.Int64).to_numpy()
    available = reference["evidence_available_at"].cast(pl.Int64).to_numpy()
    times = query["decision_at"].cast(pl.Int64).to_numpy()
    known = reference["reference_known"].to_numpy()
    fills = reference["filled_quantity"].fill_null(0).to_numpy() > 0
    # Nulls above only participate under an explicit known mask, never establishing quiet.
    admitted = reference["selected_attempt"].fill_null(False).to_numpy()
    probability = reference["probability_up"].to_numpy()
    if np.any(known & (~np.isfinite(probability) | ~np.isfinite(reference["filled_quantity"].to_numpy()))):
        raise ValueError("Known reference activity requires finite predictions and observed fill quantities")
    horizon = frozen["reference"]["quiet_seconds"]
    expected = int(horizon / 300) * len(frozen["bucket_starts"])
    for i, when in enumerate(times):
        left = np.searchsorted(decision, when - horizon * 1_000_000, side="left")
        right = np.searchsorted(decision, when, side="left")
        if right - left != expected or right == left:
            continue
        # The decision grid itself is verified; repeated timestamps cannot replace a missing slot.
        window = decision[left:right]
        elapsed = (window // 1_000_000) % 300
        required = np.array(frozen["bucket_starts"]) + frozen["regular_entry_offset"]
        if len(np.unique(window)) != expected or not np.isin(elapsed, required).all():
            continue
        if not known[left:right].all() or not (available[left:right] < when).all():
            continue
        selected_fills = fills[left:right] & admitted[left:right]
        trade_times = available[left:right][selected_fills]
        columns["reference_availability_known"][i] = True
        columns["reference_quiet"][i] = float(not len(trade_times))
        columns["reference_fills_1800"][i] = len(trade_times)
        columns["reference_fills_300"][i] = np.sum(trade_times >= when - 300 * 1_000_000)
        # Count whole recent markets: every scheduled market must have all thirteen decisions.
        markets = reference["market_id"][left:right].to_list()
        market_admissions: dict[str, bool] = {}
        market_counts: dict[str, int] = {}
        for market, attempted in zip(markets, admitted[left:right], strict=True):
            market_admissions[market] = market_admissions.get(market, False) or bool(attempted)
            market_counts[market] = market_counts.get(market, 0) + 1
        columns["reference_no_admission_fraction"][i] = np.mean([
            not admitted for market, admitted in market_admissions.items()
            if market_counts[market] == len(frozen["bucket_starts"])])
        columns["reference_probability_up"][i] = probability[right - 1]
        # This is a censored, observed lookback feature, not an invented last-trade timestamp.
        columns["reference_seconds_since_fill_capped_1800"][i] = (
            min(horizon, (when - trade_times[-1]) / 1_000_000) if len(trade_times) else horizon)
    return pl.DataFrame(columns).with_columns(pl.col(pl.Float64).fill_nan(None))
