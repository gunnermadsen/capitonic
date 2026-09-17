use super::*;

pub(super) async fn run_user_ws_once(
    config: &LiveExecutionConfig,
    store: &Store,
    data_api: Option<&DataApiClient>,
    state: &Arc<Mutex<LiveTransportState>>,
    submit_guard: &Arc<Mutex<()>>,
) -> Result<()> {
    let subscription = user_ws_subscription_payload(config)?;
    {
        let mut state = state.lock().await;
        state.user_ws_connected = false;
        state.last_user_ws_pong_at = None;
    }
    let operation_timeout = config
        .user_ws_stale
        .min(USER_WS_MAX_TRANSPORT_OPERATION_TIMEOUT);
    let (mut ws, _) = tokio::time::timeout(operation_timeout, connect_async(&config.user_ws_url))
        .await
        .context("Polymarket user websocket connect timed out")?
        .context("failed to connect Polymarket user websocket")?;
    tokio::time::timeout(
        operation_timeout,
        ws.send(Message::Text(subscription.to_string().into())),
    )
    .await
    .context("Polymarket user websocket subscription send timed out")?
    .context("failed to subscribe Polymarket user websocket")?;

    let mut heartbeat = tokio::time::interval(Duration::from_secs(10));
    let mut awaiting_pong_since: Option<tokio::time::Instant> = None;
    loop {
        tokio::select! {
            _ = heartbeat.tick() => {
                if user_ws_heartbeat_ack_timed_out(
                    awaiting_pong_since.map(|sent_at| sent_at.elapsed()),
                    config.user_ws_stale,
                ) {
                    bail!("Polymarket user websocket heartbeat acknowledgement timed out");
                }
                tokio::time::timeout(
                    operation_timeout,
                    ws.send(Message::Text("PING".into())),
                )
                    .await
                    .context("Polymarket user websocket heartbeat send timed out")?
                    .context("failed to ping Polymarket user websocket")?;
                awaiting_pong_since.get_or_insert_with(tokio::time::Instant::now);
            }
            message = ws.next() => {
                let Some(message) = message else {
                    bail!("Polymarket user websocket ended without a close frame");
                };
                match message.context("failed to read Polymarket user websocket")? {
                    Message::Text(text) => {
                        let text = text.to_string();
                        let payload = match classify_user_ws_text(&text, &[])? {
                            UserWsText::HeartbeatPong => {
                                awaiting_pong_since = None;
                                let mut state = state.lock().await;
                                state.user_ws_connected = true;
                                state.last_user_ws_pong_at = Some(Utc::now());
                                continue;
                            }
                            UserWsText::HeartbeatPing => {
                                tokio::time::timeout(
                                    operation_timeout,
                                    ws.send(Message::Text("PONG".into())),
                                )
                                    .await
                                    .context("Polymarket user websocket text PONG send timed out")?
                                    .context("failed to pong Polymarket user websocket")?;
                                continue;
                            }
                            UserWsText::UserEvent(payload) => payload,
                        };
                        // Apply the websocket event atomically with respect to live submission.
                        // Connectivity and event delivery never mutate process authorization.
                        let _event_guard = submit_guard.lock().await;
                        let event = LiveVenue::parse_user_event(payload);
                        let inserted = store.insert_live_venue_event(&event).await?;
                        let bot_fill_persisted = match LiveVenue::persist_fill_from_live_event(&store, &event).await {
                            Ok(persisted) => persisted > 0,
                            Err(error) => {
                                warn!(error = %error, "failed to persist Polymarket live user websocket fill event");
                                false
                            }
                        };
                        if !bot_fill_persisted {
                            if let Some(account_address) = configured_account_address(config)? {
                                if let Some(account_trade) = account_trade_from_live_event(&account_address, &event) {
                                    if let Err(error) = store.upsert_account_trade(&account_trade).await {
                                        warn!(error = %error, "failed to persist Polymarket account trade from user websocket event");
                                    }
                                    if let Some(data_api) = data_api.cloned() {
                                        let store = store.clone();
                                        let token_id = account_trade.token_id.clone();
                                        tokio::spawn(async move {
                                            tokio::time::sleep(Duration::from_secs(2)).await;
                                            let request = AccountReconcileRequest {
                                                account_address: Some(account_address),
                                                lookback_hours: Some(1),
                                                process_id: None,
                                                account_ref: None,
                                                credential_account_fingerprint_sha256: None,
                                                dry_run: false,
                                                token_id: Some(token_id),
                                                source: Some("user_ws".to_string()),
                                            };
                                            if let Err(error) = reconcile_account_positions(&store, &data_api, request).await {
                                                warn!(error = %error, "websocket-triggered account reconciliation failed");
                                            }
                                        });
                                    }
                                }
                            }
                        }
                        if let Err(error) = LiveVenue::persist_order_update_from_live_event(&store, &event).await {
                            warn!(error = %error, "failed to persist Polymarket live user websocket order event");
                        }
                        if inserted {
                            debug!(
                                event_type = %event.event_type,
                                venue_order_id = ?event.venue_order_id,
                                venue_trade_id = ?event.venue_trade_id,
                                "persisted Polymarket live user websocket event"
                            );
                        }
                        let mut state = state.lock().await;
                        state.user_ws_connected = true;
                    }
                    Message::Ping(payload) => {
                        tokio::time::timeout(operation_timeout, ws.send(Message::Pong(payload)))
                            .await
                            .context("Polymarket user websocket control PONG send timed out")?
                            .context("failed to answer websocket control ping")?;
                    }
                    Message::Pong(_) => {
                        awaiting_pong_since = None;
                        let mut state = state.lock().await;
                        state.user_ws_connected = true;
                        state.last_user_ws_pong_at = Some(Utc::now());
                    }
                    Message::Close(_) => bail!("Polymarket user websocket closed"),
                    Message::Binary(_) => bail!("Polymarket user websocket sent an unsupported binary frame"),
                    _ => {}
                }
            }
        }
    }
}

#[derive(Debug)]
pub(super) enum UserWsText {
    HeartbeatPong,
    HeartbeatPing,
    UserEvent(Value),
}

pub(super) fn user_ws_subscription_payload(config: &LiveExecutionConfig) -> Result<Value> {
    Ok(json!({
        "auth": {
            "apiKey": config.clob_api_key.as_deref().unwrap_or(""),
            "secret": config.clob_secret.as_deref().unwrap_or(""),
            "passphrase": config.clob_passphrase.as_deref().unwrap_or("")
        },
        "type": "user"
    }))
}

pub(super) fn classify_user_ws_text(
    text: &str,
    subscribed_markets: &[String],
) -> Result<UserWsText> {
    match text.trim() {
        "PONG" => return Ok(UserWsText::HeartbeatPong),
        "PING" => return Ok(UserWsText::HeartbeatPing),
        _ => {}
    }
    let payload: Value = serde_json::from_str(text)
        .context("failed to decode Polymarket user websocket text payload")?;
    let object = payload
        .as_object()
        .context("Polymarket user websocket payload must be an object")?;
    let explicit_error = object
        .get("error")
        .is_some_and(|value| !value.is_null() && value != &Value::Bool(false))
        || object
            .get("success")
            .and_then(Value::as_bool)
            .is_some_and(|success| !success)
        || object
            .get("status")
            .and_then(Value::as_str)
            .is_some_and(|status| {
                matches!(
                    status.trim().to_ascii_lowercase().as_str(),
                    "error" | "failed" | "rejected" | "unauthorized" | "forbidden"
                )
            });
    if explicit_error {
        bail!("Polymarket user websocket returned an authentication or subscription error");
    }

    let event_type = object
        .get("event_type")
        .and_then(Value::as_str)
        .map(|value| value.trim().to_ascii_lowercase())
        .context("Polymarket user websocket payload has no recognized event_type")?;
    if !matches!(event_type.as_str(), "order" | "trade") {
        bail!("Polymarket user websocket payload has unsupported event_type");
    }
    let event_id_present = object
        .get("id")
        .or_else(|| object.get("order_id"))
        .or_else(|| object.get("trade_id"))
        .and_then(Value::as_str)
        .is_some_and(|value| !value.trim().is_empty());
    let market = object
        .get("market")
        .or_else(|| object.get("condition_id"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .context("Polymarket user websocket event is missing its market identity")?;
    let asset_present = object
        .get("asset_id")
        .or_else(|| object.get("asset"))
        .or_else(|| object.get("token_id"))
        .and_then(Value::as_str)
        .is_some_and(|value| !value.trim().is_empty());
    if !event_id_present || !asset_present {
        bail!("Polymarket user websocket event is missing an immutable order/trade identity");
    }
    if !subscribed_markets.is_empty()
        && !subscribed_markets
            .iter()
            .any(|subscribed| subscribed == market)
    {
        bail!("Polymarket user websocket event does not match the configured subscription");
    }
    Ok(UserWsText::UserEvent(payload))
}
