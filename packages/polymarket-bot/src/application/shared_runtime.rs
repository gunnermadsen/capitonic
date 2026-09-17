use super::*;

impl BtcProcessManager {
    pub(super) async fn ensure_shared_runtime(
        &self,
        config: &BtcRuntimeConfig,
        sources: &[SourceSelector],
    ) -> Result<
        (
            Arc<tokio::sync::RwLock<polymarket_bot::btc::RealtimeState>>,
            Arc<tokio::sync::RwLock<BookRegistry>>,
        ),
        HttpError,
    > {
        // Durable processes resume concurrently after a container restart.
        // Serialize only shared transport initialization so they converge on
        // one selector-union consumer instead of racing to create one each.
        let _startup_guard = self.shared_runtime_startup.lock().await;
        let active_guard = self.active_playbooks.lock().await;
        let active_playbooks = active_guard.len();
        let source_union = merge_source_selectors(
            active_guard
                .values()
                .flat_map(|playbook| playbook.sources.clone())
                .chain(sources.iter().cloned()),
        )?;
        drop(active_guard);
        let retired_runtime = {
            let mut shared = self.shared_runtime.lock().await;
            if let Some(existing) = shared.as_ref() {
                let compatible = shared_market_data_config_compatible(&existing.config, config);
                if compatible
                    && existing
                        .runtime
                        .as_ref()
                        .is_some_and(BtcRuntimeHandle::is_running)
                {
                    if let Some(runtime) = existing.runtime.as_ref() {
                        runtime.update_sources(source_union.clone());
                    }
                    return Ok((existing.state.clone(), existing.books.clone()));
                }
                if active_playbooks > 0 {
                    let message = if compatible {
                        "BTC shared market-data runtime is not running"
                    } else {
                        "BTC playbook market-data runtime settings differ from the active shared runtime"
                    };
                    return Err(HttpError::conflict(message));
                }
            }
            shared.take()
        };
        if let Some(retired) = retired_runtime {
            if let Some(runtime) = retired.runtime {
                match tokio::time::timeout(BTC_RUNTIME_SHUTDOWN_TIMEOUT, runtime.shutdown()).await {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => warn!(
                        error = ?error,
                        "retired BTC shared market-data runtime reported an integrity failure"
                    ),
                    Err(_) => warn!(
                        timeout_secs = BTC_RUNTIME_SHUTDOWN_TIMEOUT.as_secs(),
                        "timed out retiring BTC shared market-data runtime"
                    ),
                }
            }
        }

        let state = Arc::new(tokio::sync::RwLock::new(
            polymarket_bot::btc::RealtimeState::default(),
        ));
        let books = Arc::new(tokio::sync::RwLock::new(BookRegistry::new(
            uuid::Uuid::new_v4(),
        )));
        let runtime = BtcRuntime::new(config.clone(), self.repository.clone())
            .with_sources(source_union)
            .with_shared_state(state.clone())
            .with_shared_book_registry(books.clone())
            .start()
            .await
            .map_err(|error| {
                HttpError::internal(format!(
                    "failed to start shared BTC market-data runtime: {error:#}"
                ))
            })?;
        info!("BTC shared market-data gRPC runtime started");
        let mut shared = self.shared_runtime.lock().await;
        debug_assert!(shared.is_none());
        *shared = Some(SharedBtcRuntime {
            config: config.clone(),
            state: state.clone(),
            books: books.clone(),
            runtime: Some(runtime),
        });
        Ok((state, books))
    }

    pub(super) async fn refresh_shared_sources(&self) -> Result<(), HttpError> {
        let source_union = merge_source_selectors(
            self.active_playbooks
                .lock()
                .await
                .values()
                .flat_map(|playbook| playbook.sources.clone()),
        )?;
        if source_union.is_empty() {
            return Ok(());
        }
        if let Some(runtime) = self
            .shared_runtime
            .lock()
            .await
            .as_ref()
            .and_then(|shared| shared.runtime.as_ref())
        {
            runtime.update_sources(source_union);
        }
        Ok(())
    }

    pub(super) async fn recover_shared_runtime(&self) {
        if self.shutting_down.load(Ordering::Acquire) {
            return;
        }
        let _transition_guard = self.transition.lock().await;
        if self.shutting_down.load(Ordering::Acquire)
            || self.active_playbooks.lock().await.is_empty()
        {
            return;
        }

        let (config, state, books, failed_runtime) = {
            let mut shared_guard = self.shared_runtime.lock().await;
            let Some(shared) = shared_guard.as_mut() else {
                error!(
                    "BTC shared market-data runtime handle is unavailable; preserving active process configuration"
                );
                return;
            };
            if shared
                .runtime
                .as_ref()
                .is_some_and(BtcRuntimeHandle::is_running)
            {
                return;
            }
            (
                shared.config.clone(),
                shared.state.clone(),
                shared.books.clone(),
                shared.runtime.take(),
            )
        };

        if let Some(runtime) = failed_runtime {
            match tokio::time::timeout(BTC_RUNTIME_SHUTDOWN_TIMEOUT, runtime.shutdown()).await {
                Ok(Ok(())) => {}
                Ok(Err(error)) => warn!(
                    error = ?error,
                    "failed BTC shared market-data runtime reported an integrity failure during recovery"
                ),
                Err(_) => warn!(
                    timeout_secs = BTC_RUNTIME_SHUTDOWN_TIMEOUT.as_secs(),
                    "timed out retiring failed BTC shared market-data runtime during recovery"
                ),
            }
        }

        invalidate_shared_market_data_evidence(&state, &books).await;

        let sources = self
            .active_playbooks
            .lock()
            .await
            .values()
            .flat_map(|playbook| playbook.sources.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let recovery = BtcRuntime::new(config.clone(), self.repository.clone())
            .with_sources(sources)
            .with_shared_state(state)
            .with_shared_book_registry(books)
            .start()
            .await;
        match recovery {
            Ok(runtime) => {
                let mut shared_guard = self.shared_runtime.lock().await;
                let Some(shared) = shared_guard.as_mut() else {
                    warn!("recovered BTC shared market-data runtime lost its manager slot");
                    drop(runtime);
                    return;
                };
                shared.runtime = Some(runtime);
                drop(shared_guard);
                info!(
                    active_process_count = self.active_playbooks.lock().await.len(),
                    "BTC shared market-data runtime recovered without changing durable process state"
                );
            }
            Err(error) => warn!(
                error = ?error,
                "BTC shared market-data runtime recovery deferred; active process configuration remains enabled"
            ),
        }
    }

    pub(super) async fn shutdown_shared_runtime_if_idle(&self) {
        if !self.active_playbooks.lock().await.is_empty() {
            return;
        }
        let shared = { self.shared_runtime.lock().await.take() };
        let Some(shared) = shared else {
            return;
        };
        let Some(runtime) = shared.runtime else {
            return;
        };
        match tokio::time::timeout(BTC_RUNTIME_SHUTDOWN_TIMEOUT, runtime.shutdown()).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => warn!(
                error = ?error,
                "idle BTC shared market-data runtime reported an integrity failure during shutdown"
            ),
            Err(_) => warn!(
                timeout_secs = BTC_RUNTIME_SHUTDOWN_TIMEOUT.as_secs(),
                "timed out shutting down idle BTC shared market-data runtime"
            ),
        }
    }
}
