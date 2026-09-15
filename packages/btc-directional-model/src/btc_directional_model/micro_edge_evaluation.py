"""Independent bucket and chronological router replay, with five-share economics."""

from __future__ import annotations

from itertools import product

import numpy as np
import polars as pl

from .time_bucket_specialist_tournament import _economic_metrics

POLICIES = tuple(
    product(
        (0.50, 0.60, 0.70, 0.80, 0.90),
        (0.01, 0.04, 0.08),
        (0.70, 0.85, 0.95),
        ("both", "up", "down"),
    )
)


def opportunities(frame: pl.DataFrame, probability, candidate: str, gate=None) -> pl.DataFrame:
    p = np.asarray(probability)
    up = p >= 0.5
    cost = np.where(up, frame["up_ask_vwap_5"].to_numpy(), frame["down_ask_vwap_5"].to_numpy())
    age = np.where(
        up, frame["pm_up_book_age_seconds"].to_numpy(), frame["pm_down_book_age_seconds"].to_numpy()
    )
    rate = frame["fee_rate"].to_numpy()
    fee = rate * cost * (1 - cost)
    confidence = np.maximum(p, 1 - p)
    edge = confidence - cost - fee - 0.005
    won = up == (frame["label_up"].to_numpy() == 1)
    pnl = 5 * (won.astype(float) - cost - fee - 0.005)
    valid = (
        np.isfinite(cost)
        & (cost > 0)
        & (cost < 1)
        & np.isfinite(age)
        & (age >= 0)
        & (age <= 2)
        & np.isfinite(rate)
        & np.isfinite(p)
    )
    if gate is not None:
        valid &= np.asarray(gate)
    base = frame.select(
        "market_id",
        "window_start",
        "window_end",
        "observed_at",
        "seconds_elapsed",
        "label_up",
        "settlement_regime",
    )
    return (
        base.with_columns(
            pl.Series("probability", p),
            pl.Series("side", np.where(up, "up", "down")),
            pl.Series("share_cost", cost),
            pl.Series("selected_probability", confidence),
            pl.Series("expected_edge", edge),
            pl.Series("net_pnl", pnl),
            pl.Series("stress_net_pnl", pnl - 0.05),
            pl.Series("won", won),
            pl.Series("executable", valid),
            pl.lit(candidate).alias("candidate"),
            (pl.col("seconds_elapsed") // 10 * 10).alias("bucket"),
        )
        .filter("executable")
        .sort(["window_start", "market_id", "seconds_elapsed"])
    )


def select(frame, policy, independent=False):
    confidence, edge, cost, side = policy
    selected = frame.filter(
        (pl.col("selected_probability") >= confidence)
        & (pl.col("expected_edge") >= edge)
        & (pl.col("share_cost") <= cost)
        & (pl.lit(side == "both") | (pl.col("side") == side))
    )
    return selected.unique(
        ["market_id", "bucket"] if independent else ["market_id"], keep="first", maintain_order=True
    )


def fit_policies(history: pl.DataFrame) -> dict[int, tuple]:
    result = {}
    if history.is_empty():
        return result
    # Use NumPy masks to avoid a Polars query for every policy/bucket combination.
    for key, part in history.partition_by("bucket", as_dict=True).items():
        market_ids = part["market_id"].to_numpy()
        ids = np.r_[0, np.cumsum(market_ids[1:] != market_ids[:-1])]
        conf = part["selected_probability"].to_numpy()
        edges = part["expected_edge"].to_numpy()
        costs = part["share_cost"].to_numpy()
        sides = part["side"].to_numpy()
        pnl = part["stress_net_pnl"].to_numpy()
        best_score = 0.0
        best = None
        for policy in POLICIES:
            c, e, m, side = policy
            ix = np.flatnonzero(
                (conf >= c)
                & (edges >= e)
                & (costs <= m)
                & ((sides == side) if side != "both" else True)
            )
            if not len(ix):
                continue
            ix = ix[np.r_[True, ids[ix[1:]] != ids[ix[:-1]]]]
            # Conservative dispersion penalty, no hard minimum trade count.
            score = float(pnl[ix].sum() - 0.5 * np.sqrt(np.sum(pnl[ix] ** 2)))
            if score > best_score:
                best_score, best = score, policy
        if best is not None:
            result[int(key[0])] = best
    return result


def apply_policies(frame, policies, independent=False):
    parts = [
        select(frame.filter(pl.col("bucket") == int(bucket)), policy)
        for bucket, policy in policies.items()
    ]
    if not parts:
        return frame.head(0)
    all_rows = pl.concat(parts).sort(["window_start", "market_id", "seconds_elapsed"])
    return (
        all_rows if independent else all_rows.unique("market_id", keep="first", maintain_order=True)
    )


def metrics(trades, total_markets, total_days=None, executable_markets=None):
    trades = trades.sort(["window_start", "market_id", "seconds_elapsed"])
    m = _economic_metrics(trades, total_markets)
    m["break_even_win_rate"] = (
        m["recovery_wins_per_loss"] / (1 + m["recovery_wins_per_loss"])
        if m["recovery_wins_per_loss"] is not None
        else None
    )
    m["losses_per_100_trades"] = 100 * m["losses"] / max(m["trades"], 1)
    m["trades_per_day"] = m["trades"] / max(total_days or 1, 1)
    m["executable_market_coverage"] = trades["market_id"].n_unique() / max(
        executable_markets or total_markets, 1
    )
    m["calendar_market_coverage"] = trades["market_id"].n_unique() / max(288 * (total_days or 1), 1)
    m["zero_pnl_trades"] = m["trades"] - m["wins"] - m["losses"]
    streak = maximum = 0
    for v in trades["net_pnl"].to_list():
        streak = streak + 1 if v < 0 else 0
        maximum = max(streak, maximum)
    m["longest_losing_streak"] = maximum
    return m


def router(frame, reservation=None):
    if frame.is_empty():
        return frame
    if reservation:
        frame = frame.with_columns(
            pl.struct("candidate", "bucket")
            .map_elements(
                lambda x: reservation.get((x["candidate"], x["bucket"]), 0.0),
                return_dtype=pl.Float64,
            )
            .alias("_historical")
        )
        # A fixed training-side reservation schedule, no future-market observations.
        allowed = {b for (_, b), score in reservation.items() if score > 0}
        frame = frame.filter(pl.col("bucket").is_in(sorted(allowed)))
    return frame.sort(
        ["window_start", "market_id", "seconds_elapsed", "expected_edge", "candidate"],
        descending=[False, False, False, True, False],
    ).unique("market_id", keep="first", maintain_order=True)


def bootstrap_daily(trades, days, seed=20260915):
    if trades.is_empty():
        return [0.0, 0.0]
    sums = (
        trades.with_columns(pl.col("window_start").dt.date().alias("day"))
        .group_by("day")
        .agg(pl.col("stress_net_pnl").sum())
    )
    by_day = dict(zip(sums["day"].to_list(), sums["stress_net_pnl"].to_list(), strict=True))
    values = np.array([by_day.get(d, 0.0) for d in days])
    rng = np.random.default_rng(seed)
    totals = np.array(
        [rng.choice(values, size=len(values), replace=True).sum() for _ in range(500)]
    )
    return np.quantile(totals, [0.025, 0.975]).tolist()
