#!/usr/bin/env bash

set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/local-image-ci.sh <polymarket-bot|ingester|db-migrate> --checks-only
       scripts/local-image-ci.sh <polymarket-bot|ingester|db-migrate> --next-version
       scripts/local-image-ci.sh <polymarket-bot|ingester|db-migrate> --build

Run local checks on working-tree changes, or check a clean commit and build its production image.
--build selects the next unused local candidate version only for a distinct built image.
Each new image receives one version tag and one image-ID provenance tag on its source commit.
Unchanged committed image inputs reuse the existing image and Git tag pair.
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
    checks_description="local formatting, Clippy, and component tests passed"
    image_inputs=(.dockerignore Cargo.toml Cargo.lock packages/polymarket-bot/Cargo.toml packages/market-data-ingester/Cargo.toml packages/polymarket-bot/build.rs common/proto packages/polymarket-bot/src packages/btc-directional-model/runtime-models "$dockerfile")
    ;;
  ingester)
    dockerfile="packages/market-data-ingester/Dockerfile.production"
    revision_arg="INGESTER_GIT_REVISION"
    base_version="1.2.1"
    checks_description="local formatting, Clippy, component tests, and docs passed"
    image_inputs=(.dockerignore Cargo.toml Cargo.lock packages/polymarket-bot/Cargo.toml packages/market-data-ingester/Cargo.toml packages/market-data-ingester/build.rs common/proto packages/market-data-ingester/src "$dockerfile")
    ;;
  db-migrate)
    dockerfile="packages/db-migrate/Dockerfile.production"
    revision_arg="DB_MIGRATE_GIT_REVISION"
    base_version="0.2.0"
    checks_description="local TypeScript build and component tests passed"
    image_inputs=(.dockerignore packages/db-migrate/package.json packages/db-migrate/package-lock.json packages/db-migrate/tsconfig.json packages/db-migrate/src "$dockerfile")
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

image_inputs_sha256() {
  git ls-tree -r --full-tree "$1" -- "${image_inputs[@]}" | LC_ALL=C shasum -a 256 | awk '{print $1}'
}

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
  for tool in shasum; do
    command -v "$tool" >/dev/null 2>&1 || { echo "Missing $tool" >&2; exit 69; }
  done
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
if [[ -f scripts/tests/test_local_image_ci.py ]]; then
  PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s scripts/tests -p 'test_local_image_ci.py'
fi
case "$component" in
  polymarket-bot)
    cargo +1.92.0 fmt --all -- --check
    cargo +1.92.0 clippy --locked --package polymarket-bot --all-targets -- -D warnings
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
if command -v nerdctl >/dev/null 2>&1 && nerdctl --namespace k8s.io info >/dev/null 2>&1; then
  image_cli=(nerdctl --namespace k8s.io)
elif command -v docker >/dev/null 2>&1 && docker info >/dev/null 2>&1; then
  image_cli=(docker)
else
  echo "No local container image builder is running (Rancher Desktop containerd or Docker)." >&2
  exit 69
fi

git_common_dir="$(git rev-parse --path-format=absolute --git-common-dir)"
build_lock="$git_common_dir/local-image-ci-$component.lock"
if ! mkdir "$build_lock" 2>/dev/null; then
  echo "Another local image CI build holds $build_lock; refusing concurrent version allocation." >&2
  exit 69
fi
release_build_lock() { rmdir "$build_lock"; }
trap release_build_lock EXIT

tag_field() {
  git cat-file -p "refs/tags/$1" | sed -n "s/^$2: //p" | head -1
}

validate_candidate_pair() {
  local version_tag="$1" prior_version prior_image prior_id prior_revision hash_tag
  [[ "$(git cat-file -t "refs/tags/$version_tag")" == tag ]] || {
    echo "Candidate $version_tag is not annotated." >&2; exit 67;
  }
  prior_version="$(tag_field "$version_tag" version)"
  prior_image="$(tag_field "$version_tag" image)"
  prior_id="$(tag_field "$version_tag" image_id)"
  prior_revision="$(tag_field "$version_tag" source_revision)"
  [[ "$prior_version" =~ ^v[0-9]+\.[0-9]+\.[0-9]+-local\.[1-9][0-9]*$ \
    && "$prior_image" == "capitonic/$component:$prior_version" \
    && "$prior_id" =~ ^sha256:[0-9a-f]{64}$ \
    && "$prior_revision" =~ ^[0-9a-f]{40}$ \
    && "$(git rev-list -n 1 "refs/tags/$version_tag")" == "$prior_revision" ]] || {
    echo "Candidate $version_tag has inconsistent version, image, or source metadata." >&2; exit 67;
  }
  hash_tag="image/$component/sha256-${prior_id#sha256:}"
  git show-ref --verify --quiet "refs/tags/$hash_tag" || {
    echo "Candidate $version_tag lacks mandatory hash tag $hash_tag." >&2; exit 67;
  }
  [[ "$(git cat-file -t "refs/tags/$hash_tag")" == tag \
    && "$(git rev-list -n 1 "refs/tags/$hash_tag")" == "$prior_revision" \
    && "$(tag_field "$hash_tag" version)" == "$prior_version" \
    && "$(tag_field "$hash_tag" image)" == "$prior_image" \
    && "$(tag_field "$hash_tag" image_id)" == "$prior_id" \
    && "$(tag_field "$hash_tag" source_revision)" == "$prior_revision" ]] || {
    echo "Candidate $version_tag and $hash_tag disagree." >&2; exit 67;
  }
  "${image_cli[@]}" image inspect "$prior_image" >/dev/null 2>&1 || {
    echo "Candidate image $prior_image is missing; refusing to mint a replacement version." >&2; exit 67;
  }
  [[ "$("${image_cli[@]}" image inspect --format '{{ .Id }}' "$prior_image")" == "$prior_id" \
    && "$("${image_cli[@]}" image inspect --format '{{ index .Config.Labels "org.opencontainers.image.revision" }}' "$prior_image")" == "$prior_revision" \
    && "$("${image_cli[@]}" image inspect --format '{{ index .Config.Labels "org.opencontainers.image.version" }}' "$prior_image")" == "${prior_version#v}" ]] || {
    echo "Candidate image $prior_image does not match its Git tag pair." >&2; exit 67;
  }
  verified_image="$prior_image"
  verified_image_id="$prior_id"
}

inputs_sha256="$(image_inputs_sha256 "$git_revision")"
current_commit_versions=0
while IFS= read -r existing_tag; do
  [[ "$(git rev-list -n 1 "refs/tags/$existing_tag")" == "$git_revision" ]] || continue
  current_commit_versions=$((current_commit_versions + 1))
done < <(git tag --list "image/$component/v*-local.*")
(( current_commit_versions <= 1 )) || {
  echo "This commit already has multiple $component local version tags." >&2; exit 67;
}

while IFS= read -r existing_tag; do
  prior_revision="$(git rev-list -n 1 "refs/tags/$existing_tag")"
  git merge-base --is-ancestor "$prior_revision" "$git_revision" || continue
  [[ "$(image_inputs_sha256 "$prior_revision")" == "$inputs_sha256" ]] || continue
  validate_candidate_pair "$existing_tag"
  echo "Reusing $verified_image ($verified_image_id) from $existing_tag; no new image or tags were created."
  exit 0
done < <(git tag --list "image/$component/v*-local.*")

(( current_commit_versions == 0 )) || {
  echo "This commit already has a $component version tag for different image inputs." >&2; exit 67;
}

version="$(next_local_version)"
if [[ -n "$requested_version" && "$requested_version" != "$version" ]]; then
  echo "Requested v$requested_version is not the next available candidate (v$version)." >&2
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
cleanup_build() {
  cleanup_temporary_image
  release_build_lock
}
trap cleanup_build EXIT
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
# A repeated image ID cannot acquire another source-commit tag pair.
while IFS= read -r existing_tag; do
  if [[ "$(tag_field "$existing_tag" image_id)" == "$image_id" ]]; then
    echo "Image ID $image_id already has $existing_tag; refusing a second source-commit tag pair." >&2
    exit 67
  fi
done < <(git tag --list "image/$component/sha256-*")

version_tag="image/$component/v$version"
hash_tag="image/$component/sha256-${image_id#sha256:}"
if [[ "$(next_local_version)" != "$version" ]] || git show-ref --verify --quiet "refs/tags/$version_tag" || git show-ref --verify --quiet "refs/tags/$hash_tag" || "${image_cli[@]}" image inspect "$image" >/dev/null 2>&1; then
  echo "Candidate version v$version was allocated elsewhere during the build; refusing to reuse it." >&2
  exit 69
fi
tagger_identity="$(git var GIT_COMMITTER_IDENT)"
version_tag_object="$(git mktag <<EOF
object $git_revision
type commit
tag $version_tag
tagger $tagger_identity

component: $component
version: v$version
image: $image
image_id: $image_id
inputs_sha256: $inputs_sha256
source_branch: $source_branch
source_revision: $git_revision
checks: $checks_description
deployment: not deployed; golden status: not assigned
EOF
)"
hash_tag_object="$(git mktag <<EOF
object $git_revision
type commit
tag $hash_tag
tagger $tagger_identity

component: $component
version: v$version
image: $image
image_id: $image_id
inputs_sha256: $inputs_sha256
source_branch: $source_branch
source_revision: $git_revision
checks: $checks_description
deployment: not deployed; golden status: not assigned
EOF
)"
"${image_cli[@]}" image tag "$temporary_image" "$image"
if ! git update-ref --stdin <<EOF
start
verify refs/heads/$source_branch $git_revision
create refs/tags/$version_tag $version_tag_object
create refs/tags/$hash_tag $hash_tag_object
prepare
commit
EOF
then
  "${image_cli[@]}" image rm "$image" >/dev/null 2>&1 || true
  exit 69
fi
validate_candidate_pair "$version_tag"

printf 'Candidate image: %s\nImage ID: %s\nGit revision: %s\nVersion tag: %s\nHash tag: %s\n' \
  "$image" "$image_id" "$git_revision" "$version_tag" "$hash_tag"
