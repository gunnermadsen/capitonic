#!/usr/bin/env bash
set -euo pipefail
set +x

APP_DIRECTORY="${APP_DIRECTORY:-/opt/polymarket-bot}"
NAMESPACE="${CAPITONIC_NAMESPACE:-capitonic}"
AWS_REGION="${AWS_REGION:-eu-west-1}"
APP_SECRET_NAME="${APP_SECRET_NAME:-capitonic/polymarket-bot/production}"
CERT_MANAGER_VERSION="${CERT_MANAGER_VERSION:-v1.18.2}"
export KUBECONFIG="${KUBECONFIG:-/etc/rancher/k3s/k3s.yaml}"
cd "$APP_DIRECTORY"

[[ "$AWS_REGION" == "eu-west-1" ]]
[[ -z "$(git status --porcelain --untracked-files=no)" ]]
deployment_revision="$(git rev-parse HEAD)"
[[ "$deployment_revision" =~ ^[0-9a-f]{40}$ ]]
case "${DEPLOYMENT_SCOPE:-full-stack}" in
  selected) IFS=, read -ra components <<< "${DEPLOY_COMPONENTS:-}" ;;
  full-stack) components=(polymarket-bot ingester db-migrate) ;;
  *) echo "Unsupported deployment scope: $DEPLOYMENT_SCOPE" >&2; exit 64 ;;
esac
scripts/production/install-yq.sh

for component in "${components[@]}"; do
  case "$component" in
    polymarket-bot|ingester|db-migrate) ;;
    grafana|prometheus|loki|alloy|cloudflared) continue ;;
    *) echo "Unsupported deployment component: $component" >&2; exit 64 ;;
  esac
  image="$(yq -r .image "capitonic-helm-chart/environments/production/$component.yaml")"
  [[ "$image" =~ ^192200846560\.dkr\.ecr\.eu-west-1\.amazonaws\.com/capitonic/$component@sha256:[0-9a-f]{64}$ ]]
  [[ "$image" != *sha256:0000000000000000000000000000000000000000000000000000000000000000 ]]
done

deploy_chart() {
  local chart="$1" wait_for_jobs=()
  shift
  [[ "$chart" == "db-migrate" ]] && wait_for_jobs=(--wait-for-jobs)
  helm lint "capitonic-helm-chart/charts/$chart" \
    -f "capitonic-helm-chart/environments/production/$chart.yaml" "$@"
  helm upgrade --install "$chart" "capitonic-helm-chart/charts/$chart" \
    --namespace "$NAMESPACE" --create-namespace \
    -f "capitonic-helm-chart/environments/production/$chart.yaml" \
    --atomic --wait "${wait_for_jobs[@]}" --timeout 15m "$@"
}

if [[ "${DEPLOYMENT_SCOPE:-full-stack}" == selected ]]; then
  source scripts/production/deploy-selected-components.sh
  deploy_selected_components
  exit 0
fi

scripts/production/refresh-ecr-pull-secret.sh
RESTART_SCOPE=none scripts/production/sync-kubernetes-secrets.sh
prometheus_user="$(kubectl -n "$NAMESPACE" get secret prometheus-auth -o jsonpath='{.data.username}' | base64 -d)"
PROMETHEUS_BASIC_AUTH_USER="$prometheus_user" python3 scripts/helm/bake-assets.py
unset prometheus_user
trap 'python3 scripts/helm/bake-assets.py --clean >/dev/null 2>&1 || true' EXIT

helm upgrade --install cert-manager oci://quay.io/jetstack/charts/cert-manager \
  --version "$CERT_MANAGER_VERSION" --namespace cert-manager --create-namespace \
  --set crds.enabled=true --atomic --wait --timeout 10m

secret_file="$(mktemp /run/capitonic-deploy-secret.XXXXXX)"
trap 'rm -f "$secret_file"; python3 scripts/helm/bake-assets.py --clean >/dev/null 2>&1 || true' EXIT
aws secretsmanager get-secret-value --region "$AWS_REGION" --secret-id "$APP_SECRET_NAME" \
  --query SecretString --output text >"$secret_file"
chmod 0600 "$secret_file"
acme_email="$(jq -er '.CLOUDFLARE_EMAIL_ADDRESS' "$secret_file")"
tunnel_id="$(kubectl -n "$NAMESPACE" get secret cloudflare-tunnel -o jsonpath='{.data.credentials\.json}' | base64 -d | jq -r '.TunnelID // .tunnelID // .tunnel_id')"
[[ "$tunnel_id" =~ ^[0-9a-f-]{36}$ ]]

helm upgrade --install cert-manager-config capitonic-helm-chart/charts/cert-manager-config \
  --namespace "$NAMESPACE" --create-namespace --set-string email="$acme_email" --wait --timeout 5m


helm lint capitonic-helm-chart/charts/timescaledb \
  -f capitonic-helm-chart/environments/production/timescaledb.yaml
helm upgrade --install timescaledb capitonic-helm-chart/charts/timescaledb \
  --namespace "$NAMESPACE" --create-namespace \
  -f capitonic-helm-chart/environments/production/timescaledb.yaml --timeout 15m
for attempt in $(seq 1 180); do
  database_ready="$(kubectl -n "$NAMESPACE" get pod timescaledb-0 \
    -o jsonpath='{.status.containerStatuses[?(@.name=="timescaledb")].ready}' 2>/dev/null || true)"
  [[ "$database_ready" == "true" ]] && break
  sleep 5
done
[[ "${database_ready:-}" == "true" ]]

source_migrations="$(find packages/db-migrate/src/migrations packages/db-migrate/src/fresh-install \
  -maxdepth 1 -type f -name '[0-9]*.ts' -exec basename {} \; | cut -d- -f1 | sort -n)"
applied_migrations="$(kubectl -n "$NAMESPACE" exec timescaledb-0 -c timescaledb -- psql -U postgres -d polymarket -Atc \
  "SELECT timestamp FROM public.migrations ORDER BY timestamp" 2>/dev/null || true)"
unexpected_applied="$(comm -23 <(printf '%s\n' "$applied_migrations" | sed '/^$/d' | sort -n) <(printf '%s\n' "$source_migrations" | sort -n))"
[[ -z "$unexpected_applied" ]] || { echo "Database ledger contains migrations outside this release: $unexpected_applied" >&2; exit 1; }
pending_migrations="$(comm -13 <(printf '%s\n' "$applied_migrations" | sed '/^$/d' | sort -n) <(printf '%s\n' "$source_migrations" | sort -n))"
printf 'Approved committed pending migrations:\n%s\n' "${pending_migrations:-none}"

source_weather_migrations="$(find packages/db-migrate/src/migrations/weather \
  -maxdepth 1 -type f -name '[0-9]*.ts' -exec basename {} \; | cut -d- -f1 | sort -n)"
applied_weather_migrations="$(kubectl -n "$NAMESPACE" exec timescaledb-0 -c timescaledb -- psql -U postgres -d polymarket -Atc \
  "SELECT timestamp FROM public.weather_migrations ORDER BY timestamp" 2>/dev/null || true)"
unexpected_weather_applied="$(comm -23 <(printf '%s\n' "$applied_weather_migrations" | sed '/^$/d' | sort -n) <(printf '%s\n' "$source_weather_migrations" | sort -n))"
[[ -z "$unexpected_weather_applied" ]] || { echo "Weather migration ledger contains migrations outside this release: $unexpected_weather_applied" >&2; exit 1; }
pending_weather_migrations="$(comm -13 <(printf '%s\n' "$applied_weather_migrations" | sed '/^$/d' | sort -n) <(printf '%s\n' "$source_weather_migrations" | sort -n))"
printf 'Approved committed pending weather migrations:\n%s\n' "${pending_weather_migrations:-none}"

for migration_job in db-migrate-baseline db-migrate-weather db-migrate; do
  kubectl -n "$NAMESPACE" delete job "$migration_job" --ignore-not-found --wait=true >/dev/null
done

helm upgrade --install db-migrate-baseline capitonic-helm-chart/charts/db-migrate \
  --namespace "$NAMESPACE" --create-namespace \
  -f capitonic-helm-chart/environments/production/db-migrate.yaml \
  --set-string jobName=db-migrate-baseline \
  --set-string 'migrationsPattern=migrations/baseline-only/*.js' \
  --atomic --wait --wait-for-jobs --timeout 15m

helm upgrade --install db-migrate-weather capitonic-helm-chart/charts/db-migrate \
  --namespace "$NAMESPACE" --create-namespace \
  -f capitonic-helm-chart/environments/production/db-migrate.yaml \
  --set-string jobName=db-migrate-weather \
  --set-json 'command=["node","dist/weather-main.js"]' \
  --atomic --wait --wait-for-jobs --timeout 15m
latest_weather_expected="$(tail -n1 <<<"$source_weather_migrations")"
latest_weather_applied="$(kubectl -n "$NAMESPACE" exec timescaledb-0 -c timescaledb -- psql -U postgres -d polymarket -Atc \
  'SELECT coalesce(max(timestamp),0) FROM public.weather_migrations')"
[[ "$latest_weather_applied" == "$latest_weather_expected" ]]

deploy_chart db-migrate
latest_expected="$(tail -n1 <<<"$source_migrations")"
latest_applied="$(kubectl -n "$NAMESPACE" exec timescaledb-0 -c timescaledb -- psql -U postgres -d polymarket -Atc \
  'SELECT coalesce(max(timestamp),0) FROM public.migrations')"
[[ "$latest_applied" == "$latest_expected" ]]

deploy_chart timescaledb
deploy_chart pgbouncer
if ! kubectl -n argocd get application ingester >/dev/null 2>&1; then
  deploy_chart ingester
fi
deploy_chart prometheus
deploy_chart loki
deploy_chart grafana
deploy_chart alloy

scripts/production/configure-tunnel-ssh.sh
helm lint capitonic-helm-chart/charts/cloudflared \
  -f capitonic-helm-chart/environments/production/cloudflared.yaml
helm upgrade --install cloudflared capitonic-helm-chart/charts/cloudflared \
  --namespace "$NAMESPACE" \
  -f capitonic-helm-chart/environments/production/cloudflared.yaml \
  --set-string tunnelId="$tunnel_id" --atomic --wait --timeout 10m

if kubectl -n argocd get application polymarket-bot >/dev/null 2>&1; then
  scripts/production/reconcile-application-charts.sh
else
  deploy_chart polymarket-bot
fi
scripts/production/reconcile-production-profiles.sh
scripts/production/deploy-pilot-pair.sh
scripts/production/verify-production.sh

printf 'Production k3s deployment complete at revision %s.\n' "$deployment_revision"
