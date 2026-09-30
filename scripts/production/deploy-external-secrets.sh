#!/usr/bin/env bash
set -euo pipefail
set +x
export KUBECONFIG="${KUBECONFIG:-/etc/rancher/k3s/k3s.yaml}"
cd "${APP_DIRECTORY:-/opt/polymarket-bot}"
scripts/production/install-yq.sh
release=infra/production/external-secrets-release.json
version="$(jq -er .version "$release")"
repository="$(jq -er .repository "$release")"
# Once adopted, the workflow asks Argo to reconcile instead of competing with it.
if kubectl -n argocd get application external-secrets >/dev/null 2>&1; then
  revision="$(git rev-parse HEAD)"
  patch="$(kubectl -n argocd get application external-secrets -o json | jq -c --arg version "$version" --arg revision "$revision" '{spec:{sources:(.spec.sources | .[0].targetRevision=$version | .[1].targetRevision=$revision)}}')"
  kubectl -n argocd patch application external-secrets --type=merge -p "$patch" >/dev/null
  kubectl -n argocd annotate application external-secrets argocd.argoproj.io/refresh=hard --overwrite >/dev/null
else
  helm upgrade --install external-secrets external-secrets --repo "$repository" --version "$version" \
    -n external-secrets --create-namespace -f capitonic-helm-chart/environments/production/external-secrets.yaml \
    --atomic --wait --timeout 10m
fi
kubectl wait --for=condition=Established crd/externalsecrets.external-secrets.io crd/secretstores.external-secrets.io --timeout=2m
kubectl -n external-secrets rollout status deployment/external-secrets --timeout=5m
kubectl -n external-secrets rollout status deployment/external-secrets-webhook --timeout=5m
kubectl -n external-secrets rollout status deployment/external-secrets-cert-controller --timeout=5m
