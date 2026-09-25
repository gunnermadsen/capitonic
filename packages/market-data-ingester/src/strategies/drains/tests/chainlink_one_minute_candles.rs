use super::ChainlinkOneMinuteCandlesDrain;
use crate::domain::{DrainRequest, DrainWorkerStrategy, ExecutionSelector};
use chrono::{Duration, Utc};
#[test]
fn preserves_two_day_retention() {
    let adapter = ChainlinkOneMinuteCandlesDrain::from_environment().unwrap();
    let request = DrainRequest {
        strategy_key: adapter.descriptor().strategy_key.to_string(),
        cutoff: Utc::now() - Duration::days(1),
        dry_run: true,
        mode: Default::default(),
        execution: ExecutionSelector::default(),
    };
    assert!(adapter.validate_request(&request).is_err());
    assert!(adapter
        .validate_request(&DrainRequest {
            cutoff: Utc::now() - Duration::days(3),
            ..request
        })
        .is_ok());
}
