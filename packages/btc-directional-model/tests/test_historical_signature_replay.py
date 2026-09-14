from __future__ import annotations

from pathlib import Path

import polars as pl

from btc_directional_model.historical_signature_replay import (
    _bucket_results,
    _canonical_files,
    _normalize_ledger,
    _training_relative,
)


def test_training_relative_uses_deepest_training_results_segment() -> None:
    path = Path("/archive/snapshot/training-results/tournament/run/ledgers/trades.parquet")
    assert _training_relative(path) == Path("tournament/run/ledgers/trades.parquet")


def test_canonical_files_exclude_replay_outputs(tmp_path: Path) -> None:
    local = tmp_path / "training-results"
    historical = local / "historical-model" / "run" / "trades.parquet"
    replay = local / "btc-5m-historical-signature-replay-20260914" / "run" / "normalized-trades.parquet"
    historical.parent.mkdir(parents=True)
    replay.parent.mkdir(parents=True)
    historical.touch()
    replay.touch()
    files, _ = _canonical_files(local, tmp_path / "missing-archive")
    assert set(files) == {Path("historical-model/run/trades.parquet")}


def test_normalize_ledger_reconstructs_auditable_q5_economics(tmp_path: Path) -> None:
    source = tmp_path / "trades.parquet"
    pl.DataFrame(
        {
            "market_id": ["a", "b"],
            "window_start": [
                "2026-08-01T00:00:00Z",
                "2026-08-01T00:05:00Z",
            ],
            "seconds_elapsed": [61, 68],
            "label_up": [1, 0],
            "direction_correct": [True, False],
            "selected_cost_5": [0.60, 0.40],
            "net_pnl": [20.0, -20.0],
        }
    ).with_columns(pl.col("window_start").str.to_datetime(time_zone="UTC")).write_parquet(source)
    relative = Path("tournament/run/development-ledgers/model.parquet")
    frame, reason = _normalize_ledger(source, relative)
    assert reason is None
    assert frame is not None
    assert frame["side"].to_list() == ["up", "up"]
    assert frame["net_pnl"].to_list() == [2.0, -2.0]
    assert frame["economic_basis"].to_list() == [
        "reconstructed_q5_at_archived_entry_price:selected_cost_5",
        "reconstructed_q5_at_archived_entry_price:selected_cost_5",
    ]


def test_bucket_results_preserve_signature_and_granularity() -> None:
    frame = pl.DataFrame(
        {
            "tournament": ["t", "t"],
            "run_id": ["r", "r"],
            "candidate": ["c", "c"],
            "evidence_role": ["sealed", "sealed"],
            "source_artifact": ["a", "a"],
            "economic_basis": ["archived:net_pnl", "archived:net_pnl"],
            "market_id": ["1", "2"],
            "window_start": [
                "2026-08-01T00:00:00Z",
                "2026-08-01T00:05:00Z",
            ],
            "seconds_elapsed": [61, 69],
            "side": ["up", "down"],
            "direction_correct": [True, False],
            "share_cost": [0.5, 0.5],
            "net_pnl": [2.5, -2.5],
            "stress_net_pnl": [2.45, -2.55],
        }
    ).with_columns(pl.col("window_start").str.to_datetime(time_zone="UTC"))
    results = _bucket_results(frame)
    ten = results.filter(pl.col("bucket_width") == 10)
    assert ten.height == 1
    assert ten["bucket_start"].item() == 60
    assert ten["trades"].item() == 2
