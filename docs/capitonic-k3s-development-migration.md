# Capitonic k3s development migration plan

Status: proposed. This is the user-supplied plan, preserved for implementation planning. Repository-policy conflicts identified during review require resolution before the affected Kubernetes rollout steps. No application, Compose, or shared configuration changes are authorized by this document alone.

## Recommended architecture

Use independently deployable Helm charts under `charts/`, organized by microservice:

```text
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
charts/ingester/
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

A small orchestration layer can coordinate installation order without becoming another chart:

```text
scripts/helm/
  bake-dev.sh
  install-dev.sh
  upgrade-dev.sh
  verify-dev.sh
  uninstall-dev.sh
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

The Helm process should package or bake those files into Kubernetes ConfigMaps rather than duplicate them manually inside each chart.

The standardized bake process should:

1. Validate required local configuration and secrets.
2. Clone the required startup scripts and configuration files into an ephemeral staging directory.
3. Render or package them into ConfigMaps and Secrets.
4. Run Helm deployment using the rendered values.
5. Delete the staging directory.
6. Never commit rendered secrets or generated manifests.

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

The deployment script should read the existing `.env` files, validate required keys, and create or update Kubernetes Secrets at deployment time.

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
Namespace: capitonic-dev
Release: capitonic
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

No schema changes are part of this migration plan.

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

- Deployment;
- Service;
- Secrets;
- ConfigMaps;
- readiness/liveness probes;
- graceful shutdown;
- process-scoped health metrics;
- database and ingester dependencies;
- development-only access configuration.

The chart must preserve durable trading-process intent and existing process-scoped recovery behavior.

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

### Phase 1: Monitoring stack

Deploy:

- Loki;
- Prometheus;
- Alloy;
- Grafana;
- node-exporter or equivalent host metrics;
- cAdvisor or equivalent container metrics where appropriate.

Tasks:

1. Create the development namespace.
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

Soak period:

- several hours of continuous operation;
- no sustained scrape failures;
- no unexplained Alloy delivery errors;
- no persistent pod restarts;
- no storage or resource pressure;
- dashboards accessible throughout the soak.

### Phase 2: Database, PgBouncer, and migrations

Deploy:

- TimescaleDB;
- PgBouncer;
- db-migrate Job.

Tasks:

1. Create development PVCs.
2. Deploy TimescaleDB.
3. Verify database readiness and persistent storage.
4. Deploy PgBouncer.
5. Verify each service identity and pool route.
6. Run the committed migration set through the db-migrate Job.
7. Verify migration completion and ledger state.
8. Confirm Grafana can read the database through its intended route.
9. Confirm PgBouncer pool behavior and bounded connection counts.
10. Confirm database health metrics and logs appear in the monitoring stack.

Soak period:

- sustained database health;
- no migration retries;
- no PgBouncer connection exhaustion;
- no unexpected restarts;
- no storage errors;
- no monitoring regressions.

### Phase 3: Ingester and workers

Deploy:

- ingester-master;
- fixed number of ingester-worker replicas.

Tasks:

1. Deploy the master.
2. Verify master readiness.
3. Deploy workers with the same immutable ingester image.
4. Verify worker registration.
5. Verify worker capability and contract versions.
6. Verify realtime profile assignment.
7. Verify allocation limits.
8. Verify worker metrics.
9. Verify backfill API basics.
10. Schedule a bounded development backfill.
11. Confirm the master assigns the job correctly.
12. Confirm the worker reports progress and completion.
13. Restart one worker and verify lease recovery.
14. Confirm healthy workers continue operating.
15. Confirm no worker can exceed capacity.
16. Confirm Prometheus and Grafana show the expected worker state.

Explicitly excluded from this phase:

- no automatic worker scaling;
- no Kubernetes API credentials;
- no master-driven replica changes;
- no automated image rollout controller.

### Phase 4: Polymarket bot

Deploy:

- polymarket-bot.

Tasks:

1. Verify the bot resolves the ingester master Service.
2. Verify database connectivity through PgBouncer.
3. Verify startup and readiness behavior.
4. Verify process-scoped health metrics.
5. Confirm configured trading-process intent is preserved.
6. Confirm runtime readiness remains process-scoped.
7. Confirm trading safety checks remain intact.
8. Confirm no unexpected process disablement occurs due to Kubernetes restarts.
9. Verify dashboards and alerts for trading-path health.
10. Keep live capital disabled until the development rollout has completed its soak and all acceptance evidence is recorded.

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
charts/ingester/
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

## Repository review notes (pending resolution)

- Standalone charts need distinct Helm release names. Keep `capitonic` as the shared application identity and namespace naming convention, but do not use it as one release name for all independently installed charts.
- `AGENTS.md` currently requires ingester workers to be created and scaled only through the Compose `ingester-worker` service. A Kubernetes worker Deployment requires an explicit, narrowly scoped repository-policy amendment before rollout.
- `AGENTS.md` currently permits applying migrations only by recreating the Compose `db-migrate` container. Running the same committed migrations in a Kubernetes Job requires an explicit repository-policy amendment before execution. A new empty development database may still need its baseline migration ledger established.
- Existing shared configuration contains Docker-specific discovery and hostnames. In particular, Alloy reads `/var/run/docker.sock`, Prometheus has static Compose targets, and PgBouncer points to `timescaledb-0`. The Helm path needs additive Kubernetes-specific rendering or overlays while leaving the Compose source configuration intact.
- The plan provisions a new development database but does not specify whether existing data should be copied. Treat the database as empty until the intended data scope and a separate safe data-migration procedure are defined.
