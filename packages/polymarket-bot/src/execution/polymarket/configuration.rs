//! Additive configuration compiler. Existing process APIs call this before their normal persistence.
//! It never starts a runner, connects to an account, or submits an order.
use std::collections::{BTreeSet, HashSet};

use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::btc::feed_contract::BtcModelFeedRequirement;
use crate::btc::unified_model_runtime::{
    risk::RiskStrategySelection,
    router::{RouterDefinition, Selection},
};
use crate::btc::BtcStrategyConfig;
use crate::market_data_stream::SourceSelector;
use crate::models::TradingProcessConfig;

use super::{policies::OrderPolicies, PROCESS_SCHEMA_VERSION};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RailSelection {
    pub name: String,
    pub version: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionMode {
    Paper,
    Live,
}

impl ExecutionMode {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Paper => "paper",
            Self::Live => "live",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeControl {
    pub strategy_interval_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelControl {
    pub decision_strategy: RouterDefinition,
    pub max_directional_feature_age_ms: Option<i64>,
    pub required_model_feeds: Vec<BtcModelFeedRequirement>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessControl {
    pub schema_version: String,
    pub playbook_version: String,
    pub next_experiment_key: String,
    pub preregistration_sha256: String,
    pub strategy: ModelControl,
    pub risk_strategies: Vec<RiskStrategySelection>,
    pub runtime: RuntimeControl,
}

/// No process object or database handle crosses this boundary into execution.
#[derive(Debug, Clone, Serialize)]
pub struct ExecutionSettings {
    pub rail: RailSelection,
    pub mode: ExecutionMode,
    pub execute_signals: bool,
    pub live_capital: bool,
    pub account_ref: Option<String>,
    pub policies: OrderPolicies,
}

#[derive(Debug, Clone)]
pub struct CompiledDefinition {
    pub execution: ExecutionSettings,
    pub control: ProcessControl,
    pub sources: Vec<SourceSelector>,
    pub config_sha256: String,
    compatible_config_sha256: Vec<String>,
}

impl CompiledDefinition {
    pub(crate) fn verify_identity(&self) -> Result<()> {
        let canonical = serde_json::json!({"rail_contract_version":super::RAIL_CONTRACT_VERSION,"oms_contract_version":super::OMS_CONTRACT_VERSION,"execution":self.execution,"control":self.control,"sources":self.sources});
        ensure!(
            self.config_sha256 == format!("{:x}", Sha256::digest(serde_json::to_vec(&canonical)?)),
            "compiled execution identity mismatch"
        );
        Ok(())
    }

    pub fn matches_config_sha256(&self, value: &str) -> bool {
        self.config_sha256 == value
            || self
                .compatible_config_sha256
                .iter()
                .any(|compatible| compatible == value)
    }
}

/// Any marker selects strict new-contract validation, never a fallback to legacy.
pub fn is_selected(config: &TradingProcessConfig) -> bool {
    config.rail.is_some()
        || config.order_policies.is_some()
        || config
            .btc_realtime_paper
            .get("schema_version")
            .and_then(serde_json::Value::as_str)
            == Some(PROCESS_SCHEMA_VERSION)
}

pub fn compile(config: &TradingProcessConfig) -> Result<CompiledDefinition> {
    let e = config;
    let rail = e.rail.as_ref().context("explicit rail is required")?;
    ensure!(
        rail.name == "polymarket" && rail.version == 1,
        "unsupported execution rail or contract version"
    );
    ensure!(
        e.taker_fee_rate.is_none()
            && e.max_order_notional_usd.is_none()
            && e.max_open_notional_usd.is_none()
            && e.max_open_positions.is_none()
            && e.max_daily_loss_usd.is_none()
            && e.require_exit_book.is_none(),
        "legacy flat order parameters are not allowed with order_policies"
    );
    let mode = match e.mode.as_deref() {
        Some("paper") => {
            ensure!(
                e.execute_signals && !e.live_capital && e.account_ref.is_none(),
                "invalid paper execution authorization"
            );
            ExecutionMode::Paper
        }
        Some("live") => {
            ensure!(
                e.execute_signals == e.live_capital,
                "live execution authorization must be enabled together"
            );
            let account = e
                .account_ref
                .as_deref()
                .context("live account_ref is required")?;
            ensure!(
                !account.is_empty()
                    && account.len() <= 128
                    && account.bytes().all(
                        |v| v.is_ascii_alphanumeric() || matches!(v, b'.' | b'_' | b'-' | b':')
                    ),
                "invalid account_ref"
            );
            ExecutionMode::Live
        }
        _ => anyhow::bail!("execution.mode must be paper or live"),
    };
    let policies = OrderPolicies::compile(
        e.order_policies
            .as_deref()
            .context("order_policies are required")?,
        mode == ExecutionMode::Paper,
    )?;
    let mut sources = config.sources.clone();
    let value = &config.btc_realtime_paper;
    if let Some(members) = value
        .pointer("/strategy/decision_strategy/models")
        .and_then(serde_json::Value::as_array)
    {
        ensure!(
            members.iter().all(|m| m.get("entry_order_type").is_none()),
            "entry_order_type belongs only in order_execution_policy"
        );
    }
    // The v6 contract was not certified with `runtime.trigger`. Ignore that retired draft
    // member when reattaching already-persisted test definitions; it has no compiled meaning.
    let retired_trigger = value
        .pointer("/runtime/trigger")
        .and_then(serde_json::Value::as_str)
        .filter(|value| matches!(*value, "api" | "umr"));
    let mut compatible_value = value.clone();
    if let Some(runtime) = compatible_value
        .get_mut("runtime")
        .and_then(serde_json::Value::as_object_mut)
    {
        runtime.remove("trigger");
    }
    let mut control: ProcessControl = serde_json::from_value(compatible_value)
        .context("invalid execution process configuration")?;
    ensure!(
        control.schema_version == PROCESS_SCHEMA_VERSION && control.playbook_version == "v1.2",
        "unsupported process schema or playbook version"
    );
    let key = &control.next_experiment_key;
    ensure!(
        !key.is_empty()
            && key.len() <= 128
            && key.bytes().enumerate().all(|(i, v)| v.is_ascii_lowercase()
                || v.is_ascii_digit()
                || (i > 0 && matches!(v, b'.' | b'_' | b'-'))),
        "invalid next_experiment_key"
    );
    ensure!(
        control.preregistration_sha256.len() == 64
            && control
                .preregistration_sha256
                .bytes()
                .all(|v| v.is_ascii_hexdigit()),
        "invalid preregistration_sha256"
    );
    control.preregistration_sha256.make_ascii_lowercase();
    ensure!(
        (1..=60_000).contains(&control.runtime.strategy_interval_ms),
        "invalid strategy_interval_ms"
    );
    ensure!(!sources.is_empty(), "sources are required");
    let mut keys = HashSet::new();
    for source in &sources {
        source.validate()?;
        ensure!(keys.insert(source.key.as_str()), "duplicate source");
    }
    let router = &control.strategy.decision_strategy;
    router.validate()?;
    ensure!(
        router
            .models
            .iter()
            .map(|m| &m.member_id)
            .collect::<BTreeSet<_>>()
            == policies.order_execution.member_order_types.keys().collect(),
        "order type mappings must exactly match router members"
    );
    for member in &router.models {
        member.selection.identity().validate()?;
        ensure!(
            policies.edge.directional_model_entry_policy
                != crate::btc::BtcDirectionalModelEntryPolicy::ExecuteDirectionalPrediction
                || !matches!(member.selection, Selection::BtcAsymmetricValueModel { .. }),
            "execute_directional_prediction requires a directional model or agent"
        );
        if mode == ExecutionMode::Live {
            ensure!(
                !matches!(member.selection, Selection::OpenaiAgent { .. }),
                "agent live execution is not supported"
            );
            let model = crate::btc::runtime_model(&member.selection.identity())?;
            ensure!(
                model.live_capital_allowed(),
                "model artifact does not authorize live capital"
            );
        }
    }
    // Reuse existing UMR compatibility validation, not a second model/configuration engine.
    // This is a validation view only: it is never persisted or handed to a legacy runner.
    let evidence = &policies.evidence;
    let base = BtcStrategyConfig {
        target_size: policies.sizing.default_share,
        min_seconds_after_open: policies.entry_timing.min_seconds_after_open,
        min_seconds_before_close: policies.entry_timing.min_seconds_before_close,
        max_reference_age_ms: evidence.max_reference_age_ms,
        max_book_age_ms: evidence.max_book_age_ms,
        max_chainlink_open_delay_ms: evidence.max_chainlink_open_delay_ms,
        max_source_skew_ms: evidence.max_source_skew_ms,
        max_fee_age_ms: evidence.max_fee_age_ms,
        max_directional_feature_age_ms: control.strategy.max_directional_feature_age_ms,
        required_model_feeds: control.strategy.required_model_feeds.clone(),
        ..BtcStrategyConfig::default()
    };
    router.compile_members(&base, &keys)?;
    crate::btc::unified_model_runtime::risk::validate_selections(&control.risk_strategies)?;
    if mode == ExecutionMode::Live {
        ensure!(
            evidence.max_reference_age_ms <= 2000 && evidence.max_book_age_ms <= 2000,
            "live execution freshness exceeds existing 2000ms bound"
        );
    }
    sources.sort_by(|a, b| a.key.cmp(&b.key));
    let execution = ExecutionSettings {
        rail: rail.clone(),
        mode,
        execute_signals: e.execute_signals,
        live_capital: e.live_capital,
        account_ref: e.account_ref.clone(),
        policies,
    };
    let canonical = serde_json::json!({"rail_contract_version": super::RAIL_CONTRACT_VERSION, "oms_contract_version": super::OMS_CONTRACT_VERSION, "execution": execution, "control": control, "sources": sources});
    let config_sha256 = format!("{:x}", Sha256::digest(serde_json::to_vec(&canonical)?));
    let compatible_config_sha256 = retired_trigger
        .map(|trigger| {
            let mut legacy_control = serde_json::to_value(&control)?;
            legacy_control["runtime"]["trigger"] = serde_json::json!(trigger);
            let legacy = serde_json::json!({"rail_contract_version": super::RAIL_CONTRACT_VERSION, "oms_contract_version": super::OMS_CONTRACT_VERSION, "execution": execution, "control": legacy_control, "sources": sources});
            Ok::<_, anyhow::Error>(format!(
                "{:x}",
                Sha256::digest(serde_json::to_vec(&legacy)?)
            ))
        })
        .transpose()?
        .into_iter()
        .collect();
    Ok(CompiledDefinition {
        execution,
        control,
        sources,
        config_sha256,
        compatible_config_sha256,
    })
}

/// Definition writes reuse the UMR pinning extension, then validate the entire candidate.
/// The caller persists only after this succeeds. No live credentials are accessed.
pub fn pin_definition(config: &mut TradingProcessConfig) -> Result<()> {
    let mut candidate = config.clone();
    let selected = candidate
        .btc_realtime_paper
        .pointer_mut("/strategy/decision_strategy")
        .context("missing model router")?;
    ensure!(
        selected
            .get("models")
            .and_then(serde_json::Value::as_array)
            .is_none_or(|members| members.iter().all(|m| m.get("entry_order_type").is_none())),
        "entry_order_type belongs only in order_execution_policy"
    );
    let mut router: RouterDefinition = serde_json::from_value(selected.clone())?;
    router.resolve_pins()?;
    *selected = serde_json::to_value(router)?;
    compile(&candidate)?;
    *config = candidate;
    Ok(())
}
