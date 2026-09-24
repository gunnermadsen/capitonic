#!/usr/bin/env python3
"""Bake repository-owned configuration and scripts into ignored chart assets."""

from __future__ import annotations

import argparse
import re
import shutil
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
CHARTS = ROOT / "capitonic-helm-chart" / "charts"
RELEASES = ("loki", "prometheus", "alloy", "grafana", "pgbouncer")


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


def clean() -> None:
    for release in RELEASES:
        shutil.rmtree(CHARTS / release / "assets", ignore_errors=True)


def bake() -> None:
    prometheus_env = env_values(
        ROOT / ".env.prometheus",
        ("PROMETHEUS_BASIC_AUTH_USER",),
    )
    if not re.fullmatch(r"[A-Za-z0-9._-]+", prometheus_env["PROMETHEUS_BASIC_AUTH_USER"]):
        raise ValueError("PROMETHEUS_BASIC_AUTH_USER contains unsupported characters")

    source = (ROOT / "common/configs/prometheus/prometheus.yml").read_text()
    jobs = re.split(r"(?=^  - job_name: )", source, flags=re.MULTILINE)
    selected = [
        part
        for part in jobs[1:]
        if part.startswith(
            (
                "  - job_name: polymarket-bot\n",
                "  - job_name: ingester-master\n",
                "  - job_name: ingester-worker\n",
                "  - job_name: alloy\n",
                "  - job_name: loki\n",
            )
        )
    ]
    if len(selected) != 5:
        raise ValueError("Expected bot, ingester, Alloy, and Loki scrape jobs in the shared Prometheus configuration")
    database_scrapes = (ROOT / "common/configs/prometheus/kubernetes-database-scrapes.yml").read_text()
    if database_scrapes.count("  - job_name: ") != 2:
        raise ValueError("Expected TimescaleDB and PgBouncer Kubernetes scrape jobs")
    config = (
        jobs[0]
        + "  - job_name: prometheus\n"
        + "    basic_auth:\n"
        + f"      username: {prometheus_env['PROMETHEUS_BASIC_AUTH_USER']}\n"
        + "      password_file: /etc/prometheus-auth/password\n"
        + "    static_configs:\n"
        + "      - targets: [127.0.0.1:9090]\n"
        + "".join(selected)
        + database_scrapes
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
        "provisioning/alerting/rules-polymarket-bot.yml",
    ):
        write_asset("grafana", relative, (grafana_source / relative).read_text())
    write_asset(
        "grafana",
        "provisioning/datasources/datasources.yml",
        (grafana_source / "provisioning/datasources/kubernetes.yml").read_text(),
    )

    pgbouncer = (ROOT / "common/configs/pgbouncer/pgbouncer.ini").read_text()
    if pgbouncer.count("host=timescaledb-0") != 8:
        raise ValueError("Expected eight database routes in the shared PgBouncer configuration")
    write_asset("pgbouncer", "pgbouncer.ini", pgbouncer.replace("host=timescaledb-0", "host=timescaledb"))
    write_asset(
        "pgbouncer",
        "pgbouncer-entrypoint.sh",
        (ROOT / "common/scripts/pgbouncer-entrypoint.sh").read_text(),
    )


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--clean", action="store_true", help="remove only generated chart assets")
    args = parser.parse_args()
    if args.clean:
        clean()
        print("Removed generated chart assets")
    else:
        clean()
        try:
            bake()
        except Exception:
            clean()
            raise
        print(f"Baked chart assets under {CHARTS}")


if __name__ == "__main__":
    main()
