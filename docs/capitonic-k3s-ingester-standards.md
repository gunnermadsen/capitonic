# Local Kubernetes ingester standards

The `ingester` Helm release in `capitonic-helm-chart/charts/ingester/` owns two Deployments: `ingester-master` and `ingester-worker`. Both use one immutable ARM64 `capitonic/ingester` image; `INGESTER_MODE` selects the existing runtime role. The master is one replica with a ClusterIP Service on 8098. Workers use the same image and a headless Service exposing HTTP 8099 and gRPC 50051. The headless Service supports the existing Prometheus DNS discovery. No separate worker image or per-job pod is introduced.

The source code's existing `/health/live` and `/health/ready` endpoints are the startup, liveness, and readiness probes for each role. Master readiness uses its cached database and supervisor state; worker readiness follows supervisor reconciliation. These probes are lightweight and do not trigger ingestion. Worker identities come from pod names, and each worker advertises its pod IP for the existing master-to-worker gRPC contract. The master and worker use their existing, separate PgBouncer roles and transaction-pool aliases.

`postgres-credentials` and `ingester-auth` are Kubernetes Secrets created from the main worktree's `.env` files; values are never stored in the chart or Git. The image tag, digest, and embedded Git revision are recorded in chart values. The chart contains no application config copies or startup scripts. `common/configs/` and `common/scripts/` remain authoritative for those files. `scripts/helm/bake-assets.py` only prepares ignored chart assets; the ingester Prometheus jobs are copied from `common/configs/prometheus/prometheus.yml`. Run the bake before upgrading Prometheus, then clean it with `--clean`.

The local Traefik route owns `/api` and removes that prefix before forwarding to `ingester-master`. Application routes remain `/backfills`, `/ingesters`, and `/workers`; no application route gains `/api`. The administrative bearer token is still required. The route is intended for local development access at `http://localhost/api/` and does not expose worker gRPC, PostgreSQL, or Prometheus. Docker Compose remains independent and keeps its existing image.

The master owns durable job scheduling and worker assignment. When `workerScaling.enabled` is true, its Kubernetes scaler reconciles desired realtime profiles and unfinished backfill and drain jobs against the existing `ingester-worker` Deployment. It changes only that Deployment's `/scale` resource through the `ingester-master` service account in `capitonic`. Helm omits the worker replica field in this mode, so a normal chart upgrade leaves runtime scale under the master's ownership. The scaler has a one-worker floor, a four-worker local ceiling, and reduces to the floor only after all demand and leases are gone for a full idle interval. Compose does not enable this mode. Never create one pod per job or issue a manual scale command to satisfy an API request.

The master and worker have no PVC in this rollout. Durable profile, job, lease, and registration state belongs in TimescaleDB. Worker disk caches and archive paths are unused by the selected database-backed strategies; provision storage before activating a strategy that writes them. Helm owns the worker image, pod template, and service; the master owns replica count while Kubernetes scaling is enabled. The bake never invokes Helm or kubectl.

From the feature worktree, the normal chart commands are:

```sh
helm lint capitonic-helm-chart/charts/ingester
helm template ingester capitonic-helm-chart/charts/ingester -n capitonic
helm upgrade --install ingester capitonic-helm-chart/charts/ingester -n capitonic --atomic --wait --timeout 5m
python3 scripts/helm/bake-assets.py
helm upgrade prometheus capitonic-helm-chart/charts/prometheus -n capitonic --atomic --wait --timeout 5m
python3 scripts/helm/bake-assets.py --clean
```

Verify pod readiness and restart counts, the exact image ID and embedded revision, master and worker health, worker registration and heartbeat, distinct realtime leases, shard assignment, persisted coverage, capacity limits, automatic scale-up and safe idle scale-down, ingester Prometheus targets, and the unchanged Compose image ID. The existing monitoring and database pods must remain healthy. Keep trading processes disabled during ingestion tests.

For a controlled recovery check, run `python3 scripts/helm/test-ingester-recovery.py --start-date YYYY-MM-DD` from the ingestion-control worktree. Choose 24 consecutive UTC history days within Binance's available range; the script fails if a request is reused. It requires zero enabled trading processes and no active ingester work. It starts two realtime profiles, tests worker and master pod replacement, checks mixed realtime and backfill work, then tests backfill scaling from the idle worker floor with 18 daily shards. It also exercises cancellation and retry on a separate request and runs a copy-only, source-read-only drain dry run. `--soak-seconds N` adds a bounded realtime health observation, up to four hours. The script restores the profiles to stopped, cancels any unfinished jobs, waits for one ready worker, and writes a JSON result to `/private/tmp/capitonic-ingester-recovery.json`. Inspect that result and the ingester logs if it fails; do not treat a partially completed run as a pass.

This suite does not interrupt the Kubernetes API or run a Parquet-publishing drain. The former would require changing cluster access during a live test; the latter requires durable worker storage. Those cases need their own guarded test once the storage contract is provisioned. The current scaler also retains excess replicas while any demand remains, so a drop from four jobs to two active profiles will not shrink to two until all work ends.
