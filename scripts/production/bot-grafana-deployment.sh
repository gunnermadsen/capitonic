#!/usr/bin/env bash
# Scoped rollout functions used by the owning deploy-k3s entry point.

bot_grafana_database_state() {
  kubectl -n "$NAMESPACE" exec timescaledb-0 -c timescaledb -- psql -U postgres -d polymarket -Atc \
    "SELECT 'migration',timestamp::text FROM public.migrations
     UNION ALL SELECT 'weather_migration',timestamp::text FROM public.weather_migrations
     UNION ALL SELECT 'process',process_id::text || '|' || enabled::text || '|' || md5(config::text)
       FROM polymarket.trading_processes ORDER BY 1,2"
}

bot_grafana_unaffected_workloads() {
  kubectl -n "$NAMESPACE" get deployments,statefulsets,jobs -o json | jq -S '
    [.items[] | select(.metadata.name != "polymarket-bot" and .metadata.name != "grafana") |
      {kind, name: .metadata.name, spec}] | sort_by(.kind,.name)'
}

bot_grafana_processes_healthy() {
  local minimum_epoch="$1" unhealthy
  unhealthy="$(kubectl -n "$NAMESPACE" exec timescaledb-0 -c timescaledb -- psql -U postgres -d polymarket -Atc \
    "SELECT count(*) FROM polymarket.trading_processes WHERE enabled AND
       (status IS DISTINCT FROM 'running' OR heartbeat_at IS NULL OR heartbeat_at < to_timestamp($minimum_epoch))")" || return 1
  [[ "$unhealthy" == 0 ]]
}

bot_grafana_recovered() {
  local minimum_epoch="$1" expected_image="$2" attempt
  kubectl -n "$NAMESPACE" get pods -l app.kubernetes.io/name=polymarket-bot -o json | \
    jq -e --arg image "$expected_image" '
      [.items[] | select(.metadata.deletionTimestamp == null)] as $pods |
      ($pods | length) > 0 and all($pods[];
        .spec.containers[0].image == $image and
        .status.containerStatuses[0].ready == true and .status.containerStatuses[0].restartCount == 0)' >/dev/null || return 1
  kubectl -n "$NAMESPACE" rollout status deployment/grafana --timeout=5m || return 1
  for ((attempt=0; attempt<24; attempt++)); do
    bot_grafana_processes_healthy "$minimum_epoch" && return 0
    sleep 5
  done
  return 1
}

bot_grafana_feature_healthy() {
  local snapshot="$1" token grafana_user grafana_password dashboard status=1 attempt
  local bot_forward grafana_forward
  token="$(kubectl -n "$NAMESPACE" get secret polymarket-bot-auth -o jsonpath='{.data.admin-token}' | base64 -d)"
  grafana_user="$(kubectl -n "$NAMESPACE" get secret grafana-auth -o jsonpath='{.data.admin-user}' | base64 -d)"
  grafana_password="$(kubectl -n "$NAMESPACE" get secret grafana-auth -o jsonpath='{.data.admin-password}' | base64 -d)"
  kubectl -n "$NAMESPACE" port-forward service/polymarket-bot 18097:8097 > "$snapshot/bot-port-forward.log" 2>&1 & bot_forward=$!
  kubectl -n "$NAMESPACE" port-forward service/grafana 13000:3000 > "$snapshot/grafana-port-forward.log" 2>&1 & grafana_forward=$!
  for ((attempt=0; attempt<30; attempt++)); do
    if curl -fsS -H "Authorization: Bearer $token" \
      'http://127.0.0.1:18097/admin/strategy/btc-5m/entry-status?scope=All%20processes' > "$snapshot/entry-all.json" 2>/dev/null &&
      dashboard="$(curl -fsS -u "$grafana_user:$grafana_password" \
        'http://127.0.0.1:13000/api/dashboards/uid/polymarket-bot' 2>/dev/null)"; then
      jq -e '.enabled_count >= 0 and .total_count >= .enabled_count and
        .alert_enabled == (if .total_count > 0 and .enabled_count == .total_count then 1 else 0 end)'
        "$snapshot/entry-all.json" >/dev/null &&
      jq -e '[.dashboard.panels[]|select(.id==25)] | length == 1' <<<"$dashboard" >/dev/null &&
      jq -e '[.dashboard.panels[]|select(.id==25)|.targets[].columns[]|.selector] |
        index("value") != null and index("text") != null and index("color") != null' <<<"$dashboard" >/dev/null &&
      jq -e '[.dashboard.panels[]|select(.id==25)|.transformations[].id] |
        index("configFromData") != null' <<<"$dashboard" >/dev/null && status=0
      break
    fi
    sleep 1
  done
  kill "$bot_forward" "$grafana_forward" 2>/dev/null || true
  wait "$bot_forward" "$grafana_forward" 2>/dev/null || true
  return "$status"
}

deploy_bot_grafana() {
  local snapshot rollout_epoch bot_revision grafana_revision rollback_image ready desired backfills
  local expected_image="$image" failed=false rollback_failed=false
  snapshot="$(mktemp -d /run/capitonic-bot-grafana.XXXXXX)"
  chmod 0700 "$snapshot"
  for release in polymarket-bot grafana; do
    helm -n "$NAMESPACE" get manifest "$release" > "$snapshot/$release-manifest.yaml"
    helm -n "$NAMESPACE" get values "$release" -a -o yaml > "$snapshot/$release-values.yaml"
    helm -n "$NAMESPACE" history "$release" -o json > "$snapshot/$release-history.json"
  done
  bot_revision="$(jq -er '[.[]|select(.status=="deployed")]|last|.revision' "$snapshot/polymarket-bot-history.json")"
  grafana_revision="$(jq -er '[.[]|select(.status=="deployed")]|last|.revision' "$snapshot/grafana-history.json")"
  rollback_image="$(kubectl -n "$NAMESPACE" get deployment polymarket-bot -o jsonpath='{.spec.template.spec.containers[0].image}')"
  kubectl -n "$NAMESPACE" get pods -o json > "$snapshot/pods.json"
  bot_grafana_database_state > "$snapshot/database-before.txt"
  bot_grafana_unaffected_workloads > "$snapshot/workloads-before.json"
  kubectl -n "$NAMESPACE" exec deployment/ingester-master -- sh -c \
    'curl -fsS -H "Authorization: Bearer $INGESTER_ADMIN_TOKEN" http://127.0.0.1:8098/ingesters' > "$snapshot/profiles.json"
  kubectl -n "$NAMESPACE" exec timescaledb-0 -c timescaledb -- psql -U postgres -d polymarket -Atc \
    "SELECT coalesce(sum(allocation_units),0) FROM ingester.backfill_jobs WHERE assigned_worker_id IS NOT NULL AND status IN ('running','cancel_requested')" > "$snapshot/backfills.txt"
  ready="$(kubectl -n "$NAMESPACE" get deployment ingester-worker -o jsonpath='{.status.readyReplicas}')"
  desired="$(jq '[.[]|select(.desired_state=="running")]|length' "$snapshot/profiles.json")"
  backfills="$(cat "$snapshot/backfills.txt")"
  (( ready >= desired + backfills )) || { echo "Insufficient worker capacity; no rollout started." >&2; return 70; }
  jq -e 'all(.[]|select(.desired_state=="running"); .observed_state=="running" and .health_status=="healthy" and .lease_owner!=null)' "$snapshot/profiles.json" >/dev/null
  bot_grafana_processes_healthy "$(($(date -u +%s)-90))" || { echo "Enabled processes are unhealthy before deployment." >&2; return 70; }
  echo "Bot/Grafana rollback snapshot: $snapshot"

  scripts/production/refresh-ecr-pull-secret.sh
  local prometheus_user
  prometheus_user="$(kubectl -n "$NAMESPACE" get secret prometheus-auth -o jsonpath='{.data.username}' | base64 -d)"
  PROMETHEUS_BASIC_AUTH_USER="$prometheus_user" python3 scripts/helm/bake-assets.py
  unset prometheus_user
  trap 'python3 scripts/helm/bake-assets.py --clean >/dev/null 2>&1 || true' EXIT
  rollout_epoch="$(date -u +%s)"
  deploy_chart polymarket-bot || failed=true
  if [[ "$failed" == false ]]; then deploy_chart grafana || failed=true; fi
  if [[ "$failed" == false ]]; then
    bot_grafana_recovered "$rollout_epoch" "$expected_image" || failed=true
  fi
  if [[ "$failed" == false ]]; then
    bot_grafana_feature_healthy "$snapshot" || failed=true
  fi
  if [[ "$failed" == true ]]; then
    kubectl -n "$NAMESPACE" logs deployment/polymarket-bot --since=5m --tail=300 > "$snapshot/bot-failure.log" 2>&1 || true
    kubectl -n "$NAMESPACE" logs deployment/grafana --since=5m --tail=100 > "$snapshot/grafana-failure.log" 2>&1 || true
    bot_grafana_database_state > "$snapshot/database-failure.txt"
    cmp -s "$snapshot/database-before.txt" "$snapshot/database-failure.txt" || {
      echo "Database state changed; rollback compatibility is unproven. Preserve $snapshot." >&2; return 71;
    }
    rollout_epoch="$(date -u +%s)"
    if [[ "$(helm -n "$NAMESPACE" history grafana -o json | jq -er '[.[]|select(.status=="deployed")]|last|.revision')" != "$grafana_revision" ]]; then
      helm rollback grafana "$grafana_revision" -n "$NAMESPACE" --wait --timeout 10m || rollback_failed=true
    fi
    if [[ "$(helm -n "$NAMESPACE" history polymarket-bot -o json | jq -er '[.[]|select(.status=="deployed")]|last|.revision')" != "$bot_revision" ]]; then
      helm rollback polymarket-bot "$bot_revision" -n "$NAMESPACE" --wait --timeout 15m || rollback_failed=true
    fi
    bot_grafana_recovered "$rollout_epoch" "$rollback_image" || rollback_failed=true
    echo "Affected rollout failed; rollback failed=$rollback_failed; preserve $snapshot." >&2
    return 71
  fi
  bot_grafana_database_state > "$snapshot/database-after.txt"
  bot_grafana_unaffected_workloads > "$snapshot/workloads-after.json"
  cmp -s "$snapshot/database-before.txt" "$snapshot/database-after.txt" && \
    cmp -s "$snapshot/workloads-before.json" "$snapshot/workloads-after.json" || {
      echo "Unrelated state changed; attribution unresolved. Preserve $snapshot; no unrelated rollback attempted." >&2; return 71;
    }
  # Reuse the existing read-only production checks; never reconcile processes here.
  scripts/production/verify-production.sh
  printf 'Bot/Grafana deployment verified at revision %s; rollback snapshot %s.\n' "$deployment_revision" "$snapshot"
}
