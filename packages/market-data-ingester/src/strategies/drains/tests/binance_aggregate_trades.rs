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
