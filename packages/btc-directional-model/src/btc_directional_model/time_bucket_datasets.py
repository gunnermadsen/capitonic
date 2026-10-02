"""Persist full-history and exact matched complete-case training tracks on SSD."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path

import polars as pl

from .time_bucket_candidates import CORE, complete_cases, registry
from .time_bucket_flow_panels import FEATURES
from .time_bucket_source_audit import checked_run, sha256, write_json
from .time_bucket_training import DECISION, IDENTITY, read_day


def prepare(run: Path, *, quiet: bool = False) -> None:
    frozen = json.loads((run / "inputs/candidate-feature-freeze.json").read_text())
    if frozen["arms"] != registry(FEATURES):
        raise ValueError("Candidate groups changed after feature freeze")
    arms = [arm for arm in frozen["arms"] if bool(arm.get("quiet_only")) == quiet]
    layers = ["refprice-twap", "flow"] + (["simulated-refresh"] if quiet else [])
    source_manifests = ["core-panels.json", "refprice-twap-panels.json", "flow-panels.json"]
    if quiet:
        source_manifests.append("simulated-refresh-panels.json")
    for name in source_manifests:
        if json.loads((run / "manifests" / name).read_text())["status"] != "complete":
            raise ValueError("Source feature layers are not complete")
    identity = {"features": sha256(run / "inputs/candidate-feature-freeze.json"),
                "source_panels": {name: sha256(run / "manifests" / name) for name in source_manifests},
                "code": {name: sha256(Path(__file__).with_name(name)) for name in [
                    "time_bucket_datasets.py", "time_bucket_candidates.py"]}}
    manifest_path = run / "manifests" / ("quiet-complete-case-panels.json" if quiet else "complete-case-panels.json")
    manifest = json.loads(manifest_path.read_text()) if manifest_path.exists() else {
        "identity": identity, "status": "in_progress", "days": [], "tracks": ["full_history_core", "product_specific", "four_product_matched"]}
    if manifest["identity"] != identity:
        raise ValueError("Complete-case panel identity changed during execution")
    completed = {record["date"]: record for record in manifest["days"]}
    for path in sorted((run / "datasets/core").glob("*.parquet")):
        if path.stem in completed:
            for artifact in completed[path.stem]["artifacts"]:
                if sha256(Path(artifact["path"])) != artifact["sha256"]:
                    raise ValueError("Existing complete-case panel changed")
            continue
        day = read_day(run, path.stem, layers)
        records = []
        matched_ids = None
        for arm in arms:
            clock = day.filter(pl.col("entry_offset").is_in(arm["entry_offsets"]) & (pl.col("seconds_elapsed") < 260))
            panel = complete_cases(clock, arm)
            if arm["matched_products"]:
                identifiers = panel["point_id"].sort().to_list()
                if matched_ids is None:
                    matched_ids = identifiers
                elif matched_ids != identifiers:
                    raise ValueError("All fifteen matched ablations must share exact point identities")
            columns = list(dict.fromkeys(IDENTITY + DECISION + arm["features"]))
            destination = run / "datasets/complete-cases" / arm["candidate"] / arm["arm"] / path.name
            destination.parent.mkdir(parents=True, exist_ok=True)
            if destination.exists():
                raise ValueError("Unrecorded complete-case output must not be overwritten")
            panel.select(columns).write_parquet(destination, compression="zstd")
            records.append({"candidate": arm["candidate"], "arm": arm["arm"], "path": str(destination),
                            "sha256": sha256(destination), "rows": panel.height})
        if not quiet:
            panel = complete_cases(day, {"features": CORE, "matched_products": []})
            destination = run / "datasets/exit-features" / path.name
            destination.parent.mkdir(parents=True, exist_ok=True)
            panel.select(list(dict.fromkeys(IDENTITY + DECISION + CORE))).write_parquet(destination, compression="zstd")
            records.append({"candidate": "two_sided_buy_sell", "arm": "post_entry_observations",
                            "path": str(destination), "sha256": sha256(destination), "rows": panel.height})
        manifest["days"].append({"date": path.stem, "artifacts": records,
            "matched_point_id_sha256": hashlib.sha256("\n".join(matched_ids or []).encode()).hexdigest(),
            "matched_rows": len(matched_ids or [])})
        write_json(manifest_path, manifest)
        print(json.dumps({"date": path.stem, "arms": len(arms), "matched_rows": len(matched_ids or [])}), flush=True)
    manifest["status"] = "complete"
    write_json(manifest_path, manifest)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("run", type=Path)
    parser.add_argument("--quiet", action="store_true")
    args = parser.parse_args()
    prepare(checked_run(args.run), quiet=args.quiet)


if __name__ == "__main__":
    main()
