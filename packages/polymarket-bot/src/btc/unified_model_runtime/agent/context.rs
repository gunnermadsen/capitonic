use crate::btc::{strategy::BtcFeatureSnapshot, types::RealtimeState};
use serde_json::{json, Value};

/// Read existing bounded repositories at the immutable observation boundary.
/// No historical SSD access, additional subscriptions or consumer-owned caches.
pub fn build_context(
    state: &RealtimeState,
    snapshot: &BtcFeatureSnapshot,
    sources: &[crate::market_data_stream::SourceSelector],
) -> Value {
    let at = snapshot.observed_at;
    let since = at - chrono::Duration::seconds(60);
    let selected_age = |key: &str| {
        sources
            .iter()
            .find(|s| s.key == key)
            .map(|s| s.effective_maximum_age_ms() as i64)
    };
    let rtds: Vec<_> = state.directional_external.rtds().points_as_of(at)
        .filter(|point| point.source_timestamp >= since)
        .map(|point| json!({"source_timestamp":point.source_timestamp,"available_at":point.available_at,"price":point.price}))
        .collect();
    let candles: Vec<_> = state
        .binance_one_second_window
        .completed()
        .iter()
        .filter(|c| {
            c.close_timestamp <= at
                && c.max_received_at <= at
                && c.close_timestamp >= since
                && c.source_complete
        })
        .collect();
    let oracle = state.directional_external.oracle.iter()
        .filter(|p| p.available_at <= at && p.block_timestamp <= at
            && selected_age(crate::market_data_stream::PRODUCT_POLYGON_ORACLE).is_some_and(|age| (at - p.block_timestamp).num_milliseconds() <= age))
        .max_by_key(|p| p.block_timestamp)
        .map(|p| json!({"source_timestamp":p.source_timestamp,"available_at":p.available_at,"price":p.price}));
    let open_interest = state.directional_external.open_interest.iter()
        .filter(|p| p.available_at <= at && p.source_timestamp <= at
            && selected_age(crate::market_data_stream::PRODUCT_BINANCE_OPEN_INTEREST).is_some_and(|age| (at - p.source_timestamp).num_milliseconds() <= age))
        .max_by_key(|p| p.source_timestamp)
        .map(|p| json!({"source_timestamp":p.source_timestamp,"available_at":p.available_at,"sum_open_interest":p.sum_open_interest,"sum_open_interest_value":p.sum_open_interest_value}));
    json!({"version":super::CONTEXT_VERSION,"execution_snapshot":snapshot,
        "resolution_rule":"UP when closing Chainlink BTC/USD >= opening Chainlink BTC/USD; otherwise DOWN",
        "rtds_last_60_seconds":rtds,"binance_closed_one_second_candles":candles,
        "polygon_oracle":oracle,"binance_futures_open_interest":open_interest,
        "twap_context":null,"twap_context_status":"display_only_not_an_inference_source"})
}
