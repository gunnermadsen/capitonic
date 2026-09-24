use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use rust_decimal::{prelude::ToPrimitive, Decimal};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    execution::{ExecutionVenue, ReconciliationReport},
    models::OrderRequest,
};

use super::{
    paper::{PaperPreviewConfig, PaperPreviewResult, PaperVenue},
    repository::{BtcRepository, BtcSettlementHealth},
};

const PAPER_RECONCILE_INTERVAL: Duration = Duration::from_secs(5);
const LIVE_UNCLEAN_RECONCILIATION_GATE_REASON: &str = "live_reconciliation_unclean";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BtcExecutionMode {
    #[default]
    Paper,
    Live,
}

impl BtcExecutionMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Paper => "paper",
            Self::Live => "live",
        }
    }
}

/// Owns the venue-specific lifecycle around the shared BTC strategy and order pathway.
///
/// Order construction and submission remain on `ExecutionVenue`. These hooks are limited to
/// state that cannot be shared between a simulated balance and an exchange account: resume,
/// settlement/reconciliation, and optional non-mutating execution previews.
#[async_trait]
pub trait BtcExecutionLifecycle: Send + Sync {
    fn mode(&self) -> BtcExecutionMode;

    fn reconcile_interval(&self) -> Duration;

    fn reconciliation_requested(&self) -> bool {
        false
    }

    async fn resume_run(
        &self,
        repository: &BtcRepository,
        process_id: Uuid,
        run_id: Uuid,
    ) -> Result<()>;

    async fn reconcile_run(
        &self,
        repository: &BtcRepository,
        process_id: Uuid,
        run_id: Uuid,
        config_hash: &str,
    ) -> Result<()>;

    async fn preview_order(
        &self,
        _request: &OrderRequest,
        _config: &PaperPreviewConfig,
    ) -> Result<Option<PaperPreviewResult>> {
        Ok(None)
    }
}

pub struct PaperExecutionLifecycle {
    venue: Arc<PaperVenue>,
}

impl PaperExecutionLifecycle {
    pub fn new(venue: Arc<PaperVenue>) -> Self {
        Self { venue }
    }
}

#[async_trait]
impl BtcExecutionLifecycle for PaperExecutionLifecycle {
    fn mode(&self) -> BtcExecutionMode {
        BtcExecutionMode::Paper
    }

    fn reconcile_interval(&self) -> Duration {
        PAPER_RECONCILE_INTERVAL
    }

    async fn resume_run(
        &self,
        repository: &BtcRepository,
        process_id: Uuid,
        run_id: Uuid,
    ) -> Result<()> {
        repository
            .recover_process_official_resolution_watches(process_id, BtcExecutionMode::Paper)
            .await?;
        let state = repository
            .paper_venue_resume_state(process_id, run_id)
            .await?;
        self.venue
            .rehydrate_capital(
                state.entry_debits_usd,
                state.settlement_credits_usd,
                state.order_count,
                state.fill_count,
                state.credited_settlement_ids,
            )
            .await
    }

    async fn reconcile_run(
        &self,
        repository: &BtcRepository,
        process_id: Uuid,
        run_id: Uuid,
        config_hash: &str,
    ) -> Result<()> {
        recover_settlement_state(repository, process_id, run_id, BtcExecutionMode::Paper).await?;
        let pending = repository
            .discover_pending_paper_settlements(process_id, run_id)
            .await?;
        for settlement in pending {
            let disposition = paper_settlement_disposition(settlement.run_id, run_id);
            let credit = if disposition.apply_venue_credit {
                Some(
                    self.venue
                        .apply_settlement_credit(settlement.settlement_id, settlement.payout)
                        .await?,
                )
            } else {
                None
            };
            let settlement_config_hash = if disposition.apply_venue_credit {
                config_hash.to_string()
            } else {
                repository
                    .run_config_hash(process_id, settlement.run_id)
                    .await?
            };
            let evidence = serde_json::json!({
                "evidence_version": disposition.evidence_version,
                "credit_kind": disposition.credit_kind,
                "settlement_id": settlement.settlement_id,
                "process_id": settlement.process_id,
                "run_id": settlement.run_id,
                "order_id": settlement.order_id,
                "model_member_id": settlement.member_id,
                "model_attribution": settlement.model_attribution,
                "market_id": settlement.market_id,
                "token_id": settlement.token_id,
                "fill_ids": settlement.fill_ids,
                "official_outcome": settlement.official_outcome,
                "official_winning_token_id": settlement.official_winning_token_id,
                "official_resolution_received_at": settlement.official_resolution_received_at,
                "official_resolution_source": settlement.official_resolution_source,
                "filled_size": settlement.filled_size,
                "entry_notional": settlement.entry_notional,
                "entry_fees": settlement.entry_fees,
                "payout": settlement.payout,
                "net_pnl": settlement.net_pnl,
                "venue_credit": credit,
                "credited_by_config_hash": settlement_config_hash,
            });
            let marked = repository
                .mark_paper_settlement_credited(
                    process_id,
                    settlement.run_id,
                    settlement.settlement_id,
                    &evidence,
                )
                .await?;
            if marked {
                use rust_decimal::prelude::ToPrimitive;
                super::unified_model_runtime::telemetry::settlement(
                    process_id,
                    settlement.net_pnl.to_f64().unwrap_or(0.0),
                    settlement.entry_fees.to_f64().unwrap_or(0.0),
                );
                super::unified_model_runtime::telemetry::member_settlement(
                    process_id,
                    settlement.member_id.as_deref(),
                    settlement.entry_notional.to_f64().unwrap_or(0.0),
                    settlement.net_pnl.to_f64().unwrap_or(0.0),
                    settlement.entry_fees.to_f64().unwrap_or(0.0),
                );
                info!(
                    process_id = %process_id,
                    run_id = %settlement.run_id,
                    settlement_id = %settlement.settlement_id,
                    order_id = %settlement.order_id,
                    current_run = disposition.apply_venue_credit,
                    credit_kind = disposition.credit_kind,
                    "BTC paper settlement credited from official resolution"
                );
            }
            if !marked {
                warn!(
                    settlement_id = %settlement.settlement_id,
                    run_id = %run_id,
                    "BTC paper settlement was already credited by a concurrent reconciliation"
                );
            }
        }
        observe_settlement_health(
            process_id,
            &repository
                .settlement_health(process_id, run_id, BtcExecutionMode::Paper)
                .await?,
        );
        Ok(())
    }

    async fn preview_order(
        &self,
        request: &OrderRequest,
        config: &PaperPreviewConfig,
    ) -> Result<Option<PaperPreviewResult>> {
        self.venue.preview_order(request, config).await.map(Some)
    }
}

pub struct LiveExecutionLifecycle {
    venue: Arc<dyn ExecutionVenue>,
    reconcile_interval: Duration,
    settlement_telemetry_refresh_required: AtomicBool,
}

impl LiveExecutionLifecycle {
    pub fn new(venue: Arc<dyn ExecutionVenue>, reconcile_interval: Duration) -> Result<Self> {
        if reconcile_interval.is_zero() {
            bail!("live execution reconcile interval must be positive");
        }
        Ok(Self {
            venue,
            reconcile_interval,
            settlement_telemetry_refresh_required: AtomicBool::new(false),
        })
    }
}

#[async_trait]
impl BtcExecutionLifecycle for LiveExecutionLifecycle {
    fn mode(&self) -> BtcExecutionMode {
        BtcExecutionMode::Live
    }

    fn reconcile_interval(&self) -> Duration {
        self.reconcile_interval
    }

    fn reconciliation_requested(&self) -> bool {
        self.venue.reconciliation_requested()
    }

    async fn resume_run(
        &self,
        repository: &BtcRepository,
        process_id: Uuid,
        _run_id: Uuid,
    ) -> Result<()> {
        repository
            .recover_process_official_resolution_watches(process_id, BtcExecutionMode::Live)
            .await?;
        hydrate_live_settlement_telemetry(repository, process_id).await?;
        self.settlement_telemetry_refresh_required
            .store(false, Ordering::Release);
        // The runner performs one mandatory reconciliation after resume hydration and admission
        // initialization. Durable settlement economics are restored above; live capital remains
        // venue-owned and is reconciled by that mandatory pass.
        Ok(())
    }

    async fn reconcile_run(
        &self,
        repository: &BtcRepository,
        process_id: Uuid,
        run_id: Uuid,
        config_hash: &str,
    ) -> Result<()> {
        let attempt = async {
            recover_settlement_state(repository, process_id, run_id, BtcExecutionMode::Live)
                .await?;
            let pending = repository
                .discover_pending_settlements(process_id, run_id, BtcExecutionMode::Live)
                .await?;
            let mut pending_redemption_count = 0usize;
            for settlement in &pending {
                if settlement.payout == Decimal::ZERO {
                    let settlement_config_hash = if settlement.run_id == run_id {
                        config_hash.to_string()
                    } else {
                        repository
                            .run_config_hash(process_id, settlement.run_id)
                            .await?
                    };
                    let recognized = repository
                        .recognize_live_zero_payout_settlement(
                            process_id,
                            settlement.run_id,
                            settlement,
                            &settlement_config_hash,
                        )
                        .await?;
                    if recognized {
                        self.settlement_telemetry_refresh_required
                            .store(true, Ordering::Release);
                        super::unified_model_runtime::telemetry::gauge(
                            process_id,
                            "settlement_telemetry_synced",
                            0.0,
                        );
                        use rust_decimal::prelude::ToPrimitive;
                        super::unified_model_runtime::telemetry::settlement(
                            process_id,
                            settlement.net_pnl.to_f64().unwrap_or(0.0),
                            settlement.entry_fees.to_f64().unwrap_or(0.0),
                        );
                        super::unified_model_runtime::telemetry::member_settlement(
                            process_id,
                            settlement.member_id.as_deref(),
                            settlement.entry_notional.to_f64().unwrap_or(0.0),
                            settlement.net_pnl.to_f64().unwrap_or(0.0),
                            settlement.entry_fees.to_f64().unwrap_or(0.0),
                        );
                        info!(
                            process_id = %process_id,
                            run_id = %settlement.run_id,
                            settlement_id = %settlement.settlement_id,
                            order_id = %settlement.order_id,
                            net_pnl = %settlement.net_pnl,
                            "BTC live zero-payout settlement recognized from official resolution"
                        );
                    }
                } else {
                    pending_redemption_count = pending_redemption_count.saturating_add(1);
                    self.settlement_telemetry_refresh_required
                        .store(true, Ordering::Release);
                    super::unified_model_runtime::telemetry::gauge(
                        process_id,
                        "settlement_telemetry_synced",
                        0.0,
                    );
                }
            }
            // Zero-payout settlement evidence is durable before wallet reconciliation so the
            // same cycle observes the credited ledger instead of waiting for the next poll.
            let reconciliation = self.venue.reconcile().await;
            if self
                .settlement_telemetry_refresh_required
                .load(Ordering::Acquire)
            {
                match hydrate_live_settlement_telemetry(repository, process_id).await {
                    Ok(()) => {
                        self.settlement_telemetry_refresh_required
                            .store(false, Ordering::Release);
                        info!(
                            event = "btc_live_settlement_telemetry_refreshed",
                            process_id = %process_id,
                            run_id = %run_id,
                            pending_redemption_count,
                            "UMR settlement telemetry refreshed from the durable live ledger"
                        );
                    }
                    Err(error) => warn!(
                        event = "btc_live_settlement_telemetry_refresh_failed",
                        process_id = %process_id,
                        run_id = %run_id,
                        pending_redemption_count,
                        error = %error,
                        "UMR settlement telemetry refresh failed; durable accounting remains authoritative and refresh will retry"
                    ),
                }
            }
            let reconciliation = reconciliation?;
            observe_settlement_health(
                process_id,
                &repository
                    .settlement_health(process_id, run_id, BtcExecutionMode::Live)
                    .await?,
            );
            Ok::<_, anyhow::Error>((reconciliation, pending_redemption_count))
        }
        .await;

        match attempt {
            Ok((reconciliation, pending_redemption_count)) => {
                let missing_order_results = repository
                    .approved_live_decisions_without_order_result(
                        process_id,
                        chrono::Utc::now()
                            - chrono::Duration::from_std(self.reconcile_interval)
                                .unwrap_or_else(|_| chrono::Duration::seconds(30)),
                    )
                    .await?;
                super::unified_model_runtime::telemetry::gauge(
                    process_id,
                    "approved_live_decisions_without_order_result",
                    missing_order_results as f64,
                );
                if missing_order_results > 0 {
                    warn!(
                        event = "approved_live_decision_without_order_result",
                        process_id = %process_id,
                        run_id = %run_id,
                        missing_order_results,
                        "approved live decisions exceeded the execution deadline without a durable order result"
                    );
                }
                let reason =
                    live_reconciliation_gate_reason(&reconciliation, pending_redemption_count)
                        .map(str::to_string);
                if let Err(error) = self
                    .venue
                    .update_live_reconciliation_health(pending_redemption_count, reason.clone())
                    .await
                {
                    warn!(
                        process_id = %process_id,
                        run_id = %run_id,
                        error = %error,
                        "failed to update process-scoped reconciliation readiness"
                    );
                }
                if let Some(reason) = reason {
                    warn!(
                        process_id = %process_id,
                        run_id = %run_id,
                        pending_settlement_count = pending_redemption_count,
                        balances_checked = reconciliation.balances_checked,
                        reconciliation_mismatches = reconciliation.mismatches_found,
                        reconciliation_unresolved = reconciliation.unresolved_count,
                        reason,
                        "BTC live reconciliation is degraded; process authorization is preserved and clean retry restores entries"
                    );
                }
            }
            Err(error) => {
                let error_chain = format!("{error:#}");
                let transient = self.venue.reconciliation_error_is_transient(&error);
                if reconciliation_failure_blocks_entries(transient) {
                    if let Err(readiness_error) = self
                        .venue
                        .update_live_reconciliation_health(0, Some(error_chain.clone()))
                        .await
                    {
                        warn!(
                            process_id = %process_id,
                            run_id = %run_id,
                            error = %readiness_error,
                            "failed to mark process-scoped reconciliation unsafe"
                        );
                    }
                }
                warn!(
                    process_id = %process_id,
                    run_id = %run_id,
                    transient,
                    error = %error_chain,
                    "BTC live reconciliation failed; retrying without changing durable process authorization"
                );
            }
        }
        Ok(())
    }
}

async fn hydrate_live_settlement_telemetry(
    repository: &BtcRepository,
    process_id: Uuid,
) -> Result<()> {
    use super::unified_model_runtime::telemetry::{self, SettlementTelemetrySnapshot};

    let rows = repository.live_settlement_telemetry(process_id).await?;
    let mut process_snapshot = None;
    let mut member_snapshots = BTreeMap::new();
    for row in rows {
        let snapshot = SettlementTelemetrySnapshot {
            wins: row
                .wins
                .try_into()
                .context("negative live settlement win count")?,
            losses: row
                .losses
                .try_into()
                .context("negative live settlement loss count")?,
            pushes: row
                .pushes
                .try_into()
                .context("negative live settlement push count")?,
            entry_notional_usd: row
                .entry_notional
                .to_f64()
                .context("live settlement entry notional exceeds telemetry range")?,
            fees_usd: row
                .fees
                .to_f64()
                .context("live settlement fees exceed telemetry range")?,
            realized_pnl_usd: row
                .realized_pnl
                .to_f64()
                .context("live settlement PnL exceeds telemetry range")?,
            gross_profit_usd: row
                .gross_profit
                .to_f64()
                .context("live settlement gross profit exceeds telemetry range")?,
            gross_loss_usd: row
                .gross_loss
                .to_f64()
                .context("live settlement gross loss exceeds telemetry range")?,
            equity_high_usd: row
                .equity_high
                .to_f64()
                .context("live settlement equity high exceeds telemetry range")?,
            max_drawdown_usd: row
                .max_drawdown
                .to_f64()
                .context("live settlement drawdown exceeds telemetry range")?,
        };
        match (row.scope.as_str(), row.member_id) {
            ("process", None) if process_snapshot.is_none() => process_snapshot = Some(snapshot),
            ("member", Some(member_id)) => {
                member_snapshots.insert(member_id, snapshot);
            }
            _ => bail!("invalid durable live settlement telemetry scope"),
        }
    }
    let process_snapshot = process_snapshot.context("missing process live settlement telemetry")?;
    telemetry::hydrate_settlements(process_id, &process_snapshot, &member_snapshots);
    Ok(())
}

async fn recover_settlement_state(
    repository: &BtcRepository,
    process_id: Uuid,
    run_id: Uuid,
    execution_mode: BtcExecutionMode,
) -> Result<()> {
    let recovered_resolutions = repository
        .recover_process_official_resolutions(process_id, execution_mode)
        .await
        .map_err(|error| {
            warn!(
                process_id = %process_id,
                run_id = %run_id,
                execution_mode = execution_mode.as_str(),
                stage = "resolution_projection",
                error_code = "btc_settlement_autoheal_failed",
                error = %error,
                "BTC settlement auto-heal failed"
            );
            error
        })?;
    let recovered_watches = repository
        .recover_process_official_resolution_watches(process_id, execution_mode)
        .await
        .map_err(|error| {
            warn!(
                process_id = %process_id,
                run_id = %run_id,
                execution_mode = execution_mode.as_str(),
                stage = "resolution_watch",
                error_code = "btc_settlement_autoheal_failed",
                error = %error,
                "BTC settlement auto-heal failed"
            );
            error
        })?;

    if recovered_resolutions > 0 {
        super::unified_model_runtime::telemetry::event(
            process_id,
            "settlement_autoheal",
            "resolution_projection",
        );
    }
    if recovered_watches > 0 {
        super::unified_model_runtime::telemetry::event(
            process_id,
            "settlement_autoheal",
            "resolution_watch",
        );
    }
    if recovered_resolutions > 0 || recovered_watches > 0 {
        info!(
            process_id = %process_id,
            run_id = %run_id,
            execution_mode = execution_mode.as_str(),
            recovered_resolution_count = recovered_resolutions,
            recovered_watch_count = recovered_watches,
            "BTC settlement state auto-healed from durable resolution evidence"
        );
    }
    Ok(())
}

fn observe_settlement_health(process_id: Uuid, health: &BtcSettlementHealth) {
    use super::unified_model_runtime::telemetry::gauge;

    gauge(
        process_id,
        "settlement_unresolved_fills",
        health.unresolved_fills as f64,
    );
    gauge(
        process_id,
        "settlement_oldest_unresolved_age_seconds",
        health.oldest_unresolved_age_seconds,
    );
    gauge(
        process_id,
        "settlement_actionable_stale_fills",
        health.actionable_stale_fills as f64,
    );
    gauge(
        process_id,
        "settlement_oldest_actionable_stale_age_seconds",
        health.oldest_actionable_stale_age_seconds,
    );
    gauge(
        process_id,
        "settlement_resolution_missing",
        health.resolution_missing as f64,
    );
    gauge(
        process_id,
        "settlement_projection_missing",
        health.projection_missing as f64,
    );
    gauge(
        process_id,
        "settlement_watch_missing",
        health.watch_missing as f64,
    );
    gauge(
        process_id,
        "settlement_ledger_missing",
        health.ledger_missing as f64,
    );
    gauge(
        process_id,
        "settlement_ledger_pending",
        health.ledger_pending as f64,
    );
    gauge(
        process_id,
        "settlement_prior_run_unresolved",
        health.prior_run_unresolved as f64,
    );
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PaperSettlementDisposition {
    apply_venue_credit: bool,
    evidence_version: &'static str,
    credit_kind: &'static str,
}

fn paper_settlement_disposition(
    settlement_run_id: Uuid,
    current_run_id: Uuid,
) -> PaperSettlementDisposition {
    if settlement_run_id == current_run_id {
        PaperSettlementDisposition {
            apply_venue_credit: true,
            evidence_version: "btc_paper_capital_credit_v1",
            credit_kind: "current_run_capital_credit",
        }
    } else {
        PaperSettlementDisposition {
            apply_venue_credit: false,
            evidence_version: "btc_paper_historical_settlement_v1",
            credit_kind: "prior_run_accounting_only",
        }
    }
}

const fn reconciliation_failure_blocks_entries(transient: bool) -> bool {
    !transient
}

fn live_reconciliation_gate_reason(
    report: &ReconciliationReport,
    _pending_settlement_count: usize,
) -> Option<&'static str> {
    if !report.balances_checked || report.mismatches_found != 0 || report.unresolved_count != 0 {
        Some(LIVE_UNCLEAN_RECONCILIATION_GATE_REASON)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use crate::execution::ReconciliationReport;

    use super::{
        live_reconciliation_gate_reason, paper_settlement_disposition,
        reconciliation_failure_blocks_entries, BtcExecutionMode,
        LIVE_UNCLEAN_RECONCILIATION_GATE_REASON,
    };

    #[test]
    fn execution_mode_names_are_stable() {
        assert_eq!(BtcExecutionMode::Paper.as_str(), "paper");
        assert_eq!(BtcExecutionMode::Live.as_str(), "live");
    }

    #[test]
    fn live_reconciliation_gate_is_nonfatal_and_fail_closed() {
        let clean = ReconciliationReport {
            open_orders: 0,
            balances_checked: true,
            mismatches_found: 0,
            unresolved_count: 0,
            checked_at: Utc::now(),
        };
        assert_eq!(live_reconciliation_gate_reason(&clean, 0), None);

        let mut unchecked = clean.clone();
        unchecked.balances_checked = false;
        assert_eq!(
            live_reconciliation_gate_reason(&unchecked, 0),
            Some(LIVE_UNCLEAN_RECONCILIATION_GATE_REASON)
        );

        let mut mismatched = clean.clone();
        mismatched.mismatches_found = 1;
        assert_eq!(
            live_reconciliation_gate_reason(&mismatched, 0),
            Some(LIVE_UNCLEAN_RECONCILIATION_GATE_REASON)
        );

        let mut unresolved = clean;
        unresolved.unresolved_count = 1;
        assert_eq!(
            live_reconciliation_gate_reason(&unresolved, 0),
            Some(LIVE_UNCLEAN_RECONCILIATION_GATE_REASON)
        );
        assert_eq!(
            live_reconciliation_gate_reason(&unresolved, 1),
            Some(LIVE_UNCLEAN_RECONCILIATION_GATE_REASON),
            "an actual unresolved order remains fail-closed"
        );
    }

    #[test]
    fn pending_live_settlement_is_diagnostic_when_reconciliation_is_clean() {
        let clean = ReconciliationReport {
            open_orders: 0,
            balances_checked: true,
            mismatches_found: 0,
            unresolved_count: 0,
            checked_at: Utc::now(),
        };
        assert_eq!(live_reconciliation_gate_reason(&clean, 1), None);
    }

    #[test]
    fn only_non_transient_reconciliation_failures_block_entries() {
        assert!(!reconciliation_failure_blocks_entries(true));
        assert!(reconciliation_failure_blocks_entries(false));
    }

    #[test]
    fn prior_run_paper_settlement_repairs_accounting_without_current_capital_credit() {
        let current_run = uuid::Uuid::from_u128(1);
        let prior_run = uuid::Uuid::from_u128(2);

        let current = paper_settlement_disposition(current_run, current_run);
        assert!(current.apply_venue_credit);
        assert_eq!(current.evidence_version, "btc_paper_capital_credit_v1");
        assert_eq!(current.credit_kind, "current_run_capital_credit");

        let prior = paper_settlement_disposition(prior_run, current_run);
        assert!(!prior.apply_venue_credit);
        assert_eq!(prior.evidence_version, "btc_paper_historical_settlement_v1");
        assert_eq!(prior.credit_kind, "prior_run_accounting_only");
    }
}
