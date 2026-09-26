#!/usr/bin/env bash

set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/local-image-ci.sh <polymarket-bot|ingester|db-migrate> --checks-only
       scripts/local-image-ci.sh <polymarket-bot|ingester|db-migrate> <MAJOR.MINOR.PATCH-local.N>

Run local checks on working-tree changes, or check a clean commit and build its production image.
--checks-only does not build or tag an image.
Images are not deployed, promoted, pushed, or marked golden by this script.
EOF
}

if [[ "${1:-}" == "--help" || "${1:-}" == "-h" ]]; then
  usage
  exit 0
fi
if (( $# < 2 || $# > 3 )) || { (( $# == 3 )) && [[ "$3" != "--checks-only" ]]; }; then
  usage >&2
  exit 64
fi

component="$1"
version=""
checks_only=""
if [[ "$2" == "--checks-only" ]]; then
  checks_only="--checks-only"
else
  version="$2"
  checks_only="${3:-}"
  if [[ -z "$checks_only" && ! "$version" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)-local\.([1-9][0-9]*)$ ]]; then
    echo "Use a local candidate version such as 1.2.1-local.1." >&2
    exit 64
  fi
fi

case "$component" in
  polymarket-bot)
    dockerfile="packages/polymarket-bot/Dockerfile.production"
    revision_arg="POLYMARKET_GIT_REVISION"
    ;;
  ingester)
    dockerfile="packages/market-data-ingester/Dockerfile.production"
    revision_arg="INGESTER_GIT_REVISION"
    ;;
  db-migrate)
    dockerfile="packages/db-migrate/Dockerfile.production"
    revision_arg="DB_MIGRATE_GIT_REVISION"
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

require_clean_source() {
  if [[ -n "$(git status --porcelain --untracked-files=all)" ]]; then
    echo "Commit or remove working-tree changes before running local image CI." >&2
    exit 65
  fi
}

if [[ "$checks_only" != "--checks-only" ]]; then
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

if [[ "$checks_only" == "--checks-only" ]]; then
  echo "Checks passed for $component; no image was built."
  exit 0
fi
if [[ "$(git rev-parse HEAD)" != "$git_revision" ]]; then
  echo "HEAD changed during checks; refusing to build." >&2
  exit 68
fi
require_clean_source

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

# An image/<component>/sha256-* tag is the repository's existing provenance record.
# Reserve each semantic version once across locally known image provenance tags.
while IFS= read -r existing_tag; do
  if git cat-file -p "refs/tags/$existing_tag" | grep -Fqx "version: v$version"; then
    echo "Version v$version is already recorded for $component in $existing_tag." >&2
    exit 69
  fi
done < <(git tag --list "image/$component/sha256-*")

"${image_cli[@]}" build \
  --file "$dockerfile" \
  --build-arg "$revision_arg=$git_revision" \
  --label "org.opencontainers.image.version=$version" \
  --tag "$image" \
  .

image_id="$("${image_cli[@]}" image inspect --format '{{ .Id }}' "$image")"
image_revision="$("${image_cli[@]}" image inspect --format '{{ index .Config.Labels "org.opencontainers.image.revision" }}' "$image")"
image_version="$("${image_cli[@]}" image inspect --format '{{ index .Config.Labels "org.opencontainers.image.version" }}' "$image")"
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

provenance_tag="image/$component/sha256-${image_id#sha256:}"
if git show-ref --verify --quiet "refs/tags/$provenance_tag"; then
  echo "Image provenance tag $provenance_tag already exists; refusing to move it." >&2
  exit 69
fi
git tag -a "$provenance_tag" "$git_revision" \
  -m "component: $component" \
  -m "version: v$version" \
  -m "image: $image" \
  -m "image_id: $image_id" \
  -m "source_branch: $source_branch" \
  -m "source_revision: $git_revision" \
  -m "checks: local formatting, lint, and component tests passed" \
  -m "deployment: not deployed; golden status: not assigned"

printf 'Candidate image: %s\nImage ID: %s\nGit revision: %s\nProvenance tag: %s\n' \
  "$image" "$image_id" "$git_revision" "$provenance_tag"
