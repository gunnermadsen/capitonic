# Capitonic local Kubernetes database

The `timescaledb`, `db-migrate`, and `pgbouncer` charts are independent releases under `capitonic-helm-chart/charts/` in the `capitonic` namespace. The TimescaleDB StatefulSet owns pod `timescaledb-0`; its client Service is `timescaledb`. PgBouncer has one Deployment and a `pgbouncer` ClusterIP Service. The migration runner is a separately invoked, one-shot Job. Compose remains a separate deployment and database.

## Ownership and storage

`common/configs/pgbouncer/pgbouncer.ini` and `common/scripts/pgbouncer-entrypoint.sh` are the only checked-in sources for PgBouncer configuration and startup. Run `python3 scripts/helm/bake-assets.py` before linting, rendering, or upgrading PgBouncer; it copies the script and changes only `host=timescaledb-0` to `host=timescaledb` in the ignored chart asset. Run `python3 scripts/helm/bake-assets.py --clean` afterward. The bake only prepares assets; Helm and kubectl remain separate commands.

The main worktree's `.env.postgres` and `.env.postgres.roles` provide the five database passwords. The feature worktree symlinks these files. Create or update the `postgres-credentials` Kubernetes Secret from those files; chart templates contain only Secret references. The existing `.env.postgres.pgbouncer` role values must match `.env.postgres.roles` before deployment. Never commit a generated Secret manifest or a baked asset.

TimescaleDB has one 20Gi `local-path` ReadWriteOnce claim. The StatefulSet retains its claim when deleted or scaled, but deleting the PVC causes the provisioner to delete its data. The local cluster is a single failure domain; this is an empty development database, not a backup of Compose. Do not delete or replace its claim as an automatic rollback. Database tuning follows the existing two-CPU production Compose profile, with a 3Gi pod memory limit and 40 PostgreSQL connection slots, leaving room for monitoring and later services on the eight-CPU, 8GiB VM.

## Deployment order

1. Verify `rancher-desktop` context, namespace `capitonic`, monitoring readiness, free disk, chart commit, and imported ARM64 image identities. The charts use `imagePullPolicy: Never` so k3s cannot silently substitute a registry image. Record the exact image IDs after deployment.
2. Create `postgres-credentials` from the main-worktree `.env` sources, then install `timescaledb` with Helm. Verify the pod, readiness, PVC, client Service, and persistence after a controlled pod restart.
3. Before installing `db-migrate`, verify the policy commit is in the deployment lineage and the approved pending list exactly matches the reviewed baseline plus seven newer migrations. Install the Job deliberately with `helm install db-migrate ... --wait --wait-for-jobs`; do not use an automatic Helm hook or `--atomic`, which would remove a failed Job's evidence. Preserve Job logs and verify the ledger and zero pending migrations.
4. Install `pgbouncer` with Helm after the service roles exist. Verify every canonical route with its intended identity, the 30-backend pool ceiling, ready pods, and logs. Leave the Compose stack and database untouched.

For a first install from the feature worktree:

```bash
kubectl --context rancher-desktop -n capitonic create secret generic postgres-credentials \
  --from-env-file=.env.postgres --from-env-file=.env.postgres.roles
python3 scripts/helm/bake-assets.py
helm lint capitonic-helm-chart/charts/timescaledb capitonic-helm-chart/charts/db-migrate capitonic-helm-chart/charts/pgbouncer
helm upgrade --install timescaledb capitonic-helm-chart/charts/timescaledb -n capitonic --atomic --wait --timeout 10m
helm install db-migrate capitonic-helm-chart/charts/db-migrate -n capitonic --wait --wait-for-jobs --timeout 15m
helm upgrade --install pgbouncer capitonic-helm-chart/charts/pgbouncer -n capitonic --atomic --wait --timeout 5m
python3 scripts/helm/bake-assets.py --clean
```

These commands are sequential gates, not a script. Stop and investigate before proceeding if any gate fails. The migration Job must use the exact prebuilt image whose embedded Git revision contains the approved baseline and later migrations. Migrations and corrections remain the exclusive responsibility of committed TypeORM code. No multi-hour monitoring-only soak is required; extended soak follows the deployment of code-driven services and live data inputs.
