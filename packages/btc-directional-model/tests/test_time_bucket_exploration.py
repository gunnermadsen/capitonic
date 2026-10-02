from datetime import UTC, datetime, timedelta

import polars as pl
from polars.testing import assert_frame_equal

from btc_directional_model.time_bucket_exploration import (
    coverage,
    execution_study,
    study_training,
    training_rows,
)

FROZEN = {"purge_seconds": 1800, "exploration_end_exclusive": "2026-08-22"}
ARM = {"candidate": "fixture", "arm": "primary", "features": ["return_5_bps"],
       "matched_products": [], "entry_offsets": [19]}
CUTOFF = datetime(2026, 8, 22, tzinfo=UTC)


def sample():
    rows = []
    for i, role in enumerate(["training", "training", "calibration", "evaluation", "holdout", "training"]):
        end = CUTOFF - timedelta(hours=2) if i < 5 else CUTOFF - timedelta(minutes=29)
        row = {"market_id": str(i), "role": role, "role_fold": None,
               "window_start": end - timedelta(minutes=5), "window_end": end,
               "entry_offset": 19, "seconds_elapsed": 19, "bucket_start": 0,
               "return_5_bps": None if i == 1 else float(i + 2), "label_up": i % 2}
        for side in ("up", "down"):
            for kind in ("fak", "fok"):
                row.update({f"{side}_{kind}_evidence_known": True, f"{side}_{kind}_quantity_5": 5.0,
                            f"{side}_{kind}_net_5": 1.0, f"{side}_{kind}_stress_5": 0.5})
        rows.append(row)
    return pl.DataFrame(rows, schema_overrides={"role_fold": pl.String})


def test_protected_labels_and_economics_cannot_change_training_study():
    original = sample()
    outcome_columns = [name for name in original.columns if name == "label_up" or name.endswith(("_net_5", "_stress_5"))]
    changed = original.with_columns(
        pl.when(pl.col("role") != "training").then(pl.lit(99999)).otherwise(pl.col(name)).alias(name)
        for name in outcome_columns)
    for left, right in zip(study_training(original, [ARM], FROZEN), study_training(changed, [ARM], FROZEN), strict=True):
        assert_frame_equal(left, right)
    assert_frame_equal(execution_study(original, [ARM], FROZEN), execution_study(changed, [ARM], FROZEN))
    assert_frame_equal(coverage(original, [ARM]), coverage(changed, [ARM]))


def test_missing_feature_is_counted_and_never_zero_filled():
    distributions, associations = study_training(sample(), [ARM], FROZEN)
    row = distributions.row(0, named=True)
    assert row["rows"] == 2 and row["finite_rows"] == 1 and row["missing_rows"] == 1
    assert row["minimum"] == row["maximum"] == row["sum"] == 2.0
    assert sum(row["histogram_counts"]) == 1
    assert associations.filter(pl.col("target") == "label_up")["paired_rows"].item() == 1


def test_purge_boundary_and_arm_clock_precede_the_study():
    frame = sample()
    assert training_rows(frame, FROZEN)["market_id"].to_list() == ["0", "1"]
    frame = frame.with_columns(pl.when(pl.col("market_id") == "0").then(9).otherwise(pl.col("entry_offset")).alias("entry_offset"))
    distributions, _ = study_training(frame, [ARM], FROZEN)
    assert distributions["rows"].item() == 1
    assert distributions["finite_rows"].item() == 0
    assert distributions["minimum"].item() is None
    assert distributions["mean"].item() is None


def test_quiet_coverage_is_pending_and_unknown_not_zero():
    quiet = {**ARM, "candidate": "quiet_explorer", "features": ["reference_quiet"], "quiet_only": True}
    counts = coverage(sample(), [quiet])
    assert counts["eligible_rows"].null_count() == counts.height
    assert counts["status"].unique().to_list() == ["pending_causal_simulated_refresh"]
    assert study_training(sample(), [quiet], FROZEN)[0].is_empty()
