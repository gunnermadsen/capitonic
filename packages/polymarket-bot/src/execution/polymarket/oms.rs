//! Shared admission. No process lookup, JSON policy parsing, inference or venue submission.
use super::configuration::{CompiledDefinition, ExecutionMode};
use crate::{
    btc::{
        admission::{
            AdmissionDisposition, DailyRealizedPnlHighWaterMarkState,
            LossRegimeConfidenceFloorState, ProposedEntryExposure,
        },
        process_runner::book_features,
        strategy::{
            self, BtcDecision, BtcFeatureSnapshot, BtcStrategyConfig, BtcStrategyPrediction,
        },
        types::{BtcOutcome, OrderbookCheckpoint},
        BtcDirectionalModelEntryPolicy,
    },
    models::{OrderRequest, OrderSide},
};
use anyhow::{ensure, Context, Result};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    Umr,
    Api,
}
impl Origin {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Umr => "umr",
            Self::Api => "api",
        }
    }
}

/// A candidate carries prediction evidence, never caller-selected quantity or order type.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub(crate) id: Uuid,
    pub(crate) member: String,
    pub(crate) snapshot: BtcFeatureSnapshot,
    pub(crate) outcome: BtcOutcome,
    pub(crate) probability: Decimal,
    pub(crate) conservative_probability: Decimal,
    pub(crate) decision: Option<BtcDecision>,
    pub(crate) api_request_hash: Option<String>,
}
impl Candidate {
    pub fn synthetic(
        id: Uuid,
        member: String,
        snapshot: BtcFeatureSnapshot,
        outcome: BtcOutcome,
        probability: Decimal,
    ) -> Result<Self> {
        ensure!(
            !id.is_nil()
                && !member.is_empty()
                && (Decimal::ZERO..=Decimal::ONE).contains(&probability),
            "invalid synthetic candidate"
        );
        Ok(Self {
            id,
            member,
            snapshot,
            outcome,
            probability,
            conservative_probability: probability,
            decision: None,
            api_request_hash: None,
        })
    }
    pub fn qualified(
        id: Uuid,
        member: String,
        snapshot: BtcFeatureSnapshot,
        decision: BtcDecision,
    ) -> Result<Self> {
        let intent = decision
            .approved_intent
            .as_ref()
            .context("UMR candidate has no qualified intent")?;
        ensure!(
            decision.reject_reason.is_none()
                && intent.process_id == snapshot.process_id
                && intent.feature_snapshot_id == snapshot.snapshot_id
                && intent.market_id == snapshot.market_id
                && decision.feature_snapshot_id == snapshot.snapshot_id
                && decision.process_id == snapshot.process_id,
            "UMR candidate identity or qualification mismatch"
        );
        let outcome = intent.outcome;
        ensure!(
            matches!(
                (decision.action, outcome),
                (strategy::BtcDecisionAction::BuyUp, BtcOutcome::Up)
                    | (strategy::BtcDecisionAction::BuyDown, BtcOutcome::Down)
            ),
            "qualified action mismatch"
        );
        let (probability, conservative_probability) = match &decision.prediction {
            Some(BtcStrategyPrediction::DirectionalPrediction {
                outcome: predicted,
                probability,
                conservative_probability,
                ..
            }) => {
                ensure!(*predicted == outcome, "prediction outcome mismatch");
                (*probability, *conservative_probability)
            }
            Some(_) => anyhow::bail!("UMR candidate has no executable prediction"),
            None => {
                let fair = decision
                    .fair_value
                    .as_ref()
                    .context("missing qualified fair value")?;
                match outcome {
                    BtcOutcome::Up => (fair.up_probability, fair.up_lower_bound),
                    BtcOutcome::Down => (fair.down_probability, fair.down_lower_bound),
                }
            }
        };
        let mut result = Self::synthetic(id, member, snapshot, outcome, probability)?;
        ensure!(
            (Decimal::ZERO..=probability).contains(&conservative_probability),
            "invalid conservative probability"
        );
        result.conservative_probability = conservative_probability;
        result.decision = Some(decision);
        Ok(result)
    }
    pub fn origin(&self) -> Origin {
        if self.decision.is_some() {
            Origin::Umr
        } else {
            Origin::Api
        }
    }
}

#[derive(Debug, Clone)]
pub struct Ownership {
    pub process_id: Uuid,
    pub run_id: Uuid,
}
#[derive(Debug, Clone)]
pub(crate) struct Accounting {
    pub available: Decimal,
    pub open_notional: Decimal,
    pub open_markets: Vec<String>,
    pub daily_pnl: Decimal,
    pub high_water: Option<DailyRealizedPnlHighWaterMarkState>,
    pub loss_regime: Option<LossRegimeConfidenceFloorState>,
}
/// Only the OMS can construct this capability; adapters accept no unapproved public request.
#[derive(Debug, Clone)]
pub(crate) struct ExecutionIntent {
    pub request: OrderRequest,
    pub origin: Origin,
}

pub(crate) struct Oms {
    pub definition: CompiledDefinition,
    pub owner: Ownership,
    pub run_config_sha256: String,
}
impl Oms {
    #[cfg(test)]
    pub fn new(definition: CompiledDefinition, owner: Ownership) -> Result<Self> {
        let run_config_sha256 = definition.config_sha256.clone();
        Self::new_for_run(definition, owner, run_config_sha256)
    }

    pub fn new_for_run(
        definition: CompiledDefinition,
        owner: Ownership,
        run_config_sha256: String,
    ) -> Result<Self> {
        ensure!(
            !owner.process_id.is_nil() && !owner.run_id.is_nil(),
            "invalid order ownership"
        );
        definition.verify_identity()?;
        ensure!(
            definition.matches_config_sha256(&run_config_sha256),
            "run configuration identity mismatch"
        );
        Ok(Self {
            definition,
            owner,
            run_config_sha256,
        })
    }
    pub fn strategy(&self) -> BtcStrategyConfig {
        self.definition.strategy()
    }
}
impl CompiledDefinition {
    pub fn strategy(&self) -> BtcStrategyConfig {
        let p = &self.execution.policies;
        BtcStrategyConfig {
            target_size: p.sizing.default_share,
            min_entry_price: p.sizing.min_entry_price,
            max_entry_price: p.sizing.max_entry_price,
            max_depth_participation: p.sizing.max_depth_participation,
            min_seconds_after_open: p.entry_timing.min_seconds_after_open,
            min_seconds_before_close: p.entry_timing.min_seconds_before_close,
            max_reference_age_ms: p.evidence.max_reference_age_ms,
            max_chainlink_open_delay_ms: p.evidence.max_chainlink_open_delay_ms,
            max_source_skew_ms: p.evidence.max_source_skew_ms,
            max_book_age_ms: p.evidence.max_book_age_ms,
            max_fee_age_ms: p.evidence.max_fee_age_ms,
            max_fee_rate: p.execution_cost.max_fee_rate,
            spread_reserve_fraction: p.execution_cost.spread_reserve_fraction,
            slippage_reserve_bps: p.execution_cost.slippage_reserve_bps,
            latency_reserve_per_share: p.execution_cost.latency_reserve_per_share,
            min_net_edge_per_share: p.edge.min_net_edge_per_share,
            min_net_edge_usd: p.edge.min_net_edge_usd,
            max_directional_feature_age_ms: self.control.strategy.max_directional_feature_age_ms,
            required_model_feeds: self.control.strategy.required_model_feeds.clone(),
            ..BtcStrategyConfig::default()
        }
    }

    pub fn runtime_strategy(&self) -> Result<BtcStrategyConfig> {
        let base = self.strategy();
        let keys = self
            .sources
            .iter()
            .map(|source| source.key.as_str())
            .collect();
        self.control
            .strategy
            .decision_strategy
            .compile_members(&base, &keys)?
            .into_iter()
            .next()
            .map(|(_, strategy, _)| strategy)
            .context("execution model router has no members")
    }
}
impl Oms {
    pub(crate) fn validate_accounting(
        &self,
        market: &str,
        probability: Decimal,
        accounting: &Accounting,
        exposure: &ProposedEntryExposure,
        now: DateTime<Utc>,
    ) -> Result<()> {
        self.validate_accounting_report(
            market,
            probability,
            accounting,
            exposure,
            now,
            &mut super::admission_report::AdmissionReport::default(),
        )
    }
    fn validate_accounting_report(
        &self,
        market: &str,
        probability: Decimal,
        accounting: &Accounting,
        exposure: &ProposedEntryExposure,
        now: DateTime<Utc>,
        report: &mut super::admission_report::AdmissionReport,
    ) -> Result<()> {
        report.begin("collateral", "venue_requirement");
        let p = &self.definition.execution.policies;
        ensure!(
            accounting.available >= exposure.entry_debit_usd(),
            "insufficient_collateral"
        );
        report.begin("exposure_policy", "process_policy");
        ensure!(
            p.exposure
                .max_open_notional_usd
                .is_none_or(|limit| accounting.open_notional + exposure.entry_debit_usd() <= limit),
            "open_notional_limit"
        );
        ensure!(
            p.exposure.max_open_positions.is_none_or(|limit| accounting
                .open_markets
                .contains(&market.to_string())
                || accounting.open_markets.len() < limit),
            "open_position_limit"
        );
        report.begin("loss_limit_policy", "process_policy");
        ensure!(
            p.loss_limit
                .max_daily_loss_usd
                .is_none_or(|limit| accounting.daily_pnl > -limit),
            "daily_loss_limit"
        );
        if let Some(policy) = &p.loss_limit.daily_realized_pnl_high_water_mark {
            let state = accounting
                .high_water
                .as_ref()
                .context("high_water_evidence_unavailable")?;
            ensure!(
                state.process_id == self.owner.process_id
                    && state.as_of <= now
                    && state.period_end_utc > now,
                "high_water_evidence_mismatch"
            );
            ensure!(
                state.evaluate(policy, exposure)?.disposition == AdmissionDisposition::Allow,
                "high_water_risk_budget_exceeded"
            );
        }
        if p.loss_regime.is_none() {
            report.not_applicable("loss_regime_policy", "process_policy", "not_configured");
        }
        if let Some(policy) = &p.loss_regime {
            report.begin("loss_regime_policy", "process_policy");
            ensure!(
                accounting
                    .loss_regime
                    .as_ref()
                    .context("loss_regime_evidence_unavailable")?
                    .evaluate(policy, probability)?
                    .disposition
                    == AdmissionDisposition::Allow,
                "loss_regime_confidence_floor"
            );
        }
        Ok(())
    }
    pub fn admit(
        &self,
        candidate: &Candidate,
        books: [Option<&OrderbookCheckpoint>; 2],
        accounting: &Accounting,
        now: DateTime<Utc>,
    ) -> Result<ExecutionIntent> {
        let (result, policy_results) = self.admit_report(candidate, books, accounting, now);
        result.map_err(|error| {
            super::AdmissionRejection {
                reason: error.to_string(),
                policy_results,
            }
            .into()
        })
    }
    pub(crate) fn admit_report(
        &self,
        candidate: &Candidate,
        books: [Option<&OrderbookCheckpoint>; 2],
        accounting: &Accounting,
        now: DateTime<Utc>,
    ) -> (Result<ExecutionIntent>, serde_json::Value) {
        let mut report = super::admission_report::AdmissionReport::default();
        let mut result = self.admit_inner(candidate, books, accounting, now, &mut report);
        let report = report.finish(result.as_ref().err());
        if let Ok(intent) = &mut result {
            intent.request.metadata["polymarket_execution"]["policy_results"] = report.clone();
        }
        (result, report)
    }
    fn admit_inner(
        &self,
        candidate: &Candidate,
        books: [Option<&OrderbookCheckpoint>; 2],
        accounting: &Accounting,
        now: DateTime<Utc>,
        report: &mut super::admission_report::AdmissionReport,
    ) -> Result<ExecutionIntent> {
        report.begin("execution_authority", "process_policy");
        let definition = &self.definition;
        let p = &definition.execution.policies;
        ensure!(
            definition.execution.execute_signals
                && (definition.execution.mode != ExecutionMode::Live
                    || definition.execution.live_capital),
            "order_submission_disabled"
        );
        ensure!(
            candidate.snapshot.process_id == self.owner.process_id,
            "candidate ownership mismatch"
        );
        report.begin("order_execution_policy", "process_policy");
        let order_type = *p
            .order_execution
            .member_order_types
            .get(&candidate.member)
            .context("unknown member")?;
        report.begin("market_availability", "venue_requirement");
        let snapshot = &candidate.snapshot;
        ensure!(
            snapshot.market_active && !snapshot.market_closed && snapshot.accepting_orders,
            "market_unavailable"
        );
        ensure!(
            snapshot.observed_at <= now && now < snapshot.window_end,
            "market_closed_or_future_evidence"
        );
        report.begin("entry_timing_policy", "process_policy");
        let elapsed = (now - snapshot.window_start).num_seconds();
        let fixed = p.entry_timing.min_seconds_after_open == 120
            && p.entry_timing.min_seconds_before_close == 180;
        ensure!(
            elapsed >= p.entry_timing.min_seconds_after_open
                && (if fixed {
                    elapsed == 120
                } else {
                    now < snapshot.window_end
                        - chrono::Duration::seconds(p.entry_timing.min_seconds_before_close)
                }),
            "outside_entry_window"
        );
        // Quantity is resolved before either depth walk and before any quantity-dependent cost.
        report.begin("sizing_policy", "process_policy");
        let config = self.strategy();
        report.begin("evidence_policy", "process_policy");
        let mut current = snapshot.clone();
        current.observed_at = now;
        for (book, original, outcome) in [
            (books[0], &snapshot.up_book, BtcOutcome::Up),
            (books[1], &snapshot.down_book, BtcOutcome::Down),
        ] {
            if let Some(book) = book {
                ensure!(
                    book.market_id == snapshot.market_id && book.token_id == original.token_id,
                    "book identity mismatch"
                );
            }
            let features =
                book_features(outcome, &original.token_id, book, now, config.target_size);
            match outcome {
                BtcOutcome::Up => current.up_book = features,
                BtcOutcome::Down => current.down_book = features,
            }
        }
        let selected = match candidate.outcome {
            BtcOutcome::Up => &current.up_book,
            BtcOutcome::Down => &current.down_book,
        };
        let opposite = match candidate.outcome {
            BtcOutcome::Up => &current.down_book,
            BtcOutcome::Down => &current.up_book,
        };
        let check = |result: std::result::Result<(), strategy::BtcRejectReason>| {
            result.map_err(|e| anyhow::anyhow!(e.as_str()))
        };
        check(strategy::validate_execution_book(
            &config,
            &current,
            selected,
            strategy::BtcRejectReason::MissingUpBook,
        ))?;
        report.begin("liquidity_policy", "process_policy");
        if p.liquidity.require_exit_book {
            check(strategy::validate_execution_book(
                &config,
                &current,
                opposite,
                strategy::BtcRejectReason::MissingDownBook,
            ))?;
        }
        report.begin("execution_cost_policy", "process_policy");
        check(strategy::validate_fee(&config, &current))?;
        let fee_rate = if current.fees_enabled {
            current.fee_rate.context("missing_fee_rate")?
        } else {
            Decimal::ZERO
        };
        let edge = strategy::quote_outcome_edge(
            &config,
            selected,
            candidate.conservative_probability,
            fee_rate,
            current.minimum_order_size,
        )
        .map_err(|e| anyhow::anyhow!(e.as_str()))?;
        report.begin("edge_policy", "process_policy");
        let directional = candidate
            .decision
            .as_ref()
            .is_none_or(|d| d.prediction.is_some());
        if directional {
            ensure!(
                p.edge.directional_model_entry_policy
                    == BtcDirectionalModelEntryPolicy::ExecuteDirectionalPrediction
                    || candidate.probability - edge.executable_price - edge.taker_fee / edge.size
                        > Decimal::ZERO,
                "prediction_direct_edge_non_positive"
            );
        } else {
            ensure!(
                strategy::edge_passes(&config, &edge),
                "edge_below_threshold"
            );
        }
        if definition.control.risk_strategies.is_empty() {
            report.not_applicable("risk_strategy", "process_policy", "not_configured");
        }
        if let Some(selection) = definition.control.risk_strategies.first() {
            report.begin("risk_strategy", "process_policy");
            use crate::btc::unified_model_runtime::risk;
            let intent = strategy::ApprovedIntent {
                intent_id: candidate.id,
                process_id: self.owner.process_id,
                feature_snapshot_id: snapshot.snapshot_id,
                market_id: snapshot.market_id.clone(),
                window_start: snapshot.window_start,
                outcome: candidate.outcome,
                token_id: selected.token_id.clone(),
                limit_price: edge.marketable_limit_price,
                size: edge.size,
                expected_net_edge: edge.net_edge,
                expected_net_edge_per_share: edge.net_edge_per_share,
                strategy_version: "synthetic_order_candidate".into(),
                feature_schema_version: snapshot.feature_schema_version.clone(),
            };
            let risk_candidate = BtcDecision {
                decision_id: candidate.id,
                process_id: self.owner.process_id,
                feature_snapshot_id: snapshot.snapshot_id,
                evaluated_at: now,
                action: match candidate.outcome {
                    BtcOutcome::Up => strategy::BtcDecisionAction::BuyUp,
                    BtcOutcome::Down => strategy::BtcDecisionAction::BuyDown,
                },
                reject_reason: None,
                fair_value: None,
                up_edge: None,
                down_edge: None,
                approved_intent: Some(intent),
                prediction: Some(BtcStrategyPrediction::DirectionalPrediction {
                    outcome: candidate.outcome,
                    probability: candidate.probability,
                    conservative_probability: candidate.conservative_probability,
                    minimum_conservative_probability: Decimal::ZERO,
                    probability_uncertainty: Decimal::ZERO,
                    executable_price: Some(edge.executable_price),
                    direct_taker_fee_per_share: Some(edge.taker_fee / edge.size),
                    direct_net_edge_per_share: Some(
                        candidate.probability - edge.executable_price - edge.taker_fee / edge.size,
                    ),
                    entry_policy: p.edge.directional_model_entry_policy,
                }),
            };
            ensure!(
                risk::load(selection)?
                    .evaluate(&current, &risk_candidate)?
                    .disposition
                    == risk::RiskDisposition::Allow,
                "risk_defer"
            );
        }
        let exposure = ProposedEntryExposure::for_buy_cash_budget(
            edge.size,
            edge.marketable_limit_price,
            fee_rate,
        )?;
        self.validate_accounting_report(
            &snapshot.market_id,
            candidate.conservative_probability,
            accounting,
            &exposure,
            now,
            report,
        )?;
        if let Some(decision) = &candidate.decision {
            let qualified = decision
                .approved_intent
                .as_ref()
                .context("missing qualified intent")?;
            ensure!(
                edge.marketable_limit_price <= qualified.limit_price,
                "qualified_price_exceeded"
            );
        }
        let mode = match definition.execution.mode {
            ExecutionMode::Paper => "paper",
            ExecutionMode::Live => "live",
        };
        let mut request = OrderRequest {
            client_order_id: candidate.id,
            process_id: Some(self.owner.process_id),
            market_id: snapshot.market_id.clone(),
            token_id: selected.token_id.clone(),
            side: OrderSide::Buy,
            order_type,
            price: edge.marketable_limit_price,
            size: edge.size,
            metadata: serde_json::json!({"execution_intent":"entry","process_id":self.owner.process_id,"run_id":self.owner.run_id,
                "model_member_id":candidate.member,"outcome":candidate.outcome,"dynamic_fee_rate":fee_rate,
                "decision_at":snapshot.observed_at,"polymarket_execution":{"version":super::RAIL_CONTRACT_VERSION,"oms_version":super::OMS_CONTRACT_VERSION,
                "config_sha256":self.run_config_sha256,"origin":candidate.origin(),"mode":mode,"account_ref":definition.execution.account_ref,
                "candidate_sha256":candidate_identity(candidate)?,"starting_collateral_usd":p.paper_execution.as_ref().map(|p|p.starting_collateral_usd),
                "shared_admission_at":now,"quantity":edge.size,
                "policy_evidence":{"order_policies":p,"book_received_at":selected.received_at,"book_source_at":selected.source_timestamp,
                "fee_observed_at":current.fee_rate_observed_at,"max_entry_debit_usd":exposure.entry_debit_usd(),
                "loss_regime_applied":p.loss_regime.is_some(),"high_water_applied":p.loss_limit.daily_realized_pnl_high_water_mark.is_some(),
                "model_qualification_owner":if candidate.decision.is_some(){"umr"}else{"synthetic_api"}},"quantity_semantics":"buy_cash_budget","api_request_sha256":candidate.api_request_hash,"net_edge":edge.net_edge}}),
        };
        report.begin("marketable_depth", "venue_requirement");
        let selected_checkpoint = match candidate.outcome {
            BtcOutcome::Up => books[0],
            BtcOutcome::Down => books[1],
        }
        .context("missing execution book")?;
        ensure!(
            crate::btc::live_execution::validate_marketable_depth(
                selected_checkpoint,
                &request,
                p.sizing.max_depth_participation
            )?
            .is_none(),
            "orderbook_marketability"
        );
        if candidate.decision.is_none() {
            report.not_applicable("model_qualification", "artifact", "synthetic_api_candidate");
        }
        if let Some(original) = &candidate.decision {
            report.begin("model_qualification", "artifact");
            use crate::btc::execution_guard::{
                BtcExecutionFreshnessBounds, BtcReferenceExecutionGuard,
            };
            use sha2::{Digest, Sha256};
            let keys = definition.sources.iter().map(|s| s.key.as_str()).collect();
            let mut base = config.clone();
            base.max_directional_feature_age_ms =
                definition.control.strategy.max_directional_feature_age_ms;
            base.required_model_feeds = definition.control.strategy.required_model_feeds.clone();
            let (_, strategy, _) = definition
                .control
                .strategy
                .decision_strategy
                .compile_members(&base, &keys)?
                .into_iter()
                .find(|(id, _, _)| id == &candidate.member)
                .context("unknown member")?;
            let mut decision = original.clone();
            let intent = decision
                .approved_intent
                .as_mut()
                .context("missing qualified intent")?;
            ensure!(
                intent.strategy_version == strategy.strategy_version
                    && intent.feature_schema_version == strategy.feature_schema_version,
                "qualified model mismatch"
            );
            intent.limit_price = request.price;
            intent.size = request.size;
            let mut metadata = crate::btc::process_runner::btc_entry_order_metadata(
                &strategy,
                intent,
                decision.prediction.as_ref(),
                decision.decision_id,
                self.owner.process_id,
                self.owner.run_id,
                fee_rate,
            )?;
            metadata
                .as_object_mut()
                .context("invalid metadata")?
                .extend(
                    request
                        .metadata
                        .as_object()
                        .context("invalid metadata")?
                        .clone(),
                );
            request.metadata = metadata;
            let bounds = BtcExecutionFreshnessBounds {
                max_reference_age_ms: p.evidence.max_reference_age_ms,
                max_directional_feature_age_ms: strategy
                    .effective_max_directional_feature_age_ms()?,
            };
            let hash = format!("{:x}", Sha256::digest(serde_json::to_vec(snapshot)?));
            let guard = BtcReferenceExecutionGuard::from_snapshot(
                snapshot,
                &decision,
                decision
                    .approved_intent
                    .as_ref()
                    .context("missing intent")?,
                &request,
                &hash,
                fee_rate,
                bounds,
            )?;
            guard.insert_into_metadata(&mut request.metadata)?;
        }
        Ok(ExecutionIntent {
            request,
            origin: candidate.origin(),
        })
    }
}

pub(crate) fn candidate_identity(candidate: &Candidate) -> Result<String> {
    use sha2::{Digest, Sha256};
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(
            &serde_json::json!({"id":candidate.id,"member":candidate.member,
        "origin":candidate.origin(),"snapshot":candidate.snapshot,"outcome":candidate.outcome,"probability":candidate.probability,
        "conservative_probability":candidate.conservative_probability,"decision":candidate.decision})
        )?)
    ))
}
