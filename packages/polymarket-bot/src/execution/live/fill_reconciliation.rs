use super::*;

pub(super) fn rest_fill_backfill_plan(
    process_id: Uuid,
    local_nonterminal: &[OrderRecord],
    owned_orders: &HashMap<String, String>,
    trades: &[TradeResponse],
    checked_at: DateTime<Utc>,
) -> Result<Vec<FillRecord>> {
    if process_id.is_nil() {
        bail!("live REST fill backfill requires a non-nil process_id");
    }
    if local_nonterminal.len() > MAX_PROCESS_NONTERMINAL_ORDERS
        || owned_orders.len() > MAX_CLOB_RECONCILIATION_ORDER_IDS
        || trades.len() > MAX_CLOB_RECONCILIATION_ROWS
    {
        bail!("live REST fill backfill exceeds its bounded evidence window");
    }

    let mut local_orders = HashMap::with_capacity(local_nonterminal.len());
    for order in local_nonterminal {
        if order.request.process_id != Some(process_id) {
            bail!("live REST fill backfill crossed process order ownership");
        }
        if order.order_id.trim().is_empty() || order.order_id.len() > 256 {
            bail!("live REST fill backfill has an invalid local order identity");
        }
        if local_orders
            .insert(order.order_id.as_str(), order)
            .is_some()
        {
            bail!("live REST fill backfill has duplicate local order evidence");
        }
    }

    let mut seen_trade_ids = HashSet::with_capacity(trades.len());
    let mut fills = Vec::new();
    for trade in trades {
        if trade.id.trim().is_empty() || trade.id.trim() != trade.id || trade.id.len() > 256 {
            bail!("live REST fill backfill has an invalid venue trade identity");
        }
        if !seen_trade_ids.insert(trade.id.as_str()) {
            bail!(
                "live REST fill backfill has duplicate venue trade {}",
                trade.id
            );
        }
        if !is_rest_fill_trade_status(&trade.status)? {
            continue;
        }

        let mut candidates = Vec::new();
        if let Some(local_order_id) = owned_orders.get(&trade.taker_order_id) {
            if local_orders.contains_key(local_order_id.as_str()) {
                candidates.push((trade.taker_order_id.as_str(), local_order_id.as_str()));
            }
        }
        for maker_order in &trade.maker_orders {
            if let Some(local_order_id) = owned_orders.get(&maker_order.order_id) {
                if local_orders.contains_key(local_order_id.as_str()) {
                    candidates.push((maker_order.order_id.as_str(), local_order_id.as_str()));
                }
            }
        }
        candidates.sort_unstable();
        candidates.dedup();
        if candidates.is_empty() {
            continue;
        }
        for (venue_order_id, local_order_id) in candidates {
            let order = local_orders
                .get(local_order_id)
                .context("live REST fill ownership points to missing local order")?;
            fills.push(fill_record_from_trade_for_order(
                order,
                venue_order_id,
                trade,
                checked_at,
            )?);
        }
    }
    Ok(fills)
}

pub(super) fn reconciliation_trade_window_start(
    local_nonterminal: &[OrderRecord],
    checked_at: DateTime<Utc>,
) -> Result<DateTime<Utc>> {
    if local_nonterminal.len() > MAX_PROCESS_NONTERMINAL_ORDERS {
        bail!("live reconciliation trade window exceeds its bounded local order evidence");
    }
    let oldest_local_created_at = local_nonterminal
        .iter()
        .map(|order| order.created_at)
        .min()
        .unwrap_or(checked_at);
    if oldest_local_created_at > checked_at {
        bail!("live reconciliation found a future-dated local order");
    }
    oldest_local_created_at
        .checked_sub_signed(LIVE_FILL_RECONCILIATION_SKEW)
        .context("live reconciliation trade window underflow")
}

pub(super) struct RestFillBackfill {
    pub(super) fills: Vec<FillRecord>,
    pub(super) recovered_count: usize,
}

pub(super) async fn persist_rest_fill_backfill(
    store: &Store,
    process_id: Uuid,
    local_nonterminal: &[OrderRecord],
    owned_orders: &HashMap<String, String>,
    trades: &[TradeResponse],
    checked_at: DateTime<Utc>,
) -> Result<RestFillBackfill> {
    let fills = rest_fill_backfill_plan(
        process_id,
        local_nonterminal,
        owned_orders,
        trades,
        checked_at,
    )?;
    let mut fill_ids_by_order: HashMap<String, Vec<Uuid>> = HashMap::new();
    let mut new_fill_size_by_order: HashMap<String, Decimal> = HashMap::new();
    let mut recovered_count = 0usize;
    for fill in &fills {
        let already_durable = store
            .find_fill_order_id(fill.fill_id, crate::store::FillIdentitySite::RestBackfill)
            .await?
            .is_some();
        store.insert_fill(fill).await?;
        if !already_durable {
            recovered_count = recovered_count.saturating_add(1);
            *new_fill_size_by_order
                .entry(fill.order_id.clone())
                .or_default() += fill.size;
        }
        fill_ids_by_order
            .entry(fill.order_id.clone())
            .or_default()
            .push(fill.fill_id);
    }

    let local_orders = local_nonterminal
        .iter()
        .map(|order| (order.order_id.as_str(), order))
        .collect::<HashMap<_, _>>();
    for (order_id, fill_ids) in fill_ids_by_order {
        let order = local_orders
            .get(order_id.as_str())
            .context("live REST fill progress points to missing local order")?;
        let (cumulative_filled_size, cumulative_filled_notional) = store
            .order_filled_economics(
                &order_id,
                order
                    .request
                    .process_id
                    .context("live REST fill order has no process identity")?,
                crate::store::FillEconomicsSite::RestBackfill,
            )
            .await?;
        validate_live_cumulative_fill_economics(
            &order.request,
            cumulative_filled_size,
            cumulative_filled_notional,
        )?;
        let updated = store
            .mark_order_fill_progress(
                &order_id,
                cumulative_filled_size,
                json!({
                    "source": "rest_reconcile_backfill",
                    "cumulative_filled_size": cumulative_filled_size,
                    "fill_ids": fill_ids,
                }),
            )
            .await?
            .with_context(|| {
                format!("live REST fill progress could not find local order {order_id}")
            })?;
        let partial_fak = order.request.order_type == OrderType::Fak
            && cumulative_filled_size < order.request.size;
        let expected_state = if cumulative_filled_size >= order.request.size {
            OrderState::Filled
        } else if order.state == OrderState::Cancelled {
            OrderState::Cancelled
        } else {
            OrderState::PartiallyFilled
        };
        if updated.state != expected_state {
            bail!(
                "live REST fill progress did not persist expected state for order {}",
                order_id
            );
        }
        if partial_fak && updated.state != OrderState::Cancelled {
            store
                .mark_order_cancelled(
                    &order_id,
                    json!({
                        "source": "rest_reconcile_fak_remainder",
                        "cumulative_filled_size": cumulative_filled_size,
                    }),
                )
                .await?
                .with_context(|| {
                    format!("FAK remainder cancellation could not find order {order_id}")
                })?;
        }
        if let Some(new_size) = new_fill_size_by_order.get(&order_id) {
            record_member_live_fill_progress(order, *new_size, cumulative_filled_size);
        }
    }
    Ok(RestFillBackfill {
        fills,
        recovered_count,
    })
}

pub(super) fn record_member_live_fill_progress(
    order: &OrderRecord,
    new_size: Decimal,
    cumulative_size: Decimal,
) {
    let (Some(process_id), Some(member_id), Some(new_shares)) = (
        order.request.process_id,
        order
            .request
            .metadata
            .pointer("/router/member_id")
            .and_then(Value::as_str),
        new_size.to_f64(),
    ) else {
        return;
    };
    crate::btc::unified_model_runtime::telemetry::member_entry_fill_progress(
        process_id,
        member_id,
        order.request.order_type.as_str(),
        if cumulative_size >= order.request.size {
            "full"
        } else {
            "partial"
        },
        new_shares,
    );
}

pub(super) async fn live_fill_records_from_event(
    store: &Store,
    event: &LiveVenueEvent,
    path: LiveEventPath,
) -> Result<Vec<(OrderRecord, FillRecord)>> {
    if event.event_type != "trade" || !is_fill_trade_status(event.event_status.as_deref()) {
        return Ok(Vec::new());
    }

    let Some(trade_id) = event
        .venue_trade_id
        .as_deref()
        .or_else(|| json_str(&event.raw_payload, "id"))
    else {
        return Ok(Vec::new());
    };

    let legacy_fill_id = Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("polymarket:trade:{trade_id}").as_bytes(),
    );
    let legacy_order_id = store
        .find_fill_order_id(legacy_fill_id, path.legacy_identity_site())
        .await?;
    let mut resolved_order_ids = HashSet::new();
    let mut resolved = Vec::new();
    for candidate in live_event_order_id_candidates(&event.raw_payload) {
        let Some(order) = store.find_order_by_venue_order_id(&candidate).await? else {
            continue;
        };
        if !resolved_order_ids.insert(order.order_id.clone()) {
            continue;
        }
        let details = live_event_fill_details_for_order(&event.raw_payload, &candidate)?;
        let price = details
            .price
            .unwrap_or(json_decimal(&event.raw_payload, "price")?);
        let size = details
            .size
            .unwrap_or(json_decimal(&event.raw_payload, "size")?);
        let fee = live_fill_fee(&order, price, size)
            .context("live websocket fill is missing sealed dynamic fee evidence")?;
        let token_id = details
            .token_id
            .or_else(|| json_str(&event.raw_payload, "asset_id").map(str::to_string))
            .unwrap_or_else(|| order.request.token_id.clone());
        let filled_at = json_timestamp(&event.raw_payload, "match_time")
            .or_else(|| json_timestamp(&event.raw_payload, "timestamp"))
            .unwrap_or_else(Utc::now);
        let composite_fill_id = Uuid::new_v5(
            &Uuid::NAMESPACE_URL,
            format!("polymarket:trade-order:{trade_id}:{candidate}").as_bytes(),
        );
        let fill = FillRecord {
            fill_id: if legacy_order_id.as_deref() == Some(order.order_id.as_str()) {
                legacy_fill_id
            } else {
                composite_fill_id
            },
            process_id: order.request.process_id,
            order_id: order.order_id.clone(),
            token_id,
            price,
            size,
            fee,
            source: FillSource::Live,
            filled_at,
        };
        resolved.push((order, fill));
    }

    Ok(resolved)
}

pub(super) fn live_fill_fee(order: &OrderRecord, price: Decimal, size: Decimal) -> Result<Decimal> {
    let fee_rate = sealed_dynamic_fee_rate(&order.request.metadata)?;
    Ok(dynamic_crypto_taker_fee(size, fee_rate, price))
}

pub(super) fn is_fill_trade_status(status: Option<&str>) -> bool {
    matches!(
        status.map(|value| value.to_ascii_uppercase()),
        Some(value) if matches!(value.as_str(), "MATCHED" | "MINED" | "CONFIRMED")
    )
}

pub(super) fn is_cancelled_order_status(status: Option<&str>) -> bool {
    matches!(
        status.map(|value| value.trim().to_ascii_uppercase()),
        Some(value) if matches!(value.as_str(), "CANCELLATION" | "CANCELED" | "CANCELLED")
    )
}

pub(super) fn is_definitive_live_submit_error(error: &anyhow::Error) -> bool {
    for cause in error.chain() {
        if let Some(status) = cause.downcast_ref::<SdkStatus>() {
            return status.status_code.is_client_error()
                && !matches!(status.status_code.as_u16(), 408 | 409 | 425 | 429);
        }
        if let Some(error) = cause.downcast_ref::<polymarket_client_sdk_v2::error::Error>() {
            if matches!(
                error.kind(),
                SdkErrorKind::Validation | SdkErrorKind::Geoblock
            ) {
                return true;
            }
        }
    }
    false
}

pub(super) fn is_retryable_live_pre_submit_error(error: &anyhow::Error) -> bool {
    for cause in error.chain() {
        if let Some(status) = cause.downcast_ref::<SdkStatus>() {
            return status.status_code.is_server_error()
                || matches!(status.status_code.as_u16(), 408 | 409 | 425 | 429);
        }
        if let Some(error) = cause.downcast_ref::<reqwest::Error>() {
            return error.is_timeout()
                || error.is_connect()
                || error.status().is_some_and(|status| {
                    status.is_server_error() || matches!(status.as_u16(), 408 | 409 | 425 | 429)
                });
        }
        if let Some(error) = cause.downcast_ref::<std::io::Error>() {
            if matches!(
                error.kind(),
                std::io::ErrorKind::TimedOut
                    | std::io::ErrorKind::ConnectionReset
                    | std::io::ErrorKind::ConnectionAborted
                    | std::io::ErrorKind::NotConnected
                    | std::io::ErrorKind::BrokenPipe
                    | std::io::ErrorKind::WouldBlock
                    | std::io::ErrorKind::Interrupted
            ) {
                return true;
            }
        }
        if let Some(error) = cause.downcast_ref::<polymarket_client_sdk_v2::error::Error>() {
            if matches!(error.kind(), SdkErrorKind::Synchronization) {
                return true;
            }
        }
    }
    false
}

pub(super) fn live_pre_submit_transient_gate_order(
    request: OrderRequest,
    stage: &'static str,
    error: &anyhow::Error,
) -> Result<OrderRecord> {
    let mut error_chain = format!("{error:#}");
    error_chain.truncate(512);
    let last_completed_stage = match stage {
        "authentication" => "entry_gate",
        "order_metadata" => "authentication",
        "order_build" => "risk_and_metadata",
        _ => "pre_submit_validation",
    };
    warn!(
        client_order_id = %request.client_order_id,
        process_id = ?request.process_id,
        decision_id = ?request.metadata.get("decision_id"),
        plan_id = ?request.metadata.get("plan_id"),
        stage,
        error = %error_chain,
        "transient live pre-submit failure skipped without a venue POST; preserving trading process liveness"
    );
    let mut order =
        live_execution_gate_closed_order(request, LiveExecutionGateReason::VenueReadiness)?;
    let metadata = order
        .request
        .metadata
        .as_object_mut()
        .context("live pre-submit transient failure requires object order metadata")?;
    metadata.insert(
        "live_pre_submit_error".to_string(),
        json!({
            "stage": stage,
            "last_completed_stage": last_completed_stage,
            "error_chain": error_chain,
            "post_attempted": false,
            "retryable": true,
        }),
    );
    Ok(order)
}

pub(super) fn is_clob_operation_timeout(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .is_some_and(|error| error.kind() == std::io::ErrorKind::TimedOut)
    })
}

pub(super) fn live_event_order_id_candidates(payload: &Value) -> Vec<String> {
    let mut candidates = Vec::new();
    if let Some(order_id) = json_str(payload, "taker_order_id") {
        push_unique(&mut candidates, order_id);
    }
    if let Some(order_id) = json_str(payload, "order_id").or_else(|| json_str(payload, "id")) {
        push_unique(&mut candidates, order_id);
    }
    if let Some(maker_orders) = payload.get("maker_orders").and_then(Value::as_array) {
        for maker_order in maker_orders {
            if let Some(order_id) = json_str(maker_order, "order_id") {
                push_unique(&mut candidates, order_id);
            }
        }
    }
    candidates
}

#[derive(Debug, Default)]
struct LiveEventFillDetails {
    price: Option<Decimal>,
    size: Option<Decimal>,
    token_id: Option<String>,
}

fn live_event_fill_details_for_order(
    payload: &Value,
    order_id: &str,
) -> Result<LiveEventFillDetails> {
    let Some(maker_orders) = payload.get("maker_orders").and_then(Value::as_array) else {
        return Ok(LiveEventFillDetails::default());
    };
    for maker_order in maker_orders {
        if json_str(maker_order, "order_id") == Some(order_id) {
            return Ok(LiveEventFillDetails {
                price: Some(json_decimal(maker_order, "price")?),
                size: Some(json_decimal(maker_order, "matched_amount")?),
                token_id: json_str(maker_order, "asset_id").map(str::to_string),
            });
        }
    }
    Ok(LiveEventFillDetails::default())
}

pub(super) fn json_str<'a>(payload: &'a Value, field: &str) -> Option<&'a str> {
    payload.get(field).and_then(Value::as_str)
}

pub(super) fn json_decimal(payload: &Value, field: &str) -> Result<Decimal> {
    let value = payload
        .get(field)
        .with_context(|| format!("missing decimal field {field}"))?;
    if let Some(text) = value.as_str() {
        return Decimal::from_str(text)
            .with_context(|| format!("failed to parse decimal field {field}={text}"));
    }
    if let Some(number) = value.as_f64() {
        return Decimal::from_str(&number.to_string())
            .with_context(|| format!("failed to parse decimal field {field}={number}"));
    }
    bail!("decimal field {field} is not a string or number")
}

pub(super) fn json_timestamp(payload: &Value, field: &str) -> Option<DateTime<Utc>> {
    let raw = payload.get(field)?;
    let value = raw
        .as_i64()
        .or_else(|| raw.as_str().and_then(|text| text.parse::<i64>().ok()))?;
    let seconds = if value > 9_999_999_999 {
        value / 1000
    } else {
        value
    };
    DateTime::<Utc>::from_timestamp(seconds, 0)
}

pub(super) fn push_unique(candidates: &mut Vec<String>, candidate: &str) {
    if !candidates.iter().any(|existing| existing == candidate) {
        candidates.push(candidate.to_string());
    }
}

pub(super) fn post_order_response_payload(response: &PostOrderResponse) -> serde_json::Value {
    json!({
        "order_id": response.order_id,
        "status": response.status.to_string(),
        "success": response.success,
        "making_amount": response.making_amount.to_string(),
        "taking_amount": response.taking_amount.to_string(),
        "trade_ids": response.trade_ids.clone(),
        "transaction_hashes": response.transaction_hashes.iter().map(ToString::to_string).collect::<Vec<_>>(),
        "error_msg": response.error_msg.clone(),
    })
}

pub(super) const LIVE_VENUE_FOK_UNFILLED_REASON: &str = "venue_fok_unfilled";
pub(super) const LIVE_VENUE_FAK_UNFILLED_REASON: &str = "venue_fak_unfilled";
pub(super) const LIVE_VENUE_REJECTED_REASON: &str = "venue_rejected";

pub(super) fn definitive_live_venue_reject_reason(
    order_type: OrderType,
    error: Option<&str>,
) -> &'static str {
    let fok_unfilled = order_type == OrderType::Fok
        && error.is_some_and(|error| {
            let normalized = error.to_ascii_lowercase();
            normalized.contains("couldn't be fully filled")
                || normalized.contains("could not be fully filled")
                || normalized.contains("fully filled or killed")
        });
    if fok_unfilled {
        LIVE_VENUE_FOK_UNFILLED_REASON
    } else if order_type == OrderType::Fak
        && error.is_some_and(|error| {
            let normalized = error.to_ascii_lowercase();
            normalized.contains("no orders found to match")
                || normalized.contains("no matching orders")
        })
    {
        LIVE_VENUE_FAK_UNFILLED_REASON
    } else {
        LIVE_VENUE_REJECTED_REASON
    }
}

pub(super) fn with_live_venue_reject_reason(
    mut payload: serde_json::Value,
    reject_reason: &str,
) -> serde_json::Value {
    if let Some(object) = payload.as_object_mut() {
        object.insert(
            "reject_reason".to_string(),
            serde_json::Value::String(reject_reason.to_string()),
        );
    }
    payload
}

pub(super) fn normalized_address(value: Option<&str>) -> Option<String> {
    value
        .map(|address| address.trim().to_ascii_lowercase())
        .filter(|address| !address.is_empty())
}

pub(super) fn addresses_equal(left: Option<&str>, right: Option<&str>) -> Option<bool> {
    Some(normalized_address(left)? == normalized_address(right)?)
}

pub(super) fn signed_order_address_field(signed_order: &Value, field: &str) -> Option<String> {
    signed_order
        .get("order")
        .and_then(|order| order.get(field))
        .and_then(Value::as_str)
        .map(str::to_string)
}

pub(super) fn signed_order_signature_type(signed_order: &Value) -> Option<String> {
    signed_order
        .get("order")
        .and_then(|order| order.get("signatureType"))
        .map(|value| {
            value
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| value.to_string())
        })
}

pub(super) fn redact_signed_order_secrets(signed_order: &mut Value) {
    if let Some(owner) = signed_order.get_mut("owner") {
        *owner = json!("<redacted>");
    }
    if let Some(signature) = signed_order
        .get_mut("order")
        .and_then(|order| order.get_mut("signature"))
    {
        *signature = json!("<redacted>");
    }
}

pub(super) fn is_poly1271_signature_type(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "3" | "poly1271" | "poly_1271"
    )
}

pub(super) fn decimal_string_positive(value: Option<&str>) -> bool {
    value
        .and_then(|value| value.parse::<Decimal>().ok())
        .is_some_and(|value| value > Decimal::ZERO)
}

pub(super) fn collateral_allowances_positive(allowances: &HashMap<Address, String>) -> bool {
    [CTF_EXCHANGE_V2_ADDRESS, NEG_RISK_CTF_EXCHANGE_V2_ADDRESS]
        .into_iter()
        .all(|required_exchange| {
            allowances.iter().any(|(exchange, allowance)| {
                exchange.to_string().eq_ignore_ascii_case(required_exchange)
                    && allowance
                        .trim()
                        .parse::<U256>()
                        .ok()
                        .is_some_and(|allowance| allowance > U256::ZERO)
            })
        })
}

pub(super) fn is_address_like(value: &str) -> bool {
    let trimmed = value.trim();
    let Some(hex) = trimmed.strip_prefix("0x") else {
        return false;
    };
    hex.len() == 40 && hex.chars().all(|character| character.is_ascii_hexdigit())
}

pub(super) fn push_unique_candidate_address(candidates: &mut Vec<String>, candidate: Option<&str>) {
    let Some(candidate) = candidate
        .map(str::trim)
        .filter(|candidate| is_address_like(candidate))
    else {
        return;
    };
    if candidates
        .iter()
        .any(|existing| addresses_equal(Some(existing), Some(candidate)) == Some(true))
    {
        return;
    }
    candidates.push(candidate.to_string());
}

pub(super) async fn persist_pre_submit_gate_rejection(
    store: &Store,
    pending: OrderRecord,
    reason: LiveExecutionGateReason,
) -> Result<OrderRecord> {
    let mut rejected = live_execution_gate_closed_order(pending.request.clone(), reason)?;
    rejected.order_id = pending.order_id;
    rejected.created_at = pending.created_at;
    store.mark_order_pre_submit_rejected(&rejected).await
}

pub(super) async fn persist_pre_submit_hard_failure(
    store: &Store,
    pending: &OrderRecord,
    error: &anyhow::Error,
) -> Result<()> {
    let mut message = format!("{error:#}");
    message.truncate(512);
    store
        .mark_order_submit_failed(
            pending.request.client_order_id,
            "local_pre_submit_failure",
            json!({
                "error": message,
                "post_attempted": false,
            }),
        )
        .await?;
    Ok(())
}
