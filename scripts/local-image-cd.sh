#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/local-image-cd.sh pin <ingester|polymarket-bot> <vMAJOR.MINOR.PATCH-local.N>
       scripts/local-image-cd.sh rc <ingester|polymarket-bot> <vMAJOR.MINOR.PATCH-local.N> <vMAJOR.MINOR.PATCH-rc.N>
       scripts/local-image-cd.sh prepare-ingester
       scripts/local-image-cd.sh deploy <ingester|polymarket-bot>

pin edits local Helm values only. Commit and review those values before deploy.
rc aliases verified local candidate bytes and pins the RC image in Helm values.
prepare-ingester applies only the worker rolling-update strategy with the old image.
deploy applies the exact committed, prebuilt image pinned in the chart.
Database migration Jobs and golden promotion are separate workflows.
EOF
}

if [[ "${1:-}" == "--help" || "${1:-}" == "-h" ]]; then usage; exit 0; fi
case "${1:-}" in
  pin) [[ $# == 3 ]] || { usage >&2; exit 64; }; component="$2"; version="$3" ;;
  rc) [[ $# == 4 ]] || { usage >&2; exit 64; }; component="$2"; local_version="$3"; rc_version="$4"; version="$3" ;;
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

tag_field() {
  git cat-file -p "refs/tags/$1" | sed -n "s/^$2: //p" | head -1
}

validate_candidate_tag() {
  local tag="$1" expected_version="$2" expected_image="$3"
  git show-ref --verify --quiet "refs/tags/$tag" &&
    [[ "$(git cat-file -t "refs/tags/$tag")" == tag ]] &&
    [[ "$(git rev-list -n 1 "refs/tags/$tag")" == "$image_revision" ]] &&
    [[ "$(tag_field "$tag" component)" == "$component" ]] &&
    [[ "$(tag_field "$tag" version)" == "$expected_version" ]] &&
    [[ "$(tag_field "$tag" image)" == "$expected_image" ]] &&
    [[ "$(tag_field "$tag" image_id)" == "$image_id" ]] &&
    [[ "$(tag_field "$tag" source_revision)" == "$image_revision" ]]
}

candidate_identity() {
  [[ "$version" =~ ^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)-(local|rc)\.([1-9][0-9]*)$ ]] || {
    echo "Select a local candidate or RC version." >&2; exit 64;
  }
  image="capitonic/$component:$version"
  nerdctl --namespace k8s.io image inspect "$image" >/dev/null 2>&1 || {
    echo "Candidate $image is absent from k3s containerd." >&2; exit 69;
  }
  image_id="$(nerdctl --namespace k8s.io image inspect --format '{{ .Id }}' "$image")"
  image_revision="$(nerdctl --namespace k8s.io image inspect --format '{{ index .Config.Labels "org.opencontainers.image.revision" }}' "$image")"
  image_version="$(nerdctl --namespace k8s.io image inspect --format '{{ index .Config.Labels "org.opencontainers.image.version" }}' "$image")"
  provenance_version="v$image_version"
  [[ "$image_id" =~ ^sha256:[0-9a-f]{64}$ && "$image_revision" =~ ^[0-9a-f]{40}$
    && "$provenance_version" =~ ^v[0-9]+\.[0-9]+\.[0-9]+-local\.[1-9][0-9]*$
    && "${version%%-*}" == "${provenance_version%%-*}" ]] || {
    echo "Candidate image labels or identity are invalid." >&2; exit 67;
  }
  if [[ "$version" == *-local.* && "$version" != "$provenance_version" ]]; then
    echo "Candidate tag disagrees with its build version label." >&2; exit 67
  fi
  version_tag="image/$component/$provenance_version"
  hash_tag="image/$component/sha256-${image_id#sha256:}"
  provenance_image="capitonic/$component:$provenance_version"
  validate_candidate_tag "$version_tag" "$provenance_version" "$provenance_image" &&
    validate_candidate_tag "$hash_tag" "$provenance_version" "$provenance_image" || {
    echo "Candidate $image requires matching annotated version and image-hash Git tags." >&2
    exit 67
  }
  [[ "$(tag_field "$version_tag" inputs_sha256)" == "$(tag_field "$hash_tag" inputs_sha256)" ]] || {
    echo "Candidate Git tag pair disagrees on image inputs." >&2; exit 67
  }
  if [[ "$version" == *-rc.* ]]; then
    [[ "$(nerdctl --namespace k8s.io image inspect --format '{{ .Id }}' "$provenance_image")" == "$image_id" ]] || {
      echo "RC alias differs from its tested local candidate bytes." >&2; exit 67;
    }
  fi
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

pod_image_id() {
  kubectl -n "$namespace" get pods -l "app.kubernetes.io/name=$1" -o json | jq -er '
    [.items[] | select(.metadata.deletionTimestamp == null) | .status.containerStatuses[0].imageID] |
    unique | if length == 1 and .[0] != null then .[0] else error("pods do not share one image ID") end'
}

route_failure_count() {
  local window="${1:-30s}"
  kubectl -n "$namespace" logs deployment/polymarket-bot --since="$window" --tail=300 |
    jq -Rr 'fromjson? | select(.fields.message? == "market-data worker route unavailable; preserving other worker streams") | .timestamp' |
    wc -l | tr -d ' '
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

healthy_processes() {
  local minimum_epoch="$1" unhealthy
  [[ "$minimum_epoch" =~ ^[0-9]+$ ]] || return 1
  unhealthy="$(kubectl -n "$namespace" exec timescaledb-0 -c timescaledb -- psql -U postgres -d polymarket -Atc \
    "SELECT count(*) FROM polymarket.trading_processes WHERE enabled AND (status IS DISTINCT FROM 'running' OR heartbeat_at IS NULL OR heartbeat_at < to_timestamp($minimum_epoch))")" || return 1
  [[ "$unhealthy" == 0 ]]
}

wait_for_process_recovery() {
  local minimum_epoch="$1" attempt
  for ((attempt = 0; attempt < 24; attempt++)); do
    healthy_processes "$minimum_epoch" && return 0
    sleep 5
  done
  echo "Enabled trading processes did not resume with fresh heartbeats." >&2
  return 1
}

check_ingester_capacity() {
  local destination="$1" desired active ready
  desired="$(jq '[.[] | select(.desired_state == "running")] | length' "$destination/profiles.json")"
  active="$(cut -d '|' -f 2 "$destination/backfills.txt")"
  ready="$(kubectl -n "$namespace" get deployment ingester-worker -o json | jq -r '.status.readyReplicas // 0')"
  printf 'Ingester capacity: %s ready workers, %s desired realtime profiles, %s active backfill shards\n' "$ready" "$desired" "$active"
  (( ready >= desired + active )) || {
    echo "Worker capacity is below the conservative rollout requirement; master must add capacity first." >&2; return 1;
  }
  jq -e 'all(.[] | select(.desired_state == "running");
    .observed_state == "running" and .health_status == "healthy" and .lease_owner != null)' \
    "$destination/profiles.json" >/dev/null || {
    echo "A desired realtime profile lacks a healthy owner." >&2; return 1;
  }
}

save_rollback_snapshot() {
  snapshot="$(mktemp -d "${TMPDIR:-/private/tmp}/local-image-cd.XXXXXX")"
  rollback_revision="$(helm -n "$namespace" history "$component" -o json | jq -er '[.[] | select(.status == "deployed")] | last | .revision')"
  printf '%s\n' "$rollback_revision" > "$snapshot/helm-revision.txt"
  helm -n "$namespace" get manifest "$component" > "$snapshot/helm-manifest.yaml"
  helm -n "$namespace" get values "$component" -a -o yaml > "$snapshot/helm-values.yaml"
  ingester_snapshot "$snapshot/pre"
  check_ingester_capacity "$snapshot/pre"
  wait_for_process_recovery "$(($(date -u +%s) - 90))"
  [[ "$(route_failure_count)" -lt 2 ]] || {
    echo "Bot routes were already failing before deployment." >&2; exit 70;
  }
  if [[ "$component" == ingester ]]; then
    kubectl -n "$namespace" get deployment ingester-master ingester-worker -o json > "$snapshot/ingester-deployments.json"
    rollback_image="$(live_image ingester-master)"
    [[ "$rollback_image" == "$(live_image ingester-worker)" ]] || { echo "Ingester roles run different images." >&2; exit 70; }
    rollback_image_id="$(pod_image_id ingester-master)"
    [[ "$rollback_image_id" == "$(pod_image_id ingester-worker)" ]] || { echo "Ingester roles run different image IDs." >&2; exit 70; }
  else
    kubectl -n "$namespace" get deployment polymarket-bot -o json > "$snapshot/deployment.json"
    rollback_image="$(live_image polymarket-bot)"
    rollback_image_id="$(pod_image_id polymarket-bot)"
  fi
  printf '%s\n' "$rollback_image" > "$snapshot/image.txt"
  printf '%s\n' "$rollback_image_id" > "$snapshot/image-id.txt"
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
  local deployment="$1" expected_id="$2" expected_image="$3"
  kubectl -n "$namespace" rollout status "deployment/$deployment" --timeout=15m || return 1
  kubectl -n "$namespace" get pods -l "app.kubernetes.io/name=$deployment" -o json | jq -e --arg id "$expected_id" --arg image "$expected_image" '
    [.items[] | select(.metadata.deletionTimestamp == null)] as $pods |
    ($pods | length) > 0 and all($pods[];
      .status.phase == "Running" and
      .spec.containers[0].image == $image and
      (.status.containerStatuses[0].ready == true) and
      (.status.containerStatuses[0].imageID == $id) and
      (.status.containerStatuses[0].restartCount == 0))' >/dev/null
}

verify_bot_routes() {
  local attempt failures quiet=0
  # A replaced worker may publish missing history shortly after becoming ready.
  # Require a quiet route window, with a bounded deadline for sustained failures.
  sleep 10
  for ((attempt = 0; attempt < 12; attempt++)); do
    failures="$(route_failure_count 10s)" || return 1
    if (( failures == 0 )); then
      quiet=$((quiet + 1))
      if (( quiet >= 2 )); then return 0; fi
    else
      quiet=0
    fi
    sleep 5
  done
  echo "The bot still reports repeated unavailable ingester routes after the recovery window." >&2
  return 1
}

verify_profile_recovery() {
  local destination="$1"
  jq -e -s '
    (.[0] | map(select(.desired_state == "running") | {key: .strategy_key, value: .}) | from_entries) as $prior |
    (.[1] | map(select(.desired_state == "running") | {key: .strategy_key, value: .}) | from_entries) as $current |
    ($prior | keys) == ($current | keys) and
    all($current[];
      .observed_state == "running" and .health_status == "healthy" and
      .lease_owner != null and .heartbeat_at != null and
      .heartbeat_at > ($prior[.strategy_key].heartbeat_at // ""))' \
    "$snapshot/pre/profiles.json" "$destination/profiles.json" >/dev/null
}

wait_for_profile_recovery() {
  local destination="$1" attempt
  # Resolution evidence may not mature until the next five-minute market window.
  for ((attempt = 0; attempt < 96; attempt++)); do
    verify_profile_recovery "$destination" && return 0
    sleep 5
    ingester_snapshot "$destination" || return 1
  done
  echo "Desired ingester profiles did not recover healthy owners and new heartbeats." >&2
  return 1
}

verify_feed_advancement() {
  jq -e -s '
    (.[0] | map({key: .strategy_key, value: .}) | from_entries) as $prior |
    all(.[1][] | select(.desired_state == "running" and
      (.strategy_key == "binance_spot_btcusdt_one_second_ohlcv" or
       .strategy_key == "polymarket_btc_five_minute_orderbooks" or
       .strategy_key == "polymarket_chainlink_btcusd_twap"));
      (.source_watermark // "") > ($prior[.strategy_key].source_watermark // ""))' \
    "$snapshot/pre/profiles.json" "$snapshot/post/profiles.json" >/dev/null
}

verify_runtime_recovery() {
  verify_bot_routes || return 1
  ingester_snapshot "$snapshot/post" || return 1
  cmp -s "$snapshot/pre/enabled-processes.txt" "$snapshot/post/enabled-processes.txt" || return 1
  cmp -s "$snapshot/pre/migrations.txt" "$snapshot/post/migrations.txt" || return 1
  wait_for_profile_recovery "$snapshot/post" || return 1
  check_ingester_capacity "$snapshot/post" || return 1
  wait_for_process_recovery "$rollout_epoch" || return 1
}

verify_bot_recovery() {
  verify_bot_routes || return 1
  wait_for_process_recovery "$rollout_epoch" || return 1
}

verify_shared_runtime() {
  ingester_snapshot "$snapshot/post" || return 1
  cmp -s "$snapshot/pre/enabled-processes.txt" "$snapshot/post/enabled-processes.txt" || return 1
  cmp -s "$snapshot/pre/migrations.txt" "$snapshot/post/migrations.txt" || return 1
  wait_for_profile_recovery "$snapshot/post" || return 1
  check_ingester_capacity "$snapshot/post" || return 1
  verify_feed_advancement
}

rollback_release() {
  local reason="$1" rollback_epoch
  echo "Deployment failed: $reason; checking rollback compatibility." >&2
  kubectl -n "$namespace" logs deployment/polymarket-bot --since=5m --tail=300 > "$snapshot/bot-failure.log" 2>&1 || true
  if [[ "$component" == ingester ]]; then
    kubectl -n "$namespace" logs deployment/ingester-master --since=5m --tail=300 > "$snapshot/ingester-failure.log" 2>&1 || true
  fi
  kubectl -n "$namespace" exec timescaledb-0 -c timescaledb -- psql -U postgres -d polymarket -Atc \
    'SELECT count(*),coalesce(max(timestamp),0) FROM public.migrations' > "$snapshot/failure-migrations.txt" || {
    echo "Cannot verify migration state; automatic rollback is blocked. Preserve $snapshot." >&2
    return 1
  }
  cmp -s "$snapshot/pre/migrations.txt" "$snapshot/failure-migrations.txt" || {
    echo "Migration state changed; automatic rollback is blocked. Preserve $snapshot." >&2
    return 1
  }
  echo "Restoring Helm revision $rollback_revision." >&2
  rollback_epoch="$(date -u +%s)"
  if ! helm rollback "$component" "$rollback_revision" -n "$namespace" --wait --timeout 15m; then
    echo "Rollback failed; preserve $snapshot and inspect the affected release immediately." >&2
    return 1
  fi
  if [[ "$component" == ingester ]]; then
    verify_image_pods ingester-master "$rollback_image_id" "$rollback_image" || return 1
    verify_image_pods ingester-worker "$rollback_image_id" "$rollback_image" || return 1
  else
    verify_image_pods polymarket-bot "$rollback_image_id" "$rollback_image" || return 1
  fi
  verify_bot_routes || return 1
  ingester_snapshot "$snapshot/rollback" || return 1
  cmp -s "$snapshot/pre/enabled-processes.txt" "$snapshot/rollback/enabled-processes.txt" || return 1
  cmp -s "$snapshot/pre/migrations.txt" "$snapshot/rollback/migrations.txt" || return 1
  wait_for_profile_recovery "$snapshot/rollback" || return 1
  check_ingester_capacity "$snapshot/rollback" || return 1
  wait_for_process_recovery "$rollback_epoch" || return 1
  echo "Restored $component to $rollback_image ($rollback_image_id); chart pin still needs reconciliation." >&2
}

if [[ "$1" == rc ]]; then
  [[ "$rc_version" =~ ^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)-rc\.([1-9][0-9]*)$
    && "${rc_version%%-*}" == "${local_version%%-*}" ]] || {
    echo "RC version must share the candidate's major.minor.patch version." >&2; exit 64;
  }
  candidate_identity
  rc_image="capitonic/$component:$rc_version"
  if nerdctl --namespace k8s.io image inspect "$rc_image" >/dev/null 2>&1; then
    [[ "$(nerdctl --namespace k8s.io image inspect --format '{{ .Id }}' "$rc_image")" == "$image_id" ]] || {
      echo "RC tag already names different image bytes." >&2; exit 67;
    }
  else
    nerdctl --namespace k8s.io image tag "$image" "$rc_image"
  fi
  version="$rc_version"
  candidate_identity
fi

if [[ "$1" == pin || "$1" == rc ]]; then
  candidate_identity
  if [[ "$component" == ingester ]]; then
    IMAGE="$image" IMAGE_ID="$image_id" IMAGE_REVISION="$image_revision" \
      yq -i '.image = strenv(IMAGE) | .imageDigest = strenv(IMAGE_ID) | .gitRevision = strenv(IMAGE_REVISION)' "$values"
  else
    IMAGE="$image" yq -i '.image = strenv(IMAGE)' "$values"
  fi
  if [[ "$1" == rc ]]; then
    VERSION="$version" yq -i '.appVersion = strenv(VERSION)' "$chart/Chart.yaml"
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
  if cmp -s <(normalized_manifest exact "$snapshot/helm-manifest.yaml") \
    <(normalized_manifest exact "$snapshot/selected-manifest.yaml"); then
    echo "Ingester worker RollingUpdate is already installed; no Helm release was changed."
    exit 0
  fi
  rollout_epoch="$(date -u +%s)"
  if ! helm upgrade --install ingester "$chart" -n "$namespace" --wait --timeout 15m; then
    rollback_release "worker strategy upgrade failed" || exit 71
    exit 70
  fi
  if [[ "$(kubectl -n "$namespace" get deployment ingester-worker -o json | jq -r '.spec.strategy.type')" != RollingUpdate ]] ||
     [[ "$(live_image ingester-master)" != "$(chart_image)" || "$(live_image ingester-worker)" != "$(chart_image)" ]] ||
     ! ready_deployment ingester-master || ! ready_deployment ingester-worker ||
     ! verify_runtime_recovery; then
    rollback_release "worker strategy verification failed" || exit 71
    exit 70
  fi
  echo "Worker RollingUpdate is active; ingester image remains $(chart_image)."
  exit 0
fi

version="${2:-}"
if [[ "$1" == deploy ]]; then
  version="$(chart_image)"
  version="${version#capitonic/$component:}"
  candidate_identity
  [[ "$(chart_image)" == "$image" ]] || exit 67
  if [[ "$version" == *-rc.* ]]; then
    [[ "$(yq -r '.appVersion' "$chart/Chart.yaml")" == "$version" ]] || {
      echo "Chart appVersion disagrees with its RC image." >&2; exit 67;
    }
  fi
  if [[ "$component" == ingester ]]; then
    [[ "$(yq -r '.imageDigest' "$values")" == "$image_id" && "$(yq -r '.gitRevision' "$values")" == "$image_revision" ]] || {
      echo "Ingester chart image ID or revision does not match candidate provenance." >&2; exit 67;
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
  if cmp -s <(normalized_manifest exact "$snapshot/helm-manifest.yaml") \
    <(normalized_manifest exact "$snapshot/selected-manifest.yaml") &&
    [[ "$rollback_image" == "$image" && "$rollback_image_id" == "$image_id" ]]; then
    echo "Already deployed $image ($image_id); no Helm release was changed."
    exit 0
  fi
  rollout_epoch="$(date -u +%s)"
  if ! helm upgrade --install "$component" "$chart" -n "$namespace" --wait --timeout 15m; then
    rollback_release "Helm upgrade failed" || exit 71
    exit 70
  fi
  if [[ "$component" == ingester ]]; then
    if ! verify_image_pods ingester-master "$image_id" "$image" ||
       ! verify_image_pods ingester-worker "$image_id" "$image" ||
       ! verify_runtime_recovery || ! verify_feed_advancement; then
      rollback_release "ingester image or data-path verification failed" || exit 71
      exit 70
    fi
  else
    if ! verify_image_pods polymarket-bot "$image_id" "$image" || ! verify_bot_recovery; then
      rollback_release "bot image or process recovery verification failed" || exit 71
      exit 70
    fi
    if ! verify_shared_runtime; then
      echo "Shared ingester or migration checks failed after the bot rollout; attribution is unresolved, so the bot was not rolled back." >&2
      exit 71
    fi
  fi
  echo "Deployed $image ($image_id); source $image_revision; provenance $version_tag and $hash_tag."
fi
