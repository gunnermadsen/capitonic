use chrono::{Duration, Utc};

use super::BinanceAggregateTradesDrain;
use crate::domain::{DrainMode, DrainRequest, DrainWorkerStrategy, ExecutionSelector};

#[test]
fn only_the_registered_relation_is_accepted() {
    let adapter = BinanceAggregateTradesDrain::from_environment().unwrap();
    let request = DrainRequest {
        strategy_key: "market_data.anything".into(),
        cutoff: Utc::now(),
        dry_run: true,
        mode: Default::default(),
        execution: ExecutionSelector::default(),
    };
    assert_eq!(
        adapter.validate_request(&request).unwrap_err().code,
        "drain_strategy_mismatch"
    );
}

#[test]
fn parquet_schema_contains_every_source_column() {
    let schema = crate::strategies::drains::binance_schema::schema();
    assert_eq!(schema.fields().len(), 16);
    assert_eq!(schema.field(0).name(), "source");
    assert_eq!(schema.field(15).name(), "ingested_at");
}

#[test]
fn partial_historical_cutoff_is_valid_for_aggregate_trades() {
    let adapter = BinanceAggregateTradesDrain::from_environment().unwrap();
    let request = DrainRequest {
        strategy_key: "binance_spot_btcusdt_aggregate_trades".into(),
        cutoff: Utc::now() - Duration::days(2),
        dry_run: false,
        mode: DrainMode::Drain,
        execution: ExecutionSelector::default(),
    };
    adapter.validate_request(&request).unwrap();
}

#[tokio::test]
async fn empty_closed_chunk_can_publish_verified_parquet() {
    use crate::strategies::drains::{
        binance_schema,
        common::{finish_writer, publish_file, start_writer, verify_existing, Publication},
    };
    use uuid::Uuid;

    let root = std::env::temp_dir().join(format!("aggregate-empty-drain-{}", Uuid::new_v4()));
    tokio::fs::create_dir_all(&root).await.unwrap();
    let staging = root.join("empty.tmp");
    let (sender, writer) = start_writer(staging.clone(), binance_schema::schema());
    finish_writer(sender, writer).await.unwrap();
    let (sha256, byte_size) = publish_file(&staging, &root, "empty.parquet", 0)
        .await
        .unwrap();
    assert!(byte_size > 0);
    let publication = Publication {
        object_id: Uuid::new_v4(),
        row_count: 0,
        relative_path: "empty.parquet".into(),
        sha256,
        byte_size,
        status: "published".into(),
    };
    verify_existing(&root, &publication).await.unwrap();
    tokio::fs::remove_dir_all(root).await.unwrap();
}
