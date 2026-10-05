#!/usr/bin/env bash
set -euo pipefail
set +x

if (( $# != 3 )); then
  echo "Usage: $0 <repository> <image-tag> <source-revision>" >&2
  exit 64
fi

repository="$1"
image_tag="$2"
source_revision="$3"
AWS_REGION="${AWS_REGION:-eu-west-1}"

[[ "$AWS_REGION" == "eu-west-1" ]]
[[ "$repository" =~ ^capitonic/(polymarket-bot|ingester|db-migrate)$ ]]
[[ "$image_tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+(-(rc|test)\.[0-9]+)?$ ]]
[[ "$source_revision" =~ ^[0-9a-f]{40}$ ]]

accepted_media_types=(
  application/vnd.oci.image.index.v1+json
  application/vnd.docker.distribution.manifest.list.v2+json
  application/vnd.oci.image.manifest.v1+json
  application/vnd.docker.distribution.manifest.v2+json
)

root_digest="$(aws ecr describe-images --region "$AWS_REGION" --repository-name "$repository" \
  --image-ids imageTag="$image_tag" --query 'imageDetails[0].imageDigest' --output text)"
[[ "$root_digest" =~ ^sha256:[0-9a-f]{64}$ ]]

root_manifest="$(aws ecr batch-get-image --region "$AWS_REGION" --repository-name "$repository" \
  --image-ids imageDigest="$root_digest" --accepted-media-types "${accepted_media_types[@]}" \
  --query 'images[0].imageManifest' --output text)"
root_media_type="$(jq -er '.mediaType' <<<"$root_manifest")"

case "$root_media_type" in
  application/vnd.oci.image.index.v1+json|application/vnd.docker.distribution.manifest.list.v2+json)
    image_digest="$(jq -er '
      [.manifests[] | select(.platform.os == "linux" and .platform.architecture == "arm64") | .digest]
      | if length == 1 then .[0] else error("expected exactly one linux/arm64 image") end
    ' <<<"$root_manifest")"
    image_manifest="$(aws ecr batch-get-image --region "$AWS_REGION" --repository-name "$repository" \
      --image-ids imageDigest="$image_digest" \
      --accepted-media-types application/vnd.oci.image.manifest.v1+json application/vnd.docker.distribution.manifest.v2+json \
      --query 'images[0].imageManifest' --output text)"
    ;;
  application/vnd.oci.image.manifest.v1+json|application/vnd.docker.distribution.manifest.v2+json)
    image_manifest="$root_manifest"
    ;;
  *)
    echo "Unsupported ECR manifest media type for $repository:$image_tag: $root_media_type" >&2
    exit 65
    ;;
esac

config_digest="$(jq -er '.config.digest' <<<"$image_manifest")"
config_url="$(aws ecr get-download-url-for-layer --region "$AWS_REGION" --repository-name "$repository" \
  --layer-digest "$config_digest" --query downloadUrl --output text)"
curl -fsSL "$config_url" | jq -e --arg revision "$source_revision" '
  .os == "linux" and
  .architecture == "arm64" and
  .config.Labels["org.opencontainers.image.revision"] == $revision
' >/dev/null

printf '%s\n' "$root_digest"
