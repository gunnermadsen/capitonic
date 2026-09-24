#!/usr/bin/env python3
"""Bake repository-owned monitoring assets and deploy the local k3s monitoring stack."""

from __future__ import annotations

import argparse
import base64
import json
import re
import shutil
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
CHARTS = ROOT / "capitonic-helm-chart"
NAMESPACE = "capitonic"
CONTEXT = "rancher-desktop"
RELEASES = ("loki", "prometheus", "alloy", "grafana")


def run(*args: str, input_text: str | None = None) -> str:
    result = subprocess.run(
        args, input=input_text, text=True, capture_output=True, check=False
    )
    if result.returncode:
        raise RuntimeError(f"{' '.join(args[:3])} failed: {result.stderr.strip()}")
    return result.stdout


def env_values(path: Path, required: tuple[str, ...]) -> dict[str, str]:
    values: dict[str, str] = {}
    for raw in path.read_text().splitlines():
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        key, separator, value = line.partition("=")
        if not separator or not re.fullmatch(r"[A-Za-z_][A-Za-z_0-9]*", key):
            raise ValueError(f"Invalid environment entry in {path.name}")
        value = value.strip()
        if len(value) >= 2 and value[0] == value[-1] and value[0] in "\"'":
            value = value[1:-1]
        values[key] = value
    missing = [key for key in required if not values.get(key)]
    if missing:
        raise ValueError(f"Missing {', '.join(missing)} in {path.name}")
    return values


def write_asset(chart: str, relative: str, content: str) -> None:
    output = CHARTS / chart / "assets" / relative
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(content)


def bake() -> tuple[dict[str, str], dict[str, str]]:
    for release in RELEASES:
        shutil.rmtree(CHARTS / release / "assets", ignore_errors=True)

    prometheus_env = env_values(
        ROOT / ".env.prometheus",
        (
            "PROMETHEUS_BASIC_AUTH_USER",
            "PROMETHEUS_BASIC_AUTH_PASSWORD",
            "PROMETHEUS_BASIC_AUTH_PASSWORD_HASH",
        ),
    )
    grafana_env = env_values(
        ROOT / ".env.grafana", ("GRAFANA_ADMIN_USER", "GRAFANA_ADMIN_PASSWORD")
    )

    source = (ROOT / "common/configs/prometheus/prometheus.yml").read_text()
    jobs = re.split(r"(?=^  - job_name: )", source, flags=re.MULTILINE)
    selected = [part for part in jobs[1:] if part.startswith(("  - job_name: alloy\n", "  - job_name: loki\n"))]
    if len(selected) != 2:
        raise ValueError("Expected Alloy and Loki scrape jobs in the shared Prometheus configuration")
    config = (
        jobs[0]
        + "  - job_name: prometheus\n"
        + "    basic_auth:\n"
        + f"      username: {prometheus_env['PROMETHEUS_BASIC_AUTH_USER']}\n"
        + "      password_file: /etc/prometheus-auth/password\n"
        + "    static_configs:\n"
        + "      - targets: [127.0.0.1:9090]\n"
        + "".join(selected)
    )
    config = config.replace(
        "scrape_configs:\n",
        "  external_labels:\n    environment: dev\n    deployment: k3s\nscrape_configs:\n",
        1,
    )
    write_asset("prometheus", "prometheus.yml", config)
    write_asset("loki", "loki.yml", (ROOT / "common/configs/loki/loki.yml").read_text())

    write_asset(
        "alloy", "config.alloy", (ROOT / "common/configs/alloy/kubernetes.alloy").read_text()
    )

    grafana_source = ROOT / "common/configs/grafana"
    for dashboard in (grafana_source / "dashboards").glob("*.json"):
        write_asset("grafana", f"dashboards/{dashboard.name}", dashboard.read_text())
    for relative in (
        "provisioning/dashboards/dashboards.yml",
        "provisioning/alerting/rules-prometheus.yml",
    ):
        write_asset("grafana", relative, (grafana_source / relative).read_text())
    write_asset(
        "grafana",
        "provisioning/datasources/datasources.yml",
        (grafana_source / "provisioning/datasources/kubernetes.yml").read_text(),
    )
    return prometheus_env, grafana_env


def apply_secret(name: str, data: dict[str, str]) -> None:
    payload = {
        "apiVersion": "v1",
        "kind": "Secret",
        "metadata": {"name": name, "namespace": NAMESPACE},
        "type": "Opaque",
        "data": {key: base64.b64encode(value.encode()).decode() for key, value in data.items()},
    }
    run("kubectl", "apply", "-f", "-", input_text=json.dumps(payload))


def deploy(prometheus_env: dict[str, str], grafana_env: dict[str, str]) -> None:
    context = run("kubectl", "config", "current-context").strip()
    if context != CONTEXT:
        raise RuntimeError(f"Expected Kubernetes context {CONTEXT}, found {context}")
    for release in RELEASES:
        chart = CHARTS / release
        run("helm", "lint", str(chart))
        run("helm", "template", release, str(chart), "--namespace", NAMESPACE)
    run(
        "kubectl", "apply", "-f", "-",
        input_text=f"apiVersion: v1\nkind: Namespace\nmetadata:\n  name: {NAMESPACE}\n",
    )
    user = prometheus_env["PROMETHEUS_BASIC_AUTH_USER"]
    password = prometheus_env["PROMETHEUS_BASIC_AUTH_PASSWORD"]
    password_hash = prometheus_env["PROMETHEUS_BASIC_AUTH_PASSWORD_HASH"]
    if not re.fullmatch(r"[A-Za-z0-9._-]+", user) or not password_hash.startswith(("$2a$", "$2b$", "$2y$")):
        raise ValueError("Invalid Prometheus basic authentication configuration")
    apply_secret(
        "prometheus-auth",
        {
            "username": user,
            "password": password,
            "web.yml": f"basic_auth_users:\n  {user}: '{password_hash}'\n",
        },
    )
    apply_secret(
        "grafana-auth",
        {
            "admin-user": grafana_env["GRAFANA_ADMIN_USER"],
            "admin-password": grafana_env["GRAFANA_ADMIN_PASSWORD"],
        },
    )
    for release in RELEASES:
        print(f"Installing {release}...", flush=True)
        print(
            run(
                "helm", "upgrade", "--install", release, str(CHARTS / release),
                "--namespace", NAMESPACE, "--atomic", "--wait", "--timeout", "5m",
            ).strip(),
            flush=True,
        )


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="bake and validate without deploying")
    args = parser.parse_args()
    try:
        prometheus_env, grafana_env = bake()
        if args.check:
            for release in RELEASES:
                run("helm", "lint", str(CHARTS / release))
                run("helm", "template", release, str(CHARTS / release), "--namespace", NAMESPACE)
            print("Monitoring charts lint and render successfully")
        else:
            deploy(prometheus_env, grafana_env)
    finally:
        for release in RELEASES:
            shutil.rmtree(CHARTS / release / "assets", ignore_errors=True)


if __name__ == "__main__":
    main()
