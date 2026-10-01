"""Synthetic prerequisite/resume checks that never invoke an estimator."""

import hashlib
import json
from copy import deepcopy
from types import SimpleNamespace

import polars as pl
import pytest

from btc_directional_model import time_bucket_reference, time_bucket_training


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def save(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, sort_keys=True) + "\n")


@pytest.fixture
def synthetic_run(tmp_path, monkeypatch):
    """The files are synthetic identities, not evidence for any actual tournament."""
    run = tmp_path / "synthetic-fit-integrity"
    fold = {"name": "synthetic_fold", "calibration_start": "2020-01-02",
            "evaluation_start": "2020-01-03", "evaluation_end": "2020-01-04"}
    arm = {"candidate": "synthetic_candidate", "arm": "synthetic_arm", "features": ["synthetic_feature"],
           "head": "direction", "matched_products": [], "entry_offsets": [19]}
    frozen = {"synthetic_only": True, "purge_seconds": 1800, "folds": [fold]}
    configuration = run / "inputs/tournament-freeze.json"
    save(configuration, frozen)
    roles = run / "manifests/chronological-roles.json"
    save(roles, {"configuration_sha256": digest(configuration)})
    features = run / "inputs/candidate-feature-freeze.json"
    save(features, {"synthetic_only": True, "arms": [arm]})
    panel_names = ["core-panels.json", "refprice-twap-panels.json", "flow-panels.json", "complete-case-panels.json"]
    for name in panel_names:
        save(run / "manifests" / name, {"synthetic_only": True, "status": "complete", "days": []})
    eligibility = run / "manifests/training-eligibility.json"
    save(eligibility, {"synthetic_only": True, "accepted_candidates": [arm["candidate"]]})
    predictions = run / "predictions/synthetic-calibration.parquet"
    predictions.parent.mkdir(parents=True)
    pl.DataFrame({"point_id": ["synthetic@19"], "probability_up": [.5]}).write_parquet(predictions)
    checkpoint = run / "checkpoints" / arm["candidate"] / arm["arm"] / fold["name"] / "manifest.json"
    artifact = checkpoint.with_name("synthetic-artifact.json")
    save(artifact, {"synthetic_only": True, "model_fitted": False})
    record = {"candidate": arm["candidate"], "arm": arm["arm"], "fold": deepcopy(fold),
              "label_availability_contract": {"synthetic_test_only": True},
              "features": list(arm["features"]), "configuration_sha256": digest(configuration),
              "feature_freeze_sha256": digest(features),
              "panel_manifests": {name: digest(run / "manifests" / name) for name in panel_names},
              "status": "complete", "artifacts": [{"path": str(artifact), "sha256": digest(artifact)}],
              "calibration_predictions": {"path": str(predictions), "sha256": digest(predictions)}}
    save(checkpoint, record)
    calls = []

    def forbidden(*args, **kwargs):
        calls.append("model_or_panel_access")
        pytest.fail("Prerequisite/resume tests must not access labels, fit, score, or reserve compute")

    for name in ("fit_model", "score", "split_rows", "regress"):
        monkeypatch.setattr(time_bucket_training, name, forbidden)
    # Isolate identity tests from any real label authorization. Gate tests restore
    # the real contract function and exercise only missing/invalid fixture files.
    contract = time_bucket_reference.label_contract
    monkeypatch.setattr(time_bucket_reference, "label_contract", lambda _: {"synthetic_test_only": True})
    return SimpleNamespace(run=run, arm=arm, fold=fold, configuration=configuration, roles=roles,
        features=features, eligibility=eligibility, checkpoint=checkpoint, predictions=predictions,
        record=record, calls=calls, contract=contract, budget=SimpleNamespace(reserve=forbidden, finish=forbidden))


def resume(fixture):
    return time_bucket_training.fit_fold(fixture.run, pl.DataFrame(), fixture.arm, fixture.fold, fixture.budget)


def test_unchanged_checkpoint_reuses_verified_artifacts_without_model_or_label_access(synthetic_run):
    assert resume(synthetic_run) == synthetic_run.record
    assert synthetic_run.calls == []


@pytest.mark.parametrize("changed", [
    "configuration", "feature_freeze", "arm_features", "fold",
    "core-panels.json", "refprice-twap-panels.json", "flow-panels.json", "complete-case-panels.json",
])
def test_current_identity_drift_rejects_existing_checkpoint(synthetic_run, changed):
    fixture = synthetic_run
    if changed == "configuration":
        configuration = json.loads(fixture.configuration.read_text())
        configuration["purge_seconds"] += 1
        save(fixture.configuration, configuration)
        # Make the current config internally consistent so this exercises the
        # saved fit identity comparison, rather than only the config-file guard.
        save(fixture.roles, {"configuration_sha256": digest(fixture.configuration)})
    elif changed == "feature_freeze":
        save(fixture.features, {"synthetic_only": True, "arms": [], "changed": True})
    elif changed == "arm_features":
        fixture.arm["features"] = ["different_synthetic_feature"]
    elif changed == "fold":
        fixture.fold["evaluation_end"] = "2020-01-05"
    else:
        save(fixture.run / "manifests" / changed, {"synthetic_only": True, "status": "complete", "changed": True})
    before = fixture.checkpoint.read_bytes()
    with pytest.raises(ValueError, match="current frozen input/feature/fold identity"):
        resume(fixture)
    assert fixture.checkpoint.read_bytes() == before
    assert fixture.calls == []


def test_corrupted_calibration_predictions_reject_checkpoint_reuse(synthetic_run):
    fixture = synthetic_run
    # Change a valid synthetic Parquet rather than relying on a parser failure.
    pl.DataFrame({"point_id": ["synthetic@19"], "probability_up": [.9]}).write_parquet(fixture.predictions)
    with pytest.raises(ValueError, match="Calibration predictions changed"):
        resume(fixture)
    assert fixture.calls == []


@pytest.mark.parametrize("accepted", [[], ["some_other_synthetic_candidate"]])
def test_missing_candidate_acceptance_blocks_new_fit_before_model_code(synthetic_run, accepted):
    fixture = synthetic_run
    fixture.checkpoint.unlink()
    save(fixture.eligibility, {"synthetic_only": True, "accepted_candidates": accepted})
    with pytest.raises(ValueError, match="Candidate-specific training checkpoint dependencies"):
        resume(fixture)
    assert not fixture.checkpoint.exists()
    assert fixture.calls == []


@pytest.mark.parametrize("contract, message", [
    (None, "awaits the official-label availability clarification"),
    ({"synthetic_only": True, "mode": "unsupported"}, "No supported authorized label"),
    ({"synthetic_only": True, "mode": "user_authorized_offline_assumption", "seconds_after_close": 1800},
     "requires explicit user authorization"),
])
def test_absent_or_unproven_label_contract_blocks_new_fit_before_model_code(synthetic_run, monkeypatch, contract, message):
    fixture = synthetic_run
    fixture.checkpoint.unlink()
    monkeypatch.setattr(time_bucket_reference, "label_contract", fixture.contract)
    if contract is not None:
        save(fixture.run / "inputs/label-availability-contract.json", contract)
    with pytest.raises(ValueError, match=message):
        resume(fixture)
    assert not fixture.checkpoint.exists()
    assert fixture.calls == []
