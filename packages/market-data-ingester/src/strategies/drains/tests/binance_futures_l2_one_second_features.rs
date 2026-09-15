use super::BinanceFuturesL2OneSecondFeaturesDrain;
use crate::{
    domain::{DrainRequest, DrainWorkerStrategy, ExecutionSelector},
    strategies::drains::binance_l2_features::{schema, COLUMNS},
};
use chrono::{Duration, Utc};
#[test]
fn exposes_complete_schema_and_two_week_retention() {
    assert_eq!(schema().fields().len(), COLUMNS.len());
    assert_eq!(schema().field(1).name(), "second_start");
    let adapter = BinanceFuturesL2OneSecondFeaturesDrain::from_environment().unwrap();
    assert_eq!(
        adapter.descriptor().relation.as_ref(),
        "market_data.binance_futures_btcusdt_l2_one_second_features"
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
