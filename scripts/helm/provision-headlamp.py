#!/usr/bin/env python3
"""Provision local Headlamp password access from the canonical .env.headlamp."""

import argparse
import importlib.util
import json
from pathlib import Path
import re
import subprocess
import sys


ROOT = Path(__file__).resolve().parents[2]
CHART = ROOT / "capitonic-helm-chart/charts/headlamp"
sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location("bake_assets", ROOT / "scripts/helm/bake-assets.py")
bake_assets = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bake_assets)


def credential_values():
    values = bake_assets.env_values(ROOT / ".env.headlamp", ("HEADLAMP_USERNAME", "HEADLAMP_PASSWORD"))
    username = values["HEADLAMP_USERNAME"]
    password = values["HEADLAMP_PASSWORD"]
    if not re.fullmatch(r"[A-Za-z0-9._-]+", username):
        raise ValueError("HEADLAMP_USERNAME contains unsupported characters")
    if "\n" in password or "\r" in password or not 16 <= len(password.encode()) <= 72:
        raise ValueError("HEADLAMP_PASSWORD must be 16–72 bytes without newlines")
    entry = subprocess.check_output(
        ["htpasswd", "-niB", "-C", "10", username], input=password + "\n", text=True
    ).strip()
    return {"credentials": {
        "HEADLAMP_USERNAME": username,
        "HEADLAMP_PASSWORD_HASH": entry.split(":", 1)[1],
    }}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="Lint and render without deploying")
    args = parser.parse_args()
    values = json.dumps(credential_values())
    if args.check:
        subprocess.run(["helm", "lint", str(CHART), "-f", "-"], input=values, text=True, check=True)
        subprocess.run(["helm", "template", "headlamp", str(CHART), "-n", "capitonic", "-f", "-"],
                       input=values, text=True, stdout=subprocess.DEVNULL, check=True)
        print("Headlamp chart lint and rendering passed; credentials were not printed")
    else:
        subprocess.run(["helm", "upgrade", "--install", "headlamp", str(CHART),
                        "--kube-context", "rancher-desktop", "-n", "capitonic", "-f", "-",
                        "--wait", "--timeout", "5m"], input=values, text=True, check=True)


if __name__ == "__main__":
    main()
