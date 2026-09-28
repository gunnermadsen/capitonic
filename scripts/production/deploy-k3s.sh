#!/usr/bin/env bash
set -euo pipefail
set +x

APP_DIRECTORY="${APP_DIRECTORY:-/opt/polymarket-bot}"
NAMESPACE="${CAPITONIC_NAMESPACE:-capitonic}"
AWS_REGION="${AWS_REGION:-eu-west-1}"
APP_SECRET_NAME="${APP_SECRET_NAME:-capitonic/polymarket-bot/production}"
CERT_MANAGER_VERSION="${CERT_MANAGER_VERSION:-v1.18.2}"
cd "$APP_DIRECTORY"

[[ "$AWS_REGION" == "eu-west-1" ]]
[[ -z "$(git status --porcelain --untracked-files=no)" ]]
deployment_revision="$(git rev-parse HEAD)"
[[ "$deployment_revision" =~ ^[0-9a-f]{40}$ ]]

for component in polymarket-bot ingester db-migrate; do
  image="$(yq -r .image "capitonic-helm-chart/environments/production/$component.yaml")"
  [[ "$image" =~ ^192200846560\.dkr\.ecr\.eu-west-1\.amazonaws\.com/capitonic/$component@sha256:[0-9a-f]{64}$ ]]
  [[ "$image" != *sha256:0000000000000000000000000000000000000000000000000000000000000000 ]]
done

scripts/production/refresh-ecr-pull-secret.sh
RESTART_SCOPE=none scripts/production/sync-kubernetes-secrets.sh
python3 scripts/helm/bake-assets.py
trap 'python3 scripts/helm/bake-assets.py --clean >/dev/null 2>&1 || true' EXIT

helm upgrade --install cert-manager oci://quay.io/jetstack/charts/cert-manager \
  --version "$CERT_MANAGER_VERSION" --namespace cert-manager --create-namespace \
  --set crds.enabled=true --wait --timeout 10m

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

deploy_chart() {
  local chart="$1"
  helm lint "capitonic-helm-chart/charts/$chart" \
    -f "capitonic-helm-chart/environments/production/$chart.yaml"
  helm upgrade --install "$chart" "capitonic-helm-chart/charts/$chart" \
    --namespace "$NAMESPACE" --create-namespace \
    -f "capitonic-helm-chart/environments/production/$chart.yaml" --wait --timeout 15m
}

deploy_chart timescaledb

source_migrations="$(find packages/db-migrate/src/migrations -maxdepth 1 -type f -name '[0-9]*.ts' -exec basename {} \; | cut -d- -f1 | sort -n)"
applied_migrations="$(kubectl -n "$NAMESPACE" exec timescaledb-0 -c timescaledb -- psql -U postgres -d polymarket -Atc \
  "SELECT timestamp FROM public.migrations ORDER BY timestamp" 2>/dev/null || true)"
unexpected_applied="$(comm -23 <(printf '%s\n' "$applied_migrations" | sed '/^$/d' | sort -n) <(printf '%s\n' "$source_migrations" | sort -n))"
[[ -z "$unexpected_applied" ]] || { echo "Database ledger contains migrations outside this release: $unexpected_applied" >&2; exit 1; }
pending_migrations="$(comm -13 <(printf '%s\n' "$applied_migrations" | sed '/^$/d' | sort -n) <(printf '%s\n' "$source_migrations" | sort -n))"
printf 'Approved committed pending migrations:\n%s\n' "${pending_migrations:-none}"

kubectl -n "$NAMESPACE" delete job db-migrate --ignore-not-found --wait=true >/dev/null
deploy_chart db-migrate
latest_expected="$(tail -n1 <<<"$source_migrations")"
latest_applied="$(kubectl -n "$NAMESPACE" exec timescaledb-0 -c timescaledb -- psql -U postgres -d polymarket -Atc \
  'SELECT coalesce(max(timestamp),0) FROM public.migrations')"
[[ "$latest_applied" == "$latest_expected" ]]

deploy_chart pgbouncer
deploy_chart ingester
deploy_chart prometheus
deploy_chart loki
deploy_chart grafana
deploy_chart alloy

helm lint capitonic-helm-chart/charts/cloudflared
helm upgrade --install cloudflared capitonic-helm-chart/charts/cloudflared \
  --namespace "$NAMESPACE" --set-string tunnelId="$tunnel_id" --wait --timeout 10m

deploy_chart polymarket-bot
scripts/production/reconcile-production-profiles.sh
scripts/production/deploy-pilot-pair.sh
scripts/production/verify-production.sh

printf 'Production k3s deployment complete at revision %s.\n' "$deployment_revision"
