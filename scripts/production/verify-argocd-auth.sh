#!/usr/bin/env bash
set -euo pipefail
set +x

AWS_REGION="${AWS_REGION:-eu-west-1}"
APP_SECRET_NAME="${APP_SECRET_NAME:-capitonic/polymarket-bot/production}"
export KUBECONFIG="${KUBECONFIG:-/etc/rancher/k3s/k3s.yaml}"

kubectl -n argocd get configmap argocd-cm -o json | jq -e '
  .data["admin.enabled"] == "true" and
  .data["users.anonymous.enabled"] == "false"' >/dev/null

temporary_directory="$(mktemp -d /run/capitonic-argocd-auth.XXXXXX)"
chmod 0700 "$temporary_directory"
trap 'rm -rf "$temporary_directory"' EXIT
aws secretsmanager get-secret-value --region "$AWS_REGION" --secret-id "$APP_SECRET_NAME" \
  --query SecretString --output text > "$temporary_directory/production-secret.json"
chmod 0600 "$temporary_directory/production-secret.json"
jq -n --arg password "$(jq -jer '.ARGOCD_ADMIN_PASSWORD' "$temporary_directory/production-secret.json")" \
  '{username:"admin",password:$password}' > "$temporary_directory/admin-session.json"
chmod 0600 "$temporary_directory/admin-session.json"

login_status="$(curl -sS -H 'Host: ops.capitonic.com' -H 'Content-Type: application/json' \
  --data-binary @"$temporary_directory/admin-session.json" -o /dev/null -w '%{http_code}' \
  http://127.0.0.1/api/v1/session)"
[[ "$login_status" == 200 ]]
unauthenticated_status="$(curl -sS -H 'Host: ops.capitonic.com' -o /dev/null -w '%{http_code}' \
  http://127.0.0.1/api/v1/applications)"
[[ "$unauthenticated_status" == 401 || "$unauthenticated_status" == 403 ]]
echo 'Argo CD admin login works and anonymous API access is denied.'
