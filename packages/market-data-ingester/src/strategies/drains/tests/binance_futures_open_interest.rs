use super::{schema, BinanceFuturesOpenInterestDrain, COLUMNS};
use crate::domain::{DrainRequest, DrainWorkerStrategy, ExecutionSelector};
use chrono::{Duration, Utc};

#[test]
fn exposes_complete_schema_and_two_week_retention() {
    assert_eq!(schema().fields().len(), COLUMNS.len());
    assert_eq!(schema().field(1).name(), "source_timestamp");
    let adapter = BinanceFuturesOpenInterestDrain::from_environment().unwrap();
    assert_eq!(
        adapter.descriptor().relation.as_ref(),
        "market_data.binance_futures_btcusdt_open_interest"
    );
    let request = DrainRequest {
        strategy_key: adapter.descriptor().strategy_key.to_string(),
        cutoff: Utc::now() - Duration::days(13),
        dry_run: true,
        mode: Default::default(),
        execution: ExecutionSelector::default(),
    };
    assert!(adapter.validate_request(&request).is_err());
}
