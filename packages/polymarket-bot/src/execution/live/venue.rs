use super::*;

impl LiveVenue {
    pub fn new(
        config: LiveExecutionConfig,
        clob_base_url: String,
        store: Store,
        data_api: DataApiClient,
    ) -> Result<Self> {
        config.validate_for_live()?;
        let signer = configured_submit_signer(&config)?;
        let venue = Self {
            config,
            clob_base_url,
            http_client: reqwest::Client::new(),
            signer,
            authenticated_client_cache: Arc::new(OnceCell::new()),
            store: Some(store),
            data_api: Some(data_api),
            bound_process_id: None,
            bound_account_ref: None,
            bound_execution: None,
            transport_state: Arc::new(Mutex::new(LiveTransportState::initial())),
            readiness_state: Arc::new(Mutex::new(LiveVenueState::fail_closed())),
            global_entry_gate: Arc::new(Mutex::new(GlobalLiveEntryGate::fail_closed())),
            submit_guard: Arc::new(Mutex::new(())),
            reconcile_guard: Arc::new(Mutex::new(())),
        };
        venue.spawn_user_ws_task_if_enabled();
        Ok(venue)
    }

    #[cfg(test)]
    pub(super) fn new_for_test(config: LiveExecutionConfig) -> Result<Self> {
        config.validate_for_live()?;
        let signer = configured_submit_signer(&config)?;
        Ok(Self {
            config,
            clob_base_url: "https://clob-v2.polymarket.com".to_string(),
            http_client: reqwest::Client::new(),
            signer,
            authenticated_client_cache: Arc::new(OnceCell::new()),
            store: None,
            data_api: None,
            bound_process_id: None,
            bound_account_ref: None,
            bound_execution: None,
            transport_state: Arc::new(Mutex::new(LiveTransportState::initial())),
            readiness_state: Arc::new(Mutex::new(LiveVenueState::fail_closed())),
            global_entry_gate: Arc::new(Mutex::new(GlobalLiveEntryGate::fail_closed())),
            submit_guard: Arc::new(Mutex::new(())),
            reconcile_guard: Arc::new(Mutex::new(())),
        })
    }

    /// Produces the process-owned execution adapter used by a managed trading process. Transport
    /// health and the authenticated account connection are shared, while readiness, reconciliation
    /// and the manual entry gate remain isolated to the process.
    pub fn bind_process(
        &self,
        process_id: Uuid,
        execution: &EffectiveProcessExecutionConfig,
    ) -> Result<Self> {
        if process_id.is_nil() {
            bail!("live execution process_id must not be nil");
        }
        if execution.mode != "live" {
            bail!("live execution venue requires execution.mode=live");
        }
        let account_ref = execution.account_ref.as_deref().unwrap_or("").trim();
        if account_ref.is_empty() {
            bail!("live execution account_ref must not be blank");
        }
        if account_ref.len() > 128 {
            bail!("live execution account_ref must not exceed 128 bytes");
        }
        Ok(Self {
            config: self.config.clone(),
            clob_base_url: self.clob_base_url.clone(),
            http_client: self.http_client.clone(),
            signer: self.signer.clone(),
            authenticated_client_cache: self.authenticated_client_cache.clone(),
            store: self.store.clone(),
            data_api: self.data_api.clone(),
            bound_process_id: Some(process_id),
            bound_account_ref: Some(account_ref.to_string()),
            bound_execution: Some(execution.clone()),
            transport_state: self.transport_state.clone(),
            readiness_state: Arc::new(Mutex::new(LiveVenueState::fail_closed())),
            global_entry_gate: self.global_entry_gate.clone(),
            submit_guard: self.submit_guard.clone(),
            reconcile_guard: self.reconcile_guard.clone(),
        })
    }

    pub fn bound_process_id(&self) -> Option<Uuid> {
        self.bound_process_id
    }

    pub fn bound_account_ref(&self) -> Option<&str> {
        self.bound_account_ref.as_deref()
    }

    pub async fn restore_configured_entries_after_restart(&self) -> Result<()> {
        if self.bound_process_id.is_none() || !self.order_submission_enabled() {
            bail!("restart authorization requires a configured live-capital process");
        }

        let _submit_guard = self.submit_guard.lock().await;
        let mut state = self.readiness_state.lock().await;
        let mut global = self.global_entry_gate.lock().await;
        state.manual_entries_enabled = true;
        state.manual_entries_reason = None;
        if global.halted && global.reason == "global_enable_required" {
            global.halted = false;
            global.reason = "configured_resume_authorization".to_string();
        }
        Ok(())
    }

    pub(super) fn bound_execution(&self) -> Result<&EffectiveProcessExecutionConfig> {
        self.bound_execution
            .as_ref()
            .context("live execution venue is not bound to a trading process")
    }

    pub(super) fn order_submission_enabled(&self) -> bool {
        self.bound_execution.as_ref().is_some_and(|execution| {
            execution.mode == "live" && execution.execute_signals && execution.live_capital
        })
    }

    pub fn parse_user_event(raw_payload: serde_json::Value) -> LiveVenueEvent {
        let event_type = raw_payload
            .get("event_type")
            .or_else(|| raw_payload.get("type"))
            .and_then(|value| value.as_str())
            .unwrap_or("unknown")
            .to_ascii_lowercase();
        let venue_event_id = raw_payload
            .get("id")
            .and_then(|value| value.as_str())
            .map(str::to_string);
        let venue_order_id = raw_payload
            .get("order_id")
            .or_else(|| raw_payload.get("id").filter(|_| event_type == "order"))
            .or_else(|| raw_payload.get("taker_order_id"))
            .and_then(|value| value.as_str())
            .map(str::to_string);
        let venue_trade_id = raw_payload
            .get("trade_id")
            .or_else(|| raw_payload.get("id").filter(|_| event_type == "trade"))
            .and_then(|value| value.as_str())
            .map(str::to_string);
        let event_status = raw_payload
            .get("status")
            .or_else(|| raw_payload.get("type"))
            .and_then(|value| value.as_str())
            .map(str::to_string);
        LiveVenueEvent {
            source: "user_ws".to_string(),
            event_type,
            venue_event_id,
            venue_order_id,
            venue_trade_id,
            event_status,
            raw_payload,
        }
    }

    pub(super) async fn persist_fill_from_live_event(
        store: &Store,
        event: &LiveVenueEvent,
    ) -> Result<usize> {
        let fills = live_fill_records_from_event(store, event).await?;
        for (order, fill) in &fills {
            store.insert_fill(fill).await?;
            let (cumulative_filled_size, cumulative_filled_notional) =
                store.order_filled_economics(&order.order_id).await?;
            validate_live_cumulative_fill_economics(
                &order.request,
                cumulative_filled_size,
                cumulative_filled_notional,
            )?;
            store
                .mark_order_fill_progress(
                    &order.order_id,
                    cumulative_filled_size,
                    json!({
                        "source": "user_ws",
                        "event_type": event.event_type,
                        "venue_event_id": event.venue_event_id,
                        "venue_trade_id": event.venue_trade_id,
                        "cumulative_filled_size": cumulative_filled_size,
                        "raw_payload": event.raw_payload,
                    }),
                )
                .await?;
        }
        Ok(fills.len())
    }

    pub(super) async fn persist_order_update_from_live_event(
        store: &Store,
        event: &LiveVenueEvent,
    ) -> Result<bool> {
        if !matches!(event.event_type.as_str(), "order" | "cancellation")
            || !is_cancelled_order_status(event.event_status.as_deref())
        {
            return Ok(false);
        }
        let Some(order_id) = event
            .venue_order_id
            .as_deref()
            .or_else(|| json_str(&event.raw_payload, "id"))
        else {
            return Ok(false);
        };
        store
            .mark_order_cancelled(
                order_id,
                json!({
                    "source": "user_ws",
                    "event_type": event.event_type,
                    "event_status": event.event_status,
                    "raw_payload": event.raw_payload,
                }),
            )
            .await?;
        Ok(true)
    }

    pub(super) async fn backfill_fills_from_live_events(&self) -> Result<usize> {
        let store = self.store()?;
        let mut processed = 0usize;
        for event in store.recent_live_trade_events(500).await? {
            let persisted = LiveVenue::persist_fill_from_live_event(&store, &event).await?;
            if persisted > 0 {
                processed = processed.saturating_add(persisted);
            } else if let Some(account_address) = configured_account_address(&self.config)? {
                if let Some(account_trade) = account_trade_from_live_event(&account_address, &event)
                {
                    store.upsert_account_trade(&account_trade).await?;
                }
            }
        }
        for event in store.recent_live_order_events(500).await? {
            let _ = LiveVenue::persist_order_update_from_live_event(&store, &event).await?;
        }
        Ok(processed)
    }

    pub(super) fn spawn_user_ws_task_if_enabled(&self) {
        if !self.config.user_ws_auth_available() {
            return;
        }
        let config = self.config.clone();
        let Some(store) = self.store.clone() else {
            return;
        };
        let data_api = self.data_api.clone();
        let transport_state = self.transport_state.clone();
        let submit_guard = self.submit_guard.clone();
        tokio::spawn(async move {
            let mut reconnect_delay = USER_WS_RECONNECT_INITIAL_DELAY;
            loop {
                let result = run_user_ws_once(
                    &config,
                    &store,
                    data_api.as_ref(),
                    &transport_state,
                    &submit_guard,
                )
                .await;
                let was_healthy = {
                    let mut state = transport_state.lock().await;
                    let was_healthy = state.last_user_ws_pong_at.is_some();
                    state.user_ws_connected = false;
                    state.last_user_ws_pong_at = None;
                    was_healthy
                };
                let retry_delay = if was_healthy {
                    reconnect_delay = USER_WS_RECONNECT_INITIAL_DELAY;
                    USER_WS_RECONNECT_INITIAL_DELAY
                } else {
                    let retry_delay = reconnect_delay;
                    reconnect_delay = next_user_ws_reconnect_delay(reconnect_delay);
                    retry_delay
                };
                if let Err(error) = result {
                    warn!(
                        error = %error,
                        retry_delay_ms = retry_delay.as_millis(),
                        "Polymarket live user websocket disconnected; reconnecting independently of trading process state"
                    );
                }
                tokio::time::sleep(retry_delay).await;
            }
        });
    }

    pub(super) async fn authenticated_client(&self) -> Result<AuthenticatedClient> {
        if !self.config.submit_auth_available() {
            bail!("live submit auth is incomplete");
        }
        let client = self
            .authenticated_client_cache
            .get_or_try_init(|| async {
                let api_key = self
                    .config
                    .clob_api_key
                    .as_deref()
                    .context("missing CLOB API key")?;
                let secret = self
                    .config
                    .clob_secret
                    .clone()
                    .context("missing CLOB secret")?;
                let passphrase = self
                    .config
                    .clob_passphrase
                    .clone()
                    .context("missing CLOB passphrase")?;
                let signature_type = parse_signature_type(self.config.signature_type.as_deref())?;
                let signer = self
                    .signer
                    .as_deref()
                    .context("missing live submit signer")?;
                let credentials = Credentials::new(
                    Uuid::parse_str(api_key).context("POLYMARKET_CLOB_API_KEY must be a UUID")?,
                    secret,
                    passphrase,
                );
                let mut builder = SdkClient::new(&self.clob_base_url, SdkConfig::default())
                    .context("failed to create Polymarket CLOB SDK client")?
                    .authentication_builder(signer)
                    .credentials(credentials)
                    .signature_type(signature_type);
                if signature_type != SignatureType::Eoa {
                    let funder = self
                        .config
                        .funder_address
                        .as_deref()
                        .context("missing POLYMARKET_FUNDER_ADDRESS")?;
                    builder = builder.funder(
                        Address::from_str(funder)
                            .context("failed to parse POLYMARKET_FUNDER_ADDRESS")?,
                    );
                }
                builder
                    .authenticate()
                    .await
                    .context("failed to authenticate Polymarket CLOB SDK client")
            })
            .await?;
        Ok(client.clone())
    }

    pub(super) async fn prewarm_order_metadata(
        &self,
        client: &AuthenticatedClient,
        token_id: U256,
    ) -> Result<()> {
        let _ = tokio::try_join!(
            client.version(),
            client.tick_size(token_id),
            client.neg_risk(token_id),
        )
        .context("failed to prepare Polymarket CLOB order metadata")?;
        Ok(())
    }

    pub(super) fn authenticated_read_headers(
        &self,
        request: &reqwest::Request,
    ) -> Result<reqwest::header::HeaderMap> {
        let api_key = self
            .config
            .clob_api_key
            .as_deref()
            .context("missing CLOB API key")?;
        let secret = self
            .config
            .clob_secret
            .as_deref()
            .context("missing CLOB secret")?;
        let passphrase = self
            .config
            .clob_passphrase
            .as_deref()
            .context("missing CLOB passphrase")?;
        let private_key = self
            .config
            .private_key
            .as_deref()
            .context("missing private key")?;
        let signer =
            LocalSigner::from_str(private_key).context("failed to parse POLYMARKET_PRIVATE_KEY")?;
        let timestamp = Utc::now().timestamp();
        let message = format!("{}{}{}", timestamp, request.method(), request.url().path());
        let signature = clob_l2_signature(secret, &message)?;
        let mut headers = reqwest::header::HeaderMap::new();
        for (name, value) in [
            ("poly_address", signer.address().to_checksum(None)),
            ("poly_api_key", api_key.to_string()),
            ("poly_passphrase", passphrase.to_string()),
            ("poly_signature", signature),
            ("poly_timestamp", timestamp.to_string()),
        ] {
            headers.insert(
                reqwest::header::HeaderName::from_static(name),
                reqwest::header::HeaderValue::from_str(&value)
                    .with_context(|| format!("invalid {name} header"))?,
            );
        }
        Ok(headers)
    }

    pub(super) async fn tolerant_trade_page(
        &self,
        request: &TradesRequest,
        cursor: Option<&str>,
    ) -> Result<Page<TradeResponse>> {
        let endpoint = format!("{}/data/trades", self.clob_base_url.trim_end_matches('/'));
        let mut builder = self.http_client.get(endpoint).query(request);
        if let Some(cursor) = cursor {
            builder = builder.query(&[("next_cursor", cursor)]);
        }
        let mut request = builder
            .build()
            .context("failed to build Polymarket CLOB trades request")?;
        *request.headers_mut() = self.authenticated_read_headers(&request)?;
        let response = self
            .http_client
            .execute(request)
            .await
            .context("Polymarket CLOB trades fetch failed")?;
        if !response.status().is_success() {
            bail!(
                "Polymarket CLOB trades fetch failed with HTTP {}",
                response.status()
            );
        }
        let mut payload = response
            .json::<Value>()
            .await
            .context("Polymarket CLOB trades response was not valid JSON")?;
        let normalized_fee_count = normalize_blank_taker_counterparty_fees(&mut payload);
        if normalized_fee_count > 0 {
            debug!(
                normalized_fee_count,
                "normalized blank counterparty fee fields in authenticated taker trades"
            );
        }
        serde_json::from_value(payload)
            .context("Polymarket CLOB trades response failed strict decoding")
    }

    pub(super) async fn all_open_order_responses(
        &self,
        client: &AuthenticatedClient,
    ) -> Result<Vec<OpenOrderResponse>> {
        let request = OrdersRequest::builder().build();
        let mut cursor = None;
        let mut seen_cursors = HashSet::new();
        let mut orders = Vec::new();
        for _ in 0..MAX_CLOB_RECONCILIATION_PAGES {
            let page = client
                .orders(&request, cursor.clone())
                .await
                .context("Polymarket CLOB open orders fetch failed")?;
            validate_clob_page_metadata("open orders", page.data.len(), page.count, page.limit)?;
            checked_clob_row_count(orders.len(), page.data.len(), "open orders")?;
            orders.extend(page.data);
            let Some(next_cursor) = next_clob_reconciliation_cursor(
                &page.next_cursor,
                &mut seen_cursors,
                "open orders",
            )?
            else {
                return Ok(orders);
            };
            cursor = Some(next_cursor);
        }
        bail!(
            "Polymarket CLOB open orders exceed the bounded {}-page reconciliation window",
            MAX_CLOB_RECONCILIATION_PAGES
        )
    }

    pub(super) async fn all_trade_responses(
        &self,
        request: &TradesRequest,
    ) -> Result<Vec<TradeResponse>> {
        let mut cursor = None;
        let mut seen_cursors = HashSet::new();
        let mut trades = Vec::new();
        for _ in 0..MAX_CLOB_RECONCILIATION_PAGES {
            let page = self.tolerant_trade_page(request, cursor.as_deref()).await?;
            validate_clob_page_metadata("trades", page.data.len(), page.count, page.limit)?;
            checked_clob_row_count(trades.len(), page.data.len(), "trades")?;
            trades.extend(page.data);
            let Some(next_cursor) =
                next_clob_reconciliation_cursor(&page.next_cursor, &mut seen_cursors, "trades")?
            else {
                return Ok(trades);
            };
            cursor = Some(next_cursor);
        }
        bail!(
            "Polymarket CLOB trades exceed the bounded {}-page reconciliation window",
            MAX_CLOB_RECONCILIATION_PAGES
        )
    }

    pub(super) async fn bound_account_venue_order_ids<'a>(
        &self,
        store: &Store,
        account_ref: &str,
        venue_order_ids: impl IntoIterator<Item = &'a str>,
    ) -> Result<HashMap<String, String>> {
        let mut unique_ids = Vec::new();
        let mut seen_ids = HashSet::new();
        for order_id in venue_order_ids {
            let order_id = order_id.trim();
            if order_id.is_empty() || order_id.len() > 256 {
                bail!("Polymarket CLOB reconciliation returned an invalid order identity");
            }
            if seen_ids.insert(order_id.to_string()) {
                unique_ids.push(order_id.to_string());
                if unique_ids.len() > MAX_CLOB_RECONCILIATION_ORDER_IDS {
                    bail!(
                        "Polymarket CLOB reconciliation exceeds the bounded {}-order identity window",
                        MAX_CLOB_RECONCILIATION_ORDER_IDS
                    );
                }
            }
        }

        let mut owned_orders = HashMap::new();
        for chunk in unique_ids.chunks(CLOB_ORDER_ID_QUERY_CHUNK) {
            for (venue_order_id, local_order_id) in store
                .account_order_ids_by_venue_order_ids(account_ref, chunk)
                .await?
            {
                if owned_orders
                    .insert(venue_order_id.clone(), local_order_id.clone())
                    .is_some_and(|existing| existing != local_order_id)
                {
                    bail!(
                        "Polymarket CLOB venue order {} maps to conflicting local order identities",
                        venue_order_id
                    );
                }
            }
        }
        Ok(owned_orders)
    }

    pub(super) async fn canonical_live_account_identity(
        &self,
    ) -> Result<CanonicalLiveAccountIdentity> {
        let identity = canonical_configured_account_identity(
            &self.config,
            self.bound_account_ref()
                .context("live account identity requires a process account_ref")?,
        )?;
        let authenticated_address = self
            .authenticated_client()
            .await?
            .address()
            .to_checksum(None)
            .to_ascii_lowercase();
        if authenticated_address != identity.signer_address {
            bail!("authenticated CLOB address does not match configured live signer identity");
        }
        Ok(identity)
    }

    pub(super) async fn run_account_reconcile(
        &self,
        mut request: AccountReconcileRequest,
    ) -> Result<AccountReconcileReport> {
        let mut canonical_identity = None;
        if let Some(process_id) = self.bound_process_id {
            let account_ref = self
                .bound_account_ref
                .as_deref()
                .context("process-bound live venue is missing account_ref")?;
            if request
                .process_id
                .is_some_and(|requested| requested != process_id)
            {
                bail!(
                    "account reconciliation process_id must match bound process {}",
                    process_id
                );
            }
            if request
                .account_ref
                .as_deref()
                .is_some_and(|requested| requested.trim() != account_ref)
            {
                bail!(
                    "account reconciliation account_ref must match bound account_ref {}",
                    account_ref
                );
            }
            request.process_id = Some(process_id);
            request.account_ref = Some(account_ref.to_string());
            let identity = self.canonical_live_account_identity().await?;
            if request
                .account_address
                .as_deref()
                .is_some_and(|requested| !requested.eq_ignore_ascii_case(&identity.account_address))
            {
                bail!("account reconciliation address does not match canonical bound identity");
            }
            if request
                .credential_account_fingerprint_sha256
                .as_deref()
                .is_some_and(|requested| requested != identity.fingerprint_sha256)
            {
                bail!(
                    "account reconciliation credential/account fingerprint does not match bound identity"
                );
            }
            request.account_address = Some(identity.account_address.clone());
            request.credential_account_fingerprint_sha256 =
                Some(identity.fingerprint_sha256.clone());
            canonical_identity = Some(identity);
        } else if request.process_id.is_some()
            || request.account_ref.is_some()
            || request.credential_account_fingerprint_sha256.is_some()
        {
            bail!(
                "wallet-wide live account reconciliation rejects caller-supplied process scope; bind the venue to reconcile a process"
            );
        }
        let store = self.store()?;
        let data_api = self
            .data_api
            .as_ref()
            .context("Polymarket Data API client is not configured")?;
        if request.account_address.is_none() {
            request.account_address = configured_account_address(&self.config)?;
        }
        if request.account_address.is_none() {
            request.account_address = Some(
                canonical_identity
                    .as_ref()
                    .context("live account reconciliation has no canonical account identity")?
                    .account_address
                    .clone(),
            );
        }
        reconcile_account_positions(&store, data_api, request).await
    }

    pub(super) async fn poly1271_candidate_client(
        &self,
        funder_address: &str,
    ) -> Result<AuthenticatedClient> {
        let private_key = self
            .config
            .private_key
            .as_deref()
            .context("missing private key")?;
        let signer = LocalSigner::from_str(private_key)
            .context("failed to parse POLYMARKET_PRIVATE_KEY")?
            .with_chain_id(Some(POLYGON));
        let funder = Address::from_str(funder_address)
            .context("failed to parse candidate funder address")?;
        SdkClient::new(&self.clob_base_url, SdkConfig::default())
            .context("failed to create Polymarket CLOB SDK client")?
            .authentication_builder(&signer)
            .signature_type(SignatureType::Poly1271)
            .funder(funder)
            .authenticate()
            .await
            .context("failed to authenticate candidate as POLY_1271")
    }

    pub(super) async fn apply_poly1271_candidate_clob_diagnostics(
        &self,
        candidate: &mut LiveWalletCandidateAddressDiagnostics,
    ) {
        let client = match self.poly1271_candidate_client(&candidate.address).await {
            Ok(client) => client,
            Err(error) => {
                let message = error.to_string();
                candidate.poly1271_api_keys_error = Some(message.clone());
                candidate.poly1271_balance_allowance_error = Some(message.clone());
                candidate.poly1271_open_orders_error = Some(message);
                return;
            }
        };
        candidate.poly1271_authenticated_client_address = Some(client.address().to_checksum(None));

        match client.api_keys().await {
            Ok(_) => candidate.poly1271_api_keys_readable = true,
            Err(error) => candidate.poly1271_api_keys_error = Some(error.to_string()),
        }

        match client
            .balance_allowance(
                BalanceAllowanceRequest::builder()
                    .asset_type(AssetType::Collateral)
                    .signature_type(SignatureType::Poly1271)
                    .build(),
            )
            .await
        {
            Ok(balance) => {
                candidate.poly1271_balance_allowance_readable = true;
                match local_decimal(balance.balance) {
                    Ok(balance) => {
                        candidate.poly1271_collateral_balance = Some(balance.to_string())
                    }
                    Err(error) => {
                        candidate.poly1271_balance_allowance_error = Some(error.to_string())
                    }
                }
            }
            Err(error) => candidate.poly1271_balance_allowance_error = Some(error.to_string()),
        }

        match client.orders(&OrdersRequest::builder().build(), None).await {
            Ok(page) => {
                candidate.poly1271_open_orders_readable = true;
                candidate.poly1271_open_orders_count = Some(page.data.len());
            }
            Err(error) => candidate.poly1271_open_orders_error = Some(error.to_string()),
        }
    }

    pub(super) async fn poly1271_funder_probe_candidate(
        &self,
        request: &LivePoly1271FunderProbeRequest,
        order_type: OrderType,
        address: &str,
    ) -> LivePoly1271FunderProbeCandidate {
        let mut candidate = LivePoly1271FunderProbeCandidate {
            address: address.to_string(),
            address_valid: is_address_like(address),
            derive_credentials_ok: false,
            derive_credentials_error: None,
            authenticated_client_address: None,
            api_keys_readable: false,
            api_keys_error: None,
            update_balance_allowance_ok: false,
            update_balance_allowance_error: None,
            balance_allowance_readable: false,
            balance_allowance_error: None,
            collateral_balance: None,
            open_orders_readable: false,
            open_orders_error: None,
            open_orders_count: None,
            signed_order_build_ok: false,
            signed_order_error: None,
            signed_order_maker: None,
            signed_order_signer: None,
            signed_order_signature_type: None,
            maker_matches_candidate: None,
            signer_matches_candidate: None,
            signature_type_is_poly1271: None,
            ready_for_live_canary: false,
            signed_order: json!(null),
        };

        if !candidate.address_valid {
            candidate.derive_credentials_error =
                Some("candidate address is not a valid 20-byte hex address".to_string());
            return candidate;
        }

        let client = match self.poly1271_candidate_client(address).await {
            Ok(client) => {
                candidate.derive_credentials_ok = true;
                candidate.authenticated_client_address = Some(client.address().to_checksum(None));
                client
            }
            Err(error) => {
                let message = error.to_string();
                candidate.derive_credentials_error = Some(message.clone());
                candidate.api_keys_error = Some(message.clone());
                candidate.balance_allowance_error = Some(message.clone());
                candidate.open_orders_error = Some(message);
                return candidate;
            }
        };

        match client.api_keys().await {
            Ok(_) => candidate.api_keys_readable = true,
            Err(error) => candidate.api_keys_error = Some(error.to_string()),
        }

        match client
            .update_balance_allowance(
                UpdateBalanceAllowanceRequest::builder()
                    .asset_type(AssetType::Collateral)
                    .signature_type(SignatureType::Poly1271)
                    .build(),
            )
            .await
        {
            Ok(_) => candidate.update_balance_allowance_ok = true,
            Err(error) => candidate.update_balance_allowance_error = Some(error.to_string()),
        }

        match client
            .balance_allowance(
                BalanceAllowanceRequest::builder()
                    .asset_type(AssetType::Collateral)
                    .signature_type(SignatureType::Poly1271)
                    .build(),
            )
            .await
        {
            Ok(balance) => {
                candidate.balance_allowance_readable = true;
                match local_decimal(balance.balance) {
                    Ok(balance) => candidate.collateral_balance = Some(balance.to_string()),
                    Err(error) => candidate.balance_allowance_error = Some(error.to_string()),
                }
            }
            Err(error) => candidate.balance_allowance_error = Some(error.to_string()),
        }

        match client.orders(&OrdersRequest::builder().build(), None).await {
            Ok(page) => {
                candidate.open_orders_readable = true;
                candidate.open_orders_count = Some(page.data.len());
            }
            Err(error) => candidate.open_orders_error = Some(error.to_string()),
        }

        let signed = match self
            .build_signed_poly1271_candidate_order(&client, address, request, order_type)
            .await
        {
            Ok(signed) => signed,
            Err(error) => {
                candidate.signed_order_error = Some(error.to_string());
                return candidate;
            }
        };

        let mut signed_order = serde_json::to_value(&signed)
            .unwrap_or_else(|error| json!({ "error": error.to_string() }));
        candidate.signed_order_maker = signed_order_address_field(&signed_order, "maker");
        candidate.signed_order_signer = signed_order_address_field(&signed_order, "signer");
        candidate.signed_order_signature_type = signed_order_signature_type(&signed_order);
        candidate.maker_matches_candidate =
            addresses_equal(candidate.signed_order_maker.as_deref(), Some(address));
        candidate.signer_matches_candidate =
            addresses_equal(candidate.signed_order_signer.as_deref(), Some(address));
        candidate.signature_type_is_poly1271 = candidate
            .signed_order_signature_type
            .as_deref()
            .map(is_poly1271_signature_type);
        redact_signed_order_secrets(&mut signed_order);
        candidate.signed_order = signed_order;
        candidate.signed_order_build_ok = true;
        candidate.ready_for_live_canary = candidate.derive_credentials_ok
            && candidate.api_keys_readable
            && candidate.update_balance_allowance_ok
            && candidate.balance_allowance_readable
            && candidate.open_orders_readable
            && candidate.signed_order_build_ok
            && decimal_string_positive(candidate.collateral_balance.as_deref())
            && candidate.maker_matches_candidate == Some(true)
            && candidate.signer_matches_candidate == Some(true)
            && candidate.signature_type_is_poly1271 == Some(true);

        candidate
    }

    pub(super) async fn build_signed_poly1271_candidate_order(
        &self,
        client: &AuthenticatedClient,
        funder_address: &str,
        request: &LivePoly1271FunderProbeRequest,
        order_type: OrderType,
    ) -> Result<impl serde::Serialize> {
        let private_key = self
            .config
            .private_key
            .as_deref()
            .context("missing private key")?;
        let signer = LocalSigner::from_str(private_key)
            .context("failed to parse POLYMARKET_PRIVATE_KEY")?
            .with_chain_id(Some(POLYGON));
        let token_id =
            U256::from_str(&request.token_id).context("failed to parse CLOB token_id")?;
        let signable = client
            .limit_order()
            .token_id(token_id)
            .side(sdk_side(request.side))
            .price(sdk_decimal(request.price)?)
            .size(sdk_decimal(request.size)?)
            .order_type(sdk_order_type(order_type)?)
            .build()
            .await
            .with_context(|| {
                format!("failed to build POLY_1271 dry-run order for funder {funder_address}")
            })?;
        client.sign(&signer, signable).await.with_context(|| {
            format!("failed to sign POLY_1271 dry-run order for funder {funder_address}")
        })
    }

    pub(super) fn store(&self) -> Result<Store> {
        self.store
            .clone()
            .context("live persistence store is not configured")
    }

    pub(super) fn require_bound_process_id(&self) -> Result<Uuid> {
        self.bound_process_id.context(
            "live order execution requires a process-bound venue; call LiveVenue::bind_process",
        )
    }

    pub(super) fn validate_request_process(&self, request: &OrderRequest) -> Result<Uuid> {
        let process_id = self.require_bound_process_id()?;
        if request.process_id != Some(process_id) {
            bail!(
                "live order process_id must match bound process {}; received {:?}",
                process_id,
                request.process_id
            );
        }
        Ok(process_id)
    }

    pub(super) async fn bounded_nonterminal_orders(
        &self,
        process_id: Uuid,
    ) -> Result<Vec<OrderRecord>> {
        let store = self.store()?;
        let orders = store
            .live_process_nonterminal_orders(
                process_id,
                (MAX_PROCESS_NONTERMINAL_ORDERS + 1) as i64,
            )
            .await?;
        if orders.len() > MAX_PROCESS_NONTERMINAL_ORDERS {
            bail!(
                "live process {} exceeds the bounded {}-order reconciliation window",
                process_id,
                MAX_PROCESS_NONTERMINAL_ORDERS
            );
        }
        Ok(orders)
    }

    pub(super) async fn enforce_submission_risk(
        &self,
        process_id: Uuid,
        request: &OrderRequest,
        ignored_client_order_id: Option<Uuid>,
    ) -> Result<Option<LiveExecutionGateReason>> {
        let identity = canonical_configured_account_identity(
            &self.config,
            self.bound_account_ref()
                .context("live risk checks require a process account_ref")?,
        )?;
        let (process_accounting_entry_safe, reconciled_fingerprint) = {
            let state = self.readiness_state.lock().await;
            (
                state.process_accounting_entry_safe,
                state.credential_account_fingerprint_sha256.clone(),
            )
        };
        if !process_accounting_entry_safe {
            return Ok(Some(LiveExecutionGateReason::ProcessAccountingReadiness));
        }
        if reconciled_fingerprint.as_deref() != Some(identity.fingerprint_sha256.as_str()) {
            bail!("live process reconciliation identity does not match configured credentials");
        }

        let store = self.store()?;
        let max_daily_loss_usd = self.bound_execution()?.max_daily_loss_usd;
        let account_ref = (request.side == OrderSide::Buy)
            .then(|| {
                self.bound_account_ref()
                    .context("live account capital admission requires account_ref")
            })
            .transpose()?;
        let exposure_read =
            store.conservative_live_process_exposure(process_id, ignored_client_order_id);
        let daily_pnl_read = async {
            let Some(_) = max_daily_loss_usd else {
                return Ok::<_, anyhow::Error>(None);
            };
            let now = Utc::now();
            let day_start = now
                .date_naive()
                .and_hms_opt(0, 0, 0)
                .expect("midnight is a valid UTC time")
                .and_utc();
            let day_end = day_start + chrono::Duration::days(1);
            Ok(Some(
                store
                    .recognized_live_process_net_pnl_for_utc_day(
                        process_id, day_start, day_end, now,
                    )
                    .await?,
            ))
        };
        let account_orders_read = async {
            let Some(account_ref) = account_ref else {
                return Ok::<_, anyhow::Error>(None);
            };
            Ok(Some(
                store
                    .live_account_nonterminal_orders(
                        account_ref,
                        (MAX_CLOB_RECONCILIATION_ROWS + 1) as i64,
                    )
                    .await?,
            ))
        };
        let collateral_read = async {
            if request.side != OrderSide::Buy {
                return Ok::<_, anyhow::Error>(None);
            }
            Ok(Some(self.get_balances().await?))
        };
        let (exposure, recognized_net_pnl, account_orders, balances) = tokio::try_join!(
            exposure_read,
            daily_pnl_read,
            account_orders_read,
            collateral_read,
        )?;
        let requested_exposure = Store::conservative_live_request_exposure(request)?;
        let resulting_exposure = exposure
            .total_exposure_usd
            .checked_add(requested_exposure.total_exposure_usd)
            .context("live requested capital exposure overflow")?;
        if let Some(reason) = live_capital_exposure_gate(
            resulting_exposure,
            self.bound_execution()?.max_daily_loss_usd,
            self.bound_execution()?.max_open_notional_usd,
        ) {
            return Ok(Some(reason));
        }

        if let (Some(max_daily_loss_usd), Some(recognized_net_pnl)) =
            (max_daily_loss_usd, recognized_net_pnl)
        {
            if recognized_net_pnl <= -max_daily_loss_usd {
                return Ok(Some(LiveExecutionGateReason::DailyLossLimit));
            }
        }

        let mut markets = exposure
            .exposed_market_ids
            .into_iter()
            .collect::<HashSet<_>>();
        markets.insert(request.market_id.clone());
        if self
            .bound_execution()?
            .max_open_positions
            .is_some_and(|maximum| markets.len() > maximum)
        {
            return Ok(Some(LiveExecutionGateReason::OpenPositionLimit));
        }

        if let (Some(account_orders), Some(balances)) = (account_orders, balances) {
            if account_orders.len() > MAX_CLOB_RECONCILIATION_ROWS {
                bail!(
                    "live account exceeds the bounded {}-order capital admission window",
                    MAX_CLOB_RECONCILIATION_ROWS
                );
            }
            let mut reserved = Decimal::ZERO;
            for order in account_orders {
                if order.request.side != OrderSide::Buy
                    || Some(order.request.client_order_id) == ignored_client_order_id
                {
                    continue;
                }
                reserved = reserved
                    .checked_add(
                        Store::conservative_live_request_exposure(&order.request)?
                            .total_exposure_usd,
                    )
                    .context("live account buy reservation overflow")?;
            }
            let available_collateral = balances
                .into_iter()
                .find_map(|(asset, balance)| (asset == "USDC").then_some(balance))
                .context("live account collateral balance is unavailable")?;
            if reserved
                .checked_add(requested_exposure.total_exposure_usd)
                .context("live account requested reservation overflow")?
                > available_collateral
            {
                return Ok(Some(LiveExecutionGateReason::OpenNotionalLimit));
            }
        }
        Ok(None)
    }

    pub(super) async fn current_entry_gate_reason(
        &self,
    ) -> Result<Option<LiveExecutionGateReason>> {
        if !self.order_submission_enabled() {
            return Ok(Some(LiveExecutionGateReason::OrderSubmissionDisabled));
        }
        if !self.config.submit_auth_available() {
            bail!("live submit auth is incomplete");
        }
        if self.live_status().await?.entries_enabled {
            return Ok(None);
        }
        if self.global_entry_gate.lock().await.halted {
            return Ok(Some(LiveExecutionGateReason::GlobalHalt));
        }
        let state = self.readiness_state.lock().await;
        if state.reconciliation_error.is_some() {
            return Ok(Some(LiveExecutionGateReason::ProcessAccountingReadiness));
        }
        if !state.manual_entries_enabled {
            return Ok(Some(LiveExecutionGateReason::ManualEnableRequired));
        }
        if !state.process_accounting_entry_safe {
            return Ok(Some(LiveExecutionGateReason::ProcessAccountingReadiness));
        }
        Ok(Some(LiveExecutionGateReason::VenueReadiness))
    }

    pub(super) async fn final_submission_gate_reason(
        &self,
    ) -> Result<Option<LiveExecutionGateReason>> {
        if self.global_entry_gate.lock().await.halted {
            return Ok(Some(LiveExecutionGateReason::GlobalHalt));
        }
        self.current_entry_gate_reason().await
    }

    pub(super) async fn mark_idempotency_dirty(&self) {
        self.readiness_state.lock().await.idempotency_clean = false;
    }
}
