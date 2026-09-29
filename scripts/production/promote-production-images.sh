#!/usr/bin/env bash
set -euo pipefail
set +x

AWS_REGION="${AWS_REGION:-eu-west-1}"
ECR_REGISTRY="${ECR_REGISTRY:-192200846560.dkr.ecr.eu-west-1.amazonaws.com}"
RELEASE_FILE="${RELEASE_FILE:-infra/production/release.json}"
[[ "$AWS_REGION" == "eu-west-1" ]]
jq -e '.schemaVersion == 1 and .region == "eu-west-1" and .architecture == "arm64"' "$RELEASE_FILE" >/dev/null

case "${DEPLOYMENT_SCOPE:-full-stack}" in
  bot-grafana) components=(polymarket-bot) ;;
  full-stack) components=(polymarket-bot ingester db-migrate) ;;
  *) echo "Unsupported deployment scope: $DEPLOYMENT_SCOPE" >&2; exit 64 ;;
esac

for component in "${components[@]}"; do
  repository="$(jq -r --arg component "$component" '.components[$component].repository' "$RELEASE_FILE")"
  rc_version="$(jq -r --arg component "$component" '.components[$component].rcVersion' "$RELEASE_FILE")"
  source_revision="$(jq -r --arg component "$component" '.components[$component].sourceRevision' "$RELEASE_FILE")"
  [[ "$repository" == "capitonic/$component" ]]
  [[ "$rc_version" =~ ^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)-rc\.(0|[1-9][0-9]*)$ ]]
  [[ "$source_revision" =~ ^[0-9a-f]{40}$ ]]
  final_version="${rc_version%-rc.*}"
  [[ "$final_version" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]

  digest="$(scripts/production/verify-ecr-arm64-image.sh "$repository" "$rc_version" "$source_revision")"
  [[ "$digest" =~ ^sha256:[0-9a-f]{64}$ ]]
  reference="$ECR_REGISTRY/$repository@$digest"
  manifest="$(aws ecr batch-get-image --region "$AWS_REGION" --repository-name "$repository" \
    --image-ids imageDigest="$digest" \
    --accepted-media-types application/vnd.oci.image.index.v1+json application/vnd.docker.distribution.manifest.list.v2+json application/vnd.oci.image.manifest.v1+json application/vnd.docker.distribution.manifest.v2+json \
    --query 'images[0].imageManifest' --output text)"

  existing_final="$(aws ecr describe-images --region "$AWS_REGION" --repository-name "$repository" \
    --image-ids imageTag="$final_version" --query 'imageDetails[0].imageDigest' --output text 2>/dev/null || true)"
  if [[ -n "$existing_final" && "$existing_final" != None && "$existing_final" != "$digest" ]]; then
    echo "Final tag $repository:$final_version already names a different digest." >&2
    exit 1
  fi
  if [[ "$existing_final" != "$digest" ]]; then
    [[ "$manifest" == \{* ]]
    aws ecr put-image --region "$AWS_REGION" --repository-name "$repository" \
      --image-tag "$final_version" --image-manifest "$manifest" >/dev/null
  fi

  overlay="capitonic-helm-chart/environments/production/$component.yaml"
  IMAGE="$reference" VERSION="$final_version" DIGEST="$digest" REVISION="$source_revision" \
    yq -i '
      .image = strenv(IMAGE) |
      .release.version = strenv(VERSION) |
      .release.digest = strenv(DIGEST) |
      .release.sourceRevision = strenv(REVISION)
    ' "$overlay"
  if [[ "$component" == "ingester" ]]; then
    DIGEST="$digest" REVISION="$source_revision" yq -i \
      '.imageDigest = strenv(DIGEST) | .gitRevision = strenv(REVISION)' "$overlay"
  fi
  VERSION="$final_version" yq -i '.appVersion = strenv(VERSION)' "capitonic-helm-chart/charts/$component/Chart.yaml"
  release_tmp="$(mktemp)"
  jq --arg component "$component" --arg version "$final_version" --arg digest "$digest" \
    '.components[$component].finalVersion = $version | .components[$component].digest = $digest' \
    "$RELEASE_FILE" >"$release_tmp"
  mv "$release_tmp" "$RELEASE_FILE"
  printf '%s %s -> %s (%s)\n' "$component" "$rc_version" "$final_version" "$digest"
done
