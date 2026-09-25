"""Causal FOK replay for frozen champion and entry/disposition challengers."""

from __future__ import annotations

import argparse
import bisect
import hashlib
import json
from collections import defaultdict
from dataclasses import dataclass
from datetime import UTC, date, datetime, timedelta
from pathlib import Path
from typing import Any

import joblib
import numpy as np
import polars as pl
from sklearn.ensemble import HistGradientBoostingClassifier
from sklearn.linear_model import LogisticRegression

from .conservative_selective_paper_export import POLICY as CHAMPION_POLICY
from .conservative_selective_paper_export import SOURCE_SHA256
from .conservative_selective_training import (
    expected_calibration_error,
    score,
    trade_metrics,
)
from .entry_disposition_data import _books
from .fok_retry_backtest import parse_asks, walk

Q = 5
BUY_LATENCY_MS = 150
BUY_HAIRCUT = 0.80
PARTICIPATION = 0.25
BOOK_AGE = timedelta(seconds=2)
RESERVE = 0.005
STRESS = 0.010
SELL_FEATURES = (
    "side_up", "seconds_elapsed_scaled", "bid_vwap_5", "btc_path_from_window_open_bps",
    "btc_return_5s_bps", "btc_return_30s_bps", "btc_realized_volatility_30s_bps",
    "btc_path_efficiency_30s", "btc_momentum_multihorizon_score",
)


@dataclass(frozen=True)
class Book:
    available_at: datetime
    sampled_at: datetime
    received_at: datetime
    source_timestamp: datetime
    asks: str
    bids: str


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _book_index(raw: pl.DataFrame) -> dict[tuple[str, str], tuple[list[datetime], list[Book]]]:
    grouped: dict[tuple[str, str], list[Book]] = defaultdict(list)
    for row in raw.iter_rows(named=True):
        side = str(row["outcome"]).lower()
        if side not in ("up", "down"):
            continue
        grouped[(str(row["market_id"]), side)].append(Book(
            available_at=max(row["sampled_at"], row["received_at"], row["source_timestamp"]),
            sampled_at=row["sampled_at"], received_at=row["received_at"],
            source_timestamp=row["source_timestamp"], asks=row["asks"], bids=row["bids"],
        ))
    output = {}
    for key, books in grouped.items():
        books.sort(key=lambda item: item.available_at)
        output[key] = ([item.available_at for item in books], books)
    return output


def _at(index: dict, market_id: str, side: str, at: datetime) -> Book | None:
    entry = index.get((market_id, side))
    if entry is None:
        return None
    times, books = entry
    pointer = bisect.bisect_right(times, at) - 1
    if pointer < 0:
        return None
    book = books[pointer]
    if (at - book.available_at > BOOK_AGE or at - book.source_timestamp > BOOK_AGE
            or at - book.received_at > BOOK_AGE or at - book.sampled_at > BOOK_AGE):
        return None
    return book


def _bid_walk(value: str | None, quantity: int, haircut: float = 1.0,
              floor: float | None = None) -> tuple[float, float, float] | None:
    if not value:
        return None
    levels = json.loads(value) if isinstance(value, str) else value
    remaining = float(quantity)
    notional = 0.0
    worst = 1.0
    displayed = 0.0
    for price_raw, size_raw in sorted(levels, key=lambda row: -float(row[0])):
        price, size = float(price_raw), float(size_raw)
        if price <= 0 or size <= 0 or (floor is not None and price < floor - 1e-12):
            continue
        displayed += size
        fill = min(remaining, size * haircut)
        notional += fill * price
        remaining -= fill
        if fill > 0:
            worst = price
        if remaining <= 1e-9:
            return notional / quantity, worst, displayed
    return None


def _buy(index: dict, market: str, side: str, at: datetime, max_cost: float,
         latency_ms: int = BUY_LATENCY_MS, haircut: float = BUY_HAIRCUT,
         quantity: int = Q) -> dict[str, Any]:
    decision_book = _at(index, market, side, at)
    if decision_book is None:
        return {"filled": False, "reason": "missing_decision_book"}
    decision = walk(parse_asks(decision_book.asks), quantity)
    if decision is None:
        return {"filled": False, "reason": "insufficient_decision_ask_depth"}
    decision_vwap, limit, _ = decision
    if decision_vwap > max_cost or limit > max_cost:
        return {"filled": False, "reason": "ask_above_limit"}
    arrival = _at(index, market, side, at + timedelta(milliseconds=latency_ms))
    if arrival is None:
        return {"filled": False, "reason": "missing_arrival_book"}
    opposite = "down" if side == "up" else "up"
    if _at(index, market, opposite, at + timedelta(milliseconds=latency_ms)) is None:
        return {"filled": False, "reason": "missing_opposite_book"}
    asks = parse_asks(arrival.asks)
    displayed = sum(size for price, size in asks if price <= limit + 1e-12)
    fill = walk(asks, quantity, haircut=haircut, limit=limit)
    if fill is None or quantity > displayed * PARTICIPATION + 1e-9:
        return {"filled": False, "reason": "arrival_fok_rejected"}
    return {"filled": True, "reason": "filled", "share_cost": fill[0], "limit_price": limit,
            "decision_vwap": decision_vwap}


def _sell(index: dict, market: str, side: str, at: datetime,
          latency_ms: int = BUY_LATENCY_MS, haircut: float = BUY_HAIRCUT,
          quantity: int = Q) -> dict[str, Any]:
    decision_book = _at(index, market, side, at)
    if decision_book is None:
        return {"filled": False, "reason": "missing_decision_bid"}
    decision = _bid_walk(decision_book.bids, quantity)
    if decision is None:
        return {"filled": False, "reason": "insufficient_decision_bid_depth"}
    _, floor, _ = decision
    arrival = _at(index, market, side, at + timedelta(milliseconds=latency_ms))
    if arrival is None:
        return {"filled": False, "reason": "missing_arrival_bid"}
    displayed = sum(float(size) for price, size in json.loads(arrival.bids) if float(price) >= floor - 1e-12)
    fill = _bid_walk(arrival.bids, quantity, haircut=haircut, floor=floor)
    if fill is None or quantity > displayed * PARTICIPATION + 1e-9:
        return {"filled": False, "reason": "sell_fok_rejected"}
    return {"filled": True, "reason": "filled", "sale_price": fill[0], "limit_price": floor}


def _eligible(row: dict, source: str, policy: dict | None) -> tuple[bool, str, float, float]:
    if policy is None:
        return False, "", 0.0, 0.0
    second = int(row["seconds_elapsed"])
    if second < policy["start_second"] or second > policy["end_second"]:
        return False, "", 0.0, 0.0
    p = row[f"{source}_probability_up"]
    if p is None:
        return False, "", 0.0, 0.0
    side = "up" if p >= 0.5 else "down"
    conservative = row[f"{source}_conservative_probability_up"]
    confidence = conservative if side == "up" else 1 - conservative
    cost = row[f"{side}_ask_vwap_5"]
    edge = confidence - cost - RESERVE - STRESS if cost is not None else float("-inf")
    return bool(cost is not None and confidence >= policy["minimum_confidence"]
                and edge >= policy["minimum_stressed_edge"]
                and cost <= policy["maximum_share_cost"]), side, confidence, edge


def _hold(trade: dict) -> dict:
    won = trade["side"] == trade["official_outcome"]
    value = 1.0 if won else 0.0
    return {**trade, "won": won, "exit_type": "settlement", "exit_second": 300,
            "net_pnl": trade["quantity"] * (value - trade["share_cost"] - RESERVE),
            "stress_pnl": trade["quantity"] * (value - trade["share_cost"] - RESERVE - STRESS)}


def _sell_frame(frame: pl.DataFrame) -> pl.DataFrame:
    parts = []
    for side in ("up", "down"):
        bids = [_bid_walk(value, Q) for value in frame[f"{side}_bids"].to_list()]
        part = frame.select("market_id", "window_start", "official_outcome", "seconds_elapsed", *SELL_FEATURES[3:]).with_columns(
            pl.lit(1 if side == "up" else 0).alias("side_up"),
            pl.Series("bid_vwap_5", [None if row is None else row[0] for row in bids]),
            (pl.col("official_outcome") == side).cast(pl.Int8).alias("settles"),
            (pl.col("seconds_elapsed") / 300.0).alias("seconds_elapsed_scaled"),
        )
        parts.append(part)
    return pl.concat(parts, how="vertical").drop_nulls(["bid_vwap_5"])


def _fit_sell(causal_dir: Path, test_start: date, output: Path) -> tuple | None:
    first = date(2026, 7, 13)
    cal_start = test_start - timedelta(days=7)
    if cal_start < first + timedelta(days=7):
        return None
    dates = [first + timedelta(days=i) for i in range((test_start - first).days)]
    fit_dates = [day for day in dates if day < cal_start]
    cal_dates = [day for day in dates if day >= cal_start]
    def load(days: list[date]) -> pl.DataFrame:
        frames = []
        for day in days:
            path = causal_dir / f"date={day.isoformat()}.parquet"
            if path.exists():
                frames.append(_sell_frame(pl.read_parquet(path).filter(pl.col("seconds_elapsed") % 10 == 0)))
        return pl.concat(frames, how="vertical") if frames else pl.DataFrame()
    fit, cal = load(fit_dates), load(cal_dates)
    if fit.is_empty() or cal.is_empty() or fit["settles"].n_unique() < 2 or cal["settles"].n_unique() < 2:
        return None
    estimator = HistGradientBoostingClassifier(
        learning_rate=0.05, max_iter=80, max_bins=127, max_leaf_nodes=15,
        min_samples_leaf=120, l2_regularization=10, early_stopping=False,
        random_state=20260924,
    )
    weights = fit.select((1 / pl.len().over("market_id")).alias("weight"))["weight"].to_numpy()
    estimator.fit(fit.select(SELL_FEATURES).to_numpy(), fit["settles"].to_numpy(), sample_weight=weights)
    raw = np.clip(estimator.predict_proba(cal.select(SELL_FEATURES).to_numpy())[:, 1], 1e-6, 1 - 1e-6)
    calibrator = LogisticRegression(C=1, max_iter=300, random_state=20260924)
    calibration_weights = cal.select((1 / pl.len().over("market_id")).alias("weight"))["weight"].to_numpy()
    calibrator.fit(np.log(raw / (1 - raw)).reshape(-1, 1), cal["settles"].to_numpy(), sample_weight=calibration_weights)
    p = calibrator.predict_proba(np.log(raw / (1 - raw)).reshape(-1, 1))[:, 1]
    ece = expected_calibration_error(cal["settles"].to_numpy(), p, 10)
    fitted = (estimator, calibrator, float(ece))
    joblib.dump(fitted, output / "models" / f"sell-{test_start.isoformat()}.joblib")
    return fitted


def _sell_probability(model: tuple, row: dict, side: str, bid: float) -> float:
    estimator, calibrator, ece = model
    vector = [1 if side == "up" else 0, row["seconds_elapsed_scaled"], bid]
    vector += [row[name] for name in SELL_FEATURES[3:]]
    x = np.asarray(vector, dtype=float).reshape(1, -1)
    raw = np.clip(estimator.predict_proba(x)[0, 1], 1e-6, 1 - 1e-6)
    p = calibrator.predict_proba(np.log(raw / (1 - raw)).reshape(1, -1))[0, 1]
    return float(min(1.0, p + max(ece, 0.02)))


def _dispose(trade: dict, rows: list[dict], index: dict, sell_model: tuple | None,
             mode: str, quantity: int = Q, latency_ms: int = BUY_LATENCY_MS,
             haircut: float = BUY_HAIRCUT) -> tuple[dict, list[dict]]:
    if sell_model is None and mode == "model":
        return _hold(trade), []
    events = []
    for row in rows:
        second = int(row["seconds_elapsed"])
        if second < trade["seconds_elapsed"] + 5 or second >= 220:
            continue
        bid_result = _bid_walk(row[f"{trade['side']}_bids"], quantity)
        if bid_result is None:
            continue
        bid = bid_result[0]
        if mode == "model":
            signal_bid = _bid_walk(row[f"{trade['side']}_bids"], Q)
            if signal_bid is None:
                continue
            continue_value = _sell_probability(sell_model, row, trade["side"], signal_bid[0])
            intent = bid - RESERVE - STRESS >= continue_value + 0.03
        else:
            difference = bid - trade["share_cost"]
            intent = difference >= 0.10 or difference <= -0.10
            continue_value = None
        if not intent:
            continue
        at = row["window_start"] + timedelta(seconds=second)
        result = _sell(index, trade["market_id"], trade["side"], at,
                       latency_ms=latency_ms, haircut=haircut, quantity=quantity)
        events.append({"market_id": trade["market_id"], "window_start": trade["window_start"],
                       "mode": mode, "second": second, "bid_vwap": bid,
                       "continue_value": continue_value, **result})
        if result["filled"]:
            sale = result["sale_price"]
            return ({**trade, "exit_type": "sell", "exit_second": second, "sale_price": sale,
                     "net_pnl": quantity * (sale - trade["share_cost"] - 2 * RESERVE),
                     "stress_pnl": quantity * (sale - trade["share_cost"] - 2 * RESERVE - 2 * STRESS)}, events)
    return _hold(trade), events


def run(run_dir: Path, champion: Path, canonical: Path, verified: Path) -> None:
    if _sha256(champion) != SOURCE_SHA256:
        raise RuntimeError("frozen champion artifact identity changed")
    champion_bundle = joblib.load(champion)
    champion_model = (champion_bundle["estimator"], champion_bundle["calibrator"],
                      tuple(champion_bundle["features"]), champion_bundle["calibration_ece"])
    causal_dir = run_dir / "datasets" / "causal-book-panel"
    folds = json.loads((run_dir / "metrics" / "training-folds.json").read_text())
    buy_folds = [row for row in folds if "exact_test_markets" in row]
    for name in ("backtests", "models", "metrics"):
        (run_dir / name).mkdir(exist_ok=True)
    all_trades: dict[str, list[dict]] = {name: [] for name in ("champion", "buy_hold", "buy_sell", "champion_specialist", "fixed_exit")}
    attempts, sell_events, displacement = [], [], []
    sell_models: dict[date, tuple | None] = {}
    for fold in buy_folds:
        test_start = date.fromisoformat(fold["test_start"][:10])
        test_end = date.fromisoformat(fold["test_end"][:10])
        sell_model = _fit_sell(causal_dir, test_start, run_dir)
        sell_models[test_start] = sell_model
        day = test_start
        while day < test_end:
            path = causal_dir / f"date={day.isoformat()}.parquet"
            if not path.exists():
                day += timedelta(days=1)
                continue
            causal = pl.read_parquet(path)
            day_start = datetime.combine(day, datetime.min.time(), UTC)
            day_end = day_start + timedelta(days=1)
            predictions = pl.read_parquet(run_dir / "predictions" / f"buy-fold-{fold['fold']:02d}.parquet").filter(
                (pl.col("window_start") >= day_start) & (pl.col("window_start") < day_end)
            ).select("market_id", "seconds_elapsed", "probability_up", "conservative_probability_up")
            predictions = predictions.rename({"probability_up": "buy_probability_up",
                                              "conservative_probability_up": "buy_conservative_probability_up"})
            scored = score(causal, champion_model, 0.02).select(
                "market_id", "seconds_elapsed", "probability_up", "conservative_probability_up"
            ).rename({"probability_up": "champion_probability_up",
                      "conservative_probability_up": "champion_conservative_probability_up"})
            frame = causal.join(scored, on=["market_id", "seconds_elapsed"], how="left")
            frame = frame.join(predictions, on=["market_id", "seconds_elapsed"], how="left")
            raw, _ = _books(day, canonical, verified)
            index = _book_index(raw)
            buy_policy = fold["policy"]
            if buy_policy is not None:
                buy_policy = {"start_second": 60, "end_second": 210,
                              "minimum_confidence": buy_policy["confidence_floor"],
                              "minimum_stressed_edge": buy_policy["minimum_stressed_edge"],
                              "maximum_share_cost": buy_policy["maximum_share_cost"]}
            day_trades: dict[str, list[dict]] = {key: [] for key in all_trades}
            for _, group in frame.sort(["market_id", "seconds_elapsed"]).group_by("market_id", maintain_order=True):
                rows = group.to_dicts()
                market = str(rows[0]["market_id"])
                state: dict[str, dict | None] = {"champion": None, "buy_hold": None, "champion_specialist": None}
                for row in rows:
                    at = row["window_start"] + timedelta(seconds=int(row["seconds_elapsed"]))
                    champion_ok, champion_side, champion_conf, champion_edge = _eligible(row, "champion", CHAMPION_POLICY)
                    buy_ok, buy_side, buy_conf, buy_edge = _eligible(row, "buy", buy_policy)
                    if champion_ok and state["champion"] is None:
                        result = _buy(index, market, champion_side, at, CHAMPION_POLICY["maximum_share_cost"])
                        attempts.append({"entry": "champion", "market_id": market, "at": at, **result})
                        if result["filled"]:
                            state["champion"] = {"entry": "champion", "market_id": market,
                                "window_start": row["window_start"], "official_outcome": row["official_outcome"],
                                "seconds_elapsed": int(row["seconds_elapsed"]), "side": champion_side,
                                "share_cost": result["share_cost"], "quantity": Q, "confidence": champion_conf,
                                "stressed_edge": champion_edge, "fold": fold["fold"]}
                    if buy_ok and state["buy_hold"] is None:
                        result = _buy(index, market, buy_side, at, buy_policy["maximum_share_cost"])
                        attempts.append({"entry": "buy_hold", "market_id": market, "at": at, **result})
                        if result["filled"]:
                            state["buy_hold"] = {"entry": "buy_hold", "market_id": market,
                                "window_start": row["window_start"], "official_outcome": row["official_outcome"],
                                "seconds_elapsed": int(row["seconds_elapsed"]), "side": buy_side,
                                "share_cost": result["share_cost"], "quantity": Q, "confidence": buy_conf,
                                "stressed_edge": buy_edge, "fold": fold["fold"]}
                    if state["champion_specialist"] is None:
                        entry = "champion" if champion_ok else ("specialist" if buy_ok else None)
                        side = champion_side if champion_ok else buy_side
                        if entry is not None:
                            max_cost = (CHAMPION_POLICY if entry == "champion" else buy_policy)["maximum_share_cost"]
                            result = _buy(index, market, side, at, max_cost)
                            attempts.append({"entry": "champion_specialist_" + entry, "market_id": market, "at": at, **result})
                            if result["filled"]:
                                state["champion_specialist"] = {"entry": entry, "market_id": market,
                                    "window_start": row["window_start"], "official_outcome": row["official_outcome"],
                                    "seconds_elapsed": int(row["seconds_elapsed"]), "side": side,
                                    "share_cost": result["share_cost"], "quantity": Q,
                                    "confidence": champion_conf if entry == "champion" else buy_conf,
                                    "stressed_edge": champion_edge if entry == "champion" else buy_edge,
                                    "fold": fold["fold"]}
                if state["champion"] is not None:
                    day_trades["champion"].append(_hold(state["champion"]))
                if state["buy_hold"] is not None:
                    held = _hold(state["buy_hold"])
                    day_trades["buy_hold"].append(held)
                    disposed, events = _dispose(state["buy_hold"], rows, index, sell_model, "model")
                    day_trades["buy_sell"].append(disposed)
                    sell_events.extend(events)
                    fixed, events = _dispose(state["buy_hold"], rows, index, sell_model, "fixed")
                    day_trades["fixed_exit"].append(fixed)
                    sell_events.extend(events)
                if state["champion_specialist"] is not None:
                    day_trades["champion_specialist"].append(_hold(state["champion_specialist"]))
                    if (state["champion_specialist"]["entry"] == "specialist" and state["champion"] is not None):
                        displacement.append({"market_id": market, "window_start": row["window_start"],
                            "specialist_second": state["champion_specialist"]["seconds_elapsed"],
                            "champion_second": state["champion"]["seconds_elapsed"],
                            "specialist_stress_pnl": day_trades["champion_specialist"][-1]["stress_pnl"],
                            "champion_stress_pnl": day_trades["champion"][-1]["stress_pnl"]})
            for key, values in day_trades.items():
                if values:
                    pl.DataFrame(values).write_parquet(run_dir / "backtests" / f"{key}-{day.isoformat()}.parquet")
                    all_trades[key].extend(values)
            print(json.dumps({"day": day.isoformat(), "trades": {key: len(value) for key, value in day_trades.items()},
                              "sell_model": sell_model is not None}), flush=True)
            day += timedelta(days=1)
    for key, values in all_trades.items():
        if values:
            pl.DataFrame(values).sort("window_start").write_parquet(run_dir / "backtests" / f"{key}.parquet")
    if attempts:
        pl.DataFrame(attempts).write_parquet(run_dir / "backtests" / "buy-attempts.parquet")
    if sell_events:
        pl.DataFrame(sell_events).write_parquet(run_dir / "backtests" / "sell-attempts.parquet")
    if displacement:
        pl.DataFrame(displacement).write_parquet(run_dir / "backtests" / "specialist-displacement.parquet")
    result = {"champion_source_sha256": SOURCE_SHA256, "champion_policy": CHAMPION_POLICY,
              "fok": {"quantity": Q, "latency_ms": BUY_LATENCY_MS, "depth_haircut": BUY_HAIRCUT,
                      "participation_limit": PARTICIPATION, "maximum_book_age_seconds": BOOK_AGE.total_seconds()},
              "sell_model_fold_starts": [str(day) for day, model in sell_models.items() if model is not None],
              "displaced_champion_trades": len(displacement),
              "displacement_stress_pnl_delta": sum(row["specialist_stress_pnl"] - row["champion_stress_pnl"] for row in displacement),
              "trades": {key: trade_metrics(pl.DataFrame(values).sort("window_start") if values else pl.DataFrame())
                         for key, values in all_trades.items()},
              "sell_intents": len([row for row in sell_events if row["mode"] == "model"]),
              "sell_intents_unfilled": len([row for row in sell_events if row["mode"] == "model" and not row["filled"]])}
    (run_dir / "metrics" / "replay.json").write_text(json.dumps(result, indent=2, default=str) + "\n")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--run-dir", type=Path, required=True)
    parser.add_argument("--champion", type=Path, required=True)
    parser.add_argument("--canonical-books", type=Path, required=True)
    parser.add_argument("--verified-books", type=Path, required=True)
    args = parser.parse_args()
    run(args.run_dir, args.champion, args.canonical_books, args.verified_books)
