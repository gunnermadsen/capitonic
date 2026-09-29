#!/usr/bin/env bash
# Sourced by deploy-k3s.sh after DEPLOY_COMPONENTS has been validated.

selected_database_state() {
  kubectl -n "$NAMESPACE" exec timescaledb-0 -c timescaledb -- psql -U postgres -d polymarket -Atc \
    "SELECT 'migration',timestamp::text FROM public.migrations
     UNION ALL SELECT 'weather_migration',timestamp::text FROM public.weather_migrations
     UNION ALL SELECT 'process',process_id::text || '|' || enabled::text || '|' || md5(config::text)
       FROM polymarket.trading_processes ORDER BY 1,2"
}

selected_unaffected_workloads() {
  local names="$1"
  kubectl -n "$NAMESPACE" get deployments,statefulsets,jobs -o json | jq -S --arg names ",$names," '
    [.items[] | (.metadata.labels["app.kubernetes.io/instance"] // "") as $instance |
      select(($names | contains("," + $instance + ",")) | not) |
      {kind, name: .metadata.name, spec}] | sort_by(.kind,.name)'
}

selected_bot_ready() {
  local expected_image="$1"
  kubectl -n "$NAMESPACE" rollout status deployment/polymarket-bot --timeout=5m || return 1
  kubectl -n "$NAMESPACE" get pods -l app.kubernetes.io/name=polymarket-bot -o json | jq -e --arg image "$expected_image" '
    [.items[] | select(.metadata.deletionTimestamp == null)] as $pods |
    ($pods | length) > 0 and all($pods[];
      .spec.containers[0].image == $image and
      .status.containerStatuses[0].ready == true and .status.containerStatuses[0].restartCount == 0)' >/dev/null
}

selected_bot_feature() {
  local snapshot="$1" token status=1 attempt forward
  token="$(kubectl -n "$NAMESPACE" get secret polymarket-bot-auth -o jsonpath='{.data.admin-token}' | base64 -d)"
  kubectl -n "$NAMESPACE" port-forward service/polymarket-bot 18097:8097 > "$snapshot/bot-port-forward.log" 2>&1 & forward=$!
  for ((attempt=0; attempt<30; attempt++)); do
    if curl -fsS -H "Authorization: Bearer $token" \
      'http://127.0.0.1:18097/admin/strategy/btc-5m/entry-status?scope=All%20processes' > "$snapshot/entry-all.json" 2>/dev/null &&
      jq -e '.enabled_count >= 0 and .total_count >= .enabled_count and
        .alert_enabled == (if .total_count > 0 and .enabled_count == .total_count then 1 else 0 end)' "$snapshot/entry-all.json" >/dev/null; then
      status=0
      break
    fi
    sleep 1
  done
  kill "$forward" 2>/dev/null || true
  wait "$forward" 2>/dev/null || true
  return "$status"
}

selected_grafana_feature() {
  local snapshot="$1" user password dashboard status=1 attempt forward
  user="$(kubectl -n "$NAMESPACE" get secret grafana-auth -o jsonpath='{.data.admin-user}' | base64 -d)"
  password="$(kubectl -n "$NAMESPACE" get secret grafana-auth -o jsonpath='{.data.admin-password}' | base64 -d)"
  kubectl -n "$NAMESPACE" port-forward service/grafana 13000:3000 > "$snapshot/grafana-port-forward.log" 2>&1 & forward=$!
  for ((attempt=0; attempt<30; attempt++)); do
    dashboard="$(curl -fsS -u "$user:$password" \
      'http://127.0.0.1:13000/api/dashboards/uid/polymarket-bot' 2>/dev/null)" || dashboard=''
    if jq -e '[.dashboard.panels[]|select(.id==25)] | length == 1' <<<"$dashboard" >/dev/null 2>&1 &&
      jq -e '[.dashboard.panels[]|select(.id==25)|.targets[].columns[]|.selector] |
        index("value") != null and index("text") != null and index("color") != null' <<<"$dashboard" >/dev/null &&
      jq -e '[.dashboard.panels[]|select(.id==25)|.transformations[].id] |
        index("configFromData") != null' <<<"$dashboard" >/dev/null; then
      status=0
      break
    fi
    sleep 1
  done
  kill "$forward" 2>/dev/null || true
  wait "$forward" 2>/dev/null || true
  return "$status"
}

selected_profile_state() {
  kubectl -n "$NAMESPACE" exec timescaledb-0 -c timescaledb -- psql -U postgres -d polymarket -Atc \
    "SELECT coalesce(json_agg(json_build_object(
       'strategy_key',strategy_key,'observed_state',observed_state,
       'health_status',health_status,'lease_owner',lease_owner)
       ORDER BY strategy_key),'[]'::json)
     FROM ingester.profiles WHERE desired_state='running'" | jq '.'
}

selected_healthy_processes() {
  local minimum_epoch="$1"
  kubectl -n "$NAMESPACE" exec timescaledb-0 -c timescaledb -- psql -U postgres -d polymarket -Atc \
    "SELECT process_id FROM polymarket.trading_processes WHERE enabled AND status='running'
       AND heartbeat_at >= to_timestamp($minimum_epoch) ORDER BY process_id"
}

selected_migrations_unchanged() {
  local directory="$1" ledger="$2" source applied
  local -a directories=()
  read -ra directories <<< "$directory"
  source="$(find "${directories[@]}" -maxdepth 1 -type f -name '[0-9]*.ts' -exec basename {} \; | cut -d- -f1 | sort -n)"
  applied="$(kubectl -n "$NAMESPACE" exec timescaledb-0 -c timescaledb -- psql -U postgres -d polymarket -Atc \
    "SELECT timestamp FROM public.$ledger ORDER BY timestamp")" || return 1
  [[ -z "$(comm -3 <(printf '%s\n' "$source" | sed '/^$/d' | sort -n) \
    <(printf '%s\n' "$applied" | sed '/^$/d' | sort -n))" ]]
}

selected_verify_component() {
  local component="$1" snapshot="$2" expected_image
  case "$component" in
    polymarket-bot)
      expected_image="$(yq -r .image capitonic-helm-chart/environments/production/polymarket-bot.yaml)"
      selected_bot_ready "$expected_image" && selected_bot_feature "$snapshot"
      ;;
    grafana)
      kubectl -n "$NAMESPACE" rollout status deployment/grafana --timeout=5m && selected_grafana_feature "$snapshot"
      ;;
    ingester)
      expected_image="$(yq -r .image capitonic-helm-chart/environments/production/ingester.yaml)"
      kubectl -n "$NAMESPACE" rollout status deployment/ingester-master --timeout=5m &&
        kubectl -n "$NAMESPACE" rollout status deployment/ingester-worker --timeout=5m &&
        [[ "$(kubectl -n "$NAMESPACE" get deployment ingester-master -o jsonpath='{.spec.template.spec.containers[0].image}')" == "$expected_image" ]] &&
        [[ "$(kubectl -n "$NAMESPACE" get deployment ingester-worker -o jsonpath='{.spec.template.spec.containers[0].image}')" == "$expected_image" ]]
      ;;
    db-migrate)
      kubectl -n "$NAMESPACE" wait --for=condition=complete job/db-migrate --timeout=5m
      ;;
    prometheus|loki|alloy)
      kubectl -n "$NAMESPACE" rollout status "deployment/$component" --timeout=5m
      ;;
  esac
}

deploy_selected_components() {
  local snapshot component revision rollout_epoch failed=false rollback_failed=false ready desired backfills
  local -a selected=() deployed=()
  IFS=, read -ra selected <<< "$DEPLOY_COMPONENTS"
  ((${#selected[@]})) || { echo 'No production components selected.' >&2; return 64; }
  for component in "${selected[@]}"; do
    case "$component" in
      polymarket-bot|ingester|db-migrate|grafana|prometheus|loki|alloy) ;;
      *) echo "Unsupported deployment component: $component" >&2; return 64 ;;
    esac
  done
  snapshot="$(mktemp -d /run/capitonic-selected.XXXXXX)"
  chmod 0700 "$snapshot"
  for component in "${selected[@]}"; do
    helm -n "$NAMESPACE" history "$component" -o json > "$snapshot/$component-history.json"
    helm -n "$NAMESPACE" get manifest "$component" > "$snapshot/$component-manifest.yaml"
    revision="$(jq -er '[.[]|select(.status=="deployed")]|last|.revision' "$snapshot/$component-history.json")"
    printf '%s\n' "$revision" > "$snapshot/$component-revision"
  done
  selected_unaffected_workloads "$DEPLOY_COMPONENTS" > "$snapshot/workloads-before.json"
  if [[ ",$DEPLOY_COMPONENTS," == *,polymarket-bot,* || ",$DEPLOY_COMPONENTS," == *,db-migrate,* ]]; then
    selected_database_state > "$snapshot/database-before.txt"
  fi
  if [[ ",$DEPLOY_COMPONENTS," == *,polymarket-bot,* ]]; then
    selected_healthy_processes "$(($(date -u +%s)-90))" > "$snapshot/healthy-processes-before.txt"
  fi
  if [[ ",$DEPLOY_COMPONENTS," == *,ingester,* || ",$DEPLOY_COMPONENTS," == *,polymarket-bot,* ]]; then
    selected_profile_state > "$snapshot/profile-health-before.json"
    ready="$(kubectl -n "$NAMESPACE" get deployment ingester-worker -o jsonpath='{.status.readyReplicas}')"
    desired="$(jq length "$snapshot/profile-health-before.json")"
    backfills="$(kubectl -n "$NAMESPACE" exec timescaledb-0 -c timescaledb -- psql -U postgres -d polymarket -Atc \
      "SELECT coalesce(sum(allocation_units),0) FROM ingester.backfill_jobs WHERE assigned_worker_id IS NOT NULL AND status IN ('running','cancel_requested')")"
    (( ready >= desired + backfills )) || { echo 'Insufficient worker capacity; no rollout started.' >&2; return 70; }
  fi
  if [[ ",$DEPLOY_COMPONENTS," == *,db-migrate,* ]]; then
    selected_migrations_unchanged 'packages/db-migrate/src/migrations packages/db-migrate/src/fresh-install' migrations &&
      selected_migrations_unchanged packages/db-migrate/src/migrations/weather weather_migrations || {
        echo 'Pending or unexpected migrations require their separately approved exact migration set; no rollout started.' >&2
        return 70
      }
  fi
  echo "Selected production components: $DEPLOY_COMPONENTS; rollback snapshot: $snapshot"

  if [[ ",$DEPLOY_COMPONENTS," == *,polymarket-bot,* || ",$DEPLOY_COMPONENTS," == *,ingester,* ]]; then
    scripts/production/refresh-ecr-pull-secret.sh
  fi
  if [[ ",$DEPLOY_COMPONENTS," == *,grafana,* || ",$DEPLOY_COMPONENTS," == *,prometheus,* || ",$DEPLOY_COMPONENTS," == *,loki,* || ",$DEPLOY_COMPONENTS," == *,alloy,* ]]; then
    local prometheus_user
    prometheus_user="$(kubectl -n "$NAMESPACE" get secret prometheus-auth -o jsonpath='{.data.username}' | base64 -d)"
    PROMETHEUS_BASIC_AUTH_USER="$prometheus_user" python3 scripts/helm/bake-assets.py
    unset prometheus_user
    trap 'python3 scripts/helm/bake-assets.py --clean >/dev/null 2>&1 || true' EXIT
  fi
  rollout_epoch="$(date -u +%s)"
  for component in "${selected[@]}"; do
    if [[ "$component" == db-migrate ]]; then
      kubectl -n "$NAMESPACE" delete job db-migrate --ignore-not-found --wait=true
    fi
    if deploy_chart "$component"; then
      deployed+=("$component")
    else
      failed=true
      [[ "$component" == db-migrate ]] && deployed+=("$component")
      break
    fi
  done
  if [[ "$failed" == false ]]; then
    for component in "${selected[@]}"; do
      selected_verify_component "$component" "$snapshot" || { failed=true; break; }
    done
  fi
  if [[ "$failed" == false && -f "$snapshot/healthy-processes-before.txt" ]]; then
    local attempt missing
    for ((attempt=0; attempt<24; attempt++)); do
      selected_healthy_processes "$rollout_epoch" > "$snapshot/healthy-processes-after.txt" || { failed=true; break; }
      missing="$(comm -23 "$snapshot/healthy-processes-before.txt" "$snapshot/healthy-processes-after.txt")"
      [[ -z "$missing" ]] && break
      sleep 5
    done
    [[ -z "${missing:-}" ]] || failed=true
  fi
  if [[ "$failed" == true ]]; then
    for ((i=${#deployed[@]}-1; i>=0; i--)); do
      component="${deployed[i]}"
      revision="$(cat "$snapshot/$component-revision")"
      [[ "$component" == db-migrate ]] && kubectl -n "$NAMESPACE" delete job db-migrate --ignore-not-found --wait=true
      helm rollback "$component" "$revision" -n "$NAMESPACE" --wait --timeout 15m || rollback_failed=true
    done
    echo "Selected rollout failed; rollback failed=$rollback_failed; snapshot=$snapshot" >&2
    return 71
  fi
  selected_unaffected_workloads "$DEPLOY_COMPONENTS" > "$snapshot/workloads-after.json"
  cmp -s "$snapshot/workloads-before.json" "$snapshot/workloads-after.json" || {
    echo "Unrelated workload changed during rollout; preserve $snapshot." >&2; return 71;
  }
  if [[ -f "$snapshot/database-before.txt" ]]; then
    selected_database_state > "$snapshot/database-after.txt"
    cmp -s "$snapshot/database-before.txt" "$snapshot/database-after.txt" || {
      echo "Migration or durable process configuration changed; preserve $snapshot." >&2; return 71;
    }
  fi
  if [[ -f "$snapshot/profile-health-before.json" ]]; then
    selected_profile_state > "$snapshot/profile-health-after.json"
    echo "Before and after ingester profile health recorded in $snapshot; unrelated profile health did not veto deployment."
  fi
  printf 'Selected production components verified at revision %s; snapshot %s.\n' "$deployment_revision" "$snapshot"
}
