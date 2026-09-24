# Local Kubernetes ingester standards

The `ingester` Helm release in `capitonic-helm-chart/charts/ingester/` owns two Deployments: `ingester-master` and `ingester-worker`. Both use one immutable ARM64 `capitonic/ingester` image; `INGESTER_MODE` selects the existing runtime role. The master is one replica with a ClusterIP Service on 8098. Workers use the same image, a Helm-managed replica count, and a headless Service exposing HTTP 8099 and gRPC 50051. The headless Service supports the existing Prometheus DNS discovery. No separate worker image, scaling controller, RBAC, or Kubernetes API credential is introduced.

The source code's existing `/health/live` and `/health/ready` endpoints are the startup, liveness, and readiness probes for each role. Master readiness uses its cached database and supervisor state; worker readiness follows supervisor reconciliation. These probes are lightweight and do not trigger ingestion. Worker identities come from pod names, and each worker advertises its pod IP for the existing master-to-worker gRPC contract. The master and worker use their existing, separate PgBouncer roles and transaction-pool aliases.

`postgres-credentials` and `ingester-auth` are Kubernetes Secrets created from the main worktree's `.env` files; values are never stored in the chart or Git. The image tag, digest, and embedded Git revision are recorded in chart values. The chart contains no application config copies or startup scripts. `common/configs/` and `common/scripts/` remain authoritative for those files. `scripts/helm/bake-assets.py` only prepares ignored chart assets; the ingester Prometheus jobs are copied from `common/configs/prometheus/prometheus.yml`. Run the bake before upgrading Prometheus, then clean it with `--clean`.

This startup-only rollout keeps seeded ingester profiles stopped through the master's authenticated lifecycle API before any worker replica starts. Confirm zero desired-running profiles and zero active backfill or drain jobs before setting `worker.replicaCount: 1`. The worker may register and reconcile stopped profiles; no trading process, realtime collection, or backfill is started for this verification. Future data activation requires a separate, capacity-aware decision. Do not change the Docker Compose ingester image or deploy this image through Compose.

The master and worker have no PVC in this rollout. Durable profile, job, lease, and registration state belongs in TimescaleDB. Worker disk caches and archive paths are unused until collection is enabled; provision their storage before activating a strategy that writes them. Scale or upgrade the local workers only by changing the `ingester` chart value and running Helm. Helm owns the pod lifecycle; the bake never invokes Helm or kubectl.

From the feature worktree, the normal chart commands are:

```sh
helm lint capitonic-helm-chart/charts/ingester
helm template ingester capitonic-helm-chart/charts/ingester -n capitonic
helm upgrade --install ingester capitonic-helm-chart/charts/ingester -n capitonic --atomic --wait --timeout 5m
python3 scripts/helm/bake-assets.py
helm upgrade prometheus capitonic-helm-chart/charts/prometheus -n capitonic --atomic --wait --timeout 5m
python3 scripts/helm/bake-assets.py --clean
```

Verify pod readiness and restart counts, the expected immutable image ID and embedded revision, the master and worker health endpoints, worker registration and heartbeat, zero running profiles and jobs, ingester Prometheus targets, and the unchanged Compose image ID. The existing monitoring and database pods must remain healthy.
