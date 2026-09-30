#!/usr/bin/env bash
set -euo pipefail
set +x
export KUBECONFIG="${KUBECONFIG:-/etc/rancher/k3s/k3s.yaml}"
cd "${APP_DIRECTORY:-/opt/polymarket-bot}"
revision="$(git rev-parse HEAD)"
snapshot="$(mktemp -d /run/capitonic-argo-release.XXXXXX)"
chmod 0700 "$snapshot"
kubectl -n argocd get application ingester polymarket-bot -o json > "$snapshot/applications-before.json"
kubectl -n capitonic get deployments -o json > "$snapshot/deployments-before.json"
kubectl -n capitonic exec timescaledb-0 -c timescaledb -- psql -U postgres -d polymarket -Atc \
 "SELECT process_id FROM polymarket.trading_processes WHERE enabled ORDER BY process_id" > "$snapshot/enabled-before.txt"
ready="$(kubectl -n capitonic get deployment ingester-worker -o jsonpath='{.status.readyReplicas}')"
desired="$(kubectl -n capitonic exec timescaledb-0 -c timescaledb -- psql -U postgres -d polymarket -Atc "SELECT count(*) FROM ingester.profiles WHERE desired_state='running'")"
backfills="$(kubectl -n capitonic exec timescaledb-0 -c timescaledb -- psql -U postgres -d polymarket -Atc "SELECT coalesce(sum(allocation_units),0) FROM ingester.backfill_jobs WHERE assigned_worker_id IS NOT NULL AND status IN ('running','cancel_requested')")"
(( ready >= desired + backfills )) || { echo 'Worker capacity is insufficient; no Argo target changed.' >&2; exit 70; }
echo "Argo release $revision; rollback snapshot $snapshot; workers=$ready profiles=$desired backfills=$backfills"
verified=true
# ingester-auth is also consumed by the bot; synchronize ingester before admitting
# the bot target so cross-application credentials cannot race the bot rollout.
for application in ingester polymarket-bot; do
 kubectl -n argocd patch application "$application" --type=merge -p \
  "{\"spec\":{\"source\":{\"targetRevision\":\"$revision\"},\"syncPolicy\":{\"automated\":{\"prune\":false,\"selfHeal\":true}}}}" >/dev/null
 kubectl -n argocd annotate application "$application" argocd.argoproj.io/refresh=hard --overwrite >/dev/null
 healthy=false
 for ((attempt=0; attempt<120; attempt++)); do
  if kubectl -n argocd get application "$application" -o json | jq -e --arg revision "$revision" \
   '.status.sync.revision == $revision and .status.sync.status == "Synced" and .status.health.status == "Healthy" and (.status.operationState.phase // "Succeeded") != "Running"' >/dev/null; then
   healthy=true; break
  fi
  sleep 5
 done
 if [[ "$healthy" != true ]] || ! python3 scripts/production/prepare-runtime-secrets.py --verify --component "$application"; then
  verified=false; break
 fi
done
if [[ "$verified" == true ]]; then
 python3 scripts/production/prepare-runtime-secrets.py --verify || verified=false
fi
if [[ "$verified" == true ]]; then
 if ! scripts/production/verify-production.sh; then
   echo "Full verification failed; investigate attribution before rollback. Preserve $snapshot." >&2
   exit 72
 fi
fi
if [[ "$verified" == true ]]; then
 kubectl -n capitonic exec timescaledb-0 -c timescaledb -- psql -U postgres -d polymarket -Atc \
  "SELECT process_id FROM polymarket.trading_processes WHERE enabled AND status='running' AND heartbeat_at > now()-interval '90 seconds' ORDER BY process_id" > "$snapshot/enabled-after.txt"
 cmp "$snapshot/enabled-before.txt" "$snapshot/enabled-after.txt" || verified=false
fi
if [[ "$verified" != true ]]; then
 # Restore the exact previously admitted Git tuples, including secret versions.
 # Never rollback Helm while Argo is reconciling a different desired state.
 for application in ingester polymarket-bot; do
  previous="$(jq -r --arg name "$application" '.items[]|select(.metadata.name==$name)|.spec.source.targetRevision' "$snapshot/applications-before.json")"
  [[ "$previous" =~ ^[0-9a-f]{40}$ ]] || { echo "No immutable rollback revision for $application; preserve $snapshot" >&2; exit 71; }
  kubectl -n argocd patch application "$application" --type=merge -p "{\"spec\":{\"source\":{\"targetRevision\":\"$previous\"}}}" >/dev/null
 done
 echo "Argo release failed; previous immutable targets restored; preserve $snapshot for recovery verification and RCA." >&2
 exit 71
fi
echo "Argo application release verified at $revision; snapshot $snapshot"
