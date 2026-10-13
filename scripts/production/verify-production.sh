#!/usr/bin/env bash
set -euo pipefail
set +x

NAMESPACE="${CAPITONIC_NAMESPACE:-capitonic}"
APP_DIRECTORY="${APP_DIRECTORY:-/opt/polymarket-bot}"
BOT_PORT="${BOT_LOCAL_PORT:-18097}"
INGESTER_PORT="${INGESTER_LOCAL_PORT:-18098}"
PROMETHEUS_PORT="${PROMETHEUS_LOCAL_PORT:-19090}"
GRAFANA_PORT="${GRAFANA_LOCAL_PORT:-13000}"

workload_pods=()
while IFS= read -r pod_name; do
  workload_pods+=("pod/$pod_name")
done < <(kubectl -n "$NAMESPACE" get pods -o json \
  | jq -r '.items[] | select(.status.phase != "Succeeded") | .metadata.name')
(( ${#workload_pods[@]} > 0 ))
kubectl -n "$NAMESPACE" wait --for=condition=Ready --timeout=15m "${workload_pods[@]}"
unhealthy="$(kubectl -n "$NAMESPACE" get pods -o json | jq '[.items[] | select(.status.phase != "Succeeded") | select(any(.status.containerStatuses[]?; .ready != true))] | length')"
[[ "$unhealthy" == "0" ]]
kubectl -n "$NAMESPACE" get job db-migrate -o json | jq -e '.status.succeeded == 1 and (.status.failed // 0) == 0' >/dev/null

expected_bot="$(yq -r '.image | sub("-rc\\.[0-9]+$", "")' "$APP_DIRECTORY/capitonic-helm-chart/environments/production/polymarket-bot.yaml")"
expected_ingester="$(yq -r '.image | sub("-rc\\.[0-9]+$", "")' "$APP_DIRECTORY/capitonic-helm-chart/environments/production/ingester.yaml")"
expected_migrate="$(yq -r '.image | sub("-rc\\.[0-9]+$", "")' "$APP_DIRECTORY/capitonic-helm-chart/environments/production/db-migrate.yaml")"
for image in "$expected_bot" "$expected_ingester" "$expected_migrate"; do
  # Production ECR images MUST use the /capitonic/ repository namespace; unnamespaced images are forbidden.
  [[ "$image" =~ ^192200846560\.dkr\.ecr\.eu-west-1\.amazonaws\.com/capitonic/(polymarket-bot|ingester|db-migrate)(@sha256:[0-9a-f]{64}|:v[0-9]+\.[0-9]+\.[0-9]+)$ ]]
  [[ "$image" != *sha256:0000000000000000000000000000000000000000000000000000000000000000 ]]
done
[[ "$(kubectl -n "$NAMESPACE" get deployment polymarket-bot -o jsonpath='{.spec.template.spec.containers[0].image}')" == "$expected_bot" ]]
[[ "$(kubectl -n "$NAMESPACE" get deployment ingester-master -o jsonpath='{.spec.template.spec.containers[0].image}')" == "$expected_ingester" ]]
[[ "$(kubectl -n "$NAMESPACE" get deployment ingester-worker -o jsonpath='{.spec.template.spec.containers[0].image}')" == "$expected_ingester" ]]
[[ "$(kubectl -n "$NAMESPACE" get job db-migrate -o jsonpath='{.spec.template.spec.containers[0].image}')" == "$expected_migrate" ]]

latest_committed_migration="$(find "$APP_DIRECTORY/packages/db-migrate/src/migrations" "$APP_DIRECTORY/packages/db-migrate/src/fresh-install" -maxdepth 1 -type f -name '[0-9]*.ts' -exec basename {} \; | cut -d- -f1 | sort -n | tail -n1)"
latest_applied_migration="$(kubectl -n "$NAMESPACE" exec timescaledb-0 -c timescaledb -- \
  psql -U postgres -d polymarket -Atc 'SELECT coalesce(max(timestamp),0) FROM public.migrations')"
[[ "$latest_applied_migration" == "$latest_committed_migration" ]]
latest_committed_weather_migration="$(find "$APP_DIRECTORY/packages/db-migrate/src/migrations/weather" -maxdepth 1 -type f -name '[0-9]*.ts' -exec basename {} \; | cut -d- -f1 | sort -n | tail -n1)"
latest_applied_weather_migration="$(kubectl -n "$NAMESPACE" exec timescaledb-0 -c timescaledb -- \
  psql -U postgres -d polymarket -Atc 'SELECT coalesce(max(timestamp),0) FROM public.weather_migrations')"
[[ "$latest_applied_weather_migration" == "$latest_committed_weather_migration" ]]

bot_token="$(kubectl -n "$NAMESPACE" get secret polymarket-bot-auth -o jsonpath='{.data.admin-token}' | base64 -d)"
[[ "$(kubectl -n "$NAMESPACE" exec deployment/grafana -- printenv POLYMARKET_HTTP_ADMIN_TOKEN)" == "$bot_token" ]]
ingester_token="$(kubectl -n "$NAMESPACE" get secret ingester-auth -o jsonpath='{.data.admin-token}' | base64 -d)"
prom_user="$(kubectl -n "$NAMESPACE" get secret prometheus-auth -o jsonpath='{.data.username}' | base64 -d)"
prom_password="$(kubectl -n "$NAMESPACE" get secret prometheus-auth -o jsonpath='{.data.password}' | base64 -d)"
grafana_user="$(kubectl -n "$NAMESPACE" get secret grafana-auth -o jsonpath='{.data.admin-user}' | base64 -d)"
grafana_password="$(kubectl -n "$NAMESPACE" get secret grafana-auth -o jsonpath='{.data.admin-password}' | base64 -d)"

kubectl -n "$NAMESPACE" port-forward service/polymarket-bot "$BOT_PORT:8097" >/tmp/capitonic-verify-bot.log 2>&1 & bot_forward=$!
kubectl -n "$NAMESPACE" port-forward service/ingester-master "$INGESTER_PORT:8098" >/tmp/capitonic-verify-ingester.log 2>&1 & ingester_forward=$!
kubectl -n "$NAMESPACE" port-forward service/prometheus "$PROMETHEUS_PORT:9090" >/tmp/capitonic-verify-prometheus.log 2>&1 & prometheus_forward=$!
kubectl -n "$NAMESPACE" port-forward service/grafana "$GRAFANA_PORT:3000" >/tmp/capitonic-verify-grafana.log 2>&1 & grafana_forward=$!
trap 'kill "$bot_forward" "$ingester_forward" "$prometheus_forward" "$grafana_forward" 2>/dev/null || true' EXIT
for attempt in $(seq 1 90); do
  curl -fsS "http://127.0.0.1:$BOT_PORT/health/ready" >/dev/null 2>&1 && \
  curl -fsS "http://127.0.0.1:$INGESTER_PORT/health/ready" >/dev/null 2>&1 && \
  curl -fsS "http://127.0.0.1:$GRAFANA_PORT/api/health" >/dev/null 2>&1 && break
  sleep 1
done

bot_unauthorized="$(curl -sS -o /dev/null -w '%{http_code}' "http://127.0.0.1:$BOT_PORT/admin/trading-processes?limit=1")"
ingester_unauthorized="$(curl -sS -o /dev/null -w '%{http_code}' "http://127.0.0.1:$INGESTER_PORT/ingesters")"
prometheus_unauthorized="$(curl -sS -o /dev/null -w '%{http_code}' "http://127.0.0.1:$PROMETHEUS_PORT/api/v1/query?query=up")"
[[ "$bot_unauthorized" == "401" || "$bot_unauthorized" == "403" ]]
[[ "$ingester_unauthorized" == "401" || "$ingester_unauthorized" == "403" ]]
[[ "$prometheus_unauthorized" == "401" ]]

profiles_ready=false
for attempt in $(seq 1 90); do
  profiles="$(curl -fsS -H "Authorization: Bearer $ingester_token" "http://127.0.0.1:$INGESTER_PORT/ingesters")"
  if jq -e '
    all(.[] | select(.desired_state == "running");
      .observed_state == "running" and .health_status == "healthy" and .lease_owner != null and .source_watermark != null)
  ' <<<"$profiles" >/dev/null; then
    profiles_ready=true
    break
  fi
  sleep 5
done
[[ "$profiles_ready" == "true" ]]

curl -fsS "http://127.0.0.1:$GRAFANA_PORT/api/health" | jq -e '.database == "ok"' >/dev/null
curl -fsS -u "$grafana_user:$grafana_password" \
  "http://127.0.0.1:$GRAFANA_PORT/api/datasources/uid/polymarket-bot-runtime" \
  | jq -e '.uid == "polymarket-bot-runtime" and .type == "yesoreyeram-infinity-datasource"' >/dev/null
curl -fsS -u "$grafana_user:$grafana_password" \
  "http://127.0.0.1:$GRAFANA_PORT/api/plugins/yesoreyeram-infinity-datasource/settings" \
  | jq -e '.id == "yesoreyeram-infinity-datasource" and .type == "datasource"' >/dev/null
curl -fsS -u "$grafana_user:$grafana_password" \
  "http://127.0.0.1:$GRAFANA_PORT/api/datasources/uid/polymarket-bot-runtime/health" \
  | jq -e '.status == "OK"' >/dev/null
kubectl -n "$NAMESPACE" get certificate grafana-tls -o json | jq -e 'any(.status.conditions[]?; .type == "Ready" and .status == "True")' >/dev/null
for certificate in api-tls metrics-tls; do
  kubectl -n "$NAMESPACE" wait --for=condition=Ready "certificate/$certificate" --timeout=5m
done
kubectl -n "$NAMESPACE" get deployment cloudflared -o json | jq -e '
  .spec.replicas > 0 and
  .status.readyReplicas == .spec.replicas and
  .status.availableReplicas == .spec.replicas
' >/dev/null
cloudflared_config="$(kubectl -n "$NAMESPACE" get configmap cloudflared -o jsonpath='{.data.config\.yml}')"
[[ "$(yq -r '.ingress[] | select(.hostname == "ssh.capitonic.com") | .service' <<<"$cloudflared_config")" == "ssh://127.0.0.1:22" ]]
[[ "$(yq -r '.ingress[] | select(.hostname == "ops.capitonic.com") | .service' <<<"$cloudflared_config")" == "http://traefik.kube-system.svc.cluster.local:80" ]]
[[ "$(yq -r '[.ingress[] | select(.hostname == "monitor.capitonic.com")] | length' <<<"$cloudflared_config")" == "0" ]]
ss -ltnH '( sport = :22 )' | awk '{print $4}' | grep -qx '127.0.0.1:22'
[[ "$(ss -ltnH '( sport = :22 )' | wc -l | tr -d ' ')" == "1" ]]
grafana_headers="$(mktemp /tmp/capitonic-grafana-headers.XXXXXX)"
trap 'rm -f "$grafana_headers"; kill "$bot_forward" "$ingester_forward" "$prometheus_forward" "$grafana_forward" 2>/dev/null || true' EXIT
grafana_status="$(curl -sS --resolve monitor.capitonic.com:443:127.0.0.1 \
  -D "$grafana_headers" -o /dev/null -w '%{http_code}' https://monitor.capitonic.com/)"
[[ "$grafana_status" == "302" ]]
grep -Eiq '^location: /login([?[:space:]]|$)' "$grafana_headers"
public_bot_unauthorized="$(curl -sS --resolve api.capitonic.com:443:127.0.0.1 -o /dev/null -w '%{http_code}' \
  'https://api.capitonic.com/admin/trading-processes?limit=1')"
public_ingester_unauthorized="$(curl -sS --resolve api.capitonic.com:443:127.0.0.1 -o /dev/null -w '%{http_code}' \
  'https://api.capitonic.com/ingesters')"
public_prometheus_unauthorized="$(curl -sS --resolve metrics.capitonic.com:443:127.0.0.1 -o /dev/null -w '%{http_code}' \
  'https://metrics.capitonic.com/api/v1/query?query=up')"
[[ "$public_bot_unauthorized" == "401" || "$public_bot_unauthorized" == "403" ]]
[[ "$public_ingester_unauthorized" == "401" || "$public_ingester_unauthorized" == "403" ]]
[[ "$public_prometheus_unauthorized" == "401" ]]
curl -fsS --resolve api.capitonic.com:443:127.0.0.1 -H "Authorization: Bearer $bot_token" \
  'https://api.capitonic.com/admin/trading-processes?limit=1' | jq -e '.processes | type == "array"' >/dev/null
curl -fsS --resolve api.capitonic.com:443:127.0.0.1 -H "Authorization: Bearer $ingester_token" \
  'https://api.capitonic.com/ingesters' | jq -e 'type == "array"' >/dev/null
curl -fsS --resolve metrics.capitonic.com:443:127.0.0.1 -u "$prom_user:$prom_password" \
  'https://metrics.capitonic.com/api/v1/query?query=up' | jq -e '.status == "success"' >/dev/null
kubectl get ingress -A -o json | jq -e '
  [.items[] as $ingress | $ingress.spec.rules[]? |
    select(.host == "monitor.capitonic.com") |
    {namespace: $ingress.metadata.namespace, name: $ingress.metadata.name, paths: [.http.paths[].path]}]
  == [{namespace: "capitonic", name: "grafana", paths: ["/"]}]
' >/dev/null

jq -n --arg bot_image "$expected_bot" --arg ingester_image "$expected_ingester" --arg migration "$latest_applied_migration" \
  --argjson realtime_profiles "$(jq '[.[] | select(.desired_state == "running")] | length' <<<"$profiles")" \
  --argjson workers "$(kubectl -n "$NAMESPACE" get deployment ingester-worker -o json | jq '.status.readyReplicas // 0')" \
  '{status:"ready",architecture:"arm64",bot_image:$bot_image,ingester_image:$ingester_image,latest_migration:$migration,realtime_profiles:$realtime_profiles,workers:$workers}'
