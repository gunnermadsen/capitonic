use super::*;

impl BtcProcessManager {
    pub(super) fn is_managed_process(process: &TradingProcess) -> bool {
        Self::is_managed_identity(&process.process_type, &process.process_scope)
    }

    pub(super) fn is_managed_identity(process_type: &str, process_scope: &str) -> bool {
        process_type == BTC_PROCESS_TYPE && process_scope == BTC_PROCESS_SCOPE
    }

    pub(super) async fn record_event(
        &self,
        process_id: uuid::Uuid,
        level: &str,
        event_type: &str,
        message: &str,
        metadata: serde_json::Value,
    ) {
        if let Err(event_error) = self
            .store
            .record_trading_process_event(process_id, level, event_type, Some(message), metadata)
            .await
        {
            warn!(
                error = %event_error,
                process_id = %process_id,
                event_type,
                "failed to persist trading process lifecycle event"
            );
        }
    }

    pub(super) fn validate_start_definition(
        &self,
        process: &TradingProcess,
    ) -> Result<ResolvedBtcProcessDefinition, HttpError> {
        self.validate_definition(process, BtcDefinitionUse::ExplicitStart)
    }

    pub(super) fn validate_inactive_definition(
        &self,
        process: &TradingProcess,
    ) -> Result<ResolvedBtcProcessDefinition, HttpError> {
        self.validate_definition(process, BtcDefinitionUse::InactiveDefinition)
    }

    pub(super) fn validate_resume_definition(
        &self,
        process: &TradingProcess,
    ) -> Result<ResolvedBtcProcessDefinition, HttpError> {
        self.validate_definition(process, BtcDefinitionUse::DurableResume)
    }

    pub(super) fn validate_definition(
        &self,
        process: &TradingProcess,
        definition_use: BtcDefinitionUse,
    ) -> Result<ResolvedBtcProcessDefinition, HttpError> {
        match definition_use {
            BtcDefinitionUse::ExplicitStart => validate_btc_start_eligibility(process)?,
            BtcDefinitionUse::DurableResume => {
                validate_btc_process_capability(process)?;
                validate_btc_execution_activation(process)?;
            }
            BtcDefinitionUse::InactiveDefinition => validate_btc_process_capability(process)?,
        }
        if process
            .config
            .execution
            .as_ref()
            .and_then(|execution| execution.taker_fee_rate)
            .is_some()
        {
            return Err(HttpError::bad_request(
                "BTC realtime execution config accepts only execution safety fields and raw.btc_realtime_paper settings",
            ));
        }
        let raw = process.config.raw.as_object().ok_or_else(|| {
            HttpError::bad_request("BTC process config.raw must be a JSON object")
        })?;
        if raw.len() != 1 || !raw.contains_key("btc_realtime_paper") {
            return Err(HttpError::bad_request(
                "BTC process config.raw may contain only btc_realtime_paper",
            ));
        }
        let control_value = process
            .config
            .raw
            .get("btc_realtime_paper")
            .cloned()
            .ok_or_else(|| {
                HttpError::bad_request(
                    "process config.raw.btc_realtime_paper is required before start",
                )
            })?;
        let mut control = parse_btc_process_control(control_value, definition_use)?;
        control.next_experiment_key = control.next_experiment_key.trim().to_string();
        control.preregistration_sha256 = control.preregistration_sha256.trim().to_ascii_lowercase();
        if control.next_experiment_key.is_empty() {
            return Err(HttpError::bad_request(
                "next_experiment_key must not be empty",
            ));
        }
        if control.next_experiment_key.len() > 128
            || !control
                .next_experiment_key
                .bytes()
                .enumerate()
                .all(|(index, byte)| {
                    byte.is_ascii_lowercase()
                        || byte.is_ascii_digit()
                        || (index > 0 && matches!(byte, b'.' | b'_' | b'-'))
                })
        {
            return Err(HttpError::bad_request(
                "next_experiment_key must be a lowercase slug of at most 128 characters",
            ));
        }
        if control.preregistration_sha256.len() != 64
            || !control
                .preregistration_sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(HttpError::bad_request(
                "preregistration_sha256 must be a 64-character hexadecimal digest",
            ));
        }
        let members = resolve_btc_members(&control)?;
        for (_, member) in &members {
            validate_directional_model_entry_policy(
                member,
                control.paper.directional_model_entry_policy,
            )?;
            validate_btc_entry_timing(member)?;
            if process.effective_execution().mode == "live"
                && definition_use != BtcDefinitionUse::InactiveDefinition
            {
                validate_btc_live_model_authorization(member)?;
                validate_btc_live_execution_freshness(member)?;
            }
        }
        let strategy = members
            .into_iter()
            .next()
            .expect("validated nonempty router")
            .1;
        if process.effective_execution().mode == "live"
            && definition_use != BtcDefinitionUse::InactiveDefinition
        {
            validate_btc_live_model_authorization(&strategy)?;
        }
        validate_directional_model_entry_policy(
            &strategy,
            control.paper.directional_model_entry_policy,
        )?;
        if let Some(entry_admission) = control.entry_admission.as_ref() {
            entry_admission
                .validate()
                .map_err(|error| HttpError::bad_request(error.to_string()))?;
        }
        risk_runtime::validate_selections(&control.risk_strategies)
            .map_err(|error| HttpError::bad_request(error.to_string()))?;
        validate_btc_entry_timing(&strategy)?;
        if !(1..=60_000).contains(&control.runtime.strategy_interval_ms) {
            return Err(HttpError::bad_request(
                "BTC runtime timings exceed the supported process safety bounds",
            ));
        }
        let runtime = BtcRuntimeConfig {
            enabled: true,
            strategy_interval: Duration::from_millis(control.runtime.strategy_interval_ms),
            max_book_age: Duration::from_millis(strategy.max_book_age_ms as u64),
            max_reference_age: Duration::from_millis(strategy.max_reference_age_ms as u64),
            ..BtcRuntimeConfig::default()
        };
        runtime
            .validate()
            .map_err(|error| HttpError::bad_request(error.to_string()))?;
        if !(1..=60_000).contains(&control.paper.arrival_latency_ms)
            || control.paper.starting_collateral_usd <= rust_decimal::Decimal::ZERO
            || control.paper.starting_collateral_usd > dec!(1000000)
        {
            return Err(HttpError::bad_request(
                "BTC paper arrival latency and starting collateral must be positive",
            ));
        }
        let paper_venue = PaperVenueConfig {
            arrival_latency: Duration::from_millis(control.paper.arrival_latency_ms),
            visible_depth_haircut: control.paper.visible_depth_haircut,
            max_book_age: chrono::Duration::milliseconds(strategy.max_book_age_ms),
            starting_collateral_usd: control.paper.starting_collateral_usd,
        };
        paper_venue
            .validate()
            .map_err(|error| HttpError::bad_request(error.to_string()))?;
        if let Some(high_water_mark) = control
            .entry_admission
            .as_ref()
            .and_then(|admission| admission.daily_realized_pnl_high_water_mark.as_ref())
        {
            high_water_mark
                .validate_against_starting_collateral(control.paper.starting_collateral_usd)
                .map_err(|error| HttpError::bad_request(error.to_string()))?;
        }
        let mut preview_keys = HashSet::new();
        let mut paper_stress_previews = Vec::with_capacity(control.paper.stress_previews.len());
        for preview in &control.paper.stress_previews {
            if !(1..=60_000).contains(&preview.arrival_latency_ms) {
                return Err(HttpError::bad_request(
                    "BTC paper stress-preview latency must be positive",
                ));
            }
            let resolved = PaperPreviewConfig {
                scenario_key: preview.scenario_key.trim().to_string(),
                arrival_latency: Duration::from_millis(preview.arrival_latency_ms),
                visible_depth_haircut: preview.visible_depth_haircut,
            };
            resolved
                .validate()
                .map_err(|error| HttpError::bad_request(error.to_string()))?;
            if !preview_keys.insert(resolved.scenario_key.clone()) {
                return Err(HttpError::bad_request(format!(
                    "duplicate BTC paper stress-preview scenario {}",
                    resolved.scenario_key
                )));
            }
            paper_stress_previews.push(resolved);
        }
        Ok(ResolvedBtcProcessDefinition {
            entry_admission: control.entry_admission.clone(),
            risk_strategies: control.risk_strategies.clone(),
            control,
            strategy,
            runtime,
            paper_venue,
            paper_stress_previews,
        })
    }

    pub(super) fn prepare_start_definition(
        &self,
        process: &TradingProcess,
    ) -> Result<PreparedBtcStartDefinition, HttpError> {
        prepare_btc_start_definition_for_execution(
            self.validate_start_definition(process)?,
            &process.effective_execution(),
        )
    }

    pub(super) fn prepare_resume_definition(
        &self,
        process: &TradingProcess,
    ) -> Result<PreparedBtcStartDefinition, HttpError> {
        prepare_btc_start_definition_for_execution(
            self.validate_resume_definition(process)?,
            &process.effective_execution(),
        )
    }

    pub(super) async fn live_preflight(
        &self,
        process_id: uuid::Uuid,
    ) -> Result<TradingProcessLivePreflightResponse, HttpError> {
        let process = self
            .store
            .get_trading_process(process_id)
            .await
            .map_err(|error| HttpError::internal(error.to_string()))?
            .ok_or_else(|| HttpError::not_found("trading process not found"))?;
        if process.enabled || matches!(process.status.as_str(), "starting" | "running" | "stopping")
        {
            return Err(HttpError::conflict(
                "credential preflight requires an inactive trading process",
            ));
        }
        self.validate_inactive_definition(&process)?;
        let execution = process.effective_execution();
        if execution.mode != "live" {
            return Err(HttpError::bad_request(
                "process-scoped live preflight requires execution.mode=live",
            ));
        }
        let account_ref = execution
            .account_ref
            .as_deref()
            .ok_or_else(|| HttpError::internal("validated live process is missing account_ref"))?
            .to_string();
        let global = self.config.live_venue.as_ref().cloned().ok_or_else(|| {
            HttpError::bad_request("live execution credentials are not configured")
        })?;
        let venue = Arc::new(
            global
                .bind_process(process_id, &execution)
                .map_err(|error| HttpError::bad_request(error.to_string()))?,
        );
        let identity = venue
            .live_identity_diagnostics()
            .await
            .map_err(|error| HttpError::bad_request(error.to_string()))?;
        let reconciliation_result = venue.reconcile().await;
        let (reconciliation, reconciliation_error) = match reconciliation_result {
            Ok(report) => (Some(report), None),
            Err(error) => {
                let mut message = error.to_string();
                message.truncate(512);
                (None, Some(message))
            }
        };
        let status = venue
            .live_status()
            .await
            .map_err(|error| HttpError::internal(error.to_string()))?;
        let credential_connectivity_ready = identity.credentials_present
            && identity.account_identity_valid
            && identity.account_identity_fingerprint_sha256.is_some()
            && identity.api_keys_readable
            && identity.balance_allowance_readable
            && identity.balance_allowance_error.is_none()
            && identity
                .collateral_balance
                .as_deref()
                .and_then(|balance| balance.parse::<Decimal>().ok())
                .is_some_and(|balance| balance > Decimal::ZERO)
            && identity.open_orders_readable
            && identity.signer_address.is_some()
            && identity.configured_funder_address.is_some()
            && identity.resolved_signature_type.is_some()
            && identity.authenticated_client_address.is_some();
        let reconciliation_ready = reconciliation.as_ref().is_some_and(|report| {
            report.balances_checked
                && report.mismatches_found == 0
                && report.unresolved_count == 0
                && status.process_accounting_proven
        });
        let trading_disabled = !execution.execute_signals
            && !execution.live_capital
            && !status.order_submit_enabled
            && !status.entries_enabled;
        let mut reasons = Vec::with_capacity(3);
        if !credential_connectivity_ready {
            reasons.push("credential_connectivity_failed".to_string());
        }
        if !reconciliation_ready {
            reasons.push("process_reconciliation_not_clean".to_string());
        }
        if !trading_disabled {
            reasons.push("credential_preflight_requires_trading_disabled".to_string());
        }
        let ready = reasons.is_empty();
        let checked_at = Utc::now();
        self.record_event(
            process_id,
            if ready { "info" } else { "warn" },
            "btc_live_preflight_completed",
            "BTC live credential and reconciliation preflight completed without order submission",
            serde_json::json!({
                "account_ref": &account_ref,
                "ready": ready,
                "credential_connectivity_ready": credential_connectivity_ready,
                "reconciliation_ready": reconciliation_ready,
                "trading_disabled": trading_disabled,
                "reasons": &reasons,
                "checked_at": checked_at,
            }),
        )
        .await;
        Ok(TradingProcessLivePreflightResponse {
            process_id,
            account_ref,
            credential_connectivity_ready,
            reconciliation_ready,
            trading_disabled,
            ready,
            reasons,
            identity,
            status,
            reconciliation,
            reconciliation_error,
            checked_at,
        })
    }

    pub(super) async fn set_live_entries_enabled(
        &self,
        process_id: uuid::Uuid,
        enabled: bool,
    ) -> Result<LiveVenueStatus, HttpError> {
        let _transition_guard = self.transition.lock().await;
        self.set_live_entries_enabled_locked(process_id, enabled)
            .await
    }

    pub(super) async fn set_live_entries_enabled_locked(
        &self,
        process_id: uuid::Uuid,
        enabled: bool,
    ) -> Result<LiveVenueStatus, HttpError> {
        let (venue, active_run_id, active_run_key, active_config_hash) = {
            let active = self.active_playbooks.lock().await;
            let active = active
                .get(&process_id)
                .filter(|active| active.execution_mode == BtcExecutionMode::Live)
                .ok_or_else(|| HttpError::conflict("active live process runtime not found"))?;
            let venue = active
                .live_venue
                .clone()
                .ok_or_else(|| HttpError::internal("active live runtime omitted its venue"))?;
            (
                venue,
                active.run_id,
                active.run_key.clone(),
                active.config_hash.clone(),
            )
        };
        if enabled {
            let process = self
                .store
                .get_trading_process(process_id)
                .await
                .map_err(|error| HttpError::internal(error.to_string()))?
                .ok_or_else(|| HttpError::not_found("trading process not found"))?;
            if !process.enabled || process.status != "running" {
                return Err(HttpError::conflict(
                    "live entries require the exact active process to be enabled and running",
                ));
            }
            let prepared = self.prepare_resume_definition(&process)?;
            if prepared.run_id != active_run_id || prepared.run_key != active_run_key {
                return Err(HttpError::conflict(
                    "live process definition no longer matches the active run identity",
                ));
            }
            let manifest = self
                .repository
                .load_run_manifest(process_id, active_run_id, &active_run_key)
                .await
                .map_err(|error| HttpError::internal(error.to_string()))?
                .ok_or_else(|| HttpError::conflict("active live run manifest is missing"))?;
            if manifest.config_hash != active_config_hash
                || resume_process_contract_projection(
                    serde_json::to_value(&prepared.frozen_process_config)
                        .map_err(|error| HttpError::internal(error.to_string()))?,
                ) != resume_process_contract_projection(manifest.frozen_process_config)
            {
                return Err(HttpError::conflict(
                    "live process definition or manifest drifted from the active runtime",
                ));
            }
            let active_still_matches = self
                .active_playbooks
                .lock()
                .await
                .get(&process_id)
                .is_some_and(|active| {
                    active.execution_mode == BtcExecutionMode::Live
                        && active.run_id == active_run_id
                        && active.config_hash == active_config_hash
                        && active.live_venue.is_some()
                });
            if !active_still_matches {
                return Err(HttpError::conflict(
                    "active live runtime changed during entry-enable validation",
                ));
            }
        }
        // The venue owns the authoritative close/drain/reconcile/generation-CAS sequence.
        // Bound the whole operation instead of performing an earlier, non-authoritative
        // reconciliation that could go stale before the gate is reopened.
        let status = tokio::time::timeout(
            BTC_LIVE_CONTROL_TIMEOUT,
            venue.set_live_entries_enabled(
                enabled,
                (!enabled).then(|| "process_manual_disable".to_string()),
            ),
        )
        .await
        .map_err(|_| {
            HttpError::conflict(format!(
                "live entry transition timed out after {}s; entries remain fail-closed",
                BTC_LIVE_CONTROL_TIMEOUT.as_secs()
            ))
        })?
        .map_err(|error| HttpError::conflict(error.to_string()))?;
        if enabled && !status.entries_enabled {
            let _ = venue
                .set_live_entries_enabled(
                    false,
                    Some("entry_enable_preconditions_failed".to_string()),
                )
                .await;
            return Err(HttpError::conflict(format!(
                "live entry enable preconditions failed: {}",
                status.reason.as_deref().unwrap_or("unknown")
            )));
        }
        self.record_event(
            process_id,
            "warn",
            if enabled {
                "btc_live_entries_enabled"
            } else {
                "btc_live_entries_disabled"
            },
            if enabled {
                "BTC live entries manually enabled after strict reconciliation"
            } else {
                "BTC live entries manually disabled"
            },
            serde_json::json!({
                "entries_enabled": status.entries_enabled,
                "reason": &status.reason,
                "source": "manual_process_control",
            }),
        )
        .await;
        Ok(status)
    }

    pub(super) async fn ensure_start_slot_available(
        &self,
        process_id: uuid::Uuid,
    ) -> Result<(), HttpError> {
        if let Some(pending) = self.terminal_pending.lock().await.get(&process_id) {
            return Err(HttpError::conflict(format!(
                "BTC run {} still has a pending terminal transition",
                pending.run_key
            )));
        }
        if let Some(active) = self.active_playbooks.lock().await.get(&process_id) {
            return Err(HttpError::conflict(format!(
                "BTC process {} is already running execution run {}",
                active.process_id, active.run_key
            )));
        }
        Ok(())
    }

    pub(super) async fn quiesce_live_venue(
        venue: Option<&Arc<LiveVenue>>,
        reason: &str,
    ) -> Option<String> {
        let Some(venue) = venue else {
            return None;
        };
        let quiesce = async {
            let mut failures = Vec::with_capacity(3);
            if let Err(error) = venue
                .set_live_entries_enabled(false, Some(reason.to_string()))
                .await
            {
                failures.push(format!("live_entry_disable_failed: {error:#}"));
            }
            if let Err(error) = venue.cancel_all().await {
                failures.push(format!("live_order_cancel_failed: {error:#}"));
            }
            match venue.reconcile().await {
                Ok(report) if report.open_orders == 0 && report.unresolved_count == 0 => {}
                Ok(report) => warn!(
                    open_orders = report.open_orders,
                    unresolved_count = report.unresolved_count,
                    "live stop reconciliation remains unresolved without changing terminal intent"
                ),
                Err(error) => warn!(
                    error = %error,
                    "live stop reconciliation failed without changing terminal intent"
                ),
            }
            failures
        };
        match tokio::time::timeout(BTC_LIVE_QUIESCE_TIMEOUT, quiesce).await {
            Ok(failures) => (!failures.is_empty()).then(|| failures.join("; ")),
            Err(_) => Some(format!(
                "live_quiesce_timeout_after_{}s",
                BTC_LIVE_QUIESCE_TIMEOUT.as_secs()
            )),
        }
    }

    pub(super) async fn preview_start(
        &self,
        process_id: uuid::Uuid,
    ) -> Result<TradingProcessStartPreviewResponse, HttpError> {
        if self.shutting_down.load(Ordering::Acquire) {
            return Err(HttpError::conflict(
                "BTC runtime cannot start while the service is shutting down",
            ));
        }
        let _transition_guard = self.transition.lock().await;
        if self.shutting_down.load(Ordering::Acquire) {
            return Err(HttpError::conflict(
                "BTC runtime cannot start while the service is shutting down",
            ));
        }
        self.ensure_start_slot_available(process_id).await?;
        let process = self
            .store
            .get_trading_process(process_id)
            .await
            .map_err(|error| HttpError::internal(error.to_string()))?
            .ok_or_else(|| HttpError::not_found("trading process not found"))?;
        let prepared = self.prepare_start_definition(&process)?;
        preflight_btc_run_identity(&self.repository, &prepared.run_key, prepared.run_id)
            .await
            .map_err(|error| HttpError::conflict(error.to_string()))?;
        Ok(TradingProcessStartPreviewResponse {
            process_id,
            run_id: prepared.run_id,
            run_key: prepared.run_key,
            preregistration_sha256: prepared.preregistration_sha256,
            config_hash: prepared.config_hash,
            frozen_process_config: prepared.frozen_process_config,
        })
    }

    pub(super) async fn start_process(
        &self,
        process_id: uuid::Uuid,
    ) -> Result<TradingProcess, HttpError> {
        if self.shutting_down.load(Ordering::Acquire) {
            return Err(HttpError::conflict(
                "BTC runtime cannot start while the service is shutting down",
            ));
        }
        let transition_guard = self.transition.clone().lock_owned().await;
        if self.shutting_down.load(Ordering::Acquire) {
            return Err(HttpError::conflict(
                "BTC runtime cannot start while the service is shutting down",
            ));
        }
        let manager = self.clone();
        tokio::spawn(async move {
            let _transition_guard = transition_guard;
            manager.start_process_locked(process_id).await
        })
        .await
        .map_err(|error| {
            HttpError::internal(format!("BTC start transition task failed: {error}"))
        })?
    }

    pub(super) async fn resume_durable_processes(&self) -> Result<Vec<TradingProcess>, HttpError> {
        if self.shutting_down.load(Ordering::Acquire) {
            return Err(HttpError::conflict(
                "BTC runtime cannot resume while the service is shutting down",
            ));
        }
        let _transition_guard = self.transition.lock().await;
        let candidates = sqlx::query_scalar::<_, uuid::Uuid>(
            r#"
            SELECT p.process_id
            FROM polymarket.trading_processes p
            JOIN LATERAL (
              SELECT event.timestamp_utc
              FROM polymarket.trading_process_events event
              WHERE event.process_id = p.process_id
                AND event.event_type = 'btc_run_manifest'
                AND event.metadata #>> '{run_key}' =
                    p.config #>> '{raw,btc_realtime_paper,next_experiment_key}'
              ORDER BY event.timestamp_utc DESC, event.created_at DESC
              LIMIT 1
            ) run ON true
            WHERE p.process_type = 'btc_5m'
              AND p.process_scope = 'realtime_paper'
              AND p.config #>> '{raw,btc_realtime_paper,schema_version}' IN ($1, $2, $3, $4)
              AND p.enabled
              AND p.status IN ('starting','running','stopping')
              AND p.stopped_at IS NULL
            ORDER BY run.timestamp_utc DESC
            "#,
        )
        .bind(ROUTER_PROCESS_SCHEMA_VERSION)
        .bind(SELECTABLE_BTC_PROCESS_SCHEMA_VERSION)
        .bind(BTC_PROCESS_SCHEMA_VERSION)
        .bind(LEGACY_BTC_PROCESS_SCHEMA_VERSION)
        .fetch_all(&self.pool)
        .await
        .map_err(|error| HttpError::internal(error.to_string()))?;
        if candidates.is_empty() {
            let durable_claims = sqlx::query_scalar::<_, i64>(
                r#"
                SELECT COUNT(*)
                FROM polymarket.trading_processes p
                WHERE p.process_type = 'btc_5m'
                  AND p.process_scope = 'realtime_paper'
                  AND p.enabled
                  AND p.status IN ('starting','running','stopping')
                  AND p.stopped_at IS NULL
                "#,
            )
            .fetch_one(&self.pool)
            .await
            .map_err(|error| HttpError::internal(error.to_string()))?;
            if durable_claims > 0 {
                return Err(HttpError::conflict(
                    "durable BTC realtime execution state exists but cannot be reattached exactly; database state was left unchanged",
                ));
            }
            return Ok(Vec::new());
        }

        let mut resumed = Vec::with_capacity(candidates.len());
        for process_id in candidates {
            self.ensure_start_slot_available(process_id).await?;
            resumed.push(self.resume_durable_process_locked(process_id).await?);
        }
        Ok(resumed)
    }

    pub(super) async fn resume_durable_process_locked(
        &self,
        process_id: uuid::Uuid,
    ) -> Result<TradingProcess, HttpError> {
        let process = self
            .store
            .get_trading_process(process_id)
            .await
            .map_err(|error| HttpError::internal(error.to_string()))?
            .ok_or_else(|| HttpError::not_found("trading process not found"))?;
        let prepared = self.prepare_resume_definition(&process)?;
        let manifest = self
            .repository
            .load_run_manifest(process_id, prepared.run_id, &prepared.run_key)
            .await
            .map_err(|error| HttpError::internal(error.to_string()))?
            .ok_or_else(|| {
                HttpError::conflict("durable BTC run disappeared before runtime reattachment")
            })?;
        let config_hash = manifest.config_hash;
        let frozen_process_config_value = manifest.frozen_process_config;
        let current_frozen_process_config =
            serde_json::to_value(&prepared.frozen_process_config)
                .map_err(|error| HttpError::internal(error.to_string()))?;
        if resume_process_contract_projection(current_frozen_process_config)
            != resume_process_contract_projection(frozen_process_config_value.clone())
        {
            return Err(HttpError::conflict(
                "durable BTC run parameters changed and cannot be resumed by this process definition",
            ));
        }
        let PreparedBtcStartDefinition {
            run_id,
            run_key,
            preregistration_sha256,
            strategy,
            sources,
            entry_admission,
            risk_strategies,
            directional_model_entry_policy,
            runtime: runtime_config,
            paper_venue: paper_venue_config,
            paper_stress_previews,
            execution_mode,
            execution,
            frozen_process_config: _,
            config_hash: current_config_hash,
        } = prepared;
        self.record_event(
            process_id,
            "info",
            "btc_runtime_resuming",
            "BTC realtime execution runtime resume accepted",
            serde_json::json!({
                "run_id": run_id,
                "run_key": &run_key,
                "preregistration_sha256": &preregistration_sha256,
                "config_hash": &config_hash,
                "current_definition_config_hash": &current_config_hash,
                "compiled_source_identity": COMPILED_SOURCE_IDENTITY,
            }),
        )
        .await;
        let active_strategy = strategy.clone();

        let startup_result: Result<(BtcPlaybookRuntimeHandle, Option<Arc<LiveVenue>>)> = async {
            let (state, books) = self
                .ensure_shared_runtime(&runtime_config, &sources)
                .await
                .map_err(|error| anyhow::anyhow!("{error:?}"))?;
            let components = self.execution_components(
                execution_mode,
                &execution,
                process_id,
                books.clone(),
                paper_venue_config,
                &strategy,
            )?;
            let live_process_venue = components.live_venue.clone();
            if should_resume_configured_live_entries(&process, &execution) {
                live_process_venue
                    .as_ref()
                    .context("configured live resume omitted its process venue")?
                    .restore_configured_entries_after_restart()
                    .await
                    .context("failed to restore configured live authorization before resume")?;
            }
            let process_runner = Arc::new(
                BtcProcessRunner::new_with_execution(
                    self.repository.clone(),
                    self.store.clone(),
                    components.venue,
                    books.clone(),
                    components.lifecycle,
                    BtcProcessConfig {
                        run_id,
                        run_key: run_key.clone(),
                        process_id,
                        config_hash: config_hash.clone(),
                        frozen_process_config: frozen_process_config_value,
                        strategy,
                        entry_admission,
                        risk_strategies,
                        directional_model_entry_policy,
                        execution_enabled: true,
                        paper_stress_previews,
                    },
                )?
                .with_primary_persistence_state(state.clone()),
            );
            process_runner
                .resume()
                .await
                .context("failed to reattach immutable BTC run before feed resume")?;
            Ok((
                BtcPlaybookRuntimeHandle::start(runtime_config, process_runner, state, books)?,
                live_process_venue,
            ))
        }
        .await;
        let (runtime, live_process_venue) = match startup_result {
            Ok(runtime) => runtime,
            Err(resume_error) => {
                return Err(HttpError::internal(format!(
                    "BTC durable runtime resume failed without mutating lifecycle state: {resume_error:#}"
                )));
            }
        };
        self.active_playbooks.lock().await.insert(
            process_id,
            ActiveBtcPlaybook {
                process_id,
                run_id,
                run_key: run_key.clone(),
                config_hash: config_hash.clone(),
                execution_mode,
                strategy: active_strategy,
                sources,
                live_venue: live_process_venue,
                runtime,
            },
        );
        self.refresh_shared_sources().await?;
        self.record_event(
            process_id,
            "info",
            "btc_runtime_resumed",
            "BTC realtime execution runtime resumed after service restart",
            serde_json::json!({
                "run_id": run_id,
                "run_key": &run_key,
                "config_hash": &config_hash,
                "current_definition_config_hash": &current_config_hash,
                "compiled_source_identity": COMPILED_SOURCE_IDENTITY,
            }),
        )
        .await;
        info!(
            process_id = %process_id,
            run_id = %run_id,
            run_key = %run_key,
            config_hash = %config_hash,
            "durable BTC realtime execution runtime resumed"
        );
        Ok(process)
    }

    pub(super) async fn start_process_locked(
        &self,
        process_id: uuid::Uuid,
    ) -> Result<TradingProcess, HttpError> {
        self.ensure_start_slot_available(process_id).await?;
        let process = self
            .store
            .get_trading_process(process_id)
            .await
            .map_err(|error| HttpError::internal(error.to_string()))?
            .ok_or_else(|| HttpError::not_found("trading process not found"))?;
        let PreparedBtcStartDefinition {
            run_id,
            run_key,
            preregistration_sha256,
            strategy,
            sources,
            entry_admission,
            risk_strategies,
            directional_model_entry_policy,
            runtime: runtime_config,
            paper_venue: paper_venue_config,
            paper_stress_previews,
            execution_mode,
            execution,
            frozen_process_config,
            config_hash,
        } = self.prepare_start_definition(&process)?;
        preflight_btc_run_identity(&self.repository, &run_key, run_id)
            .await
            .map_err(|error| HttpError::conflict(error.to_string()))?;
        let frozen_process_config_value = serde_json::to_value(&frozen_process_config)
            .map_err(|error| HttpError::internal(error.to_string()))?;

        let start_pending = PendingBtcTerminal {
            process_id,
            run_id,
            run_key: run_key.clone(),
            config_hash: config_hash.clone(),
            terminal_status: "failed".to_string(),
            terminal_reason: "btc_start_transition_interrupted".to_string(),
            allow_inactive_process: true,
        };
        self.terminal_pending
            .lock()
            .await
            .insert(process_id, start_pending.clone());
        match self
            .store
            .update_trading_process_status(process_id, "starting", true, None)
            .await
        {
            Ok(Some(_)) => {}
            Ok(None) => {
                let reason = "BTC process disappeared while entering starting state".to_string();
                return Err(self
                    .terminalize_start_failure_locked(start_pending, reason)
                    .await);
            }
            Err(error) => {
                let reason = format!("failed to mark BTC process starting: {error:#}");
                return Err(self
                    .terminalize_start_failure_locked(start_pending, reason)
                    .await);
            }
        }
        self.record_event(
            process_id,
            "info",
            "btc_runtime_starting",
            "BTC realtime execution runtime start accepted",
            serde_json::json!({
                "run_id": run_id,
                "run_key": &run_key,
                "preregistration_sha256": &preregistration_sha256,
            }),
        )
        .await;

        let startup_result: Result<(BtcPlaybookRuntimeHandle, Option<Arc<LiveVenue>>)> = async {
            let (state, books) = self
                .ensure_shared_runtime(&runtime_config, &sources)
                .await
                .map_err(|error| anyhow::anyhow!("{error:?}"))?;
            let components = self.execution_components(
                execution_mode,
                &execution,
                process_id,
                books.clone(),
                paper_venue_config,
                &strategy,
            )?;
            let live_process_venue = components.live_venue.clone();
            let process_runner = Arc::new(
                BtcProcessRunner::new_with_execution(
                    self.repository.clone(),
                    self.store.clone(),
                    components.venue,
                    books.clone(),
                    components.lifecycle,
                    BtcProcessConfig {
                        run_id,
                        run_key: run_key.clone(),
                        process_id,
                        config_hash: config_hash.clone(),
                        frozen_process_config: frozen_process_config_value,
                        strategy: strategy.clone(),
                        entry_admission: entry_admission.clone(),
                        risk_strategies: risk_strategies.clone(),
                        directional_model_entry_policy,
                        execution_enabled: true,
                        paper_stress_previews: paper_stress_previews.clone(),
                    },
                )?
                .with_primary_persistence_state(state.clone()),
            );
            process_runner
                .initialize()
                .await
                .context("failed to initialize immutable BTC run before feed startup")?;
            Ok((
                BtcPlaybookRuntimeHandle::start(runtime_config, process_runner, state, books)?,
                live_process_venue,
            ))
        }
        .await;

        let (runtime, live_process_venue) = match startup_result {
            Ok(runtime) => runtime,
            Err(startup_error) => {
                let reason = format!("btc_startup_failed: {startup_error:#}");
                return Err(self
                    .terminalize_start_failure_locked(start_pending, reason)
                    .await);
            }
        };

        let running_process = match self
            .store
            .update_trading_process_status(process_id, "running", true, None)
            .await
        {
            Ok(Some(process)) => process,
            Ok(None) => {
                let reason = "BTC process disappeared while runtime was starting".to_string();
                let _ =
                    tokio::time::timeout(BTC_RUNTIME_SHUTDOWN_TIMEOUT, runtime.shutdown()).await;
                return Err(self
                    .terminalize_start_failure_locked(start_pending, reason)
                    .await);
            }
            Err(update_error) => {
                let reason = format!("failed to mark BTC process running: {update_error:#}");
                let _ =
                    tokio::time::timeout(BTC_RUNTIME_SHUTDOWN_TIMEOUT, runtime.shutdown()).await;
                return Err(self
                    .terminalize_start_failure_locked(start_pending, reason)
                    .await);
            }
        };

        self.active_playbooks.lock().await.insert(
            process_id,
            ActiveBtcPlaybook {
                process_id,
                run_id,
                run_key: run_key.clone(),
                config_hash: config_hash.clone(),
                execution_mode,
                strategy,
                sources,
                live_venue: live_process_venue,
                runtime,
            },
        );
        self.refresh_shared_sources().await?;
        let mut pending_guard = self.terminal_pending.lock().await;
        if pending_guard
            .get(&process_id)
            .is_some_and(|pending| pending.run_id == run_id)
        {
            pending_guard.remove(&process_id);
        }
        drop(pending_guard);
        self.record_event(
            process_id,
            "info",
            "btc_runtime_started",
            "BTC realtime execution runtime is running",
            serde_json::json!({
                "run_id": run_id,
                "run_key": &run_key,
                "config_hash": &config_hash,
            }),
        )
        .await;
        info!(
            process_id = %process_id,
            run_id = %run_id,
            run_key = %run_key,
            config_hash = %config_hash,
            compiled_source_identity = COMPILED_SOURCE_IDENTITY,
            "API-owned BTC realtime execution runtime started"
        );
        Ok(running_process)
    }

    pub(super) async fn terminalize_start_failure_locked(
        &self,
        mut pending: PendingBtcTerminal,
        reason: String,
    ) -> HttpError {
        pending.terminal_status = "failed".to_string();
        pending.terminal_reason = reason.clone();
        self.terminal_pending
            .lock()
            .await
            .insert(pending.process_id, pending.clone());
        self.record_event(
            pending.process_id,
            "error",
            "btc_runtime_start_failed",
            &reason,
            serde_json::json!({
                "run_id": pending.run_id,
                "run_key": pending.run_key,
                "config_hash": pending.config_hash,
            }),
        )
        .await;
        let result = self.finalize_pending_locked(pending).await;
        self.shutdown_shared_runtime_if_idle().await;
        match result {
            Ok(_) => HttpError::internal(reason),
            Err(persistence_error) => persistence_error,
        }
    }

    pub(super) async fn stop_process(
        &self,
        process_id: uuid::Uuid,
        reason: &str,
    ) -> Result<TradingProcess, HttpError> {
        self.stop_process_for_generation(process_id, None, reason, false, "stopped")
            .await
    }

    pub(super) async fn complete_process(
        &self,
        process_id: uuid::Uuid,
        reason: &str,
    ) -> Result<TradingProcess, HttpError> {
        self.stop_process_for_generation(process_id, None, reason, false, "completed")
            .await
    }

    pub(super) async fn stop_process_for_generation(
        &self,
        process_id: uuid::Uuid,
        expected_run_id: Option<uuid::Uuid>,
        reason: &str,
        runtime_failed: bool,
        successful_terminal_status: &str,
    ) -> Result<TradingProcess, HttpError> {
        if !matches!(successful_terminal_status, "stopped" | "completed") {
            return Err(HttpError::internal(
                "unsupported successful BTC terminal status",
            ));
        }
        let transition_guard = self.transition.clone().lock_owned().await;
        let manager = self.clone();
        let reason = reason.to_string();
        let successful_terminal_status = successful_terminal_status.to_string();
        tokio::spawn(async move {
            let _transition_guard = transition_guard;
            manager
                .stop_process_locked(
                    process_id,
                    expected_run_id,
                    &reason,
                    runtime_failed,
                    &successful_terminal_status,
                )
                .await
        })
        .await
        .map_err(|error| HttpError::internal(format!("BTC stop transition task failed: {error}")))?
    }

    pub(super) async fn stop_process_locked(
        &self,
        process_id: uuid::Uuid,
        expected_run_id: Option<uuid::Uuid>,
        reason: &str,
        runtime_failed: bool,
        successful_terminal_status: &str,
    ) -> Result<TradingProcess, HttpError> {
        let process = self
            .store
            .get_trading_process(process_id)
            .await
            .map_err(|error| HttpError::internal(error.to_string()))?
            .ok_or_else(|| HttpError::not_found("trading process not found"))?;
        if !Self::is_managed_process(&process) {
            return Err(HttpError::bad_request(
                "process is not a managed BTC realtime execution process",
            ));
        }

        let pending = { self.terminal_pending.lock().await.get(&process_id).cloned() };
        if let Some(pending) = pending {
            if expected_run_id.is_some_and(|expected| expected != pending.run_id) {
                return Ok(process);
            }
            return self.finalize_pending_locked(pending).await;
        }
        let active_identity = self
            .active_playbooks
            .lock()
            .await
            .get(&process_id)
            .map(|active| (active.run_id, active.run_key.clone()));
        let Some((active_run_id, active_run_key)) = active_identity else {
            if process.enabled
                || matches!(process.status.as_str(), "starting" | "running" | "stopping")
            {
                return Err(HttpError::conflict(
                    "BTC process claims to be active but this service owns no runtime handle",
                ));
            }
            if successful_terminal_status == "completed" && process.status != "completed" {
                return Err(HttpError::conflict(
                    "only an active or already completed BTC process can be completed",
                ));
            }
            return Ok(process);
        };
        if expected_run_id.is_some_and(|expected| expected != active_run_id) {
            return Ok(process);
        }
        self.store
            .update_trading_process_status(process_id, "stopping", true, None)
            .await
            .map_err(|error| HttpError::internal(error.to_string()))?
            .ok_or_else(|| HttpError::not_found("trading process not found"))?;
        self.record_event(
            process_id,
            "info",
            "btc_runtime_stopping",
            "BTC realtime execution runtime stop accepted",
            serde_json::json!({
                "run_id": active_run_id,
                "run_key": active_run_key,
                "reason": reason,
            }),
        )
        .await;

        let active = self
            .active_playbooks
            .lock()
            .await
            .remove(&process_id)
            .expect("active BTC playbook exists while lifecycle transition is held");
        let remaining_sources = merge_source_selectors(
            self.active_playbooks
                .lock()
                .await
                .values()
                .flat_map(|playbook| playbook.sources.clone()),
        )?;
        if let Some(shared) = self.shared_runtime.lock().await.as_ref() {
            if let Some(runtime) = shared.runtime.as_ref() {
                if !remaining_sources.is_empty() {
                    runtime.update_sources(remaining_sources);
                }
            }
        }
        let provisional_pending = PendingBtcTerminal {
            process_id: active.process_id,
            run_id: active.run_id,
            run_key: active.run_key.clone(),
            config_hash: active.config_hash.clone(),
            terminal_status: "failed".to_string(),
            terminal_reason: format!("{reason}; stop_transition_interrupted"),
            allow_inactive_process: false,
        };
        self.terminal_pending
            .lock()
            .await
            .insert(process_id, provisional_pending);
        let live_quiesce_failure =
            Self::quiesce_live_venue(active.live_venue.as_ref(), "process_runtime_stopping").await;
        let shutdown_result =
            tokio::time::timeout(BTC_RUNTIME_SHUTDOWN_TIMEOUT, active.runtime.shutdown()).await;
        let shutdown_failure = match shutdown_result {
            Ok(Ok(())) => None,
            Ok(Err(shutdown_error)) => {
                Some(format!("btc_runtime_shutdown_failed: {shutdown_error:#}"))
            }
            Err(_) => Some(format!(
                "btc_runtime_shutdown_timeout_after_{}s",
                BTC_RUNTIME_SHUTDOWN_TIMEOUT.as_secs()
            )),
        };
        let terminal_status =
            if runtime_failed || live_quiesce_failure.is_some() || shutdown_failure.is_some() {
                "failed"
            } else {
                successful_terminal_status
            };
        let mut terminal_failures = Vec::with_capacity(2);
        if let Some(failure) = live_quiesce_failure {
            terminal_failures.push(failure);
        }
        if let Some(failure) = shutdown_failure {
            terminal_failures.push(failure);
        }
        let terminal_reason = if terminal_failures.is_empty() {
            reason.to_string()
        } else {
            format!("{reason}; {}", terminal_failures.join("; "))
        };
        let pending = PendingBtcTerminal {
            process_id: active.process_id,
            run_id: active.run_id,
            run_key: active.run_key,
            config_hash: active.config_hash,
            terminal_status: terminal_status.to_string(),
            terminal_reason,
            allow_inactive_process: false,
        };
        self.terminal_pending
            .lock()
            .await
            .insert(process_id, pending.clone());
        let result = self.finalize_pending_locked(pending).await;
        self.shutdown_shared_runtime_if_idle().await;
        result
    }

    pub(super) async fn finalize_pending_locked(
        &self,
        pending: PendingBtcTerminal,
    ) -> Result<TradingProcess, HttpError> {
        if let Err(terminal_error) = mark_btc_process_terminal(
            &self.pool,
            pending.process_id,
            &pending.terminal_status,
            &pending.terminal_reason,
            pending.allow_inactive_process,
        )
        .await
        {
            let persistence_reason = format!(
                "{}; terminal_persistence_pending: {terminal_error:#}",
                pending.terminal_reason
            );
            self.record_event(
                pending.process_id,
                "error",
                "btc_runtime_terminal_persistence_pending",
                &persistence_reason,
                serde_json::json!({
                    "run_id": pending.run_id,
                    "run_key": pending.run_key,
                    "config_hash": pending.config_hash,
                    "desired_terminal_status": pending.terminal_status,
                }),
            )
            .await;
            return Err(HttpError::internal(persistence_reason));
        }
        let mut pending_guard = self.terminal_pending.lock().await;
        if pending_guard
            .get(&pending.process_id)
            .is_some_and(|current| current.run_id == pending.run_id)
        {
            pending_guard.remove(&pending.process_id);
        }
        drop(pending_guard);
        let (level, event_type) = match pending.terminal_status.as_str() {
            "stopped" => ("info", "btc_runtime_stopped"),
            "completed" => ("info", "btc_runtime_completed"),
            _ => ("error", "btc_runtime_stop_failed"),
        };
        self.record_event(
            pending.process_id,
            level,
            event_type,
            &pending.terminal_reason,
            serde_json::json!({
                "run_id": pending.run_id,
                "run_key": pending.run_key,
                "config_hash": pending.config_hash,
            }),
        )
        .await;
        self.store
            .get_trading_process(pending.process_id)
            .await
            .map_err(|error| HttpError::internal(error.to_string()))?
            .ok_or_else(|| HttpError::not_found("trading process not found"))
    }

    pub(super) async fn quiesce_for_shutdown(&self, reason: &str) -> Result<(), HttpError> {
        self.begin_shutdown();
        // Wait behind any already-admitted lifecycle transition, then inspect
        // state while owning the same gate so no start can publish afterward.
        let _transition_guard = self.transition.lock().await;
        let pending = self.terminal_pending.lock().await.values().next().cloned();
        if let Some(pending) = pending {
            return Err(HttpError::conflict(format!(
                "BTC run {} has a pending API lifecycle transition; service shutdown left durable state unchanged",
                pending.run_key
            )));
        }
        let active_runs = self
            .active_playbooks
            .lock()
            .await
            .drain()
            .map(|(_, active)| active)
            .collect::<Vec<_>>();
        let mut failures = Vec::new();
        for active in active_runs {
            let process_id = active.process_id;
            let run_id = active.run_id;
            let run_key = active.run_key.clone();
            let config_hash = active.config_hash.clone();
            if let Some(failure) =
                Self::quiesce_live_venue(active.live_venue.as_ref(), "service_shutdown").await
            {
                failures.push(format!("{process_id}: {failure}"));
            }
            let shutdown_result =
                tokio::time::timeout(BTC_RUNTIME_SHUTDOWN_TIMEOUT, active.runtime.shutdown()).await;
            if let Some(shutdown_failure) = match shutdown_result {
                Ok(Ok(())) => None,
                Ok(Err(error)) => Some(format!("btc_runtime_shutdown_failed: {error:#}")),
                Err(_) => Some(format!(
                    "btc_runtime_shutdown_timeout_after_{}s",
                    BTC_RUNTIME_SHUTDOWN_TIMEOUT.as_secs()
                )),
            } {
                failures.push(format!("{process_id}: {shutdown_failure}"));
                continue;
            }
            self.record_event(
                process_id,
                "info",
                "btc_runtime_suspended",
                "BTC realtime execution runtime suspended for service shutdown",
                serde_json::json!({
                    "run_id": run_id,
                    "run_key": run_key,
                    "config_hash": config_hash,
                    "reason": reason,
                    "resume_on_service_restart": true,
                }),
            )
            .await;
            info!(
                process_id = %process_id,
                run_id = %run_id,
                "BTC realtime execution runtime suspended with durable resume intent"
            );
        }
        let shared_runtime = { self.shared_runtime.lock().await.take() };
        if let Some(shared) = shared_runtime {
            if let Some(runtime) = shared.runtime {
                let shutdown_result =
                    tokio::time::timeout(BTC_RUNTIME_SHUTDOWN_TIMEOUT, runtime.shutdown()).await;
                if let Some(shutdown_failure) = match shutdown_result {
                    Ok(Ok(())) => None,
                    Ok(Err(error)) => {
                        Some(format!("shared_btc_runtime_shutdown_failed: {error:#}"))
                    }
                    Err(_) => Some(format!(
                        "shared_btc_runtime_shutdown_timeout_after_{}s",
                        BTC_RUNTIME_SHUTDOWN_TIMEOUT.as_secs()
                    )),
                } {
                    failures.push(shutdown_failure);
                }
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(HttpError::internal(format!(
                "BTC runtimes did not quiesce cleanly during service shutdown; durable lifecycle state was left unchanged: {}",
                failures.join("; ")
            )))
        }
    }

    pub(super) fn begin_shutdown(&self) {
        self.shutting_down.store(true, Ordering::Release);
    }
}
