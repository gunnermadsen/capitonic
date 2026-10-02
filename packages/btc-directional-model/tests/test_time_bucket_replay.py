import json
from datetime import UTC, datetime, timedelta

import numpy as np
import polars as pl
import pytest

from btc_directional_model.time_bucket_execution import book_index
from btc_directional_model.time_bucket_policy import economic_metrics
from btc_directional_model.time_bucket_replay import replay_rows


class FixedAdvantage:
    def predict(self, matrix):
        return np.ones(len(matrix))


def fixture(delay):
    start = datetime(2026, 8, 1, tzinfo=UTC)
    books = pl.DataFrame([{"market_id": "m", "outcome": side, "token_id": side, "book_sha256": side,
        "available_at": start, "source_timestamp": start, "received_at": start, "sampled_at": start,
        "provider_available_at": start, "ingest_sequence": 1,
        "bids": '[["0.49","8"]]', "asks": '[["0.5","8"]]'} for side in ["up", "down"]])
    rows = pl.DataFrame({"point_id": ["m@19"], "market_id": ["m"], "decision_at": [start],
                         "bucket_start": [0], "chosen_side": ["up"], "official_outcome": ["up"]})
    later = pl.DataFrame({"market_id": ["m"], "decision_at": [start + timedelta(seconds=delay)],
                          "seconds_elapsed": [200], "observable": [1.]})
    frozen = {"book": {"arrival_ms": 150, "max_age_seconds": 2, "depth_haircut": .8, "participation_cap": .25,
                         "reserve_per_share_per_leg": .005, "stress_extra_per_share_per_leg": .01},
              "fees": {"rate_assumption": .07}}
    return rows, book_index(books), frozen, later


def test_partial_exit_accounts_for_actual_shares_and_each_fee_leg():
    rows, books, frozen, later = fixture(1)
    ledger = replay_rows(rows, books, frozen, 5, "FAK", {"name": "base"}, .7,
        exit_bundle=(FixedAdvantage(), ["observable", "side_is_up"]),
        exit_policy={"minimum_advantage": .01, "fraction": .5}, exit_rows=later, control="learned")
    assert ledger["filled_quantity"][0] == 2
    assert ledger["residual_quantity"][0] == 1
    assert json.loads(ledger["sales_json"][0])[0]["filled_quantity"] == 1
    assert ledger["stress_pnl"][0] == pytest.approx(-2 * (.5 + .0175 + .005) + .49 - .07*.49*.51 - .005 + 1 - .03)


def test_unknown_exit_is_unknown_economics_not_hold_or_zero():
    rows, books, frozen, later = fixture(3)
    ledger = replay_rows(rows, books, frozen, 5, "FAK", {"name": "base"}, .7,
        exit_bundle=(FixedAdvantage(), ["observable", "side_is_up"]),
        exit_policy={"minimum_advantage": .01, "fraction": 1}, exit_rows=later, control="learned")
    assert ledger["net_pnl"][0] is None
    assert ledger["stress_pnl"][0] is None
    assert economic_metrics(ledger)["unknown_attempts"] == 1
