use super::BinanceSpotL2SnapshotsDrain;
use crate::domain::{DrainRequest, DrainWorkerStrategy, ExecutionSelector};
use chrono::{Duration, Utc};
#[test]
fn accepts_one_day_retention_and_rejects_active_tail() {
    let adapter = BinanceSpotL2SnapshotsDrain::from_environment().unwrap();
    let request = DrainRequest {
        strategy_key: adapter.descriptor().strategy_key.to_string(),
        cutoff: Utc::now() - Duration::hours(23),
        dry_run: true,
        mode: Default::default(),
        execution: ExecutionSelector::default(),
    };
    assert!(adapter.validate_request(&request).is_err());
    let request = DrainRequest {
        cutoff: Utc::now() - Duration::days(2),
        ..request
    };
    assert!(adapter.validate_request(&request).is_ok());
}
