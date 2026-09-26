# Local Kubernetes ingester standards

The `ingester` Helm release in `capitonic-helm-chart/charts/ingester/` owns two Deployments: `ingester-master` and `ingester-worker`. Both use one immutable ARM64 `capitonic/ingester` image; `INGESTER_MODE` selects the existing runtime role. The master is one replica with a ClusterIP Service on 8098. Workers use the same image and a headless Service exposing HTTP 8099 and gRPC 50051. The headless Service supports the existing Prometheus DNS discovery. No separate worker image or per-job pod is introduced.

The source code's existing `/health/live` and `/health/ready` endpoints are the startup, liveness, and readiness probes for each role. Master readiness uses its cached database and supervisor state; worker readiness follows supervisor reconciliation. These probes are lightweight and do not trigger ingestion. Worker identities come from pod names, and each worker advertises its pod IP for the existing master-to-worker gRPC contract. The master and worker use their existing, separate PgBouncer roles and transaction-pool aliases.

`postgres-credentials` and `ingester-auth` are Kubernetes Secrets created from the main worktree's `.env` files; values are never stored in the chart or Git. The image tag, digest, and embedded Git revision are recorded in chart values. The chart contains no application config copies or startup scripts. `common/configs/` and `common/scripts/` remain authoritative for those files. `scripts/helm/bake-assets.py` only prepares ignored chart assets; the ingester Prometheus jobs are copied from `common/configs/prometheus/prometheus.yml`. Run the bake before upgrading Prometheus, then clean it with `--clean`.

The local Traefik route owns `/api/ingester` and removes that prefix before forwarding to `ingester-master`. Application routes remain `/backfills`, `/ingesters`, and `/workers`; no application route gains `/api`. The administrative bearer token is still required. The route is intended for local development access at `http://localhost/api/ingester/` and does not expose worker gRPC, PostgreSQL, or Prometheus. Docker Compose remains independent and keeps its existing image.

The master owns durable job scheduling and worker assignment. When `workerScaling.enabled` is true, its Kubernetes scaler reconciles desired realtime profiles and unfinished backfill and drain jobs against the existing `ingester-worker` Deployment. It changes only that Deployment's `/scale` resource through the `ingester-master` service account in `capitonic`. Helm omits the worker replica field in this mode, so a normal chart upgrade leaves runtime scale under the master's ownership. The scaler has a one-worker floor, a fourteen-worker local ceiling, and reduces to the floor only after all demand and leases are gone for a full idle interval. Compose does not enable this mode. Never create one pod per job or issue a manual scale command to satisfy an API request.

The master and worker mount the `ingester-archives` claim backed by the Mac external SSD shared into Rancher Desktop. It contains the existing Parquet archives used by registered drain and backfill strategies. The source path is `/Volumes/docker-data/polymarket-bot`; the chart mounts it at `/var/lib/capitonic-data` with matching runtime paths. Durable profile, job, lease, and registration state belongs in TimescaleDB. Helm owns the worker image, pod template, and service; the master owns replica count while Kubernetes scaling is enabled. The bake never invokes Helm or kubectl.

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

The historical `scripts/helm/test-ingester-recovery.py` suite is for an isolated database with zero enabled trading processes and no active ingester work. Do not run it against the restored cutover database: its safety gate intentionally rejects enabled processes. For the cutover, use bounded API-scheduled jobs on retained source windows and verify the parent and shard records through `/api/ingester/backfills/:job_id`. The 2026-09-26 test deleted an assigned worker pod during a two-shard job; Kubernetes replaced the pod and the affected shard completed on attempt two. See `docs/capitonic-k3s-cutover.md` for the result and backup identity.

The current scaler retains excess replicas while any realtime demand remains, so completed backfills may leave more ready workers than the nine required realtime owners. Operational job scheduling still goes through the master API; do not reduce workers manually to compensate.
