use crate::btc::{strategy::BtcFeatureSnapshot, types::RealtimeState};
use serde_json::{json, Value};

pub fn build_volatility_context(
    state: &RealtimeState,
    snapshot: &BtcFeatureSnapshot,
    sources: &[crate::market_data_stream::SourceSelector],
) -> Value {
    let mut context = build_settlement_context(state, snapshot, sources);
    context["version"] = json!(super::VOLATILITY_CONTEXT_VERSION);
    let observation = rtds_volatility(state, snapshot.observed_at, sources);
    context["rtds_volatility"] = json!({
        "source": crate::market_data_stream::PRODUCT_CHAINLINK,
        "meaning": "Supporting RTDS midpoint indicator, not settlement-price volatility or a probability. Sample standard deviation of one-second log returns over 60 seconds, in basis points; not annualized. Null means unknown, not zero.",
        "window_seconds": 60,
        "observation": observation,
    });
    context
}

fn rtds_volatility(
    state: &RealtimeState,
    at: chrono::DateTime<chrono::Utc>,
    sources: &[crate::market_data_stream::SourceSelector],
) -> Option<Value> {
    use rust_decimal::prelude::ToPrimitive;
    let selector = sources
        .iter()
        .find(|s| s.key == crate::market_data_stream::PRODUCT_CHAINLINK)?;
    let repository = state.directional_external.rtds();
    let last = repository.points_as_of(at).last()?;
    if (at - last.source_timestamp).num_milliseconds() > selector.effective_maximum_age_ms() as i64
    {
        return None;
    }
    let start = last.source_timestamp - chrono::Duration::seconds(60);
    let points: Vec<_> = repository
        .points_as_of(at)
        .filter(|p| p.source_timestamp >= start)
        .collect();
    // Do not forward-fill missing ticks or treat an incomplete window as quiet.
    if points.len() != 61
        || points.windows(2).any(|pair| {
            pair[1].source_timestamp - pair[0].source_timestamp != chrono::Duration::seconds(1)
        })
    {
        return None;
    }
    let logs: Option<Vec<_>> = points
        .iter()
        .map(|p| {
            p.price
                .to_f64()
                .filter(|p| p.is_finite() && *p > 0.0)
                .map(f64::ln)
        })
        .collect();
    let returns: Vec<_> = logs?.windows(2).map(|p| p[1] - p[0]).collect();
    let bps =
        crate::btc::directional_features::rolling_volatility(&returns, 60, 60, 60)? * 10_000.0;
    bps.is_finite().then(|| {
        json!({"value_bps":bps,"return_count":60,
        "source_start":start,"source_end":last.source_timestamp,
        "available_at":points.iter().map(|p| p.available_at).max()})
    })
}

/// Settlement evidence is optional context, never a new execution/readiness gate.
pub fn build_settlement_context(
    state: &RealtimeState,
    snapshot: &BtcFeatureSnapshot,
    sources: &[crate::market_data_stream::SourceSelector],
) -> Value {
    let at = snapshot.observed_at;
    let selected = sources
        .iter()
        .find(|source| source.key == crate::market_data_stream::PRODUCT_TWAP);
    let causal = |point: &&crate::btc::types::ChainlinkTwap60Point| {
        selected.is_some()
            && point.price > rust_decimal::Decimal::ZERO
            && point.source_timestamp <= at
            && point.available_at <= at
    };
    let opening = state
        .chainlink_twap_60
        .iter()
        .filter(causal)
        .find(|point| point.source_timestamp == snapshot.window_start);
    let current = state
        .chainlink_twap_60
        .iter()
        .filter(causal)
        .filter(|point| {
            selected.is_some_and(|source| {
                (at - point.source_timestamp).num_milliseconds()
                    <= source.effective_maximum_age_ms() as i64
            })
        })
        .max_by_key(|point| point.source_timestamp);
    let observation = |point: &crate::btc::types::ChainlinkTwap60Point| json!({"price":point.price,"source_timestamp":point.source_timestamp,"available_at":point.available_at});
    let mut context = build_context(state, snapshot, sources);
    context["version"] = json!(super::SETTLEMENT_CONTEXT_VERSION);
    context["resolution_rule"] = json!(super::SETTLEMENT_RULE);
    context["twap_context"] = json!({"source":crate::market_data_stream::PRODUCT_TWAP,"window_seconds":60,
        "opening":opening.map(observation),"current":current.map(observation)});
    context["twap_context_status"] = json!(if selected.is_none() {
        "not_selected"
    } else if opening.is_none() || current.is_none() {
        "incomplete_unknown_observations"
    } else {
        "available"
    });
    context
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::btc::rtds_repository::RtdsPoint;
    use chrono::{Duration, TimeZone, Utc};
    use rust_decimal::Decimal;

    #[test]
    fn volatility_reuses_causal_history_and_distinguishes_unknown_from_zero() {
        let at = Utc.with_ymd_and_hms(2026, 10, 6, 12, 1, 0).unwrap();
        let sources =
            vec![
                serde_json::from_value(json!(crate::market_data_stream::PRODUCT_CHAINLINK))
                    .unwrap(),
            ];
        let history = |flat: bool, gap: Option<i64>| {
            let mut state = RealtimeState::default();
            for second in 0..=60 {
                if gap == Some(second) {
                    continue;
                }
                let timestamp = at - Duration::seconds(60 - second);
                std::sync::Arc::make_mut(&mut state.directional_external.rtds).insert_fixture(
                    RtdsPoint {
                        source_timestamp: timestamp,
                        available_at: timestamp,
                        price: Decimal::from(if flat || second % 2 == 0 { 100 } else { 101 }),
                    },
                );
            }
            state
        };
        let mut state = history(false, None);
        let value = rtds_volatility(&state, at, &sources).unwrap();
        let expected = 1.01_f64.ln() * (60.0_f64 / 59.0).sqrt() * 10_000.0;
        assert!((value["value_bps"].as_f64().unwrap() - expected).abs() < 1e-8);
        assert_eq!(value["return_count"], 60);
        for (source_timestamp, available_at) in [
            (at + Duration::seconds(1), at),
            (at + Duration::seconds(2), at + Duration::seconds(3)),
        ] {
            std::sync::Arc::make_mut(&mut state.directional_external.rtds).insert_fixture(
                RtdsPoint {
                    source_timestamp,
                    available_at,
                    price: Decimal::from(500),
                },
            );
        }
        assert_eq!(rtds_volatility(&state, at, &sources).unwrap(), value);
        let mut late = history(false, Some(30));
        std::sync::Arc::make_mut(&mut late.directional_external.rtds).insert_fixture(RtdsPoint {
            source_timestamp: at - Duration::seconds(30),
            available_at: at + Duration::seconds(1),
            price: Decimal::from(100),
        });
        assert!(rtds_volatility(&late, at, &sources).is_none());
        assert!(rtds_volatility(&history(false, Some(30)), at, &sources).is_none());
        assert!(
            rtds_volatility(&history(false, None), at + Duration::seconds(11), &sources).is_none()
        );
        assert!(rtds_volatility(&state, at, &[]).is_none());
        assert!(rtds_volatility(&RealtimeState::default(), at, &sources).is_none());
        assert_eq!(
            rtds_volatility(&history(true, None), at, &sources).unwrap()["value_bps"],
            0.0
        );
    }
}
