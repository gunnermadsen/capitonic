//! Stable, bounded process-scoped operational telemetry. No database reads on scrape.
use super::contract::EVALUATION_VERSION;
use super::risk::{RiskDisposition, RiskEvaluation, RiskStrategySelection};
use crate::btc::directional_model::{RuntimeModelScore, RuntimeModelSelection};
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    sync::{Mutex, OnceLock},
    time::Instant,
};
use uuid::Uuid;

const MAX_PROCESSES: usize = 256;
const MAX_PENDING: usize = 2048;
const LATENCY_BUCKETS: [f64; 9] = [0.0001, 0.0005, 0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0];
#[derive(Clone, Default)]
struct Histogram {
    buckets: [u64; 9],
    count: u64,
    sum: f64,
}
impl Histogram {
    fn observe(&mut self, value: f64) {
        if !value.is_finite() || value < 0.0 {
            return;
        }
        self.count += 1;
        self.sum += value;
        for (i, b) in LATENCY_BUCKETS.iter().enumerate() {
            if value <= *b {
                self.buckets[i] += 1;
            }
        }
    }
}
#[derive(Clone, Serialize)]
pub struct PredictionRecord {
    pub contract_version: &'static str,
    pub process_id: Uuid,
    pub model: RuntimeModelSelection,
    pub member_id: Option<String>,
    pub router_disposition: Option<String>,
    pub run_id: Option<Uuid>,
    pub config_hash: String,
    pub execution_mode: String,
    pub feature_snapshot_id: Uuid,
    pub feature_as_of: DateTime<Utc>,
    pub input_sha256: String,
    pub score: RuntimeModelScore,
    pub inference_seconds: f64,
    pub admission: Option<serde_json::Value>,
}
#[derive(Clone)]
struct Pending {
    market: String,
    record: PredictionRecord,
}
#[derive(Clone)]
struct RouterMember {
    identity: RuntimeModelSelection,
    bucket_start: i64,
    bucket_end: i64,
    active: bool,
    ready: bool,
    counters: BTreeMap<&'static str, u64>,
    performance: MemberPerformance,
}
#[derive(Clone, Default)]
struct MemberPerformance {
    prediction_outcomes: BTreeMap<&'static str, u64>,
    order_outcomes: BTreeMap<String, u64>,
    trade_outcomes: BTreeMap<&'static str, u64>,
    bucket_evaluations: BTreeMap<&'static str, u64>,
    feature_failures: BTreeMap<String, u64>,
    brier_sum: f64,
    brier_count: u64,
    calibration_count: [u64; 10],
    calibration_probability_sum: [f64; 10],
    calibration_up_outcomes: [u64; 10],
    feature_second_sum: f64,
    feature_second_count: u64,
    last_feature_second: f64,
    decision_second_sum: f64,
    decision_second_count: u64,
    last_decision_second: f64,
    entry_second_sum: f64,
    entry_second_count: u64,
    last_entry_second: f64,
    fill_notional_usd: f64,
    filled_shares: f64,
    fill_fees_usd: f64,
    slippage_notional_usd: f64,
    settled_entry_notional_usd: f64,
    settlement_fees_usd: f64,
    realized_pnl_usd: f64,
    gross_profit_usd: f64,
    gross_loss_usd: f64,
    equity_high_usd: f64,
    max_drawdown_usd: f64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SettlementTelemetrySnapshot {
    pub wins: u64,
    pub losses: u64,
    pub pushes: u64,
    pub entry_notional_usd: f64,
    pub fees_usd: f64,
    pub realized_pnl_usd: f64,
    pub gross_profit_usd: f64,
    pub gross_loss_usd: f64,
    pub equity_high_usd: f64,
    pub max_drawdown_usd: f64,
}
#[derive(Clone, Default)]
struct Process {
    members: BTreeMap<String, RouterMember>,
    identity: Option<RuntimeModelSelection>,
    risk_identity: Option<RiskStrategySelection>,
    run_id: Option<Uuid>,
    config_hash: String,
    mode: String,
    enabled: bool,
    ready: bool,
    last_observation: f64,
    last_success: f64,
    counters: BTreeMap<(&'static str, String), u64>,
    submission_risk_counters: BTreeMap<(&'static str, &'static str), u64>,
    gauges: BTreeMap<&'static str, f64>,
    histograms: BTreeMap<&'static str, Histogram>,
    latest: Option<PredictionRecord>,
    records: VecDeque<PredictionRecord>,
    pending: VecDeque<Pending>,
    last_market: String,
    last_eligible_market: String,
    last_inferred_market: String,
    last_admitted_market: String,
    calibration_count: [u64; 10],
    calibration_sum: [f64; 10],
    calibration_outcomes: [u64; 10],
}
#[derive(Default)]
struct Registry {
    processes: HashMap<Uuid, Process>,
    dropped: u64,
}
static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
fn registry() -> &'static Mutex<Registry> {
    REGISTRY.get_or_init(Default::default)
}
fn update(id: Uuid, f: impl FnOnce(&mut Process)) {
    let Ok(mut r) = registry().lock() else {
        return;
    };
    if !r.processes.contains_key(&id) && r.processes.len() >= MAX_PROCESSES {
        r.dropped += 1;
        return;
    }
    f(r.processes.entry(id).or_default());
}
fn increment(p: &mut Process, metric: &'static str, reason: &str) {
    *p.counters.entry((metric, reason.into())).or_default() += 1;
}
pub fn register(
    id: Uuid,
    run: Uuid,
    config: &str,
    mode: &str,
    selection: Option<&RuntimeModelSelection>,
) {
    update(id, |p| {
        if p.identity.as_ref() != selection || p.run_id != Some(run) || p.config_hash != config {
            *p = Process::default();
        }
        p.identity = selection.cloned();
        p.run_id = Some(run);
        p.config_hash = config.into();
        p.mode = mode.into();
        p.enabled = true;
        for (metric, reasons) in [
            ("opportunities", &["scheduled"][..]),
            (
                "book_input_failures",
                &[
                    "missing_snapshot",
                    "identity_mismatch",
                    "epoch_mismatch",
                    "invalid_integrity",
                    "future_timestamp",
                    "stale_source_timestamp",
                    "stale_received_timestamp",
                ][..],
            ),
            ("inferences", &["success", "error"][..]),
            ("feature_builds", &["success", "error"][..]),
            ("model_admission", &["accepted", "rejected"][..]),
            ("execution", &["filled", "rejected", "submitted"][..]),
            ("prediction_outcomes", &["correct", "incorrect"][..]),
            ("trade_outcomes", &["win", "loss", "push"][..]),
            (
                "markets",
                &["observed", "eligible", "inferred", "admitted"][..],
            ),
            ("telemetry_dropped", &["pending_prediction_capacity"][..]),
        ] {
            for reason in reasons {
                p.counters.entry((metric, (*reason).into())).or_default();
            }
        }
        for (outcome, reasons) in [
            ("allowed", &["allowed"][..]),
            (
                "rejected",
                &[
                    "process_accounting_readiness",
                    "daily_loss_limit",
                    "open_notional_limit",
                    "open_position_limit",
                ][..],
            ),
            (
                "evidence_error",
                &[
                    "account_identity_evidence_unavailable",
                    "exposure_evidence_unavailable",
                    "daily_loss_evidence_unavailable",
                    "account_order_evidence_unavailable",
                    "collateral_evidence_unavailable",
                ][..],
            ),
        ] {
            for reason in reasons {
                p.submission_risk_counters
                    .entry((outcome, *reason))
                    .or_default();
            }
        }
    });
    tracing::info!(event="umr_model_registered",process_id=%id,run_id=%run,config_hash=config,model_key=selection.map(|v|v.model_key.as_str()),"UMR process model registered");
}
pub fn register_risk(id: Uuid, selection: Option<&RiskStrategySelection>) {
    update(id, |p| {
        p.risk_identity = selection.cloned();
        for reason in ["allow", "defer", "inference_error"] {
            p.counters
                .entry(("risk_disposition", reason.into()))
                .or_default();
        }
    });
}
pub fn risk_evaluation(id: Uuid, evaluation: &RiskEvaluation) {
    update(id, |p| {
        increment(
            p,
            "risk_disposition",
            if evaluation.disposition == RiskDisposition::Defer {
                "defer"
            } else {
                "allow"
            },
        );
        p.gauges.insert("risk_score", evaluation.score);
        p.gauges.insert("risk_threshold", evaluation.threshold);
        p.histograms
            .entry("risk_inference")
            .or_default()
            .observe(evaluation.inference_seconds);
    });
}
pub fn enabled(id: Uuid, value: bool) {
    update(id, |p| {
        p.enabled = value;
        if !value {
            p.ready = false;
        }
    });
}
pub fn event(id: Uuid, metric: &'static str, reason: &str) {
    update(id, |p| increment(p, metric, reason));
}
fn bounded_failure_reason(detail: &str) -> &'static str {
    let lower = detail.to_ascii_lowercase();
    if lower.contains("schema") || lower.contains("width") {
        "schema_mismatch"
    } else if lower.contains("nonfinite") || lower.contains("non_finite") {
        "non_finite_output"
    } else if lower.contains("opening") || lower.contains("pre-window") {
        "opening_reference_unavailable"
    } else if lower.contains("book") {
        "orderbook_unavailable"
    } else if lower.contains("history") {
        "history_unavailable"
    } else if lower.contains("stale") || lower.contains("age") {
        "stale_source_data"
    } else if lower.contains("unavailable") || lower.contains("missing") {
        "missing_source_data"
    } else {
        "invalid_value"
    }
}
/// Classify errors without placing arbitrary diagnostic text in metric labels.
pub fn failure(id: Uuid, stage: &'static str, detail: &str) {
    let reason = bounded_failure_reason(detail);
    event(
        id,
        if stage == "features" {
            "feature_failures"
        } else {
            "inference_failures"
        },
        reason,
    );
    update(id, |p| {
        tracing::warn!(event="umr_evaluation_failed",process_id=%id,run_id=?p.run_id,model_key=p.identity.as_ref().map(|v|v.model_key.as_str()),execution_mode=%p.mode,stage,reason,detail,"UMR evaluation unavailable");
    });
}
pub fn eligible_market(id: Uuid, market: &str) {
    update(id, |p| {
        if p.last_eligible_market != market {
            increment(p, "markets", "eligible");
            p.last_eligible_market = market.into();
        }
    });
}
pub fn duration(id: Uuid, stage: &'static str, value: f64) {
    update(id, |p| {
        p.histograms.entry(stage).or_default().observe(value)
    });
}
pub fn gauge(id: Uuid, name: &'static str, value: f64) {
    if value.is_finite() {
        update(id, |p| {
            p.gauges.insert(name, value);
        });
    }
}

#[allow(clippy::too_many_arguments)]
pub fn live_submission_risk_check(
    id: Uuid,
    outcome: &'static str,
    reason: &'static str,
    evidence_ready: bool,
    daily_net_pnl_usd: Option<f64>,
    daily_loss_headroom_usd: Option<f64>,
    pending_redemption_count: usize,
    credited_count: usize,
    open_exposure_usd: Option<f64>,
    requested_exposure_usd: Option<f64>,
    resulting_exposure_usd: Option<f64>,
    open_market_count: Option<usize>,
    resulting_market_count: Option<usize>,
    open_notional_headroom_usd: Option<f64>,
    open_position_headroom: Option<f64>,
    has_unredeemed_settlement: Option<bool>,
) {
    update(id, |p| {
        *p.submission_risk_counters
            .entry((outcome, reason))
            .or_default() += 1;
        p.gauges.insert(
            "live_submission_risk_evidence_ready",
            f64::from(evidence_ready),
        );
        p.gauges.insert(
            "live_submission_risk_last_check_timestamp_seconds",
            Utc::now().timestamp_millis() as f64 / 1000.0,
        );
        if let Some(value) = daily_net_pnl_usd.filter(|value| value.is_finite()) {
            p.gauges.insert("live_daily_loss_net_pnl_usd", value);
        }
        if let Some(value) = daily_loss_headroom_usd.filter(|value| value.is_finite()) {
            p.gauges.insert("live_daily_loss_headroom_usd", value);
        }
        p.gauges.insert(
            "live_daily_loss_pending_redemption_settlements",
            pending_redemption_count as f64,
        );
        p.gauges.insert(
            "live_daily_loss_credited_settlements",
            credited_count as f64,
        );
        for (name, value) in [
            ("live_submission_open_exposure_usd", open_exposure_usd),
            (
                "live_submission_requested_exposure_usd",
                requested_exposure_usd,
            ),
            (
                "live_submission_resulting_exposure_usd",
                resulting_exposure_usd,
            ),
            (
                "live_submission_open_notional_headroom_usd",
                open_notional_headroom_usd,
            ),
            (
                "live_submission_open_position_headroom",
                open_position_headroom,
            ),
        ] {
            if let Some(value) = value.filter(|value| value.is_finite()) {
                p.gauges.insert(name, value);
            }
        }
        if let Some(value) = open_market_count {
            p.gauges
                .insert("live_submission_open_market_count", value as f64);
        }
        if let Some(value) = resulting_market_count {
            p.gauges
                .insert("live_submission_resulting_market_count", value as f64);
        }
        if let Some(value) = has_unredeemed_settlement {
            p.gauges.insert(
                "live_submission_has_unredeemed_settlement",
                f64::from(value),
            );
        }
    });
}
pub fn member_active(id: Uuid, member_id: &str, active: bool) {
    update(id, |p| {
        if let Some(member) = p.members.get_mut(member_id) {
            member.active = active;
            if !active {
                member.ready = false;
            }
        }
        p.ready = p
            .members
            .values()
            .any(|member| member.active && member.ready);
    });
}
pub fn member_readiness(id: Uuid, member_id: &str, ready: bool) {
    update(id, |p| {
        if let Some(member) = p.members.get_mut(member_id) {
            if member.ready != ready {
                tracing::info!(event="umr_member_readiness_changed",process_id=%id,member_id,ready,"UMR member readiness changed");
            }
            member.ready = ready;
        }
        p.ready = p
            .members
            .values()
            .any(|member| member.active && member.ready);
    });
}

pub fn readiness(id: Uuid, value: bool) {
    update(id, |p| {
        if p.ready != value {
            tracing::info!(event="umr_readiness_changed",process_id=%id,ready=value,"UMR process readiness changed");
        }
        p.ready = value;
    });
}
pub fn observation(id: Uuid, market: Option<&str>) {
    update(id, |p| {
        p.last_observation = Utc::now().timestamp_millis() as f64 / 1000.0;
        increment(p, "observations", "observed");
        if let Some(m) = market {
            if p.last_market != m {
                increment(p, "markets", "observed");
                p.last_market = m.into();
            }
        }
    });
}
#[allow(clippy::too_many_arguments)]
pub fn prediction(
    id: Uuid,
    snapshot_id: Uuid,
    market: &str,
    model: &RuntimeModelSelection,
    at: DateTime<Utc>,
    input: &str,
    score: RuntimeModelScore,
    seconds: f64,
    admission: Option<serde_json::Value>,
) {
    update(id, |p| {
        if p.latest
            .as_ref()
            .is_some_and(|r| r.feature_as_of == at && r.input_sha256 == input && r.model == *model)
        {
            return;
        }
        let record = PredictionRecord {
            contract_version: EVALUATION_VERSION,
            process_id: id,
            model: model.clone(),
            member_id: None,
            router_disposition: None,
            run_id: p.run_id,
            config_hash: p.config_hash.clone(),
            execution_mode: p.mode.clone(),
            feature_snapshot_id: snapshot_id,
            feature_as_of: at,
            input_sha256: input.into(),
            score,
            inference_seconds: seconds,
            admission,
        };
        p.last_success = Utc::now().timestamp_millis() as f64 / 1000.0;
        increment(p, "inferences", "success");
        if p.last_inferred_market != market {
            increment(p, "markets", "inferred");
            p.last_inferred_market = market.into();
        }
        if score.accepted && p.last_admitted_market != market {
            increment(p, "markets", "admitted");
            p.last_admitted_market = market.into();
        }
        increment(
            p,
            "probability_bin",
            &((score.probability_up * 10.0) as usize).min(9).to_string(),
        );
        increment(
            p,
            "confidence_bin",
            &((score.confidence * 10.0) as usize).min(9).to_string(),
        );
        increment(
            p,
            "actions",
            if !score.accepted {
                "abstain"
            } else if score.probability_up >= 0.5 {
                "up"
            } else {
                "down"
            },
        );
        if let Some(a) = record.admission.as_ref() {
            if let Some(reason) = a.get("reason").and_then(|v| v.as_str()) {
                increment(p, "admission_reasons", reason);
            }
            for (field, metric) in [
                ("admission_probability", "admission_probability"),
                ("predicted_stress_edge", "predicted_stress_edge"),
                ("predicted_loss", "predicted_loss"),
                ("temporal_std", "temporal_std"),
                ("temporal_agreement", "temporal_agreement"),
            ] {
                if let Some(v) = a.get(field).and_then(|v| v.as_f64()) {
                    p.gauges.insert(metric, v);
                }
            }
        }
        increment(
            p,
            "predictions",
            if score.probability_up >= 0.5 {
                "up"
            } else {
                "down"
            },
        );
        increment(
            p,
            "model_admission",
            if score.accepted {
                "accepted"
            } else {
                "rejected"
            },
        );
        p.histograms
            .entry("inference")
            .or_default()
            .observe(seconds);
        p.gauges.insert("probability_up", score.probability_up);
        p.gauges.insert("confidence", score.confidence);
        if p.pending.len() >= MAX_PENDING {
            p.pending.pop_front();
            increment(p, "telemetry_dropped", "pending_prediction_capacity");
        }
        p.pending.push_back(Pending {
            market: market.into(),
            record: record.clone(),
        });
        p.records.push_back(record.clone());
        while p.records.len() > 64 {
            p.records.pop_front();
        }
        p.latest = Some(record);
    });
}
pub fn register_member(
    id: Uuid,
    member: &str,
    identity: &RuntimeModelSelection,
    start: i64,
    end: i64,
) {
    update(id, |p| {
        if p.members.len() < 32 {
            p.members.insert(
                member.into(),
                RouterMember {
                    identity: identity.clone(),
                    bucket_start: start,
                    bucket_end: end,
                    active: false,
                    ready: false,
                    counters: [
                        "opportunities",
                        "inferences",
                        "qualified",
                        "selected",
                        "not_selected",
                    ]
                    .into_iter()
                    .map(|k| (k, 0))
                    .collect(),
                    performance: MemberPerformance {
                        prediction_outcomes: [("correct", 0), ("incorrect", 0)].into(),
                        trade_outcomes: [("win", 0), ("loss", 0), ("push", 0)].into(),
                        bucket_evaluations: [("inside", 0), ("early", 0), ("late", 0)].into(),
                        ..Default::default()
                    },
                },
            );
        }
    });
}
pub fn member_event(id: Uuid, member: &str, event: &'static str) {
    update(id, |p| {
        if let Some(m) = p.members.get_mut(member) {
            if let Some(n) = m.counters.get_mut(event) {
                *n += 1;
            }
        }
    });
}
pub fn member_feature_failure(id: Uuid, member: &str, reason: &str) {
    let reason = bounded_failure_reason(reason);
    update(id, |p| {
        if let Some(m) = p.members.get_mut(member) {
            *m.performance
                .feature_failures
                .entry(reason.to_string())
                .or_default() += 1;
        }
    });
}
pub fn member_feature_second(id: Uuid, member: &str, value: f64) {
    if !value.is_finite() || value < 0.0 {
        return;
    }
    update(id, |p| {
        if let Some(m) = p.members.get_mut(member) {
            let result = if value < m.bucket_start as f64 {
                "early"
            } else if value > m.bucket_end as f64 {
                "late"
            } else {
                "inside"
            };
            *m.performance.bucket_evaluations.entry(result).or_default() += 1;
            m.performance.feature_second_sum += value;
            m.performance.feature_second_count += 1;
            m.performance.last_feature_second = value;
        }
    });
}
pub fn member_decision_second(id: Uuid, member: &str, value: f64) {
    if !value.is_finite() || value < 0.0 {
        return;
    }
    update(id, |p| {
        if let Some(m) = p.members.get_mut(member) {
            m.performance.decision_second_sum += value;
            m.performance.decision_second_count += 1;
            m.performance.last_decision_second = value;
        }
    });
}
pub fn member_order(id: Uuid, member: &str, state: &str) {
    let state = match state {
        "filled" | "rejected" | "submitted" => state,
        _ => "other",
    };
    update(id, |p| {
        if let Some(m) = p.members.get_mut(member) {
            *m.performance
                .order_outcomes
                .entry(state.to_string())
                .or_default() += 1;
        }
    });
}
#[allow(clippy::too_many_arguments)]
pub fn member_fill(
    id: Uuid,
    member: &str,
    price: f64,
    size: f64,
    fee: f64,
    entry_second: f64,
    quoted: f64,
) {
    if [price, size, fee, entry_second, quoted]
        .into_iter()
        .any(|value| !value.is_finite() || value < 0.0)
    {
        return;
    }
    update(id, |p| {
        if let Some(m) = p.members.get_mut(member) {
            m.performance.fill_notional_usd += price * size;
            m.performance.filled_shares += size;
            m.performance.fill_fees_usd += fee;
            m.performance.slippage_notional_usd += (price - quoted) * size;
            m.performance.entry_second_sum += entry_second;
            m.performance.entry_second_count += 1;
            m.performance.last_entry_second = entry_second;
        }
    });
}
pub fn member_settlement(id: Uuid, member: Option<&str>, entry_notional: f64, pnl: f64, fees: f64) {
    let Some(member) = member else { return };
    if !entry_notional.is_finite() || entry_notional < 0.0 || !pnl.is_finite() || !fees.is_finite()
    {
        return;
    }
    update(id, |p| {
        if let Some(m) = p.members.get_mut(member) {
            let outcome = if pnl > 0.0 {
                "win"
            } else if pnl < 0.0 {
                "loss"
            } else {
                "push"
            };
            *m.performance.trade_outcomes.entry(outcome).or_default() += 1;
            m.performance.settled_entry_notional_usd += entry_notional;
            m.performance.settlement_fees_usd += fees;
            m.performance.realized_pnl_usd += pnl;
            if pnl >= 0.0 {
                m.performance.gross_profit_usd += pnl;
            } else {
                m.performance.gross_loss_usd += pnl.abs();
            }
            m.performance.equity_high_usd = m
                .performance
                .equity_high_usd
                .max(m.performance.realized_pnl_usd);
            let drawdown = m.performance.equity_high_usd - m.performance.realized_pnl_usd;
            m.performance.max_drawdown_usd = m.performance.max_drawdown_usd.max(drawdown);
        }
    });
}
pub fn attribute_member(id: Uuid, snapshot: Uuid, member: &str, disposition: &str) {
    update(id, |p| {
        for record in p
            .records
            .iter_mut()
            .chain(p.latest.iter_mut())
            .chain(p.pending.iter_mut().map(|v| &mut v.record))
        {
            if record.feature_snapshot_id == snapshot {
                record.member_id = Some(member.into());
                record.router_disposition = Some(disposition.into());
            }
        }
    });
}
pub fn prediction_record(id: Uuid, snapshot_id: Uuid) -> Option<PredictionRecord> {
    let r = registry().lock().ok()?;
    r.processes
        .get(&id)?
        .records
        .iter()
        .find(|v| v.feature_snapshot_id == snapshot_id)
        .cloned()
}
/// Resolutions are official observed facts. Remove pending predictions once so repeated
/// delivery cannot double count. Counters cover this instrumentation session only.
pub fn resolve(market: &str, up: bool) {
    let Ok(mut r) = registry().lock() else {
        return;
    };
    for p in r.processes.values_mut() {
        let mut retained = VecDeque::new();
        let mut member_outcomes = Vec::new();
        while let Some(item) = p.pending.pop_front() {
            if item.market != market {
                retained.push_back(item);
                continue;
            }
            let probability = item.record.score.probability_up;
            let correct = (probability >= 0.5) == up;
            increment(
                p,
                "prediction_outcomes",
                if correct { "correct" } else { "incorrect" },
            );
            *p.gauges.entry("brier_sum").or_default() += (probability - f64::from(up)).powi(2);
            *p.gauges.entry("brier_count").or_default() += 1.0;
            let bin = ((probability * 10.0) as usize).min(9);
            p.calibration_count[bin] += 1;
            p.calibration_sum[bin] += probability;
            p.calibration_outcomes[bin] += u64::from(up);
            if let Some(member_id) = item.record.member_id {
                member_outcomes.push((member_id, probability, correct, bin));
            }
        }
        p.pending = retained;
        for (member_id, probability, correct, bin) in member_outcomes {
            if let Some(member) = p.members.get_mut(&member_id) {
                *member
                    .performance
                    .prediction_outcomes
                    .entry(if correct { "correct" } else { "incorrect" })
                    .or_default() += 1;
                member.performance.brier_sum += (probability - f64::from(up)).powi(2);
                member.performance.brier_count += 1;
                member.performance.calibration_count[bin] += 1;
                member.performance.calibration_probability_sum[bin] += probability;
                member.performance.calibration_up_outcomes[bin] += u64::from(up);
            }
        }
    }
}
pub fn fill(id: Uuid, price: f64, size: f64, fee: f64, entry_second: f64, quoted: f64) {
    update(id, |p| {
        increment(p, "fills", "observed");
        for (name, value) in [
            ("fill_notional_usd", price * size),
            ("filled_shares", size),
            ("fill_fees_usd", fee),
            ("entry_seconds_sum", entry_second),
            ("entry_fill_count", 1.0),
            ("slippage_notional_usd", (price - quoted) * size),
        ] {
            *p.gauges.entry(name).or_default() += value;
        }
    });
}
pub fn settlement(id: Uuid, pnl: f64, fees: f64) {
    update(id, |p| {
        increment(
            p,
            "trade_outcomes",
            if pnl > 0.0 {
                "win"
            } else if pnl < 0.0 {
                "loss"
            } else {
                "push"
            },
        );
        let net = p.gauges.entry("realized_pnl_usd").or_default();
        *net += pnl;
        let net = *net;
        *p.gauges.entry("fees_usd").or_default() += fees;
        let high = p.gauges.entry("equity_high_usd").or_default();
        *high = high.max(net);
        let dd = *high - net;
        let max = p.gauges.entry("max_drawdown_usd").or_default();
        *max = max.max(dd);
        *p.gauges
            .entry(if pnl >= 0.0 {
                "gross_profit_usd"
            } else {
                "gross_loss_usd"
            })
            .or_default() += pnl.abs();
    });
}

pub fn hydrate_settlements(
    id: Uuid,
    process_snapshot: &SettlementTelemetrySnapshot,
    member_snapshots: &BTreeMap<String, SettlementTelemetrySnapshot>,
) {
    update(id, |p| {
        for (outcome, value) in [
            ("win", process_snapshot.wins),
            ("loss", process_snapshot.losses),
            ("push", process_snapshot.pushes),
        ] {
            p.counters.insert(("trade_outcomes", outcome.into()), value);
        }
        for (name, value) in [
            ("fees_usd", process_snapshot.fees_usd),
            ("realized_pnl_usd", process_snapshot.realized_pnl_usd),
            ("gross_profit_usd", process_snapshot.gross_profit_usd),
            ("gross_loss_usd", process_snapshot.gross_loss_usd),
            ("equity_high_usd", process_snapshot.equity_high_usd),
            ("max_drawdown_usd", process_snapshot.max_drawdown_usd),
        ] {
            p.gauges.insert(name, value);
        }
        for (member_id, member) in &mut p.members {
            let snapshot = member_snapshots.get(member_id).cloned().unwrap_or_default();
            member.performance.trade_outcomes = [
                ("win", snapshot.wins),
                ("loss", snapshot.losses),
                ("push", snapshot.pushes),
            ]
            .into();
            member.performance.settled_entry_notional_usd = snapshot.entry_notional_usd;
            member.performance.settlement_fees_usd = snapshot.fees_usd;
            member.performance.realized_pnl_usd = snapshot.realized_pnl_usd;
            member.performance.gross_profit_usd = snapshot.gross_profit_usd;
            member.performance.gross_loss_usd = snapshot.gross_loss_usd;
            member.performance.equity_high_usd = snapshot.equity_high_usd;
            member.performance.max_drawdown_usd = snapshot.max_drawdown_usd;
        }
    });
}
pub struct ObservationGuard {
    id: Uuid,
    start: Instant,
    success: bool,
}
impl ObservationGuard {
    pub fn new(id: Uuid) -> Self {
        Self {
            id,
            start: Instant::now(),
            success: false,
        }
    }
    pub fn complete(&mut self) {
        self.success = true;
    }
}
impl Drop for ObservationGuard {
    fn drop(&mut self) {
        duration(self.id, "observation", self.start.elapsed().as_secs_f64());
        if !self.success {
            event(self.id, "observations_failed", "error");
        }
    }
}
fn escaped(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('"', "\\\"")
}
pub fn prometheus_metrics() -> String {
    use std::fmt::Write;
    let Ok(r) = registry().lock() else {
        return String::new();
    };
    let processes = r
        .processes
        .iter()
        .map(|(id, p)| {
            let mut gauges = p.gauges.clone();
            gauges.insert("pending_predictions", p.pending.len() as f64);
            (
                *id,
                Process {
                    identity: p.identity.clone(),
                    members: p.members.clone(),
                    risk_identity: p.risk_identity.clone(),
                    run_id: p.run_id,
                    config_hash: p.config_hash.clone(),
                    mode: p.mode.clone(),
                    enabled: p.enabled,
                    ready: p.ready,
                    last_observation: p.last_observation,
                    last_success: p.last_success,
                    counters: p.counters.clone(),
                    submission_risk_counters: p.submission_risk_counters.clone(),
                    gauges,
                    histograms: p.histograms.clone(),
                    calibration_count: p.calibration_count,
                    calibration_sum: p.calibration_sum,
                    calibration_outcomes: p.calibration_outcomes,
                    ..Default::default()
                },
            )
        })
        .collect::<Vec<_>>();
    let dropped = r.dropped;
    drop(r);
    let mut out = String::new();
    let _=writeln!(out,"# HELP polymarket_umr_registry_dropped_total Process registrations exceeding telemetry capacity.\n# TYPE polymarket_umr_registry_dropped_total counter\npolymarket_umr_registry_dropped_total {dropped}");
    let mut declared = std::collections::HashSet::new();
    for (id, p) in processes {
        let labels = format!("process_id=\"{id}\"");
        for (name, value) in [
            ("enabled", f64::from(p.enabled)),
            ("runtime_ready", f64::from(p.ready)),
            ("last_observation_timestamp_seconds", p.last_observation),
            ("last_success_timestamp_seconds", p.last_success),
        ]
        .into_iter()
        .chain(p.gauges)
        {
            if declared.insert(name.to_string()) {
                let _=writeln!(out,"# HELP polymarket_umr_{name} UMR process {name}; session-scoped unless stated otherwise.\n# TYPE polymarket_umr_{name} gauge");
            }
            let _ = writeln!(out, "polymarket_umr_{name}{{{labels}}} {value}");
        }
        for (member_id, member) in &p.members {
            if declared.insert("router_member_info".into()) {
                out.push_str("# HELP polymarket_umr_router_member_info Immutable router member and bucket identity.\n# TYPE polymarket_umr_router_member_info gauge\n");
                out.push_str("# HELP polymarket_umr_router_member_events_total Member opportunity and arbitration transitions.\n# TYPE polymarket_umr_router_member_events_total counter\n");
            }
            let _=writeln!(out,"polymarket_umr_router_member_info{{{labels},member_id=\"{}\",model_key=\"{}\",artifact_sha256=\"{}\",bucket_start=\"{}\",bucket_end=\"{}\"}} 1",escaped(member_id),escaped(&member.identity.model_key),escaped(&member.identity.artifact_sha256),member.bucket_start,member.bucket_end);
            if declared.insert("router_member_ready".into()) {
                out.push_str("# HELP polymarket_umr_router_member_ready Active member inference readiness.\n# TYPE polymarket_umr_router_member_ready gauge\n");
            }
            let _ = writeln!(
                out,
                "polymarket_umr_router_member_ready{{{labels},member_id=\"{}\"}} {}",
                escaped(member_id),
                u8::from(member.active && member.ready)
            );
            for (event, count) in &member.counters {
                let _=writeln!(out,"polymarket_umr_router_member_events_total{{{labels},member_id=\"{}\",event=\"{}\"}} {count}",escaped(member_id),event);
            }
            let member_labels = format!("{labels},member_id=\"{}\"", escaped(member_id));
            if declared.insert("model_member_info".into()) {
                out.push_str("# HELP polymarket_umr_model_member_info Immutable model-member identity and claimed evaluation bucket; emitted for singleton and router processes.\n# TYPE polymarket_umr_model_member_info gauge\n");
                out.push_str("# HELP polymarket_umr_model_member_active Whether the member is currently inside its claimed evaluation bucket.\n# TYPE polymarket_umr_model_member_active gauge\n");
                out.push_str("# HELP polymarket_umr_model_member_ready Whether an active member has healthy inputs and produced an inference.\n# TYPE polymarket_umr_model_member_ready gauge\n");
                out.push_str("# HELP polymarket_umr_model_member_bucket_start_seconds Immutable claimed evaluation bucket start in seconds after market open.\n# TYPE polymarket_umr_model_member_bucket_start_seconds gauge\n");
                out.push_str("# HELP polymarket_umr_model_member_bucket_end_seconds Immutable claimed evaluation bucket end in seconds after market open.\n# TYPE polymarket_umr_model_member_bucket_end_seconds gauge\n");
                out.push_str("# HELP polymarket_umr_model_member_activity_events_total Session-scoped member opportunity and arbitration transitions.\n# TYPE polymarket_umr_model_member_activity_events_total counter\n");
                out.push_str("# HELP polymarket_umr_model_member_prediction_outcomes_total Session-scoped resolved prediction outcomes.\n# TYPE polymarket_umr_model_member_prediction_outcomes_total counter\n");
                out.push_str("# HELP polymarket_umr_model_member_order_outcomes_total Session-scoped execution order outcomes attributed to the selected member.\n# TYPE polymarket_umr_model_member_order_outcomes_total counter\n");
                out.push_str("# HELP polymarket_umr_model_member_trade_outcomes_total Session-scoped settled economic outcomes attributed to the selected member.\n# TYPE polymarket_umr_model_member_trade_outcomes_total counter\n");
                out.push_str("# HELP polymarket_umr_model_member_bucket_evaluations_total Session-scoped inference evaluations classified against the immutable claimed bucket.\n# TYPE polymarket_umr_model_member_bucket_evaluations_total counter\n");
                out.push_str("# HELP polymarket_umr_model_member_feature_failures_total Session-scoped feature failures by bounded reason.\n# TYPE polymarket_umr_model_member_feature_failures_total counter\n");
            }
            let identity_labels = format!(
                "{member_labels},model_key=\"{}\",artifact_sha256=\"{}\",feature_schema_sha256=\"{}\",bucket_start=\"{}\",bucket_end=\"{}\"",
                escaped(&member.identity.model_key),
                escaped(&member.identity.artifact_sha256),
                escaped(&member.identity.feature_schema_sha256),
                member.bucket_start,
                member.bucket_end
            );
            let _ = writeln!(
                out,
                "polymarket_umr_model_member_info{{{identity_labels}}} 1"
            );
            let _ = writeln!(
                out,
                "polymarket_umr_model_member_active{{{member_labels}}} {}",
                u8::from(member.active)
            );
            let _ = writeln!(
                out,
                "polymarket_umr_model_member_ready{{{member_labels}}} {}",
                u8::from(member.active && member.ready)
            );
            let _ = writeln!(
                out,
                "polymarket_umr_model_member_bucket_start_seconds{{{member_labels}}} {}",
                member.bucket_start
            );
            let _ = writeln!(
                out,
                "polymarket_umr_model_member_bucket_end_seconds{{{member_labels}}} {}",
                member.bucket_end
            );
            for (event, count) in &member.counters {
                let _ = writeln!(out, "polymarket_umr_model_member_activity_events_total{{{member_labels},event=\"{event}\"}} {count}");
            }
            for (outcome, count) in &member.performance.prediction_outcomes {
                let _ = writeln!(out, "polymarket_umr_model_member_prediction_outcomes_total{{{member_labels},outcome=\"{outcome}\"}} {count}");
            }
            for (state, count) in &member.performance.order_outcomes {
                let _ = writeln!(out, "polymarket_umr_model_member_order_outcomes_total{{{member_labels},state=\"{}\"}} {count}", escaped(state));
            }
            for (outcome, count) in &member.performance.trade_outcomes {
                let _ = writeln!(out, "polymarket_umr_model_member_trade_outcomes_total{{{member_labels},outcome=\"{outcome}\"}} {count}");
            }
            for (result, count) in &member.performance.bucket_evaluations {
                let _ = writeln!(out, "polymarket_umr_model_member_bucket_evaluations_total{{{member_labels},result=\"{result}\"}} {count}");
            }
            for (reason, count) in &member.performance.feature_failures {
                let _ = writeln!(out, "polymarket_umr_model_member_feature_failures_total{{{member_labels},reason=\"{}\"}} {count}", escaped(reason));
            }
            for (name, value) in [
                ("brier_sum", member.performance.brier_sum),
                ("brier_count", member.performance.brier_count as f64),
                ("feature_second_sum", member.performance.feature_second_sum),
                (
                    "feature_second_count",
                    member.performance.feature_second_count as f64,
                ),
                (
                    "last_feature_second",
                    member.performance.last_feature_second,
                ),
                (
                    "decision_second_sum",
                    member.performance.decision_second_sum,
                ),
                (
                    "decision_second_count",
                    member.performance.decision_second_count as f64,
                ),
                (
                    "last_decision_second",
                    member.performance.last_decision_second,
                ),
                ("entry_second_sum", member.performance.entry_second_sum),
                (
                    "entry_second_count",
                    member.performance.entry_second_count as f64,
                ),
                ("last_entry_second", member.performance.last_entry_second),
                ("fill_notional_usd", member.performance.fill_notional_usd),
                ("filled_shares", member.performance.filled_shares),
                ("fill_fees_usd", member.performance.fill_fees_usd),
                (
                    "slippage_notional_usd",
                    member.performance.slippage_notional_usd,
                ),
                (
                    "settled_entry_notional_usd",
                    member.performance.settled_entry_notional_usd,
                ),
                (
                    "settlement_fees_usd",
                    member.performance.settlement_fees_usd,
                ),
                ("realized_pnl_usd", member.performance.realized_pnl_usd),
                ("gross_profit_usd", member.performance.gross_profit_usd),
                ("gross_loss_usd", member.performance.gross_loss_usd),
                ("max_drawdown_usd", member.performance.max_drawdown_usd),
            ] {
                if declared.insert(format!("model_member_{name}")) {
                    let _ = writeln!(out, "# HELP polymarket_umr_model_member_{name} Session-scoped per-model member {name}.\n# TYPE polymarket_umr_model_member_{name} gauge");
                }
                let _ = writeln!(
                    out,
                    "polymarket_umr_model_member_{name}{{{member_labels}}} {value}"
                );
            }
            for bin in 0..10 {
                for (name, value) in [
                    (
                        "calibration_count",
                        member.performance.calibration_count[bin] as f64,
                    ),
                    (
                        "calibration_probability_sum",
                        member.performance.calibration_probability_sum[bin],
                    ),
                    (
                        "calibration_up_outcomes",
                        member.performance.calibration_up_outcomes[bin] as f64,
                    ),
                ] {
                    if declared.insert(format!("model_member_{name}")) {
                        let _ = writeln!(out, "# HELP polymarket_umr_model_member_{name} Session-scoped per-model resolved calibration evidence.\n# TYPE polymarket_umr_model_member_{name} gauge");
                    }
                    let _ = writeln!(out, "polymarket_umr_model_member_{name}{{{member_labels},bin=\"{bin}\"}} {value}");
                }
            }
        }
        if let Some(m) = p.identity {
            if declared.insert("model_info".into()) {
                out.push_str("# HELP polymarket_umr_model_info Immutable active model and process configuration identity.\n# TYPE polymarket_umr_model_info gauge\n");
            }
            let _=writeln!(out,"polymarket_umr_model_info{{{labels},model_key=\"{}\",artifact_sha256=\"{}\",feature_schema_sha256=\"{}\",execution_mode=\"{}\",config_hash=\"{}\"}} 1",escaped(&m.model_key),escaped(&m.artifact_sha256),escaped(&m.feature_schema_sha256),escaped(&p.mode),escaped(&p.config_hash));
        }
        if let Some(m) = p.risk_identity {
            if declared.insert("risk_model_info".into()) {
                out.push_str("# HELP polymarket_umr_risk_model_info Immutable active risk model identity.\n# TYPE polymarket_umr_risk_model_info gauge\n");
            }
            let _=writeln!(out,"polymarket_umr_risk_model_info{{{labels},model_key=\"{}\",artifact_sha256=\"{}\",config_hash=\"{}\"}} 1",escaped(&m.model_key),escaped(&m.artifact_sha256),escaped(&p.config_hash));
        }
        for ((name, reason), value) in p.counters {
            if declared.insert(format!("{name}_total")) {
                let _=writeln!(out,"# HELP polymarket_umr_{name}_total UMR {name} events by bounded reason.\n# TYPE polymarket_umr_{name}_total counter");
            }
            let _ = writeln!(
                out,
                "polymarket_umr_{name}_total{{{labels},reason=\"{}\"}} {value}",
                escaped(&reason)
            );
        }
        if declared.insert("live_submission_risk_checks_total".into()) {
            out.push_str("# HELP polymarket_umr_live_submission_risk_checks_total Process-scoped live pre-submit risk checks by bounded outcome and reason.\n# TYPE polymarket_umr_live_submission_risk_checks_total counter\n");
        }
        for ((outcome, reason), value) in p.submission_risk_counters {
            let _ = writeln!(
                out,
                "polymarket_umr_live_submission_risk_checks_total{{{labels},outcome=\"{outcome}\",reason=\"{reason}\"}} {value}"
            );
        }
        for (stage, h) in p.histograms {
            if declared.insert("stage_duration_seconds".into()) {
                out.push_str("# HELP polymarket_umr_stage_duration_seconds UMR stage latency in seconds.\n# TYPE polymarket_umr_stage_duration_seconds histogram\n");
            }
            for (i, b) in LATENCY_BUCKETS.iter().enumerate() {
                let _=writeln!(out,"polymarket_umr_stage_duration_seconds_bucket{{{labels},stage=\"{stage}\",le=\"{b}\"}} {}",h.buckets[i]);
            }
            let _=writeln!(out,"polymarket_umr_stage_duration_seconds_bucket{{{labels},stage=\"{stage}\",le=\"+Inf\"}} {}\npolymarket_umr_stage_duration_seconds_sum{{{labels},stage=\"{stage}\"}} {}\npolymarket_umr_stage_duration_seconds_count{{{labels},stage=\"{stage}\"}} {}",h.count,h.sum,h.count);
        }
        for bin in 0..10 {
            for (name, value) in [
                ("calibration_count", p.calibration_count[bin] as f64),
                ("calibration_probability_sum", p.calibration_sum[bin]),
                (
                    "calibration_up_outcomes",
                    p.calibration_outcomes[bin] as f64,
                ),
            ] {
                if declared.insert(name.into()) {
                    let _=writeln!(out,"# HELP polymarket_umr_{name} Resolved evaluation-weighted calibration bin {name}.\n# TYPE polymarket_umr_{name} gauge");
                }
                let _ = writeln!(
                    out,
                    "polymarket_umr_{name}{{{labels},bin=\"{bin}\"}} {value}"
                );
            }
        }
    }
    out
}

#[cfg(test)]
mod router_tests {
    use super::*;
    #[test]
    fn an_unready_member_does_not_poison_a_ready_member() {
        let id = Uuid::new_v4();
        let identity = RuntimeModelSelection {
            model_key: "test".into(),
            artifact_sha256: "a".repeat(64),
            feature_schema_sha256: "b".repeat(64),
        };
        for name in ["first", "second"] {
            register_member(id, name, &identity, 60, 89);
            member_active(id, name, true);
        }
        member_readiness(id, "first", true);
        member_readiness(id, "second", false);
        assert!(registry().lock().unwrap().processes[&id].ready);
        member_active(id, "first", false);
        assert!(!registry().lock().unwrap().processes[&id].ready);
        member_readiness(id, "second", true);
        assert!(registry().lock().unwrap().processes[&id].ready);
    }

    #[test]
    fn singleton_member_exports_bucket_execution_and_settlement_evidence() {
        let id = Uuid::new_v4();
        let identity = RuntimeModelSelection {
            model_key: "singleton".into(),
            artifact_sha256: "a".repeat(64),
            feature_schema_sha256: "b".repeat(64),
        };
        register_member(id, "legacy_primary", &identity, 60, 89);
        member_active(id, "legacy_primary", true);
        member_readiness(id, "legacy_primary", true);
        for second in [59.0, 60.0, 89.0, 90.0] {
            member_feature_second(id, "legacy_primary", second);
        }
        member_decision_second(id, "legacy_primary", 61.0);
        member_order(id, "legacy_primary", "filled");
        member_fill(id, "legacy_primary", 0.4, 5.0, 0.02, 62.0, 0.39);
        member_settlement(id, Some("legacy_primary"), 2.0, 2.98, 0.02);

        let metrics = prometheus_metrics();
        let scoped = metrics
            .lines()
            .filter(|line| line.contains(&id.to_string()))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(scoped.contains("polymarket_umr_model_member_info"));
        assert!(scoped.contains("model_key=\"singleton\""));
        assert!(scoped.contains("bucket_start=\"60\""));
        assert!(scoped.contains("bucket_end=\"89\""));
        assert!(scoped.contains("result=\"inside\"} 2"));
        assert!(scoped.contains("result=\"early\"} 1"));
        assert!(scoped.contains("result=\"late\"} 1"));
        assert!(scoped.contains("state=\"filled\"} 1"));
        assert!(scoped.contains("outcome=\"win\"} 1"));
        assert!(scoped.contains("polymarket_umr_model_member_realized_pnl_usd"));
        assert!(scoped.contains("polymarket_umr_model_member_entry_second_count"));
    }

    #[test]
    fn durable_settlement_hydration_replaces_session_totals_idempotently() {
        let id = Uuid::new_v4();
        let identity = RuntimeModelSelection {
            model_key: "settlement-hydration".into(),
            artifact_sha256: "a".repeat(64),
            feature_schema_sha256: "b".repeat(64),
        };
        register(id, Uuid::new_v4(), "config", "live", Some(&identity));
        register_member(id, "primary", &identity, 60, 89);
        settlement(id, 99.0, 10.0);
        member_settlement(id, Some("primary"), 50.0, 99.0, 10.0);

        let process = SettlementTelemetrySnapshot {
            wins: 2,
            losses: 2,
            entry_notional_usd: 9.75,
            fees_usd: 0.35,
            realized_pnl_usd: 0.096,
            gross_profit_usd: 5.446,
            gross_loss_usd: 5.35,
            equity_high_usd: 0.096,
            max_drawdown_usd: 5.35,
            ..Default::default()
        };
        let members = BTreeMap::from([("primary".into(), process.clone())]);
        hydrate_settlements(id, &process, &members);
        hydrate_settlements(id, &process, &members);

        let metrics = prometheus_metrics();
        let scoped = metrics
            .lines()
            .filter(|line| line.contains(&id.to_string()))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(scoped.contains("polymarket_umr_trade_outcomes_total{process_id=\""));
        assert!(scoped.contains("reason=\"win\"} 2"));
        assert!(scoped.contains("reason=\"loss\"} 2"));
        assert!(scoped.contains("polymarket_umr_realized_pnl_usd"));
        assert!(scoped.contains("} 0.096"));
        assert!(scoped.contains("member_id=\"primary\",outcome=\"win\"} 2"));
        assert!(scoped.contains("polymarket_umr_model_member_settled_entry_notional_usd"));
        assert!(!scoped.contains("} 99"));
    }

    #[test]
    fn live_submission_risk_exports_zero_baselines_and_first_evidence_failure() {
        let id = Uuid::new_v4();
        register(id, Uuid::new_v4(), "config", "live", None);

        let initial = prometheus_metrics();
        assert!(initial.contains(&format!(
            "polymarket_umr_live_submission_risk_checks_total{{process_id=\"{id}\",outcome=\"evidence_error\",reason=\"daily_loss_evidence_unavailable\"}} 0"
        )));

        live_submission_risk_check(
            id,
            "evidence_error",
            "daily_loss_evidence_unavailable",
            false,
            None,
            None,
            0,
            0,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        );
        let failed = prometheus_metrics();
        assert!(failed.contains(&format!(
            "polymarket_umr_live_submission_risk_checks_total{{process_id=\"{id}\",outcome=\"evidence_error\",reason=\"daily_loss_evidence_unavailable\"}} 1"
        )));
        assert!(failed.contains(&format!(
            "polymarket_umr_live_submission_risk_evidence_ready{{process_id=\"{id}\"}} 0"
        )));

        live_submission_risk_check(
            id,
            "allowed",
            "allowed",
            true,
            Some(2.21306),
            Some(5.21306),
            1,
            0,
            Some(0.0),
            Some(1.82963),
            Some(1.82963),
            Some(0),
            Some(1),
            Some(1.17037),
            Some(0.0),
            Some(true),
        );
        let recovered = prometheus_metrics();
        assert!(recovered.contains(&format!(
            "polymarket_umr_live_submission_risk_evidence_ready{{process_id=\"{id}\"}} 1"
        )));
        assert!(recovered.contains(&format!(
            "polymarket_umr_live_daily_loss_pending_redemption_settlements{{process_id=\"{id}\"}} 1"
        )));
        assert!(recovered.contains(&format!(
            "polymarket_umr_live_daily_loss_net_pnl_usd{{process_id=\"{id}\"}} 2.21306"
        )));
        assert!(recovered.contains(&format!(
            "polymarket_umr_live_submission_open_exposure_usd{{process_id=\"{id}\"}} 0"
        )));
        assert!(recovered.contains(&format!(
            "polymarket_umr_live_submission_resulting_market_count{{process_id=\"{id}\"}} 1"
        )));
        assert!(recovered.contains(&format!(
            "polymarket_umr_live_submission_has_unredeemed_settlement{{process_id=\"{id}\"}} 1"
        )));
    }
}
