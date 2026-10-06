#!/usr/bin/env bash
set -euo pipefail

base="${1:?usage: detect-components.sh <last-successful-production-revision> <candidate-revision>}"
candidate="${2:?usage: detect-components.sh <last-successful-production-revision> <candidate-revision>}"
git merge-base --is-ancestor "$base" "$candidate"

charts=("")
chart() {
  case "$1" in
    polymarket-bot|ingester|db-migrate|grafana|prometheus|loki|alloy|cloudflared) charts+=("$1") ;;
    *) echo "No automatic deployment owner for chart $1; leaving it unchanged." >&2 ;;
  esac
}

while IFS= read -r path; do
  case "$path" in
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

chart_list="$(printf '%s\n' "${charts[@]}" | sed '/^$/d' | sort -u | paste -sd, -)"
{
  echo "images="
  echo "charts=$chart_list"
  if [[ -n "$chart_list" ]]; then echo 'any=true'; else echo 'any=false'; fi
} >> "${GITHUB_OUTPUT:-/dev/stdout}"
