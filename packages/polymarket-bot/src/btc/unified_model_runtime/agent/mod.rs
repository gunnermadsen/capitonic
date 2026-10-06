//! Asynchronous decision providers within UMR. Process runners alone own execution.
pub mod auth;
mod context;
mod provider;
mod session;
mod usage;
use anyhow::{ensure, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
pub use context::build_context;
pub use provider::OpenAiProvider;
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
    pub instructions: &'static str,
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
        instructions: "Forecast the UP or DOWN settlement of this BTC five-minute market using only the supplied causal observations. Use the exact market resolution rule. Attempt a direction even when uncertain; mistakes in this paper process are acceptable. Return JSON only: direction (up/down), probability_up (0..1), confidence (0..1, self-assessed evidence strength, not a calibrated probability), reason_codes (1..8 short snake_case identifiers). UP requires probability_up >= 0.5; DOWN requires probability_up < 0.5. Distinguish your forecast from the market price. Missing optional context is not evidence of zero. Do not invent observations, browse, or submit orders.",
        attempts_seconds: [45, 75, 105],
        decision_ceiling_seconds: 120,
        timeout_seconds: 25,
        context_version: CONTEXT_VERSION,
    }
}

pub fn hash(value: &impl Serialize) -> Result<String> {
    Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(value)?)))
}

impl AgentSelection {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.profile_key == PROFILE_KEY, "unsupported agent profile");
        ensure!(
            self.profile_sha256 == hash(&profile())?,
            "agent profile checksum mismatch"
        );
        Ok(())
    }
    pub fn identity(&self) -> super::super::directional_model::RuntimeModelSelection {
        // The existing identity fields describe the selected inference package, not ML weights.
        super::super::directional_model::RuntimeModelSelection {
            model_key: self.profile_key.clone(),
            artifact_sha256: self.profile_sha256.clone(),
            feature_schema_sha256: format!("{:x}", Sha256::digest(CONTEXT_VERSION.as_bytes())),
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
}
