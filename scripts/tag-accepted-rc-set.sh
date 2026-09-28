#!/usr/bin/env bash

set -euo pipefail

repository_root="$(git rev-parse --show-toplevel 2>/dev/null)" || {
  echo "Run this command from within the polymarket-bot Git repository." >&2
  exit 64
}
cd "$repository_root"

if [[ -n "$(git status --porcelain --untracked-files=all)" ]]; then
  echo "Commit or remove working-tree changes before tagging an accepted RC set." >&2
  exit 65
fi

for tool in jq git; do
  command -v "$tool" >/dev/null 2>&1 || { echo "Missing $tool" >&2; exit 69; }
done

components=(polymarket-bot ingester db-migrate)
for component in "${components[@]}"; do
  CAPITONIC_DEFER_RC_PUSH=true scripts/local-image-ci.sh "$component" --tag-rc
done

accepted="$(git rev-parse --verify 'development^{commit}')"
checkpoint="checkpoint/development/git-$accepted"
push_refs=(refs/heads/development "refs/tags/$checkpoint")

current_branch="$(git symbolic-ref --quiet --short HEAD || true)"
if [[ "$current_branch" == integration-* \
  && "$(git rev-parse --verify 'HEAD^{commit}')" == "$accepted" ]]; then
  push_refs+=("refs/heads/$current_branch")
fi

for component in "${components[@]}"; do
  version="$(jq -er --arg component "$component" '.components[$component].rcVersion' infra/production/release.json)"
  rc_tag="rc/$component/$version"
  annotation="$(git cat-file -p "refs/tags/$rc_tag")"
  candidate_tag="$(awk '$1 == "candidate_tag:" { print $2 }' <<<"$annotation")"
  golden_tag="$(awk '$1 == "golden_tag:" { print $2 }' <<<"$annotation")"
  image_id="$(awk '$1 == "image_id:" { print $2 }' <<<"$annotation")"
  hash_tag="image/$component/sha256-${image_id#sha256:}"
  for tag in "$candidate_tag" "$hash_tag" "$golden_tag" "$rc_tag"; do
    [[ -n "$tag" && "$(git cat-file -t "refs/tags/$tag" 2>/dev/null)" == tag ]] || {
      echo "Required annotated release tag is missing: $tag" >&2
      exit 67
    }
    push_refs+=("refs/tags/$tag")
  done
done

git push --atomic origin "${push_refs[@]}"
echo "Atomically pushed the complete accepted RC set and its provenance refs to origin."
