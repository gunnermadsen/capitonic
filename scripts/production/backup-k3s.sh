#!/usr/bin/env bash
set -euo pipefail
set +x

NAMESPACE="${CAPITONIC_NAMESPACE:-capitonic}"
AWS_REGION="${AWS_REGION:-eu-west-1}"
BACKUP_BUCKET="${BACKUP_BUCKET:-capitonic-polybot-backups-192200846560-euw1}"
APP_DIRECTORY="${APP_DIRECTORY:-/opt/polymarket-bot}"
export KUBECONFIG="${KUBECONFIG:-/etc/rancher/k3s/k3s.yaml}"
timestamp="$(date -u +%Y%m%dT%H%M%SZ)"
revision="$(git -C "$APP_DIRECTORY" rev-parse HEAD)"
prefix="production/$timestamp-$revision"
backup_directory="$(mktemp -d /var/lib/capitonic-backup.XXXXXX)"
monitoring_stopped=false
prometheus_replicas=0
grafana_replicas=0

restore_monitoring() {
  [[ "$monitoring_stopped" == "true" ]] || return 0
  kubectl -n "$NAMESPACE" scale statefulset/prometheus --replicas="$prometheus_replicas" >/dev/null
  kubectl -n "$NAMESPACE" scale deployment/grafana --replicas="$grafana_replicas" >/dev/null
  if (( prometheus_replicas > 0 )); then
    kubectl -n "$NAMESPACE" rollout status statefulset/prometheus --timeout=10m >/dev/null
  fi
  if (( grafana_replicas > 0 )); then
    kubectl -n "$NAMESPACE" rollout status deployment/grafana --timeout=10m >/dev/null
  fi
  monitoring_stopped=false
}

cleanup() {
  restore_monitoring || true
  rm -rf "$backup_directory"
}
trap cleanup EXIT
chmod 0700 "$backup_directory"

kubectl -n "$NAMESPACE" exec timescaledb-0 -c timescaledb -- \
  pg_dump -U postgres -d polymarket -Fc >"$backup_directory/postgres-polymarket.dump"
test -s "$backup_directory/postgres-polymarket.dump"
kubectl -n "$NAMESPACE" exec -i timescaledb-0 -c timescaledb -- pg_restore --list \
  <"$backup_directory/postgres-polymarket.dump" >/dev/null

prometheus_replicas="$(kubectl -n "$NAMESPACE" get statefulset/prometheus -o jsonpath='{.spec.replicas}')"
grafana_replicas="$(kubectl -n "$NAMESPACE" get deployment/grafana -o jsonpath='{.spec.replicas}')"
[[ "$prometheus_replicas" =~ ^[0-9]+$ ]]
[[ "$grafana_replicas" =~ ^[0-9]+$ ]]
monitoring_stopped=true
kubectl -n "$NAMESPACE" scale statefulset/prometheus --replicas=0
kubectl -n "$NAMESPACE" scale deployment/grafana --replicas=0
kubectl -n "$NAMESPACE" rollout status statefulset/prometheus --timeout=10m
kubectl -n "$NAMESPACE" rollout status deployment/grafana --timeout=10m

archive_pvc() {
  local claim="$1" output="$2" volume path
  volume="$(kubectl -n "$NAMESPACE" get pvc "$claim" -o jsonpath='{.spec.volumeName}')"
  path="$(kubectl get pv "$volume" -o json | jq -r '.spec.hostPath.path // .spec.local.path // empty')"
  [[ "$path" == /var/lib/rancher/k3s/* ]]
  tar -C "$path" -czf "$backup_directory/$output" .
}
archive_pvc data-prometheus-0 prometheus-tsdb.tar.gz
grafana_volume="$(kubectl -n "$NAMESPACE" get pvc grafana-data -o jsonpath='{.spec.volumeName}')"
grafana_path="$(kubectl get pv "$grafana_volume" -o json | jq -r '.spec.hostPath.path // .spec.local.path // empty')"
[[ "$grafana_path" == /var/lib/rancher/k3s/* ]]
tar -C "$grafana_path" -czf "$backup_directory/grafana-database.tar.gz" grafana.db

for artifact in postgres-polymarket.dump prometheus-tsdb.tar.gz grafana-database.tar.gz; do
  test -s "$backup_directory/$artifact"
done
tar -tzf "$backup_directory/prometheus-tsdb.tar.gz" >/dev/null
[[ "$(tar -tzf "$backup_directory/grafana-database.tar.gz")" == "grafana.db" ]]
restore_monitoring

(
  cd "$backup_directory"
  find . -type f ! -name SHA256SUMS -print0 | sort -z | xargs -0 shasum -a 256 >SHA256SUMS
  shasum -a 256 -c SHA256SUMS
)
aws s3 cp "$backup_directory" "s3://$BACKUP_BUCKET/$prefix/" --recursive --only-show-errors --sse AES256

remote_objects="$(aws s3api list-objects-v2 --region "$AWS_REGION" --bucket "$BACKUP_BUCKET" \
  --prefix "$prefix/" --output json)"
jq -e --arg prefix "$prefix/" '
  [(.Contents // [])[] | {name: (.Key | ltrimstr($prefix)), size: .Size}] as $objects |
  ($objects | length) == 4 and
  all($objects[]; .size > 0) and
  (([$objects[].name] | sort) ==
    ["SHA256SUMS", "grafana-database.tar.gz", "postgres-polymarket.dump", "prometheus-tsdb.tar.gz"])
' <<<"$remote_objects" >/dev/null
object_count="$(jq '(.Contents // []) | length' <<<"$remote_objects")"
aws s3 cp "s3://$BACKUP_BUCKET/$prefix/SHA256SUMS" "$backup_directory/SHA256SUMS.remote" --only-show-errors
cmp "$backup_directory/SHA256SUMS" "$backup_directory/SHA256SUMS.remote"

printf '%s\n' "$prefix" >/var/lib/capitonic-last-backup
printf 'Validated production backup: s3://%s/%s/ (%s objects)\n' "$BACKUP_BUCKET" "$prefix" "$object_count"
