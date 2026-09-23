use super::*;

pub(super) fn stateful_age_seconds(
    value: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> Option<i64> {
    value.and_then(|value| (value <= now).then(|| (now - value).num_seconds()))
}

pub(super) fn bounded_live_gate_reason(reason: Option<&str>, fallback: &str) -> String {
    let reason = reason
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback);
    reason.chars().take(128).collect()
}

pub(super) fn next_clob_reconciliation_cursor(
    next_cursor: &str,
    seen_cursors: &mut HashSet<String>,
    resource: &str,
) -> Result<Option<String>> {
    if next_cursor == CLOB_TERMINAL_CURSOR {
        return Ok(None);
    }
    if next_cursor.is_empty()
        || next_cursor.len() > MAX_CLOB_CURSOR_BYTES
        || next_cursor.trim() != next_cursor
        || !next_cursor.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '+' | '/' | '=' | '-' | '_')
        })
        || !seen_cursors.insert(next_cursor.to_string())
    {
        bail!("Polymarket CLOB {resource} returned an invalid pagination cursor");
    }
    Ok(Some(next_cursor.to_string()))
}

pub(super) fn normalize_blank_taker_counterparty_fees(payload: &mut Value) -> usize {
    let Some(trades) = payload.get_mut("data").and_then(Value::as_array_mut) else {
        return 0;
    };
    let mut normalized = 0;
    for trade in trades {
        let authenticated_user_is_taker = trade
            .get("trader_side")
            .or_else(|| trade.get("traderSide"))
            .and_then(Value::as_str)
            .is_some_and(|side| side.eq_ignore_ascii_case("taker"));
        if !authenticated_user_is_taker {
            continue;
        }
        let maker_orders_key = if trade.get("maker_orders").is_some() {
            "maker_orders"
        } else {
            "makerOrders"
        };
        let Some(maker_orders) = trade
            .get_mut(maker_orders_key)
            .and_then(Value::as_array_mut)
        else {
            continue;
        };
        for maker_order in maker_orders {
            let fee_rate_key = if maker_order.get("fee_rate_bps").is_some() {
                "fee_rate_bps"
            } else {
                "feeRateBps"
            };
            let Some(fee_rate_bps) = maker_order.get_mut(fee_rate_key) else {
                continue;
            };
            if fee_rate_bps.as_str() == Some("") {
                *fee_rate_bps = Value::String("0".to_string());
                normalized += 1;
            }
        }
    }
    normalized
}

pub(super) fn clob_l2_signature(secret: &str, message: &str) -> Result<String> {
    let decoded_secret = URL_SAFE
        .decode(secret)
        .context("failed to decode CLOB API secret")?;
    let mut mac = Hmac::<Sha256>::new_from_slice(&decoded_secret)
        .context("failed to initialize CLOB API signature")?;
    mac.update(message.as_bytes());
    Ok(URL_SAFE.encode(mac.finalize().into_bytes()))
}

pub(super) fn validate_clob_page_metadata(
    resource: &str,
    data_len: usize,
    count: u64,
    limit: u64,
) -> Result<()> {
    let data_len = u64::try_from(data_len).context("Polymarket CLOB page length overflow")?;
    if limit == 0
        || limit > MAX_CLOB_RECONCILIATION_ROWS as u64
        || count != data_len
        || count > limit
    {
        bail!("Polymarket CLOB {resource} returned inconsistent pagination metadata");
    }
    Ok(())
}

pub(super) fn checked_clob_row_count(
    current: usize,
    page_len: usize,
    resource: &str,
) -> Result<usize> {
    let total = current
        .checked_add(page_len)
        .context("Polymarket CLOB reconciliation row count overflow")?;
    if total > MAX_CLOB_RECONCILIATION_ROWS {
        bail!(
            "Polymarket CLOB {resource} exceed the bounded {}-row reconciliation window",
            MAX_CLOB_RECONCILIATION_ROWS
        );
    }
    Ok(total)
}

pub(super) fn next_user_ws_reconnect_delay(current: Duration) -> Duration {
    current
        .checked_mul(2)
        .unwrap_or(USER_WS_RECONNECT_MAX_DELAY)
        .min(USER_WS_RECONNECT_MAX_DELAY)
}

pub(super) fn record_global_entry_halt(gate: &mut GlobalLiveEntryGate, reason: &str) -> u64 {
    gate.halted = true;
    if let Some(next_generation) = gate.safety_generation.checked_add(1) {
        gate.safety_generation = next_generation;
        gate.reason = bounded_live_gate_reason(Some(reason), "live_global_halt");
    } else {
        gate.reason = "safety_generation_exhausted".to_string();
    }
    gate.safety_generation
}

pub(super) fn commit_checked_live_enable(
    gate: &mut GlobalLiveEntryGate,
    state: &mut LiveVenueState,
    expected_safety_generation: u64,
) -> bool {
    if expected_safety_generation == u64::MAX
        || gate.safety_generation != expected_safety_generation
        || state.reconciled_safety_generation != Some(expected_safety_generation)
    {
        return false;
    }
    state.manual_entries_enabled = true;
    state.manual_entries_reason = None;
    if gate.halted {
        gate.halted = false;
        gate.reason = "account_checked_enable".to_string();
    }
    true
}

pub(super) fn commit_live_post_attempt(
    gate: &mut GlobalLiveEntryGate,
    state: &mut LiveVenueState,
    admitted_safety_generation: u64,
) -> Option<LiveExecutionGateReason> {
    if admitted_safety_generation == u64::MAX
        || gate.halted
        || gate.safety_generation != admitted_safety_generation
        || state.reconciled_safety_generation != Some(admitted_safety_generation)
        || !state.manual_entries_enabled
    {
        return Some(LiveExecutionGateReason::GlobalHalt);
    }
    None
}

pub(super) fn user_ws_heartbeat_ack_timed_out(
    awaiting_pong_elapsed: Option<Duration>,
    timeout: Duration,
) -> bool {
    awaiting_pong_elapsed.is_some_and(|elapsed| elapsed >= timeout)
}

pub(super) fn live_capital_exposure_gate(
    resulting_exposure: Decimal,
    max_open_notional_usd: Option<Decimal>,
) -> Option<LiveExecutionGateReason> {
    if max_open_notional_usd.is_some_and(|maximum| resulting_exposure > maximum) {
        Some(LiveExecutionGateReason::OpenNotionalLimit)
    } else {
        None
    }
}

pub(super) fn signer_address_from_private_key(private_key: Option<&str>) -> Result<Option<String>> {
    private_key
        .map(|private_key| {
            LocalSigner::from_str(private_key)
                .context("failed to parse POLYMARKET_PRIVATE_KEY")
                .map(|signer| {
                    signer
                        .with_chain_id(Some(POLYGON))
                        .address()
                        .to_checksum(None)
                })
        })
        .transpose()
}

pub(super) async fn check_relayer_deployed(url: &str) -> Result<bool> {
    let value: Value = reqwest::Client::new()
        .get(url)
        .send()
        .await
        .with_context(|| format!("failed to request relayer deployment status from {url}"))?
        .error_for_status()
        .with_context(|| format!("relayer deployment status request failed for {url}"))?
        .json()
        .await
        .with_context(|| format!("failed to decode relayer deployment status from {url}"))?;

    if let Some(deployed) = value.as_bool() {
        return Ok(deployed);
    }
    if let Some(deployed) = value.get("deployed").and_then(Value::as_bool) {
        return Ok(deployed);
    }
    if let Some(deployed) = value.get("isDeployed").and_then(Value::as_bool) {
        return Ok(deployed);
    }
    if let Some(deployed) = value.get("result").and_then(Value::as_bool) {
        return Ok(deployed);
    }
    bail!("relayer deployment response did not contain a boolean deployment status: {value}");
}

pub(super) async fn check_candidate_relayer_deployment(
    relayer_base_url: &str,
    address: Option<&str>,
    wallet_type: &str,
) -> (Option<bool>, Option<String>, Option<String>) {
    let Some(address) = address
        .map(str::trim)
        .filter(|address| is_address_like(address))
    else {
        return (None, None, None);
    };
    let url = format!(
        "{}/deployed?address={}&type={}",
        relayer_base_url.trim_end_matches('/'),
        address,
        wallet_type
    );
    match check_relayer_deployed(&url).await {
        Ok(deployed) => (Some(deployed), None, Some(url)),
        Err(error) => (None, Some(error.to_string()), Some(url)),
    }
}
