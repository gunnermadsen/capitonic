"""Final fitting cannot select favorable evaluation folds or consume protected labels."""

import json
from datetime import UTC, datetime, timedelta

import polars as pl
import pytest

from btc_directional_model.time_bucket_final import final_rows, latest_supported
from btc_directional_model.time_bucket_source_audit import sha256


def test_final_selects_latest_supported_fold_without_economics(tmp_path):
    arm = {"candidate": "synthetic", "arm": "primary"}
    frozen = {"folds": [{"name": name} for name in ["early", "later", "holdout"]]}
    for i, fold in enumerate(frozen["folds"]):
        root = tmp_path / "checkpoints/synthetic/primary" / fold["name"]
        root.mkdir(parents=True)
        artifact = root / "fixture"
        artifact.write_text("synthetic checkpoint")
        (root / "manifest.json").write_text(json.dumps({
            "status": "complete" if i < 2 else "insufficient_complete_case_support",
            "evaluation_pnl": 1000 if i == 0 else -1000,
            "artifacts": [{"path": str(artifact), "sha256": sha256(artifact)}]}))
    assert latest_supported(tmp_path, arm, frozen)[0]["name"] == "later"
    (tmp_path / "checkpoints/synthetic/primary/later/fixture").write_text("changed")
    with pytest.raises(ValueError, match="changed"):
        latest_supported(tmp_path, arm, frozen)


def test_final_preserves_global_roles_and_verified_late_label_override():
    start = datetime(2026, 8, 1, tzinfo=UTC)
    roles = ["training", "training", "evaluation", "holdout", "calibration", "calibration"]
    times = [start, start, start, start, start + timedelta(days=13, hours=1), start + timedelta(days=13, hours=2)]
    panel = pl.DataFrame({"market_id": [str(i) for i in range(6)], "window_start": times,
        "window_end": [t + timedelta(minutes=5) for t in times], "role": roles,
        "role_fold": [None] * 4 + ["holdout"] * 2})
    labels = {"seconds_after_close": 1800, "verified_later_availability": [
        {"market_id": name, "available_at": "2026-09-01T00:00:00+00:00", "evidence": "synthetic", "semantics_verified": True}
        for name in ["1", "5"]]}
    train, calibration = final_rows(panel, labels, {"purge_seconds": 1800},
        {"name": "holdout", "calibration_start": "2026-08-14", "evaluation_start": "2026-08-15"})
    assert train["market_id"].to_list() == ["0"]
    assert calibration["market_id"].to_list() == ["4"]
