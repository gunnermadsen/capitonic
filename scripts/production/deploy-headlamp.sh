#!/usr/bin/env bash
set -euo pipefail
set +x
export KUBECONFIG="${KUBECONFIG:-/etc/rancher/k3s/k3s.yaml}"
cd "${APP_DIRECTORY:-/opt/polymarket-bot}"
scripts/production/install-yq.sh
[[ -z "$(git status --porcelain --untracked-files=no)" ]]
kubectl -n external-secrets rollout status deployment/external-secrets --timeout=2m
kubectl wait --for=condition=Established crd/externalsecrets.external-secrets.io --timeout=2m
kubectl wait --for=condition=Ready clusterissuer/letsencrypt-production --timeout=2m

configuration="$(python3 scripts/production/prepare-runtime-secrets.py --component headlamp)"
committed="$(yq -o=json '.externalSecrets' capitonic-helm-chart/environments/production/headlamp.yaml | jq -Sc .)"
selected="$(jq -Sc '.headlamp.externalSecrets' <<< "$configuration")"
[[ "$committed" == "$selected" ]] || {
  echo 'Headlamp credentials changed: commit the selected externalSecrets version/refreshAfter before deployment.' >&2
  printf '%s\n' "$configuration"
  exit 64
}

snapshot="$(mktemp -d /run/capitonic-headlamp.XXXXXX)"
chmod 0700 "$snapshot"
kubectl -n capitonic get deployments,statefulsets,daemonsets -o json |
  jq -S '[.items[] | select(.metadata.name != "headlamp") | {kind, name: .metadata.name, spec}] | sort_by(.kind,.name)' > "$snapshot/workloads-before.json"
previous="$(helm history headlamp -n capitonic -o json 2>/dev/null | jq -r '[.[] | select(.status == "deployed")] | last | .revision // empty' || true)"
recover() {
  local result=$?
  trap - EXIT
  if (( result != 0 )); then
    if [[ -n "$previous" ]]; then
      helm rollback headlamp "$previous" -n capitonic --wait --timeout 5m
    elif helm status headlamp -n capitonic >/dev/null 2>&1; then
      helm uninstall headlamp -n capitonic --wait --timeout 5m
    fi
    echo "Headlamp deployment failed; rollback target=${previous:-new release removed}; evidence=$snapshot" >&2
  else
    rm -rf "$snapshot"
  fi
  exit "$result"
}
trap recover EXIT
helm repo add headlamp https://kubernetes-sigs.github.io/headlamp/ --force-update
helm dependency build capitonic-helm-chart/charts/headlamp
helm upgrade --install headlamp capitonic-helm-chart/charts/headlamp -n capitonic \
  -f capitonic-helm-chart/environments/production/headlamp.yaml --wait --timeout 5m
kubectl -n capitonic wait --for=condition=Ready secretstore/headlamp-aws --timeout=2m
kubectl -n capitonic wait --for=condition=Ready externalsecret/headlamp-basic-auth --timeout=2m
kubectl -n capitonic rollout status deployment/headlamp --timeout=2m
tls_secret="$(yq -r '.upstream.ingress.tls[0].secretName' capitonic-helm-chart/environments/production/headlamp.yaml)"
kubectl -n capitonic wait --for=condition=Ready "certificate/$tls_secret" --timeout=5m
python3 scripts/production/prepare-runtime-secrets.py --verify --component headlamp
kubectl -n capitonic get deployments,statefulsets,daemonsets -o json |
  jq -S '[.items[] | select(.metadata.name != "headlamp") | {kind, name: .metadata.name, spec}] | sort_by(.kind,.name)' > "$snapshot/workloads-after.json"
cmp "$snapshot/workloads-before.json" "$snapshot/workloads-after.json"
echo "Headlamp production release verified at $(git rev-parse HEAD); existing workloads unchanged."
