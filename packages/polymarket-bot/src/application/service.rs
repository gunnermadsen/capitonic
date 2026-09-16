use super::*;

pub(crate) async fn run() -> Result<()> {
    install_tls_crypto_provider();
    init_tracing();

    let config = AppConfig::from_env()?;
    let live_user_ws_enabled = config.live.user_ws_auth_available();
    info!(
        service = "polymarket-bot",
        live_order_submit_enabled = false,
        live_user_ws_enabled,
        btc_realtime_enabled = true,
        btc_paper_enabled = true,
        grafana_live_enabled = config.grafana_live.enabled,
        compiled_source_identity = COMPILED_SOURCE_IDENTITY,
        "starting Polymarket bot"
    );

    let pool = PgPoolOptions::new()
        .max_connections(8)
        .connect(&config.postgres.database_url())
        .await
        .context("failed to connect Polymarket application to Postgres")?;
    let store = Store::from_pool(pool.clone());
    store.healthcheck().await?;
    store
        .insert_service_event(&ServiceEvent::new(
            "service_started",
            serde_json::json!({
                "execution_control": "trade_processes",
                "live_order_submit_enabled": false,
                "live_user_ws_enabled": live_user_ws_enabled,
                "btc_realtime_enabled": true,
                "btc_paper_enabled": true,
                "grafana_live_enabled": config.grafana_live.enabled,
                "compiled_source_identity": COMPILED_SOURCE_IDENTITY,
                "kafka_required": false
            }),
        ))
        .await?;

    let data_api = DataApiClient::new(config.data_api_base_url.clone());
    let live_venue: Option<Arc<LiveVenue>> = if config.live.live_auth_available() {
        Some(Arc::new(LiveVenue::new(
            config.live.clone(),
            config.live.clob_api_base_url.clone(),
            store.clone(),
            data_api.clone(),
        )?))
    } else {
        None
    };
    let btc_manager = {
        let repository = BtcRepository::from_pool(pool.clone());
        Some(BtcProcessManager::new(
            store.clone(),
            pool.clone(),
            repository,
            BtcProcessManagerConfig {
                live_venue: live_venue.clone(),
                live_reconcile_interval: config.live.reconcile_interval,
            },
        ))
    };
    if let Some(manager) = &btc_manager {
        if let Err(error) = manager.resume_durable_processes().await {
            bail!(
                "failed to resume durable BTC realtime execution state; database lifecycle state was left unchanged: {error:?}"
            );
        }
    }

    let (grafana_live_shutdown_tx, grafana_live_shutdown_rx) = tokio::sync::watch::channel(false);
    let grafana_live_task = if config.grafana_live.enabled {
        let manager = btc_manager
            .clone()
            .context("Grafana Live countdown requires the BTC process manager")?;
        let publisher = GrafanaLivePublisher::new(config.grafana_live.clone())?;
        Some(tokio::spawn(run_grafana_live(
            manager,
            publisher,
            grafana_live_shutdown_rx,
        )))
    } else {
        None
    };

    let metrics = RuntimeMetrics::new();
    let shared_metrics = Arc::new(Mutex::new(metrics.clone()));
    if config.http.enabled {
        let control: control_http::SharedControlApi = Arc::new(RuntimeControl {
            store: store.clone(),
            live_venue: live_venue.clone(),
            metrics: shared_metrics.clone(),
            btc_manager: btc_manager.clone(),
        });
        let app = control_http::router(control, config.http.admin_token.clone());
        let bind = config.http.bind.clone();
        tokio::spawn(async move {
            match tokio::net::TcpListener::bind(&bind).await {
                Ok(listener) => {
                    info!(bind = %bind, "Polymarket control HTTP server listening");
                    if let Err(error) = axum::serve(listener, app).await {
                        error!(error = %error, "Polymarket control HTTP server failed");
                    }
                }
                Err(error) => {
                    error!(error = %error, bind = %bind, "failed to bind Polymarket control HTTP server")
                }
            }
        });
    }
    let mut health_interval = tokio::time::interval(config.health_interval);
    health_interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let shutdown = shutdown_signal();
    tokio::pin!(shutdown);

    loop {
        tokio::select! {
            biased;

            _ = &mut shutdown => {
                warn!("shutdown signal received");
                let _ = grafana_live_shutdown_tx.send(true);
                if let Some(manager) = &btc_manager {
                    if let Err(error) = manager.quiesce_for_shutdown("service_shutdown").await {
                        error!(
                            error = ?error,
                            "failed to stop active BTC runtime during service shutdown"
                        );
                    }
                }
                store.insert_service_event(&ServiceEvent::new("service_stopped", serde_json::json!({}))).await.ok();
                break;
            }

            _ = health_interval.tick() => {
                if let Some(manager) = &btc_manager {
                    manager.reconcile_failed_runtime().await;
                }
                if let Err(error) = store.healthcheck().await {
                    error!(error = %error, "database healthcheck failed");
                }
                info!(
                    target: "metrics",
                    uptime_secs = (Utc::now() - metrics.started_at).num_seconds(),
                    "polymarket bot liveness ok"
                );
                if let Ok(mut shared) = shared_metrics.lock() {
                    *shared = metrics.clone();
                }
            }

        }
    }

    if let Some(task) = grafana_live_task {
        if let Err(join_error) = task.await {
            warn!(error = %join_error, "Grafana Live countdown publisher task did not join cleanly");
        }
    }

    Ok(())
}

fn install_tls_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .json()
        .with_current_span(false)
        .with_span_list(false)
        .init();
}

async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(error) = tokio::signal::ctrl_c().await {
            warn!(error = %error, "failed to install ctrl-c handler");
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(error) => warn!(error = %error, "failed to install terminate signal handler"),
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
