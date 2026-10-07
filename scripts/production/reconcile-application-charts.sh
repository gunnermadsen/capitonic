#!/usr/bin/env bash
set -euo pipefail
set +x
export KUBECONFIG="${KUBECONFIG:-/etc/rancher/k3s/k3s.yaml}"
cd "${APP_DIRECTORY:-/opt/polymarket-bot}"
revision="$(git rev-parse HEAD)"
preflight_only=false
case "${1:-}" in
 --preflight-only) preflight_only=true ;;
 '') ;;
 *) echo 'Usage: reconcile-application-charts.sh [--preflight-only]' >&2; exit 64 ;;
esac
applications=()
case "${DEPLOYMENT_SCOPE:-full-stack}" in
 selected)
  IFS=, read -ra requested <<< "${DEPLOY_COMPONENTS:-}"
  for component in "${requested[@]}"; do
   case "$component" in
    ingester|polymarket-bot|grafana|prometheus|loki|alloy|cloudflared|db-migrate) ;;
    *) echo "Unsupported deployment component: $component" >&2; exit 64 ;;
   esac
  done
  # Preserve the established shared-credential ordering within the selected set.
  for application in ingester polymarket-bot; do
   [[ ",${DEPLOY_COMPONENTS:-}," != *,$application,* ]] || applications+=("$application")
  done
  ;;
 full-stack) applications=(ingester polymarket-bot) ;;
 *) echo "Unsupported deployment scope: $DEPLOYMENT_SCOPE" >&2; exit 64 ;;
esac
((${#applications[@]})) || exit 0
snapshot="$(mktemp -d /run/capitonic-argo-release.XXXXXX)"
chmod 0700 "$snapshot"
kubectl -n argocd get application "${applications[@]}" -o json | jq 'if .kind == "Application" then {items: [.]} else . end' > "$snapshot/applications-before.json"
kubectl -n capitonic get deployments -o json > "$snapshot/deployments-before.json"
kubectl -n capitonic exec timescaledb-0 -c timescaledb -- psql -U postgres -d polymarket -Atc \
 "SELECT process_id FROM polymarket.trading_processes WHERE enabled ORDER BY process_id" > "$snapshot/enabled-before.txt"
ready="$(kubectl -n capitonic get deployment ingester-worker -o jsonpath='{.status.readyReplicas}')"
desired="$(kubectl -n capitonic exec timescaledb-0 -c timescaledb -- psql -U postgres -d polymarket -Atc "SELECT count(*) FROM ingester.profiles WHERE desired_state='running'")"
backfills="$(kubectl -n capitonic exec timescaledb-0 -c timescaledb -- psql -U postgres -d polymarket -Atc "SELECT coalesce(sum(allocation_units),0) FROM ingester.backfill_jobs WHERE assigned_worker_id IS NOT NULL AND status IN ('running','cancel_requested')")"
(( ready >= desired + backfills )) || { echo 'Worker capacity is insufficient; no Argo target changed.' >&2; exit 70; }
echo "Argo release $revision; rollback snapshot $snapshot; workers=$ready profiles=$desired backfills=$backfills"
wait_application() {
 local name="$1" target="$2" attempt
 for ((attempt=0; attempt<120; attempt++)); do
  if kubectl -n argocd get application "$name" -o json | jq -e --arg revision "$target" \
   '.status.sync.revision == $revision and .status.sync.status == "Synced" and .status.health.status == "Healthy" and (.status.operationState.phase // "Succeeded") != "Running"' >/dev/null; then
   return 0
  fi
  sleep 5
 done
 return 1
}
grafana_changed=false
coordinate_grafana=false
verified=true
for application in "${applications[@]}"; do
 values="capitonic-helm-chart/environments/production/$application.yaml"
 entries="$(yq -o=json '.externalSecrets.entries' "$values" | jq -ce '
  if type == "array" and length > 0 and all(.[];
    (.consumers | type == "array" and length > 0) and (.properties | type == "object" and length > 0))
  then . else error("Missing shared-secret consumer declarations") end')"
 while IFS= read -r entry; do
  name="$(jq -er .name <<< "$entry")"
  current="$(kubectl -n capitonic get externalsecret "$name" -o json)"
  wanted="$(CONFIG="$(yq -o=json '.externalSecrets' "$values")" ENTRY="$entry" python3 -c '
import json, os
config, entry = json.loads(os.environ["CONFIG"]), json.loads(os.environ["ENTRY"])
print(json.dumps({key: [config["secretId"], prop, "uuid/" + entry["version"]] for key, prop in entry["properties"].items()}, sort_keys=True))')"
  actual="$(jq -cS '[.spec.data[] | {key: .secretKey, value: [.remoteRef.key, .remoteRef.property, .remoteRef.version]}] | from_entries' <<< "$current")"
  if [[ "$(jq -cS . <<< "$wanted")" != "$actual" ]]; then
   while IFS= read -r consumer; do
    case "$consumer" in
     ingester-master|ingester-worker) owner=ingester ;;
     polymarket-bot|grafana) owner="$consumer" ;;
     *) echo "Unknown shared-secret consumer $consumer; no Argo target changed." >&2; exit 70 ;;
    esac
    if [[ "${DEPLOYMENT_SCOPE:-full-stack}" == selected && ",${DEPLOY_COMPONENTS:-}," != *,$owner,* ]]; then
     echo "Shared secret $name changes require explicit selection of $owner; no Argo target changed." >&2
     exit 70
    fi
   done < <(jq -er '.consumers[]' <<< "$entry")
  fi
 done < <(jq -c '.[]' <<< "$entries")
done
if [[ ",${applications[*]}," == *polymarket-bot* ]]; then
 desired_grafana="$(yq -r '.credentialRevisions.grafana // ""' capitonic-helm-chart/environments/production/grafana.yaml)"
 actual_grafana="$(kubectl -n capitonic get deployment grafana -o json | jq -r '.spec.template.metadata.annotations["capitonic.io/credential-revision"] // ""')"
 if [[ "$desired_grafana" != "$actual_grafana" ]]; then
  if [[ "${DEPLOYMENT_SCOPE:-full-stack}" == selected && ",${DEPLOY_COMPONENTS:-}," != *,grafana,* ]]; then
   echo 'Bot credential revision requires explicit selection of grafana; no Argo target changed.' >&2
   exit 70
  fi
  coordinate_grafana=true
  helm -n capitonic history grafana -o json | jq -er '[.[]|select(.status=="deployed")]|last|.revision' > "$snapshot/grafana-revision"
 fi
fi
# Each selected target must have a recorded immutable rollback revision before writes.
for application in "${applications[@]}"; do
 previous="$(jq -er --arg name "$application" '.items[]|select(.metadata.name==$name)|.spec.source.targetRevision' "$snapshot/applications-before.json")"
 [[ "$previous" =~ ^[0-9a-f]{40}$ ]] || { echo "No immutable rollback revision for $application; no Argo target changed." >&2; exit 70; }
done
if [[ "$preflight_only" == true ]]; then
 echo "Argo preflight passed for selected applications: ${applications[*]}; no targets changed."
 rm -rf "$snapshot"
 exit 0
fi
for application in "${applications[@]}"; do
 kubectl -n argocd patch application "$application" --type=merge -p \
  "{\"spec\":{\"source\":{\"targetRevision\":\"$revision\"},\"syncPolicy\":{\"automated\":{\"prune\":false,\"selfHeal\":true}}}}" >/dev/null
 kubectl -n argocd annotate application "$application" argocd.argoproj.io/refresh=hard --overwrite >/dev/null
 if ! wait_application "$application" "$revision" || ! python3 scripts/production/prepare-runtime-secrets.py --verify --component "$application"; then
  verified=false; break
 fi
done
# Grafana remains with its Helm owner and must be explicitly selected for changes.
if [[ "$verified" == true && "$coordinate_grafana" == true ]]; then
 grafana_changed=true
 ARGO_CREDENTIAL_ROLLBACK_OWNER=true DEPLOYMENT_SCOPE=selected DEPLOY_COMPONENTS=grafana scripts/production/deploy-k3s.sh || verified=false
fi
# The deployment caller runs full release verification after Helm releases and pilot reconciliation.
if [[ "$verified" == true ]]; then
 kubectl -n capitonic exec timescaledb-0 -c timescaledb -- psql -U postgres -d polymarket -Atc \
  "SELECT process_id FROM polymarket.trading_processes WHERE enabled AND status='running' AND heartbeat_at > now()-interval '90 seconds' ORDER BY process_id" > "$snapshot/enabled-after.txt"
 cmp "$snapshot/enabled-before.txt" "$snapshot/enabled-after.txt" || verified=false
fi
if [[ "$verified" != true ]]; then
 # Restore the exact previously admitted Git tuples, including secret versions.
 # Never rollback Helm while Argo is reconciling a different desired state.
 for application in "${applications[@]}"; do
  previous="$(jq -r --arg name "$application" '.items[]|select(.metadata.name==$name)|.spec.source.targetRevision' "$snapshot/applications-before.json")"
  [[ "$previous" =~ ^[0-9a-f]{40}$ ]] || { echo "No immutable rollback revision for $application; preserve $snapshot" >&2; exit 71; }
  kubectl -n argocd patch application "$application" --type=merge -p "{\"spec\":{\"source\":{\"targetRevision\":\"$previous\"}}}" >/dev/null
 done
 recovery=true
 for application in "${applications[@]}"; do
  previous="$(jq -r --arg name "$application" '.items[]|select(.metadata.name==$name)|.spec.source.targetRevision' "$snapshot/applications-before.json")"
  wait_application "$application" "$previous" && python3 scripts/production/prepare-runtime-secrets.py --verify --component "$application" --revision "$previous" || recovery=false
 done
 if [[ "$grafana_changed" == true ]]; then
  helm rollback grafana "$(cat "$snapshot/grafana-revision")" -n capitonic --wait --timeout 10m || recovery=false
 fi
 # Verify old image pins without changing the host checkout or database state.
 mkdir -p "$snapshot/rollback/capitonic-helm-chart/environments/production"
 cp capitonic-helm-chart/environments/production/*.yaml "$snapshot/rollback/capitonic-helm-chart/environments/production/"
 ln -s "$PWD/packages" "$snapshot/rollback/packages"
 for application in "${applications[@]}"; do
  previous="$(jq -r --arg name "$application" '.items[]|select(.metadata.name==$name)|.spec.source.targetRevision' "$snapshot/applications-before.json")"
  git show "$previous:capitonic-helm-chart/environments/production/$application.yaml" > "$snapshot/rollback/capitonic-helm-chart/environments/production/$application.yaml"
 done
 APP_DIRECTORY="$snapshot/rollback" scripts/production/verify-production.sh || recovery=false
 kubectl -n capitonic exec timescaledb-0 -c timescaledb -- psql -U postgres -d polymarket -Atc \
  "SELECT process_id FROM polymarket.trading_processes WHERE enabled AND status='running' AND heartbeat_at > now()-interval '90 seconds' ORDER BY process_id" > "$snapshot/enabled-recovered.txt"
 cmp "$snapshot/enabled-before.txt" "$snapshot/enabled-recovered.txt" || recovery=false
 echo "Argo release failed; rollback functional verification=$recovery; preserve $snapshot for RCA." >&2
 exit 71
fi
echo "Argo application release verified at $revision; snapshot $snapshot"
