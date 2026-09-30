#!/usr/bin/env bash
set -euo pipefail

base="${1:?usage: detect-components.sh <last-successful-production-revision> <candidate-revision>}"
candidate="${2:?usage: detect-components.sh <last-successful-production-revision> <candidate-revision>}"
git merge-base --is-ancestor "$base" "$candidate"

images=("") charts=("")
image() { images+=("$1"); charts+=("$1"); }
chart() {
  case "$1" in
    polymarket-bot|ingester|db-migrate|grafana|prometheus|loki|alloy|cloudflared) charts+=("$1") ;;
    *) echo "No automatic deployment owner for chart $1; leaving it unchanged." >&2 ;;
  esac
}

while IFS= read -r path; do
  case "$path" in
    Cargo.toml|Cargo.lock|common/proto/*) image polymarket-bot; image ingester ;;
    packages/polymarket-bot/*|packages/btc-directional-model/runtime-models/*) image polymarket-bot ;;
    packages/market-data-ingester/*) image ingester ;;
    packages/db-migrate/*) image db-migrate ;;
    common/configs/grafana/*) chart grafana ;;
    common/configs/prometheus/*) chart prometheus ;;
    common/configs/loki/*) chart loki ;;
    common/configs/alloy/*) chart alloy ;;
    capitonic-helm-chart/charts/*/*)
      component="${path#capitonic-helm-chart/charts/}"
      chart "${component%%/*}"
      ;;
    capitonic-helm-chart/environments/production/*.yaml)
      component="${path##*/}"
      chart "${component%.yaml}"
      ;;
  esac
done < <(git diff --name-only "$base" "$candidate")

# Release metadata is component-owned. A new accepted RC requires promotion even
# when its source commit predates this production push.
for component in polymarket-bot ingester db-migrate; do
  previous="$(git show "$base:infra/production/release.json" | jq -c --arg component "$component" '.components[$component] | {rcVersion,sourceRevision}')"
  current="$(git show "$candidate:infra/production/release.json" | jq -c --arg component "$component" '.components[$component] | {rcVersion,sourceRevision}')"
  [[ "$previous" == "$current" ]] || image "$component"
done

image_list="$(printf '%s\n' "${images[@]}" | sed '/^$/d' | sort -u | paste -sd, -)"
chart_list="$(printf '%s\n' "${charts[@]}" | sed '/^$/d' | sort -u | paste -sd, -)"
{
  echo "images=$image_list"
  echo "charts=$chart_list"
  if [[ -n "$chart_list" ]]; then echo 'any=true'; else echo 'any=false'; fi
} >> "${GITHUB_OUTPUT:-/dev/stdout}"
