use super::{export_slices, BtcFeatureSnapshotsDrain};
use crate::domain::{DrainRequest, DrainWorkerStrategy, ExecutionSelector};
use chrono::{Duration, TimeZone, Utc};
#[test]
fn export_slices_cover_chunk_without_overlap() {
    let start = Utc.with_ymd_and_hms(2026, 9, 15, 0, 0, 0).unwrap();
    let end = start + Duration::minutes(37);
    assert_eq!(
        export_slices(start, end),
        vec![
            (start, start + Duration::minutes(15)),
            (start + Duration::minutes(15), start + Duration::minutes(30)),
            (start + Duration::minutes(30), end),
        ]
    );
}
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
