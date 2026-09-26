#!/usr/bin/env bash

set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/local-image-ci.sh <polymarket-bot|ingester|db-migrate> --checks-only
       scripts/local-image-ci.sh <polymarket-bot|ingester|db-migrate> --next-version
       scripts/local-image-ci.sh <polymarket-bot|ingester|db-migrate> --build

Run local checks on working-tree changes, or check a clean commit and build its production image.
--build selects the next unused local candidate version after a successful build.
--checks-only does not build or tag an image.
Images are not deployed, promoted, pushed, or marked golden by this script.
EOF
}

if [[ "${1:-}" == "--help" || "${1:-}" == "-h" ]]; then
  usage
  exit 0
fi
if (( $# != 2 )); then
  usage >&2
  exit 64
fi

component="$1"
mode="$2"
requested_version=""
case "$mode" in
  --checks-only|--next-version|--build) ;;
  *)
    if [[ "$mode" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)-local\.([1-9][0-9]*)$ ]]; then
      requested_version="$mode"
      mode=--build
    else
      usage >&2; exit 64
    fi
    ;;
esac

case "$component" in
  polymarket-bot)
    dockerfile="packages/polymarket-bot/Dockerfile.production"
    revision_arg="POLYMARKET_GIT_REVISION"
    base_version="3.2.1"
    ;;
  ingester)
    dockerfile="packages/market-data-ingester/Dockerfile.production"
    revision_arg="INGESTER_GIT_REVISION"
    base_version="1.2.1"
    ;;
  db-migrate)
    dockerfile="packages/db-migrate/Dockerfile.production"
    revision_arg="DB_MIGRATE_GIT_REVISION"
    base_version="0.2.0"
    ;;
  *)
    usage >&2
    exit 64
    ;;
esac

repository_root="$(git rev-parse --show-toplevel 2>/dev/null)" || {
  echo "Run this command from within the polymarket-bot Git repository." >&2
  exit 64
}
cd "$repository_root"

next_local_version() {
  local highest=0 tag annotation numbered
  while IFS= read -r tag; do
    numbered="${tag#image/$component/v$base_version-local.}"
    if [[ "$numbered" =~ ^[1-9][0-9]*$ ]] && (( numbered > highest )); then
      highest="$numbered"
    fi
  done < <(git tag --list "image/$component/v$base_version-local.*")
  while IFS= read -r tag; do
    annotation="$(git cat-file -p "refs/tags/$tag")"
    if [[ "$annotation" =~ version:\ v${base_version//./\.}-local\.([1-9][0-9]*) ]]; then
      numbered="${BASH_REMATCH[1]}"
      if (( numbered > highest )); then
        highest="$numbered"
      fi
    fi
  done < <(git tag --list "image/$component/sha256-*")
  printf '%s-local.%s\n' "$base_version" "$((highest + 1))"
}

if [[ "$mode" == "--next-version" ]]; then
  next_local_version
  exit 0
fi

require_clean_source() {
  if [[ -n "$(git status --porcelain --untracked-files=all)" ]]; then
    echo "Commit or remove working-tree changes before running local image CI." >&2
    exit 65
  fi
}

if [[ "$mode" == "--build" ]]; then
  require_clean_source
  git_revision="$(git rev-parse --verify 'HEAD^{commit}')"
  source_branch="$(git symbolic-ref --quiet --short HEAD)" || {
    echo "Build from a named source branch so image provenance is unambiguous." >&2
    exit 66
  }
  if [[ ! "$git_revision" =~ ^([0-9a-f]{40}|[0-9a-f]{64})$ ]]; then
    echo "Git returned an invalid full commit ID: $git_revision" >&2
    exit 66
  fi
fi

echo "Checking $component locally"
case "$component" in
  polymarket-bot)
    cargo +1.92.0 test --locked --package polymarket-bot --all-targets
    ;;
  ingester)
    cargo +1.92.0 fmt --all -- --check
    cargo +1.92.0 clippy --locked --package market-data-ingester --all-targets --all-features -- -D warnings -A clippy::too_many_arguments
    cargo +1.92.0 test --locked --package market-data-ingester --all-targets --all-features
    cargo +1.92.0 doc --locked --package market-data-ingester --no-deps --all-features
    ;;
  db-migrate)
    npm ci --prefix packages/db-migrate
    npm run build --prefix packages/db-migrate
    node --test packages/db-migrate/tests/*.test.js
    ;;
esac

if [[ "$mode" == "--checks-only" ]]; then
  echo "Checks passed for $component; no image was built."
  exit 0
fi
if [[ "$(git rev-parse HEAD)" != "$git_revision" ]]; then
  echo "HEAD changed during checks; refusing to build." >&2
  exit 68
fi
require_clean_source
version="$(next_local_version)"
if [[ -n "$requested_version" && "$requested_version" != "$version" ]]; then
  echo "Requested v$requested_version is not the next available candidate (v$version)." >&2
  exit 69
fi

if command -v nerdctl >/dev/null 2>&1 && nerdctl --namespace k8s.io info >/dev/null 2>&1; then
  image_cli=(nerdctl --namespace k8s.io)
elif command -v docker >/dev/null 2>&1 && docker info >/dev/null 2>&1; then
  image_cli=(docker)
else
  echo "No local container image builder is running (Rancher Desktop containerd or Docker)." >&2
  exit 69
fi

image="capitonic/$component:v$version"
if "${image_cli[@]}" image inspect "$image" >/dev/null 2>&1; then
  echo "Image tag $image already exists locally; select or reuse its existing immutable image." >&2
  exit 69
fi

temporary_image="capitonic/$component:build-${git_revision:0:12}-$$"
cleanup_temporary_image() {
  "${image_cli[@]}" image rm "$temporary_image" >/dev/null 2>&1 || true
}
trap cleanup_temporary_image EXIT
"${image_cli[@]}" build \
  --file "$dockerfile" \
  --build-arg "$revision_arg=$git_revision" \
  --label "org.opencontainers.image.version=$version" \
  --tag "$temporary_image" \
  .

image_id="$("${image_cli[@]}" image inspect --format '{{ .Id }}' "$temporary_image")"
image_revision="$("${image_cli[@]}" image inspect --format '{{ index .Config.Labels "org.opencontainers.image.revision" }}' "$temporary_image")"
image_version="$("${image_cli[@]}" image inspect --format '{{ index .Config.Labels "org.opencontainers.image.version" }}' "$temporary_image")"
if [[ ! "$image_id" =~ ^sha256:[0-9a-f]{64}$ \
  || "$image_revision" != "$git_revision" \
  || "$image_version" != "$version" ]]; then
  echo "Built image identity or provenance labels do not match the selected source." >&2
  exit 67
fi
if [[ "$(git rev-parse HEAD)" != "$git_revision" ]]; then
  echo "HEAD changed during the build; do not deploy this image." >&2
  exit 68
fi
require_clean_source

provenance_tag="image/$component/v$version"
if [[ "$(next_local_version)" != "$version" ]] || git show-ref --verify --quiet "refs/tags/$provenance_tag" || "${image_cli[@]}" image inspect "$image" >/dev/null 2>&1; then
  echo "Candidate version v$version was allocated elsewhere during the build; refusing to reuse it." >&2
  exit 69
fi
"${image_cli[@]}" image tag "$temporary_image" "$image"
if ! git tag -a "$provenance_tag" "$git_revision" \
  -m "component: $component" \
  -m "version: v$version" \
  -m "image: $image" \
  -m "image_id: $image_id" \
  -m "source_branch: $source_branch" \
  -m "source_revision: $git_revision" \
  -m "checks: local formatting, lint, and component tests passed" \
  -m "deployment: not deployed; golden status: not assigned"; then
  "${image_cli[@]}" image rm "$image" >/dev/null 2>&1 || true
  exit 69
fi

printf 'Candidate image: %s\nImage ID: %s\nGit revision: %s\nProvenance tag: %s\n' \
  "$image" "$image_id" "$git_revision" "$provenance_tag"
