# Capitonic local Kubernetes monitoring

The `capitonic` namespace on the Rancher Desktop k3s cluster holds Capitonic services. The monitoring charts live directly under `capitonic-helm-chart/`; each service has its own Helm release named after the service. The platform label is `capitonic-platform` and the environment label is `dev`.

## Workload and storage ownership

| Service | Kubernetes workload | Storage | Reason |
| --- | --- | --- | --- |
| Loki | single-replica StatefulSet | `local-path` ReadWriteOnce PVC | Durable local log index and chunks; one database-like replica on the single node. |
| Prometheus | single-replica StatefulSet | `local-path` ReadWriteOnce PVC | Durable local TSDB with stable volume identity. |
| Grafana | single-replica Deployment with `Recreate` strategy | `local-path` ReadWriteOnce PVC | SQLite and dashboard state persist without simultaneous writers. |
| Alloy | DaemonSet | ephemeral working directory | One log collector per node; log data is stored by Loki. |

The local-path provisioner has a `Delete` reclaim policy. Treat these claims as disposable development data; chart upgrades retain them, but deleting PVCs destroys the stored metrics, logs, or Grafana state. Do not run multiple replicas against the same local PVC.

The monitoring rollout does not include node-exporter or cAdvisor. They are reserved for production. Database, ingester, and trading services are not deployed by these charts.

## Asset and secret ownership

`common/configs/` is the only checked-in source for service configuration, dashboards, and alert rules. Its additive `alloy/kubernetes.alloy` and `grafana/provisioning/datasources/kubernetes.yml` files hold the Kubernetes-specific configuration. `scripts/helm/deploy-monitoring.py` is the maintained bake and deployment entry point under `scripts/`. Runtime `.env.prometheus` and `.env.grafana` in the main worktree remain the only source for monitoring credentials; the feature worktree symlinks them. Existing Compose startup scripts remain at `common/scripts/` because Compose references that path. The monitoring charts do not need a startup script.

The bake reads the shared source files, copies them to ignored `assets/` directories inside each chart, and filters the Docker-specific Prometheus target list to currently deployed Kubernetes services. It does not edit the shared sources or generated chart assets by hand. After lint, rendering, and deployment, it deletes those assets. Running the bake again is required before linting or packaging the charts independently. No generated manifest or secret is committed.

Kubernetes Secrets `prometheus-auth` and `grafana-auth` are populated from the runtime environment files at deployment time. Helm templates contain only secret references. Prometheus authentication remains enabled. Grafana provisions Prometheus and Loki datasources, shared dashboards, and the Prometheus connectivity alert. Database and trading datasources and alerts wait for their services.

## Commands

Run from the assigned feature worktree with Kubernetes context `rancher-desktop`:

```bash
python3 scripts/helm/deploy-monitoring.py --check
python3 scripts/helm/deploy-monitoring.py
kubectl -n capitonic get pods,svc,pvc
helm -n capitonic list
kubectl -n capitonic port-forward service/grafana 3030:3000
```

The deployment command validates source files and secrets, bakes ignored chart assets, lints and renders all four charts, creates the namespace and native Secrets, then installs Loki, Prometheus, Alloy, and Grafana in dependency order with atomic Helm waits. It cleans baked assets even if a command fails. Access Grafana only through a local port-forward at `http://127.0.0.1:3030`.

## Acceptance checks

- All four workloads have their intended ready pod count, no restart loop, and the expected immutable image where a digest is specified.
- Loki, Prometheus, and Grafana PVCs are Bound on `local-path`.
- Prometheus self, Loki, and Alloy targets are up; unavailable future trading services are not configured as development targets yet.
- Alloy delivers current `capitonic` pod logs to Loki without persistent delivery errors.
- Grafana reports a healthy API, provisions its shared dashboards and both datasources, and can query Prometheus and Loki.
- Monitoring configuration is provisioned only through the charts. After successful observability deployment, record the repository-required `provisioned/observability/dev/<timestamp>` tag on the exact deployed commit.

No database migration, Rust image build, Docker Compose edit, or trading-process change is part of this rollout.
