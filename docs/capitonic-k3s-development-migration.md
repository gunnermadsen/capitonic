# Capitonic k3s development migration plan

Status: monitoring, database, migration runner, and bot service are deployed on `feature/capitonic-monitoring-k3s`. Ingester master, workers, and trading-process acceptance follow them. Docker Compose remains unchanged.

## Recommended architecture

Use independently deployable Helm charts under `capitonic-helm-chart/charts/`, organized by microservice:

```text
capitonic-helm-chart/
  charts/
    prometheus/
    grafana/
    loki/
    alloy/
    timescaledb/
    pgbouncer/
    db-migrate/
    ingester/
    polymarket-bot/
```

Each service directory is a complete Helm chart:

```text
capitonic-helm-chart/charts/ingester/
  Chart.yaml
  values.yaml
  values-dev.yaml
  templates/
    _helpers.tpl
    serviceaccount.yaml
    configmap.yaml
    secret.yaml
    master-deployment.yaml
    worker-deployment.yaml
    master-service.yaml
    worker-service.yaml
    master-servicemonitor.yaml
    worker-servicemonitor.yaml
    pdb.yaml
    networkpolicy.yaml
  files/
    startup/
    config/
```

The charts should remain standalone and deployable. We should not flatten manifests into one directory or package Docker Compose files inside Helm.

A small asset bake utility prepares chart-local files without invoking Helm or Kubernetes:

```text
scripts/helm/
  bake-assets.py
```

Later, a parent platform chart can be introduced if useful, but it should not be required for the initial migration.

## Configuration ownership

Keep `common/` as the repository source for shared configuration:

```text
common/
  configs/
    prometheus/
    grafana/
    loki/
    alloy/
    pgbouncer/
  scripts/
    grafana-provisioning-entrypoint.sh
    prometheus-provisioning-entrypoint.sh
    pgbouncer-entrypoint.sh
```

The Helm process should package or bake those files into Kubernetes ConfigMaps rather than duplicate them manually inside each chart. `common/scripts/` remains the source for microservice startup scripts; root `scripts/` holds project utilities, including Helm bake and deployment commands. No startup script is required by the monitoring chart.

The standardized bake process should:

1. Validate the source configuration needed to render chart assets.
2. Copy or render the required configuration and startup files into ignored `assets/` directories inside the respective charts.
3. Leave those assets in place for ordinary `helm lint`, `helm template`, and `helm upgrade` commands.
4. Remove the generated assets with `bake-assets.py --clean` after the Helm operation.
5. Never commit rendered secrets or generated manifests.

The generated Kubernetes objects may contain Secret manifests during deployment, but the rendered output must remain ephemeral and must not be committed to Git.

## Secret handling

Sensitive values remain in local environment files and are not committed:

- database passwords;
- service-role passwords;
- API keys;
- private keys;
- Grafana credentials;
- trading credentials;
- Chainlink credentials;
- Polygon or provider credentials.

Kubernetes Secrets should be created or updated separately from baking and Helm chart rendering, using the existing `.env` files. The bake command does not read secret values or create Kubernetes resources.

Non-sensitive configuration should live in Helm values or ConfigMaps:

- service URLs;
- ports;
- resource requests and limits;
- scrape intervals;
- deployment identifiers;
- database aliases;
- feature configuration;
- worker capacity settings;
- persistence sizes.

The initial implementation should use native Kubernetes Secrets. External secret management can remain a future production concern.

## Naming and labels

All charts should use a consistent namespace and metadata standard:

```text
Namespace: capitonic
Releases: prometheus, grafana, loki, alloy (one per standalone chart)
Platform: capitonic-platform
Environment: dev
```

Every resource should use standard labels:

```yaml
app.kubernetes.io/name
app.kubernetes.io/instance
app.kubernetes.io/component
app.kubernetes.io/part-of: capitonic
app.kubernetes.io/managed-by: Helm
capitonic.io/environment
capitonic.io/rollout-domain
```

This will make future production cutovers, selectors, dashboards, and rollout verification much easier.

## Service chart responsibilities

### Monitoring charts

`prometheus`, `grafana`, `loki`, and `alloy` should contain:

- Deployments or StatefulSets as appropriate;
- Services;
- ConfigMaps from `common/configs`;
- PVCs;
- readiness and liveness probes;
- resource requests and limits;
- provisioning configuration;
- scrape configuration;
- Grafana dashboards and alert rules;
- access configuration for development;
- service accounts and minimal RBAC;
- PodDisruptionBudgets where meaningful.

The monitoring charts should preserve the existing provisioned configuration. Manual Grafana UI changes should not become the source of truth.

### Database chart

`timescaledb` should contain:

- a StatefulSet;
- a headless service;
- a client service;
- persistent storage;
- initialization configuration;
- readiness and liveness probes;
- resource requests and limits;
- controlled shutdown behavior;
- development-only storage configuration.

The first migration should not attempt database HA. A single-node local cluster remains a single database failure domain.

### PgBouncer chart

`pgbouncer` should contain:

- Deployment;
- Service;
- ConfigMap generated from the existing configuration;
- Secret-backed credentials;
- readiness and liveness probes;
- bounded resource settings;
- connection-pool configuration;
- service identity routing.

The current session-versus-transaction pooling behavior must remain unchanged.

### db-migrate chart

`db-migrate` should be modeled as a controlled Kubernetes Job, not a long-running Deployment.

It should:

- run only after TimescaleDB and PgBouncer prerequisites are healthy;
- use the existing migration image;
- execute the committed TypeORM migrations;
- report completion;
- fail visibly when migration execution fails;
- be independently verifiable before dependent services start.

The Job runs only the repository's committed migration code. On a fresh database,
that code establishes the existing schema baseline and then applies migrations
not already recorded in its ledger; this rollout does not author new migrations.

### Ingester chart

The `ingester` chart should contain both runtime roles while preserving the existing binary contract:

```text
INGESTER_MODE=master
INGESTER_MODE=worker
```

It should provide:

- master Deployment;
- worker Deployment;
- master Service;
- worker metrics Service;
- ConfigMaps and Secrets;
- readiness/liveness/startup probes;
- worker capacity configuration;
- deployment identifier;
- graceful termination;
- worker drain behavior;
- Prometheus scrape metadata;
- resource requests and limits;
- initial fixed worker replica count.

The initial rollout explicitly excludes:

- autoscaling;
- Kubernetes API integration;
- master-driven replica changes;
- worker pool automation.

Those are later additions after the static Kubernetes deployment is proven.

### Polymarket bot chart

The `polymarket-bot` chart should contain:

- one Deployment with `Recreate` strategy and one internal Service;
- references to separately managed Kubernetes Secrets sourced from the main worktree `.env` files;
- startup and liveness probes independent of database availability, plus database-backed readiness;
- the existing graceful shutdown and process-scoped health metrics;
- the PgBouncer route and an ingester master URL reserved for later process activation.

The chart must preserve durable trading-process intent and existing process-scoped recovery behavior. See `docs/capitonic-k3s-bot-standards.md` for the local bot deployment contract.

## Health and observability standard

Every service chart should define:

- startup probe where initialization is meaningful;
- readiness probe for dependency-backed serving readiness;
- liveness probe only for actual process failure;
- metrics endpoint;
- Prometheus scrape configuration;
- bounded termination grace period;
- explicit resource requests and limits.

Health should distinguish:

- process is alive;
- process is ready;
- dependencies are reachable;
- data is fresh;
- workload is eligible;
- service is operationally healthy.

For the ingester specifically, health verification must include:

- master readiness;
- worker registration;
- worker heartbeat freshness;
- allocation capacity;
- realtime lease ownership;
- backfill assignment;
- worker saturation;
- API availability;
- source freshness;
- persistence freshness.

Kubernetes probes must not become blunt trading kill switches. They should prevent unsafe routing or restart genuinely failed pods without changing durable trading intent.

## Rollout phases

### Monitoring stack rollout

Deploy:

- Loki;
- Prometheus;
- Alloy;
- Grafana;

Tasks:

1. Create the `capitonic` namespace.
2. Deploy persistent storage.
3. Deploy Loki.
4. Deploy Prometheus.
5. Deploy Alloy.
6. Deploy Grafana.
7. Load dashboards, alerts, and datasources from `common/`.
8. Expose Grafana through a development-safe local access method.
9. Verify Kubernetes probes and metrics collection.
10. Confirm logs arrive in Loki.
11. Confirm Prometheus targets are healthy.
12. Confirm Grafana dashboards render against the expected datasources.

Monitoring acceptance is a current health check, not a multi-hour soak. Verify
ready pods, Bound storage, healthy scrape targets, fresh logs, and working
Grafana datasources before adding dependent services. Run the extended system
soak after the code-driven microservices are deployed and producing data.

### Database, PgBouncer, and migrations rollout

Monitoring entry gate, checked on 2026-09-24: Alloy, Grafana, Loki, and
Prometheus each have one ready pod with zero restarts; all three monitoring
PVCs are Bound. Prometheus reports its three configured targets healthy,
Loki receives fresh namespace logs, and both provisioned Grafana datasource
health checks pass. Recheck current health after any Rancher Desktop restart
and before deploying the database; no monitoring-only soak is required.

Deployment boundary: create a **new, isolated, empty development database**.
Do not copy the Compose database, switch existing applications to Kubernetes,
or run migrations against the Compose database during this rollout. Preserve
the Compose services and configuration as they are. A data copy would require
a separate, explicit data-scope and recovery plan.

Predeployment gates:

1. The Kubernetes Job exception is committed in `AGENTS.md` on
   `docs/model-training-artifact-lifecycle`. Follow repository branch policy
   to bring that commit into the deployment lineage before executing the Job.
   The existing pending-migration approval and exact-list gates still apply.
2. Size the Rancher Desktop node and database together. Rancher Desktop is
   configured for eight CPUs and 8 GiB of memory; after restart, Kubernetes
   reported eight allocatable CPUs and about 7.75 GiB allocatable memory.
   The development Compose database
   has a 6 GiB limit and tunes PostgreSQL for that capacity. Set database
   requests, limits, and tuning within the actual node budget, reserving
   capacity for monitoring and later services; do not copy Compose tuning
   unchanged.
3. Confirm free host disk, the selected database PVC capacity, and a recovery
   method before creating data. The local-path StorageClass cannot expand a
   claim in place and has a Delete reclaim policy. Pin immutable, locally
   available arm64 image identities for TimescaleDB, PgBouncer, and db-migrate;
   verify the migration image contains the intended committed migration set.
4. Confirm the exact pending migration list and its effects against the empty
   database before running the Job. Do not add new migration code as part of
   this Helm rollout.

Implementation and order:

1. Add independent `charts/timescaledb`, `charts/db-migrate`, and
   `charts/pgbouncer` releases to `capitonic-helm-chart`; keep all resources
   in namespace `capitonic` with the existing names and labels. Use a
   single-replica TimescaleDB StatefulSet with a retained data PVC, a headless
   governing Service, a stable client Service, probes, bounded resources, and
   graceful shutdown. Do not add database HA to this single-node environment.
2. Model db-migrate as a separately invoked, one-shot Kubernetes Job after
   the policy and migration-authorization gates are met. Connect directly to
   the new TimescaleDB Service as `postgres`, using the existing image and
   committed TypeORM runner. Do not put migration execution in database pod
   startup or in a Helm hook that could rerun on an ordinary upgrade. Record
   the exact image, Job result, migration ledger, and pending-count result.
3. Use a PgBouncer Deployment and ClusterIP Service. Bake the canonical
   `common/configs/pgbouncer/pgbouncer.ini` into ignored chart assets, changing
   only the Kubernetes database hostname in the generated copy. Bake the
   existing `common/scripts/pgbouncer-entrypoint.sh` unchanged. Keep all
   session and transaction aliases, pool sizes, and the 30-backend global
   ceiling. Source password values only from main-worktree `.env` files into
   Kubernetes Secrets; do not commit, print, or bake secret values.
4. Render, lint, and inspect the charts and generated assets. Deploy and
   verify TimescaleDB first, including storage persistence across a pod
   restart. Run and verify the migration Job next. Deploy PgBouncer only after
   its service roles exist; check its readiness and every route with the
   intended least-privilege identity.
5. After the Job succeeds, add a PostgreSQL datasource to the canonical
   Kubernetes Grafana provisioning file in `common/configs/`, with its
   credentials from `.env` and its route through PgBouncer. Bake and upgrade
   Grafana through Helm. Provision database and PgBouncer health metrics
   through the existing observability stack, then verify a bounded read-only
   query, metrics, and fresh logs without disrupting the four monitoring
   releases. Record the required observability provisioning tag for the exact
   deployed commit.

Expected effects of the existing migration runner on this **empty** database:
the fresh-install baseline creates the TimescaleDB and pgcrypto extensions,
the `ingester`, `market_data`, and `polymarket` schemas, 47 application tables,
their functions, indexes, and hypertables, and 113 historical migration-ledger
entries. The seven later committed migrations create four PostgreSQL service
roles and grants; seed 11 ingester profiles (eight have desired state
`running`, though no ingester is deployed in this rollout); change the Binance
one-second profile's websocket URL; add drain job events and a trigger;
extend and secure verified-drain functions. No existing operational rows are
copied. The baseline and profile seed are irreversible migrations. On this
empty database, schema locks are local to the new database; creating
TimescaleDB objects and indexes may use CPU and disk, and the migration Job
must complete before any dependent service starts. A failed or partial Job is
investigated from its logs and ledger; never reset or reverse database state
with an ad hoc command. Preserve the PVC for recovery and use an approved
TypeORM migration for any later correction.

Acceptance: the database and PgBouncer remain ready without restart loops;
the PVC is Bound and survives a database pod restart; the Job completes once
with the expected ledger and zero pending migrations; role permissions and
session/transaction routes match the existing contract; backend connections
remain below the configured limit; Grafana's provisioned database datasource
passes a read-only query; database and PgBouncer metrics and fresh logs appear;
and all four monitoring services remain healthy. Check for migration retries,
connection exhaustion, restarts, and storage errors before deploying
application services. The extended soak belongs to the integrated system once
the code-driven services are producing data.

### Polymarket bot rollout

Deploy:

- polymarket-bot.

Tasks:

1. Confirm the isolated Kubernetes database has no trading processes and the database and monitoring services are healthy.
2. Build only the bot image from committed source with its Git revision embedded, and import it into k3s without changing the Compose image.
3. Create the admin-token Secret from the main worktree `.env`; reuse the existing trading database credential Secret.
4. Deploy the single bot replica with Helm and verify the PgBouncer route, startup, lightweight liveness, database readiness, and authenticated admin API.
5. Provision its Prometheus scrape and Grafana availability alert from `common/configs/` through the existing bake and Helm commands.
6. Confirm bot metrics and logs arrive, restart the pod once, and verify automatic reconnection without a restart loop or change to Compose.

No trading process is created during this rollout. Market-data route resolution, process-scoped readiness, trading safety, and the integrated soak require the ingester and its data inputs.

### Ingester and workers rollout

Deploy one `ingester-master` and one `ingester-worker` from the same immutable image. Verify existing lightweight live and ready endpoints, database connectivity, worker registration and heartbeat, immutable image identity, and Prometheus targets. Before starting the worker, set the seeded development profiles to stopped through the master's authenticated lifecycle API and confirm zero desired-running profiles and zero active jobs. This rollout verifies startup health only; realtime ingestion, backfills, trading process creation, and integrated soak require separate activation. See `docs/capitonic-k3s-ingester-standards.md`.

Explicitly excluded from this phase:

- no automatic worker scaling;
- no Kubernetes API credentials;
- no master-driven replica changes;
- no automated image rollout controller.

## Future scaling architecture

After the static ingester deployment is stable, add the Capitonic-to-Kubernetes scaling bridge.

The ownership boundary should be:

```text
Ingester master:
  desired worker capacity
  job priority
  worker compatibility
  lease ownership
  job progress
  scale-down eligibility

Kubernetes:
  actual replica count
  pod lifecycle
  image rollout
  resource scheduling
  readiness and termination
```

The master should calculate desired capacity from:

- queued allocation units;
- active realtime profiles;
- backfill demand;
- job age;
- worker capacity;
- compatibility constraints;
- deployment state.

Kubernetes should then reconcile the desired worker replica count.

Do not use CPU-only autoscaling for ingester workers. Domain demand is the primary scaling signal.

The scaling bridge should use narrowly scoped Kubernetes RBAC:

- read worker Deployment status;
- read pod readiness;
- update only the ingester worker Deployment replica count;
- optionally update a dedicated worker-pool custom resource later.

It must not have unrestricted cluster-admin access.

## Future safe image rollout

The worker rollout should be capacity-first:

1. Publish an immutable worker image.
2. Update the Helm values or image digest.
3. Increase surge capacity.
4. Start new workers.
5. Wait for registration and compatibility.
6. Verify new workers are healthy.
7. Route new assignments to the new deployment identifier.
8. Drain old workers.
9. Confirm realtime leases and backfill jobs have transferred or completed.
10. Scale old workers down.
11. Verify data freshness and allocation health.
12. Retain the previous image identity for rollback until verification completes.

Jobs should generally be pinned to a logical deployment or capability pool, not an individual pod. Pod identities are ephemeral. Specific worker pinning should remain an exceptional contract supported only when required.

## Development-to-production compatibility

Development is the only initial environment, but the chart structure should leave room for later values:

```text
capitonic-helm-chart/charts/ingester/
  values.yaml
  values-dev.yaml
  values-prod.yaml
```

The first implementation should avoid building a full multi-environment abstraction. However, templates should already parameterize:

- namespace;
- image repository and digest;
- replica counts;
- resource requests and limits;
- storage classes;
- persistence sizes;
- external endpoints;
- ingress/access behavior;
- deployment identifiers;
- secret references;
- rollout strategy;
- environment labels.

Production should be a new values and infrastructure profile, not a forked chart architecture.

## Verification standard

Every phase should produce lightweight evidence:

- rendered Helm manifests;
- `helm lint` output;
- `helm template` validation;
- pod and service status;
- probe results;
- metrics target status;
- relevant logs;
- storage status;
- dependency readiness;
- phase-specific acceptance results.

The rollout should stop at the first failed phase gate. Do not proceed to dependent services while the preceding layer has unresolved health or persistence problems.

This plan preserves the existing Compose configuration as the current deployment source while establishing Kubernetes as a parallel, deliberate development deployment path. No Compose files, Dockerfiles, database migrations, or runtime contracts need to be changed as part of the planning step.

## Repository review notes (remaining for later services)

- `AGENTS.md` permits local Kubernetes ingester workers only through the Helm-managed `ingester-worker` Deployment in `capitonic`. Compose workers remain Compose-managed.
- The Kubernetes `db-migrate` Job policy is in the deployment lineage. The Job established the baseline migration ledger in the new empty development database; later schema changes still require committed, approved TypeORM migrations.
- Existing shared configuration contains Docker-specific discovery and hostnames. In particular, Alloy reads `/var/run/docker.sock`, Prometheus has static Compose targets, and PgBouncer points to `timescaledb-0`. The Helm path needs additive Kubernetes-specific rendering or overlays while leaving the Compose source configuration intact.
- The plan provisions a new development database but does not specify whether existing data should be copied. Treat the database as empty until the intended data scope and a separate safe data-migration procedure are defined.

## Monitoring rollout decisions

- Development monitoring deploys Loki and Prometheus as single-replica StatefulSets with independent `local-path` PVCs, Grafana as a single-replica `Recreate` Deployment with its own PVC, and Alloy as a namespace-scoped DaemonSet.
- Development does not deploy node-exporter or cAdvisor. Existing Compose files remain unchanged; their later removal is outside this task.
- `common/configs/` remains authoritative for checked-in service configuration, and `common/scripts/` remains authoritative for microservice startup scripts. Root `scripts/` owns project utilities, including the maintained `scripts/helm/bake-assets.py` command. Runtime `.env` files remain authoritative for secrets. The bake copies configuration into ignored chart assets for standard Helm commands; `--clean` removes them afterward. Generated assets are never edited in the chart or committed.
- Only the Grafana Prometheus alert and available dashboards are provisioned in the monitoring rollout. Database and trading alerts/datasources are installed with their owning services after those dependencies exist.
- See `docs/capitonic-k3s-monitoring-standards.md` for the bake command, chart layout, storage, access, and verification standards.
- See `docs/capitonic-k3s-database-standards.md` for database chart ownership, the separate migration Job, storage, secrets, and deployment commands.
