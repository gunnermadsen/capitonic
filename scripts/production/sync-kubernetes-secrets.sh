#!/usr/bin/env bash
set -euo pipefail
set +x

AWS_REGION="${AWS_REGION:-eu-west-1}"
APP_SECRET_NAME="${APP_SECRET_NAME:-capitonic/polymarket-bot/production}"
NAMESPACE="${CAPITONIC_NAMESPACE:-capitonic}"
RESTART_SCOPE="${RESTART_SCOPE:-all}"
ALLOW_DATABASE_CREDENTIAL_ROTATION="${ALLOW_DATABASE_CREDENTIAL_ROTATION:-false}"

[[ "$AWS_REGION" == "eu-west-1" ]]
case "$RESTART_SCOPE" in all|none|application|monitoring) ;; *) echo "Invalid RESTART_SCOPE." >&2; exit 64 ;; esac

temporary_directory="$(mktemp -d /run/capitonic-secrets.XXXXXX)"
trap 'rm -rf "$temporary_directory"' EXIT
chmod 0700 "$temporary_directory"
secret_json="$temporary_directory/secret.json"
secret_version="$(aws secretsmanager get-secret-value --region "$AWS_REGION" --secret-id "$APP_SECRET_NAME" \
  --query VersionId --output text)"
aws secretsmanager get-secret-value --region "$AWS_REGION" --secret-id "$APP_SECRET_NAME" \
  --query SecretString --output text >"$secret_json"
chmod 0600 "$secret_json"
jq -e 'type == "object"' "$secret_json" >/dev/null

required_keys=(
  POSTGRES_PASSWORD CAPITONIC_TRADING_POSTGRES_PASSWORD
  CAPITONIC_INGESTER_MASTER_POSTGRES_PASSWORD CAPITONIC_INGESTER_WORKER_POSTGRES_PASSWORD
  CAPITONIC_GRAFANA_POSTGRES_PASSWORD POLYMARKET_HTTP_ADMIN_TOKEN
  MARKET_DATA_INGESTER_ADMIN_TOKEN GRAFANA_ADMIN_PASSWORD
  PROMETHEUS_BASIC_AUTH_USER PROMETHEUS_BASIC_AUTH_PASSWORD PROMETHEUS_BASIC_AUTH_PASSWORD_HASH
  POLYMARKET_CLOB_API_KEY POLYMARKET_CLOB_SECRET POLYMARKET_CLOB_PASSPHRASE
  POLYMARKET_PRIVATE_KEY POLYMARKET_FUNDER_ADDRESS POLYMARKET_SIGNATURE_TYPE
  CLOUDFLARE_API_TOKEN CLOUDFLARED_PRODUCTION_TUNNEL_CREDENTIALS_B64
)
for key in "${required_keys[@]}"; do
  jq -e --arg key "$key" '(.[$key] // "") | type == "string" and length > 0' "$secret_json" >/dev/null || {
    echo "AWS Secrets Manager secret is missing required key $key." >&2
    exit 1
  }
done

kubectl create namespace "$NAMESPACE" --dry-run=client -o yaml | kubectl apply -f - >/dev/null
kubectl create namespace cert-manager --dry-run=client -o yaml | kubectl apply -f - >/dev/null

json_value() { jq -er --arg key "$1" '.[$key]' "$secret_json"; }
existing_value() {
  kubectl -n "$NAMESPACE" get secret postgres-credentials -o jsonpath="{.data.$1}" 2>/dev/null | base64 -d || true
}
if kubectl -n "$NAMESPACE" get secret postgres-credentials >/dev/null 2>&1; then
  for key in POSTGRES_PASSWORD CAPITONIC_TRADING_POSTGRES_PASSWORD CAPITONIC_INGESTER_MASTER_POSTGRES_PASSWORD CAPITONIC_INGESTER_WORKER_POSTGRES_PASSWORD CAPITONIC_GRAFANA_POSTGRES_PASSWORD; do
    if [[ "$(existing_value "$key")" != "$(json_value "$key")" && "$ALLOW_DATABASE_CREDENTIAL_ROTATION" != "true" ]]; then
      echo "Database credential $key changed; refresh cannot rotate database roles. Run an approved rotation workflow." >&2
      exit 1
    fi
  done
fi

write_env_file() {
  local destination="$1"; shift
  : >"$destination"
  chmod 0600 "$destination"
  while (( $# > 0 )); do
    local output_key="$1" json_key="$2"
    shift 2
    printf '%s=%s\n' "$output_key" "$(json_value "$json_key")" >>"$destination"
  done
}

apply_env_secret() {
  local name="$1" file="$2"
  kubectl -n "$NAMESPACE" create secret generic "$name" --from-env-file="$file" \
    --dry-run=client -o yaml | kubectl apply -f - >/dev/null
  kubectl -n "$NAMESPACE" annotate secret "$name" capitonic.io/asm-version="$secret_version" --overwrite >/dev/null
}

write_env_file "$temporary_directory/postgres.env" \
  POSTGRES_PASSWORD POSTGRES_PASSWORD \
  CAPITONIC_TRADING_POSTGRES_PASSWORD CAPITONIC_TRADING_POSTGRES_PASSWORD \
  CAPITONIC_INGESTER_MASTER_POSTGRES_PASSWORD CAPITONIC_INGESTER_MASTER_POSTGRES_PASSWORD \
  CAPITONIC_INGESTER_WORKER_POSTGRES_PASSWORD CAPITONIC_INGESTER_WORKER_POSTGRES_PASSWORD \
  CAPITONIC_GRAFANA_POSTGRES_PASSWORD CAPITONIC_GRAFANA_POSTGRES_PASSWORD
apply_env_secret postgres-credentials "$temporary_directory/postgres.env"

write_env_file "$temporary_directory/bot.env" admin-token POLYMARKET_HTTP_ADMIN_TOKEN
apply_env_secret polymarket-bot-auth "$temporary_directory/bot.env"

write_env_file "$temporary_directory/live.env" \
  POLYMARKET_CLOB_API_KEY POLYMARKET_CLOB_API_KEY \
  POLYMARKET_CLOB_SECRET POLYMARKET_CLOB_SECRET \
  POLYMARKET_CLOB_PASSPHRASE POLYMARKET_CLOB_PASSPHRASE \
  POLYMARKET_PRIVATE_KEY POLYMARKET_PRIVATE_KEY \
  POLYMARKET_FUNDER_ADDRESS POLYMARKET_FUNDER_ADDRESS \
  POLYMARKET_SIGNATURE_TYPE POLYMARKET_SIGNATURE_TYPE
apply_env_secret polymarket-live-auth "$temporary_directory/live.env"

write_env_file "$temporary_directory/ingester.env" admin-token MARKET_DATA_INGESTER_ADMIN_TOKEN
apply_env_secret ingester-auth "$temporary_directory/ingester.env"

printf 'POLYMARKET_CHAINLINK_DATA_STREAMS_API_KEY=%s\n' "$(jq -r '.POLYMARKET_CHAINLINK_DATA_STREAMS_API_KEY // ""' "$secret_json")" >"$temporary_directory/providers.env"
printf 'POLYMARKET_CHAINLINK_DATA_STREAMS_API_SECRET=%s\n' "$(jq -r '.POLYMARKET_CHAINLINK_DATA_STREAMS_API_SECRET // ""' "$secret_json")" >>"$temporary_directory/providers.env"
printf 'POLYMARKET_CHAINLINK_DATA_STREAMS_CANDLESTICK_API_KEY=%s\n' "$(jq -r '.POLYMARKET_CHAINLINK_DATA_STREAMS_CANDLESTICK_API_KEY // ""' "$secret_json")" >>"$temporary_directory/providers.env"
chmod 0600 "$temporary_directory/providers.env"
apply_env_secret ingester-provider-auth "$temporary_directory/providers.env"

grafana_user="$(jq -r '.GRAFANA_ADMIN_USER // "admin"' "$secret_json")"
printf 'admin-user=%s\nadmin-password=%s\n' "$grafana_user" "$(json_value GRAFANA_ADMIN_PASSWORD)" >"$temporary_directory/grafana.env"
chmod 0600 "$temporary_directory/grafana.env"
apply_env_secret grafana-auth "$temporary_directory/grafana.env"

write_env_file "$temporary_directory/prometheus.env" \
  username PROMETHEUS_BASIC_AUTH_USER password PROMETHEUS_BASIC_AUTH_PASSWORD password-hash PROMETHEUS_BASIC_AUTH_PASSWORD_HASH
jq -n \
  --arg username "$(json_value PROMETHEUS_BASIC_AUTH_USER)" \
  --arg password_hash "$(json_value PROMETHEUS_BASIC_AUTH_PASSWORD_HASH)" \
  '{basic_auth_users: {($username): $password_hash}}' >"$temporary_directory/web.yml"
chmod 0600 "$temporary_directory/web.yml"
kubectl -n "$NAMESPACE" create secret generic prometheus-auth \
  --from-env-file="$temporary_directory/prometheus.env" \
  --from-file=web.yml="$temporary_directory/web.yml" \
  --dry-run=client -o yaml | kubectl apply -f - >/dev/null
kubectl -n "$NAMESPACE" annotate secret prometheus-auth capitonic.io/asm-version="$secret_version" --overwrite >/dev/null

write_env_file "$temporary_directory/cloudflare-dns.env" api-token CLOUDFLARE_API_TOKEN
apply_env_secret cloudflare-dns "$temporary_directory/cloudflare-dns.env"
kubectl -n cert-manager create secret generic cloudflare-dns \
  --from-env-file="$temporary_directory/cloudflare-dns.env" --dry-run=client -o yaml | kubectl apply -f - >/dev/null
kubectl -n cert-manager annotate secret cloudflare-dns capitonic.io/asm-version="$secret_version" --overwrite >/dev/null

json_value CLOUDFLARED_PRODUCTION_TUNNEL_CREDENTIALS_B64 | base64 -d >"$temporary_directory/credentials.json"
jq -e '(.TunnelID // .tunnelID // .tunnel_id // "") | length > 0' "$temporary_directory/credentials.json" >/dev/null
chmod 0600 "$temporary_directory/credentials.json"
kubectl -n "$NAMESPACE" create secret generic cloudflare-tunnel \
  --from-file=credentials.json="$temporary_directory/credentials.json" \
  --dry-run=client -o yaml | kubectl apply -f - >/dev/null
kubectl -n "$NAMESPACE" annotate secret cloudflare-tunnel capitonic.io/asm-version="$secret_version" --overwrite >/dev/null

if [[ "$RESTART_SCOPE" == "all" || "$RESTART_SCOPE" == "application" ]]; then
  for deployment in pgbouncer ingester-master ingester-worker polymarket-bot; do
    kubectl -n "$NAMESPACE" get deployment "$deployment" >/dev/null 2>&1 && kubectl -n "$NAMESPACE" rollout restart "deployment/$deployment"
  done
fi
if [[ "$RESTART_SCOPE" == "all" || "$RESTART_SCOPE" == "monitoring" ]]; then
  for deployment in grafana cloudflared; do
    kubectl -n "$NAMESPACE" get deployment "$deployment" >/dev/null 2>&1 && kubectl -n "$NAMESPACE" rollout restart "deployment/$deployment"
  done
  kubectl -n "$NAMESPACE" get statefulset prometheus >/dev/null 2>&1 && kubectl -n "$NAMESPACE" rollout restart statefulset/prometheus
fi

if [[ "$RESTART_SCOPE" != "none" ]]; then
  for workload in deployment/pgbouncer deployment/ingester-master deployment/ingester-worker deployment/polymarket-bot deployment/grafana deployment/cloudflared statefulset/prometheus; do
    kubectl -n "$NAMESPACE" get "$workload" >/dev/null 2>&1 && kubectl -n "$NAMESPACE" rollout status "$workload" --timeout=15m
  done
fi

echo "Synchronized allowlisted ASM version $secret_version into Kubernetes secrets; host-only credentials were excluded."
