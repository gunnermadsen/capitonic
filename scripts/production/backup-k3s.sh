#!/usr/bin/env bash
set -euo pipefail
set +x

NAMESPACE="${CAPITONIC_NAMESPACE:-capitonic}"
AWS_REGION="${AWS_REGION:-eu-west-1}"
BACKUP_BUCKET="${BACKUP_BUCKET:-capitonic-polybot-backups-192200846560-euw1}"
APP_SECRET_NAME="${APP_SECRET_NAME:-capitonic/polymarket-bot/production}"
APP_DIRECTORY="${APP_DIRECTORY:-/opt/polymarket-bot}"
timestamp="$(date -u +%Y%m%dT%H%M%SZ)"
revision="$(git -C "$APP_DIRECTORY" rev-parse HEAD)"
prefix="production/$timestamp-$revision"
backup_directory="$(mktemp -d /var/lib/capitonic-backup.XXXXXX)"
recovery_needed=true

restore_runtime() {
  [[ "$recovery_needed" == "true" ]] || return 0
  kubectl -n "$NAMESPACE" scale statefulset/timescaledb --replicas=1 >/dev/null 2>&1 || true
  kubectl -n "$NAMESPACE" rollout status statefulset/timescaledb --timeout=10m >/dev/null 2>&1 || true
  kubectl -n "$NAMESPACE" scale deployment/pgbouncer --replicas=1 >/dev/null 2>&1 || true
  kubectl -n "$NAMESPACE" scale statefulset/prometheus statefulset/loki --replicas=1 >/dev/null 2>&1 || true
  kubectl -n "$NAMESPACE" scale deployment/grafana deployment/ingester-master deployment/polymarket-bot --replicas=1 >/dev/null 2>&1 || true
  kubectl -n "$NAMESPACE" scale deployment/ingester-worker --replicas=4 >/dev/null 2>&1 || true
}
trap 'restore_runtime' ERR INT TERM
trap 'rm -rf "$backup_directory"' EXIT
chmod 0700 "$backup_directory"

kubectl -n "$NAMESPACE" exec timescaledb-0 -c timescaledb -- \
  pg_dump -U postgres -d polymarket -Fc >"$backup_directory/postgres-polymarket.dump"
kubectl -n "$NAMESPACE" exec daemonset/alloy -- \
  tar -C /var/lib/alloy/data -czf - . >"$backup_directory/alloy-positions.tar.gz"

kubectl -n "$NAMESPACE" get all,pvc,ingress,certificate -o yaml >"$backup_directory/kubernetes-runtime.yaml"
kubectl get pv -o yaml >"$backup_directory/kubernetes-persistent-volumes.yaml"
helm -n "$NAMESPACE" list -o json >"$backup_directory/helm-releases.json"
kubectl -n "$NAMESPACE" get pods -o json | jq '[.items[] | {name:.metadata.name,images:[.status.containerStatuses[]? | {image,imageID,restartCount}]}]' \
  >"$backup_directory/image-identities.json"
kubectl -n "$NAMESPACE" exec timescaledb-0 -c timescaledb -- psql -U postgres -d polymarket -At -F '|' -c \
  'SELECT process_id,process_key,status,enabled,heartbeat_at FROM polymarket.trading_processes ORDER BY process_id' \
  >"$backup_directory/trading-processes.txt"
kubectl -n "$NAMESPACE" exec timescaledb-0 -c timescaledb -- psql -U postgres -d polymarket -At -F '|' -c \
  'SELECT timestamp,name FROM public.migrations ORDER BY timestamp' >"$backup_directory/migrations.txt"
aws secretsmanager describe-secret --region "$AWS_REGION" --secret-id "$APP_SECRET_NAME" \
  --query '{ARN:ARN,LastChangedDate:LastChangedDate}' --output json >"$backup_directory/asm-secret.json"
aws secretsmanager list-secret-version-ids --region "$AWS_REGION" --secret-id "$APP_SECRET_NAME" \
  --include-deprecated --query 'Versions[?contains(VersionStages, `AWSCURRENT`)].{VersionId:VersionId,VersionStages:VersionStages}' \
  --output json >"$backup_directory/asm-version.json"

kubectl -n "$NAMESPACE" scale deployment/ingester-master --replicas=0
kubectl -n "$NAMESPACE" scale deployment/ingester-worker deployment/polymarket-bot --replicas=0
kubectl -n "$NAMESPACE" scale deployment/grafana --replicas=0
kubectl -n "$NAMESPACE" scale statefulset/prometheus statefulset/loki --replicas=0
kubectl -n "$NAMESPACE" rollout status deployment/ingester-master --timeout=10m
kubectl -n "$NAMESPACE" rollout status deployment/ingester-worker --timeout=10m
kubectl -n "$NAMESPACE" rollout status deployment/polymarket-bot --timeout=10m
kubectl -n "$NAMESPACE" rollout status deployment/grafana --timeout=10m
kubectl -n "$NAMESPACE" rollout status statefulset/prometheus --timeout=10m
kubectl -n "$NAMESPACE" rollout status statefulset/loki --timeout=10m

archive_pvc() {
  local claim="$1" output="$2" volume path
  volume="$(kubectl -n "$NAMESPACE" get pvc "$claim" -o jsonpath='{.spec.volumeName}')"
  path="$(kubectl get pv "$volume" -o json | jq -r '.spec.hostPath.path // .spec.local.path // empty')"
  [[ "$path" == /var/lib/rancher/k3s/* ]]
  tar -C "$path" -czf "$backup_directory/$output" .
}
archive_pvc data-prometheus-0 prometheus-tsdb.tar.gz
archive_pvc data-loki-0 loki-data.tar.gz
archive_pvc grafana-data grafana-data.tar.gz
archive_pvc ingester-archives ingester-archives.tar.gz

kubectl -n "$NAMESPACE" scale deployment/pgbouncer --replicas=0
kubectl -n "$NAMESPACE" scale statefulset/timescaledb --replicas=0
kubectl -n "$NAMESPACE" rollout status deployment/pgbouncer --timeout=10m
kubectl -n "$NAMESPACE" rollout status statefulset/timescaledb --timeout=10m
archive_pvc data-timescaledb-0 timescaledb-physical.tar.gz

(
  cd "$backup_directory"
  find . -type f ! -name SHA256SUMS -print0 | sort -z | xargs -0 shasum -a 256 >SHA256SUMS
  shasum -a 256 -c SHA256SUMS
)
aws s3 cp "$backup_directory" "s3://$BACKUP_BUCKET/$prefix/" --recursive --only-show-errors --sse AES256

object_count="$(aws s3api list-objects-v2 --region "$AWS_REGION" --bucket "$BACKUP_BUCKET" \
  --prefix "$prefix/" --query 'length(Contents)' --output text)"
local_count="$(find "$backup_directory" -type f | wc -l | tr -d ' ')"
[[ "$object_count" == "$local_count" ]]
aws s3 cp "s3://$BACKUP_BUCKET/$prefix/SHA256SUMS" "$backup_directory/SHA256SUMS.remote" --only-show-errors
cmp "$backup_directory/SHA256SUMS" "$backup_directory/SHA256SUMS.remote"

printf '%s\n' "$prefix" >/var/lib/capitonic-last-backup
recovery_needed=false
trap - ERR INT TERM
printf 'Validated production backup: s3://%s/%s/ (%s objects)\n' "$BACKUP_BUCKET" "$prefix" "$object_count"
