#!/usr/bin/env python3
"""Fingerprint committed component inputs, including consumed workspace inputs."""

import hashlib
import subprocess
import sys


INPUTS = {
    "polymarket-bot": [
        ".dockerignore", "Cargo.toml", "Cargo.lock",
        "packages/polymarket-bot/Cargo.toml",
        "packages/market-data-ingester/Cargo.toml",
        "packages/polymarket-bot/build.rs", "packages/polymarket-bot/src",
        "packages/polymarket-bot/Dockerfile.production",
        "packages/polymarket-bot/model-manifest.json",
        "packages/polymarket-bot/scripts/package_models.py",
        "packages/btc-directional-model/runtime-models", "common/proto",
    ],
    "ingester": [
        ".dockerignore", "Cargo.toml", "Cargo.lock",
        "packages/polymarket-bot/Cargo.toml",
        "packages/market-data-ingester/Cargo.toml",
        "packages/market-data-ingester/build.rs",
        "packages/market-data-ingester/src",
        "packages/market-data-ingester/Dockerfile.production", "common/proto",
    ],
    "db-migrate": [
        ".dockerignore", "packages/db-migrate/package.json",
        "packages/db-migrate/package-lock.json", "packages/db-migrate/tsconfig.json",
        "packages/db-migrate/src", "packages/db-migrate/Dockerfile.production",
    ],
}


def fingerprint(component, revision):
    paths = INPUTS[component]
    entries = subprocess.check_output(
        ["git", "ls-tree", "-rz", "--full-tree", revision, "--", *paths]
    ).split(b"\0")
    digest = hashlib.sha256()
    # Increment this contract when test/build requirements change.
    digest.update(b"ci-component-v2:linux-arm64:rust-1.92.0\0")
    for entry in sorted(filter(None, entries)):
        metadata, path = entry.split(b"\t", 1)
        mode, kind, object_id = metadata.split()
        if kind != b"blob":
            raise ValueError("Component inputs must be tracked files")
        content = subprocess.check_output(["git", "cat-file", "blob", object_id])
        digest.update(mode + b"\0" + path + b"\0" + hashlib.sha256(content).digest())
    return digest.hexdigest()


if __name__ == "__main__":
    component = sys.argv[1]
    revision = sys.argv[2] if len(sys.argv) > 2 else "HEAD"
    print(fingerprint(component, revision))
