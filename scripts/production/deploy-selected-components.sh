#!/usr/bin/env bash
# Sourced by deploy-k3s.sh. The selected releases are the entire mutation scope.

selected_processes_healthy_since() {
  kubectl -n "$NAMESPACE" exec timescaledb-0 -c timescaledb -- psql -U postgres -d polymarket -Atc \
    "SELECT process_id FROM polymarket.trading_processes WHERE enabled AND status='running'
       AND heartbeat_at >= to_timestamp($1) ORDER BY process_id"
}

selected_migrations_unchanged() {
  local directories="$1" ledger="$2" source applied
  source="$(find $directories -maxdepth 1 -type f -name '[0-9]*.ts' -exec basename {} \; | cut -d- -f1 | sort -n)"
  applied="$(kubectl -n "$NAMESPACE" exec timescaledb-0 -c timescaledb -- psql -U postgres -d polymarket -Atc \
    "SELECT timestamp FROM public.$ledger ORDER BY timestamp")" || return 1
  [[ -z "$(comm -3 <(printf '%s\n' "$source" | sed '/^$/d') <(printf '%s\n' "$applied" | sed '/^$/d'))" ]]
}

selected_release_ready() {
  local component="$1" expected_image
  case "$component" in
    polymarket-bot)
      expected_image="$(yq -r .image capitonic-helm-chart/environments/production/polymarket-bot.yaml)"
      kubectl -n "$NAMESPACE" rollout status deployment/polymarket-bot --timeout=5m &&
        [[ "$(kubectl -n "$NAMESPACE" get deployment polymarket-bot -o jsonpath='{.spec.template.spec.containers[0].image}')" == "$expected_image" ]]
      ;;
    ingester)
      expected_image="$(yq -r .image capitonic-helm-chart/environments/production/ingester.yaml)"
      kubectl -n "$NAMESPACE" rollout status deployment/ingester-master --timeout=5m &&
        kubectl -n "$NAMESPACE" rollout status deployment/ingester-worker --timeout=5m &&
        [[ "$(kubectl -n "$NAMESPACE" get deployment ingester-master -o jsonpath='{.spec.template.spec.containers[0].image}')" == "$expected_image" ]] &&
        [[ "$(kubectl -n "$NAMESPACE" get deployment ingester-worker -o jsonpath='{.spec.template.spec.containers[0].image}')" == "$expected_image" ]]
      ;;
    db-migrate)
      expected_image="$(yq -r .image capitonic-helm-chart/environments/production/db-migrate.yaml)"
      kubectl -n "$NAMESPACE" wait --for=condition=complete job/db-migrate --timeout=5m &&
        [[ "$(kubectl -n "$NAMESPACE" get job db-migrate -o jsonpath='{.spec.template.spec.containers[0].image}')" == "$expected_image" ]]
      ;;
    grafana) kubectl -n "$NAMESPACE" rollout status deployment/grafana --timeout=5m ;;
    prometheus|loki) kubectl -n "$NAMESPACE" rollout status "statefulset/$component" --timeout=5m ;;
    alloy) kubectl -n "$NAMESPACE" rollout status daemonset/alloy --timeout=5m ;;
    cloudflared) kubectl -n "$NAMESPACE" rollout status deployment/cloudflared --timeout=5m ;;
  esac
}

deploy_selected_components() {
  local component revision snapshot started missing ready desired backfills attempt tunnel_id failed=false rollback_failed=false
  local -a selected=() deployed=() chart_options=()
  if [[ ",$DEPLOY_COMPONENTS," == *,polymarket-bot,* || ",$DEPLOY_COMPONENTS," == *,ingester,* || ",$DEPLOY_COMPONENTS," == *,db-migrate,* ]]; then
    scripts/production/refresh-ecr-pull-secret.sh
  fi
  if [[ ",$DEPLOY_COMPONENTS," == *,ingester,* || ",$DEPLOY_COMPONENTS," == *,polymarket-bot,* ]]; then
    scripts/production/reconcile-application-charts.sh
  fi
  IFS=, read -ra requested <<< "$DEPLOY_COMPONENTS"
  for component in "${requested[@]}"; do
    case "$component" in ingester|polymarket-bot) ;; *) selected+=("$component") ;; esac
  done
  ((${#selected[@]})) || return 0
  ((${#selected[@]})) || { echo 'No production components selected.' >&2; return 64; }
  for component in "${selected[@]}"; do
    case "$component" in
      polymarket-bot|ingester|db-migrate|grafana|prometheus|loki|alloy|cloudflared) ;;
      *) echo "Unsupported deployment component: $component" >&2; return 64 ;;
    esac
  done

  snapshot="$(mktemp -d /run/capitonic-selected.XXXXXX)"
  chmod 0700 "$snapshot"
  for component in "${selected[@]}"; do
    helm -n "$NAMESPACE" get manifest "$component" > "$snapshot/$component-manifest.yaml"
    helm -n "$NAMESPACE" history "$component" -o json | jq -er \
      '[.[]|select(.status=="deployed")]|last|.revision' > "$snapshot/$component-revision"
  done
  if [[ ",$DEPLOY_COMPONENTS," == *,db-migrate,* ]]; then
    selected_migrations_unchanged 'packages/db-migrate/src/migrations packages/db-migrate/src/fresh-install' migrations &&
      selected_migrations_unchanged packages/db-migrate/src/migrations/weather weather_migrations || {
        echo 'Pending or unexpected migrations need their separate approval; no rollout started.' >&2; return 70;
      }
  fi
  if [[ ",$DEPLOY_COMPONENTS," == *,polymarket-bot,* ]]; then
    selected_processes_healthy_since "$(($(date -u +%s)-90))" > "$snapshot/healthy-processes-before.txt"
  fi
  if [[ ",$DEPLOY_COMPONENTS," == *,ingester,* ]]; then
    ready="$(kubectl -n "$NAMESPACE" get deployment ingester-worker -o jsonpath='{.status.readyReplicas}')"
    desired="$(kubectl -n "$NAMESPACE" exec timescaledb-0 -c timescaledb -- psql -U postgres -d polymarket -Atc \
      "SELECT count(*) FROM ingester.profiles WHERE desired_state='running'")"
    backfills="$(kubectl -n "$NAMESPACE" exec timescaledb-0 -c timescaledb -- psql -U postgres -d polymarket -Atc \
      "SELECT coalesce(sum(allocation_units),0) FROM ingester.backfill_jobs WHERE assigned_worker_id IS NOT NULL AND status IN ('running','cancel_requested')")"
    (( ready >= desired + backfills )) || { echo 'Insufficient worker capacity for ingester rollout.' >&2; return 70; }
  fi
  echo "Deploying only: $DEPLOY_COMPONENTS; rollback snapshot: $snapshot"

  if [[ ",$DEPLOY_COMPONENTS," == *,grafana,* || ",$DEPLOY_COMPONENTS," == *,prometheus,* || ",$DEPLOY_COMPONENTS," == *,loki,* || ",$DEPLOY_COMPONENTS," == *,alloy,* ]]; then
    local prometheus_user
    prometheus_user="$(kubectl -n "$NAMESPACE" get secret prometheus-auth -o jsonpath='{.data.username}' | base64 -d)"
    PROMETHEUS_BASIC_AUTH_USER="$prometheus_user" python3 scripts/helm/bake-assets.py
    unset prometheus_user
    trap 'python3 scripts/helm/bake-assets.py --clean >/dev/null 2>&1 || true' EXIT
  fi
  started="$(date -u +%s)"
  for component in "${selected[@]}"; do
    [[ "$component" == db-migrate ]] && kubectl -n "$NAMESPACE" delete job db-migrate --ignore-not-found --wait=true
    if [[ "$component" == cloudflared ]]; then
      tunnel_id="$(kubectl -n "$NAMESPACE" get secret cloudflare-tunnel -o jsonpath='{.data.credentials\.json}' | base64 -d | jq -r '.TunnelID // .tunnelID // .tunnel_id')"
      [[ "$tunnel_id" =~ ^[0-9a-f-]{36}$ ]] || { echo 'Cloudflare tunnel ID is invalid.' >&2; return 70; }
      chart_options=(--set-string "tunnelId=$tunnel_id")
    else
      chart_options=()
    fi
    if deploy_chart "$component" "${chart_options[@]}"; then
      deployed+=("$component")
    else
      failed=true
      [[ "$component" == db-migrate ]] && deployed+=("$component")
      break
    fi
    selected_release_ready "$component" || { failed=true; break; }
  done
  if [[ "$failed" == false && -f "$snapshot/healthy-processes-before.txt" ]]; then
    for ((attempt=0; attempt<24; attempt++)); do
      selected_processes_healthy_since "$started" > "$snapshot/healthy-processes-after.txt" || { failed=true; break; }
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
    echo "Selected rollout failed; rollback failed=$rollback_failed; preserve $snapshot." >&2
    return 71
  fi
  printf 'Selected releases verified at revision %s; snapshot %s.\n' "$deployment_revision" "$snapshot"
}
