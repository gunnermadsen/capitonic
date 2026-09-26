# Capitonic local Kubernetes monitoring

The `capitonic` namespace on the Rancher Desktop k3s cluster holds Capitonic services. The monitoring charts live under `capitonic-helm-chart/charts/`; each service has its own Helm release named after the service. The platform label is `capitonic-platform` and the environment label is `dev`.

## Workload and storage ownership

| Service | Kubernetes workload | Storage | Reason |
| --- | --- | --- | --- |
| Loki | single-replica StatefulSet | `local-path` ReadWriteOnce PVC | Durable local log index and chunks; one database-like replica on the single node. |
| Prometheus | single-replica StatefulSet | `local-path` ReadWriteOnce PVC | Durable local TSDB with stable volume identity. |
| Grafana | single-replica Deployment with `Recreate` strategy | `local-path` ReadWriteOnce PVC | SQLite and dashboard state persist without simultaneous writers. |
| Alloy | DaemonSet | ephemeral working directory | One log collector per node; log data is stored by Loki. |

The local-path provisioner has a `Delete` reclaim policy. Chart upgrades retain the claims, but deleting PVCs destroys the stored metrics, logs, or Grafana state. Before storage maintenance, take verified cold backups on the external SSD. Do not run multiple replicas against the same local PVC. Prometheus and Loki retain 30 days of data; watch actual Rancher Desktop VM disk usage because local-path does not enforce the PVC's requested capacity.

The monitoring rollout does not include node-exporter or cAdvisor. They are reserved for production. Database, ingester, and trading services are not deployed by these charts.

## Asset and secret ownership

`common/configs/` is the only checked-in source for service configuration, dashboards, and alert rules. Its additive `alloy/kubernetes.alloy` and `grafana/provisioning/datasources/kubernetes.yml` files hold the Kubernetes-specific configuration. `common/scripts/` is the source for microservice startup scripts. Root `scripts/` holds project utilities, including the maintained `scripts/helm/bake-assets.py` asset bake. Runtime `.env.prometheus` and `.env.grafana` in the main worktree remain the only source for monitoring credentials; the feature worktree symlinks them. The monitoring charts do not need a startup script.

The bake reads the shared source files, copies them to ignored `assets/` directories inside each chart, and filters the Docker-specific Prometheus target list to currently deployed Kubernetes services. It does not edit the shared sources or generated chart assets by hand. It does not invoke Helm or kubectl, manage Secrets, or clean up automatically. Run `--clean` after the Helm operation to remove only the generated assets. Running the bake is required before linting, rendering, packaging, or upgrading the charts from their directories; the chart templates fail if required assets are missing. No generated manifest or secret is committed.

Kubernetes Secrets `prometheus-auth` and `grafana-auth` are managed separately from baking and Helm using the runtime environment files. They already exist in the development namespace and must be present before installing these charts on a new cluster. Helm templates contain only secret references. Prometheus authentication remains enabled. Grafana provisions Prometheus, Loki, and PostgreSQL datasources, shared dashboards, and every alert rule in `common/configs/grafana/provisioning/alerting/`. Alert evaluation depends on the corresponding services and process data being present.

## Commands

Run from the assigned feature worktree with Kubernetes context `rancher-desktop`. For a chart or configuration change, bake first, then use ordinary Helm commands. For example, to upgrade Grafana:

```bash
python3 scripts/helm/bake-assets.py
helm lint capitonic-helm-chart/charts/grafana
helm upgrade --install grafana capitonic-helm-chart/charts/grafana -n capitonic --atomic --wait --timeout 5m
python3 scripts/helm/bake-assets.py --clean
```

For an initial install, create the `capitonic` namespace and required Secrets first, then install the separate Helm releases in this order: Loki, Prometheus, Alloy, Grafana. Use the same `helm upgrade --install <service> capitonic-helm-chart/charts/<service> -n capitonic --atomic --wait --timeout 5m` form for each. Chart and ConfigMap changes go through Helm; their checksum annotations trigger the needed pod rollouts. A restart with no chart change needs no bake or Helm upgrade:

```bash
kubectl -n capitonic rollout restart deployment/grafana
kubectl -n capitonic rollout status deployment/grafana
```

Use `statefulset/prometheus`, `statefulset/loki`, or `daemonset/alloy` for those workloads. Check the workload and dependent services after any restart. The Helm-managed Traefik route serves Grafana at `http://localhost/monitor/` and strips `/monitor` before forwarding, so in-cluster Grafana API paths stay at their existing root paths.

## Acceptance checks

- All four workloads have their intended ready pod count, no restart loop, and the expected immutable image where a digest is specified.
- Loki, Prometheus, and Grafana PVCs are Bound on `local-path`.
- Prometheus self, Loki, Alloy, database, pool, bot, and deployed ingester targets are up. The ingester scrape jobs come from the shared Prometheus configuration through the bake.
- Alloy delivers current `capitonic` pod logs to Loki without persistent delivery errors.
- Grafana reports a healthy API, provisions its shared dashboards and the Prometheus, Loki, and PostgreSQL datasources, and can query each deployed dependency.
- Monitoring configuration is provisioned only through the charts. After successful observability deployment, record the repository-required `provisioned/observability/dev/<timestamp>` tag on the exact deployed commit.

No database migration, Rust image build, Docker Compose edit, or trading-process change is part of this rollout.
