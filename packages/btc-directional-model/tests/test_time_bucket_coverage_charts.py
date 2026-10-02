from datetime import UTC, date, datetime

import pytest

from btc_directional_model.time_bucket_coverage_charts import (
    arm_calendar,
    figure,
    preserve_or_write,
    quiet_coverage,
    quiet_figure,
    source_calendar,
    source_status,
)


def test_missing_unproven_and_partial_source_evidence_stay_distinct():
    day = date(2026, 8, 1)
    rows = [
        {
            "product": "p",
            "hour": datetime(2026, 8, 1, h, tzinfo=UTC),
            "raw_rows": 10,
            "eligible_rows": n,
        }
        for h, n in [(1, None), (2, 0), (3, 5), (4, 10)]
    ]
    states, _, daily, summary = source_calendar(rows, "p", [day])
    assert [states[h][0] for h in range(5)] == [0, 1, 1, 2, 3]
    assert daily == [2]
    assert summary[0]["eligible_hours"] == 2
    assert summary[0]["raw_hours"] == 4


def test_duplicate_hours_and_impossible_counts_fail_closed():
    row = {"product": "p", "hour": "2026-08-01T00:00:00+00:00", "raw_rows": 2, "eligible_rows": 1}
    with pytest.raises(ValueError, match="Duplicate"):
        source_calendar([row, row], "p", [date(2026, 8, 1)])
    with pytest.raises(ValueError, match="counts"):
        source_status(1, 2)


def test_pending_arm_is_neither_zero_nor_missing():
    day = date(2026, 8, 1)
    rows = [
        {
            "candidate": "quiet",
            "arm": "primary",
            "date": str(day),
            "role": "training",
            "status": "pending_causal_simulated_refresh",
            "decision_rows": 4,
            "eligible_rows": None,
        }
    ]
    _, states, _ = arm_calendar(rows, [day, date(2026, 8, 2)])
    assert states == [[1, 0]]
    rows[0].update(candidate="measured", status="measured", eligible_rows=0)
    assert arm_calendar(rows, [day])[1] == [[2]]
    rows[0]["eligible_rows"] = None
    with pytest.raises(ValueError, match="Measured"):
        arm_calendar(rows, [day])


def test_figure_keeps_frozen_boundaries_and_offline_export():
    frozen = {
        "folds": [
            {
                "name": "holdout",
                "calibration_start": "2026-08-01",
                "evaluation_start": "2026-08-02",
                "evaluation_end": "2026-08-03",
            }
        ]
    }
    fig = figure(
        "Coverage", "No outcomes", [date(2026, 8, 1)], ["p"], [[0]], [["No hourly record"]], frozen
    )
    assert len(fig.layout.shapes) == 3
    assert fig.layout.shapes[1].x0 == "2026-08-02"
    rendered = fig.to_html(include_plotlyjs="directory", div_id="coverage-test")
    assert 'src="plotly.min.js"' in rendered
    assert "cdn.plot.ly" not in rendered


@pytest.fixture
def quiet_manifests():
    days = ["2026-08-01", "2026-08-02", "2026-08-03"]
    frozen = {
        "range_start": days[0],
        "range_end_exclusive": "2026-08-04",
        "bucket_starts": list(range(0, 260, 20)),
        "entry_offsets": [0, 9, 19],
        "regular_entry_offset": 19,
        "additional_exit_seconds": [279, 299],
        "folds": [
            {
                "name": "holdout",
                "calibration_start": days[0],
                "evaluation_start": days[1],
                "evaluation_end": "2026-08-04",
            }
        ],
    }
    quiet = {
        "status": "complete",
        "days": [
            {
                "date": day,
                "artifacts": [
                    {"candidate": "quiet_explorer", "arm": "primary", "rows": 4 if i == 0 else 0},
                    {"candidate": "quiet_explorer", "arm": "reference_disagreement", "rows": 0},
                ],
            }
            for i, day in enumerate(days)
        ],
    }
    core = {
        "status": "complete",
        "days": [{"date": day, "rows": 82 if i < 2 else 0} for i, day in enumerate(days)],
    }
    reference = {
        "status": "complete",
        "label": "simulated refresh activity",
        "actual_incumbent_complementarity_proven": False,
        "days": [{"date": day, "known_rows": 10 if i == 0 else 0} for i, day in enumerate(days)],
    }
    return quiet, core, reference, frozen


def test_quiet_support_uses_regular_schedule_and_preserves_unknown_reference(quiet_manifests):
    rows = quiet_coverage(*quiet_manifests)
    assert len(rows) == 6
    assert rows[0]["scheduled_regular_points"] == 26
    assert rows[0]["complete_fraction"] == pytest.approx(4 / 26)
    assert rows[0]["reference_known_core_points"] == 10
    assert rows[0]["reference_status"] == "partial"
    assert rows[2]["complete_rows"] == 0
    assert rows[2]["complete_fraction"] == 0
    assert rows[2]["reference_status"] == "unknown"
    assert rows[4]["complete_fraction"] is None


def test_quiet_calendar_rejects_missing_dates_and_impossible_support(quiet_manifests):
    quiet, core, reference, frozen = quiet_manifests
    core["days"][0]["rows"] = 83
    with pytest.raises(ValueError, match="schedule"):
        quiet_coverage(quiet, core, reference, frozen)
    core["days"][0]["rows"] = 82
    quiet["days"][1]["artifacts"][0]["rows"] = 1
    with pytest.raises(ValueError, match="known reference"):
        quiet_coverage(quiet, core, reference, frozen)
    quiet["days"][1]["artifacts"][0]["rows"] = 0
    quiet["days"].pop()
    with pytest.raises(ValueError, match="calendar"):
        quiet_coverage(quiet, core, reference, frozen)


def test_quiet_plot_retains_reference_context_and_frozen_boundaries(quiet_manifests):
    rows = quiet_coverage(*quiet_manifests)
    fig = quiet_figure(rows, quiet_manifests[-1])
    assert len(fig.data[0].y) == 2
    assert len(fig.data[0].x) == 3
    assert "Reference availability: unknown" in fig.data[0].customdata[0][1]
    assert list(fig.data[1].marker.color) == ["#aa81ba", "#d8dde5", "#d8dde5"]
    assert len(fig.layout.shapes) == 3
    html = fig.to_html(include_plotlyjs="plotly.min.js", div_id="quiet-arm-calendar")
    assert 'src="plotly.min.js"' in html
    assert "cdn.plot.ly" not in html


def test_identical_interrupted_output_resumes_but_different_evidence_is_preserved(tmp_path):
    path = tmp_path / "quiet.html"
    preserve_or_write(path, "original evidence")
    modified = path.stat().st_mtime_ns
    preserve_or_write(path, "original evidence")
    assert path.stat().st_mtime_ns == modified
    with pytest.raises(ValueError, match="Preserve incompatible"):
        preserve_or_write(path, "different evidence")
    assert path.read_text() == "original evidence"
