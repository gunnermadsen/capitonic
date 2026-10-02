"""Frozen chronological roles and bounded offline tournament configuration."""

from __future__ import annotations

import argparse
import json
from datetime import UTC, datetime
from pathlib import Path

import polars as pl

from .time_bucket_source_audit import checked_run, sha256, write_json


def boundary(value: str) -> datetime:
    return datetime.fromisoformat(value).replace(tzinfo=UTC)


def config(run: Path) -> dict:
    path = run / "inputs/tournament-freeze.json"
    frozen = json.loads(path.read_text())
    record = json.loads((run / "manifests/chronological-roles.json").read_text())
    if sha256(path) != record["configuration_sha256"]:
        raise ValueError("Frozen configuration changed")
    return frozen


def chronological_roles(population: pl.DataFrame, frozen: dict) -> pl.DataFrame:
    """Assign whole markets once; later fits cannot consume earlier evaluation days."""
    roles = population.select("market_id", "window_start", "window_end").with_columns(
        pl.lit("training").alias("role"), pl.lit(None, dtype=pl.String).alias("role_fold")
    )
    allocated: set[str] = set()
    for fold in frozen["folds"]:
        for role, start, end in [
            ("calibration", fold["calibration_start"], fold["evaluation_start"]),
            ("holdout" if fold["name"] == "holdout" else "evaluation",
             fold["evaluation_start"], fold["evaluation_end"]),
        ]:
            selected = pl.col("window_start").is_between(boundary(start), boundary(end), closed="left")
            ids = set(roles.filter(selected)["market_id"].to_list())
            if ids & allocated:
                raise ValueError("A market was assigned to multiple chronological roles")
            allocated.update(ids)
            roles = roles.with_columns(
                pl.when(selected).then(pl.lit(role)).otherwise(pl.col("role")).alias("role"),
                pl.when(selected).then(pl.lit(fold["name"])).otherwise(pl.col("role_fold")).alias("role_fold"),
            )
    if roles["market_id"].n_unique() != roles.height:
        raise ValueError("Duplicate markets in chronological population")
    return roles


def split_rows(frame: pl.DataFrame, frozen: dict, fold: dict, role: str) -> pl.DataFrame:
    purge = pl.duration(seconds=frozen["purge_seconds"])
    if role == "training":
        return frame.filter(
            (pl.col("role") == "training")
            & (pl.col("window_end") + purge <= boundary(fold["calibration_start"]))
        )
    if role == "calibration":
        return frame.filter(
            (pl.col("role") == "calibration") & (pl.col("role_fold") == fold["name"])
            & (pl.col("window_start") >= boundary(fold["calibration_start"]) + purge)
            & (pl.col("window_end") + purge <= boundary(fold["evaluation_start"]))
        )
    return frame.filter(
        pl.col("role").is_in(["evaluation", "holdout"])
        & (pl.col("role_fold") == fold["name"])
        & (pl.col("window_start") >= boundary(fold["evaluation_start"]) + purge)
        & (pl.col("window_end") <= boundary(fold["evaluation_end"]))
    )


def freeze(run: Path, configuration: Path) -> None:
    status_path = run / "manifests/run.json"
    status = json.loads(status_path.read_text())
    if status["checkpoints"]["source_freeze"]["status"] != "accepted":
        raise ValueError("Accept frozen input identities before assigning chronology")
    frozen = json.loads(configuration.read_text())
    target = run / "inputs/tournament-freeze.json"
    if target.exists():
        if json.loads(target.read_text()) != frozen:
            raise ValueError("Cannot replace an existing tournament freeze")
        config(run)
        return
    if len(frozen["folds"]) != 5 or frozen["folds"][-1]["name"] != "holdout":
        raise ValueError("Require four principal folds and a separate holdout")
    population_path = run / "inputs/market-identities.parquet"
    roles = chronological_roles(pl.read_parquet(population_path), frozen)
    roles_path = run / "inputs/chronological-market-roles.parquet"
    roles.write_parquet(roles_path, compression="zstd")
    write_json(target, frozen)
    write_json(run / "manifests/chronological-roles.json", {
        "configuration_sha256": sha256(target),
        "market_population_sha256": sha256(population_path),
        "market_roles_sha256": sha256(roles_path),
        "frozen_before_exploration": True,
        "counts": roles.group_by("role", "role_fold").len().sort("role", "role_fold").to_dicts(),
        "purge_seconds": frozen["purge_seconds"],
        "evaluation_excluded_from_later_training": True,
        "source_coverage_limitation": "All-four product intersection ends September 1; absent holdout source predictions remain explicit unavailable outcomes, not substituted lineages.",
        "frozen_at": datetime.now(UTC).isoformat(),
    })
    status["checkpoints"]["chronological_split_freeze"] = {
        "status": "accepted_before_exploration", "evidence": ["manifests/chronological-roles.json"],
        "coverage_balance": "Candidate-specific eligibility counts follow source validation without changing dates",
    }
    status["exploration_eligibility"] = {
        "status": "roles_frozen_panels_pending",
        "outcome_pattern_analysis_permitted": False,
        "required_filter": "role=training and window_end+purge <= exploration_end_exclusive",
    }
    status["resource_limits"].update(frozen["resources"])
    status["resource_limits"]["training_compute_budget"] = "Frozen configuration: one setting per fit, bounded policies, <=400 fitted bundles, <=8 hours, one job/two numeric threads."
    status["missing_outputs"] = [x for x in status["missing_outputs"] if x != "chronological_split_freeze"]
    write_json(status_path, status)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("run", type=Path)
    parser.add_argument("configuration", type=Path)
    args = parser.parse_args()
    freeze(checked_run(args.run), args.configuration)


if __name__ == "__main__":
    main()
