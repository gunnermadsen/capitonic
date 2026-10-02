import json
from datetime import UTC, datetime, timedelta

import polars as pl
import pytest

from btc_directional_model.time_bucket_reference import (
    build_activity_panels,
    label_contract,
    reference_splits,
)
from btc_directional_model.time_bucket_source_audit import sha256, write_json


def test_reference_splits_exclude_global_evaluation_and_future_observations():
    start = datetime(2026, 8, 1, tzinfo=UTC)
    rows = [{"market_id": str(i), "window_start": start + timedelta(days=i, hours=1),
             "window_end": start + timedelta(days=i, hours=1, minutes=5),
             "role": "evaluation" if i == 6 else "holdout" if i == 7 else "training"} for i in range(20)]
    frozen = {"purge_seconds": 1800, "reference": {"calibration_days": 2, "initial_fit_min_days": 5}}
    train, calibration = reference_splits(pl.DataFrame(rows), (start + timedelta(days=15)).date(), frozen)
    assert train["market_id"].to_list() == [str(i) for i in range(13) if i not in (6, 7)]
    assert calibration["market_id"].to_list() == ["13", "14"]
    assert train["window_end"].max() + timedelta(minutes=30) < calibration["window_start"].min()


def test_missing_label_evidence_does_not_silently_authorize_assumption(tmp_path):
    with pytest.raises(ValueError, match="awaits"):
        label_contract(tmp_path)


def activity_fixture(run):
    day = "2026-08-01"
    now = datetime(2026, 8, 1, 1, tzinfo=UTC)
    core = run / f"datasets/core/{day}.parquet"
    core.parent.mkdir(parents=True)
    pl.DataFrame({"point_id": ["query"], "decision_at": [now]}).write_parquet(core)
    reference = run / f"predictions/simulated-refresh-reference/{day}.parquet"
    reference.parent.mkdir(parents=True)
    pl.DataFrame({"point_id": ["earlier"], "market_id": ["market"],
                  "decision_at": [now - timedelta(minutes=1)], "evidence_available_at": [now - timedelta(seconds=59)],
                  "reference_known": [False], "selected_attempt": [None],
                  "probability_up": pl.Series([None], dtype=pl.Float64),
                  "filled_quantity": pl.Series([None], dtype=pl.Float64)}).write_parquet(reference)
    write_json(run / "manifests/core-panels.json", {"status": "complete", "days": [
        {"date": day, "path": str(core), "sha256": sha256(core)}]})
    write_json(run / "manifests/simulated-refresh-reference.json", {"status": "complete", "days": [
        {"date": day, "artifacts": [{"kind": "reference-predictions", "path": str(reference), "sha256": sha256(reference)}]}]})
    frozen = {"reference": {"quiet_seconds": 1800}, "bucket_starts": list(range(0, 260, 20)), "regular_entry_offset": 19}
    write_json(run / "inputs/tournament-freeze.json", frozen)
    return core, reference, frozen


def test_activity_resume_preserves_unknown_output_and_rejects_corruption(tmp_path):
    core, _, frozen = activity_fixture(tmp_path)
    build_activity_panels(tmp_path, [core], frozen)
    output = tmp_path / "datasets/simulated-refresh" / core.name
    initial_hash, initial_mtime = sha256(output), output.stat().st_mtime_ns
    result = pl.read_parquet(output)
    assert result["reference_quiet"].to_list() == [None]
    assert result["reference_availability_known"].to_list() == [False]
    build_activity_panels(tmp_path, [core], frozen)
    assert (sha256(output), output.stat().st_mtime_ns) == (initial_hash, initial_mtime)
    result.with_columns(pl.lit(1.0).alias("reference_quiet")).write_parquet(output)
    with pytest.raises(ValueError, match="quiet-state artifact changed"):
        build_activity_panels(tmp_path, [core], frozen)


def test_activity_rejects_changed_reference_and_unrecorded_outputs(tmp_path):
    core, reference, frozen = activity_fixture(tmp_path)
    original = reference.read_bytes()
    pl.read_parquet(reference).with_columns(pl.lit(0.5).alias("probability_up")).write_parquet(reference)
    with pytest.raises(ValueError, match="Reference predictions changed"):
        build_activity_panels(tmp_path, [core], frozen)
    reference.write_bytes(original)
    output = tmp_path / "datasets/simulated-refresh" / core.name
    output.parent.mkdir(parents=True)
    output.write_bytes(core.read_bytes())
    with pytest.raises(ValueError, match="cannot be overwritten"):
        build_activity_panels(tmp_path, [core], frozen)
    assert json.loads((tmp_path / "manifests/simulated-refresh-reference.json").read_text())["status"] == "complete"
