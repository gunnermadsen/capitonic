#!/usr/bin/env bash
set -euo pipefail
set +x

AWS_REGION="${AWS_REGION:-eu-west-1}"
ECR_REGISTRY="${ECR_REGISTRY:-192200846560.dkr.ecr.eu-west-1.amazonaws.com}"
NAMESPACE="${CAPITONIC_NAMESPACE:-capitonic}"

[[ "$AWS_REGION" == "eu-west-1" ]]
[[ "$ECR_REGISTRY" == *.dkr.ecr.eu-west-1.amazonaws.com ]]

kubectl create namespace "$NAMESPACE" --dry-run=client -o yaml | kubectl apply -f - >/dev/null
temporary_directory="$(mktemp -d /run/capitonic-ecr.XXXXXX)"
trap 'rm -rf "$temporary_directory"' EXIT
chmod 0700 "$temporary_directory"

password="$(aws ecr get-login-password --region "$AWS_REGION")"
auth="$(printf 'AWS:%s' "$password" | base64 -w0)"
jq -n --arg registry "$ECR_REGISTRY" --arg auth "$auth" \
  '{auths:{($registry):{auth:$auth,username:"AWS"}}}' >"$temporary_directory/config.json"
chmod 0600 "$temporary_directory/config.json"
unset password auth

kubectl -n "$NAMESPACE" create secret generic ecr-registry \
  --type=kubernetes.io/dockerconfigjson \
  --from-file=.dockerconfigjson="$temporary_directory/config.json" \
  --dry-run=client -o yaml | kubectl apply -f - >/dev/null

kubectl -n "$NAMESPACE" patch serviceaccount default --type=merge \
  -p '{"imagePullSecrets":[{"name":"ecr-registry"}]}' >/dev/null

echo "Refreshed the Ireland ECR pull secret in namespace $NAMESPACE."
