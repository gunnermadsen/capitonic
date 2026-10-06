#!/usr/bin/env bash
set -euo pipefail
set +x

check_only=false
case "${1:-}" in
  --check) check_only=true ;;
  '') ;;
  *) echo "Usage: $0 [--check]" >&2; exit 64 ;;
esac

AWS_REGION="${AWS_REGION:-eu-west-1}"
ECR_REGISTRY="${ECR_REGISTRY:-192200846560.dkr.ecr.eu-west-1.amazonaws.com}"
RELEASE_FILE="${RELEASE_FILE:-infra/production/release.json}"
[[ "$AWS_REGION" == "eu-west-1" ]]
jq -e '.schemaVersion == 1 and .region == "eu-west-1" and .architecture == "arm64"' "$RELEASE_FILE" >/dev/null

case "${DEPLOYMENT_SCOPE:-full-stack}" in
  selected)
    [[ -n "${PROMOTE_COMPONENTS:-}" ]] || exit 0
    IFS=, read -ra components <<< "$PROMOTE_COMPONENTS"
    ;;
  full-stack) components=(polymarket-bot ingester db-migrate) ;;
  *) echo "Unsupported deployment scope: $DEPLOYMENT_SCOPE" >&2; exit 64 ;;
esac

for component in "${components[@]}"; do
  case "$component" in
    polymarket-bot|ingester|db-migrate) ;;
    *) echo "Unsupported image component: $component" >&2; exit 64 ;;
  esac
  repository="$(jq -r --arg component "$component" '.components[$component].repository' "$RELEASE_FILE")"
  rc_version="$(jq -r --arg component "$component" '.components[$component].rcVersion' "$RELEASE_FILE")"
  source_revision="$(jq -r --arg component "$component" '.components[$component].sourceRevision' "$RELEASE_FILE")"
  [[ "$repository" == "capitonic/$component" ]]
  [[ "$rc_version" =~ ^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)-rc\.(0|[1-9][0-9]*)$ ]]
  [[ "$source_revision" =~ ^[0-9a-f]{40}$ ]]
  final_version="${rc_version%-rc.*}"
  [[ "$final_version" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]

  values="capitonic-helm-chart/environments/production/$component.yaml"
  expected_digest="$(jq -r --arg component "$component" '.components[$component].digest' "$RELEASE_FILE")"
  [[ "$(jq -r --arg component "$component" '.components[$component].finalVersion' "$RELEASE_FILE")" == "$final_version" ]]
  pinned_version="$(yq -r '.release.version' "$values")"
  [[ "$pinned_version" == "$rc_version" || "$pinned_version" == "$final_version" ]]
  [[ "$(yq -r '.environment' "$values")" == production ]]
  [[ "$(yq -r '.image' "$values")" == "$ECR_REGISTRY/$repository:$pinned_version" ]]
  [[ "$(yq -r '.release.sourceRevision' "$values")" == "$source_revision" ]]
  [[ "$(yq -r '.release.digest' "$values")" == "$expected_digest" ]]
  digest="$(scripts/production/verify-ecr-arm64-image.sh "$repository" "$rc_version" "$source_revision")"
  [[ "$digest" =~ ^sha256:[0-9a-f]{64}$ && "$digest" == "$expected_digest" ]]
  manifest="$(aws ecr batch-get-image --region "$AWS_REGION" --repository-name "$repository" \
    --image-ids imageDigest="$digest" \
    --accepted-media-types application/vnd.oci.image.index.v1+json application/vnd.docker.distribution.manifest.list.v2+json application/vnd.oci.image.manifest.v1+json application/vnd.docker.distribution.manifest.v2+json \
    --query 'images[0].imageManifest' --output text)"

  lookup_error="$(mktemp)"
  if existing_final="$(aws ecr describe-images --region "$AWS_REGION" --repository-name "$repository" \
    --image-ids imageTag="$final_version" --query 'imageDetails[0].imageDigest' --output text 2>"$lookup_error")"; then
    rm -f "$lookup_error"
    [[ "$existing_final" =~ ^sha256:[0-9a-f]{64}$ ]]
  else
    if ! grep -q '(ImageNotFoundException)' "$lookup_error"; then
      cat "$lookup_error" >&2
      rm -f "$lookup_error"
      exit 1
    fi
    rm -f "$lookup_error"
    existing_final=''
  fi
  if [[ -n "$existing_final" && "$existing_final" != None && "$existing_final" != "$digest" ]]; then
    echo "Final tag $repository:$final_version already names a different digest." >&2
    exit 1
  fi
  if [[ "$check_only" == true ]]; then
    printf 'Planned promotion: %s %s -> %s (%s); final tag %s; no registry writes.\n' \
      "$component" "$rc_version" "$final_version" "$digest" "${existing_final:-absent}"
    continue
  fi
  if [[ "$existing_final" != "$digest" ]]; then
    [[ "$manifest" == \{* ]]
    aws ecr put-image --region "$AWS_REGION" --repository-name "$repository" \
      --image-tag "$final_version" --image-manifest "$manifest" >/dev/null
  fi

  [[ "$(scripts/production/verify-ecr-arm64-image.sh "$repository" "$final_version" "$source_revision")" == "$digest" ]]
  printf '%s %s -> %s (%s)\n' "$component" "$rc_version" "$final_version" "$digest"
done
