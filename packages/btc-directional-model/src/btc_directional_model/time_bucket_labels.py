"""Explicit conditional label eligibility; never a settlement or runtime clock."""

from __future__ import annotations

import json
from datetime import UTC, datetime
from pathlib import Path

import polars as pl


def label_contract(run: Path) -> dict:
    path = run / "inputs/label-availability-contract.json"
    if not path.exists():
        raise ValueError("Chronological fitting awaits the official-label availability clarification")
    record = json.loads(path.read_text())
    if record.get("mode") != "user_authorized_offline_assumption" or record.get("seconds_after_close") != 1800:
        raise ValueError("No supported authorized label-availability contract")
    if not record.get("user_authorization"):
        raise ValueError("The label-availability assumption requires explicit user authorization")
    return record


def with_label_availability(frame: pl.DataFrame, contract: dict) -> pl.DataFrame:
    """A verified later time can delay, but never accelerate, the assumed lower bound."""
    records = contract.get("verified_later_availability", [])
    by_market = {}
    for row in records:
        if not row.get("evidence") or not row.get("semantics_verified"):
            raise ValueError("Label availability overrides require identified verified evidence")
        timestamp = datetime.fromisoformat(row["available_at"])
        if timestamp.tzinfo is None:
            raise ValueError("Verified label timestamps require a timezone")
        market = str(row["market_id"])
        by_market[market] = max(timestamp.astimezone(UTC), by_market.get(market, timestamp.astimezone(UTC)))
    result = frame.with_columns((pl.col("window_end") + pl.duration(seconds=contract["seconds_after_close"]))
                                 .alias("label_use_at"))
    if by_market:
        overrides = pl.DataFrame({"market_id": list(by_market), "verified_label_available_at": list(by_market.values())})
        result = result.join(overrides, on="market_id", how="left", validate="m:1").with_columns(
            pl.max_horizontal("label_use_at", "verified_label_available_at").alias("label_use_at"))
    return result


def eligible_labels(frame: pl.DataFrame, cutoff: datetime) -> pl.DataFrame:
    if "label_use_at" not in frame:
        raise ValueError("Supervised selection requires explicit label-use eligibility")
    return frame.filter(pl.col("label_use_at") <= cutoff)


def prediction_features_only(features: list[str] | tuple[str, ...]) -> None:
    forbidden = [name for name in features if any(term in name for term in
        ("outcome", "label", "pnl", "_fak_", "_fok_", "selected_attempt", "filled_quantity"))]
    if forbidden:
        raise ValueError(f"Retrospective fields cannot be prediction features: {forbidden}")
