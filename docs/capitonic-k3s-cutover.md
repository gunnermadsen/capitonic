# Capitonic local k3s cutover

Rancher Desktop k3s uses namespace `capitonic`. Each service has one owning Helm chart under `capitonic-helm-chart/charts/`. This document records the Docker-to-k3s data cutover and the normal local operations contract. The main worktree's `.env` files remain the only secret source; feature worktrees symlink them. `common/configs/` and `common/scripts/` remain the checked-in configuration and microservice script sources. `scripts/helm/bake-assets.py` copies these into ignored chart `assets/` directories only; it does not deploy resources.

As of 2026-09-26 04:07 UTC, the restored k3s stack is running on the selected golden bot and ingester image IDs, all long-running pods are ready with zero restarts, all 21 Prometheus targets are up, and current bot logs reach Loki. Nine desired realtime profiles have healthy leases. Seven previously enabled processes (one live, six paper) resumed with fresh heartbeats, rising callbacks, zero strategy errors, and registered models. The live process reports proven accounting, connected user websocket, enabled order submission under its existing per-order limits, and zero unresolved live orders. Docker Compose services remain stopped. The only firing Grafana alert is the expected RTDS Chainlink candle-window gap from the 03:18–03:35 UTC shutdown interval; live RTDS ticks are current. The `RTDS candle recovery` task checks for automatic 61-minute window recovery and alert clearance every five minutes. No branch merge or image rebuild was performed.

## Cutover data and rollback identity

The Compose stack was stopped before any source backup. The database was cleanly shut down, and the bot, ingester master/workers, PgBouncer, and monitoring containers were stopped. No Compose service was restarted during the restore. The cold physical PostgreSQL volume came from PostgreSQL 14 with the same TimescaleDB image as the k3s target. This preserves the complete `polymarket` database: migrations, trading processes, model references, realtime profiles, jobs, leases, and application data. The PgBouncer alias `polymarket_trading` maps to that database; it is not a second database.

The external SSD backup root is `/Volumes/docker-data/capitonic-cutover/2026-09-26/`:

The imported golden image IDs are `sha256:defd46e79f698eb8492a02b5fce42a035bea120ffb0ae09002ba0fb34223c69a` for `polymarket-bot` (embedded revision `8e471a1c7e6789b84928b514a5aa8c6170e5e0e9`) and `sha256:084ee3e02c5457d480cf76b22d33c770a8beefa042d8d7a5d02554290a57bfb0` for both ingester roles (embedded revision `3760b0ffdf2d3b63a278e34af0f44d4454c463c9`).

| Directory | Contents |
| --- | --- |
| `source/` | Cold Docker volume archives for PostgreSQL, Prometheus, Loki, Grafana, and Alloy, plus `SHA256SUMS`. |
| `verified-copy/` | Independent copies of every source archive; `shasum -a 256 -c SHA256SUMS` passed for the full set. |
| `target-before-restore/` | Cold k3s PostgreSQL, Prometheus, Loki, and Grafana PVC snapshots and checksums. |
| `images/` | Exact golden bot and ingester Docker image archives and checksums imported into k3s containerd. |

All source and target archives were fully listed with `tar -tf` and hashed. The original stopped Docker named volumes remain intact. The old target PVC directories were retained with `.pre-cutover` suffixes in the Rancher Desktop VM. Do not delete any backup or original volume until the k3s deployment has passed full acceptance and its rollback window has ended. A restore must stop the owning pods first, verify an archive against its checksum, preserve the current PVC contents, extract with the original ownership into that PVC, and only then restart the chart. PostgreSQL physical restore additionally requires matching major version, a cleanly stopped source, and matching role credentials. Never run migrations as a restore shortcut.

## Local routes and commands

Traefik alone exposes Grafana and the APIs on the Mac host:

| Host route | Internal target | Authentication |
| --- | --- | --- |
| `http://localhost/monitor/` | Grafana | Grafana login |
| `http://localhost/api/bot/` | Bot API; prefix stripped | Bot admin bearer token for admin routes |
| `http://localhost/api/ingester/` | Ingester master API; prefix stripped | Ingester admin bearer token for work routes |
| `http://localhost/api/metrics/` | Prometheus query API; prefix stripped | Prometheus Basic Auth |

Do not add `/api` to downstream application route definitions. PostgreSQL, PgBouncer, Loki, and worker gRPC remain internal to the cluster; Prometheus is reachable from the Mac host through its authenticated Traefik route. Use `kubectl -n capitonic get pods,ingress,pvc` and `helm list -n capitonic` for service inventory. Use `kubectl -n capitonic rollout status deployment/<name>` or `statefulset/<name>` for readiness. Changes to chart resources or provisioned configuration go through ordinary Helm commands:

```sh
python3 scripts/helm/bake-assets.py
helm lint capitonic-helm-chart/charts/<service>
helm upgrade --install <service> capitonic-helm-chart/charts/<service> -n capitonic --wait --timeout 5m
python3 scripts/helm/bake-assets.py --clean
```

For a restart with no manifest change, use `kubectl -n capitonic rollout restart deployment/<name>` and verify rollout status. The database is a StatefulSet. The `db-migrate` chart is a separate, one-shot Job and runs only for an approved, committed migration set; it is not a startup hook. Do not run it merely because the database was restored.

## Local image candidates

Run `scripts/local-image-ci.sh <component> --checks-only` to check working-tree changes without building an image. For an explicitly requested build, run `scripts/local-image-ci.sh <component> <MAJOR.MINOR.PATCH-local.N>` from a clean, committed branch. The script uses the production Dockerfile and Rancher Desktop k3s containerd when available (otherwise Docker), embeds its source commit and local candidate version, verifies the image ID, and records it in an annotated `image/<component>/sha256-*` Git tag. Starting version lines are `v0.2.0` for db-migrate, `v1.2.1` for ingester, and `v3.2.1` for polymarket-bot. Each component has its own version sequence. An image tag is a candidate identity; the Git image tag does not confer golden status.

To deploy a selected candidate, first make the exact image available in k3s containerd because the application charts use `imagePullPolicy: Never`. Commit a separate chart values change that pins `capitonic/<component>:v<MAJOR.MINOR.PATCH-local.N>`; the ingester chart also requires the matching `imageDigest` and `gitRevision`, and its master and workers share one image. Record the running image IDs, configuration, enabled processes, realtime allocations, and active backfills as the rollback tuple. Then use the owning chart's ordinary `helm upgrade --install` command and verify actual pod image IDs, worker leases, required feed timestamps, process heartbeats, and affected trading paths. The ingester master retains ownership of worker scaling. A db-migrate candidate is never run during a normal image upgrade; applying migrations requires the separate approved one-shot Job and pending-migration checks. `scripts/local-image-ci.sh` does not edit Helm values, deploy, push, merge, or mint a golden tag.

Schedule and stop realtime profiles and backfills through the ingester master's authenticated API at `/api/ingester/ingesters` and `/api/ingester/backfills`. The master scales the single `ingester-worker` Deployment itself using its namespace-scoped `/scale` permission. Do not call `kubectl scale`, edit Deployment replicas, or set Helm worker replicas to satisfy API work. Helm omits the replica field while master scaling is enabled. The master may retain spare workers while realtime demand remains; account for that in the 8 CPU, 8 GiB Rancher Desktop budget.

## Acceptance before Compose retirement

1. Verify source and verified-copy checksums, the original Docker volumes, and the pre-restore target snapshots remain readable on the SSD.
2. Verify the restored `polymarket` database, expected migration ledger, processes and model references, PgBouncer routes, and bound PVCs. Do not run schema migrations or write administrative SQL during cutover.
3. Verify exact golden image IDs and embedded revisions, all pod readiness and restart counts, and no resource or disk pressure.
4. Verify all nine desired realtime profiles have current healthy worker owners; master-controlled capacity covers their leases and active backfills. Test realtime and active-shard reassignment by deleting a disposable worker pod and checking successful replacement and job completion. Choose a backfill range outside already drained source chunks.
5. Verify Grafana `/monitor`, bot `/api/bot`, and ingester `/api/ingester`; Prometheus targets, fresh Loki logs, 30-day Prometheus and Loki retention, and no new sustained critical Grafana alerts. Source-owned alert rules are baked and provisioned through the Grafana chart.
6. Verify each previously enabled trading process retains its durable intent and has a fresh heartbeat after bot startup. Confirm model artifact availability, required ingester routes, fresh inputs, gRPC, order reconciliation, settlement, and accounting. Do not force a trade. Start the bot only when the operator accepts that the restored enabled live process may resume real orders.
7. Keep the Docker Compose services stopped after all checks pass. Compose remains maintained for focused tests and recovery, but do not run it concurrently against the live k3s process state.

The controlled backfill recovery test on 2026-09-26 scheduled two Binance futures open-interest shards through `/api/ingester/backfills`. One worker pod was deleted while its shard was running; that shard completed on attempt two, and the parent completed both shards. A prior test over already drained August/September chunks failed with the database's intended historical insert guard and is not evidence of a worker recovery failure.
