"""Small synthetic OOS orchestration fixtures; no estimator is ever fitted."""

import json
import os
from datetime import timedelta
from pathlib import Path
from types import SimpleNamespace

import numpy as np
import polars as pl
import pytest

from btc_directional_model import time_bucket_tournament as tournament
from btc_directional_model.time_bucket_candidates import QUIET_FIELDS, registry
from btc_directional_model.time_bucket_evaluation import chosen_rows
from btc_directional_model.time_bucket_execution import book_index
from btc_directional_model.time_bucket_flow_panels import FEATURES
from btc_directional_model.time_bucket_policy import intentions
from btc_directional_model.time_bucket_protocol import boundary
from btc_directional_model.time_bucket_replay import replay_rows
from btc_directional_model.time_bucket_source_audit import sha256


class ConstantProbability:
    def predict_proba(self, matrix):
        return np.tile([.1, .9], (len(matrix), 1))

    def fit(self, *args, **kwargs):
        pytest.fail("Synthetic OOS verification must never fit an estimator")


def save(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value) + "\n")


def contracts():
    source = os.environ.get("BTC_TOURNAMENT_CONTRACT_RUN")
    if source:
        root = Path(source)
        return (json.loads((root / "inputs/tournament-freeze.json").read_text()),
                json.loads((root / "inputs/candidate-feature-freeze.json").read_text())["arms"])
    root = Path(__file__).parents[1]
    return json.loads((root / "configs/btc-time-bucket-tournament.json").read_text()), registry(FEATURES)


def panel(fold, arms, frozen):
    fields = list(dict.fromkeys(name for arm in arms for name in arm["features"]))
    rows = []
    for category, hours in [("eligible", 1), ("active", 2), ("missing", 3), ("purged", 0), ("unknown_quiet", 4)]:
        start = boundary(fold["evaluation_start"]) + timedelta(hours=hours)
        market = f"synthetic_{category}_{fold['name']}"
        for bucket in frozen["bucket_starts"]:
            elapsed = bucket + frozen["regular_entry_offset"]
            row = dict.fromkeys(fields, .5)
            row.update(market_id=market, point_id=f"{market}@{elapsed}", window_start=start,
                window_end=start + timedelta(minutes=5), decision_at=start + timedelta(seconds=elapsed),
                seconds_elapsed=elapsed, bucket_start=bucket, entry_offset=frozen["regular_entry_offset"],
                role="holdout" if fold["name"] == "holdout" else "evaluation", role_fold=fold["name"],
                official_outcome="up", label_up=1, reference_quiet=0. if category == "active" else 1.,
                reference_availability_known=category != "unknown_quiet")
            for side in ("up", "down"):
                row.update({f"{side}_decision_partial_vwap_5": .4, f"{side}_limit_5": .4,
                            f"{side}_book_sha256": "synthetic_book", f"{side}_vwap_grid": [.4] * len(frozen["quantity_grid"])})
                for kind in ("fak", "fok"):
                    row.update({f"{side}_{kind}_evidence_known": True, f"{side}_{kind}_reason": "synthetic",
                        f"{side}_{kind}_quantity_5": 2. if kind == "fak" else 0.,
                        f"{side}_{kind}_price_5": .4 if kind == "fak" else None,
                        f"{side}_{kind}_net_5": 1. if kind == "fak" else 0.,
                        f"{side}_{kind}_stress_5": .9 if kind == "fak" else 0.})
            if category == "missing":
                row["up_best_bid"] = None
            if category == "unknown_quiet":
                row.update(dict.fromkeys(QUIET_FIELDS))
            rows.append(row)
    return pl.DataFrame(rows, schema_overrides={f"{side}_fok_price_5": pl.Float64 for side in ("up", "down")})


@pytest.fixture(scope="module")
def oos(tmp_path_factory):
    frozen, all_arms = contracts()
    names = {"conservative_selective_refresh", "quiet_explorer", "payoff_recovery_aware_selector"}
    arms = [arm for arm in all_arms if arm["candidate"] in names and arm["arm"] == "primary"]
    assert len(arms) == 3
    run = tmp_path_factory.mktemp("synthetic-oos")
    save(run / "inputs/tournament-freeze.json", frozen)
    save(run / "inputs/label-availability-contract.json", {"synthetic_only": True})
    save(run / "inputs/qualification-reporting-contract.json", {"synthetic_only": True})
    save(run / "inputs/candidate-feature-freeze.json", {"synthetic_only": True, "arms": arms})
    save(run / "manifests/chronological-roles.json", {"configuration_sha256": sha256(run / "inputs/tournament-freeze.json")})
    for name in ("independent-fold-training.json", "quiet-fold-training.json"):
        save(run / "manifests" / name, {"synthetic_only": True, "status": "complete_with_explicit_support_outcomes"})
    policy = {"confidence": frozen["policy_search"]["confidence"][0],
              "minimum_stressed_edge": frozen["policy_search"]["minimum_stressed_edge"][0],
              "maximum_share_cost": frozen["policy_search"]["maximum_share_cost"][0],
              "bucket_mask": frozen["bucket_starts"]}
    bundles = {arm["candidate"]: {"direction": (ConstantProbability(), ConstantProbability(), tuple(arm["features"]), 0.),
        "policy": policy, "selected_quantity": frozen["primary_quantity"], "value": None, "exit": None} for arm in arms}
    for fold in frozen["folds"]:
        destination = run / "datasets/core" / f"{fold['evaluation_start']}.parquet"
        destination.parent.mkdir(parents=True, exist_ok=True)
        panel(fold, arms, frozen).write_parquet(destination)
        for arm in arms:
            root = run / "checkpoints" / arm["candidate"] / arm["arm"] / fold["name"]
            available = arm["candidate"] != "payoff_recovery_aware_selector"
            artifact = root / "model-bundle.joblib"
            save(artifact, {"synthetic_only": True, "deterministic_stub": True, "fitted_estimator": False})
            save(root / "manifest.json", {"synthetic_only": True,
                "status": "complete" if available else "insufficient_complete_case_support",
                "artifacts": [{"path": str(artifact), "sha256": sha256(artifact)}]})
    for name in ["core-panels", "refprice-twap-panels", "flow-panels", "simulated-refresh-panels"]:
        save(run / "manifests" / f"{name}.json", {"status": "complete", "synthetic_only": True,
            "days": [{"date": path.stem, "path": str(path), "sha256": sha256(path)}
                     for path in sorted((run / "datasets/core").glob("*.parquet"))]})
    with pytest.MonkeyPatch.context() as patch:
        patch.setattr(tournament, "arms_at_freeze", lambda _: arms)
        patch.setattr(tournament, "read_day", lambda root, day, layers: pl.read_parquet(root / "datasets/core" / f"{day}.parquet"))
        patch.setattr(tournament.joblib, "load", lambda path: bundles[path.parents[2].name])
        tournament.score_evaluation(run)
    outputs = {arm["candidate"]: pl.read_parquet(sorted((run / "predictions/evaluation" / arm["candidate"] / "primary").glob("*.parquet"))) for arm in arms}
    return SimpleNamespace(run=run, frozen=frozen, arms=arms, outputs=outputs)


def category(frame, name):
    return frame.filter(pl.col("market_id").str.starts_with(f"synthetic_{name}_"))


def test_oos_retains_all_thirteen_bucket_identities_and_each_rejection(oos):
    for candidate, frame in oos.outputs.items():
        assert frame.height == 5 * 13 * len(oos.frozen["folds"])
        assert frame["point_id"].n_unique() == frame.height
        assert frame.group_by("market_id").agg(pl.col("bucket_start").n_unique())["bucket_start"].to_list() == [13] * 25
        assert category(frame, "purged")["admission_reason"].unique().to_list() == ["purged_boundary_market"]
        assert category(frame, "missing")["admission_reason"].unique().to_list() == ["missing_required_causal_features"]
        assert not category(frame, "purged")["admitted"].any()
        assert not category(frame, "missing")["admitted"].any()
        assert sorted(frame["bucket_start"].unique()) == oos.frozen["bucket_starts"]
        if candidate == "payoff_recovery_aware_selector":
            assert category(frame, "eligible")["admission_reason"].unique().to_list() == ["model_unavailable_insufficient_support"]


def test_unavailable_predictions_have_typed_nulls_without_a_fabricated_side(oos):
    unavailable = oos.outputs["payoff_recovery_aware_selector"]
    for name in ["probability_up", "probability_down", "conservative_probability_up", "chosen_probability",
                 "expected_value_up", "expected_value_down", "selected_quantity", "expected_stressed_value_per_share"]:
        assert unavailable.schema[name] == pl.Float64
        assert unavailable[name].null_count() == unavailable.height
    assert unavailable.schema["chosen_side"] == pl.String
    assert unavailable["chosen_side"].null_count() == unavailable.height
    available = oos.outputs["conservative_selective_refresh"]
    for name in ("missing", "purged"):
        rejected = category(available, name)
        assert rejected["probability_up"].null_count() == rejected.height
        assert rejected["chosen_side"].null_count() == rejected.height


def test_known_active_quiet_diagnostic_is_separate_from_quiet_only_admission(oos):
    quiet = oos.outputs["quiet_explorer"]
    active = category(quiet, "active")
    assert active["probability_up"].null_count() == 0
    assert active["general_market_diagnostic_admitted"].all()
    assert not active["admitted"].any()
    assert active["admission_reason"].unique().to_list() == ["outside_quiet_population"]
    assert category(quiet, "eligible")["admitted"].all()
    unknown = category(quiet, "unknown_quiet")
    assert not unknown["admitted"].any() and not unknown["general_market_diagnostic_admitted"].any()
    assert unknown["probability_up"].null_count() == unknown.height
    diagnostic = chosen_rows(active, "sequential_policy", general_market=True)
    assert diagnostic.height == len(oos.frozen["folds"])
    assert chosen_rows(active, "sequential_policy").is_empty()


def test_replay_keeps_first_unknown_attempt_and_independent_fak_fok(oos):
    frame = category(oos.outputs["conservative_selective_refresh"], "eligible").filter(pl.col("fold") == oos.frozen["folds"][0]["name"])
    attempts = chosen_rows(frame, "sequential_policy")
    assert attempts.height == 1 and attempts["bucket_start"].item() == 0
    later = frame.filter(pl.col("bucket_start") == 20)["decision_at"].item()
    market = attempts["market_id"].item()

    def books(timestamp):
        return book_index(pl.DataFrame([{"market_id": market, "outcome": side, "token_id": side,
            "book_sha256": "synthetic", "available_at": timestamp, "source_timestamp": timestamp,
            "received_at": timestamp, "sampled_at": timestamp, "provider_available_at": timestamp,
            "ingest_sequence": 1, "bids": '[["0.39","8"]]', "asks": '[["0.4","8"]]'} for side in ("up", "down")]))

    for kind in ("FAK", "FOK"):
        unknown = replay_rows(attempts, books(later), oos.frozen, 5, kind, {"name": "base"}, .55)
        assert unknown.height == 1 and not unknown["evidence_known"].item()
        assert unknown["net_pnl"].item() is None and unknown["stress_pnl"].item() is None
    now = attempts["decision_at"].item()
    fak, fok = [replay_rows(attempts, books(now), oos.frozen, 5, kind, {"name": "base"}, .55) for kind in ("FAK", "FOK")]
    assert fak["filled_quantity"].item() == 2 and fok["filled_quantity"].item() == 0
    assert fak["order_type"].item() == "FAK" and fok["order_type"].item() == "FOK"
    assert chosen_rows(frame, "bucket_diagnostic").height == 13


def test_bucket_counterfactual_keeps_neighbors_outside_sequential_mask(oos):
    frame = category(oos.outputs["conservative_selective_refresh"], "eligible").filter(pl.col("fold") == "fold_1")
    policy = {"confidence": .55, "minimum_stressed_edge": .01, "maximum_share_cost": .55, "bucket_mask": [120, 140, 160]}
    sequential = intentions(frame, policy, oos.frozen)
    diagnostic = intentions(frame, {**policy, "bucket_mask": oos.frozen["bucket_starts"]}, oos.frozen)
    frame = sequential.with_columns(diagnostic["admitted"].alias("bucket_diagnostic_admitted"))
    assert chosen_rows(frame, "sequential_policy")["bucket_start"].to_list() == [120]
    assert chosen_rows(frame, "bucket_diagnostic")["bucket_start"].to_list() == oos.frozen["bucket_starts"]
