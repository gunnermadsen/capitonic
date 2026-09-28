#!/usr/bin/env bash
set -euo pipefail
set +x

NAMESPACE="${CAPITONIC_NAMESPACE:-capitonic}"
PORT="${INGESTER_LOCAL_PORT:-18098}"
allowed=(
  polymarket_btc_five_minute_market_contracts
  polymarket_btc_five_minute_orderbooks
  polymarket_btc_five_minute_resolutions
  binance_spot_btcusdt_one_second_ohlcv
)

token="$(kubectl -n "$NAMESPACE" get secret ingester-auth -o jsonpath='{.data.admin-token}' | base64 -d)"
[ -n "$token" ]
kubectl -n "$NAMESPACE" port-forward service/ingester-master "$PORT:8098" >/tmp/capitonic-ingester-port-forward.log 2>&1 &
forward_pid=$!
trap 'kill "$forward_pid" 2>/dev/null || true' EXIT
for attempt in $(seq 1 60); do
  curl -fsS "http://127.0.0.1:$PORT/health/ready" >/dev/null 2>&1 && break
  sleep 1
done

api() {
  local method="$1" path="$2"; shift 2
  curl -fsS -X "$method" -H "Authorization: Bearer $token" "$@" "http://127.0.0.1:$PORT$path"
}
is_allowed() {
  local candidate="$1" item
  for item in "${allowed[@]}"; do [[ "$candidate" == "$item" ]] && return 0; done
  return 1
}

profiles="$(api GET /ingesters)"
jq -e 'type == "array"' <<<"$profiles" >/dev/null
while IFS=$'\t' read -r key desired generation; do
  action=stop
  expected=stopped
  if is_allowed "$key"; then action=start; expected=running; fi
  [[ "$desired" == "$expected" ]] && continue
  api POST "/ingesters/$key/$action" -H "If-Match: $generation" >/dev/null
done < <(jq -r '.[] | [.strategy_key,.desired_state,(.desired_generation|tostring)] | @tsv' <<<"$profiles")

for attempt in $(seq 1 180); do
  profiles="$(api GET /ingesters)"
  if jq -e --argjson allowed "$(printf '%s\n' "${allowed[@]}" | jq -R . | jq -s .)" '
    ([.[] | select(.desired_state == "running") | .strategy_key] | sort) == ($allowed | sort) and
    all(.[] | select(.desired_state == "running");
      .observed_state == "running" and .health_status == "healthy" and .lease_owner != null)
  ' <<<"$profiles" >/dev/null; then
    ready_workers="$(kubectl -n "$NAMESPACE" get deployment ingester-worker -o jsonpath='{.status.readyReplicas}')"
    [[ "$ready_workers" == "4" ]] && {
      jq -c '[.[] | select(.desired_state == "running") | {strategy_key,lease_owner,health_status,source_watermark}]' <<<"$profiles"
      echo "Production ingestion allowlist converged with four healthy owners and no extra realtime profiles."
      exit 0
    }
  fi
  sleep 5
done

echo "Production ingestion profiles did not converge within 15 minutes." >&2
exit 1
