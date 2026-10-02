"""Training-only quiet/reference diagnostics; no fitting or checkpoint acceptance."""

from __future__ import annotations

import argparse
import json
import math
from collections.abc import Iterable
from datetime import datetime
from pathlib import Path

import numpy as np
import polars as pl

from .time_bucket_candidates import complete_cases, registry
from .time_bucket_exploration import _save, clock_rows, execution_study, study_training
from .time_bucket_flow_panels import FEATURES
from .time_bucket_labels import eligible_labels, label_contract, with_label_availability
from .time_bucket_policy import cached_q5
from .time_bucket_protocol import boundary, config
from .time_bucket_source_audit import checked_run, sha256, write_json
from .time_bucket_training import read_day

STATE_FEATURES = [
    "reference_probability_up",
    "reference_no_admission_fraction",
    "reference_fills_300",
    "reference_fills_1800",
    "reference_seconds_since_fill_capped_1800",
]
EVENT_SCHEMA = {
    "point_id": pl.String,
    "market_id": pl.String,
    "decision_at": pl.Datetime("us", "UTC"),
    "evidence_available_at": pl.Datetime("us", "UTC"),
    "reference_known": pl.Boolean,
    "selected_attempt": pl.Boolean,
    "probability_up": pl.Float64,
    "filled_quantity": pl.Float64,
    "net_pnl": pl.Float64,
    "stress_pnl": pl.Float64,
    "reference_availability_known": pl.Boolean,
    "reference_quiet": pl.Float64,
    "reference_seconds_since_fill_capped_1800": pl.Float64,
}
BURST_SCHEMA = {
    "start": pl.Datetime("us", "UTC"),
    "end": pl.Datetime("us", "UTC"),
    "trade_count": pl.Int64,
    "markets": pl.Int64,
    "net_pnl": pl.Float64,
    "stress_pnl": pl.Float64,
    "economics_complete": pl.Boolean,
    "preceding_quiet_seconds_censored_1800": pl.Float64,
    "preceding_quiet_known": pl.Boolean,
    "close_reason": pl.String,
}


def allowed_training(frame: pl.DataFrame, frozen: dict, labels: dict) -> pl.DataFrame:
    """Apply role and label-time gates before any feature/outcome study or aggregation."""
    role = frame.filter(pl.col("role") == "training")
    # Recompute from the owning contract; an existing label_use_at cannot weaken it.
    role = role.drop("label_use_at", "verified_label_available_at", strict=False)
    timed = with_label_availability(role, labels)
    return eligible_labels(timed, boundary(frozen["exploration_end_exclusive"]))


def quiet_contract(frozen: dict) -> None:
    if (
        frozen["reference"]["quiet_seconds"] != 1800
        or frozen["bucket_starts"] != list(range(0, 260, 20))
        or frozen["regular_entry_offset"] != 19
    ):
        raise ValueError("Quiet study requires the frozen 1800-second/78-slot reference contract")


def state_rows(frame: pl.DataFrame) -> pl.DataFrame:
    known = pl.col("reference_availability_known").fill_null(False)
    return frame.with_columns(
        pl.when(known & (pl.col("reference_quiet") == 1))
        .then(pl.lit("known_quiet"))
        .when(known & (pl.col("reference_quiet") == 0))
        .then(pl.lit("known_active"))
        .otherwise(pl.lit("unknown"))
        .alias("reference_state")
    )


def state_summary(allowed: pl.DataFrame, frozen: dict) -> pl.DataFrame:
    regular = state_rows(
        allowed.filter(
            (pl.col("entry_offset") == frozen["regular_entry_offset"])
            & (pl.col("seconds_elapsed") < 260)
        )
    )
    records = []
    for (bucket, state), rows in regular.partition_by(
        ["bucket_start", "reference_state"], as_dict=True
    ).items():
        known = state != "unknown"
        for name in STATE_FEATURES:
            values = rows[name].cast(pl.Float64).to_numpy()
            finite = np.isfinite(values) & known
            x = values[finite]
            record = {
                "bucket_start": bucket,
                "reference_state": state,
                "feature": name,
                "rows": rows.height,
                "markets": rows["market_id"].n_unique(),
                "finite_rows": len(x),
                "missing_rows": rows.height - len(x),
                "minimum": float(x.min()) if len(x) else None,
                "maximum": float(x.max()) if len(x) else None,
                "sum": float(x.sum()),
                "mean": float(x.mean()) if len(x) else None,
                "mean_direction_confidence": None,
            }
            if name == "reference_probability_up" and len(x):
                record["mean_direction_confidence"] = float(np.maximum(x, 1 - x).mean())
            records.append(record)
    return pl.DataFrame(
        records,
        schema={
            "bucket_start": pl.Int64,
            "reference_state": pl.String,
            "feature": pl.String,
            **dict.fromkeys(["rows", "markets", "finite_rows", "missing_rows"], pl.Int64),
            **dict.fromkeys(
                ["minimum", "maximum", "sum", "mean", "mean_direction_confidence"], pl.Float64
            ),
        },
    )


def arm_eligibility(allowed: pl.DataFrame, arms: list[dict]) -> pl.DataFrame:
    records = []
    for arm in arms:
        rows = state_rows(clock_rows(allowed, arm))
        quiet = rows.filter(pl.col("reference_state") == "known_quiet")
        complete = complete_cases(quiet, arm)
        for bucket in range(0, 260, 20):
            part = rows.filter(pl.col("bucket_start") == bucket)
            valid = complete.filter(pl.col("bucket_start") == bucket)
            records.append(
                {
                    "candidate": arm["candidate"],
                    "arm": arm["arm"],
                    "bucket_start": bucket,
                    "training_rows": part.height,
                    "unknown_rows": part.filter(pl.col("reference_state") == "unknown").height,
                    "known_quiet_rows": part.filter(
                        pl.col("reference_state") == "known_quiet"
                    ).height,
                    "known_active_rows": part.filter(
                        pl.col("reference_state") == "known_active"
                    ).height,
                    "complete_case_rows": valid.height,
                    "complete_case_markets": valid["market_id"].n_unique(),
                    "up_labels": valid.filter(pl.col("label_up") == 1).height,
                    "down_labels": valid.filter(pl.col("label_up") == 0).height,
                }
            )
    return pl.DataFrame(records)


def capacity_study(quiet: pl.DataFrame, arms: list[dict], frozen: dict) -> pl.DataFrame:
    """Recorded displayed full-quantity ask VWAP, separate from FAK/FOK execution."""
    records = []
    grid = frozen["quantity_grid"]
    q5 = grid.index(5)
    for arm in arms:
        valid = complete_cases(clock_rows(quiet, arm), arm)
        for (bucket,), rows in valid.partition_by("bucket_start", as_dict=True).items():
            for side in ["up", "down"]:
                costs = rows[f"{side}_vwap_grid"].to_list()
                if any(value is not None and len(value) != len(grid) for value in costs):
                    raise ValueError("Stored capacity grid differs from the frozen quantity grid")
                for j, quantity in enumerate(grid):
                    finite = [
                        v[j]
                        for v in costs
                        if v is not None and v[j] is not None and math.isfinite(v[j])
                    ]
                    paired = [
                        v[j] - v[q5]
                        for v in costs
                        if v is not None
                        and v[j] is not None
                        and v[q5] is not None
                        and math.isfinite(v[j])
                        and math.isfinite(v[q5])
                    ]
                    records.append(
                        {
                            "candidate": arm["candidate"],
                            "arm": arm["arm"],
                            "bucket_start": bucket,
                            "side": side,
                            "quantity": float(quantity),
                            "rows": rows.height,
                            "observed_full_quantity_vwaps": len(finite),
                            "missing_vwaps": rows.height - len(finite),
                            "mean_vwap": float(np.mean(finite)) if finite else None,
                            "paired_q5_rows": len(paired),
                            "mean_vwap_change_from_q5": float(np.mean(paired)) if paired else None,
                        }
                    )
    return pl.DataFrame(
        records,
        schema={
            **dict.fromkeys(["candidate", "arm", "side"], pl.String),
            **dict.fromkeys(
                [
                    "bucket_start",
                    "rows",
                    "observed_full_quantity_vwaps",
                    "missing_vwaps",
                    "paired_q5_rows",
                ],
                pl.Int64,
            ),
            **dict.fromkeys(["quantity", "mean_vwap", "mean_vwap_change_from_q5"], pl.Float64),
        },
    )


def reference_events(allowed: pl.DataFrame, reference: pl.DataFrame, frozen: dict) -> pl.DataFrame:
    """Join recorded attempts; recover their cached Q5 result without admission or replay."""
    if reference["point_id"].n_unique() != reference.height:
        raise ValueError("Duplicate simulated reference identities")
    columns = [
        "point_id",
        "decision_at",
        "probability_up",
        "reference_known",
        "filled_quantity",
        "selected_attempt",
        "evidence_available_at",
    ]
    renamed = reference.select(columns).rename(
        {n: f"stored_{n}" for n in columns if n != "point_id"}
    )
    rows = allowed.filter(
        (pl.col("entry_offset") == frozen["regular_entry_offset"])
        & (pl.col("seconds_elapsed") < 260)
    ).join(renamed, on="point_id", how="left", validate="1:1")
    if rows.filter(
        pl.col("stored_decision_at").is_not_null()
        & (pl.col("stored_decision_at") != pl.col("decision_at"))
    ).height:
        raise ValueError("Reference point identity has a different decision timestamp")
    known = pl.col("stored_reference_known").fill_null(False)
    if rows.filter(
        known
        & (
            ~pl.col("stored_probability_up").is_finite().fill_null(False)
            | ~pl.col("stored_filled_quantity").is_finite().fill_null(False)
            | pl.col("stored_selected_attempt").is_null()
            | pl.col("stored_evidence_available_at").is_null()
            | (pl.col("stored_evidence_available_at") < pl.col("decision_at"))
        )
    ).height:
        raise ValueError(
            "Known reference observations lack their original prediction/activity clock"
        )
    # The immutable refresh owner is a direction head: selected_attempts uses p>=.5.
    # Only stored selected attempts are used; no current policy is chosen or reapplied.
    chosen = rows.with_columns(
        pl.when(pl.col("stored_probability_up") >= 0.5)
        .then(pl.lit("up"))
        .otherwise(pl.lit("down"))
        .alias("chosen_side")
    )
    cached = cached_q5(chosen)
    selected = known & pl.col("stored_selected_attempt").fill_null(False)
    mismatch = selected & (
        ~pl.col("evidence_known").fill_null(False)
        | ~pl.col("filled_quantity").eq_missing(pl.col("stored_filled_quantity"))
    )
    if cached.filter(mismatch).height:
        raise ValueError("Stored reference attempt and immutable Q5 execution disagree")
    return (
        cached.select(
            "point_id",
            "market_id",
            "decision_at",
            pl.col("stored_evidence_available_at").alias("evidence_available_at"),
            known.alias("reference_known"),
            pl.col("stored_selected_attempt").alias("selected_attempt"),
            pl.col("stored_probability_up").alias("probability_up"),
            pl.col("stored_filled_quantity").alias("filled_quantity"),
            *(
                pl.when(selected).then(pl.col(n)).otherwise(pl.lit(None, dtype=pl.Float64)).alias(n)
                for n in ["net_pnl", "stress_pnl"]
            ),
            "reference_availability_known",
            "reference_quiet",
            "reference_seconds_since_fill_capped_1800",
        )
        .cast(EVENT_SCHEMA)
        .sort("decision_at")
    )


def burst_records(events: Iterable[dict], frozen: dict) -> pl.DataFrame:
    """Chronological training bursts stop at missing/unknown/protected evidence gaps."""
    records, current, previous = [], [], None

    def flush(reason):
        if not current:
            return
        first = current[0]
        complete = all(
            all(row[n] is not None and math.isfinite(row[n]) for n in ["net_pnl", "stress_pnl"])
            for row in current
        )
        quiet_known = bool(first["reference_availability_known"] and first["reference_quiet"] == 1)
        records.append(
            {
                "start": first["evidence_available_at"],
                "end": current[-1]["evidence_available_at"],
                "trade_count": len(current),
                "markets": len({r["market_id"] for r in current}),
                "net_pnl": sum(r["net_pnl"] for r in current) if complete else None,
                "stress_pnl": sum(r["stress_pnl"] for r in current) if complete else None,
                "economics_complete": complete,
                "preceding_quiet_known": quiet_known,
                "preceding_quiet_seconds_censored_1800": first[
                    "reference_seconds_since_fill_capped_1800"
                ]
                if quiet_known
                else None,
                "close_reason": reason,
            }
        )
        current.clear()

    for event in events:
        now = event["decision_at"]
        if previous is not None:
            if now <= previous:
                raise ValueError("Reference study events must be strictly chronological")
            expected = 60 if (int(previous.timestamp()) % 300) == 259 else 20
            if (now - previous).total_seconds() != expected:
                flush("source_or_eligibility_gap")
        previous = now
        if not event["reference_known"]:
            flush("unknown_reference_observation")
            continue
        if not event["selected_attempt"] or not (event["filled_quantity"] > 0):
            continue
        if (
            current
            and (
                event["evidence_available_at"] - current[-1]["evidence_available_at"]
            ).total_seconds()
            > frozen["charts"]["burst_max_gap_minutes"] * 60
        ):
            flush("frozen_burst_gap")
        current.append(event)
    flush("end_of_training_support")
    return pl.DataFrame(records, schema=BURST_SCHEMA)


def study_day(
    frame: pl.DataFrame, reference: pl.DataFrame, arms: list[dict], frozen: dict, labels: dict
) -> tuple[dict[str, pl.DataFrame], int]:
    quiet_contract(frozen)
    allowed = allowed_training(frame, frozen, labels)
    quiet = state_rows(allowed).filter(pl.col("reference_state") == "known_quiet")
    # Existing study owners skip quiet arms pending reference evidence. Here the
    # population is already filtered with that exact rule; feature lists stay fixed.
    descriptive_arms = [{**arm, "quiet_only": False} for arm in arms]
    distributions, associations = study_training(quiet, descriptive_arms, frozen)
    return {
        "state-summary": state_summary(allowed, frozen),
        "arm-eligibility": arm_eligibility(allowed, arms),
        "feature-distributions": distributions,
        "feature-associations": associations,
        "execution-counterfactuals": execution_study(quiet, descriptive_arms, frozen),
        "displayed-capacity": capacity_study(quiet, arms, frozen),
        "reference-events": reference_events(allowed, reference, frozen),
    }, allowed.height


def build(run: Path) -> None:
    frozen, labels = config(run), label_contract(run)
    quiet_contract(frozen)
    state = json.loads((run / "manifests/run.json").read_text())
    if (
        state.get("exploration_eligibility", {}).get("outcome_pattern_analysis_permitted")
        is not True
    ):
        raise ValueError("Primary must authorize training exploration before this study")
    feature_path = run / "inputs/candidate-feature-freeze.json"
    feature_freeze = json.loads(feature_path.read_text())
    if feature_freeze["arms"] != registry(FEATURES):
        raise ValueError("Candidate feature registry differs from the pre-exploration freeze")
    arms = [arm for arm in feature_freeze["arms"] if arm.get("quiet_only")]
    names = [
        "core-panels",
        "refprice-twap-panels",
        "flow-panels",
        "simulated-refresh-panels",
        "simulated-refresh-reference",
    ]
    manifests = {
        name: json.loads((run / "manifests" / f"{name}.json").read_text()) for name in names
    }
    if any(value["status"] != "complete" for value in manifests.values()):
        raise ValueError("Quiet study requires completed panels and causal refresh predictions")
    ref_path = run / "manifests/simulated-refresh-reference.json"
    if manifests["simulated-refresh-panels"]["identity"]["reference_manifest"] != sha256(ref_path):
        raise ValueError("Activity panels do not bind the completed reference predictions")
    if manifests["simulated-refresh-reference"]["identity"]["label_contract"] != labels:
        raise ValueError("Reference labels do not match the authorized timing contract")
    roles_path = run / "inputs/chronological-market-roles.parquet"
    role_record = json.loads((run / "manifests/chronological-roles.json").read_text())
    if (
        sha256(roles_path) != role_record["market_roles_sha256"]
        or not role_record["frozen_before_exploration"]
    ):
        raise ValueError("Frozen training roles changed")
    identity = {
        "configuration": sha256(run / "inputs/tournament-freeze.json"),
        "features": sha256(feature_path),
        "labels": sha256(run / "inputs/label-availability-contract.json"),
        "roles": sha256(roles_path),
        "manifests": {n: sha256(run / "manifests" / f"{n}.json") for n in names},
        "code": {
            n: sha256(Path(__file__).with_name(n))
            for n in [
                "time_bucket_quiet_study.py",
                "time_bucket_exploration.py",
                "time_bucket_training.py",
                "time_bucket_candidates.py",
                "time_bucket_labels.py",
                "time_bucket_policy.py",
                "time_bucket_activity.py",
            ]
        },
    }
    destination = run / "manifests/quiet-training-study.json"
    report = (
        json.loads(destination.read_text())
        if destination.exists()
        else {"identity": identity, "status": "in_progress", "days": []}
    )
    if report["identity"] != identity:
        raise ValueError("Quiet study identity changed; refusing mixed-evidence resume")
    if report["status"] == "complete_pending_primary_checkpoint_acceptance":
        for record in report["days"]:
            for artifact in record["artifacts"].values():
                if sha256(Path(artifact["path"])) != artifact["sha256"]:
                    raise ValueError("Quiet study artifact checksum mismatch")
        for artifact in report["outputs"]:
            if sha256(Path(artifact["path"])) != artifact["sha256"]:
                raise ValueError("Quiet study summary checksum mismatch")
        return
    by_day = {name: {r["date"]: r for r in value["days"]} for name, value in manifests.items()}
    if any(set(value) != set(by_day["core-panels"]) for value in by_day.values()):
        raise ValueError("Quiet study inputs omit scheduled source dates")
    done = {r["date"]: r for r in report["days"]}
    roles = pl.read_parquet(roles_path).select("market_id", "role", "role_fold")
    for day in sorted(by_day["core-panels"]):
        if boundary(day) >= boundary(frozen["exploration_end_exclusive"]):
            continue  # Do not even load protected later-day outcomes for this study.
        if day in done:
            for artifact in done[day]["artifacts"].values():
                if sha256(Path(artifact["path"])) != artifact["sha256"]:
                    raise ValueError("Recorded daily quiet study artifact changed")
            continue
        for name in names[:-1]:
            record = by_day[name][day]
            if sha256(Path(record["path"])) != record["sha256"]:
                raise ValueError("Frozen quiet study panel changed")
        ref = by_day["simulated-refresh-reference"][day]
        if ref["status"] == "complete":
            cutoff = datetime.fromisoformat(ref["information_cutoff"])
            if (
                any(
                    datetime.fromisoformat(ref[name]) > cutoff
                    for name in ["latest_fit_label_use_at", "latest_calibration_label_use_at"]
                )
                or cutoff > datetime.fromisoformat(ref["scored_start"])
                or set(ref["excluded_roles"]) != {"calibration", "evaluation", "holdout"}
            ):
                raise ValueError(
                    "Reference chronology cannot establish training-only causal study eligibility"
                )
        reference = next(r for r in ref["artifacts"] if r["kind"] == "reference-predictions")
        if sha256(Path(reference["path"])) != reference["sha256"]:
            raise ValueError("Stored simulated reference prediction changed")
        frame = read_day(run, day, ["refprice-twap", "flow", "simulated-refresh"])
        checked = (
            frame.select("market_id", "role", "role_fold")
            .unique()
            .join(roles, on="market_id", how="left", validate="1:1", suffix="_frozen")
        )
        if checked.filter(
            ~pl.col("role").eq_missing(pl.col("role_frozen"))
            | ~pl.col("role_fold").eq_missing(pl.col("role_fold_frozen"))
        ).height:
            raise ValueError("Panel roles differ from the frozen training allocation")
        tables, count = study_day(frame, pl.read_parquet(reference["path"]), arms, frozen, labels)
        report["days"].append(
            {
                "date": day,
                "training_rows": count,
                "artifacts": {
                    name: _save(
                        run / "metrics/quiet-study" / day / f"{name}.parquet",
                        table.with_columns(pl.lit(day).alias("date")),
                    )
                    for name, table in tables.items()
                },
            }
        )
        write_json(destination, report)
        print(json.dumps({"quiet_study_date": day, "allowed_training_rows": count}), flush=True)
    records = sorted(report["days"], key=lambda r: r["date"])

    def events():
        for record in records:
            yield from pl.read_parquet(record["artifacts"]["reference-events"]["path"]).iter_rows(
                named=True
            )

    bursts = burst_records(events(), frozen)
    summary = (
        pl.concat([pl.read_parquet(r["artifacts"]["arm-eligibility"]["path"]) for r in records])
        .group_by("candidate", "arm")
        .agg(
            pl.col(
                "training_rows",
                "unknown_rows",
                "known_quiet_rows",
                "known_active_rows",
                "complete_case_rows",
                "up_labels",
                "down_labels",
            ).sum(),
            pl.col("date")
            .filter(pl.col("complete_case_rows") > 0)
            .n_unique()
            .alias("days_with_complete_cases"),
        )
    )
    outputs = [
        _save(run / "metrics/quiet-study/burst-profitability.parquet", bursts),
        _save(run / "metrics/quiet-study/arm-training-support.parquet", summary),
    ]
    markdown = run / "diagnostics/quiet-training-study.md"
    if markdown.exists():
        raise ValueError("Unrecorded quiet study report must not be overwritten")
    markdown.parent.mkdir(parents=True, exist_ok=True)
    markdown.write_text(
        "# Training-only quiet/reference study\n\n"
        f"{sum(r['training_rows'] for r in records)} points on {len(records)} dates passed role=training and "
        f"label_use_at <= {frozen['exploration_end_exclusive']} UTC. The label clock is conditional on the authorized close+1800 assumption, "
        "with verified later times respected. No calibration, evaluation or holdout outcomes enter these tables.\n\n"
        "Quiet means all 78 causal reference slots in the preceding1800 seconds are known and no selected simulated entry filled. "
        "Unknown is separate from quiet and active. Reference confidence uses the stored strictly-prior probability; no-admission fraction "
        "and recent fills use the existing activity owner. Elapsed time is censored at1800 seconds, not an exact last-trade timestamp.\n\n"
        "Daily Parquet tables preserve per-arm complete-case support, finite/missing feature distributions, direction/loss/value associations, "
        "both-side Q5 FAK/FOK counterfactuals and displayed Q1–Q200 VWAP capacity. Counterfactual sums are not sequential income. "
        "Burst results use only recorded simulated-refresh selected attempts and their matching cached Q5 fills; bursts stop at unknown or "
        "missing/protected intervals. They are not quiet-candidate model results or actual incumbent paper/live trades.\n\n"
        "Exact confidence rejection reasons and an uncensored actual last-trade time are not stored and were not invented. "
        "No new groups, thresholds, policies, quantities or hypotheses were selected. Zero complete cases is an explicit support limitation. "
        "The support table supplies evidence for primary-owned candidate checkpoint acceptance; this study accepts no checkpoints.\n\n"
        + "\n".join(
            f"- {r['candidate']}/{r['arm']}: {r['complete_case_rows']} complete quiet training points on {r['days_with_complete_cases']} days."
            for r in summary.to_dicts()
        )
        + "\n"
    )
    outputs.append({"path": str(markdown), "sha256": sha256(markdown)})
    report.update(
        status="complete_pending_primary_checkpoint_acceptance",
        outputs=outputs,
        candidate_support=summary.to_dicts(),
        checkpoint_accepted=False,
        conditional_on_label_timing_assumption=True,
        actual_incumbent_activity_proven=False,
        frozen_quiet_seconds=1800,
        required_known_slots=78,
        limitations=[
            "Censored quiet duration is not actual time since last trade",
            "No stored exact confidence rejection reason",
            "Displayed capacity is not execution; Q5 FAK/FOK counterfactuals and selected reference bursts remain separate",
        ],
    )
    write_json(destination, report)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("run", type=Path)
    args = parser.parse_args()
    build(checked_run(args.run))


if __name__ == "__main__":
    main()
