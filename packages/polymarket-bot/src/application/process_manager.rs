use super::*;

pub(super) struct ActiveBtcPlaybook {
    pub(super) process_id: uuid::Uuid,
    pub(super) run_id: uuid::Uuid,
    pub(super) run_key: String,
    pub(super) config_hash: String,
    pub(super) execution_mode: BtcExecutionMode,
    pub(super) strategy: BtcStrategyConfig,
    pub(super) sources: Vec<SourceSelector>,
    pub(super) live_venue: Option<Arc<LiveVenue>>,
    pub(super) runtime: BtcPlaybookRuntimeHandle,
}

pub(super) struct BtcExecutionComponents {
    pub(super) venue: Arc<dyn ExecutionVenue>,
    pub(super) lifecycle: Arc<dyn BtcExecutionLifecycle>,
    pub(super) live_venue: Option<Arc<LiveVenue>>,
}

pub(super) struct SharedBtcRuntime {
    pub(super) config: BtcRuntimeConfig,
    pub(super) state: Arc<tokio::sync::RwLock<polymarket_bot::btc::RealtimeState>>,
    pub(super) books: Arc<tokio::sync::RwLock<BookRegistry>>,
    pub(super) runtime: Option<BtcRuntimeHandle>,
}

#[derive(Debug, Clone)]
pub(super) struct PendingBtcTerminal {
    pub(super) process_id: uuid::Uuid,
    pub(super) run_id: uuid::Uuid,
    pub(super) run_key: String,
    pub(super) config_hash: String,
    pub(super) terminal_status: String,
    pub(super) terminal_reason: String,
    pub(super) allow_inactive_process: bool,
}

#[derive(Clone)]
pub(super) struct BtcProcessManager {
    pub(super) store: Store,
    pub(super) pool: PgPool,
    pub(super) repository: BtcRepository,
    pub(super) config: BtcProcessManagerConfig,
    pub(super) shutting_down: Arc<AtomicBool>,
    pub(super) transition: Arc<tokio::sync::Mutex<()>>,
    pub(super) active_playbooks: Arc<tokio::sync::Mutex<HashMap<uuid::Uuid, ActiveBtcPlaybook>>>,
    pub(super) shared_runtime: Arc<tokio::sync::Mutex<Option<SharedBtcRuntime>>>,
    pub(super) shared_runtime_startup: Arc<tokio::sync::Mutex<()>>,
    pub(super) terminal_pending: Arc<tokio::sync::Mutex<HashMap<uuid::Uuid, PendingBtcTerminal>>>,
}

impl BtcProcessManager {
    pub(super) fn new(
        store: Store,
        pool: PgPool,
        repository: BtcRepository,
        config: BtcProcessManagerConfig,
    ) -> Self {
        Self {
            store,
            pool,
            repository,
            config,
            shutting_down: Arc::new(AtomicBool::new(false)),
            transition: Arc::new(tokio::sync::Mutex::new(())),
            active_playbooks: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            shared_runtime: Arc::new(tokio::sync::Mutex::new(None)),
            shared_runtime_startup: Arc::new(tokio::sync::Mutex::new(())),
            terminal_pending: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        }
    }

    pub(super) fn execution_components(
        &self,
        execution_mode: BtcExecutionMode,
        execution: &EffectiveProcessExecutionConfig,
        process_id: uuid::Uuid,
        books: Arc<tokio::sync::RwLock<BookRegistry>>,
        paper_venue_config: PaperVenueConfig,
        strategy: &BtcStrategyConfig,
    ) -> Result<BtcExecutionComponents> {
        let max_directional_feature_age = strategy
            .effective_max_directional_feature_age_ms()?
            .map(chrono::Duration::milliseconds);
        match execution_mode {
            BtcExecutionMode::Paper => {
                let paper_venue = Arc::new(
                    BtcPaperVenue::new_with_reference_execution_guard_and_controls(
                        books,
                        paper_venue_config,
                        strategy.max_depth_participation,
                        execution.clone(),
                        process_id,
                        chrono::Duration::milliseconds(strategy.max_reference_age_ms),
                        max_directional_feature_age,
                    )?,
                );
                let venue: Arc<dyn ExecutionVenue> = paper_venue.clone();
                let lifecycle: Arc<dyn BtcExecutionLifecycle> =
                    Arc::new(PaperExecutionLifecycle::new(paper_venue));
                Ok(BtcExecutionComponents {
                    venue,
                    lifecycle,
                    live_venue: None,
                })
            }
            BtcExecutionMode::Live => {
                execution
                    .account_ref
                    .as_deref()
                    .context("live BTC execution is missing account_ref")?;
                let global = self
                    .config
                    .live_venue
                    .as_ref()
                    .context("live BTC execution credentials are not configured")?;
                let live_venue = Arc::new(global.bind_process(process_id, execution)?);
                let delegate: Arc<dyn ExecutionVenue> = live_venue.clone();
                let venue: Arc<dyn ExecutionVenue> = Arc::new(BtcLiveExecutionAdapter::new(
                    delegate,
                    books,
                    process_id,
                    chrono::Duration::milliseconds(strategy.max_reference_age_ms),
                    max_directional_feature_age,
                    chrono::Duration::milliseconds(strategy.max_book_age_ms),
                    strategy.max_depth_participation,
                    execution.require_exit_book.unwrap_or(false),
                )?);
                let lifecycle: Arc<dyn BtcExecutionLifecycle> =
                    Arc::new(LiveExecutionLifecycle::new(
                        venue.clone(),
                        self.config.live_reconcile_interval,
                    )?);
                Ok(BtcExecutionComponents {
                    venue,
                    lifecycle,
                    live_venue: Some(live_venue),
                })
            }
        }
    }
}
