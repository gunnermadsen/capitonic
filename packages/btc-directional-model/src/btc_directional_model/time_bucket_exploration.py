"""Descriptive training-only exploration and outcome-blind source/arm coverage."""

from __future__ import annotations

import argparse
import json
from pathlib import Path

import numpy as np
import polars as pl

from . import time_bucket_candidates, time_bucket_training
from .time_bucket_candidates import complete_cases, prefixes, reference_features, registry
from .time_bucket_flow_panels import FEATURES
from .time_bucket_protocol import boundary, config
from .time_bucket_source_audit import PRODUCTS, checked_run, sha256, write_json
from .time_bucket_training import read_day

# Fixed bins describe heterogeneous declared features without learning thresholds.
HISTOGRAM_EDGES = [-np.inf, -10000, -1000, -100, -10, -1, -.1, 0,
                   .1, 1, 10, 100, 1000, 10000, np.inf]
KEYS = ["candidate", "arm"]
PENDING = "pending_causal_simulated_refresh"


def training_rows(frame: pl.DataFrame, frozen: dict) -> pl.DataFrame:
    """Apply the frozen role and purge boundary before accessing outcomes or returns."""
    return frame.filter(
        (pl.col("role") == "training")
        & (pl.col("window_end") + pl.duration(seconds=frozen["purge_seconds"])
           <= boundary(frozen["exploration_end_exclusive"]))
    )


def clock_rows(frame: pl.DataFrame, arm: dict) -> pl.DataFrame:
    return frame.filter(pl.col("entry_offset").is_in(arm["entry_offsets"])
                        & (pl.col("seconds_elapsed") < 260))


def required_features(arm: dict) -> list[str]:
    return list(dict.fromkeys(arm["features"] + [name for product in arm["matched_products"]
                                                for name in reference_features([product])]))


def source_groups() -> dict[str, list[str]]:
    groups = {source: [f"{source}_price"] for source in ("rtds_chainlink", "direct_binance")}
    groups.update({product: [f"{prefix}_price" for prefix in prefixes(product)] for product in PRODUCTS})
    groups.update({product: [f"{product}_available_at"] for product in FEATURES})
    groups.update({f"polymarket_{side}": [f"{side}_book_available_at"] for side in ("up", "down")})
    return groups


def source_presence(frame: pl.DataFrame) -> pl.DataFrame:
    """Record every configured within-bucket point, without accessing any outcome."""
    groups = source_groups()
    frame = frame.filter(pl.col("seconds_elapsed") < 260)
    expressions = []
    for i, fields in enumerate(groups.values()):
        present = pl.all_horizontal(pl.col(name).is_not_null() for name in fields)
        if set(fields) - set(frame.columns):
            raise ValueError(f"Source-presence schema missing: {fields}")
        expressions.append(present.cast(pl.UInt64) * (1 << i))
    return frame.select("market_id", "window_start", "role", "role_fold", "bucket_start",
                        "entry_offset", "decision_at", pl.sum_horizontal(expressions).alias("source_mask"))


def coverage(frame: pl.DataFrame, arms: list[dict]) -> pl.DataFrame:
    """Only presence, roles and row counts are used, including on protected dates."""
    records = []
    for arm in arms:
        eligible = clock_rows(frame, arm)
        grouping = ["bucket_start", "role", "role_fold"]
        totals = eligible.group_by(grouping).agg(
            pl.len().alias("decision_rows"), pl.col("market_id").n_unique().alias("markets"))
        missing = sorted(set(required_features(arm)) - set(eligible.columns))
        pending = bool(arm.get("quiet_only"))
        if missing and not pending:
            raise ValueError(f"Declared non-quiet feature schema missing: {missing}")
        if pending:
            counts = totals.with_columns(pl.lit(None, dtype=pl.UInt32).alias("eligible_rows"),
                                         pl.lit(None, dtype=pl.UInt32).alias("eligible_markets"))
        else:
            valid = complete_cases(eligible, arm).group_by(grouping).agg(
                pl.len().alias("eligible_rows"), pl.col("market_id").n_unique().alias("eligible_markets"))
            counts = totals.join(valid, on=grouping, how="left", nulls_equal=True).with_columns(
                pl.col("eligible_rows", "eligible_markets").fill_null(0))
        records.append(counts.with_columns(
            pl.lit(arm["candidate"]).alias("candidate"), pl.lit(arm["arm"]).alias("arm"),
            pl.lit(PENDING if pending else "measured").alias("status"),
            pl.lit(",".join(missing)).alias("missing_schema_features")))
    return pl.concat(records, how="vertical_relaxed").sort(*KEYS, "bucket_start", "role")


def _values(frame: pl.DataFrame, field: str) -> np.ndarray:
    return frame[field].cast(pl.Float64).to_numpy()


def _targets(frame: pl.DataFrame) -> dict[str, np.ndarray]:
    """Called only after training eligibility and arm-clock filtering."""
    targets = {"label_up": _values(frame, "label_up")}
    for side in ("up", "down"):
        for kind in ("fak", "fok"):
            prefix = f"{side}_{kind}"
            known = frame[f"{prefix}_evidence_known"].fill_null(False).to_numpy()
            quantity = _values(frame, f"{prefix}_quantity_5")
            pnl = _values(frame, f"{prefix}_stress_5")
            executed = known & np.isfinite(quantity) & (quantity > 0) & np.isfinite(pnl)
            targets[f"{prefix}_fill_ratio"] = np.where(known, quantity / 5, np.nan)
            targets[f"{prefix}_stress_per_filled_share"] = np.divide(
                pnl, quantity, out=np.full(frame.height, np.nan), where=executed)
            targets[f"{prefix}_loss_when_filled"] = np.where(executed, pnl < 0, np.nan)
    return targets


def study_training(frame: pl.DataFrame, arms: list[dict], frozen: dict) -> tuple[pl.DataFrame, pl.DataFrame]:
    """Fixed per-feature distributions and pairwise associations; no feature selection."""
    allowed = training_rows(frame, frozen)
    distributions, associations = [], []
    for arm in arms:
        if arm.get("quiet_only"):
            continue
        selected = clock_rows(allowed, arm)
        if not selected.height:
            continue
        targets = _targets(selected)
        for name in arm["features"]:
            value = _values(selected, name)
            finite = np.isfinite(value)
            x = value[finite]
            identity = {"candidate": arm["candidate"], "arm": arm["arm"], "feature": name}
            distributions.append({**identity, "rows": selected.height, "finite_rows": int(finite.sum()),
                "missing_rows": int((~finite).sum()), "sum": float(x.sum()), "sum_squares": float(x @ x),
                "minimum": float(x.min()) if len(x) else None, "maximum": float(x.max()) if len(x) else None,
                "mean": float(x.mean()) if len(x) else None, "stddev": float(x.std()) if len(x) else None,
                "histogram_counts": np.histogram(x, bins=HISTOGRAM_EDGES)[0].tolist()})
            for target, outcome in targets.items():
                paired = finite & np.isfinite(outcome)
                x, y = value[paired], outcome[paired]
                correlation = None
                if len(x) >= 2 and np.ptp(x) > 0 and np.ptp(y) > 0:
                    correlation = float(np.corrcoef(x, y)[0, 1])
                associations.append({**identity, "target": target, "paired_rows": len(x),
                    "sum_x": float(x.sum()), "sum_y": float(y.sum()), "sum_x2": float(x @ x),
                    "sum_y2": float(y @ y), "sum_xy": float(x @ y), "pearson_correlation": correlation})
    distribution_schema = {**dict.fromkeys(KEYS + ["feature"], pl.String),
        **dict.fromkeys(["rows", "finite_rows", "missing_rows"], pl.Int64),
        **dict.fromkeys(["sum", "sum_squares", "minimum", "maximum", "mean", "stddev"], pl.Float64),
        "histogram_counts": pl.List(pl.Int64)}
    association_schema = {**dict.fromkeys(KEYS + ["feature", "target"], pl.String),
        "paired_rows": pl.Int64,
        **dict.fromkeys(["sum_x", "sum_y", "sum_x2", "sum_y2", "sum_xy", "pearson_correlation"], pl.Float64)}
    return (pl.DataFrame(distributions, schema=distribution_schema),
            pl.DataFrame(associations, schema=association_schema))


def execution_study(frame: pl.DataFrame, arms: list[dict], frozen: dict) -> pl.DataFrame:
    """Training-only bucket counterfactuals, never sequential-policy economics."""
    allowed = training_rows(frame, frozen)
    records = []
    for arm in arms:
        if arm.get("quiet_only"):
            continue
        selected = complete_cases(clock_rows(allowed, arm), arm)
        for side in ("up", "down"):
            for kind in ("fak", "fok"):
                prefix = f"{side}_{kind}"
                for key, bucket in selected.partition_by("bucket_start", as_dict=True).items():
                    known = bucket[f"{prefix}_evidence_known"].fill_null(False).to_numpy()
                    quantity = _values(bucket, f"{prefix}_quantity_5")
                    net, stress = [_values(bucket, f"{prefix}_{field}_5") for field in ("net", "stress")]
                    known &= np.isfinite(quantity) & np.isfinite(net) & np.isfinite(stress)
                    filled = known & (quantity > 0)
                    values = stress[filled]
                    records.append({"candidate": arm["candidate"], "arm": arm["arm"], "bucket_start": key[0],
                        "side": side, "order_type": kind.upper(), "rows": bucket.height,
                        "known_rows": int(known.sum()), "unknown_rows": int((~known).sum()),
                        "fills": int(filled.sum()), "partial_fills": int((filled & (quantity < 5 - 1e-9)).sum()),
                        "filled_shares": float(quantity[known].sum()), "unfilled_shares": float((5 - quantity[known]).sum()),
                        "net_pnl": float(net[filled].sum()), "stress_pnl": float(values.sum()),
                        "stress_gains": float(values[values > 0].sum()), "stress_losses": float(-values[values < 0].sum())})
    return pl.DataFrame(records, schema={**dict.fromkeys(KEYS + ["side", "order_type"], pl.String),
        **dict.fromkeys(["bucket_start", "rows", "known_rows", "unknown_rows", "fills", "partial_fills"], pl.Int64),
        **dict.fromkeys(["filled_shares", "unfilled_shares", "net_pnl", "stress_pnl", "stress_gains", "stress_losses"], pl.Float64)})


def _save(path: Path, frame: pl.DataFrame) -> dict:
    if path.exists():
        raise ValueError(f"Refusing to overwrite unrecorded exploration artifact: {path}")
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(".parquet.pending")
    frame.write_parquet(temporary, compression="zstd")
    temporary.rename(path)
    return {"path": str(path), "sha256": sha256(path), "rows": frame.height}


def build(run: Path) -> None:
    frozen = config(run)
    arms = registry(FEATURES)
    run_status = json.loads((run / "manifests/run.json").read_text())
    if run_status.get("exploration_eligibility", {}).get("outcome_pattern_analysis_permitted") is not True:
        raise ValueError("Primary must establish and record exploration eligibility before outcome analysis")
    paths = {layer: run / "manifests" / f"{layer}-panels.json" for layer in ("core", "refprice-twap", "flow")}
    panels = {layer: json.loads(path.read_text()) for layer, path in paths.items()}
    if any(panel["status"] != "complete" for panel in panels.values()):
        raise ValueError("Complete all three causal panel layers before exploration")
    roles_path = run / "inputs/chronological-market-roles.parquet"
    role_manifest = json.loads((run / "manifests/chronological-roles.json").read_text())
    if sha256(roles_path) != role_manifest["market_roles_sha256"] or not role_manifest["frozen_before_exploration"]:
        raise ValueError("Frozen market roles must be verified before exploration")
    feature_freeze = run / "inputs/candidate-feature-freeze.json"
    if not feature_freeze.exists():
        raise ValueError("Primary-owned feature/hypothesis freeze must exist before exploration")
    if json.loads(feature_freeze.read_text()).get("arms") != arms:
        raise ValueError("Declared candidate registry differs from the primary-owned feature freeze")
    identity = {"configuration_sha256": sha256(run / "inputs/tournament-freeze.json"),
                "feature_freeze_sha256": sha256(feature_freeze), "roles_sha256": sha256(roles_path),
                "panel_manifests": {layer: {"path": str(path), "sha256": sha256(path)} for layer, path in paths.items()},
                "implementation_sha256": {str(path): sha256(path) for path in
                    [Path(__file__), Path(time_bucket_training.__file__), Path(time_bucket_candidates.__file__)]}}
    path = run / "manifests/training-exploration.json"
    manifest = json.loads(path.read_text()) if path.exists() else {"identity": identity, "status": "in_progress", "days": []}
    if manifest["identity"] != identity:
        raise ValueError("Exploration sources/features changed; refusing mixed-evidence resume")
    done = {record["date"]: record for record in manifest["days"]}
    panel_days = {layer: {record["date"]: record for record in panel["days"]} for layer, panel in panels.items()}
    if any(set(days) != set(panel_days["core"]) for days in panel_days.values()):
        raise ValueError("Panel dates must align, including explicit empty source days")
    roles = pl.read_parquet(roles_path).select("market_id", "role", "role_fold")
    totals = {"training_rows": 0, "covered_days": 0}
    for day in sorted(panel_days["core"]):
        if day in done:
            for artifact in done[day]["artifacts"].values():
                if sha256(Path(artifact["path"])) != artifact["sha256"]:
                    raise ValueError("Existing exploration artifact changed")
            totals["training_rows"] += done[day]["training_rows"]
            totals["covered_days"] += 1
            continue
        for layer, days in panel_days.items():
            record = days[day]
            expected = run / "datasets" / layer / f"{day}.parquet"
            if Path(record["path"]).resolve() != expected.resolve() or sha256(expected) != record["sha256"]:
                raise ValueError(f"Frozen panel identity changed: {layer}/{day}")
        frame = read_day(run, day, ["refprice-twap", "flow"])
        checked = frame.select("market_id", "role", "role_fold").unique().join(
            roles, on="market_id", how="left", validate="1:1", suffix="_frozen")
        if checked.filter(~pl.col("role").eq_missing(pl.col("role_frozen"))
                          | ~pl.col("role_fold").eq_missing(pl.col("role_fold_frozen"))).height:
            raise ValueError("Panel roles disagree with the frozen assignment")
        distributions, associations = study_training(frame, arms, frozen)
        tables = {"source-presence": source_presence(frame), "arm-coverage": coverage(frame, arms),
                  "feature-distributions": distributions, "feature-associations": associations,
                  "execution-counterfactuals": execution_study(frame, arms, frozen)}
        record = {"date": day, "training_rows": training_rows(frame, frozen).height,
                  "artifacts": {name: _save(run / "metrics/exploration" / day / f"{name}.parquet",
                                             table.with_columns(pl.lit(day).alias("date")))
                                for name, table in tables.items()}}
        manifest["days"].append(record)
        write_json(path, manifest)
        totals["training_rows"] += record["training_rows"]
        totals["covered_days"] += 1
        print(json.dumps({"date": day, "training_rows": record["training_rows"]}), flush=True)
    report = run / "diagnostics/training-exploration.md"
    report.parent.mkdir(parents=True, exist_ok=True)
    report.write_text(
        "# Training-only exploration and coverage\n\n"
        f"Processed {totals['covered_days']} panel dates; {totals['training_rows']} training points passed "
        f"role=training and window_end+{frozen['purge_seconds']} seconds <= {frozen['exploration_end_exclusive']} UTC. "
        "Calibration, evaluation and holdout contributed source-presence and eligibility counts only.\n\n"
        "Numerical Parquet tables are under metrics/exploration/<date>. Source masks retain every market, bucket and "
        "configured offset. Arm coverage records zero complete cases separately from pending quiet eligibility. "
        "Feature tables give finite/missing counts, fixed-bin histograms, moments and pairwise sufficient statistics "
        "for the direction label, FAK/FOK fill ratio, stressed value per filled share and loss incidence. "
        "Each feature uses its own finite rows; nothing is imputed. Constants and insufficient pairs do not imply correlation.\n\n"
        "Declared return, volatility, acceleration/reversal/exhaustion, reference disagreement/convergence, external flow, "
        "book spread/depth and capacity-slope families are described. Basis fields are included only as declared in the "
        "frozen registry. RTDS BTCUSD versus Binance BTCUSDT is apparent cross-source/quote-currency basis, not proof "
        "of arbitrage or USD/USDT parity. No extra feature group was constructed.\n\n"
        "Execution tables are training-only independent Q5 bucket/offset counterfactuals with both sides and FAK/FOK, "
        "partial fills, unknown execution, unfilled shares, stressed gains and losses. Their PnL is not deployable sequential "
        "policy economics. No candidate, threshold, date, bucket, quantity or feature was selected from these tables.\n\n"
        "Quiet, confidence and activity studies remain pending causal prequential simulated-refresh evidence. "
        "Missing reference evidence is unknown; historical in-sample activity was not substituted. "
        "This is a candidate-specific dependency, and this module does not accept tournament checkpoints.\n")
    manifest.update(status="independent_studies_complete_reference_study_pending", totals=totals,
        pending=["quiet_confidence_activity_causal_simulated_refresh"],
        source_mask_bits={name: i for i, name in enumerate(source_groups())},
        histogram_edges=[str(value) for value in HISTOGRAM_EDGES],
        report={"path": str(report), "sha256": sha256(report)},
        statistical_scope="Descriptive pooled decisions; overlapping points are not independent samples or significance tests.")
    write_json(path, manifest)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("run", type=Path)
    args = parser.parse_args()
    build(checked_run(args.run))


if __name__ == "__main__":
    main()
