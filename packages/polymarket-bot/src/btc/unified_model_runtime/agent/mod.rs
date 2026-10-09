//! Asynchronous decision providers within UMR. Process runners alone own execution.
pub mod auth;
pub(crate) mod calibration;
mod context;
pub(super) mod diagnostics;
mod market_context;
mod provider;
mod reassessment;
mod session;
mod usage;
use anyhow::{ensure, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
pub use context::{build_context, build_settlement_context, build_volatility_context};
pub use diagnostics::FailureDiagnostic;
pub use market_context::add_market_context;
pub use provider::OpenAiProvider;
pub use reassessment::{add_entry_reassessment_context, add_reassessment_context};
use serde::{Deserialize, Serialize};
use serde_json::Value;
pub use session::AgentSession;
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;
pub use usage::{TokenUsage, UsageReport};
use uuid::Uuid;

pub const BRIDGE_VERSION: &str = "capitonic-umr-async-evaluation-v1";
pub const CONTEXT_VERSION: &str = "capitonic-btc-agent-context-v1";
pub const STRATEGY_VERSION: &str = "btc_5m_openai_agent_v1";
pub const PROCESS_SCHEMA_VERSION: &str = "btc_realtime_paper_process_v5";
pub const PROFILE_KEY: &str = "btc-5m-openai-agent-v1";
pub const SETTLEMENT_PROFILE_KEY: &str = "btc-5m-openai-agent-settlement-v1";
pub const SETTLEMENT_CONTEXT_VERSION: &str = "capitonic-btc-agent-settlement-context-v1";
pub const VOLATILITY_PROFILE_KEY: &str = "btc-5m-openai-agent-volatility-v1";
pub const VOLATILITY_CONTEXT_VERSION: &str = "capitonic-btc-agent-volatility-context-v1";
pub const REASSESSMENT_PROFILE_KEY: &str = "btc-5m-openai-agent-reassessment-v1";
pub const REASSESSMENT_CONTEXT_VERSION: &str = "capitonic-btc-agent-reassessment-context-v1";
pub const ENTRY_REASSESSMENT_PROFILE_KEY: &str = "btc-5m-openai-agent-entry-reassessment-v1";
pub const ENTRY_REASSESSMENT_CONTEXT_VERSION: &str =
    "capitonic-btc-agent-entry-reassessment-context-v1";
pub const CALIBRATION_PROFILE_KEY: &str = "btc-5m-openai-agent-calibration-v1";
pub const CALIBRATION_CONTEXT_VERSION: &str = "capitonic-btc-agent-calibration-context-v1";
pub const MARKET_CONTEXT_PROFILE_KEY: &str = "btc-5m-openai-agent-market-context-v1";
pub const MARKET_CONTEXT_VERSION: &str = "capitonic-btc-agent-market-context-v1";
const MARKET_CONTEXT_INSTRUCTION: &str = "Use market_horizons to compare the 15-second, 5-minute and 15-minute RTDS midpoint paths: returns, sample volatility, path efficiency and reversals. Respect each window's source end, sampling interval and availability; closed-minute summaries lag the current observation. Use settlement_uncertainty to relate the observed official TWAP distance to remaining time, the supporting RTDS variability scale and changes in the reference/TWAP gap. The square-root-time scale assumes stationary independent returns, ignores drift and TWAP averaging, and is not a calibrated settlement distribution or probability. A zero or unavailable scale gives no certainty. Reference/TWAP gap changes can reflect different observation timestamps; they do not guarantee convergence. Missing optional summaries are unknown and do not alone require abstention. Preserve the exact settlement rule and distinguish directional evidence from entry price.";

pub fn uses_calibration(key: &str) -> bool {
    matches!(key, CALIBRATION_PROFILE_KEY | MARKET_CONTEXT_PROFILE_KEY)
}

const CALIBRATION_INSTRUCTION: &str = "Use historical_calibration as historical reliability evidence when estimating probability_up. Its bands describe selected-side probabilities, not probability_up; respect direction when interpreting them. Source profiles are separate cohorts, not measured performance of this profile. Initial and latest reassessment populations can overlap markets and are not independent. Filled versus unfilled describes historical selection, not a known property of the current forecast. Small samples, wide intervals, unavailable outcomes and truncated coverage imply uncertainty; do not mechanically substitute a historical win rate, invent certainty, or abstain solely because history is unavailable. Forecast from the current causal observations with the existing settlement rule.";

pub fn uses_entry_reassessment(key: &str) -> bool {
    matches!(
        key,
        ENTRY_REASSESSMENT_PROFILE_KEY | CALIBRATION_PROFILE_KEY | MARKET_CONTEXT_PROFILE_KEY
    )
}

const ENTRY_REASSESSMENT_INSTRUCTION: &str = "When entry_reassessment.trigger is forecast_became_affordable, the earlier forecast could not enter at its observed price and its selected side has since become affordable. Reassess whether the changed reference, TWAP and Binance evidence supports a bargain or a deteriorating forecast. Reaffirm or revise the settlement direction using current evidence; a cheaper entry alone is not evidence for the earlier direction.";
const REASSESSMENT_INSTRUCTION: &str = "When previous_prediction_comparison is available, reassess your earlier forecast using the changed evidence. Reaffirm or revise it; your previous prediction is context, not a commitment. Distinguish the likely settlement outcome from the attractiveness of the current entry price. Missing comparisons are unknown, not zero, and do not require abstention. With no previous prediction, forecast from the current observations as usual.";
pub const SETTLEMENT_RULE: &str = "UP when the official Chainlink BTC/USD 60-second TWAP at window_end is greater than or equal to its value at window_start; otherwise DOWN. RTDS midpoint, Binance spot and Polygon oracle are supporting indicators, not settlement prices. Missing TWAP observations are unknown, not zero; do not substitute midpoint prices for them.";
pub const REQUIRED_PRODUCTS: [&str; 5] = [
    "polymarket_btc_five_minute_market_contracts",
    "polymarket_btc_five_minute_orderbooks",
    "polymarket_btc_five_minute_resolutions",
    "polymarket_rtds_chainlink_reference_price",
    "binance_spot_btcusdt_one_second_ohlcv",
];
pub const OPTIONAL_PRODUCTS: [&str; 2] = [
    "polygon_chainlink_btcusd_oracle",
    "binance_futures_btcusdt_open_interest",
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentSelection {
    pub profile_key: String,
    pub profile_sha256: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentProfile {
    pub version: &'static str,
    pub key: &'static str,
    pub model: &'static str,
    pub instructions: String,
    pub attempts_seconds: [i64; 3],
    pub decision_ceiling_seconds: i64,
    pub timeout_seconds: u64,
    pub context_version: &'static str,
}

pub fn profile() -> AgentProfile {
    AgentProfile {
        version: BRIDGE_VERSION,
        key: PROFILE_KEY,
        model: "gpt-6-astra",
        instructions: "Forecast the UP or DOWN settlement of this BTC five-minute market using only the supplied causal observations. Use the exact market resolution rule. Attempt a direction even when uncertain; mistakes in this paper process are acceptable. Return JSON only: direction (up/down), probability_up (0..1), confidence (0..1, self-assessed evidence strength, not a calibrated probability), reason_codes (1..8 short snake_case identifiers). UP requires probability_up >= 0.5; DOWN requires probability_up < 0.5. Distinguish your forecast from the market price. Missing optional context is not evidence of zero. Do not invent observations, browse, or submit orders.".into(),
        attempts_seconds: [45, 75, 105],
        decision_ceiling_seconds: 120,
        timeout_seconds: 25,
        context_version: CONTEXT_VERSION,
    }
}

pub fn hash(value: &impl Serialize) -> Result<String> {
    Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(value)?)))
}

pub fn profile_for_key(key: &str) -> Result<AgentProfile> {
    let mut selected = profile();
    match key {
        PROFILE_KEY => {}
        SETTLEMENT_PROFILE_KEY => {
            selected.key = SETTLEMENT_PROFILE_KEY;
            selected.context_version = SETTLEMENT_CONTEXT_VERSION;
            selected.instructions.push(' ');
            selected.instructions.push_str(SETTLEMENT_RULE);
        }
        VOLATILITY_PROFILE_KEY => {
            selected = profile_for_key(SETTLEMENT_PROFILE_KEY)?;
            selected.key = VOLATILITY_PROFILE_KEY;
            selected.context_version = VOLATILITY_CONTEXT_VERSION;
        }
        REASSESSMENT_PROFILE_KEY => {
            selected = profile_for_key(VOLATILITY_PROFILE_KEY)?;
            selected.key = REASSESSMENT_PROFILE_KEY;
            selected.context_version = REASSESSMENT_CONTEXT_VERSION;
            selected.instructions.push(' ');
            selected.instructions.push_str(REASSESSMENT_INSTRUCTION);
        }
        ENTRY_REASSESSMENT_PROFILE_KEY => {
            selected = profile_for_key(REASSESSMENT_PROFILE_KEY)?;
            selected.key = ENTRY_REASSESSMENT_PROFILE_KEY;
            selected.context_version = ENTRY_REASSESSMENT_CONTEXT_VERSION;
            selected.instructions.push(' ');
            selected
                .instructions
                .push_str(ENTRY_REASSESSMENT_INSTRUCTION);
        }
        CALIBRATION_PROFILE_KEY => {
            selected = profile_for_key(ENTRY_REASSESSMENT_PROFILE_KEY)?;
            selected.key = CALIBRATION_PROFILE_KEY;
            selected.context_version = CALIBRATION_CONTEXT_VERSION;
            selected.instructions.push(' ');
            selected.instructions.push_str(CALIBRATION_INSTRUCTION);
        }
        MARKET_CONTEXT_PROFILE_KEY => {
            selected = profile_for_key(CALIBRATION_PROFILE_KEY)?;
            selected.key = MARKET_CONTEXT_PROFILE_KEY;
            selected.context_version = MARKET_CONTEXT_VERSION;
            selected.instructions.push(' ');
            selected.instructions.push_str(MARKET_CONTEXT_INSTRUCTION);
        }
        _ => anyhow::bail!("unsupported agent profile"),
    }
    Ok(selected)
}

impl AgentSelection {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.profile_sha256 == hash(&profile_for_key(&self.profile_key)?)?,
            "agent profile checksum mismatch"
        );
        Ok(())
    }
    pub fn context_version(&self) -> &'static str {
        if self.profile_key == MARKET_CONTEXT_PROFILE_KEY {
            MARKET_CONTEXT_VERSION
        } else if self.profile_key == CALIBRATION_PROFILE_KEY {
            CALIBRATION_CONTEXT_VERSION
        } else if self.profile_key == ENTRY_REASSESSMENT_PROFILE_KEY {
            ENTRY_REASSESSMENT_CONTEXT_VERSION
        } else if self.profile_key == REASSESSMENT_PROFILE_KEY {
            REASSESSMENT_CONTEXT_VERSION
        } else if self.profile_key == VOLATILITY_PROFILE_KEY {
            VOLATILITY_CONTEXT_VERSION
        } else if self.profile_key == SETTLEMENT_PROFILE_KEY {
            SETTLEMENT_CONTEXT_VERSION
        } else {
            CONTEXT_VERSION
        }
    }
    pub fn identity(&self) -> super::super::directional_model::RuntimeModelSelection {
        // The existing identity fields describe the selected inference package, not ML weights.
        super::super::directional_model::RuntimeModelSelection {
            model_key: self.profile_key.clone(),
            artifact_sha256: self.profile_sha256.clone(),
            feature_schema_sha256: format!(
                "{:x}",
                Sha256::digest(self.context_version().as_bytes())
            ),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluationRequest {
    pub version: String,
    pub request_id: Uuid,
    pub process_id: Uuid,
    pub run_id: Uuid,
    pub config_hash: String,
    pub selection: AgentSelection,
    pub market_id: String,
    pub window_start: DateTime<Utc>,
    pub snapshot_id: Uuid,
    pub observed_at: DateTime<Utc>,
    pub deadline: DateTime<Utc>,
    pub attempt: usize,
    pub input_sha256: String,
    pub context: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Prediction {
    pub direction: super::super::types::BtcOutcome,
    pub probability_up: f64,
    pub confidence: f64,
    pub reason_codes: Vec<String>,
}
impl Prediction {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.probability_up.is_finite() && (0.0..=1.0).contains(&self.probability_up),
            "invalid probability"
        );
        ensure!(
            self.confidence.is_finite() && (0.0..=1.0).contains(&self.confidence),
            "invalid confidence"
        );
        ensure!(
            (self.direction == super::super::types::BtcOutcome::Up) == (self.probability_up >= 0.5),
            "direction contradicts probability"
        );
        ensure!(
            !self.reason_codes.is_empty()
                && self.reason_codes.len() <= 8
                && self.reason_codes.iter().all(|r| !r.is_empty()
                    && r.len() <= 64
                    && r.bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')),
            "invalid reason codes"
        );
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderFailure {
    Authentication,
    Capacity,
    Transport,
    InvalidResponse,
    Timeout,
    Cancelled,
    Superseded,
}
impl ProviderFailure {
    pub fn code(self) -> &'static str {
        match self {
            Self::Authentication => "authentication",
            Self::Capacity => "capacity",
            Self::Transport => "transport",
            Self::InvalidResponse => "invalid_response",
            Self::Timeout => "timeout",
            Self::Cancelled => "cancelled",
            Self::Superseded => "superseded",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluationResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_diagnostic: Option<FailureDiagnostic>,
    pub request: EvaluationRequest,
    pub completed_at: DateTime<Utc>,
    pub inference_seconds: f64,
    pub response_id: Option<String>,
    pub model: Option<String>,
    #[serde(default)]
    pub usage: UsageReport,
    pub prediction: Result<Prediction, ProviderFailure>,
}

#[async_trait]
pub trait AsyncDecisionProvider: Send + Sync {
    async fn evaluate(
        &self,
        request: EvaluationRequest,
        cancellation: CancellationToken,
    ) -> EvaluationResult;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn settlement_profile_preserves_original_identity_and_schedule() {
        let original = profile();
        assert_eq!(
            hash(&original).unwrap(),
            "77293f5aeeb0588c53aadb2e71b07840a823b8b65395cf2061563f7a5c5c33f3"
        );
        let selected = profile_for_key(SETTLEMENT_PROFILE_KEY).unwrap();
        let selection = AgentSelection {
            profile_key: selected.key.into(),
            profile_sha256: hash(&selected).unwrap(),
        };
        selection.validate().unwrap();
        assert_ne!(selection.profile_sha256, hash(&original).unwrap());
        assert_eq!(selected.model, original.model);
        assert_eq!(selected.attempts_seconds, original.attempts_seconds);
        assert_eq!(
            selected.decision_ceiling_seconds,
            original.decision_ceiling_seconds
        );
        assert_eq!(selected.timeout_seconds, original.timeout_seconds);
        assert_eq!(
            selected.instructions,
            format!("{} {}", original.instructions, SETTLEMENT_RULE)
        );
        assert_eq!(selection.context_version(), SETTLEMENT_CONTEXT_VERSION);
        assert_eq!(
            hash(&selected).unwrap(),
            "be1c61203b42773ab443df3ca533e4de621f55bd75f6eceb2ceb319aa8c3ab0e"
        );
        let volatility = profile_for_key(VOLATILITY_PROFILE_KEY).unwrap();
        let volatility_selection = AgentSelection {
            profile_key: volatility.key.into(),
            profile_sha256: hash(&volatility).unwrap(),
        };
        volatility_selection.validate().unwrap();
        assert_eq!(
            volatility_selection.context_version(),
            VOLATILITY_CONTEXT_VERSION
        );
        let mut expected = serde_json::to_value(selected).unwrap();
        expected["key"] = serde_json::json!(VOLATILITY_PROFILE_KEY);
        expected["context_version"] = serde_json::json!(VOLATILITY_CONTEXT_VERSION);
        assert_eq!(serde_json::to_value(volatility).unwrap(), expected);
    }

    #[test]
    fn profile_is_pinned_and_predictions_are_consistent() {
        AgentSelection {
            profile_key: PROFILE_KEY.into(),
            profile_sha256: hash(&profile()).unwrap(),
        }
        .validate()
        .unwrap();
        let mut p = Prediction {
            direction: super::super::super::types::BtcOutcome::Up,
            probability_up: 0.5,
            confidence: 0.1,
            reason_codes: vec!["uncertain".into()],
        };
        p.validate().unwrap();
        p.probability_up = 0.49;
        assert!(p.validate().is_err());
        p.probability_up = f64::NAN;
        assert!(p.validate().is_err());
    }

    #[test]
    fn reassessment_only_extends_volatility_context_and_instructions() {
        let previous = profile_for_key(VOLATILITY_PROFILE_KEY).unwrap();
        assert_eq!(
            hash(&previous).unwrap(),
            "0ec00e99a41ed39a280f165e9f6b50fdb7b6c918bd0fd2e5e632fb7ac2a45aac"
        );
        let selected = profile_for_key(REASSESSMENT_PROFILE_KEY).unwrap();
        assert_eq!(
            hash(&selected).unwrap(),
            "e7b23b34460727ecf5693774e63f92fa0d04b2737c76b2e01a644c6b66eafa80"
        );
        let selection = AgentSelection {
            profile_key: selected.key.into(),
            profile_sha256: hash(&selected).unwrap(),
        };
        selection.validate().unwrap();
        assert_eq!(selection.context_version(), REASSESSMENT_CONTEXT_VERSION);
        let mut expected = serde_json::to_value(&previous).unwrap();
        expected["key"] = serde_json::json!(REASSESSMENT_PROFILE_KEY);
        expected["context_version"] = serde_json::json!(REASSESSMENT_CONTEXT_VERSION);
        expected["instructions"] = serde_json::json!(format!(
            "{} {}",
            previous.instructions, REASSESSMENT_INSTRUCTION
        ));
        assert_eq!(serde_json::to_value(selected).unwrap(), expected);
    }
    #[test]
    fn entry_profile_is_opt_in_and_preserves_existing_forecast_contract() {
        let old = profile_for_key(REASSESSMENT_PROFILE_KEY).unwrap();
        assert_eq!(
            hash(&old).unwrap(),
            "e7b23b34460727ecf5693774e63f92fa0d04b2737c76b2e01a644c6b66eafa80"
        );
        let new = profile_for_key(ENTRY_REASSESSMENT_PROFILE_KEY).unwrap();
        let selection = AgentSelection {
            profile_key: new.key.into(),
            profile_sha256: hash(&new).unwrap(),
        };
        selection.validate().unwrap();
        assert_eq!(
            selection.context_version(),
            ENTRY_REASSESSMENT_CONTEXT_VERSION
        );
        let mut expected = serde_json::to_value(&old).unwrap();
        expected["key"] = serde_json::json!(ENTRY_REASSESSMENT_PROFILE_KEY);
        expected["context_version"] = serde_json::json!(ENTRY_REASSESSMENT_CONTEXT_VERSION);
        expected["instructions"] = serde_json::json!(format!(
            "{} {}",
            old.instructions, ENTRY_REASSESSMENT_INSTRUCTION
        ));
        assert_eq!(serde_json::to_value(&new).unwrap(), expected);
        eprintln!("entry profile sha256: {}", hash(&new).unwrap());
    }
    #[test]
    fn calibration_extends_frozen_entry_profile_without_changing_execution_contract() {
        let old = profile_for_key(ENTRY_REASSESSMENT_PROFILE_KEY).unwrap();
        assert_eq!(
            hash(&old).unwrap(),
            "acedf40a871c3308098785e1af739aadc4996b589eb38886e8d7591ec254daeb"
        );
        let new = profile_for_key(CALIBRATION_PROFILE_KEY).unwrap();
        assert!(new.instructions.starts_with(&old.instructions));
        assert_eq!(new.model, old.model);
        assert_eq!(new.attempts_seconds, old.attempts_seconds);
        assert_eq!(new.timeout_seconds, old.timeout_seconds);
        assert_eq!(new.decision_ceiling_seconds, old.decision_ceiling_seconds);
        AgentSelection {
            profile_key: new.key.into(),
            profile_sha256: hash(&new).unwrap(),
        }
        .validate()
        .unwrap();
        assert!(uses_entry_reassessment(new.key));
    }
    #[test]
    fn market_context_profile_preserves_calibration_identity_and_execution_contract() {
        let old = profile_for_key(CALIBRATION_PROFILE_KEY).unwrap();
        assert_eq!(
            hash(&old).unwrap(),
            "ec0c11d5493ce6a4fd3eca18f7081fc068cdbf9336512e293a89cc47088b67eb"
        );
        let new = profile_for_key(MARKET_CONTEXT_PROFILE_KEY).unwrap();
        assert!(new.instructions.starts_with(&old.instructions));
        assert_eq!(new.model, old.model);
        assert_eq!(new.attempts_seconds, old.attempts_seconds);
        assert_eq!(new.timeout_seconds, old.timeout_seconds);
        assert_eq!(new.decision_ceiling_seconds, old.decision_ceiling_seconds);
        assert!(uses_calibration(new.key) && uses_entry_reassessment(new.key));
        let selection = AgentSelection {
            profile_key: new.key.into(),
            profile_sha256: hash(&new).unwrap(),
        };
        selection.validate().unwrap();
        assert_eq!(selection.context_version(), MARKET_CONTEXT_VERSION);
    }
}
