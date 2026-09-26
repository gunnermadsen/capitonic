#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/local-image-cd.sh pin <ingester|polymarket-bot> <vMAJOR.MINOR.PATCH-local.N>
       scripts/local-image-cd.sh prepare-ingester
       scripts/local-image-cd.sh deploy <ingester|polymarket-bot>

pin edits local Helm values only. Commit and review those values before deploy.
prepare-ingester applies only the worker rolling-update strategy with the old image.
deploy applies the exact committed, prebuilt image pinned in the chart.
Database migration Jobs and golden promotion are separate workflows.
EOF
}

if [[ "${1:-}" == "--help" || "${1:-}" == "-h" ]]; then usage; exit 0; fi
case "${1:-}" in
  pin) [[ $# == 3 ]] || { usage >&2; exit 64; }; component="$2"; version="$3" ;;
  prepare-ingester) [[ $# == 1 ]] || { usage >&2; exit 64; }; component=ingester ;;
  deploy) [[ $# == 2 ]] || { usage >&2; exit 64; }; component="$2" ;;
  *) usage >&2; exit 64 ;;
esac
case "$component" in ingester|polymarket-bot) ;; *) usage >&2; exit 64 ;; esac

root="$(git rev-parse --show-toplevel)"
cd "$root"
chart="capitonic-helm-chart/charts/$component"
values="$chart/values.yaml"
namespace=capitonic
for tool in git helm kubectl jq yq nerdctl; do
  command -v "$tool" >/dev/null 2>&1 || { echo "Missing $tool" >&2; exit 69; }
done

require_clean_commit() {
  [[ -z "$(git status --porcelain --untracked-files=all)" ]] || {
    echo "Commit working-tree changes before applying a Helm release." >&2; exit 65;
  }
  git symbolic-ref --quiet --short HEAD >/dev/null || {
    echo "Deploy from a named branch." >&2; exit 65;
  }
}

candidate_identity() {
  local tag annotation tagged_revision found=0
  [[ "$version" =~ ^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)-local\.([1-9][0-9]*)$ ]] || {
    echo "Select a local candidate such as v1.2.1-local.1." >&2; exit 64;
  }
  image="capitonic/$component:$version"
  nerdctl --namespace k8s.io image inspect "$image" >/dev/null 2>&1 || {
    echo "Candidate $image is absent from k3s containerd." >&2; exit 69;
  }
  image_id="$(nerdctl --namespace k8s.io image inspect --format '{{ .Id }}' "$image")"
  image_revision="$(nerdctl --namespace k8s.io image inspect --format '{{ index .Config.Labels "org.opencontainers.image.revision" }}' "$image")"
  image_version="$(nerdctl --namespace k8s.io image inspect --format '{{ index .Config.Labels "org.opencontainers.image.version" }}' "$image")"
  [[ "$image_id" =~ ^sha256:[0-9a-f]{64}$ && "$image_revision" =~ ^[0-9a-f]{40}$ && "$image_version" == "${version#v}" ]] || {
    echo "Candidate image labels or identity are invalid." >&2; exit 67;
  }
  for tag in "image/$component/$version" $(git tag --list "image/$component/sha256-*"); do
    git show-ref --verify --quiet "refs/tags/$tag" || continue
    annotation="$(git cat-file -p "refs/tags/$tag")"
    tagged_revision="$(git rev-list -n 1 "refs/tags/$tag")"
    if grep -Fqx "version: $version" <<< "$annotation" &&
       grep -Fqx "image_id: $image_id" <<< "$annotation" &&
       grep -Fqx "source_revision: $image_revision" <<< "$annotation" &&
       [[ "$tagged_revision" == "$image_revision" ]]; then
      provenance_tag="$tag"
      found=1
      break
    fi
  done
  (( found == 1 )) || { echo "No matching annotated image provenance tag exists for $image." >&2; exit 67; }
  git merge-base --is-ancestor "$image_revision" HEAD || {
    echo "Candidate source commit is outside this branch lineage." >&2; exit 67;
  }
}

chart_image() { yq -r '.image' "$values"; }
live_image() { kubectl -n "$namespace" get deployment "$1" -o json | jq -r '.spec.template.spec.containers[0].image'; }
ready_deployment() {
  kubectl -n "$namespace" get deployment "$1" -o json | jq -e '
    .spec.replicas > 0 and .status.readyReplicas == .spec.replicas and
    .status.availableReplicas == .spec.replicas' >/dev/null
}

ingester_snapshot() {
  local destination="$1"
  mkdir -p "$destination"
  kubectl -n "$namespace" exec deployment/ingester-master -- sh -c \
    'curl -fsS -H "Authorization: Bearer $INGESTER_ADMIN_TOKEN" http://127.0.0.1:8098/ingesters' > "$destination/profiles.json"
  kubectl -n "$namespace" exec timescaledb-0 -c timescaledb -- psql -U postgres -d polymarket -Atc \
    "SELECT count(*),coalesce(sum(allocation_units),0) FROM ingester.backfill_jobs WHERE assigned_worker_id IS NOT NULL AND status IN ('running','cancel_requested')" > "$destination/backfills.txt"
  kubectl -n "$namespace" exec timescaledb-0 -c timescaledb -- psql -U postgres -d polymarket -Atc \
    'SELECT process_id FROM polymarket.trading_processes WHERE enabled ORDER BY process_id LIMIT 100' > "$destination/enabled-processes.txt"
  kubectl -n "$namespace" exec timescaledb-0 -c timescaledb -- psql -U postgres -d polymarket -Atc \
    'SELECT count(*),coalesce(max(timestamp),0) FROM public.migrations' > "$destination/migrations.txt"
}

check_ingester_capacity() {
  local destination="$1" desired active ready
  desired="$(jq '[.[] | select(.desired_state == "running")] | length' "$destination/profiles.json")"
  active="$(cut -d '|' -f 1 "$destination/backfills.txt")"
  ready="$(kubectl -n "$namespace" get deployment ingester-worker -o json | jq -r '.status.readyReplicas // 0')"
  printf 'Ingester capacity: %s ready workers, %s desired realtime profiles, %s active backfill shards\n' "$ready" "$desired" "$active"
  (( ready >= desired + active )) || {
    echo "Worker capacity is below the conservative rollout requirement; master must add capacity first." >&2; exit 70;
  }
  jq -e 'all(.[] | select(.desired_state == "running");
    .observed_state == "running" and .health_status == "healthy" and .lease_owner != null)' \
    "$destination/profiles.json" >/dev/null || {
    echo "A desired realtime profile lacks a healthy owner before rollout." >&2; exit 70;
  }
}

save_rollback_snapshot() {
  snapshot="$(mktemp -d "${TMPDIR:-/private/tmp}/local-image-cd.XXXXXX")"
  helm -n "$namespace" get manifest "$component" > "$snapshot/helm-manifest.yaml"
  helm -n "$namespace" get values "$component" -a -o yaml > "$snapshot/helm-values.yaml"
  kubectl -n "$namespace" get deployment "$component" -o json > "$snapshot/deployment.json" 2>/dev/null || true
  if [[ "$component" == ingester ]]; then
    kubectl -n "$namespace" get deployment ingester-master ingester-worker -o json > "$snapshot/ingester-deployments.json"
    kubectl -n "$namespace" get pods -l app.kubernetes.io/part-of=capitonic-platform -o json > "$snapshot/pods.json"
    ingester_snapshot "$snapshot/pre"
    check_ingester_capacity "$snapshot/pre"
  fi
  printf 'Rollback snapshot: %s\n' "$snapshot"
}

normalized_manifest() {
  local mode="$1" path="$2"
  yq -o=json ea '[.]' "$path" | jq -S --arg mode "$mode" '
    map(if .kind == "Deployment" and .metadata.name == "ingester-worker" and $mode == "prepare" then
      del(.spec.strategy)
    elif .kind == "Deployment" and $mode == "ingester" and .metadata.name == "ingester-master" then
      .spec.template.spec.containers[0].image = null
    elif .kind == "Deployment" and $mode == "ingester" and .metadata.name == "ingester-worker" then
      .spec.template.spec.containers[0].image = null |
      .spec.template.spec.containers[0].env |= map(if .name == "INGESTER_IMAGE_DIGEST" or .name == "INGESTER_GIT_REVISION" then .value = null else . end)
    elif .kind == "Deployment" and $mode == "polymarket-bot" and .metadata.name == "polymarket-bot" then
      .spec.template.spec.containers[0].image = null
    else . end) | sort_by(.kind, .metadata.name)'
}

check_manifest_scope() {
  local mode="$1"
  helm template "$component" "$chart" -n "$namespace" > "$snapshot/selected-manifest.yaml"
  cmp -s <(normalized_manifest "$mode" "$snapshot/helm-manifest.yaml") \
    <(normalized_manifest "$mode" "$snapshot/selected-manifest.yaml") || {
    echo "Chart changes exceed the permitted $mode rollout fields; stop and review the manifest diff." >&2
    diff -u "$snapshot/helm-manifest.yaml" "$snapshot/selected-manifest.yaml" | head -100 >&2 || true
    exit 70
  }
}

verify_image_pods() {
  local deployment="$1" expected_id="$2"
  kubectl -n "$namespace" rollout status "deployment/$deployment" --timeout=15m
  kubectl -n "$namespace" get pods -l "app.kubernetes.io/name=$deployment" -o json | jq -e --arg id "$expected_id" '
    [.items[] | select(.metadata.deletionTimestamp == null)] as $pods |
    ($pods | length) > 0 and all($pods[];
      .status.phase == "Running" and
      (.status.containerStatuses[0].ready == true) and
      (.status.containerStatuses[0].imageID == $id) and
      (.status.containerStatuses[0].restartCount == 0))' >/dev/null
}

verify_bot_routes() {
  local failures
  # Allow a brief route handoff, then reject a repeating affected-path error.
  sleep 20
  failures="$(kubectl -n "$namespace" logs deployment/polymarket-bot --since=30s --tail=300 |
    jq -Rr 'fromjson? | select(.fields.message? == "market-data worker route unavailable; preserving other worker streams") | .timestamp' |
    wc -l | tr -d ' ')"
  if (( failures >= 2 )); then
    echo "The bot still reports repeated unavailable ingester routes; investigate before accepting this deployment." >&2
    exit 70
  fi
}

if [[ "$1" == pin ]]; then
  candidate_identity
  if [[ "$component" == ingester ]]; then
    IMAGE="$image" IMAGE_ID="$image_id" IMAGE_REVISION="$image_revision" \
      yq -i '.image = strenv(IMAGE) | .imageDigest = strenv(IMAGE_ID) | .gitRevision = strenv(IMAGE_REVISION)' "$values"
  else
    IMAGE="$image" yq -i '.image = strenv(IMAGE)' "$values"
  fi
  echo "Pinned $image ($image_id) in $values; review and commit before deploy."
  exit 0
fi

require_clean_commit
helm lint "$chart"
if [[ "$1" == prepare-ingester ]]; then
  [[ "$(chart_image)" == "$(live_image ingester-master)" && "$(chart_image)" == "$(live_image ingester-worker)" ]] || {
    echo "The chart must still pin the exact running ingester image for strategy preparation." >&2; exit 70;
  }
  ready_deployment ingester-master && ready_deployment ingester-worker || {
    echo "Ingester deployments are not ready." >&2; exit 70;
  }
  save_rollback_snapshot
  check_manifest_scope prepare
  helm upgrade --install ingester "$chart" -n "$namespace" --wait --timeout 15m
  [[ "$(kubectl -n "$namespace" get deployment ingester-worker -o json | jq -r '.spec.strategy.type')" == RollingUpdate ]] || exit 70
  [[ "$(live_image ingester-master)" == "$(chart_image)" && "$(live_image ingester-worker)" == "$(chart_image)" ]] || exit 70
  ready_deployment ingester-master && ready_deployment ingester-worker
  echo "Worker RollingUpdate is active; ingester image remains $(chart_image)."
  exit 0
fi

version="${2:-}"
if [[ "$1" == deploy ]]; then
  version="$(chart_image)"
  version="${version#capitonic/$component:}"
  candidate_identity
  [[ "$(chart_image)" == "$image" ]] || exit 67
  if [[ "$component" == ingester ]]; then
    [[ "$(yq -r '.imageDigest' "$values")" == "$image_id" && "$(yq -r '.gitRevision' "$values")" == "$image_revision" ]] || {
      echo "Ingester chart digest or revision does not match candidate provenance." >&2; exit 67;
    }
    [[ "$(kubectl -n "$namespace" get deployment ingester-worker -o json | jq -r '.spec.strategy.type')" == RollingUpdate ]] || {
      echo "Apply and verify the worker RollingUpdate strategy before replacing the image." >&2; exit 70;
    }
    ready_deployment ingester-master && ready_deployment ingester-worker || exit 70
  else
    ready_deployment polymarket-bot || exit 70
  fi
  save_rollback_snapshot
  check_manifest_scope "$component"
  helm upgrade --install "$component" "$chart" -n "$namespace" --wait --timeout 15m
  if [[ "$component" == ingester ]]; then
    verify_image_pods ingester-master "$image_id"
    verify_image_pods ingester-worker "$image_id"
    ingester_snapshot "$snapshot/post"
    check_ingester_capacity "$snapshot/post"
    cmp -s "$snapshot/pre/enabled-processes.txt" "$snapshot/post/enabled-processes.txt" || {
      echo "Enabled trading-process set changed during deployment." >&2; exit 70;
    }
    cmp -s "$snapshot/pre/migrations.txt" "$snapshot/post/migrations.txt" || {
      echo "Migration ledger changed during deployment." >&2; exit 70;
    }
    verify_bot_routes
  else
    verify_image_pods polymarket-bot "$image_id"
    verify_bot_routes
  fi
  echo "Deployed $image ($image_id); source $image_revision; provenance $provenance_tag."
fi
