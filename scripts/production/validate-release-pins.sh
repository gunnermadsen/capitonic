#!/usr/bin/env bash
# Read committed release inputs and registry metadata; write runner diagnostics only.
set -euo pipefail

report="${PIN_REPORT:?PIN_REPORT is required}"
revision="$(git rev-parse HEAD)"
mkdir -p "$(dirname "$report")"
{
  echo "Inspected revision: $revision"
  echo "Comparison baseline: ${BASELINE_REVISION:-unavailable}"
  echo "Selected components: ${DEPLOY_COMPONENTS:-none (no-op)}"
  echo
  echo '| Component | Image version | Chart version | appVersion | Verified digest | Source revision |'
  echo '|---|---|---|---|---|---|'
} > "$report"

for component in ingester polymarket-bot db-migrate; do
  values="capitonic-helm-chart/environments/production/$component.yaml"
  chart="capitonic-helm-chart/charts/$component/Chart.yaml"
  version="$(yq -r '.release.version' "$values")"
  expected_digest="$(yq -r '.release.digest' "$values")"
  source_revision="$(yq -r '.release.sourceRevision' "$values")"
  image="$(yq -r '.image' "$values")"
  chart_version="$(yq -r '.version' "$chart")"
  app_version="$(yq -r '.appVersion' "$chart")"
  [[ "$version" =~ ^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]] || {
    echo "Invalid production version for $component; test and RC tags are not final pins." >&2; exit 1;
  }
  [[ "$chart_version" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]]
  [[ "$app_version" == "$version" ]]
  [[ "$image" == "$ECR_REGISTRY/capitonic/$component:$version" ]]
  [[ "$expected_digest" =~ ^sha256:[0-9a-f]{64}$ && "$source_revision" =~ ^[0-9a-f]{40}$ ]]
  qualified=false
  while IFS= read -r tag; do
    [[ "$(git cat-file -t "refs/tags/$tag")" == tag ]] || continue
    checkpoint="$(git rev-list -n1 "$tag")"
    git merge-base --is-ancestor "$checkpoint" "$revision" || continue
    annotation="$(git for-each-ref --format='%(contents)' "refs/tags/$tag")"
    admitted_source="$(awk '$1 == "source_revision:" {print $2}' <<< "$annotation")"
    golden_tag="$(awk '$1 == "golden_tag:" {print $2}' <<< "$annotation")"
    [[ "$admitted_source" == "$source_revision" && "$golden_tag" == golden/$component/* ]] || continue
    [[ "$(git cat-file -t "refs/tags/$golden_tag" 2>/dev/null)" == tag ]] || continue
    qualified=true
    break
  done < <(git tag --list "rc/$component/$version-rc.*" --sort=-version:refname)
  [[ "$qualified" == true ]] || { echo "No accepted RC provenance for $component:$version." >&2; exit 1; }
  actual_digest="$(scripts/production/verify-ecr-arm64-image.sh "capitonic/$component" "$version" "$source_revision")"
  [[ "$actual_digest" == "$expected_digest" ]] || { echo "Registry digest mismatch for $component:$version." >&2; exit 1; }
  if [[ "$component" == ingester ]]; then
    [[ "$(yq -r '.imageDigest' "$values")" == "$actual_digest" ]]
    [[ "$(yq -r '.gitRevision' "$values")" == "$source_revision" ]]
  fi
  if [[ "$component" != db-migrate ]]; then
    yq -o=json '.externalSecrets' "$values" | jq -e '
      .enabled == true and (.entries | length > 0) and
      all(.entries[]; (.version | type == "string" and length > 0) and (.properties | length > 0))
    ' >/dev/null
  fi
  printf '| %s | %s | %s | %s | %s | %s |\n' "$component" "$version" "$chart_version" "$app_version" "$actual_digest" "$source_revision" >> "$report"
  if [[ "$component" == db-migrate ]]; then
    migration_inputs="$(git ls-tree -r --name-only "$source_revision" -- packages/db-migrate/src/migrations packages/db-migrate/src/fresh-install | wc -l | tr -d ' ')"
  fi
done

python3 scripts/helm/bake-assets.py
trap 'python3 scripts/helm/bake-assets.py --clean >/dev/null 2>&1 || true' EXIT
IFS=, read -ra selected <<< "${DEPLOY_COMPONENTS:-}"
for component in "${selected[@]}"; do
  [[ -n "$component" ]] || continue
  case "$component" in
    ingester|polymarket-bot|db-migrate|grafana|prometheus|loki|alloy|cloudflared) ;;
    *) echo "No diagnostic chart owner for $component." >&2; exit 1 ;;
  esac
  chart="capitonic-helm-chart/charts/$component"
  values="capitonic-helm-chart/environments/production/$component.yaml"
  options=()
  [[ "$component" == cloudflared ]] && options=(--set-string tunnelId=00000000-0000-0000-0000-000000000001)
  helm lint "$chart" -f "$values" "${options[@]}"
  rendered="$(mktemp)"
  helm template "$component" "$chart" -f "$values" "${options[@]}" > "$rendered"
  if [[ "$component" == ingester || "$component" == polymarket-bot || "$component" == db-migrate ]]; then
    wanted="$(yq -r '.image' "$values")"
    [[ "$(yq -r -N '.. | select(has("image")) | .image' "$rendered" | sort -u)" == "$wanted" ]]
  fi
  rm -f "$rendered"
  echo "Rendered and linted: $component" >> "$report"
done
{
  echo
  echo 'Secret-version declarations validated; live secret availability and reconciliation: UNVERIFIED.'
  echo 'Migration requirements: committed migration files only; pending ledger and compatibility: UNVERIFIED.'
  echo "Migration runner pins $migration_inputs committed migration/baseline input files; no migration execution is planned by this workflow."
  echo 'Runtime readiness, worker capacity, trading recovery and rollback: UNVERIFIED.'
  echo 'Cloudflared rendering, if selected, uses a diagnostic tunnel-ID placeholder.'
  echo 'Diagnostic validation completed; production deployment skipped.'
} >> "$report"
