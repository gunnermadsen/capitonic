#!/usr/bin/env bash
set -euo pipefail
set +x

NAMESPACE="${CAPITONIC_NAMESPACE:-capitonic}"
APP_DIRECTORY="${APP_DIRECTORY:-/opt/polymarket-bot}"
PORT="${BOT_LOCAL_PORT:-18097}"
PAPER_DEFINITION="$APP_DIRECTORY/infra/processes/btc-5m-conservative-selective-confidence-075-paper-20261001.json"
LIVE_DEFINITION="$APP_DIRECTORY/infra/processes/btc-5m-conservative-selective-confidence-075-live-pilot-20261006.json"

jq -e '.config.execution.mode == "paper" and .config.execution.live_capital == false' "$PAPER_DEFINITION" >/dev/null
jq -e '
  .config.execution.mode == "live" and .config.execution.live_capital == false and
  .config.execution.execute_signals == false and .config.execution.max_order_notional_usd == "3" and
  .config.execution.max_open_notional_usd == "3" and .config.execution.max_open_positions == 1 and
  .config.execution.max_daily_loss_usd == "3" and .config.execution.require_exit_book == true and
  .config.execution.account_ref == "polymarket-primary"
' "$LIVE_DEFINITION" >/dev/null

token="$(kubectl -n "$NAMESPACE" get secret polymarket-bot-auth -o jsonpath='{.data.admin-token}' | base64 -d)"
[ -n "$token" ]
kubectl -n "$NAMESPACE" port-forward service/polymarket-bot "$PORT:8097" >/tmp/capitonic-bot-port-forward.log 2>&1 &
forward_pid=$!
trap 'kill "$forward_pid" 2>/dev/null || true' EXIT
for attempt in $(seq 1 90); do
  curl -fsS "http://127.0.0.1:$PORT/health/ready" >/dev/null 2>&1 && break
  sleep 1
done

api() {
  local method="$1" path="$2"; shift 2
  curl -fsS -X "$method" -H "Authorization: Bearer $token" "$@" "http://127.0.0.1:$PORT$path"
}
list_processes() { api GET '/admin/trading-processes?limit=100'; }
process_id_by_key() {
  local key="$1"
  list_processes | jq -r --arg key "$key" '.processes[] | select(.process_key == $key) | .process_id' | head -n1
}
upsert_if_missing() {
  local definition="$1" key process_id request expected_selection
  key="$(jq -r .process_key "$definition")"
  process_id="$(process_id_by_key "$key")"
  if [[ -z "$process_id" ]]; then
    request="$(jq 'del(.process_key)' "$definition")"
    process_id="$(api PUT "/admin/trading-processes/by-key/$key" -H 'Content-Type: application/json' --data "$request" | jq -r '.process.process_id')"
  fi
  [[ "$process_id" =~ ^[0-9a-f-]{36}$ ]]
  expected_selection="$(jq -c '.config.raw.btc_realtime_paper.strategy.decision_strategy.models[0].selection' "$definition")"
  api GET "/admin/trading-processes/$process_id" | jq -e --argjson selection "$expected_selection" '
    .process.config.raw.btc_realtime_paper.strategy.decision_strategy.models[0].selection == $selection
  ' >/dev/null
  printf '%s' "$process_id"
}
ensure_started() {
  local process_id="$1" process
  process="$(api GET "/admin/trading-processes/$process_id")"
  if [[ "$(jq -r '.process.enabled and .process.status == "running"' <<<"$process")" != "true" ]]; then
    api GET "/admin/trading-processes/$process_id/start-preview" >/dev/null
    api POST "/admin/trading-processes/$process_id/start" >/dev/null
  fi
}
wait_running() {
  local process_id="$1"
  for attempt in $(seq 1 180); do
    status="$(api GET "/admin/trading-processes/$process_id/status")"
    if jq -e '.status.process.enabled == true and .status.process.status == "running" and .status.btc_runtime.runtime.running == true and .status.btc_runtime.runtime.readiness.ready == true' <<<"$status" >/dev/null; then
      printf '%s' "$status"
      return 0
    fi
    sleep 5
  done
  echo "Process $process_id did not become ready." >&2
  return 1
}
wait_live_preflight() {
  local process_id="$1" preflight='{}'
  for attempt in $(seq 1 60); do
    if preflight="$(api POST "/admin/trading-processes/$process_id/live-preflight")" \
      && jq -e '.ready == true and .credential_connectivity_ready == true and .reconciliation_ready == true and .trading_disabled == true' \
        <<<"$preflight" >/dev/null; then
      printf '%s' "$preflight"
      return 0
    fi
    sleep 5
  done
  jq '{ready,credential_connectivity_ready,reconciliation_ready,trading_disabled,reasons,reconciliation_error}' \
    <<<"$preflight" >&2
  echo "Live preflight did not become ready for process $process_id." >&2
  return 1
}

paper_id="$(upsert_if_missing "$PAPER_DEFINITION")"
ensure_started "$paper_id"
paper_status="$(wait_running "$paper_id")"
paper_expected_sha="$(jq -r .metadata.model_artifact_sha256 "$PAPER_DEFINITION")"
jq -e --arg sha "$paper_expected_sha" '.process.metadata.model_artifact_sha256 == $sha' \
  <<<"$(api GET "/admin/trading-processes/$paper_id")" >/dev/null

token_id="$(jq -r '.status.btc_runtime.runtime.readiness.books[] | select(.bootstrapped == true and (.best_ask != null or .best_bid != null)) | .token_id' <<<"$paper_status" | head -n1)"
[ -n "$token_id" ]
dry_run="$(api POST /admin/live/order-dry-run -H 'Content-Type: application/json' \
  --data "$(jq -n --arg token "$token_id" '{token_id:$token,side:"buy",order_type:"fak",price:"0.50",size:"1"}')")"
jq -e '.mode == "live" and .owner_redacted == true and .signature_redacted == true and (.signed_order | type == "object")' <<<"$dry_run" >/dev/null


live_id="$(upsert_if_missing "$LIVE_DEFINITION")"
live_process="$(api GET "/admin/trading-processes/$live_id")"
if [[ "$(jq -r '.process.enabled' <<<"$live_process")" != "true" ]]; then
  preflight="$(wait_live_preflight "$live_id")"

  authorization_patch="$(jq '{
    config: (.config | .execution.execute_signals = true | .execution.live_capital = true),
    metadata: (.metadata | .credential_validation_only = false |
      .live_execution_authorized = true)
  }' "$LIVE_DEFINITION")"
  api PATCH "/admin/trading-processes/$live_id" -H 'Content-Type: application/json' --data "$authorization_patch" >/dev/null
  ensure_started "$live_id"
fi

live_status="$(wait_running "$live_id")"

entry_status="$(api POST "/admin/trading-processes/$live_id/live/entries/enable")"
jq -e '
  .entries_enabled == true and .order_submit_enabled == true and .process_accounting_proven == true and
  .idempotency_clean == true and .unresolved_live_order_count == 0 and
  .max_order_notional_usd == "3" and .max_open_notional_usd == "3"
' <<<"$entry_status" >/dev/null

live_expected_sha="$(jq -r .metadata.model_artifact_sha256 "$LIVE_DEFINITION")"
live_status="$(api GET "/admin/trading-processes/$live_id/status")"
jq -e --arg sha "$live_expected_sha" '.process.metadata.model_artifact_sha256 == $sha' \
  <<<"$(api GET "/admin/trading-processes/$live_id")" >/dev/null
jq -e --arg sha "$live_expected_sha" '
  .status.process.enabled == true and .status.process.status == "running" and
  .status.btc_runtime.live_status.entries_enabled == true and
  .status.btc_runtime.live_status.order_submit_enabled == true and
  .status.btc_runtime.live_status.process_accounting_proven == true
' <<<"$live_status" >/dev/null

jq -n --arg paper_process_id "$paper_id" --arg live_process_id "$live_id" \
  --arg paper_model_sha256 "$paper_expected_sha" --arg live_model_sha256 "$live_expected_sha" \
  '{paper:{process_id:$paper_process_id,model_sha256:$paper_model_sha256,status:"running"},live:{process_id:$live_process_id,model_sha256:$live_model_sha256,status:"running",entries_enabled:true,signing_dry_run:true}}'
