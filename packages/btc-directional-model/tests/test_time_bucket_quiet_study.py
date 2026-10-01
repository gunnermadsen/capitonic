"""Synthetic quiet-study causality, missingness and resumable-evidence checks."""

import json
from datetime import UTC, datetime, timedelta

import polars as pl
import pytest

from btc_directional_model import time_bucket_quiet_study as study
from btc_directional_model.time_bucket_activity import activity_states
from btc_directional_model.time_bucket_source_audit import sha256

START = datetime(2026, 8, 21, tzinfo=UTC)
FROZEN = {
    "exploration_end_exclusive": "2026-08-22",
    "purge_seconds": 1800,
    "reference": {"quiet_seconds": 1800},
    "bucket_starts": list(range(0, 260, 20)),
    "regular_entry_offset": 19,
    "quantity_grid": [1, 5, 10, 200],
    "charts": {"burst_max_gap_minutes": 30},
}
LABELS = {
    "mode": "user_authorized_offline_assumption",
    "seconds_after_close": 1800,
    "user_authorization": "synthetic explicit fixture",
    "verified_later_availability": [],
}
ARMS = [
    {
        "candidate": "quiet_explorer",
        "arm": "primary",
        "entry_offsets": [19],
        "matched_products": [],
        "quiet_only": True,
        "features": [
            "signal",
            "reference_quiet",
            "reference_probability_up",
            "reference_seconds_since_fill_capped_1800",
        ],
    }
]


def row(i=0, **changes):
    start = START + timedelta(minutes=5 * i)
    record = {
        "point_id": f"m{i}:19",
        "market_id": f"m{i}",
        "window_start": start,
        "window_end": start + timedelta(minutes=5),
        "decision_at": start + timedelta(seconds=19),
        "role": "training",
        "role_fold": None,
        "entry_offset": 19,
        "seconds_elapsed": 19,
        "bucket_start": 0,
        "label_up": 1,
        "signal": 2.0,
        "reference_availability_known": True,
        "reference_quiet": 1.0,
        "reference_probability_up": 0.8,
        "reference_no_admission_fraction": 1.0,
        "reference_fills_300": 0.0,
        "reference_fills_1800": 0.0,
        "reference_seconds_since_fill_capped_1800": 1800.0,
        "up_vwap_grid": [0.4, 0.4, 0.42, None],
        "down_vwap_grid": [0.6, 0.6, None, None],
    }
    for side in ["up", "down"]:
        for kind in ["fak", "fok"]:
            record.update(
                {
                    f"{side}_{kind}_evidence_known": True,
                    f"{side}_{kind}_quantity_5": 5.0,
                    f"{side}_{kind}_price_5": 0.4,
                    f"{side}_{kind}_net_5": 2.0,
                    f"{side}_{kind}_stress_5": 1.5,
                }
            )
    return record | changes


def frame():
    return pl.DataFrame(
        [
            row(),
            row(1, signal=None, label_up=0),
            row(2, reference_availability_known=False, reference_quiet=1.0),
            row(3, reference_quiet=0.0, reference_fills_1800=1.0),
            row(4, role="calibration"),
            row(5, role="evaluation"),
            row(6, role="holdout"),
        ]
    )


def reference(panel):
    return panel.select("point_id", "market_id", "decision_at").with_columns(
        pl.lit(0.8).alias("probability_up"),
        pl.lit(True).alias("reference_known"),
        pl.lit(0.0).alias("filled_quantity"),
        pl.lit(False).alias("selected_attempt"),
        (pl.col("decision_at") + pl.duration(milliseconds=150)).alias("evidence_available_at"),
    )


def test_protected_labels_economics_and_features_cannot_affect_study():
    source = frame()
    first, count = study.study_day(source, reference(source), ARMS, FROZEN, LABELS)
    assert count == 4
    protected = pl.col("role") != "training"
    changed = source.with_columns(
        pl.when(protected).then(999).otherwise(pl.col("label_up")).alias("label_up"),
        pl.when(protected).then(999999.0).otherwise(pl.col("signal")).alias("signal"),
        *(
            pl.when(protected).then(-999999.0).otherwise(pl.col(c)).alias(c)
            for c in source.columns
            if "_net_" in c or "_stress_" in c
        ),
    )
    second, count2 = study.study_day(changed, reference(changed), ARMS, FROZEN, LABELS)
    assert count2 == count
    assert first.keys() == second.keys()
    for name in first:
        assert first[name].equals(second[name]), name
    distributions = first["feature-distributions"].filter(pl.col("feature") == "signal")
    assert distributions["rows"].item() == 2
    assert distributions["finite_rows"].item() == 1
    assert distributions["missing_rows"].item() == 1
    assert distributions["mean"].item() == 2
    assert first["reference-events"]["market_id"].to_list() == ["m0", "m1", "m2", "m3"]


def test_label_boundary_and_verified_later_availability_precede_all_studies():
    cutoff = datetime(2026, 8, 22, tzinfo=UTC)
    source = pl.DataFrame(
        [
            row(0, window_end=cutoff - timedelta(minutes=30)),
            row(1, window_end=cutoff - timedelta(minutes=30) + timedelta(microseconds=1)),
            row(2),
        ]
    ).with_columns(pl.lit(START).alias("label_use_at"))
    labels = {
        **LABELS,
        "verified_later_availability": [
            {
                "market_id": "m2",
                "available_at": (cutoff + timedelta(seconds=1)).isoformat(),
                "evidence": "synthetic",
                "semantics_verified": True,
            }
        ],
    }
    allowed = study.allowed_training(source, FROZEN, labels)
    assert allowed["market_id"].to_list() == ["m0"]
    assert allowed["label_use_at"].item() == cutoff


def test_unknown_activity_never_becomes_quiet_or_zero_and_capacity_missing_remains_missing():
    tables, _ = study.study_day(frame(), reference(frame()), ARMS, FROZEN, LABELS)
    counts = tables["arm-eligibility"].filter(pl.col("bucket_start") == 0)
    assert counts["unknown_rows"].item() == 1
    assert counts["known_quiet_rows"].item() == 2
    assert counts["known_active_rows"].item() == 1
    assert counts["complete_case_rows"].item() == 1
    unknown = tables["state-summary"].filter(pl.col("reference_state") == "unknown")
    assert unknown["finite_rows"].sum() == 0
    assert unknown["mean"].null_count() == unknown.height
    capacity = tables["displayed-capacity"].filter(pl.col("quantity") == 200)
    assert capacity["observed_full_quantity_vwaps"].sum() == 0
    assert capacity["mean_vwap"].null_count() == capacity.height


def test_existing_activity_owner_requires_all_78_known_strictly_prior_slots():
    query_time = START + timedelta(minutes=30)
    decisions = [
        START + timedelta(seconds=m * 300 + b + 19) for m in range(6) for b in range(0, 260, 20)
    ]
    ref = pl.DataFrame(
        {
            "point_id": [str(i) for i in range(78)],
            "market_id": [str(i // 13) for i in range(78)],
            "decision_at": decisions,
            "evidence_available_at": [t + timedelta(milliseconds=150) for t in decisions],
            "reference_known": [True] * 78,
            "selected_attempt": [False] * 78,
            "filled_quantity": [0.0] * 78,
            "probability_up": [0.8] * 78,
        }
    )
    query = pl.DataFrame({"decision_at": [query_time]})
    full = activity_states(query, ref, FROZEN)
    assert full["reference_quiet"].item() == 1
    assert full["reference_seconds_since_fill_capped_1800"].item() == 1800
    missing = activity_states(query, ref.head(77), FROZEN)
    assert missing["reference_availability_known"].item() is False
    assert missing["reference_quiet"].item() is None
    delayed = ref.with_columns(
        pl.when(pl.col("point_id") == "77")
        .then(query_time)
        .otherwise(pl.col("evidence_available_at"))
        .alias("evidence_available_at")
    )
    assert activity_states(query, delayed, FROZEN)["reference_quiet"].item() is None


def test_reference_attempt_result_must_match_stored_activity_without_reselection():
    source = pl.DataFrame([row()])
    ref = reference(source).with_columns(
        pl.lit(True).alias("selected_attempt"), pl.lit(5.0).alias("filled_quantity")
    )
    events = study.reference_events(source, ref, FROZEN)
    assert events["filled_quantity"].item() == 5
    assert events["net_pnl"].item() == 2
    mismatched = ref.with_columns(pl.lit(4.0).alias("filled_quantity"))
    with pytest.raises(ValueError, match="immutable Q5 execution disagree"):
        study.reference_events(source, mismatched, FROZEN)


def event(second, *, known=True, selected=True, pnl=2.0):
    t = START + timedelta(seconds=second)
    return {
        "point_id": str(second),
        "market_id": str(second),
        "decision_at": t,
        "evidence_available_at": t + timedelta(milliseconds=150),
        "reference_known": known,
        "selected_attempt": selected,
        "filled_quantity": 5.0 if selected else 0.0,
        "probability_up": 0.8,
        "net_pnl": pnl,
        "stress_pnl": pnl,
        "reference_availability_known": True,
        "reference_quiet": 1.0,
        "reference_seconds_since_fill_capped_1800": 1800.0,
    }


def test_burst_profitability_breaks_on_unknown_and_missing_training_intervals():
    records = [
        event(19),
        event(39, selected=False),
        event(59, known=False, selected=False),
        event(79, pnl=-1),
        event(119, pnl=None),
    ]
    result = study.burst_records(records, FROZEN)
    assert result["trade_count"].to_list() == [1, 1, 1]
    assert result["close_reason"].to_list() == [
        "unknown_reference_observation",
        "source_or_eligibility_gap",
        "end_of_training_support",
    ]
    assert result["stress_pnl"].to_list() == [2, -1, None]
    assert result["preceding_quiet_seconds_censored_1800"].to_list() == [1800, 1800, 1800]
    assert not result["economics_complete"][-1]


def synthetic_run(root, monkeypatch):
    def save(path, value):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(value))

    source = frame()
    day = str(START.date())
    for layer in ["core", "refprice-twap", "flow", "simulated-refresh"]:
        path = root / "datasets" / layer / f"{day}.parquet"
        path.parent.mkdir(parents=True, exist_ok=True)
        source.write_parquet(path)
        save(
            root / "manifests" / f"{layer}-panels.json",
            {
                "status": "complete",
                "days": [{"date": day, "path": str(path), "sha256": sha256(path)}],
            },
        )
    ref = root / "predictions" / "reference.parquet"
    ref.parent.mkdir(parents=True)
    reference(source).write_parquet(ref)
    manifest = {
        "status": "complete",
        "identity": {"label_contract": LABELS},
        "days": [
            {
                "date": day,
                "status": "complete",
                "information_cutoff": START.isoformat(),
                "latest_fit_label_use_at": (START - timedelta(days=2)).isoformat(),
                "latest_calibration_label_use_at": (START - timedelta(days=1)).isoformat(),
                "scored_start": (START + timedelta(seconds=19)).isoformat(),
                "excluded_roles": ["calibration", "evaluation", "holdout"],
                "artifacts": [
                    {"kind": "reference-predictions", "path": str(ref), "sha256": sha256(ref)}
                ],
            }
        ],
    }
    ref_manifest = root / "manifests/simulated-refresh-reference.json"
    save(ref_manifest, manifest)
    activity = root / "manifests/simulated-refresh-panels.json"
    existing = json.loads(activity.read_text())
    save(activity, {**existing, "identity": {"reference_manifest": sha256(ref_manifest)}})
    save(root / "inputs/tournament-freeze.json", FROZEN)
    save(root / "inputs/candidate-feature-freeze.json", {"arms": ARMS})
    save(root / "inputs/label-availability-contract.json", LABELS)
    save(
        root / "manifests/run.json",
        {"exploration_eligibility": {"outcome_pattern_analysis_permitted": True}},
    )
    roles = root / "inputs/chronological-market-roles.parquet"
    source.select("market_id", "role", "role_fold").write_parquet(roles)
    save(
        root / "manifests/chronological-roles.json",
        {"frozen_before_exploration": True, "market_roles_sha256": sha256(roles)},
    )
    monkeypatch.setattr(study, "config", lambda _: FROZEN)
    monkeypatch.setattr(study, "label_contract", lambda _: LABELS)
    monkeypatch.setattr(study, "registry", lambda _: ARMS)
    monkeypatch.setattr(study, "read_day", lambda *args: source)
    return root


def test_completed_resume_verifies_outputs_and_refuses_corrupted_study(tmp_path, monkeypatch):
    run = synthetic_run(tmp_path, monkeypatch)
    study.build(run)
    manifest = run / "manifests/quiet-training-study.json"
    before = manifest.read_bytes()
    study.build(run)
    assert manifest.read_bytes() == before
    record = json.loads(before)
    assert record["checkpoint_accepted"] is False
    assert record["conditional_on_label_timing_assumption"] is True
    assert record["required_known_slots"] == 78
    artifact = record["days"][0]["artifacts"]["state-summary"]["path"]
    with open(artifact, "ab") as handle:
        handle.write(b"synthetic corruption")
    with pytest.raises(ValueError, match="checksum mismatch"):
        study.build(run)
