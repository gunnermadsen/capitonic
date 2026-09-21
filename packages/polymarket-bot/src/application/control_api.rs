use super::*;

#[derive(Debug, Default, Clone)]
pub(super) struct RuntimeMetrics {
    pub(super) started_at: chrono::DateTime<Utc>,
}

impl RuntimeMetrics {
    pub(super) fn new() -> Self {
        Self {
            started_at: Utc::now(),
            ..Self::default()
        }
    }

    pub(super) fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "uptime_secs": (Utc::now() - self.started_at).num_seconds()
        })
    }
}

pub(super) struct RuntimeControl {
    pub(super) store: Store,
    pub(super) live_venue: Option<Arc<LiveVenue>>,
    pub(super) metrics: Arc<Mutex<RuntimeMetrics>>,
    pub(super) btc_manager: Option<BtcProcessManager>,
}

impl RuntimeControl {
    pub(super) fn live_venue(&self) -> Result<Arc<dyn ExecutionVenue>, HttpError> {
        self.live_venue
            .clone()
            .map(|venue| venue as Arc<dyn ExecutionVenue>)
            .ok_or_else(|| HttpError::bad_request("live execution is not configured"))
    }
}

#[async_trait]
impl ControlApi for RuntimeControl {
    async fn health(&self) -> Result<HealthResponse, HttpError> {
        self.store
            .healthcheck()
            .await
            .map_err(|error| HttpError::internal(format!("database unhealthy: {error}")))?;
        Ok(HealthResponse {
            service: "polymarket-bot".to_string(),
            status: HealthStatus::Ok,
            checked_at: Utc::now(),
        })
    }

    async fn metrics(&self) -> Result<MetricsResponse, HttpError> {
        let counters = self
            .metrics
            .lock()
            .map_err(|_| HttpError::internal("metrics lock poisoned"))?
            .to_json();
        Ok(MetricsResponse {
            service: "polymarket-bot".to_string(),
            captured_at: Utc::now(),
            counters,
            gauges: serde_json::json!({}),
        })
    }

    async fn prometheus_metrics(&self) -> Result<String, HttpError> {
        let runtime = match &self.btc_manager {
            Some(manager) => manager.runtime_status().await,
            None => serde_json::json!({}),
        };
        let metrics = runtime
            .get("shared_market_data")
            .and_then(|value| value.get("metrics"));
        let candle_ready = metrics
            .and_then(|value| value.get("rtds_chainlink_candle_window_ready"))
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        let complete_minutes = metrics
            .and_then(|value| value.get("rtds_chainlink_candle_complete_minutes"))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        let oracle_ready = metrics
            .and_then(|value| value.get("polygon_oracle_ready"))
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        let oracle_age = metrics
            .and_then(|value| value.get("polygon_oracle_age_seconds"))
            .and_then(serde_json::Value::as_u64);

        let mut output = String::from(
            "# HELP polymarket_btc_rtds_chainlink_candle_window_ready Whether the required 61 closed RTDS Chainlink candle minutes are available.\n\
# TYPE polymarket_btc_rtds_chainlink_candle_window_ready gauge\n",
        );
        output.push_str(&format!(
            "polymarket_btc_rtds_chainlink_candle_window_ready {}\n",
            u8::from(candle_ready)
        ));
        output.push_str(
            "# HELP polymarket_btc_rtds_chainlink_candle_complete_minutes Complete closed RTDS Chainlink candle minutes in the required window.\n\
# TYPE polymarket_btc_rtds_chainlink_candle_complete_minutes gauge\n",
        );
        output.push_str(&format!(
            "polymarket_btc_rtds_chainlink_candle_complete_minutes {complete_minutes}\n"
        ));
        output.push_str(
            "# HELP polymarket_btc_polygon_oracle_ready Whether a causally valid and fresh Polygon oracle round is available.\n\
# TYPE polymarket_btc_polygon_oracle_ready gauge\n",
        );
        output.push_str(&format!(
            "polymarket_btc_polygon_oracle_ready {}\n",
            u8::from(oracle_ready)
        ));
        output.push_str(
            "# HELP polymarket_btc_polygon_oracle_age_seconds Age in seconds of the latest accepted Polygon oracle round.\n\
# TYPE polymarket_btc_polygon_oracle_age_seconds gauge\n",
        );
        if let Some(oracle_age) = oracle_age {
            output.push_str(&format!(
                "polymarket_btc_polygon_oracle_age_seconds {oracle_age}\n"
            ));
        }
        output.push_str(&polymarket_bot::market_data_stream::prometheus_metrics());
        output.push_str(&polymarket_bot::btc::execution_freshness::prometheus_metrics(&runtime));
        output
            .push_str(&polymarket_bot::btc::unified_model_runtime::telemetry::prometheus_metrics());
        Ok(output)
    }

    async fn btc_realtime_status(&self) -> Result<serde_json::Value, HttpError> {
        let Some(manager) = &self.btc_manager else {
            return Ok(serde_json::json!({
                "capability_enabled": false,
                "active": false,
                "running": false,
                "readiness": {"ready": false, "reasons": ["btc_realtime_capability_disabled"]}
            }));
        };
        Ok(manager.runtime_status().await)
    }

    async fn btc_entry_status(
        &self,
        request: EntryStatusRequest,
    ) -> Result<polymarket_bot::grafana_live::EntryStatusSelection, HttpError> {
        let manager = self.btc_manager.as_ref().ok_or_else(|| {
            HttpError::internal("BTC realtime capability is disabled for this deployment")
        })?;
        let snapshot = manager
            .grafana_entry_status_snapshot(Utc::now())
            .await
            .map_err(|error| HttpError::internal(error.to_string()))?;
        Ok(snapshot.select(&request.scope, request.process_id))
    }

    async fn live_status(&self) -> Result<LiveVenueStatus, HttpError> {
        self.live_venue()?
            .live_status()
            .await
            .map_err(|error| HttpError::internal(error.to_string()))
    }

    async fn live_identity_diagnostics(&self) -> Result<LiveIdentityDiagnostics, HttpError> {
        self.live_venue()?
            .live_identity_diagnostics()
            .await
            .map_err(|error| HttpError::internal(error.to_string()))
    }

    async fn live_wallet_address_diagnostics(
        &self,
        candidate_addresses: Vec<String>,
    ) -> Result<LiveWalletAddressDiagnostics, HttpError> {
        self.live_venue()?
            .live_wallet_address_diagnostics(candidate_addresses)
            .await
            .map_err(|error| HttpError::internal(error.to_string()))
    }

    async fn live_order_dry_run(
        &self,
        request: LiveOrderDryRunRequest,
    ) -> Result<LiveOrderDryRunDiagnostics, HttpError> {
        self.live_venue()?
            .live_order_dry_run(request)
            .await
            .map_err(|error| HttpError::internal(error.to_string()))
    }

    async fn live_poly1271_funder_probe(
        &self,
        request: LivePoly1271FunderProbeRequest,
    ) -> Result<LivePoly1271FunderProbeResponse, HttpError> {
        self.live_venue()?
            .live_poly1271_funder_probe(request)
            .await
            .map_err(|error| HttpError::internal(error.to_string()))
    }

    async fn live_halt(&self) -> Result<serde_json::Value, HttpError> {
        let _transition_guard = match &self.btc_manager {
            Some(manager) => Some(manager.transition.lock().await),
            None => None,
        };
        let live = self.live_venue()?;
        let disable_result = match tokio::time::timeout(
            BTC_LIVE_CONTROL_TIMEOUT,
            live.set_live_entries_enabled(false, Some("manual_live_halt".to_string())),
        )
        .await
        {
            Ok(result) => result.map_err(|error| error.to_string()),
            Err(_) => Err(format!(
                "live disable timed out after {}s",
                BTC_LIVE_CONTROL_TIMEOUT.as_secs()
            )),
        };
        let cancel_result =
            match tokio::time::timeout(BTC_LIVE_CONTROL_TIMEOUT, live.cancel_all()).await {
                Ok(result) => result.map_err(|error| error.to_string()),
                Err(_) => Err(format!(
                    "live cancel-all timed out after {}s",
                    BTC_LIVE_CONTROL_TIMEOUT.as_secs()
                )),
            };
        let reconcile_result =
            match tokio::time::timeout(BTC_LIVE_CONTROL_TIMEOUT, live.reconcile()).await {
                Ok(result) => result.map_err(|error| error.to_string()),
                Err(_) => Err(format!(
                    "live reconciliation timed out after {}s",
                    BTC_LIVE_CONTROL_TIMEOUT.as_secs()
                )),
            };
        let halted = disable_result
            .as_ref()
            .is_ok_and(|status| !status.entries_enabled)
            && cancel_result.is_ok()
            && reconcile_result
                .as_ref()
                .is_ok_and(|report| report.open_orders == 0 && report.unresolved_count == 0);
        self.store
            .insert_service_event(&ServiceEvent::new(
                "manual_live_halt",
                serde_json::json!({
                    "halted": halted,
                    "disable_result": disable_result.as_ref().map(|status| serde_json::json!({"entries_enabled": status.entries_enabled, "reason": status.reason})).unwrap_or_else(|error| serde_json::json!({"error": error})),
                    "cancel_result": cancel_result.as_ref().map(|count| serde_json::json!({"cancelled": count})).unwrap_or_else(|error| serde_json::json!({"error": error})),
                    "reconcile_result": reconcile_result.as_ref().map(|report| serde_json::json!({
                        "open_orders": report.open_orders,
                        "mismatches_found": report.mismatches_found,
                        "unresolved_count": report.unresolved_count
                    })).unwrap_or_else(|error| serde_json::json!({"error": error}))
                }),
            ))
            .await
            .map_err(|error| HttpError::internal(error.to_string()))?;
        Ok(serde_json::json!({
            "halted": halted,
            "cancelled": cancel_result.ok(),
            "reconciled": reconcile_result.ok()
        }))
    }

    async fn live_reconcile(&self) -> Result<serde_json::Value, HttpError> {
        let report = self
            .live_venue()?
            .reconcile()
            .await
            .map_err(|error| HttpError::internal(error.to_string()))?;
        serde_json::to_value(report).map_err(|error| HttpError::internal(error.to_string()))
    }

    async fn live_account_reconcile(
        &self,
        request: control_http::AccountReconcileRequest,
    ) -> Result<control_http::AccountReconcileReport, HttpError> {
        self.live_venue()?
            .live_account_reconcile(request)
            .await
            .map_err(|error| HttpError::internal(error.to_string()))
    }

    async fn live_set_entries_enabled(&self, enabled: bool) -> Result<LiveVenueStatus, HttpError> {
        if enabled {
            return Err(HttpError::bad_request(
                "wallet-wide live entry enable is disabled; enable entries on an active process-scoped runtime",
            ));
        }
        let _transition_guard = match &self.btc_manager {
            Some(manager) => Some(manager.transition.lock().await),
            None => None,
        };
        self.live_venue()?
            .set_live_entries_enabled(enabled, (!enabled).then(|| "manual_disable".to_string()))
            .await
            .map_err(|error| HttpError::internal(error.to_string()))
    }

    async fn trading_process_live_preflight(
        &self,
        process_id: uuid::Uuid,
    ) -> Result<TradingProcessLivePreflightResponse, HttpError> {
        let manager = self.btc_manager.as_ref().ok_or_else(|| {
            HttpError::bad_request("BTC realtime capability is disabled for this deployment")
        })?;
        manager.live_preflight(process_id).await
    }

    async fn set_trading_process_live_entries_enabled(
        &self,
        process_id: uuid::Uuid,
        enabled: bool,
    ) -> Result<LiveVenueStatus, HttpError> {
        let manager = self.btc_manager.as_ref().ok_or_else(|| {
            HttpError::bad_request("BTC realtime capability is disabled for this deployment")
        })?;
        manager.set_live_entries_enabled(process_id, enabled).await
    }

    async fn list_trading_processes(
        &self,
        request: control_http::ListTradingProcessesRequest,
    ) -> Result<TradingProcessesResponse, HttpError> {
        let processes = self
            .store
            .list_trading_processes(request.limit.unwrap_or(50).clamp(1, 500))
            .await
            .map_err(|error| HttpError::internal(error.to_string()))?;
        Ok(TradingProcessesResponse { processes })
    }

    async fn get_trading_process(
        &self,
        process_id: uuid::Uuid,
    ) -> Result<TradingProcessResponse, HttpError> {
        let process = self
            .store
            .get_trading_process(process_id)
            .await
            .map_err(|error| HttpError::internal(error.to_string()))?
            .ok_or_else(|| HttpError::not_found("trading process not found"))?;
        Ok(TradingProcessResponse { process })
    }

    async fn upsert_trading_process_by_key(
        &self,
        process_key: String,
        mut request: control_http::UpsertTradingProcessByKeyRequest,
    ) -> Result<TradingProcessResponse, HttpError> {
        pin_router_config(&mut request.config)?;
        let key = process_key.trim();
        if key.is_empty() {
            return Err(HttpError::bad_request("trading process key is required"));
        }
        let name = request.name.trim();
        if name.is_empty() {
            return Err(HttpError::bad_request("trading process name is required"));
        }
        let process_type = request.process_type.trim();
        let process_scope = request.process_scope.trim();
        if !BtcProcessManager::is_managed_identity(process_type, process_scope) {
            return Err(HttpError::bad_request(
                "only btc_5m/realtime_paper process definitions are supported",
            ));
        }
        let status = request.status.trim();
        if status.is_empty() {
            return Err(HttpError::bad_request("trading process status is required"));
        }
        if request.enabled || matches!(status, "starting" | "running" | "stopping") {
            return Err(HttpError::bad_request(
                "BTC lifecycle cannot be activated through PUT; use the process /start endpoint",
            ));
        }
        if !matches!(status, "created" | "stopped" | "failed" | "completed") {
            return Err(HttpError::bad_request(
                "BTC definition status must be created, stopped, failed, or completed while inactive",
            ));
        }
        // Keep the lifecycle lock through the definition read and write. This
        // prevents /start from freezing the old definition while this request
        // concurrently replaces it.
        let lifecycle_guard = match &self.btc_manager {
            Some(manager) => Some(manager.transition.lock().await),
            None => None,
        };
        let manager = self.btc_manager.as_ref().ok_or_else(|| {
            HttpError::bad_request("BTC realtime capability is disabled for this deployment")
        })?;
        let now = Utc::now();
        manager.validate_inactive_definition(&TradingProcess {
            process_id: uuid::Uuid::nil(),
            name: name.to_string(),
            process_type: BTC_PROCESS_TYPE.to_string(),
            process_scope: BTC_PROCESS_SCOPE.to_string(),
            process_key: Some(key.to_string()),
            status: status.to_string(),
            enabled: false,
            config: request.config.clone(),
            metadata: request.metadata.clone(),
            created_at: now,
            updated_at: now,
            started_at: None,
            stopped_at: Some(now),
            last_error: None,
        })?;
        let existing = self
            .store
            .get_btc_realtime_paper_process_by_key(key)
            .await
            .map_err(|error| HttpError::internal(error.to_string()))?;
        if let Some(existing) = existing {
            if status != existing.status {
                return Err(HttpError::bad_request(
                    "BTC status is lifecycle-owned; stable-key PUT must preserve the current status",
                ));
            }
            let runtime_active = match &self.btc_manager {
                Some(manager) => manager
                    .active_playbooks
                    .lock()
                    .await
                    .contains_key(&existing.process_id),
                None => false,
            };
            let terminal_pending = match &self.btc_manager {
                Some(manager) => manager
                    .terminal_pending
                    .lock()
                    .await
                    .contains_key(&existing.process_id),
                None => false,
            };
            if runtime_active
                || terminal_pending
                || existing.enabled
                || matches!(
                    existing.status.as_str(),
                    "starting" | "running" | "stopping"
                )
            {
                return Err(HttpError::conflict(
                    "stop the BTC trading process through its /stop endpoint before reconfiguring it",
                ));
            }
        } else if status != "created" {
            return Err(HttpError::bad_request(
                "a new BTC process definition must be created with status=created",
            ));
        }
        let process = self
            .store
            .upsert_btc_realtime_paper_process_by_key(
                name,
                key,
                status,
                request.config,
                request.metadata,
            )
            .await
            .map_err(|error| HttpError::internal(error.to_string()))?;
        drop(lifecycle_guard);
        Ok(TradingProcessResponse { process })
    }

    async fn get_trading_process_status(
        &self,
        process_id: uuid::Uuid,
    ) -> Result<TradingProcessStatusResponse, HttpError> {
        let process = self
            .store
            .get_trading_process(process_id)
            .await
            .map_err(|error| HttpError::internal(error.to_string()))?
            .ok_or_else(|| HttpError::not_found("trading process not found"))?;
        let mut status = self
            .store
            .trading_process_status(process_id)
            .await
            .map_err(|error| HttpError::internal(error.to_string()))?
            .ok_or_else(|| HttpError::not_found("trading process not found"))?;
        if BtcProcessManager::is_managed_process(&process) {
            if let Some(object) = status.as_object_mut() {
                object.insert(
                    "aggregate_scope".to_string(),
                    serde_json::Value::String("process_lifetime".to_string()),
                );
                let execution_mode = process.effective_execution().mode;
                let runtime = match &self.btc_manager {
                    Some(manager) => {
                        manager
                            .runtime_status_for_process(process_id, Some(&execution_mode))
                            .await
                    }
                    None => serde_json::json!({
                        "capability_enabled": false,
                        "active": false,
                        "running": false,
                        "execution_mode": execution_mode
                    }),
                };
                object.insert("btc_runtime".to_string(), runtime);
            }
        }
        Ok(TradingProcessStatusResponse { process_id, status })
    }

    async fn preview_trading_process_start(
        &self,
        process_id: uuid::Uuid,
    ) -> Result<TradingProcessStartPreviewResponse, HttpError> {
        let current = self
            .store
            .get_trading_process(process_id)
            .await
            .map_err(|error| HttpError::internal(error.to_string()))?
            .ok_or_else(|| HttpError::not_found("trading process not found"))?;
        if !BtcProcessManager::is_managed_process(&current) {
            return Err(HttpError::bad_request(
                "start preview is available only for managed BTC realtime execution processes",
            ));
        }
        let manager = self.btc_manager.as_ref().ok_or_else(|| {
            HttpError::bad_request("BTC realtime capability is disabled for this deployment")
        })?;
        manager.preview_start(process_id).await
    }

    async fn update_trading_process(
        &self,
        process_id: uuid::Uuid,
        mut request: control_http::UpdateTradingProcessRequest,
    ) -> Result<TradingProcessResponse, HttpError> {
        if let Some(config) = request.config.as_mut() {
            pin_router_config(config)?;
        }
        let name = request.name.as_deref().map(str::trim);
        if matches!(name, Some("")) {
            return Err(HttpError::bad_request(
                "trading process name cannot be empty",
            ));
        }
        let lifecycle_guard = match &self.btc_manager {
            Some(manager) => Some(manager.transition.lock().await),
            None => None,
        };
        let current = self
            .store
            .get_trading_process(process_id)
            .await
            .map_err(|error| HttpError::internal(error.to_string()))?
            .ok_or_else(|| HttpError::not_found("trading process not found"))?;
        if !BtcProcessManager::is_managed_process(&current) {
            return Err(HttpError::bad_request(
                "only managed BTC realtime execution process definitions can be updated",
            ));
        }
        let runtime_active = match &self.btc_manager {
            Some(manager) => manager
                .active_playbooks
                .lock()
                .await
                .contains_key(&process_id),
            None => false,
        };
        let terminal_pending = match &self.btc_manager {
            Some(manager) => manager
                .terminal_pending
                .lock()
                .await
                .contains_key(&process_id),
            None => false,
        };
        let active_selector_update = runtime_active
            && request.name.is_none()
            && request.metadata.is_none()
            && request.config.as_ref().is_some_and(|config| {
                selector_only_config_change(&current.config, config).unwrap_or(false)
            });
        if terminal_pending
            || current.enabled && !active_selector_update
            || matches!(current.status.as_str(), "starting" | "stopping")
        {
            return Err(HttpError::conflict(
                "stop the BTC trading process through its /stop endpoint before reconfiguring it",
            ));
        }
        if let Some(config) = &request.config {
            let manager = self.btc_manager.as_ref().ok_or_else(|| {
                HttpError::bad_request("BTC realtime capability is disabled for this deployment")
            })?;
            let mut candidate = current.clone();
            candidate.config = config.clone();
            if active_selector_update {
                manager.validate_resume_definition(&candidate)?;
            } else {
                manager.validate_inactive_definition(&candidate)?;
            }
        }
        let process = self
            .store
            .update_btc_realtime_paper_process_definition(
                process_id,
                name,
                request.config,
                request.metadata,
            )
            .await
            .map_err(|error| HttpError::internal(error.to_string()))?
            .ok_or_else(|| HttpError::not_found("trading process not found"))?;
        if active_selector_update {
            let manager = self.btc_manager.as_ref().expect("BTC manager exists");
            let sources = manager
                .validate_resume_definition(&process)?
                .control
                .sources;
            {
                let mut active = manager.active_playbooks.lock().await;
                active
                    .get_mut(&process_id)
                    .expect("active selector update retains its runtime")
                    .sources = sources;
                let union = merge_runtime_source_selectors(
                    active
                        .values()
                        .flat_map(|playbook| playbook.sources.clone()),
                    manager.config.grafana_live_enabled,
                )?;
                if let Some(runtime) = manager
                    .shared_runtime
                    .lock()
                    .await
                    .as_ref()
                    .and_then(|shared| shared.runtime.as_ref())
                {
                    runtime.update_sources(union);
                }
            }
        }
        drop(lifecycle_guard);
        Ok(TradingProcessResponse { process })
    }

    async fn start_trading_process(
        &self,
        process_id: uuid::Uuid,
    ) -> Result<TradingProcessResponse, HttpError> {
        let current = self
            .store
            .get_trading_process(process_id)
            .await
            .map_err(|error| HttpError::internal(error.to_string()))?
            .ok_or_else(|| HttpError::not_found("trading process not found"))?;
        if !BtcProcessManager::is_managed_process(&current) {
            return Err(HttpError::bad_request(
                "only managed BTC realtime execution processes can be started",
            ));
        }
        let manager = self.btc_manager.as_ref().ok_or_else(|| {
            HttpError::bad_request("BTC realtime capability is disabled for this deployment")
        })?;
        let process = manager.start_process(process_id).await?;
        Ok(TradingProcessResponse { process })
    }

    async fn stop_trading_process(
        &self,
        process_id: uuid::Uuid,
    ) -> Result<TradingProcessResponse, HttpError> {
        let current = self
            .store
            .get_trading_process(process_id)
            .await
            .map_err(|error| HttpError::internal(error.to_string()))?
            .ok_or_else(|| HttpError::not_found("trading process not found"))?;
        if !BtcProcessManager::is_managed_process(&current) {
            return Err(HttpError::bad_request(
                "only managed BTC realtime execution processes can be stopped",
            ));
        }
        let manager = self.btc_manager.as_ref().ok_or_else(|| {
            HttpError::bad_request("BTC realtime capability is disabled for this deployment")
        })?;
        let process = manager.stop_process(process_id, "api_stop").await?;
        Ok(TradingProcessResponse { process })
    }

    async fn complete_trading_process(
        &self,
        process_id: uuid::Uuid,
    ) -> Result<TradingProcessResponse, HttpError> {
        let current = self
            .store
            .get_trading_process(process_id)
            .await
            .map_err(|error| HttpError::internal(error.to_string()))?
            .ok_or_else(|| HttpError::not_found("trading process not found"))?;
        if !BtcProcessManager::is_managed_process(&current) {
            return Err(HttpError::bad_request(
                "only managed BTC realtime execution processes can be completed",
            ));
        }
        let manager = self.btc_manager.as_ref().ok_or_else(|| {
            HttpError::bad_request("BTC realtime capability is disabled for this deployment")
        })?;
        let process = manager.complete_process(process_id, "api_complete").await?;
        Ok(TradingProcessResponse { process })
    }
}
