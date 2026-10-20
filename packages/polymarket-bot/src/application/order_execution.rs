//! Rail-selected execution reuses the process lifecycle, shared feeds, and order service.
use super::*;
use polymarket_bot::{
    btc::{
        BtcDecision, BtcFeatureSnapshot, BtcProcessConfig, BtcProcessRunner, QualifiedExecution,
        QualifiedExecutionOutcome, QualifiedExecutionReceipt, QualifiedExecutionRejection,
        QualifiedExecutionResolution,
    },
    execution::{
        orders::{EvaluationRequest, ExecuteRequest, OrderApplication, Receipt},
        polymarket::{
            configuration::{self, CompiledDefinition, ExecutionMode},
            lifecycle::Resolution,
            oms::{Candidate, Ownership},
            service::ExecutionService,
        },
    },
};
use serde_json::json;
use uuid::Uuid;

pub(super) type OrderRuntime = (
    BtcPlaybookRuntimeHandle,
    Option<Arc<LiveVenue>>,
    Option<Arc<ExecutionService>>,
);

struct PolymarketQualifiedExecution {
    service: Arc<ExecutionService>,
}

#[async_trait]
impl QualifiedExecution for PolymarketQualifiedExecution {
    async fn execute(
        &self,
        candidate_id: Uuid,
        member: String,
        snapshot: BtcFeatureSnapshot,
        decision: BtcDecision,
    ) -> Result<QualifiedExecutionOutcome> {
        let candidate = Candidate::qualified(candidate_id, member, snapshot, decision)?;
        match self.service.execute(candidate).await {
            Ok(receipt) => Ok(QualifiedExecutionOutcome::Receipt(
                QualifiedExecutionReceipt {
                    order: receipt.order,
                    resolution: match receipt.resolution {
                        Resolution::Pending => QualifiedExecutionResolution::Pending,
                        Resolution::Executed => QualifiedExecutionResolution::Executed,
                        Resolution::DefinitiveNonexecution => {
                            QualifiedExecutionResolution::DefinitiveNonexecution
                        }
                    },
                    replayed: receipt.replayed,
                },
            )),
            Err(error) => match error
                .downcast_ref::<polymarket_bot::execution::polymarket::AdmissionRejection>()
            {
                Some(rejection) => Ok(QualifiedExecutionOutcome::Rejected(
                    QualifiedExecutionRejection {
                        reason: rejection.reason.clone(),
                        policy_results: rejection.policy_results.clone(),
                    },
                )),
                None => Err(error),
            },
        }
    }

    async fn reconcile(&self) -> Result<()> {
        self.service.reconcile().await
    }
}

pub(super) fn prepare(
    process: &TradingProcess,
    resume: bool,
) -> Result<PreparedBtcStartDefinition, HttpError> {
    if resume {
        validate_btc_process_capability(process)?;
    } else {
        validate_btc_start_eligibility(process)?;
    }
    let definition = configuration::compile(&process.config)
        .map_err(|_| HttpError::bad_request("invalid order configuration"))?;
    if !definition.execution.execute_signals {
        return Err(HttpError::conflict("order execution is not authorized"));
    }
    let p = &definition.execution.policies;
    let strategy = definition
        .runtime_strategy()
        .map_err(|_| HttpError::bad_request("invalid order configuration"))?;
    let execution_mode = if definition.execution.mode == ExecutionMode::Paper {
        BtcExecutionMode::Paper
    } else {
        BtcExecutionMode::Live
    };
    let run_key = definition.control.next_experiment_key.clone();
    let namespace = match execution_mode {
        BtcExecutionMode::Paper => "polymarket-bot/btc-paper",
        BtcExecutionMode::Live => "polymarket-bot/btc-live",
    };
    Ok(PreparedBtcStartDefinition {
        run_id: Uuid::new_v5(
            &Uuid::NAMESPACE_URL,
            format!("{namespace}/{run_key}").as_bytes(),
        ),
        run_key,
        preregistration_sha256: definition.control.preregistration_sha256.clone(),
        runtime: BtcRuntimeConfig {
            enabled: true,
            strategy_interval: Duration::from_millis(
                definition.control.runtime.strategy_interval_ms,
            ),
            max_book_age: Duration::from_millis(p.evidence.max_book_age_ms as u64),
            max_reference_age: Duration::from_millis(p.evidence.max_reference_age_ms as u64),
        },
        paper_venue: PaperVenueConfig {
            arrival_latency: Duration::from_millis(
                p.paper_execution
                    .as_ref()
                    .map_or(1, |p| p.arrival_latency_ms),
            ),
            visible_depth_haircut: p
                .paper_execution
                .as_ref()
                .map_or(Decimal::ONE, |p| p.visible_depth_haircut),
            max_book_age: chrono::Duration::milliseconds(p.evidence.max_book_age_ms),
            starting_collateral_usd: p
                .paper_execution
                .as_ref()
                .map_or(Decimal::ONE, |p| p.starting_collateral_usd),
        },
        execution: effective(&definition),
        execution_mode,
        strategy,
        sources: definition.sources.clone(),
        entry_admission: None,
        risk_strategies: definition.control.risk_strategies.clone(),
        directional_model_entry_policy: p.edge.directional_model_entry_policy,
        paper_stress_previews: Vec::new(),
        frozen_process_config: process.config.clone(),
        config_hash: definition.config_sha256,
    })
}
pub(super) fn effective(definition: &CompiledDefinition) -> EffectiveProcessExecutionConfig {
    let e = &definition.execution;
    let p = &e.policies;
    EffectiveProcessExecutionConfig {
        mode: e.mode.as_str().into(),
        execute_signals: e.execute_signals,
        live_capital: e.live_capital,
        account_ref: e.account_ref.clone(),
        max_open_notional_usd: p.exposure.max_open_notional_usd,
        max_open_positions: p.exposure.max_open_positions,
        max_daily_loss_usd: p.loss_limit.max_daily_loss_usd,
        require_exit_book: Some(p.liquidity.require_exit_book),
        ..Default::default()
    }
}
impl BtcProcessManager {
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn start_order_runtime(
        &self,
        process: &TradingProcess,
        run_id: Uuid,
        run_key: &str,
        hash: &str,
        frozen: &serde_json::Value,
        config: BtcRuntimeConfig,
        state: Arc<tokio::sync::RwLock<polymarket_bot::btc::RealtimeState>>,
        books: Arc<tokio::sync::RwLock<BookRegistry>>,
        resume: bool,
    ) -> Result<OrderRuntime> {
        let definition = configuration::compile(&process.config)?;
        anyhow::ensure!(
            definition.matches_config_sha256(hash),
            "order run identity mismatch"
        );
        let owner = Ownership {
            process_id: process.process_id,
            run_id,
        };
        let strategy = definition.strategy();
        let runtime_strategy = definition.runtime_strategy()?;
        let execution_mode = match definition.execution.mode {
            ExecutionMode::Paper => BtcExecutionMode::Paper,
            ExecutionMode::Live => BtcExecutionMode::Live,
        };
        let policies = &definition.execution.policies;
        let components = self.execution_components(
            execution_mode,
            &effective(&definition),
            (process.process_id, process.created_at),
            books.clone(),
            PaperVenueConfig {
                arrival_latency: Duration::from_millis(
                    policies
                        .paper_execution
                        .as_ref()
                        .map_or(1, |policy| policy.arrival_latency_ms),
                ),
                visible_depth_haircut: policies
                    .paper_execution
                    .as_ref()
                    .map_or(Decimal::ONE, |policy| policy.visible_depth_haircut),
                max_book_age: chrono::Duration::milliseconds(policies.evidence.max_book_age_ms),
                starting_collateral_usd: policies
                    .paper_execution
                    .as_ref()
                    .map_or(Decimal::ONE, |policy| policy.starting_collateral_usd),
            },
            &runtime_strategy,
        )?;
        let live = components.live_venue.clone();
        let service = match definition.execution.mode {
            ExecutionMode::Paper => ExecutionService::paper(
                definition.clone(),
                owner,
                hash.to_string(),
                self.store.clone(),
                self.repository.clone(),
                books.clone(),
            )?,
            ExecutionMode::Live => ExecutionService::live(
                definition.clone(),
                owner,
                hash.to_string(),
                self.store.clone(),
                self.repository.clone(),
                books.clone(),
                live.clone().context("live credentials unavailable")?,
            )?,
        };
        let service = Arc::new(service);
        let observer = Arc::new(
            BtcProcessRunner::new_with_execution(
                self.repository.clone(),
                self.store.clone(),
                components.venue,
                books.clone(),
                components.lifecycle,
                BtcProcessConfig {
                    process_id: process.process_id,
                    run_id,
                    run_key: run_key.to_string(),
                    config_hash: hash.to_string(),
                    frozen_process_config: frozen.clone(),
                    strategy,
                    entry_admission: None,
                    risk_strategies: definition.control.risk_strategies.clone(),
                    directional_model_entry_policy: policies.edge.directional_model_entry_policy,
                    execution_enabled: true,
                    paper_stress_previews: Vec::new(),
                },
            )?
            .with_primary_persistence_state(state.clone())
            .with_qualified_execution(Arc::new(PolymarketQualifiedExecution {
                service: service.clone(),
            })),
        );
        if resume {
            observer.resume().await?;
        } else {
            observer.initialize().await?;
        }
        Ok((
            BtcPlaybookRuntimeHandle::start(config, observer, state, books)?,
            live,
            Some(service),
        ))
    }
    async fn order_context(
        &self,
        id: Uuid,
    ) -> Result<
        (
            Arc<ExecutionService>,
            polymarket_bot::btc::types::BtcIntervalMarket,
        ),
        HttpError,
    > {
        let process = self
            .store
            .get_trading_process(id)
            .await
            .map_err(|error| dependency(id, error))?
            .ok_or_else(|| HttpError::not_found("trading process not found"))?;
        if !process.enabled || process.status != "running" {
            return Err(HttpError::conflict("process is not running and enabled"));
        }
        let definition = configuration::compile(&process.config)
            .map_err(|_| HttpError::bad_request("invalid order configuration"))?;
        let active_guard = self.active_playbooks.lock().await;
        let active = active_guard
            .get(&id)
            .ok_or_else(|| HttpError::unavailable("process runtime is recovering"))?;
        if !definition.matches_config_sha256(&active.config_hash) {
            return Err(HttpError::conflict(
                "process configuration differs from its active run",
            ));
        }
        let service = active
            .order_service
            .clone()
            .ok_or_else(|| HttpError::not_implemented("order service unavailable"))?;
        let state = active.runtime.status_inputs().0;
        drop(active_guard);
        let state = state.read().await;
        if !state.primary_persistence_available() {
            return Err(HttpError::unavailable(
                "market persistence evidence unavailable",
            ));
        }
        let market = state
            .current_market
            .clone()
            .ok_or_else(|| HttpError::unavailable("current market evidence unavailable"))?;
        Ok((service, market))
    }
}
fn dependency(process_id: Uuid, error: anyhow::Error) -> HttpError {
    warn!(event="order_application_dependency_unavailable",process_id=%process_id,error=%error,"Order action will require fresh evidence");
    HttpError::unavailable("order evidence or execution dependency unavailable")
}
async fn receipt(
    store: &Store,
    process_id: Uuid,
    order: polymarket_bot::models::OrderRecord,
) -> Result<Receipt, HttpError> {
    if order.request.process_id != Some(process_id) {
        return Err(HttpError::not_found("order not found"));
    }
    let metadata = &order.request.metadata;
    let contract = metadata
        .get("polymarket_execution")
        .ok_or_else(|| HttpError::not_found("order not owned by this execution contract"))?;
    let accounting = store
        .execution_order_evidence(process_id, &order.order_id)
        .await
        .map_err(|error| dependency(process_id, error))?;
    let fills = store
        .execution_fills(&order)
        .await
        .map_err(|error| dependency(process_id, error))?;
    let fills=fills.into_iter().map(|f|json!({"fill_id":f.fill_id,"price":f.price,"size":f.size,"fee":f.fee,"filled_at":f.filled_at})).collect::<Vec<_>>();
    Ok(Receipt {
        version: 1,
        process_id,
        operation_id: order.request.client_order_id,
        order_id: Some(order.order_id),
        origin: contract["origin"].as_str().unwrap_or("unknown").into(),
        config_sha256: contract["config_sha256"].as_str().unwrap_or("").into(),
        status: serde_json::to_value(order.state)
            .unwrap_or_default()
            .as_str()
            .unwrap_or("unknown")
            .into(),
        policy_results: contract
            .get("policy_results")
            .cloned()
            .unwrap_or_else(|| json!([])),
        evidence: json!({"fills":fills,"execution":contract,"updated_at":order.updated_at,"durable":true,"accounting":accounting,"paper_execution":metadata.get("paper_execution"),"reject_reason":metadata.get("reject_reason")}),
        preparation: None,
    })
}
#[async_trait]
impl OrderApplication for BtcProcessManager {
    async fn preview(
        &self,
        id: Uuid,
        request: EvaluationRequest,
        prepare: bool,
    ) -> Result<Receipt, HttpError> {
        request.validate()?;
        let (service, market) = self.order_context(id).await?;
        if !service.accepts_api_member(&request.candidate.member_id) {
            return Err(HttpError::bad_request("unknown configured member"));
        }
        if market.market_id != request.candidate.market_id {
            return Err(HttpError::unavailable(
                "requested market is not in the current evidence snapshot",
            ));
        }
        let candidate = service
            .api_candidate(Uuid::new_v4(), &request, &market)
            .await
            .map_err(|error| dependency(id, error))?;
        service
            .preview_api(&candidate, prepare)
            .await
            .map_err(|error| dependency(id, error))
    }
    async fn execute(&self, id: Uuid, request: ExecuteRequest) -> Result<Receipt, HttpError> {
        let evaluation = EvaluationRequest {
            version: request.version,
            candidate: request.candidate,
        };
        evaluation.validate()?;
        if request.idempotency_key.is_nil() {
            return Err(HttpError::bad_request("idempotency_key must be non-nil"));
        }
        // Replay precedes current configuration compilation; historical ownership never changes.
        if let Some(order) = self
            .store
            .find_order_by_client_order_id(request.idempotency_key)
            .await
            .map_err(|error| dependency(id, error))?
        {
            polymarket_bot::execution::orders::validate_replay(&order, id, &evaluation)?;
            info!(event="order_api_replay",process_id=%id,client_order_id=%request.idempotency_key,execution_mode=order.request.metadata.pointer("/polymarket_execution/mode").and_then(|v|v.as_str()).unwrap_or("unknown"),origin="api","Returning original durable order without submission");
            return receipt(&self.store, id, order).await;
        }
        let (service, market) = self.order_context(id).await?;
        if !service.accepts_api_member(&evaluation.candidate.member_id) {
            return Err(HttpError::bad_request("unknown configured member"));
        }
        if market.market_id != evaluation.candidate.market_id {
            return Err(HttpError::unavailable(
                "requested market is not in the current evidence snapshot",
            ));
        }
        let candidate = service
            .api_candidate(request.idempotency_key, &evaluation, &market)
            .await
            .map_err(|error| dependency(id, error))?;
        let result = service.execute(candidate).await.map_err(|error| {
            if matches!(
                error.to_string().as_str(),
                "idempotency identity conflict" | "opportunity_has_execution_or_unresolved_order"
            ) {
                HttpError::conflict("idempotency identity conflict")
            } else {
                dependency(id, error)
            }
        })?;
        receipt(&self.store, id, result.order).await
    }
    async fn order(&self, id: Uuid, order_id: String) -> Result<Receipt, HttpError> {
        let order = self
            .store
            .execution_order(id, &order_id)
            .await
            .map_err(|error| dependency(id, error))?
            .ok_or_else(|| HttpError::not_found("order not found"))?;
        receipt(&self.store, id, order).await
    }
}

impl BtcProcessManager {
    pub(super) async fn recover_order_processes(&self) {
        if self.shutting_down.load(Ordering::Acquire) {
            return;
        }
        let active_ids = self
            .active_playbooks
            .lock()
            .await
            .keys()
            .copied()
            .collect::<Vec<_>>();
        let ids=sqlx::query_scalar::<_,Uuid>("SELECT process_id FROM polymarket.trading_processes WHERE enabled AND status IN ('starting','running') AND NOT(process_id=ANY($1)) AND config #>> '{actions,0,rail}'='polymarket' ORDER BY created_at LIMIT 64").bind(active_ids).fetch_all(&self.pool).await;
        let Ok(ids) = ids else { return };
        for id in ids {
            let _transition = self.transition.lock().await;
            if self.shutting_down.load(Ordering::Acquire) {
                return;
            }
            if self.active_playbooks.lock().await.contains_key(&id) {
                continue;
            }
            if let Err(error) = self.resume_durable_process_locked(id).await {
                warn!(process_id=%id,error=?error,"Order process recovery deferred");
            }
        }
    }
}
