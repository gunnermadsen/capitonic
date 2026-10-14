//! One callable execution entry for model and synthetic candidates. HTTP and UMR routing are
//! integrated separately; neither has an alternative submission path through this service.
use super::{
    configuration::{CompiledDefinition, ExecutionMode},
    lifecycle::{self, Resolution},
    oms::{candidate_identity, Accounting, Candidate, Oms, Ownership},
};
use crate::btc::execution_freshness::{
    LatencyOutcome, OrderLatencyStage, OrderOutcome, OrderStage,
};
use crate::{
    btc::{
        admission::LossRegimeConfidenceFloorState, BookRegistry, BtcRepository, PaperVenue,
        PaperVenueConfig,
    },
    execution::{ExecutionVenue, LiveExecutionGateReason, LivePrePostGuard},
    models::{OrderRecord, OrderRequest},
    store::Store,
};
use anyhow::{ensure, Context, Result};
use async_trait::async_trait;
use chrono::Utc;
use rust_decimal::Decimal;
use std::{sync::Arc, time::Duration};
use tokio::sync::RwLock;

mod api;

pub struct ExecutionService {
    oms: Arc<Oms>,
    store: Store,
    repository: BtcRepository,
    books: Arc<RwLock<BookRegistry>>,
    adapter: Adapter,
    resumed: tokio::sync::OnceCell<()>,
    live_lifecycle: tokio::sync::OnceCell<crate::btc::LiveExecutionLifecycle>,
    freshness: Arc<crate::btc::execution_freshness::ExecutionFreshnessMetrics>,
}
enum Adapter {
    Paper(Box<PaperVenue>),
    Live(Arc<crate::execution::live::LiveVenue>),
}
#[derive(Debug, Clone)]
pub struct ExecutionReceipt {
    pub order: OrderRecord,
    pub resolution: Resolution,
    pub replayed: bool,
}
impl ExecutionService {
    pub fn paper(
        definition: CompiledDefinition,
        owner: Ownership,
        store: Store,
        repository: BtcRepository,
        books: Arc<RwLock<BookRegistry>>,
    ) -> Result<Self> {
        ensure!(
            definition.execution.mode == ExecutionMode::Paper,
            "paper adapter requires paper settings"
        );
        let p = &definition.execution.policies;
        let paper = p.paper_execution.as_ref().context("missing paper policy")?;
        let freshness = crate::btc::execution_freshness::ExecutionFreshnessMetrics::new(
            owner.process_id,
            "paper",
            p.evidence.max_book_age_ms,
        );
        freshness.expect_order_recovery();
        let venue = PaperVenue::for_order_management(
            books.clone(),
            PaperVenueConfig {
                arrival_latency: Duration::from_millis(paper.arrival_latency_ms),
                visible_depth_haircut: paper.visible_depth_haircut,
                max_book_age: chrono::Duration::milliseconds(p.evidence.max_book_age_ms),
                starting_collateral_usd: paper.starting_collateral_usd,
            },
            p.sizing.max_depth_participation,
        )?;
        Ok(Self {
            oms: Arc::new(Oms::new(definition, owner)?),
            store,
            repository,
            books,
            adapter: Adapter::Paper(Box::new(venue.with_order_metrics(freshness.clone()))),
            freshness,
            resumed: tokio::sync::OnceCell::new(),
            live_lifecycle: tokio::sync::OnceCell::new(),
        })
    }
    /// The caller supplies the existing process-bound LiveVenue, preserving account identity,
    /// signing, durable pending-order claims, HTTP/websocket reconciliation and transport checks.
    pub fn live(
        definition: CompiledDefinition,
        owner: Ownership,
        store: Store,
        repository: BtcRepository,
        books: Arc<RwLock<BookRegistry>>,
        venue: Arc<crate::execution::live::LiveVenue>,
    ) -> Result<Self> {
        ensure!(
            definition.execution.mode == ExecutionMode::Live,
            "live adapter requires live settings"
        );
        ensure!(
            venue.bound_process_id() == Some(owner.process_id)
                && venue.bound_account_ref() == definition.execution.account_ref.as_deref(),
            "live venue ownership mismatch"
        );
        let freshness = crate::btc::execution_freshness::ExecutionFreshnessMetrics::new(
            owner.process_id,
            "live",
            definition.execution.policies.evidence.max_book_age_ms,
        );
        freshness.expect_order_recovery();
        Ok(Self {
            oms: Arc::new(Oms::new(definition, owner)?),
            store,
            repository,
            books,
            adapter: Adapter::Live(venue),
            freshness,
            resumed: tokio::sync::OnceCell::new(),
            live_lifecycle: tokio::sync::OnceCell::new(),
        })
    }
    /// Called on startup and periodically by the owning application, including after transient
    /// failures. It never changes enabled/status or assigns outstanding orders to a new run.
    pub async fn reconcile(&self) -> Result<()> {
        let result = self.reconcile_inner().await;
        if result.is_err() {
            self.freshness.order_recovery(false, None);
        }
        result
    }
    async fn reconcile_inner(&self) -> Result<()> {
        use crate::btc::execution_lifecycle::BtcExecutionLifecycle;
        self.restore_configured_authorization().await?;
        let reconciled = match &self.adapter {
            Adapter::Paper(_) => {
                crate::btc::execution_lifecycle::reconcile_paper_orders(
                    None,
                    &self.repository,
                    self.oms.owner.process_id,
                    self.oms.owner.run_id,
                    &self.oms.definition.config_sha256,
                )
                .await?;
                true
            }
            Adapter::Live(venue) => {
                // Hydration belongs to startup, not each maintenance pass. Failed hydration
                // leaves the cell empty so recovery retries without disabling the process.
                let lifecycle = self
                    .live_lifecycle
                    .get_or_try_init(|| async {
                        let lifecycle = crate::btc::LiveExecutionLifecycle::new(
                            venue.clone(),
                            Duration::from_secs(5),
                        )?;
                        lifecycle
                            .resume_run(
                                &self.repository,
                                self.oms.owner.process_id,
                                self.oms.owner.run_id,
                            )
                            .await?;
                        Ok::<_, anyhow::Error>(lifecycle)
                    })
                    .await?;
                lifecycle
                    .reconcile_run(
                        &self.repository,
                        self.oms.owner.process_id,
                        self.oms.owner.run_id,
                        &self.oms.definition.config_sha256,
                    )
                    .await?;
                let status = venue.live_status().await?;
                status.idempotency_clean
                    && status.process_accounting_proven
                    && status.unresolved_live_order_count == 0
            }
        };
        let reserved = self
            .store
            .execution_reserved_orders(self.oms.owner.process_id)
            .await?;
        let mut unresolved = 0;
        for order in reserved {
            if order.request.metadata.get("polymarket_execution").is_some()
                && self.observe(order, true, reconciled).await?.resolution == Resolution::Pending
            {
                unresolved += 1;
            }
        }
        self.freshness.order_recovery(reconciled, Some(unresolved));
        Ok(())
    }

    /// Recovery owns no durable enable switch. A failed pass is retried on the next interval.
    pub async fn run_recovery(self: Arc<Self>, cancel: tokio_util::sync::CancellationToken) {
        let mut interval = tokio::time::interval(Duration::from_secs(5));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _=cancel.cancelled()=>break,
                _=interval.tick()=>if let Err(error)=self.reconcile().await {
                    tracing::warn!(event="order_recovery_deferred",process_id=%self.oms.owner.process_id,execution_mode=self.oms.definition.execution.mode.as_str(),error=%error,"Order recovery will retry automatically");
                }
            }
        }
    }
    async fn restore_configured_authorization(&self) -> Result<()> {
        if let Adapter::Live(venue) = &self.adapter {
            let process = self
                .store
                .get_trading_process(self.oms.owner.process_id)
                .await?
                .context("unknown process")?;
            if process.enabled
                && matches!(process.status.as_str(), "starting" | "running")
                && super::configuration::compile(&process.config)?.config_sha256
                    == self.oms.definition.config_sha256
            {
                self.resumed
                    .get_or_try_init(|| async {
                        venue.restore_configured_entries_after_restart().await
                    })
                    .await?;
            }
        }
        Ok(())
    }

    pub async fn execute(&self, candidate: Candidate) -> Result<ExecutionReceipt> {
        let origin = candidate.origin();
        self.freshness
            .order_event(origin, OrderStage::Candidate, OrderOutcome::Accepted);
        let result = self
            .freshness
            .time_order_outcome(
                origin,
                OrderLatencyStage::Execution,
                self.execute_inner(candidate),
                |receipt| {
                    if receipt.order.state == crate::models::OrderState::Rejected {
                        LatencyOutcome::Rejected
                    } else {
                        LatencyOutcome::Completed
                    }
                },
            )
            .await;
        if let Err(error) = &result {
            self.freshness.order_event(
                origin,
                if error.is::<super::AdmissionRejection>() {
                    OrderStage::Admission
                } else {
                    OrderStage::Submission
                },
                if error.is::<super::AdmissionRejection>() {
                    OrderOutcome::Rejected
                } else {
                    OrderOutcome::Error
                },
            );
        }
        result
    }
    async fn execute_inner(&self, candidate: Candidate) -> Result<ExecutionReceipt> {
        let account = self
            .oms
            .definition
            .execution
            .account_ref
            .clone()
            .unwrap_or_else(|| format!("paper:{}", self.oms.owner.process_id));
        let lease = self
            .freshness
            .time_order(
                candidate.origin(),
                OrderLatencyStage::ReservationWait,
                self.store.execution_lease(&account.to_ascii_lowercase()),
            )
            .await?;
        if let Some(existing) = self
            .store
            .find_order_by_client_order_id(candidate.id)
            .await?
        {
            let payload_matches = if let Some(hash) = &candidate.api_request_hash {
                existing
                    .request
                    .metadata
                    .pointer("/polymarket_execution/api_request_sha256")
                    .and_then(|v| v.as_str())
                    == Some(hash.as_str())
            } else {
                existing
                    .request
                    .metadata
                    .pointer("/polymarket_execution/candidate_sha256")
                    .and_then(|v| v.as_str())
                    == Some(candidate_identity(&candidate)?.as_str())
            };
            ensure!(
                existing.request.process_id == Some(self.oms.owner.process_id)
                    && payload_matches
                    && (candidate.api_request_hash.is_some()
                        || existing
                            .request
                            .metadata
                            .pointer("/polymarket_execution/config_sha256")
                            .and_then(|v| v.as_str())
                            == Some(self.oms.definition.config_sha256.as_str())),
                "idempotency identity conflict"
            );
            self.freshness.order_event(
                candidate.origin(),
                OrderStage::Replay,
                OrderOutcome::Accepted,
            );
            lease.release().await?;
            return self.recover(existing, true).await;
        }
        let process = self
            .store
            .get_trading_process(self.oms.owner.process_id)
            .await?
            .context("unknown process")?;
        ensure!(
            process.enabled && matches!(process.status.as_str(), "running" | "starting"),
            "process_not_authorized"
        );
        ensure!(
            super::configuration::compile(&process.config)?.config_sha256
                == self.oms.definition.config_sha256,
            "process_configuration_changed"
        );
        ensure!(
            self.repository
                .run_config_hash(self.oms.owner.process_id, self.oms.owner.run_id)
                .await?
                == self.oms.definition.config_sha256,
            "run_configuration_identity_mismatch"
        );
        self.restore_configured_authorization().await?;
        let live: Option<Arc<dyn ExecutionVenue>> = match &self.adapter {
            Adapter::Live(venue) => Some(venue.clone()),
            Adapter::Paper(_) => None,
        };
        ensure!(
            !self
                .store
                .execution_opportunity_occupied(
                    self.oms.owner.process_id,
                    &candidate.snapshot.market_id
                )
                .await?,
            "opportunity_has_execution_or_unresolved_order"
        );
        let accounting = self
            .freshness
            .time_order(
                candidate.origin(),
                OrderLatencyStage::Accounting,
                accounting(
                    &self.oms,
                    &self.store,
                    &self.repository,
                    live.as_deref(),
                    None,
                ),
            )
            .await?;
        self.freshness.attempt();
        let intent = self
            .freshness
            .time_order(candidate.origin(), OrderLatencyStage::Admission, async {
                let books = self.books.read().await;
                let now = Utc::now();
                self.oms.admit(
                    &candidate,
                    [
                        books
                            .checkpoint(&candidate.snapshot.up_book.token_id)
                            .as_ref(),
                        books
                            .checkpoint(&candidate.snapshot.down_book.token_id)
                            .as_ref(),
                    ],
                    &accounting,
                    now,
                )
            })
            .await;
        let intent = match intent {
            Ok(intent) => intent,
            Err(error) => {
                if candidate.api_request_hash.is_some() {
                    if let Some(rejection) = error.downcast_ref::<super::AdmissionRejection>() {
                        if !rejection.evidence_unavailable() {
                            let order = self.rejected_api_order(&candidate, rejection)?;
                            self.freshness
                                .time_order(
                                    candidate.origin(),
                                    OrderLatencyStage::Persistence,
                                    self.store.insert_order(&order),
                                )
                                .await?;
                            self.freshness.order_event(
                                candidate.origin(),
                                OrderStage::Admission,
                                OrderOutcome::Rejected,
                            );
                            lease.release().await?;
                            return self.observe(order, false, true).await;
                        }
                    }
                }
                return Err(error);
            }
        };
        self.freshness
            .order_event(intent.origin, OrderStage::Admission, OrderOutcome::Accepted);
        tracing::info!(event="order_intent_admitted",process_id=%self.oms.owner.process_id,client_order_id=%candidate.id,
            execution_mode=self.oms.definition.execution.mode.as_str(),origin=intent.origin.as_str(),config_hash=%self.oms.definition.config_sha256,"Polymarket OMS admitted order");
        let guard = Arc::new(SubmissionGuard {
            oms: self.oms.clone(),
            store: self.store.clone(),
            repository: self.repository.clone(),
            books: self.books.clone(),
            candidate,
            live,
            freshness: self.freshness.clone(),
            final_book: std::sync::Mutex::new(None),
        });
        self.freshness.order_event(
            intent.origin,
            OrderStage::Submission,
            OrderOutcome::Accepted,
        );
        let origin = intent.origin;
        let order = self
            .freshness
            .time_order_outcome(
                origin,
                OrderLatencyStage::Adapter,
                async {
                    Ok(match &self.adapter {
                        Adapter::Paper(venue) => {
                            venue
                                .execute_durable(
                                    intent.request,
                                    accounting.available,
                                    Box::new(lease),
                                    guard,
                                )
                                .await?
                        }
                        Adapter::Live(venue) => {
                            let result = venue
                                .submit_order_with_pre_post_guard(
                                    intent.request.clone(),
                                    Some(guard),
                                )
                                .await;
                            // A returned pre-POST rejection may not yet have a row. Preserve its idempotency.
                            if let Ok(order) = &result {
                                if self
                                    .store
                                    .find_order_by_client_order_id(order.request.client_order_id)
                                    .await?
                                    .is_none()
                                {
                                    self.freshness
                                        .time_order(
                                            origin,
                                            OrderLatencyStage::Persistence,
                                            self.store.insert_order(order),
                                        )
                                        .await?;
                                }
                            }
                            lease.release().await?;
                            result?
                        }
                    })
                },
                |order| {
                    if order.state == crate::models::OrderState::Rejected {
                        LatencyOutcome::Rejected
                    } else {
                        LatencyOutcome::Completed
                    }
                },
            )
            .await?;
        self.recover(order, false).await
    }
    async fn recover(&self, mut order: OrderRecord, replayed: bool) -> Result<ExecutionReceipt> {
        if order.state == crate::models::OrderState::Rejected {
            return self.observe(order, replayed, true).await;
        }
        let reconciled = match &self.adapter {
            Adapter::Paper(_) => true,
            Adapter::Live(venue) => {
                venue.request_post_order_reconciliation();
                let report = self
                    .freshness
                    .time_order(
                        order_origin(&order),
                        OrderLatencyStage::Reconciliation,
                        venue.reconcile(),
                    )
                    .await?;
                if let Some(durable) = self
                    .store
                    .find_order_by_client_order_id(order.request.client_order_id)
                    .await?
                {
                    order = durable;
                }
                report.balances_checked
                    && report.unresolved_count == 0
                    && report.mismatches_found == 0
            }
        };
        self.observe(order, replayed, reconciled).await
    }

    async fn observe(
        &self,
        order: OrderRecord,
        replayed: bool,
        reconciled: bool,
    ) -> Result<ExecutionReceipt> {
        self.freshness.time_order(order_origin(&order), OrderLatencyStage::Observation, async {
        let fills = self.store.execution_fills(&order).await?;
        let resolution = lifecycle::resolution(&order, &fills, reconciled)?;
        if resolution != Resolution::Pending {
            self.freshness.time_order(order_origin(&order), OrderLatencyStage::Persistence, self.store
                .confirm_execution_resolution(
                    &order,
                    match resolution {
                        Resolution::Executed => "executed",
                        Resolution::DefinitiveNonexecution => "definitive_nonexecution",
                        Resolution::Pending => unreachable!(),
                    },
                )).await?;
        }
        let origin = if order
            .request
            .metadata
            .pointer("/polymarket_execution/origin")
            .and_then(|v| v.as_str())
            == Some("api")
        {
            super::oms::Origin::Api
        } else {
            super::oms::Origin::Umr
        };
        self.freshness.order_event(
            origin,
            OrderStage::Observation,
            match resolution {
                Resolution::Pending => OrderOutcome::Pending,
                Resolution::Executed => OrderOutcome::Executed,
                Resolution::DefinitiveNonexecution => OrderOutcome::Nonexecution,
            },
        );
        tracing::info!(event="order_execution_observed",process_id=%self.oms.owner.process_id,execution_mode=self.oms.definition.execution.mode.as_str(),origin=order.request.metadata.pointer("/polymarket_execution/origin").and_then(|v|v.as_str()).unwrap_or("unknown"),order_id=%order.order_id,?resolution,replayed,"Polymarket order evidence observed");
        Ok(ExecutionReceipt {
            order,
            resolution,
            replayed,
        })

        }).await
    }
}

struct SubmissionGuard {
    oms: Arc<Oms>,
    store: Store,
    repository: BtcRepository,
    books: Arc<RwLock<BookRegistry>>,
    candidate: Candidate,
    live: Option<Arc<dyn ExecutionVenue>>,
    freshness: Arc<crate::btc::execution_freshness::ExecutionFreshnessMetrics>,
    final_book: std::sync::Mutex<Option<crate::btc::execution_freshness::BookEvidence>>,
}
impl crate::execution::live_pre_post_guard_sealed::Sealed for SubmissionGuard {}
#[async_trait]
impl LivePrePostGuard for SubmissionGuard {
    async fn validate_pre_post(
        &self,
        request: &OrderRequest,
    ) -> Result<Option<LiveExecutionGateReason>> {
        self.freshness.time_order_outcome(self.candidate.origin(), OrderLatencyStage::FinalValidation, async {
        let process = self
            .store
            .get_trading_process(self.oms.owner.process_id)
            .await?
            .context("process authority unavailable")?;
        if !process.enabled
            || !matches!(process.status.as_str(), "running" | "starting")
            || super::configuration::compile(&process.config)?.config_sha256
                != self.oms.definition.config_sha256
        {
            return Ok(Some(LiveExecutionGateReason::OrderSubmissionDisabled));
        }
        if self.candidate.decision.is_some() {
            let guard = crate::btc::execution_guard::reference_execution_guard(request)?;
            if guard
                .validate_for_request(
                    request,
                    Utc::now(),
                    self.oms.owner.process_id,
                    chrono::Duration::milliseconds(
                        self.oms
                            .definition
                            .execution
                            .policies
                            .evidence
                            .max_reference_age_ms,
                    ),
                    guard
                        .max_directional_feature_age_ms
                        .map(chrono::Duration::milliseconds),
                )
                .is_err()
            {
                return Ok(Some(LiveExecutionGateReason::ReferenceFreshness));
            }
        }
        let accounting = self.freshness.time_order(self.candidate.origin(), OrderLatencyStage::Accounting, accounting(
            &self.oms,
            &self.store,
            &self.repository,
            self.live.as_deref(),
            Some(request.client_order_id),
        )).await?;
        let fee = crate::fees::sealed_dynamic_fee_rate(&request.metadata)
            .context("missing sealed fee")?;
        let exposure = crate::btc::ProposedEntryExposure::for_buy_cash_budget(
            request.size,
            request.price,
            fee,
        )?;
        self.oms.validate_accounting(
            &request.market_id,
            self.candidate.conservative_probability,
            &accounting,
            &exposure,
            Utc::now(),
        )?;
        let books = self.books.read().await;
        let checkpoint = books.checkpoint(&request.token_id);
        let evidence = checkpoint
            .as_ref()
            .map(crate::btc::execution_freshness::BookEvidence::from);
        self.freshness.check(evidence, Utc::now());
        let check = self.oms.admit(
            &self.candidate,
            [
                books
                    .checkpoint(&self.candidate.snapshot.up_book.token_id)
                    .as_ref(),
                books
                    .checkpoint(&self.candidate.snapshot.down_book.token_id)
                    .as_ref(),
            ],
            &accounting,
            Utc::now(),
        );
        match check {
            Ok(intent)
                if intent.request.size == request.size
                    && intent.request.price <= request.price
                    && crate::fees::sealed_dynamic_fee_rate(&intent.request.metadata).ok()
                        == Some(fee) =>
            {
                *self.final_book.lock().unwrap_or_else(|e| e.into_inner()) = evidence;
                Ok(None)
            }
            Ok(_) => Ok(Some(LiveExecutionGateReason::OrderbookMarketability)),
            Err(error) => {
                self.freshness.reject_book();
                tracing::info!(event="order_pre_submit_rejected",process_id=%self.oms.owner.process_id,execution_mode=self.oms.definition.execution.mode.as_str(),origin=self.candidate.origin().as_str(),reason=%error,"OMS action deferred");
                Ok(Some(LiveExecutionGateReason::OrderbookReadiness))
            }
        }

        }, |reason| if reason.is_some() { LatencyOutcome::Rejected } else { LatencyOutcome::Completed }).await
    }
    fn observe_persistence(&self, elapsed: Duration, success: bool) {
        self.freshness.order_duration(
            self.candidate.origin(),
            OrderLatencyStage::Persistence,
            if success {
                LatencyOutcome::Completed
            } else {
                LatencyOutcome::Error
            },
            elapsed,
        );
    }
    fn observe_post_attempt(&self, request: &OrderRequest) {
        let evidence = *self.final_book.lock().unwrap_or_else(|e| e.into_inner());
        self.freshness
            .execution(evidence, Utc::now(), request.client_order_id);
        tracing::info!(event="order_submission_attempt",process_id=%self.oms.owner.process_id,client_order_id=%request.client_order_id,origin=self.candidate.origin().as_str(),execution_mode="live","Admitted live order POST");
    }
    fn observe_post_result(
        &self,
        request: &OrderRequest,
        latency: Duration,
        outcome: &'static str,
    ) {
        self.freshness.order_duration(
            self.candidate.origin(),
            OrderLatencyStage::Transport,
            match outcome {
                "accepted" | "acknowledged" => LatencyOutcome::Completed,
                "venue_rejected" => LatencyOutcome::Rejected,
                _ => LatencyOutcome::Error,
            },
            latency,
        );
        tracing::info!(event="order_submission_result",process_id=%self.oms.owner.process_id,client_order_id=%request.client_order_id,origin=self.candidate.origin().as_str(),execution_mode="live",elapsed_ms=latency.as_millis(),outcome,"Live order POST result");
    }
}

async fn accounting(
    oms: &Oms,
    store: &Store,
    repository: &BtcRepository,
    live: Option<&dyn ExecutionVenue>,
    ignored: Option<uuid::Uuid>,
) -> Result<Accounting> {
    let now = Utc::now();
    let p = &oms.definition.execution.policies;
    let state = if live.is_none() || p.loss_limit.daily_realized_pnl_high_water_mark.is_some() {
        Some(
            repository
                .load_daily_realized_pnl_high_water_mark_state(oms.owner.process_id, now)
                .await?,
        )
    } else {
        None
    };
    let pending = store
        .execution_reserved_orders(oms.owner.process_id)
        .await?;
    ensure!(
        pending.len() <= 1000,
        "unresolved order evidence exceeds existing bound"
    );
    let mut reserved = Decimal::ZERO;
    for order in &pending {
        if Some(order.request.client_order_id) != ignored {
            reserved +=
                Store::conservative_live_request_exposure(&order.request)?.total_exposure_usd;
        }
    }
    let (open_notional, open_markets, daily_pnl) = if live.is_some() {
        let process = store
            .get_trading_process(oms.owner.process_id)
            .await?
            .context("unknown process")?;
        let exposure = store
            .conservative_live_process_exposure(oms.owner.process_id, process.created_at, ignored)
            .await?;
        let daily_pnl = if p.loss_limit.max_daily_loss_usd.is_some() {
            let start = now
                .date_naive()
                .and_hms_opt(0, 0, 0)
                .context("invalid UTC day")?
                .and_utc();
            store
                .recognized_live_process_net_pnl_for_utc_day(
                    oms.owner.process_id,
                    process.created_at,
                    start,
                    start + chrono::Duration::days(1),
                    now,
                )
                .await?
                .net_pnl
        } else {
            Decimal::ZERO
        };
        let mut markets = exposure.exposed_market_ids;
        for order in &pending {
            if Some(order.request.client_order_id) != ignored
                && !markets.contains(&order.request.market_id)
            {
                markets.push(order.request.market_id.clone());
            }
        }
        // Canonical live replay releases resolved market risk before redemption. Keep the
        // new lifecycle's cancelled-but-unreconciled reservations in addition to actual fills.
        (
            exposure.filled_buy_notional_usd + exposure.filled_buy_fees_usd + reserved,
            markets,
            daily_pnl,
        )
    } else {
        let state = state
            .as_ref()
            .context("paper accounting evidence unavailable")?;
        (
            state.unsettled_entry_debit_usd + reserved,
            store.execution_open_markets(oms.owner.process_id).await?,
            state.daily_realized_pnl_usd,
        )
    };
    let available = if let Some(venue) = live {
        let balance = venue
            .get_balances()
            .await?
            .into_iter()
            .find(|(currency, _)| currency == "USDC")
            .context("collateral evidence unavailable")?
            .1;
        let mut account_reserved = Decimal::ZERO;
        for id in store
            .execution_account_processes(
                oms.owner.process_id,
                oms.definition.execution.account_ref.as_deref(),
            )
            .await?
        {
            let orders = store.execution_reserved_orders(id).await?;
            ensure!(
                orders.len() <= 1000,
                "account order evidence exceeds existing bound"
            );
            for order in orders {
                if order
                    .request
                    .metadata
                    .pointer("/polymarket_execution/account_ref")
                    .and_then(|v| v.as_str())
                    .is_some_and(|saved| {
                        !saved.eq_ignore_ascii_case(
                            oms.definition
                                .execution
                                .account_ref
                                .as_deref()
                                .unwrap_or_default(),
                        )
                    })
                {
                    continue;
                }
                if Some(order.request.client_order_id) != ignored {
                    account_reserved += Store::conservative_live_request_exposure(&order.request)?
                        .total_exposure_usd;
                }
            }
        }
        balance - account_reserved
    } else {
        store
            .paper_capital(
                oms.owner.process_id,
                p.paper_execution
                    .as_ref()
                    .context("missing paper policy")?
                    .starting_collateral_usd,
            )
            .await?
            - reserved
    };
    let loss_regime = if let Some(policy) = &p.loss_regime {
        Some(LossRegimeConfidenceFloorState::from_candidates(
            policy,
            &repository
                .load_resolved_loss_regime_candidates(
                    oms.owner.process_id,
                    &oms.definition.config_sha256,
                    now,
                )
                .await?,
        ))
    } else {
        None
    };
    Ok(Accounting {
        available,
        open_notional,
        open_markets,
        daily_pnl,
        high_water: state,
        loss_regime,
    })
}

fn order_origin(order: &OrderRecord) -> super::oms::Origin {
    if order
        .request
        .metadata
        .pointer("/polymarket_execution/origin")
        .and_then(|v| v.as_str())
        == Some("api")
    {
        super::oms::Origin::Api
    } else {
        super::oms::Origin::Umr
    }
}
