use super::*;

impl BtcProcessManager {
    pub(super) async fn grafana_countdown_snapshot(
        &self,
        observed_at: chrono::DateTime<Utc>,
    ) -> CountdownSnapshot {
        let active_processes = self.active_playbooks.lock().await.len();
        let state = self
            .shared_runtime
            .lock()
            .await
            .as_ref()
            .map(|shared| shared.state.clone());
        let current_market = match state {
            Some(state) => state.read().await.display_market.clone(),
            None => None,
        };
        let markets = current_market
            .map(|market| vec![market; active_processes])
            .unwrap_or_default();
        CountdownSnapshot::resolve(observed_at, active_processes, markets)
    }

    pub(super) async fn grafana_entry_status_snapshot(
        &self,
        observed_at: chrono::DateTime<Utc>,
    ) -> Result<TradingEntryStatusSnapshot> {
        let processes = self.store.list_observable_btc_processes().await?;
        let active = self
            .active_playbooks
            .lock()
            .await
            .iter()
            .map(|(process_id, active)| {
                (
                    *process_id,
                    (
                        active.execution_mode,
                        active.runtime.is_running(),
                        active.live_venue.clone(),
                    ),
                )
            })
            .collect::<HashMap<_, _>>();
        let mut permissions = Vec::with_capacity(processes.len());
        for process in processes {
            let execution = process.effective_execution();
            let permission = if !process.enabled || !execution.execute_signals {
                EntryPermission::disabled()
            } else if matches!(
                process.status.as_str(),
                "stopping" | "stopped" | "failed" | "expired"
            ) {
                EntryPermission::stopped()
            } else if process.status != "running" {
                EntryPermission::unknown(Some(format!("durable_process_{}", process.status)))
            } else {
                match active.get(&process.process_id) {
                    None => EntryPermission::unknown(Some("runtime_not_attached".to_string())),
                    Some((_, false, _)) => {
                        EntryPermission::unknown(Some("runtime_not_running".to_string()))
                    }
                    Some((BtcExecutionMode::Paper, true, _)) if execution.mode == "paper" => {
                        EntryPermission::enabled()
                    }
                    Some((BtcExecutionMode::Live, true, Some(venue)))
                        if execution.mode == "live" =>
                    {
                        match tokio::time::timeout(GRAFANA_STATUS_READ_TIMEOUT, venue.live_status())
                            .await
                        {
                            Ok(Ok(status)) if status.entries_enabled => EntryPermission::enabled(),
                            Ok(Ok(status)) => EntryPermission::blocked(status.reason),
                            Ok(Err(_)) => EntryPermission::unknown(Some(
                                "live_status_unavailable".to_string(),
                            )),
                            Err(_) => EntryPermission::unknown(Some(
                                "live_status_read_timeout".to_string(),
                            )),
                        }
                    }
                    Some((BtcExecutionMode::Live, true, None)) if execution.mode == "live" => {
                        EntryPermission::unknown(Some("live_venue_unavailable".to_string()))
                    }
                    Some(_) => EntryPermission::unknown(Some(
                        "runtime_execution_mode_mismatch".to_string(),
                    )),
                }
            };
            permissions.push(ProcessEntryPermission {
                process_id: process.process_id,
                permission,
            });
        }
        Ok(TradingEntryStatusSnapshot::new(observed_at, permissions))
    }

    pub(super) async fn grafana_market_path_observation(
        &self,
    ) -> Option<(
        polymarket_bot::btc::BtcIntervalMarket,
        Vec<polymarket_bot::btc::ChainlinkTwap60Point>,
    )> {
        let state = self
            .shared_runtime
            .lock()
            .await
            .as_ref()
            .map(|shared| shared.state.clone());
        let state = state?;
        let (market, twap_history) = {
            let state = state.read().await;
            let market = state.display_market.clone()?;
            let twap_history = state.chainlink_twap_60.iter().cloned().collect::<Vec<_>>();
            (market, twap_history)
        };
        Some((market, twap_history))
    }

    pub(super) async fn reconcile_failed_runtime(&self) {
        let pending = self
            .terminal_pending
            .lock()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for pending in pending {
            let _transition_guard = self.transition.lock().await;
            let current = self
                .terminal_pending
                .lock()
                .await
                .get(&pending.process_id)
                .filter(|current| current.run_id == pending.run_id)
                .cloned();
            let Some(current) = current else {
                continue;
            };
            if let Err(stop_error) = self.finalize_pending_locked(current).await {
                error!(
                    error = ?stop_error,
                    process_id = %pending.process_id,
                    run_id = %pending.run_id,
                    "failed to retry pending BTC terminal transition"
                );
            }
        }
        let process_ids = self
            .active_playbooks
            .lock()
            .await
            .keys()
            .copied()
            .collect::<Vec<_>>();
        let shared_status_inputs = self
            .shared_runtime
            .lock()
            .await
            .as_ref()
            .and_then(|shared| shared.runtime.as_ref().map(BtcRuntimeHandle::status_inputs));
        let shared_status = match shared_status_inputs {
            Some((state, books, metrics, config, running)) => {
                Some(runtime_status_from_inputs(state, books, metrics, config, running).await)
            }
            None => None,
        };
        let shared_running = shared_status.as_ref().is_some_and(|status| status.running);
        if shared_runtime_recovery_required(process_ids.len(), shared_running) {
            warn!(
                reason = shared_status
                    .as_ref()
                    .and_then(|status| status.metrics.last_error.as_deref())
                    .unwrap_or("shared market-data runtime handle is unavailable"),
                active_process_count = process_ids.len(),
                "BTC shared market-data runtime is unavailable; preserving process state and attempting recovery"
            );
            self.recover_shared_runtime().await;
            return;
        }
        for process_id in process_ids {
            let status_input = {
                let active_guard = self.active_playbooks.lock().await;
                let Some(active) = active_guard.get(&process_id) else {
                    continue;
                };
                (active.run_id, active.runtime.status_inputs())
            };
            let (run_id, (state, books, metrics, config, running)) = status_input;
            let status = runtime_status_from_inputs(state, books, metrics, config, running).await;
            let runtime_running = status.running;
            let last_error = status.metrics.last_error;
            if runtime_running {
                match self
                    .store
                    .heartbeat_active_trading_process(process_id)
                    .await
                {
                    Ok(true) => continue,
                    Ok(false) => {}
                    Err(heartbeat_error) => {
                        warn!(
                            error = %heartbeat_error,
                            process_id = %process_id,
                            run_id = %run_id,
                            "failed to persist BTC manager heartbeat"
                        );
                        continue;
                    }
                }
            }
            let reason = last_error.unwrap_or_else(|| {
                if runtime_running {
                    "manager heartbeat rejected because the durable process is not active"
                        .to_string()
                } else {
                    "BTC runtime child task stopped unexpectedly".to_string()
                }
            });
            let terminal_reason = format!("btc_runtime_failed: {reason}");
            if let Err(stop_error) = self
                .stop_process_for_generation(
                    process_id,
                    Some(run_id),
                    &terminal_reason,
                    true,
                    "stopped",
                )
                .await
            {
                error!(
                    error = ?stop_error,
                    process_id = %process_id,
                    run_id = %run_id,
                    "failed to terminalize BTC runtime child failure"
                );
            }
        }
    }

    pub(super) async fn runtime_status(&self) -> serde_json::Value {
        let active_processes = self
            .active_playbooks
            .lock()
            .await
            .values()
            .map(|active| (active.process_id, active.execution_mode))
            .collect::<Vec<_>>();
        let mut processes = Vec::with_capacity(active_processes.len());
        for (process_id, execution_mode) in active_processes {
            processes.push(
                self.runtime_status_for_process(process_id, Some(execution_mode.as_str()))
                    .await,
            );
        }
        let shared_inputs = self
            .shared_runtime
            .lock()
            .await
            .as_ref()
            .and_then(|shared| shared.runtime.as_ref().map(BtcRuntimeHandle::status_inputs));
        let shared_market_data = match shared_inputs {
            Some((state, books, metrics, config, running)) => serde_json::to_value(
                runtime_status_from_inputs(state, books, metrics, config, running).await,
            )
            .unwrap_or_else(|_| serde_json::json!({"running": false})),
            None => serde_json::json!({
                "enabled": true,
                "running": false,
                "readiness": {"ready": false, "reasons": ["shared_market_data_inactive"]}
            }),
        };
        serde_json::json!({
            "capability_enabled": true,
            "active": !processes.is_empty(),
            "active_process_count": processes.len(),
            "shared_market_data": shared_market_data,
            "processes": processes,
        })
    }

    pub(super) async fn runtime_status_for_process(
        &self,
        process_id: uuid::Uuid,
        configured_execution_mode: Option<&str>,
    ) -> serde_json::Value {
        let active = self
            .active_playbooks
            .lock()
            .await
            .get(&process_id)
            .map(|active| {
                (
                    active.process_id,
                    active.run_id,
                    active.run_key.clone(),
                    active.config_hash.clone(),
                    active.execution_mode,
                    active.strategy.clone(),
                    active.live_venue.clone(),
                    active.runtime.status_inputs(),
                )
            });
        if let Some((
            process_id,
            run_id,
            run_key,
            config_hash,
            execution_mode,
            strategy,
            live_venue,
            inputs,
        )) = active
        {
            let (state, books, metrics, config, running) = inputs;
            let mut runtime =
                runtime_status_from_inputs(state, books, metrics, config, running).await;
            runtime.readiness = process_runtime_readiness(&strategy, &runtime.readiness);
            let mut status = serde_json::json!({
                "capability_enabled": true,
                "active": true,
                "process_id": process_id,
                "run_id": run_id,
                "run_key": run_key,
                "config_hash": config_hash,
                "execution_mode": execution_mode.as_str(),
                "runtime": runtime,
            });
            if execution_mode == BtcExecutionMode::Live {
                let live_status = match live_venue {
                    Some(venue) => venue
                        .live_status()
                        .await
                        .and_then(|status| {
                            serde_json::to_value(status).map_err(anyhow::Error::from)
                        })
                        .unwrap_or_else(|_| {
                            serde_json::json!({
                                "entries_enabled": false,
                                "reason": "bound_live_status_unavailable"
                            })
                        }),
                    None => serde_json::json!({
                        "entries_enabled": false,
                        "reason": "bound_live_venue_unavailable"
                    }),
                };
                status
                    .as_object_mut()
                    .expect("BTC runtime status is an object")
                    .insert("live_status".to_string(), live_status);
            }
            return status;
        }
        let configured_execution_mode = configured_execution_mode.unwrap_or("unknown");
        let pending = self.terminal_pending.lock().await.get(&process_id).cloned();
        if let Some(pending) = pending {
            return serde_json::json!({
                "capability_enabled": true,
                "active": false,
                "running": false,
                "lifecycle_state": "terminal_pending",
                "process_id": pending.process_id,
                "run_id": pending.run_id,
                "run_key": pending.run_key,
                "config_hash": pending.config_hash,
                "execution_mode": configured_execution_mode,
                "desired_terminal_status": pending.terminal_status,
                "terminal_reason": pending.terminal_reason,
                "readiness": {"ready": false, "reasons": ["terminal_persistence_pending"]}
            });
        }
        serde_json::json!({
            "capability_enabled": true,
            "active": false,
            "running": false,
            "process_id": process_id,
            "execution_mode": configured_execution_mode,
            "readiness": {"ready": false, "reasons": ["trading_process_inactive"]}
        })
    }
}
