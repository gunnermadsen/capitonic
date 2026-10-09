//! Exchange-neutral order application contract. HTTP never interprets a trading configuration.
use crate::http::HttpError;
use async_trait::async_trait;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyntheticPrediction {
    pub probability_up: Decimal,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Up,
    Down,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateInput {
    pub member_id: String,
    pub market_id: String,
    pub outcome: Outcome,
    pub synthetic_prediction: SyntheticPrediction,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvaluationRequest {
    pub version: u32,
    pub candidate: CandidateInput,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecuteRequest {
    pub version: u32,
    pub candidate: CandidateInput,
    pub idempotency_key: Uuid,
}
impl EvaluationRequest {
    pub fn validate(&self) -> Result<(), HttpError> {
        if self.version != 1
            || self.candidate.member_id.is_empty()
            || self.candidate.member_id.len() > 128
            || self.candidate.market_id.is_empty()
            || self.candidate.market_id.len() > 256
            || !(Decimal::ZERO..=Decimal::ONE)
                .contains(&self.candidate.synthetic_prediction.probability_up)
        {
            return Err(HttpError::bad_request("invalid version-1 order candidate"));
        }
        Ok(())
    }
    pub fn identity(&self) -> Result<String, HttpError> {
        use sha2::{Digest, Sha256};
        let mut canonical = self.clone();
        canonical.candidate.synthetic_prediction.probability_up = canonical
            .candidate
            .synthetic_prediction
            .probability_up
            .normalize();
        Ok(format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(&canonical)
                    .map_err(|_| HttpError::bad_request("invalid candidate"))?
            )
        ))
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Receipt {
    pub version: u32,
    pub process_id: Uuid,
    pub operation_id: Uuid,
    pub order_id: Option<String>,
    pub origin: String,
    pub config_sha256: String,
    pub status: String,
    pub policy_results: Value,
    pub evidence: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preparation: Option<Value>,
}
#[async_trait]
pub trait OrderApplication: Send + Sync {
    async fn preview(
        &self,
        process_id: Uuid,
        request: EvaluationRequest,
        prepare: bool,
    ) -> Result<Receipt, HttpError>;
    async fn execute(
        &self,
        process_id: Uuid,
        request: ExecuteRequest,
    ) -> Result<Receipt, HttpError>;
    async fn order(&self, process_id: Uuid, order_id: String) -> Result<Receipt, HttpError>;
}

/// Validate durable idempotency before consulting the current process definition.
pub fn validate_replay(
    order: &crate::models::OrderRecord,
    process_id: Uuid,
    request: &EvaluationRequest,
) -> Result<(), HttpError> {
    if order.request.process_id != Some(process_id)
        || order
            .request
            .metadata
            .pointer("/polymarket_execution/api_request_sha256")
            .and_then(Value::as_str)
            != Some(request.identity()?.as_str())
    {
        return Err(HttpError::conflict("idempotency identity conflict"));
    }
    Ok(())
}
