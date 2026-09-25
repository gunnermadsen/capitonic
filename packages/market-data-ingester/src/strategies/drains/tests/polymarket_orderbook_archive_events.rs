use super::{schema, PolymarketOrderbookArchiveEventsDrain, COLUMNS, SPEC};
use crate::domain::{DrainMode, DrainRequest, DrainWorkerStrategy, ExecutionSelector};
use chrono::Utc;

#[test]
fn archive_events_use_the_existing_chunk_drain_contract() {
    let adapter = PolymarketOrderbookArchiveEventsDrain::from_environment().unwrap();
    assert_eq!(SPEC.retention_days, Some(0));
    assert_eq!(SPEC.time_column, "provider_received_at");
    assert_eq!(adapter.descriptor().contract_version, 1);
    assert_eq!(adapter.descriptor().relation.as_ref(), SPEC.relation);
    assert!(adapter.root.is_absolute());
    assert_eq!(adapter.root.file_name().unwrap(), "archive-events");
    let request = DrainRequest {
        strategy_key: SPEC.key.into(),
        cutoff: Utc::now(),
        dry_run: true,
        mode: DrainMode::Drain,
        execution: ExecutionSelector::default(),
    };
    adapter.validate_request(&request).unwrap();
}

#[test]
fn archive_parquet_contains_each_source_column_in_table_order() {
    let parquet = schema();
    assert_eq!(parquet.fields().len(), COLUMNS.len());
    for (field, column) in parquet.fields().iter().zip(COLUMNS) {
        assert_eq!(field.name(), column);
    }
}
