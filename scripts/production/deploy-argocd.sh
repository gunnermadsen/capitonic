#!/usr/bin/env bash
set -euo pipefail
set +x

APP_DIRECTORY="${APP_DIRECTORY:-/opt/polymarket-bot}"
APP_SECRET_NAME="${APP_SECRET_NAME:-capitonic/polymarket-bot/production}"
AWS_REGION="${AWS_REGION:-eu-west-1}"
ARGO_CHART_VERSION=10.9.2
export KUBECONFIG="${KUBECONFIG:-/etc/rancher/k3s/k3s.yaml}"
cd "$APP_DIRECTORY"

[[ "$(git rev-parse HEAD)" =~ ^[0-9a-f]{40}$ ]]
[[ -z "$(git status --porcelain --untracked-files=no)" ]]

scripts/production/install-yq.sh
snapshot="$(mktemp -d /run/capitonic-argocd.XXXXXX)"
chmod 0700 "$snapshot"
trap 'rm -rf "$snapshot"' EXIT
kubectl -n capitonic get deployments,statefulsets,daemonsets,jobs,ingresses -o json |
  jq -S '[.items[] | {kind, name: .metadata.name, spec}] | sort_by(.kind,.name)' > "$snapshot/workloads-before.json"

aws secretsmanager get-secret-value --region "$AWS_REGION" --secret-id "$APP_SECRET_NAME" \
  --query SecretString --output text > "$snapshot/production-secret.json"
chmod 0600 "$snapshot/production-secret.json"
jq -jer '.GITHUB_PAT | select(type == "string" and length > 0)' \
  "$snapshot/production-secret.json" > "$snapshot/github-pat"
chmod 0600 "$snapshot/github-pat"
jq -jer '.ARGOCD_ADMIN_PASSWORD_HASH | select(type == "string" and test("^\\$2[aby]\\$[0-9]{2}\\$[./A-Za-z0-9]{53}$"))' \
  "$snapshot/production-secret.json" > "$snapshot/argocd-admin-hash"
chmod 0600 "$snapshot/argocd-admin-hash"
argocd_admin_password_mtime="$(jq -jer '.ARGOCD_ADMIN_PASSWORD_MTIME | select(type == "string" and test("^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$"))' "$snapshot/production-secret.json")"

helm upgrade --install argocd oci://ghcr.io/argoproj/argo-helm/argo-cd \
  --version "$ARGO_CHART_VERSION" \
  --namespace argocd --create-namespace \
  -f capitonic-helm-chart/environments/production/argocd.yaml \
  --set-file configs.secret.argocdServerAdminPassword="$snapshot/argocd-admin-hash" \
  --set-string configs.secret.argocdServerAdminPasswordMtime="$argocd_admin_password_mtime" \
  --atomic --wait --timeout 15m
kubectl wait --for=condition=Established crd/applications.argoproj.io crd/appprojects.argoproj.io --timeout=2m
kubectl -n argocd get secret argocd-secret -o json |
  jq -jr '.data["admin.password"] | @base64d' | cmp - "$snapshot/argocd-admin-hash"
kubectl -n argocd create secret generic capitonic-github \
  --from-literal=type=git \
  --from-literal=url=https://github.com/gunnermadsen/capitonic.git \
  --from-literal=username=x-access-token \
  --from-file=password="$snapshot/github-pat" \
  --dry-run=client -o yaml |
  kubectl label --local -f - argocd.argoproj.io/secret-type=repository -o yaml |
  kubectl apply -f - >/dev/null

for legacy_application in capitonic-production-bot capitonic-production-ingester; do
  if kubectl -n argocd get application "$legacy_application" >/dev/null 2>&1; then
    kubectl -n argocd get application "$legacy_application" -o json |
      jq -e '(.metadata.finalizers // [] | length) == 0' >/dev/null
    kubectl -n argocd delete application "$legacy_application" --wait=true >/dev/null
  fi
done
# Pin admitted application state before enabling sync; raw production pushes must not
# deploy old image pins before CD has selected secrets and promoted the new digest.
DEPLOY_REVISION="$(git rev-parse HEAD)" yq '
  (select(.kind == "Application" and (.metadata.name == "ingester" or .metadata.name == "polymarket-bot"))).spec.source.targetRevision = strenv(DEPLOY_REVISION) |
  (select(.kind == "Application" and .metadata.name == "external-secrets")).spec.sources[1].targetRevision = strenv(DEPLOY_REVISION)
' infra/production-k3s/argocd-observation.yaml > "$snapshot/applications.yaml"
# Preserve already admitted application targets while upgrading Argo itself.
for application in ingester polymarket-bot; do
  previous="$(kubectl -n argocd get application "$application" -o json 2>/dev/null | jq -r 'if .spec.syncPolicy.automated.selfHeal == true then .spec.source.targetRevision else empty end' || true)"
  if [[ "$previous" =~ ^[0-9a-f]{40}$ ]]; then
    APPLICATION="$application" PREVIOUS="$previous" yq -i '(select(.kind == "Application" and .metadata.name == strenv(APPLICATION))).spec.source.targetRevision = strenv(PREVIOUS)' "$snapshot/applications.yaml"
  fi
done
kubectl apply -f "$snapshot/applications.yaml" >/dev/null
kubectl -n argocd rollout status deployment/argocd-server --timeout=5m
kubectl -n argocd rollout status deployment/argocd-repo-server --timeout=5m
kubectl -n argocd get configmap argocd-cm -o json | jq -e '
  .data["admin.enabled"] == "true" and
  .data["users.anonymous.enabled"] == "false" and
  .data["application.resourceTrackingMethod"] == "label" and
  .data["application.instanceLabelKey"] == "app.kubernetes.io/instance"' >/dev/null
kubectl -n argocd get configmap argocd-rbac-cm -o json | jq -e '
  .data["policy.default"] == "role:readonly"' >/dev/null
scripts/production/verify-argocd-auth.sh
kubectl -n argocd get ingress argocd-server -o json | jq -e '
  .spec.ingressClassName == "traefik" and
  .metadata.annotations["traefik.ingress.kubernetes.io/router.entrypoints"] == "web" and
  (.spec.tls // [] | length) == 0 and
  any(.spec.rules[]; .host == "ops.capitonic.com" and
    any(.http.paths[]; .path == "/"))' >/dev/null
ops_status="$(curl -sS -H 'Host: ops.capitonic.com' -o /dev/null -w '%{http_code}' http://127.0.0.1/)"
[[ "$ops_status" == 200 || "$ops_status" == 302 ]]
direct_status="$(curl -ksS --resolve ops.capitonic.com:443:127.0.0.1 -o /dev/null -w '%{http_code}' https://ops.capitonic.com/)"
[[ "$direct_status" == 404 ]]
kubectl -n argocd get applications.argoproj.io -o json | jq -e '
  [.items[] | select(.metadata.name == "polymarket-bot" or
                     .metadata.name == "ingester")] as $apps |
  ($apps | length) == 2 and all($apps[];
    (.spec.source.targetRevision | test("^[0-9a-f]{40}$")) and
    (.spec.syncPolicy.automated.selfHeal == true and .spec.syncPolicy.automated.prune == false) and
    .spec.destination.namespace == "capitonic")' >/dev/null
for attempt in $(seq 1 30); do
  if kubectl -n argocd get applications.argoproj.io -o json | jq -e '
    [.items[] | select(.metadata.name == "polymarket-bot" or
                       .metadata.name == "ingester")] as $apps |
    ($apps | length) == 2 and all($apps[];
      (.status.sync.revision // "" | test("^[0-9a-f]{40}$")) and
      (.status.sync.status == "Synced" or .status.sync.status == "OutOfSync") and
      ([.status.conditions[]? | select(.type == "ComparisonError")] | length) == 0)
  ' >/dev/null; then
    break
  fi
  if (( attempt == 30 )); then
    echo 'Argo CD did not render both production charts from Git.' >&2
    exit 1
  fi
  sleep 5
done

kubectl -n capitonic get deployments,statefulsets,daemonsets,jobs,ingresses -o json |
  jq -S '[.items[] | {kind, name: .metadata.name, spec}] | sort_by(.kind,.name)' > "$snapshot/workloads-after.json"
# Application Deployments are verified through the existing production verifier.
# Infrastructure must retain its complete spec during Argo configuration updates.
jq '[.[] | select(.name != "polymarket-bot" and .name != "ingester-master" and .name != "ingester-worker")]' "$snapshot/workloads-before.json" > "$snapshot/infrastructure-before.json"
jq '[.[] | select(.name != "polymarket-bot" and .name != "ingester-master" and .name != "ingester-worker")]' "$snapshot/workloads-after.json" > "$snapshot/infrastructure-after.json"
cmp "$snapshot/infrastructure-before.json" "$snapshot/infrastructure-after.json"
printf 'Argo CD configured for reconciliation at production revision %s.\n' "$(git rev-parse HEAD)"
