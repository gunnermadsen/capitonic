use super::BtcFeatureSnapshotsDrain;
use crate::domain::{DrainRequest, DrainWorkerStrategy, ExecutionSelector};
use chrono::{Duration, Utc};
#[test]
fn permits_draining_all_closed_feature_chunks() {
    let adapter = BtcFeatureSnapshotsDrain::from_environment().unwrap();
    let request = DrainRequest {
        strategy_key: adapter.descriptor().strategy_key.to_string(),
        cutoff: Utc::now() - Duration::minutes(1),
        dry_run: true,
        mode: Default::default(),
        execution: ExecutionSelector::default(),
    };
    assert!(adapter.validate_request(&request).is_ok());
    let future_request = DrainRequest {
        cutoff: Utc::now() + Duration::minutes(1),
        ..request
    };
    assert!(adapter.validate_request(&future_request).is_err());
}
