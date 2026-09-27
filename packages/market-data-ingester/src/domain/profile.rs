use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use super::IngesterStrategyKey;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesiredState {
    Running,
    Stopped,
}

impl DesiredState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Stopped => "stopped",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservedState {
    Starting,
    Running,
    Degraded,
    Restarting,
    Stopping,
    Stopped,
    Failed,
    Unsupported,
}

impl ObservedState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Degraded => "degraded",
            Self::Restarting => "restarting",
            Self::Stopping => "stopping",
            Self::Stopped => "stopped",
            Self::Failed => "failed",
            Self::Unsupported => "unsupported",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthStatus {
    Unknown,
    Healthy,
    Degraded,
    Unhealthy,
}

impl HealthStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Healthy => "healthy",
            Self::Degraded => "degraded",
            Self::Unhealthy => "unhealthy",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct IngesterProfile {
    pub strategy_key: IngesterStrategyKey,
    pub config_schema_version: i32,
    pub config: Value,
    pub desired_state: DesiredState,
    pub desired_generation: i64,
    pub observed_state: ObservedState,
    pub health_status: HealthStatus,
    pub applied_generation: Option<i64>,
    pub checkpoint_schema_version: i32,
    pub checkpoint: Value,
    pub lease_owner: Option<String>,
    pub lease_token: Option<Uuid>,
    pub lease_expires_at: Option<DateTime<Utc>>,
    pub heartbeat_at: Option<DateTime<Utc>>,
    pub started_at: Option<DateTime<Utc>>,
    pub stopped_at: Option<DateTime<Utc>>,
    pub last_source_event_at: Option<DateTime<Utc>>,
    pub last_provider_available_at: Option<DateTime<Utc>>,
    pub last_persisted_at: Option<DateTime<Utc>>,
    pub source_watermark: Option<DateTime<Utc>>,
    pub availability_watermark: Option<DateTime<Utc>>,
    pub consecutive_failures: i32,
    pub restart_count: i64,
    pub last_error_code: Option<String>,
    pub last_error_message: Option<String>,
    pub last_error_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl IngesterProfile {
    pub fn lease_is_current(&self, now: DateTime<Utc>) -> bool {
        self.lease_token.is_some_and(|_| {
            self.lease_expires_at
                .is_some_and(|expires_at| expires_at > now)
        })
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone, Utc};
    use serde_json::json;
    use uuid::Uuid;

    use super::{DesiredState, HealthStatus, IngesterProfile, ObservedState};
    use crate::domain::IngesterStrategyKey;

    #[test]
    fn route_lease_requires_a_token_and_future_expiry() {
        let now = Utc.timestamp_opt(1_800_000_000, 0).single().unwrap();
        let mut profile = IngesterProfile {
            strategy_key: IngesterStrategyKey::BinanceSpotBtcusdtOneSecondOhlcv,
            config_schema_version: 1,
            config: json!({}),
            desired_state: DesiredState::Running,
            desired_generation: 1,
            observed_state: ObservedState::Running,
            health_status: HealthStatus::Healthy,
            applied_generation: Some(1),
            checkpoint_schema_version: 1,
            checkpoint: json!({}),
            lease_owner: Some("ingester-worker-1".to_owned()),
            lease_token: Some(Uuid::new_v4()),
            lease_expires_at: Some(now + Duration::seconds(1)),
            heartbeat_at: Some(now),
            started_at: Some(now),
            stopped_at: None,
            last_source_event_at: None,
            last_provider_available_at: None,
            last_persisted_at: None,
            source_watermark: None,
            availability_watermark: None,
            consecutive_failures: 0,
            restart_count: 0,
            last_error_code: None,
            last_error_message: None,
            last_error_at: None,
            created_at: now,
            updated_at: now,
        };

        assert!(profile.lease_is_current(now));
        assert!(!profile.lease_is_current(now + Duration::seconds(1)));

        profile.lease_expires_at = Some(now + Duration::seconds(1));
        profile.lease_token = None;
        assert!(!profile.lease_is_current(now));

        profile.lease_token = Some(Uuid::new_v4());
        profile.lease_expires_at = None;
        assert!(!profile.lease_is_current(now));
    }
}
