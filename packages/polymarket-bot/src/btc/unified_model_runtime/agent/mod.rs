//! Asynchronous decision providers within UMR. Process runners alone own execution.
pub mod auth;
pub(crate) mod calibration;
mod compact_context;
mod context;
pub(super) mod diagnostics;
pub(crate) mod entry_evidence;
mod market_context;
mod prompt;
mod provider;
mod reassessment;
mod session;
mod usage;
use anyhow::{ensure, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
pub use compact_context::prepare as prepare_execution_context;
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
pub const FRESH_EVIDENCE_PROFILE_KEY: &str = "btc-5m-openai-agent-fresh-evidence-v1";
pub const FRESH_EVIDENCE_CONTEXT_VERSION: &str = "capitonic-btc-agent-fresh-evidence-v1";
pub const EXECUTION_EVIDENCE_PROFILE_KEY: &str = "btc-5m-openai-agent-execution-evidence-v1";
pub const EXECUTION_EVIDENCE_CONTEXT_VERSION: &str = "capitonic-btc-agent-execution-evidence-v1";
pub const MARKET_REFERENCE_PROFILE_KEY: &str = "btc-5m-openai-agent-market-reference-v1";
pub const MARKET_REFERENCE_CONTEXT_VERSION: &str = "capitonic-btc-agent-market-reference-v1";
pub const MARKET_REFERENCE_SOL_PROFILE_KEY: &str = "btc-5m-openai-agent-market-reference-sol-v1";
pub fn uses_market_reference(key: &str) -> bool {
    matches!(
        key,
        MARKET_REFERENCE_PROFILE_KEY | MARKET_REFERENCE_SOL_PROFILE_KEY
    )
}
pub fn uses_execution_evidence(key: &str) -> bool {
    matches!(
        key,
        EXECUTION_EVIDENCE_PROFILE_KEY
            | MARKET_REFERENCE_PROFILE_KEY
            | MARKET_REFERENCE_SOL_PROFILE_KEY
    )
}

pub fn independent_forecast(key: &str) -> bool {
    matches!(
        key,
        FRESH_EVIDENCE_PROFILE_KEY
            | EXECUTION_EVIDENCE_PROFILE_KEY
            | MARKET_REFERENCE_PROFILE_KEY
            | MARKET_REFERENCE_SOL_PROFILE_KEY
    )
}

pub fn uses_market_context(key: &str) -> bool {
    matches!(
        key,
        MARKET_CONTEXT_PROFILE_KEY
            | FRESH_EVIDENCE_PROFILE_KEY
            | EXECUTION_EVIDENCE_PROFILE_KEY
            | MARKET_REFERENCE_PROFILE_KEY
            | MARKET_REFERENCE_SOL_PROFILE_KEY
    )
}

pub const MARKET_CONTEXT_VERSION: &str = "capitonic-btc-agent-market-context-v1";

pub fn uses_calibration(key: &str) -> bool {
    matches!(
        key,
        CALIBRATION_PROFILE_KEY
            | MARKET_CONTEXT_PROFILE_KEY
            | FRESH_EVIDENCE_PROFILE_KEY
            | EXECUTION_EVIDENCE_PROFILE_KEY
            | MARKET_REFERENCE_PROFILE_KEY
            | MARKET_REFERENCE_SOL_PROFILE_KEY
    )
}

pub fn uses_entry_reassessment(key: &str) -> bool {
    matches!(
        key,
        ENTRY_REASSESSMENT_PROFILE_KEY
            | CALIBRATION_PROFILE_KEY
            | MARKET_CONTEXT_PROFILE_KEY
            | FRESH_EVIDENCE_PROFILE_KEY
            | EXECUTION_EVIDENCE_PROFILE_KEY
            | MARKET_REFERENCE_PROFILE_KEY
            | MARKET_REFERENCE_SOL_PROFILE_KEY
    )
}

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
    pub prompt: String,
    pub model: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentProfile {
    pub version: &'static str,
    pub key: &'static str,
    pub attempts_seconds: [i64; 3],
    pub decision_ceiling_seconds: i64,
    pub timeout_seconds: u64,
    pub context_version: &'static str,
}

pub fn profile() -> AgentProfile {
    AgentProfile {
        version: BRIDGE_VERSION,
        key: PROFILE_KEY,
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
        }
        ENTRY_REASSESSMENT_PROFILE_KEY => {
            selected = profile_for_key(REASSESSMENT_PROFILE_KEY)?;
            selected.key = ENTRY_REASSESSMENT_PROFILE_KEY;
            selected.context_version = ENTRY_REASSESSMENT_CONTEXT_VERSION;
        }
        CALIBRATION_PROFILE_KEY => {
            selected = profile_for_key(ENTRY_REASSESSMENT_PROFILE_KEY)?;
            selected.key = CALIBRATION_PROFILE_KEY;
            selected.context_version = CALIBRATION_CONTEXT_VERSION;
        }
        MARKET_CONTEXT_PROFILE_KEY => {
            selected = profile_for_key(CALIBRATION_PROFILE_KEY)?;
            selected.key = MARKET_CONTEXT_PROFILE_KEY;
            selected.context_version = MARKET_CONTEXT_VERSION;
        }
        FRESH_EVIDENCE_PROFILE_KEY => {
            selected = profile_for_key(MARKET_CONTEXT_PROFILE_KEY)?;
            selected.key = FRESH_EVIDENCE_PROFILE_KEY;
            selected.context_version = FRESH_EVIDENCE_CONTEXT_VERSION;
        }
        EXECUTION_EVIDENCE_PROFILE_KEY => {
            selected = profile_for_key(FRESH_EVIDENCE_PROFILE_KEY)?;
            selected.key = EXECUTION_EVIDENCE_PROFILE_KEY;
            selected.context_version = EXECUTION_EVIDENCE_CONTEXT_VERSION;
        }
        MARKET_REFERENCE_PROFILE_KEY => {
            selected = profile_for_key(EXECUTION_EVIDENCE_PROFILE_KEY)?;
            selected.key = MARKET_REFERENCE_PROFILE_KEY;
            selected.context_version = MARKET_REFERENCE_CONTEXT_VERSION;
        }
        MARKET_REFERENCE_SOL_PROFILE_KEY => {
            selected = profile_for_key(MARKET_REFERENCE_PROFILE_KEY)?;
            selected.key = MARKET_REFERENCE_SOL_PROFILE_KEY;
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
        if uses_market_reference(&self.profile_key) {
            MARKET_REFERENCE_CONTEXT_VERSION
        } else if self.profile_key == EXECUTION_EVIDENCE_PROFILE_KEY {
            EXECUTION_EVIDENCE_CONTEXT_VERSION
        } else if self.profile_key == FRESH_EVIDENCE_PROFILE_KEY {
            FRESH_EVIDENCE_CONTEXT_VERSION
        } else if self.profile_key == MARKET_CONTEXT_PROFILE_KEY {
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
            "8a80a01ae9fb1db22b6673019472c394b04fbfbc2fa6f5321506be9a146eaf63"
        );
        let selected = profile_for_key(SETTLEMENT_PROFILE_KEY).unwrap();
        let selection = AgentSelection {
            profile_key: selected.key.into(),
            profile_sha256: hash(&selected).unwrap(),
            prompt: "Forecast from the supplied observations.".into(),
            model: "test-model".into(),
        };
        selection.validate().unwrap();
        assert_ne!(selection.profile_sha256, hash(&original).unwrap());
        assert_eq!(selected.attempts_seconds, original.attempts_seconds);
        assert_eq!(
            selected.decision_ceiling_seconds,
            original.decision_ceiling_seconds
        );
        assert_eq!(selected.timeout_seconds, original.timeout_seconds);
        assert_eq!(selection.context_version(), SETTLEMENT_CONTEXT_VERSION);
        assert_eq!(
            hash(&selected).unwrap(),
            "c83b5472e93783fc9774294a9dc6e2334149e2759a563c133dd331477173b2ea"
        );
        let volatility = profile_for_key(VOLATILITY_PROFILE_KEY).unwrap();
        let volatility_selection = AgentSelection {
            profile_key: volatility.key.into(),
            profile_sha256: hash(&volatility).unwrap(),
            prompt: "Forecast from the supplied observations.".into(),
            model: "test-model".into(),
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
            prompt: "Forecast from the supplied observations.".into(),
            model: "test-model".into(),
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
    fn reassessment_only_changes_profile_identity_and_context() {
        let previous = profile_for_key(VOLATILITY_PROFILE_KEY).unwrap();
        assert_eq!(
            hash(&previous).unwrap(),
            "e5cddf5c3ea5709d37a0f156b4d365ac884fbd5e33415092427eaf16ddbf5ff8"
        );
        let selected = profile_for_key(REASSESSMENT_PROFILE_KEY).unwrap();
        assert_eq!(
            hash(&selected).unwrap(),
            "bf01e5d1ce34b2075baadbdf1f5a0b80b1854c4cc0996dfa32c253404bed1a27"
        );
        let selection = AgentSelection {
            profile_key: selected.key.into(),
            profile_sha256: hash(&selected).unwrap(),
            prompt: "Forecast from the supplied observations.".into(),
            model: "test-model".into(),
        };
        selection.validate().unwrap();
        assert_eq!(selection.context_version(), REASSESSMENT_CONTEXT_VERSION);
        let mut expected = serde_json::to_value(&previous).unwrap();
        expected["key"] = serde_json::json!(REASSESSMENT_PROFILE_KEY);
        expected["context_version"] = serde_json::json!(REASSESSMENT_CONTEXT_VERSION);
        assert_eq!(serde_json::to_value(selected).unwrap(), expected);
    }
    #[test]
    fn entry_profile_is_opt_in_and_preserves_existing_forecast_contract() {
        let old = profile_for_key(REASSESSMENT_PROFILE_KEY).unwrap();
        assert_eq!(
            hash(&old).unwrap(),
            "bf01e5d1ce34b2075baadbdf1f5a0b80b1854c4cc0996dfa32c253404bed1a27"
        );
        let new = profile_for_key(ENTRY_REASSESSMENT_PROFILE_KEY).unwrap();
        let selection = AgentSelection {
            profile_key: new.key.into(),
            profile_sha256: hash(&new).unwrap(),
            prompt: "Forecast from the supplied observations.".into(),
            model: "test-model".into(),
        };
        selection.validate().unwrap();
        assert_eq!(
            selection.context_version(),
            ENTRY_REASSESSMENT_CONTEXT_VERSION
        );
        let mut expected = serde_json::to_value(&old).unwrap();
        expected["key"] = serde_json::json!(ENTRY_REASSESSMENT_PROFILE_KEY);
        expected["context_version"] = serde_json::json!(ENTRY_REASSESSMENT_CONTEXT_VERSION);
        assert_eq!(serde_json::to_value(&new).unwrap(), expected);
        eprintln!("entry profile sha256: {}", hash(&new).unwrap());
    }
    #[test]
    fn calibration_extends_frozen_entry_profile_without_changing_execution_contract() {
        let old = profile_for_key(ENTRY_REASSESSMENT_PROFILE_KEY).unwrap();
        assert_eq!(
            hash(&old).unwrap(),
            "c34a46ac7df73d8ac2384e86c048561c8a64ba4057c65134ba90a24e95a81fb1"
        );
        let new = profile_for_key(CALIBRATION_PROFILE_KEY).unwrap();
        assert_eq!(new.attempts_seconds, old.attempts_seconds);
        assert_eq!(new.timeout_seconds, old.timeout_seconds);
        assert_eq!(new.decision_ceiling_seconds, old.decision_ceiling_seconds);
        AgentSelection {
            profile_key: new.key.into(),
            profile_sha256: hash(&new).unwrap(),
            prompt: "Forecast from the supplied observations.".into(),
            model: "test-model".into(),
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
            "58e99811e761118c95c0199b65a34a44ed9eee9c2a0064ca190637f1885797e4"
        );
        let new = profile_for_key(MARKET_CONTEXT_PROFILE_KEY).unwrap();
        assert_eq!(new.attempts_seconds, old.attempts_seconds);
        assert_eq!(new.timeout_seconds, old.timeout_seconds);
        assert_eq!(new.decision_ceiling_seconds, old.decision_ceiling_seconds);
        assert!(uses_calibration(new.key) && uses_entry_reassessment(new.key));
        let selection = AgentSelection {
            profile_key: new.key.into(),
            profile_sha256: hash(&new).unwrap(),
            prompt: "Forecast from the supplied observations.".into(),
            model: "test-model".into(),
        };
        selection.validate().unwrap();
        assert_eq!(selection.context_version(), MARKET_CONTEXT_VERSION);
    }
    #[test]
    fn fresh_profile_preserves_market_profile_and_unrelated_behavior() {
        let original = profile_for_key(MARKET_CONTEXT_PROFILE_KEY).unwrap();
        assert_eq!(
            hash(&original).unwrap(),
            "fe5b6e81fd1617b6f3f200110e1e403111a346b172378b55ee0243256d9fe7ff"
        );
        let fresh = profile_for_key(FRESH_EVIDENCE_PROFILE_KEY).unwrap();
        assert_eq!(fresh.attempts_seconds, original.attempts_seconds);
        assert_eq!(
            fresh.decision_ceiling_seconds,
            original.decision_ceiling_seconds
        );
        assert_eq!(fresh.timeout_seconds, original.timeout_seconds);
        assert!(
            uses_calibration(fresh.key)
                && uses_market_context(fresh.key)
                && uses_entry_reassessment(fresh.key)
        );
        let selection = AgentSelection {
            profile_key: fresh.key.into(),
            profile_sha256: hash(&fresh).unwrap(),
            prompt: "Forecast from the supplied observations.".into(),
            model: "test-model".into(),
        };
        selection.validate().unwrap();
        assert_eq!(selection.context_version(), FRESH_EVIDENCE_CONTEXT_VERSION);
        println!(
            "Fresh evidence profile SHA256: {}",
            selection.profile_sha256
        );
    }
    #[test]
    fn market_reference_profile_preserves_execution_profile_contract() {
        let old = profile_for_key(EXECUTION_EVIDENCE_PROFILE_KEY).unwrap();
        assert_eq!(
            hash(&old).unwrap(),
            "3841bd39b89359b7eb467fa23dc605d723bb36b3f30c6f26f1eddadba1679d2f"
        );
        let new = profile_for_key(MARKET_REFERENCE_PROFILE_KEY).unwrap();
        let selection = AgentSelection {
            profile_key: new.key.into(),
            profile_sha256: hash(&new).unwrap(),
            prompt: "Forecast from the supplied observations.".into(),
            model: "test-model".into(),
        };
        selection.validate().unwrap();
        assert_eq!(
            selection.context_version(),
            MARKET_REFERENCE_CONTEXT_VERSION
        );
        assert!(
            independent_forecast(new.key)
                && uses_execution_evidence(new.key)
                && uses_market_context(new.key)
                && uses_calibration(new.key)
                && uses_entry_reassessment(new.key)
        );
        let mut unchanged = new.clone();
        unchanged.key = old.key;
        unchanged.context_version = old.context_version;
        assert_eq!(hash(&unchanged).unwrap(), hash(&old).unwrap());
        eprintln!(
            "market reference profile sha256: {}",
            selection.profile_sha256
        );
    }
    #[test]
    fn sol_variant_preserves_context_and_schedule() {
        let astra = profile_for_key(MARKET_REFERENCE_PROFILE_KEY).unwrap();
        assert_eq!(
            hash(&astra).unwrap(),
            "bd7ba45b1b23d18e17ed0b5876fa345afe43cb7f57a72eb639f1e54715766ec8"
        );
        let sol = profile_for_key(MARKET_REFERENCE_SOL_PROFILE_KEY).unwrap();
        assert!(
            uses_market_reference(sol.key)
                && uses_execution_evidence(sol.key)
                && independent_forecast(sol.key)
                && uses_market_context(sol.key)
                && uses_calibration(sol.key)
                && uses_entry_reassessment(sol.key)
        );
        let selection = AgentSelection {
            profile_key: sol.key.into(),
            profile_sha256: hash(&sol).unwrap(),
            prompt: "Forecast from the supplied observations.".into(),
            model: "test-model".into(),
        };
        selection.validate().unwrap();
        assert_eq!(selection.context_version(), astra.context_version);
        let mut comparable = sol.clone();
        comparable.key = astra.key;
        assert_eq!(hash(&comparable).unwrap(), hash(&astra).unwrap());
        eprintln!("SOL profile SHA256: {}", selection.profile_sha256);
    }
}
