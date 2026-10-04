"""Package the model manifest's exact bundles; owns no runtime admission policy."""

import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess


MANIFEST = "packages/polymarket-bot/model-manifest.json"
CATALOG = "packages/btc-directional-model/runtime-models"


def component(value):
    if not isinstance(value, str) or not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_.-]*", value):
        raise ValueError(f"Invalid bundle path component: {value!r}")
    return value


def entries(raw):
    document = json.loads(raw)
    if set(document) != {"schema_version", "models"} or document["schema_version"] != 1:
        raise ValueError("Expected model manifest schema_version 1 and models")
    models = document["models"]
    if not isinstance(models, list) or not models:
        raise ValueError("Model manifest must list at least one model")
    seen = set()
    for model in models:
        if set(model) != {"model_key", "artifact_sha256"}:
            raise ValueError("Model entries require model_key and artifact_sha256 only")
        key = component(model["model_key"])
        if key in seen:
            raise ValueError(f"Duplicate model key: {key}")
        seen.add(key)
        if not re.fullmatch(r"[0-9a-f]{64}", model["artifact_sha256"]):
            raise ValueError(f"Invalid artifact SHA-256 for {key}")
    return models


def bundle_files(model, raw):
    bundle = json.loads(raw)
    if bundle["model_key"] != model["model_key"] or bundle["model_sha256"] != model["artifact_sha256"]:
        raise ValueError(f"Bundle identity differs from model manifest: {model['model_key']}")
    names = ["manifest.json", component(bundle["model_file"]), component(bundle["golden_vectors_file"])]
    if len(set(names)) != len(names):
        raise ValueError("Bundle files must have distinct names")
    return names, [None, model["artifact_sha256"], bundle["golden_vectors_sha256"]]


def package(root, destination):
    raw = (root / MANIFEST).read_bytes()
    payload = {"model-manifest.json": raw}
    for model in entries(raw):
        key = model["model_key"]
        directory = root / CATALOG / key
        names, hashes = bundle_files(model, (directory / "manifest.json").read_bytes())
        for name, expected in zip(names, hashes):
            source = directory / name
            if directory.is_symlink() or source.is_symlink():
                raise ValueError(f"Bundle symlinks are unsupported: {source}")
            data = source.read_bytes()
            if expected is not None and hashlib.sha256(data).hexdigest() != expected:
                raise ValueError(f"Artifact hash mismatch: {source}")
            payload[f"{key}/{name}"] = data
    # Validate every input before writing; never mix with a stale catalog.
    if destination.exists() and any(destination.iterdir()):
        raise ValueError("Packaging destination must be empty")
    destination.mkdir(parents=True, exist_ok=True)
    for name, data in payload.items():
        target = destination / name
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(data)
    return hashlib.sha256(raw).hexdigest()


def git(root, *args):
    return subprocess.check_output(["git", "-C", str(root), *args])


def input_paths(root, revision):
    """Historical revisions without a manifest retain their full-catalog inputs."""
    revision = git(root, "rev-parse", "--verify", f"{revision}^{{commit}}").decode().strip()
    if not git(root, "ls-tree", revision, "--", MANIFEST):
        return [CATALOG]
    paths = [MANIFEST]
    for model in entries(git(root, "show", f"{revision}:{MANIFEST}")):
        directory = f"{CATALOG}/{model['model_key']}"
        names, _ = bundle_files(model, git(root, "show", f"{revision}:{directory}/manifest.json"))
        paths.extend(f"{directory}/{name}" for name in names)
    return paths


def fingerprint(root, revision):
    tree = git(root, "ls-tree", "-r", "--full-tree", revision, "--", *input_paths(root, revision))
    return hashlib.sha256(b"".join(sorted(tree.splitlines(keepends=True)))).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["package", "inputs", "fingerprint"])
    parser.add_argument("target", help="Output directory, or Git revision for input inspection")
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[3])
    args = parser.parse_args()
    if args.command == "package":
        print(f"model_manifest_sha256: {package(args.root, Path(args.target))}")
    elif args.command == "inputs":
        print("\n".join(input_paths(args.root, args.target)))
    else:
        print(fingerprint(args.root, args.target))


if __name__ == "__main__":
    main()
