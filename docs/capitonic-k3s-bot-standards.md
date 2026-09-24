# Capitonic local Kubernetes bot

`polymarket-bot` is one Helm release under `capitonic-helm-chart/charts/` in the `capitonic` namespace. It owns a single-replica Deployment with `Recreate` strategy and an internal ClusterIP Service on port 8097. It has no persistent volume: durable process intent and application state belong to the existing TimescaleDB schema, not the pod filesystem. The 60-second termination grace period allows the bot's existing shutdown handler to quiesce its runtime.

## Identity and configuration

Build the bot image only when explicitly authorized. Embed the committed source revision with `POLYMARKET_GIT_REVISION`, give the image a unique k3s tag, verify its ARM64 architecture and image ID, and import that image into Rancher Desktop's `k8s.io` containerd namespace. The chart uses `imagePullPolicy: Never`; verify the running pod's image ID and embedded revision. Never replace the mutable image currently used by Docker Compose or change Compose files as part of this deployment.

The main worktree's `.env` is the only source for `POLYMARKET_HTTP_ADMIN_TOKEN`; extract only that key into the separately managed `polymarket-bot-auth` Kubernetes Secret. The existing `postgres-credentials` Secret supplies `CAPITONIC_TRADING_POSTGRES_PASSWORD`. The chart references both Secrets and sets the existing `capitonic_trading` / `polymarket_trading` PgBouncer route. No secret value is baked, templated, or committed. No live trading credentials are supplied for the empty development database. This does not alter the durable trading-process contract or add a trading disable switch.

The bot keeps the existing `/health` response. `/health/live` returns immediately when its HTTP server can respond; Kubernetes startup and liveness probes use this path without querying dependencies. `/health/ready` reuses the existing bounded `SELECT 1` database health check. A temporary database outage removes the pod from Service endpoints without causing a liveness restart. These endpoints report service health; process-scoped runtime readiness remains in the existing bot API and metrics.

`common/configs/prometheus/prometheus.yml` remains the source for the bot scrape job. `scripts/helm/bake-assets.py` copies that job into the ignored Kubernetes Prometheus asset. The bot availability alert is sourced from `common/configs/grafana/provisioning/alerting/rules-polymarket-bot.yml` and baked into the existing Grafana chart. The bake prepares assets only; Helm and kubectl perform deployment. Run `python3 scripts/helm/bake-assets.py --clean` after chart operations. Trading-path alerts that require live ingester data are deferred until those services exist.

## Deployment and acceptance

Before installation, check that the Kubernetes database has no enabled trading processes, the database and monitoring pods are healthy, and the exact bot image is imported. Then run the bake, lint and server dry-run the manifests, install `polymarket-bot`, and upgrade Prometheus and Grafana using their independent charts. Verify pod readiness and image identity, authenticated admin access, both probe paths, the Prometheus target, Loki logs, and the Grafana availability rule. Confirm the bot reconnects and becomes ready after one controlled pod restart, and that the Compose bot retains its original image ID.

This rollout establishes the bot service only. It does not create a trading process, deploy the ingester, build ingester or migration images, alter migration code, or demonstrate market-data or trading-path readiness. The integrated soak follows deployment of the code-driven services and data inputs.

## Verified local deployment

The 2026-09-24 deployment uses bot source revision `1052bc9b92913c94d5828b4471b5d8ae09bde871` and ARM64 image ID `sha256:306618e67db42ee2777ac96f8eba6527665508a51ed5edf2986f9a3769cb388f`. The pod returned HTTP 200 from both probe paths before and after a controlled restart, remained at zero container restarts, and the Kubernetes database retained zero trading processes. The bot Prometheus target and all five other configured targets reported `up`; Grafana provisioned the bot availability rule and Loki received bot logs. The Docker Compose bot retained its original image ID `sha256:560472111c47cdf07efd019de49282f323292baa0bf8196a61622d86c4a986d9`.
