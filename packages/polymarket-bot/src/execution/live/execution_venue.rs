use super::*;

#[async_trait]
impl ExecutionVenue for LiveVenue {
    fn request_post_order_reconciliation(&self) {
        self.post_order_reconciliation_generation
            .fetch_add(1, Ordering::AcqRel);
    }

    fn reconciliation_requested(&self) -> bool {
        self.http_fallback_requested.load(Ordering::Acquire)
            || self
                .post_order_reconciliation_generation
                .load(Ordering::Acquire)
                > self
                    .post_order_reconciled_generation
                    .load(Ordering::Acquire)
    }

    fn synchronous_post_order_fill_lookup(&self) -> bool {
        false
    }

    fn synchronous_post_order_reconciliation(&self) -> bool {
        false
    }

    fn preserve_liveness_on_post_order_reconcile_error(&self) -> bool {
        true
    }

    fn reconciliation_error_is_transient(&self, error: &anyhow::Error) -> bool {
        is_transient_failure(error)
    }

    async fn find_existing_order(&self, request: &OrderRequest) -> Result<Option<OrderRecord>> {
        self.validate_request_process(request)?;
        let Some(store) = self.store.as_ref() else {
            if cfg!(test) {
                return Ok(None);
            }
            bail!("live persistence store is not configured");
        };
        if store
            .find_order_by_client_order_id(request.client_order_id)
            .await?
            .is_none()
        {
            return Ok(None);
        }
        let (existing, newly_created) = store.create_pending_order(request).await?;
        debug_assert!(!newly_created);
        Ok(Some(existing))
    }

    async fn submit_order(&self, request: OrderRequest) -> Result<OrderRecord> {
        self.validate_request_process(&request)?;
        bail!("process-bound live venue submission requires an adjacent pre-POST guard")
    }

    async fn submit_order_with_pre_post_guard(
        &self,
        request: OrderRequest,
        pre_post_guard: Option<Arc<dyn LivePrePostGuard>>,
    ) -> Result<OrderRecord> {
        let process_id = self.validate_request_process(&request)?;
        let pre_post_guard = pre_post_guard
            .context("process-bound live venue submission requires an adjacent pre-POST guard")?;
        let _submit_guard = self.submit_guard.lock().await;
        if let Some(existing) = self.find_existing_order(&request).await? {
            return Ok(existing);
        }
        let notional = request.price * request.size;
        if self
            .bound_execution()?
            .max_order_notional_usd
            .is_some_and(|maximum| notional > maximum)
        {
            return live_execution_gate_closed_order(
                request,
                LiveExecutionGateReason::PerOrderNotionalLimit,
            );
        }
        if !self.order_submission_enabled() {
            return live_execution_gate_closed_order(
                request,
                LiveExecutionGateReason::OrderSubmissionDisabled,
            );
        }
        let global_halted = {
            let global = self.global_entry_gate.lock().await;
            global.halted
        };
        if global_halted {
            return live_execution_gate_closed_order(request, LiveExecutionGateReason::GlobalHalt);
        }
        if let Some(reason) = self.current_entry_gate_reason().await? {
            return live_execution_gate_closed_order(request, reason);
        }
        let store = self.store()?;
        let client = match self
            .authenticated_client()
            .await
            .context("failed to prepare authenticated Polymarket CLOB client")
        {
            Ok(client) => client,
            Err(error) if is_retryable_live_pre_submit_error(&error) => {
                return live_pre_submit_transient_gate_order(
                    request,
                    "authenticated_client",
                    &error,
                );
            }
            Err(error) => return Err(error),
        };
        let token_id =
            U256::from_str(&request.token_id).context("failed to parse CLOB token_id")?;
        let (risk_result, metadata_result) = tokio::join!(
            self.enforce_submission_risk(process_id, &request, None),
            self.prewarm_order_metadata(&client, token_id),
        );
        let risk_result = match risk_result {
            Ok(result) => result,
            Err(failure) => {
                crate::btc::unified_model_runtime::telemetry::live_submission_risk_check(
                    process_id,
                    "evidence_error",
                    failure.gate_reason.as_str(),
                    false,
                    None,
                    None,
                    0,
                    0,
                );
                warn!(
                    client_order_id = %request.client_order_id,
                    process_id = ?request.process_id,
                    gate_reason = failure.gate_reason.as_str(),
                    error = %format!("{:#}", failure.error),
                    "live pre-submit risk validation rejected this order; preserving trading process liveness"
                );
                return live_execution_gate_closed_order(request, failure.gate_reason);
            }
        };
        let daily_net_pnl_usd = risk_result
            .daily_pnl
            .and_then(|evidence| evidence.net_pnl.to_f64());
        let daily_loss_headroom_usd = risk_result
            .daily_pnl
            .zip(risk_result.max_daily_loss_usd)
            .and_then(|(evidence, maximum)| (evidence.net_pnl + maximum).to_f64());
        let (pending_redemption_count, credited_count) = risk_result
            .daily_pnl
            .map(|evidence| (evidence.pending_redemption_count, evidence.credited_count))
            .unwrap_or_default();
        crate::btc::unified_model_runtime::telemetry::live_submission_risk_check(
            process_id,
            if risk_result.gate_reason.is_some() {
                "rejected"
            } else {
                "allowed"
            },
            risk_result
                .gate_reason
                .map(LiveExecutionGateReason::as_str)
                .unwrap_or("allowed"),
            true,
            daily_net_pnl_usd,
            daily_loss_headroom_usd,
            pending_redemption_count,
            credited_count,
        );
        if let Some(reason) = risk_result.gate_reason {
            return live_execution_gate_closed_order(request, reason);
        }
        if let Err(error) = metadata_result {
            if is_retryable_live_pre_submit_error(&error) {
                return live_pre_submit_transient_gate_order(request, "order_metadata", &error);
            }
            return Err(error);
        }

        // Complete all local validation, CLOB metadata reads and signing before claiming a pending
        // order. The durable row is then written immediately before the only operation whose
        // outcome can be ambiguous: the venue POST.
        let signable = match client
            .limit_order()
            .token_id(token_id)
            .side(sdk_side(request.side))
            .price(sdk_decimal(request.price)?)
            .size(sdk_decimal(request.size)?)
            .order_type(sdk_order_type(request.order_type)?)
            .build()
            .await
            .context("failed to build Polymarket CLOB order")
        {
            Ok(signable) => signable,
            Err(error) if is_retryable_live_pre_submit_error(&error) => {
                return live_pre_submit_transient_gate_order(request, "order_build", &error);
            }
            Err(error) => return Err(error),
        };
        let signer = self
            .signer
            .as_deref()
            .context("missing live submit signer")?;
        let signed = client
            .sign(signer, signable)
            .await
            .context("failed to sign Polymarket CLOB order")?;
        let (pending_order, newly_created) = store.create_pending_order(&request).await?;
        if !newly_created {
            return Ok(pending_order);
        }
        let final_gate_reason = match self.final_submission_gate_reason().await {
            Ok(reason) => reason,
            Err(error) => {
                persist_pre_submit_hard_failure(&store, &pending_order, &error).await?;
                return Err(error);
            }
        };
        if let Some(reason) = final_gate_reason {
            return persist_pre_submit_gate_rejection(&store, pending_order, reason).await;
        }
        let admitted_safety_generation = {
            let global = self.global_entry_gate.lock().await;
            if global.halted || global.safety_generation == u64::MAX {
                return persist_pre_submit_gate_rejection(
                    &store,
                    pending_order,
                    LiveExecutionGateReason::GlobalHalt,
                )
                .await;
            }
            global.safety_generation
        };
        let guard_reason = match pre_post_guard.validate_pre_post(&request).await {
            Ok(reason) => reason,
            Err(error) => {
                persist_pre_submit_hard_failure(&store, &pending_order, &error).await?;
                return Err(error);
            }
        };
        if let Some(reason) = guard_reason {
            return persist_pre_submit_gate_rejection(&store, pending_order, reason).await;
        }
        // Revalidate the checked process authorization after every awaited check and immediately
        // before the venue POST. A safety halt changes the generation and converts this pending
        // order into a durable zero-POST rejection.
        let commit_reason = {
            let mut state = self.readiness_state.lock().await;
            let mut global = self.global_entry_gate.lock().await;
            commit_live_post_attempt(&mut global, &mut state, admitted_safety_generation)
        };
        if let Some(reason) = commit_reason {
            return persist_pre_submit_gate_rejection(&store, pending_order, reason).await;
        }
        pre_post_guard.observe_post_attempt(&request);
        let post_started = std::time::Instant::now();
        let submit_result = client
            .post_order(signed)
            .await
            .context("Polymarket CLOB order submit failed");

        match submit_result {
            Ok(response) if response.success => {
                pre_post_guard.observe_post_result(
                    &request,
                    post_started.elapsed(),
                    "acknowledged",
                );
                let raw_ack = post_order_response_payload(&response);
                store
                    .mark_order_submitted(request.client_order_id, &response.order_id, raw_ack)
                    .await
            }
            Ok(response) => {
                pre_post_guard.observe_post_result(
                    &request,
                    post_started.elapsed(),
                    "venue_rejected",
                );
                let reject_reason = definitive_live_venue_reject_reason(
                    request.order_type,
                    response.error_msg.as_deref(),
                );
                let raw = with_live_venue_reject_reason(
                    post_order_response_payload(&response),
                    reject_reason,
                );
                let failed_order = self
                    .store()?
                    .mark_order_submit_failed(request.client_order_id, "venue_rejected", raw)
                    .await?;
                let error_msg = response
                    .error_msg
                    .unwrap_or_else(|| "unknown rejection".to_string());
                warn!(
                    client_order_id = %request.client_order_id,
                    error = %error_msg,
                    "Polymarket CLOB definitively rejected order; preserving trading process liveness"
                );
                Ok(failed_order)
            }
            Err(error) => {
                pre_post_guard.observe_post_result(
                    &request,
                    post_started.elapsed(),
                    "transport_failure",
                );
                let error_chain = format!("{error:#}");
                if is_definitive_live_submit_error(&error) {
                    let reject_reason =
                        definitive_live_venue_reject_reason(request.order_type, Some(&error_chain));
                    let failed_order = store
                        .mark_order_submit_failed(
                            request.client_order_id,
                            "venue_rejected",
                            with_live_venue_reject_reason(
                                json!({
                                    "error": error.to_string(),
                                    "error_chain": error_chain
                                }),
                                reject_reason,
                            ),
                        )
                        .await?;
                    warn!(
                        client_order_id = %request.client_order_id,
                        error = %error_chain,
                        "Polymarket CLOB definitively rejected order; preserving trading process liveness"
                    );
                    return Ok(failed_order);
                } else {
                    store
                        .mark_order_submit_unknown(
                            request.client_order_id,
                            "ambiguous_submit_error",
                            json!({
                                "error": error.to_string(),
                                "error_chain": error_chain
                            }),
                        )
                        .await?;
                    self.mark_idempotency_dirty().await;
                }
                Err(error)
            }
        }
    }

    async fn cancel_order(&self, order_id: &str) -> Result<OrderRecord> {
        let store = self.store()?;
        if let Some(process_id) = self.bound_process_id {
            let order = store
                .find_order_by_venue_order_id(order_id)
                .await?
                .with_context(|| {
                    format!("live process {process_id} cannot cancel an unowned order {order_id}")
                })?;
            if order.request.process_id != Some(process_id) {
                bail!(
                    "live process {} cannot cancel order {} owned by {:?}",
                    process_id,
                    order_id,
                    order.request.process_id
                );
            }
            if order.order_id.starts_with("live-pending-") {
                bail!(
                    "live process {} cannot cancel pending order {} before venue identity reconciliation",
                    process_id,
                    order_id
                );
            }
        }
        store
            .mark_cancel_requested(order_id, json!({"requested_at": Utc::now()}))
            .await?;
        let client = self.authenticated_client().await?;
        let response = client
            .cancel_order(order_id)
            .await
            .context("Polymarket CLOB cancel_order failed")?;
        if response.canceled.iter().any(|id| id == order_id) {
            if let Some(order) = self
                .store()?
                .mark_order_cancelled(
                    order_id,
                    json!({
                        "canceled": &response.canceled,
                        "not_canceled": &response.not_canceled
                    }),
                )
                .await?
            {
                return Ok(order);
            }
        }
        if let Some(reason) = response.not_canceled.get(order_id) {
            bail!("Polymarket CLOB did not cancel order {order_id}: {reason}");
        }
        bail!("Polymarket CLOB did not confirm cancellation for order {order_id}")
    }

    async fn cancel_all(&self) -> Result<usize> {
        if let Some(process_id) = self.bound_process_id {
            let orders = self.bounded_nonterminal_orders(process_id).await?;
            let mut cancelled = 0usize;
            for order in orders {
                if order.order_id.starts_with("live-pending-") {
                    self.mark_idempotency_dirty().await;
                    bail!(
                        "live process {} has pending order {} without a reconciled venue identity",
                        process_id,
                        order.order_id
                    );
                }
                self.cancel_order(&order.order_id).await?;
                cancelled = cancelled.saturating_add(1);
            }
            return Ok(cancelled);
        }

        // The unbound venue is reserved for the explicitly wallet-wide administrative halt path.
        let store = self.store()?;
        let client = self.authenticated_client().await?;
        let response = client
            .cancel_all_orders()
            .await
            .context("Polymarket CLOB cancel_all failed")?;
        let raw = json!({
            "canceled": &response.canceled,
            "not_canceled": &response.not_canceled
        });
        for order_id in &response.canceled {
            let _ = store.mark_order_cancelled(order_id, raw.clone()).await?;
        }
        Ok(response.canceled.len())
    }

    async fn get_balances(&self) -> Result<Vec<(String, Decimal)>> {
        let client = self.authenticated_client().await?;
        let balance = client
            .balance_allowance(
                BalanceAllowanceRequest::builder()
                    .asset_type(AssetType::Collateral)
                    .signature_type(parse_signature_type(self.config.signature_type.as_deref())?)
                    .build(),
            )
            .await
            .context("Polymarket CLOB balance_allowance failed")?;
        Ok(vec![("USDC".to_string(), local_decimal(balance.balance)?)])
    }

    async fn get_open_orders(&self) -> Result<Vec<OrderRecord>> {
        let client = self.authenticated_client().await?;
        self.all_open_order_responses(&client)
            .await?
            .into_iter()
            .map(order_record_from_open_order)
            .collect::<Result<Vec<_>>>()
    }

    async fn reconcile(&self) -> Result<ReconciliationReport> {
        let _reconcile_guard = self.reconcile_guard.lock().await;
        let checked_at = Utc::now();
        let post_order_generation = self
            .post_order_reconciliation_generation
            .load(Ordering::Acquire);
        let (source, fallback_reason, continuity_generation) = {
            let readiness = self.readiness_state.lock().await;
            let transport = self.transport_state.lock().await;
            if readiness.last_rest_reconcile_at.is_none() {
                ("startup", None, transport.continuity_generation)
            } else if transport.continuity_uncertain {
                (
                    "http_fallback",
                    transport.fallback_reason,
                    transport.continuity_generation,
                )
            } else if post_order_generation
                > self
                    .post_order_reconciled_generation
                    .load(Ordering::Acquire)
            {
                ("post_order", None, transport.continuity_generation)
            } else {
                ("http_periodic", None, transport.continuity_generation)
            }
        };
        if let Some(metrics) = &self.reconciliation_metrics {
            metrics.record_started(source, fallback_reason);
        }
        let reconciliation_safety_generation = {
            let global = self.global_entry_gate.lock().await;
            if global.safety_generation == u64::MAX {
                let error = anyhow::anyhow!(
                    "live safety generation is exhausted; reconciliation remains fail-closed"
                );
                if let Some(metrics) = &self.reconciliation_metrics {
                    metrics.record_failure(ReconciliationStage::SafetyGeneration, &error);
                }
                return Err(error);
            }
            global.safety_generation
        };
        let store = match at_stage(ReconciliationStage::Persistence, self.store()) {
            Ok(store) => store,
            Err(failure) => {
                if let Some(metrics) = &self.reconciliation_metrics {
                    metrics.record_failure(failure.stage, &failure.error);
                }
                return Err(failure.error);
            }
        };
        let reconcile_result = async {
            let mut fills_backfilled = at_stage(
                ReconciliationStage::UserEventBackfill,
                self.backfill_fills_from_live_events()
                    .await
                    .context("failed to backfill live user websocket fills"),
            )?;
            let mut http_fills_recovered = 0usize;
            let open_orders = at_stage(
                ReconciliationStage::OpenOrders,
                self.get_open_orders().await,
            )?;
            let mut local_nonterminal = match self.bound_process_id {
                Some(process_id) => at_stage(
                    ReconciliationStage::LocalOrders,
                    self.bounded_nonterminal_orders(process_id).await,
                )?,
                None => Vec::new(),
            };
            let trades = if self.bound_process_id.is_some() {
                let trade_window_start = at_stage(
                    ReconciliationStage::LocalOrders,
                    reconciliation_trade_window_start(&local_nonterminal, checked_at),
                )?;
                let trades_request = TradesRequest::builder()
                    .after(trade_window_start.timestamp())
                    .before(checked_at.timestamp())
                    .build();
                at_stage(
                    ReconciliationStage::Trades,
                    self.all_trade_responses(&trades_request).await,
                )?
            } else {
                Vec::new()
            };
            let balances = at_stage(
                ReconciliationStage::Balances,
                self.get_balances().await,
            )?;
            if balances.is_empty() {
                return at_stage(
                    ReconciliationStage::Balances,
                    Err(anyhow::anyhow!(
                        "Polymarket CLOB balance reconciliation returned no assets"
                    )),
                );
            }
            let owned_orders = if self.bound_process_id.is_some() {
                let mut venue_order_ids = open_orders
                    .iter()
                    .map(|order| order.order_id.clone())
                    .collect::<Vec<_>>();
                for trade in &trades {
                    let candidate_count = at_stage(
                        ReconciliationStage::OrderOwnership,
                        trade
                            .maker_orders
                            .len()
                            .checked_add(1)
                            .context("Polymarket CLOB trade order identity count overflow"),
                    )?;
                    let total_candidate_count = at_stage(
                        ReconciliationStage::OrderOwnership,
                        venue_order_ids
                            .len()
                            .checked_add(candidate_count)
                            .context("Polymarket CLOB reconciliation order identity overflow"),
                    )?;
                    if total_candidate_count > MAX_CLOB_RECONCILIATION_ORDER_IDS {
                        return at_stage(
                            ReconciliationStage::OrderOwnership,
                            Err(anyhow::anyhow!(
                                "Polymarket CLOB reconciliation exceeds the bounded {}-order identity window",
                                MAX_CLOB_RECONCILIATION_ORDER_IDS
                            )),
                        );
                    }
                    venue_order_ids.push(trade.taker_order_id.clone());
                    venue_order_ids.extend(
                        trade
                            .maker_orders
                            .iter()
                            .map(|maker_order| maker_order.order_id.clone()),
                    );
                }
                at_stage(
                    ReconciliationStage::OrderOwnership,
                    self.bound_account_venue_order_ids(
                        &store,
                        at_stage(
                            ReconciliationStage::OrderOwnership,
                            self.bound_account_ref()
                                .context("live reconciliation requires account_ref"),
                        )?,
                        venue_order_ids.iter().map(String::as_str),
                    )
                    .await,
                )?
            } else {
                HashMap::new()
            };
            if let Some(process_id) = self.bound_process_id {
                let rest_fills = at_stage(
                    ReconciliationStage::RestFillBackfill,
                    persist_rest_fill_backfill(
                        &store,
                        process_id,
                        &local_nonterminal,
                        &owned_orders,
                        &trades,
                        checked_at,
                    )
                    .await
                    .context("failed to backfill authenticated CLOB REST fills"),
                )?;
                http_fills_recovered = rest_fills.recovered_count;
                fills_backfilled = at_stage(
                    ReconciliationStage::RestFillBackfill,
                    fills_backfilled
                        .checked_add(rest_fills.fills.len())
                        .context("live reconciliation fill backfill count overflow"),
                )?;
                // Reconciliation readiness must be derived from the state after REST evidence is
                // durable and cumulative fill progress has terminalized fully filled orders.
                local_nonterminal = at_stage(
                    ReconciliationStage::LocalOrders,
                    self.bounded_nonterminal_orders(process_id).await,
                )?;
            }
            // Position ownership is reconstructed from persisted live fills, so reconcile the
            // wallet only after authenticated REST evidence has been backfilled durably.
            let account_reconcile = at_stage(
                ReconciliationStage::AccountReconciliation,
                self.run_account_reconcile(AccountReconcileRequest {
                    account_address: None,
                    lookback_hours: Some(1),
                    process_id: self.bound_process_id,
                    account_ref: self.bound_account_ref.clone(),
                    credential_account_fingerprint_sha256: None,
                    dry_run: false,
                    token_id: None,
                    source: Some("poll".to_string()),
                })
                .await
                .context("live account reconciliation polling backup failed"),
            )?;
            let owned_venue_order_ids = owned_orders
                .keys()
                .map(String::as_str)
                .collect::<HashSet<_>>();
            let owned_local_order_ids = owned_orders
                .values()
                .map(String::as_str)
                .collect::<HashSet<_>>();
            let foreign_venue_orders = if self.bound_process_id.is_some() {
                open_orders
                    .iter()
                    .filter(|order| !owned_venue_order_ids.contains(order.order_id.as_str()))
                    .count()
            } else {
                0
            };
            let foreign_wallet_trades = trades
                .iter()
                .filter(|trade| {
                    !owned_venue_order_ids.contains(trade.taker_order_id.as_str())
                        && !trade.maker_orders.iter().any(|maker_order| {
                            owned_venue_order_ids.contains(maker_order.order_id.as_str())
                        })
                })
                .count();
            let missing_local_orders = local_nonterminal
                .iter()
                .filter(|order| !owned_local_order_ids.contains(order.order_id.as_str()))
                .count();
            let unresolved = if self.bound_process_id.is_some() {
                local_nonterminal.len()
            } else {
                open_orders.len()
            };
            // Wallet trades are account-scoped and may belong to a sibling sleeve or an exact
            // reconciled manual exit. The account reconciliation report is the authoritative
            // ownership result; keep the raw REST count diagnostic-only to avoid double-counting.
            let mismatches = missing_local_orders
                .saturating_add(foreign_venue_orders)
                .saturating_add(account_reconcile.mismatches.len())
                .saturating_add(account_reconcile.unmatched_trades as usize);
            if self.global_entry_gate.lock().await.safety_generation
                != reconciliation_safety_generation
            {
                return at_stage(
                    ReconciliationStage::SafetyGeneration,
                    Err(anyhow::anyhow!(
                        "live safety generation changed during reconciliation"
                    )),
                );
            }
            Ok::<_, StagedReconciliationError>((
                fills_backfilled,
                account_reconcile,
                open_orders,
                unresolved,
                mismatches,
                foreign_venue_orders,
                foreign_wallet_trades,
                http_fills_recovered,
            ))
        }
        .await;

        let (
            fills_backfilled,
            account_reconcile,
            open_orders,
            unresolved,
            mismatches,
            foreign_venue_orders,
            foreign_wallet_trades,
            http_fills_recovered,
        ) = match reconcile_result {
            Ok(result) => result,
            Err(failure) => {
                if let Err(record_error) = store
                    .insert_live_reconciliation_run(
                        self.bound_process_id,
                        self.bound_account_ref.as_deref(),
                        "failed",
                        0,
                        0,
                        0,
                        0,
                        0,
                        1,
                        json!({
                            "process_id": self.bound_process_id,
                            "error": failure.error.to_string(),
                            "checked_at": checked_at,
                        }),
                    )
                    .await
                {
                    warn!(
                        error = %record_error,
                        reconciliation_error = %failure.error,
                        "failed to persist failed live reconciliation run"
                    );
                }
                if let Some(metrics) = &self.reconciliation_metrics {
                    metrics.record_failure(failure.stage, &failure.error);
                }
                return Err(failure.error);
            }
        };

        let report = ReconciliationReport {
            open_orders: open_orders.len(),
            balances_checked: true,
            mismatches_found: mismatches,
            unresolved_count: unresolved,
            checked_at,
        };
        let idempotency_clean = report.unresolved_count == 0 && report.mismatches_found == 0;
        if let Err(error) = store
            .insert_live_reconciliation_run(
                self.bound_process_id,
                self.bound_account_ref.as_deref(),
                "completed",
                report.open_orders as i32,
                0,
                1,
                report.mismatches_found as i32,
                fills_backfilled as i32,
                report.unresolved_count as i32,
                serde_json::json!({
                    "process_id": self.bound_process_id,
                    "venue": report,
                    "account_reconcile": account_reconcile,
                    "process_accounting_proof": &account_reconcile.process_accounting_proof,
                    "credential_account_fingerprint_sha256": &account_reconcile.credential_account_fingerprint_sha256,
                    "foreign_wallet_open_orders": foreign_venue_orders,
                    "foreign_wallet_trades": foreign_wallet_trades,
                    "idempotency_clean": idempotency_clean,
                    "reconciliation_safety_generation": reconciliation_safety_generation,
                }),
            )
            .await
        {
            if let Some(metrics) = &self.reconciliation_metrics {
                metrics.record_failure(ReconciliationStage::Persistence, &error);
            }
            return Err(error);
        }
        {
            let mut state = self.readiness_state.lock().await;
            let global = self.global_entry_gate.lock().await;
            state.last_rest_reconcile_at = Some(checked_at);
            state.unresolved_live_order_count = unresolved;
            state.idempotency_clean = idempotency_clean;
            state.process_accounting_proven = account_reconcile.process_accounting_proven;
            state.process_accounting_entry_safe = account_reconcile.process_accounting_entry_safe;
            state.process_accounting_status = account_reconcile.process_accounting_status.clone();
            state.credential_account_fingerprint_sha256 = account_reconcile
                .credential_account_fingerprint_sha256
                .clone();
            if self.bound_process_id.is_some()
                && idempotency_clean
                && global.safety_generation == reconciliation_safety_generation
            {
                state.reconciled_safety_generation = Some(reconciliation_safety_generation);
            } else {
                state.reconciled_safety_generation = None;
            }
            if self.bound_process_id.is_some()
                && idempotency_clean
                && global.safety_generation != reconciliation_safety_generation
            {
                state.idempotency_clean = false;
                let error =
                    anyhow::anyhow!("live safety generation changed before reconciliation commit");
                if let Some(metrics) = &self.reconciliation_metrics {
                    metrics.record_failure(ReconciliationStage::SafetyGeneration, &error);
                }
                return Err(error);
            }
        }
        {
            let mut transport = self.transport_state.lock().await;
            if transport.continuity_generation == continuity_generation {
                transport.continuity_uncertain = false;
                transport.fallback_reason = None;
                self.http_fallback_requested.store(false, Ordering::Release);
            }
        }
        if let Some(metrics) = &self.reconciliation_metrics {
            metrics.record_report(&report, http_fills_recovered);
        }
        self.post_order_reconciled_generation
            .store(post_order_generation, Ordering::Release);
        Ok(report)
    }

    async fn fills_for_order(&self, order_id: &str) -> Result<Vec<FillRecord>> {
        let store = self.store()?;
        let persisted_order = store
            .find_order_by_venue_order_id(order_id)
            .await?
            .with_context(|| format!("cannot reconcile fills for unknown live order {order_id}"))?;
        if let Some(process_id) = self.bound_process_id {
            if persisted_order.request.process_id != Some(process_id) {
                bail!(
                    "live process {} cannot reconcile fills for order {} owned by {:?}",
                    process_id,
                    order_id,
                    persisted_order.request.process_id
                );
            }
        }
        let order_process_id = persisted_order
            .request
            .process_id
            .context("cannot reconcile fills for a live order without process ownership")?;
        let trades_request = TradesRequest::builder()
            .asset_id(
                U256::from_str(&persisted_order.request.token_id)
                    .context("failed to parse persisted CLOB token_id")?,
            )
            .after((persisted_order.created_at - LIVE_FILL_RECONCILIATION_SKEW).timestamp())
            .before((persisted_order.created_at + LIVE_FILL_RECONCILIATION_SKEW).timestamp())
            .build();
        let trades = self.all_trade_responses(&trades_request).await?;
        let owned_orders =
            HashMap::from([(order_id.to_string(), persisted_order.order_id.clone())]);
        Ok(persist_rest_fill_backfill(
            &store,
            order_process_id,
            std::slice::from_ref(&persisted_order),
            &owned_orders,
            &trades,
            Utc::now(),
        )
        .await?
        .fills)
    }

    async fn update_live_reconciliation_health(
        &self,
        pending_settlement_count: usize,
        error: Option<String>,
    ) -> Result<()> {
        if self.bound_process_id.is_none() {
            return Ok(());
        }
        let mut state = self.readiness_state.lock().await;
        state.pending_settlement_count = pending_settlement_count;
        state.reconciliation_error = error.map(|error| {
            let mut error = error;
            error.truncate(512);
            error
        });
        if state.reconciliation_error.is_some() {
            state.reconciled_safety_generation = None;
        }
        if let Some(metrics) = &self.reconciliation_metrics {
            metrics.set_entry_safe(
                state.reconciliation_error.is_none()
                    && state.idempotency_clean
                    && state.unresolved_live_order_count == 0
                    && state.process_accounting_entry_safe,
            );
        }
        Ok(())
    }

    async fn live_status(&self) -> Result<LiveVenueStatus> {
        let transport = self.transport_state.lock().await;
        let state = self.readiness_state.lock().await;
        let global = self.global_entry_gate.lock().await;
        let now = Utc::now();
        let last_user_ws_pong_age_secs = stateful_age_seconds(transport.last_user_ws_pong_at, now);
        let last_rest_reconcile_age_secs = stateful_age_seconds(state.last_rest_reconcile_at, now);
        let rest_fresh = last_rest_reconcile_age_secs
            .map(|age| age <= self.config.stale_reconcile.as_secs() as i64)
            .unwrap_or(false);
        let order_submit_enabled = self.order_submission_enabled();
        let live_confirmed = order_submit_enabled
            && self.config.submit_auth_available()
            && state.process_accounting_entry_safe
            && rest_fresh;
        let entries_enabled = live_confirmed
            && !global.halted
            && state.manual_entries_enabled
            && state.idempotency_clean
            && state.unresolved_live_order_count == 0
            && state.reconciliation_error.is_none();
        let reason = if entries_enabled {
            None
        } else if !order_submit_enabled {
            Some("live_order_submit_disabled".to_string())
        } else if !self.config.submit_auth_available() {
            Some("live_submit_auth_missing".to_string())
        } else if global.halted {
            Some(format!("live_global_halt:{}", global.reason))
        } else if !state.manual_entries_enabled {
            state
                .manual_entries_reason
                .clone()
                .or_else(|| Some("manual_enable_required".to_string()))
        } else if !state.process_accounting_entry_safe {
            Some(format!(
                "live_process_accounting_not_proven:{}",
                state.process_accounting_status
            ))
        } else if let Some(error) = state.reconciliation_error.as_deref() {
            Some(format!("live_reconciliation_degraded:{error}"))
        } else if !rest_fresh {
            Some("live_rest_reconcile_stale".to_string())
        } else if !state.idempotency_clean {
            Some("live_idempotency_not_clean".to_string())
        } else if state.unresolved_live_order_count > 0 {
            Some("live_unresolved_orders_present".to_string())
        } else {
            Some("live_not_ready".to_string())
        };
        Ok(LiveVenueStatus {
            mode: "live".to_string(),
            live_confirmed,
            geoblock_readable: false,
            geoblock_blocked: None,
            geoblock_country: None,
            geoblock_region: None,
            last_geoblock_check_age_secs: None,
            order_submit_enabled,
            user_ws_enabled: self.config.user_ws_auth_available(),
            user_ws_connected: transport.user_ws_connected,
            last_user_ws_pong_age_secs,
            last_rest_reconcile_age_secs,
            idempotency_clean: state.idempotency_clean,
            unresolved_live_order_count: state.unresolved_live_order_count,
            process_accounting_proven: state.process_accounting_proven,
            process_accounting_status: state.process_accounting_status.clone(),
            max_order_notional_usd: self
                .bound_execution
                .as_ref()
                .and_then(|execution| execution.max_order_notional_usd)
                .unwrap_or(Decimal::ZERO),
            max_open_notional_usd: self
                .bound_execution
                .as_ref()
                .and_then(|execution| execution.max_open_notional_usd)
                .unwrap_or(Decimal::ZERO),
            entries_enabled,
            reason,
        })
    }

    async fn live_identity_diagnostics(&self) -> Result<LiveIdentityDiagnostics> {
        let signature_type = parse_signature_type(self.config.signature_type.as_deref())?;
        let signer_address = self
            .config
            .private_key
            .as_deref()
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
            .transpose()?;
        let credentials_present = self
            .config
            .clob_api_key
            .as_deref()
            .is_some_and(|value| !value.is_empty())
            && self
                .config
                .clob_secret
                .as_deref()
                .is_some_and(|value| !value.is_empty())
            && self
                .config
                .clob_passphrase
                .as_deref()
                .is_some_and(|value| !value.is_empty());

        let mut diagnostics = LiveIdentityDiagnostics {
            mode: "live".to_string(),
            clob_api_base_url: self.clob_base_url.clone(),
            geoblock_readable: false,
            geoblock_blocked: None,
            geoblock_country: None,
            geoblock_region: None,
            geoblock_error: None,
            signer_address,
            configured_funder_address: self.config.funder_address.clone(),
            configured_signature_type: self.config.signature_type.clone(),
            resolved_signature_type: Some(format!("{signature_type:?}")),
            authenticated_client_address: None,
            account_identity_valid: false,
            account_identity_fingerprint_sha256: None,
            credentials_present,
            api_keys_readable: false,
            api_keys_error: None,
            balance_allowance_readable: false,
            balance_allowance_error: None,
            collateral_balance: None,
            open_orders_readable: false,
            open_orders_error: None,
            open_orders_count: None,
            checked_at: Utc::now(),
        };

        let client = match self.authenticated_client().await {
            Ok(client) => client,
            Err(error) => {
                let message = error.to_string();
                diagnostics.api_keys_error = Some(message.clone());
                diagnostics.balance_allowance_error = Some(message.clone());
                diagnostics.open_orders_error = Some(message);
                return Ok(diagnostics);
            }
        };
        let authenticated_client_address = client.address().to_checksum(None);
        diagnostics.authenticated_client_address = Some(authenticated_client_address.clone());
        match self.bound_account_ref().map_or_else(
            || {
                Err(anyhow::anyhow!(
                    "live identity diagnostics require a process account_ref"
                ))
            },
            |account_ref| canonical_configured_account_identity(&self.config, account_ref),
        ) {
            Ok(identity)
                if identity.signature_type == signature_type
                    && authenticated_client_address
                        .eq_ignore_ascii_case(&identity.signer_address) =>
            {
                // The SDK client authenticates as the private-key signer for every signature
                // type. The canonical trading account is the signer only for EOA; for proxy,
                // Safe, and POLY_1271 it is the validated maker/funder carried by `identity`.
                diagnostics.account_identity_valid = true;
                diagnostics.account_identity_fingerprint_sha256 = Some(identity.fingerprint_sha256);
            }
            Ok(_) => {
                let message =
                    "authenticated CLOB signer does not match canonical signature-aware identity"
                        .to_string();
                diagnostics.api_keys_error = Some(message.clone());
                diagnostics.balance_allowance_error = Some(message.clone());
                diagnostics.open_orders_error = Some(message);
                return Ok(diagnostics);
            }
            Err(error) => {
                let message = error.to_string();
                diagnostics.api_keys_error = Some(message.clone());
                diagnostics.balance_allowance_error = Some(message.clone());
                diagnostics.open_orders_error = Some(message);
                return Ok(diagnostics);
            }
        }

        match client.api_keys().await {
            Ok(_) => diagnostics.api_keys_readable = true,
            Err(error) => diagnostics.api_keys_error = Some(error.to_string()),
        }

        if let Err(error) = client
            .update_balance_allowance(
                UpdateBalanceAllowanceRequest::builder()
                    .asset_type(AssetType::Collateral)
                    .signature_type(signature_type)
                    .build(),
            )
            .await
        {
            diagnostics.balance_allowance_error = Some(error.to_string());
        }

        match client
            .balance_allowance(
                BalanceAllowanceRequest::builder()
                    .asset_type(AssetType::Collateral)
                    .signature_type(signature_type)
                    .build(),
            )
            .await
        {
            Ok(balance) => {
                diagnostics.balance_allowance_readable = true;
                diagnostics.collateral_balance = Some(local_decimal(balance.balance)?.to_string());
                if !collateral_allowances_positive(&balance.allowances) {
                    diagnostics.balance_allowance_error = Some(
                        "CLOB collateral allowances are missing, zero, or invalid".to_string(),
                    );
                }
            }
            Err(error) => diagnostics.balance_allowance_error = Some(error.to_string()),
        }

        match self.all_open_order_responses(&client).await {
            Ok(orders) => {
                diagnostics.open_orders_readable = true;
                diagnostics.open_orders_count = Some(orders.len());
            }
            Err(error) => diagnostics.open_orders_error = Some(error.to_string()),
        }

        Ok(diagnostics)
    }

    async fn live_wallet_address_diagnostics(
        &self,
        candidate_addresses: Vec<String>,
    ) -> Result<LiveWalletAddressDiagnostics> {
        let signature_type = parse_signature_type(self.config.signature_type.as_deref())?;
        let signer_address = signer_address_from_private_key(self.config.private_key.as_deref())?;
        let configured_funder_address = self.config.funder_address.clone();
        let signer: Option<Address> = signer_address
            .as_deref()
            .and_then(|address| Address::from_str(address).ok());
        let derived_proxy_wallet_address = signer
            .and_then(|address| derive_proxy_wallet(address, POLYGON))
            .map(|address| address.to_checksum(None));
        let derived_safe_wallet_address = signer
            .and_then(|address| derive_safe_wallet(address, POLYGON))
            .map(|address| address.to_checksum(None));

        let authenticated_client_address = match self.authenticated_client().await {
            Ok(client) => Some(client.address().to_checksum(None)),
            Err(_) => None,
        };

        let expected_order_maker_address = match signature_type {
            SignatureType::Eoa => signer_address.clone(),
            SignatureType::Proxy | SignatureType::GnosisSafe | SignatureType::Poly1271 => {
                configured_funder_address.clone()
            }
            _ => configured_funder_address.clone(),
        };
        let expected_order_signer_field = match signature_type {
            SignatureType::Poly1271 => configured_funder_address.clone(),
            _ => signer_address.clone(),
        };

        let relayer_base_url = std::env::var("POLYMARKET_RELAYER_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_RELAYER_BASE_URL.to_string());
        let (
            configured_funder_deployed_as_deposit_wallet,
            configured_funder_deployed_as_deposit_wallet_error,
            relayer_deployment_check_url,
        ) = check_candidate_relayer_deployment(
            &relayer_base_url,
            configured_funder_address.as_deref(),
            "WALLET",
        )
        .await;

        let rpc_url = std::env::var("POLYMARKET_POLYGON_RPC_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_POLYGON_RPC_URL.to_string());
        let signer_balances = match signer_address.as_deref() {
            Some(address) if is_address_like(address) => {
                Some(wallet_token_balances(&rpc_url, address).await)
            }
            _ => None,
        };
        let configured_funder_balances = match configured_funder_address.as_deref() {
            Some(address) if is_address_like(address) => {
                Some(wallet_token_balances(&rpc_url, address).await)
            }
            _ => None,
        };
        let mut candidate_addresses = wallet_candidate_address_diagnostics(
            candidate_addresses,
            configured_funder_address.as_deref(),
            signer_address.as_deref(),
            authenticated_client_address.as_deref(),
            derived_proxy_wallet_address.as_deref(),
            derived_safe_wallet_address.as_deref(),
            &relayer_base_url,
            &rpc_url,
        )
        .await;
        for candidate in &mut candidate_addresses {
            self.apply_poly1271_candidate_clob_diagnostics(candidate)
                .await;
        }
        let verified_deposit_wallet_addresses = candidate_addresses
            .iter()
            .filter(|candidate| candidate.deployed_as_deposit_wallet == Some(true))
            .map(|candidate| candidate.address.clone())
            .collect::<Vec<_>>();

        Ok(LiveWalletAddressDiagnostics {
            mode: "live".to_string(),
            signer_address: signer_address.clone(),
            configured_funder_address: configured_funder_address.clone(),
            configured_signature_type: self.config.signature_type.clone(),
            resolved_signature_type: Some(format!("{signature_type:?}")),
            authenticated_client_address,
            derived_proxy_wallet_address: derived_proxy_wallet_address.clone(),
            derived_safe_wallet_address: derived_safe_wallet_address.clone(),
            expected_order_maker_address,
            expected_order_signer_field,
            configured_funder_matches_signer: addresses_equal(
                configured_funder_address.as_deref(),
                signer_address.as_deref(),
            ),
            configured_funder_matches_proxy_wallet: addresses_equal(
                configured_funder_address.as_deref(),
                derived_proxy_wallet_address.as_deref(),
            ),
            configured_funder_matches_safe_wallet: addresses_equal(
                configured_funder_address.as_deref(),
                derived_safe_wallet_address.as_deref(),
            ),
            configured_funder_deployed_as_deposit_wallet,
            configured_funder_deployed_as_deposit_wallet_error,
            relayer_base_url: Some(relayer_base_url),
            relayer_deployment_check_url,
            signer_balances,
            configured_funder_balances,
            candidate_addresses,
            verified_deposit_wallet_address: if verified_deposit_wallet_addresses.len() == 1 {
                verified_deposit_wallet_addresses.first().cloned()
            } else {
                None
            },
            verified_deposit_wallet_candidates_count: verified_deposit_wallet_addresses.len(),
            checked_at: Utc::now(),
        })
    }

    async fn live_order_dry_run(
        &self,
        request: LiveOrderDryRunRequest,
    ) -> Result<LiveOrderDryRunDiagnostics> {
        let signature_type = parse_signature_type(self.config.signature_type.as_deref())?;
        let private_key = self
            .config
            .private_key
            .as_deref()
            .context("missing private key")?;
        let signer = LocalSigner::from_str(private_key)
            .context("failed to parse POLYMARKET_PRIVATE_KEY")?
            .with_chain_id(Some(POLYGON));
        let signer_address = signer.address().to_checksum(None);
        let client = self.authenticated_client().await?;
        let authenticated_client_address = client.address().to_checksum(None);
        let token_id =
            U256::from_str(&request.token_id).context("failed to parse CLOB token_id")?;
        let signable = client
            .limit_order()
            .token_id(token_id)
            .side(sdk_side(request.side))
            .price(sdk_decimal(request.price)?)
            .size(sdk_decimal(request.size)?)
            .order_type(sdk_order_type(
                request.order_type.unwrap_or(OrderType::Fok),
            )?)
            .build()
            .await
            .context("failed to build Polymarket CLOB dry-run order")?;
        let signed = client
            .sign(&signer, signable)
            .await
            .context("failed to sign Polymarket CLOB dry-run order")?;
        let mut signed_order =
            serde_json::to_value(&signed).context("failed to serialize dry-run signed order")?;

        let order_signer = signed_order
            .get("order")
            .and_then(|order| order.get("signer"))
            .and_then(|value| value.as_str())
            .map(str::to_string);
        let order_maker = signed_order
            .get("order")
            .and_then(|order| order.get("maker"))
            .and_then(|value| value.as_str())
            .map(str::to_string);
        let order_signature_type = signed_order
            .get("order")
            .and_then(|order| order.get("signatureType"))
            .map(|value| {
                value
                    .as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| value.to_string())
            });

        let mut owner_redacted = false;
        if let Some(owner) = signed_order.get_mut("owner") {
            *owner = json!("<redacted>");
            owner_redacted = true;
        }
        let mut signature_redacted = false;
        if let Some(signature) = signed_order
            .get_mut("order")
            .and_then(|order| order.get_mut("signature"))
        {
            *signature = json!("<redacted>");
            signature_redacted = true;
        }

        Ok(LiveOrderDryRunDiagnostics {
            mode: "live".to_string(),
            clob_api_base_url: self.clob_base_url.clone(),
            signer_address: Some(signer_address),
            configured_funder_address: self.config.funder_address.clone(),
            configured_signature_type: self.config.signature_type.clone(),
            resolved_signature_type: Some(format!("{signature_type:?}")),
            authenticated_client_address: Some(authenticated_client_address.clone()),
            order_signer: order_signer.clone(),
            order_maker: order_maker.clone(),
            order_signature_type,
            order_signer_matches_authenticated_client: addresses_equal(
                order_signer.as_deref(),
                Some(&authenticated_client_address),
            ),
            order_signer_matches_configured_funder: addresses_equal(
                order_signer.as_deref(),
                self.config.funder_address.as_deref(),
            ),
            order_maker_matches_configured_funder: addresses_equal(
                order_maker.as_deref(),
                self.config.funder_address.as_deref(),
            ),
            owner_redacted,
            signature_redacted,
            signed_order,
            checked_at: Utc::now(),
        })
    }

    async fn live_poly1271_funder_probe(
        &self,
        request: LivePoly1271FunderProbeRequest,
    ) -> Result<LivePoly1271FunderProbeResponse> {
        let signer_address = signer_address_from_private_key(self.config.private_key.as_deref())?;
        let order_type = request.order_type.unwrap_or(OrderType::Fok);
        let mut candidates = Vec::with_capacity(request.addresses.len());

        for address in &request.addresses {
            candidates.push(
                self.poly1271_funder_probe_candidate(&request, order_type, address.trim())
                    .await,
            );
        }

        let verified_funder_addresses = candidates
            .iter()
            .filter(|candidate| candidate.ready_for_live_canary)
            .map(|candidate| candidate.address.clone())
            .collect::<Vec<_>>();

        Ok(LivePoly1271FunderProbeResponse {
            mode: "live".to_string(),
            clob_api_base_url: self.clob_base_url.clone(),
            signer_address,
            token_id: request.token_id,
            side: request.side,
            order_type,
            price: request.price,
            size: request.size,
            candidates,
            verified_funder_address: if verified_funder_addresses.len() == 1 {
                verified_funder_addresses.first().cloned()
            } else {
                None
            },
            verified_funder_candidates_count: verified_funder_addresses.len(),
            checked_at: Utc::now(),
        })
    }

    async fn live_account_reconcile(
        &self,
        request: AccountReconcileRequest,
    ) -> Result<AccountReconcileReport> {
        self.run_account_reconcile(request).await
    }

    async fn set_live_entries_enabled(
        &self,
        enabled: bool,
        reason: Option<String>,
    ) -> Result<LiveVenueStatus> {
        if !enabled {
            if self.bound_process_id.is_none() {
                let reason = bounded_live_gate_reason(reason.as_deref(), "manual_live_halt");
                // Close first. A submit that has not passed the global check will observe the
                // halt, while one already admitted remains serialized behind the barrier below.
                // Cancellation of this wait cannot reopen the gate.
                {
                    let mut global = self.global_entry_gate.lock().await;
                    record_global_entry_halt(&mut global, &reason);
                }
                let mut state = self.readiness_state.lock().await;
                state.manual_entries_enabled = false;
                state.manual_entries_reason = Some(reason);
            } else {
                let reason = bounded_live_gate_reason(reason.as_deref(), "process_manual_disable");
                let mut state = self.readiness_state.lock().await;
                state.manual_entries_enabled = false;
                state.manual_entries_reason = Some(reason);
            }
            let _submit_barrier = self.submit_guard.lock().await;
            return self.live_status().await;
        }

        if self.bound_process_id.is_none() {
            bail!("wallet-wide live entry enable is forbidden; use a checked process-bound enable");
        }
        {
            let mut state = self.readiness_state.lock().await;
            state.manual_entries_enabled = false;
            state.manual_entries_reason = Some("checked_enable_revalidation".to_string());
            state.reconciled_safety_generation = None;
        }
        // Revalidate this sleeve behind the shared submission barrier without revoking entry
        // grants held by other healthy sleeves. Account failures still advance the shared safety
        // generation and invalidate this commit.
        let expected_safety_generation = self.global_entry_gate.lock().await.safety_generation;
        if expected_safety_generation == u64::MAX {
            bail!("live safety generation is exhausted; checked enable remains fail-closed");
        }
        let _submit_guard = self.submit_guard.lock().await;
        let reconciliation = self.reconcile().await?;
        if reconciliation.mismatches_found != 0 || reconciliation.unresolved_count != 0 {
            bail!("checked process-bound live enable requires a clean reconciliation");
        }

        let now = Utc::now();
        let mut state = self.readiness_state.lock().await;
        let mut global = self.global_entry_gate.lock().await;
        let rest_fresh = stateful_age_seconds(state.last_rest_reconcile_at, now)
            .is_some_and(|age| age <= self.config.stale_reconcile.as_secs() as i64);
        let configured_identity = canonical_configured_account_identity(
            &self.config,
            self.bound_account_ref()
                .context("checked live enable requires a process account_ref")?,
        )?;
        let identity_matches = state.credential_account_fingerprint_sha256.as_deref()
            == Some(configured_identity.fingerprint_sha256.as_str());
        if !self.order_submission_enabled()
            || !self.config.submit_auth_available()
            || !rest_fresh
            || !state.process_accounting_entry_safe
            || !identity_matches
            || !state.idempotency_clean
            || state.unresolved_live_order_count != 0
            || global.safety_generation != expected_safety_generation
            || state.reconciled_safety_generation != Some(expected_safety_generation)
        {
            bail!(
                "checked process-bound live enable requires fresh reconciliation, proven accounting, matching identity, and clean idempotency"
            );
        }
        if !commit_checked_live_enable(&mut global, &mut state, expected_safety_generation) {
            bail!("live safety generation changed before checked enable commit");
        }
        drop(global);
        drop(state);
        self.live_status().await
    }
}
