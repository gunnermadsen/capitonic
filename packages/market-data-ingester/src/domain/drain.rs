use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::PgPool;
use thiserror::Error;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::ExecutionSelector;

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum DrainMode {
    #[default]
    Drain,
    Reconcile,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DrainRequest {
    pub strategy_key: String,
    pub cutoff: DateTime<Utc>,
    pub dry_run: bool,
    pub mode: DrainMode,
    #[serde(default)]
    pub execution: ExecutionSelector,
}

impl DrainMode {
    pub fn removes_source_data(self) -> bool {
        self == Self::Drain
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Drain => "drain",
            Self::Reconcile => "reconcile",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DrainDescriptor {
    pub strategy_key: Arc<str>,
    pub relation: Arc<str>,
    pub contract_version: i32,
}

#[derive(Clone)]
pub struct DrainContext {
    pub pool: PgPool,
    pub job_id: Uuid,
    pub lease_token: Uuid,
    pub worker_id: Arc<str>,
    pub shutdown: CancellationToken,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DrainOutcome {
    pub rows_exported: i64,
    pub rows_removed: i64,
    pub objects_published: i64,
    pub bytes_written: i64,
    pub summary: Value,
}

#[derive(Debug, Error)]
#[error("{message}")]
pub struct DrainExecutionError {
    pub code: &'static str,
    pub message: String,
    pub retryable: bool,
}

impl DrainExecutionError {
    pub fn new(code: &'static str, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            code,
            message: message.into(),
            retryable,
        }
    }
}

#[async_trait]
pub trait DrainWorkerStrategy: Send + Sync {
    fn descriptor(&self) -> &DrainDescriptor;
    fn validate_request(&self, request: &DrainRequest) -> Result<(), DrainExecutionError>;
    async fn execute_drain(
        &self,
        context: DrainContext,
        request: DrainRequest,
    ) -> Result<DrainOutcome, DrainExecutionError>;
}

#[cfg(test)]
mod tests {
    use super::{DrainMode, DrainRequest};

    #[test]
    fn destructive_fields_must_be_explicit() {
        let omitted: Result<DrainRequest, _> = serde_json::from_value(serde_json::json!({
            "strategy_key": "dataset",
            "cutoff": "2026-01-01T00:00:00Z"
        }));
        assert!(omitted.is_err());
        let omitted_dry_run: Result<DrainRequest, _> = serde_json::from_value(serde_json::json!({
            "strategy_key": "dataset",
            "cutoff": "2026-01-01T00:00:00Z",
            "mode": "drain"
        }));
        assert!(omitted_dry_run.is_err());
        let explicit: DrainRequest = serde_json::from_value(serde_json::json!({
            "strategy_key": "dataset",
            "cutoff": "2026-01-01T00:00:00Z",
            "mode": "drain",
            "dry_run": false
        }))
        .unwrap();
        assert!(explicit.mode.removes_source_data());
    }

    #[test]
    fn reconcile_mode_is_copy_only() {
        let request: DrainRequest = serde_json::from_value(serde_json::json!({
            "strategy_key": "dataset",
            "cutoff": "2026-01-01T00:00:00Z",
            "mode": "reconcile",
            "dry_run": false
        }))
        .unwrap();
        assert_eq!(request.mode, DrainMode::Reconcile);
        assert!(!request.mode.removes_source_data());
    }
}
