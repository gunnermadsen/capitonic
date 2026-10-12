//! Lossless wire compaction and explicit partial RTDS evidence for the new agent profile.
use super::EvaluationRequest;
use crate::{
    btc::{strategy::BtcFeatureSnapshot, types::RealtimeState},
    market_data_stream::SourceSelector,
};
use chrono::Duration;
use rust_decimal::prelude::ToPrimitive;
use serde_json::{json, Value};

pub fn prepare(
    request: &mut EvaluationRequest,
    state: &RealtimeState,
    snapshot: &BtcFeatureSnapshot,
    sources: &[SourceSelector],
) {
    if !super::uses_execution_evidence(&request.selection.profile_key) {
        return;
    }
    request.context["version"] = json!(request.selection.context_version());
    if super::uses_market_reference(&request.selection.profile_key) {
        let probability = snapshot
            .up_book
            .executable_ask_vwap
            .and_then(|p| p.to_f64())
            .zip(
                snapshot
                    .down_book
                    .executable_ask_vwap
                    .and_then(|p| p.to_f64()),
            )
            .and_then(|(up, down)| super::entry_evidence::prior(up, down));
        request.context["market_reference"] = json!({
            "probability_up": probability,
            "observed_at": snapshot.observed_at,
            "source": "normalized_two_sided_executable_asks",
            "meaning": "up_ask / (up_ask + down_ask), using the request snapshot and the existing execution proxy; not a calibrated settlement probability. Null means unavailable."
        });
    }
    request.context["rtds_contiguous_tail"] = tail(state, snapshot.observed_at, sources);
    // These arrays are not consumed by retained-entry/reassessment logic. Preserve
    // every field, null and timestamp; emit column names only once, not per tick.
    for field in ["rtds_last_60_seconds", "binance_closed_one_second_candles"] {
        if let Some(value) = request.context.get_mut(field) {
            compact(value);
        }
    }
}
fn compact(value: &mut Value) {
    let Some(rows) = value
        .as_array()
        .filter(|rows| !rows.is_empty() && rows.iter().all(Value::is_object))
    else {
        return;
    };
    let columns: std::collections::BTreeSet<_> = rows
        .iter()
        .flat_map(|r| r.as_object().unwrap().keys().cloned())
        .collect();
    let columns: Vec<_> = columns.into_iter().collect();
    let rows: Vec<Vec<Value>> = rows
        .iter()
        .map(|r| columns.iter().map(|key| r[key].clone()).collect())
        .collect();
    *value = json!({"encoding":"columns_rows_v1","columns":columns,"rows":rows});
}
fn tail(
    state: &RealtimeState,
    at: chrono::DateTime<chrono::Utc>,
    sources: &[SourceSelector],
) -> Value {
    let Some(selector) = sources
        .iter()
        .find(|s| s.key == crate::market_data_stream::PRODUCT_CHAINLINK)
    else {
        return json!({"status":"not_selected","observation":null});
    };
    let repository = state.directional_external.rtds();
    let Some(last) = repository.points_as_of(at).last() else {
        return json!({"status":"missing","observation":null});
    };
    let points: Vec<_> = repository
        .points_as_of(at)
        .filter(|p| p.source_timestamp >= last.source_timestamp - Duration::seconds(60))
        .collect();
    let age = (at - last.source_timestamp).num_milliseconds();
    let gaps = points
        .windows(2)
        .filter(|p| p[1].source_timestamp - p[0].source_timestamp != Duration::seconds(1))
        .count();
    let start = points
        .windows(2)
        .rposition(|p| p[1].source_timestamp - p[0].source_timestamp != Duration::seconds(1))
        .map_or(0, |i| i + 1);
    let tail = &points[start..];
    let status = if age > selector.effective_maximum_age_ms() as i64 {
        "stale"
    } else if tail.len() < 3 {
        "insufficient_contiguous_returns"
    } else if tail.len() == 61 {
        "complete_60_seconds"
    } else {
        "shorter_contiguous_window"
    };
    let observation = if matches!(status, "complete_60_seconds" | "shorter_contiguous_window") {
        let prices: Option<Vec<f64>> = tail
            .iter()
            .map(|p| {
                p.price
                    .to_f64()
                    .filter(|p| p.is_finite() && *p > 0.0)
                    .map(f64::ln)
            })
            .collect();
        prices.and_then(|prices| {
            let returns:Vec<_>=prices.windows(2).map(|p|p[1]-p[0]).collect();
            let volatility=crate::btc::directional_features::rolling_volatility(&returns,returns.len(),returns.len(),returns.len())?;
            Some(json!({"sample_volatility_bps":volatility*10000.0,"log_return_bps":(prices.last()?-prices.first()?)*10000.0,"return_count":returns.len(),"effective_window_seconds":(last.source_timestamp-tail[0].source_timestamp).num_seconds(),"source_start":tail[0].source_timestamp,"source_end":last.source_timestamp,"available_at":tail.iter().map(|p|p.available_at).max()}))
        })
    } else {
        None
    };
    json!({"status":status,"source_age_ms":age,"observed_points":points.len(),"gap_count":gaps,"observation":observation,"meaning":"Sample volatility of actual contiguous one-second RTDS midpoint returns only. Shorter windows are noisier and are not the complete 60-second statistic or settlement volatility. No interpolation or forward filling."})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compact_preserves_all_fields_and_reduces_repeated_keys() {
        let original = json!([{"source_timestamp":"a","available_at":"b","price":"100"},{"source_timestamp":"c","available_at":"d","price":"101"}]);
        let mut packed = original.clone();
        compact(&mut packed);
        let columns = packed["columns"].as_array().unwrap();
        let decoded: Vec<Value> = packed["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| {
                Value::Object(
                    columns
                        .iter()
                        .zip(row.as_array().unwrap())
                        .map(|(k, v)| (k.as_str().unwrap().into(), v.clone()))
                        .collect(),
                )
            })
            .collect();
        assert_eq!(json!(decoded), original);
    }
    #[test]
    fn gaps_preserve_unknown_full_window_and_expose_only_causal_contiguous_tail() {
        use crate::btc::rtds_repository::RtdsPoint;
        let at = chrono::DateTime::from_timestamp(1800000000, 0).unwrap();
        let sources =
            vec![
                serde_json::from_value(json!(crate::market_data_stream::PRODUCT_CHAINLINK))
                    .unwrap(),
            ];
        let mut state = RealtimeState::default();
        for second in 0..=60 {
            if second == 30 {
                continue;
            }
            std::sync::Arc::make_mut(&mut state.directional_external.rtds).insert_fixture(
                RtdsPoint {
                    source_timestamp: at - Duration::seconds(60 - second),
                    available_at: at - Duration::seconds(60 - second),
                    price: rust_decimal::Decimal::from(100 + second),
                },
            );
        }
        let value = tail(&state, at, &sources);
        assert_eq!(value["status"], "shorter_contiguous_window");
        assert_eq!(value["gap_count"], 1);
        assert_eq!(value["observation"]["return_count"], 29);
        assert_eq!(
            tail(&state, at + Duration::seconds(30), &sources)["status"],
            "stale"
        );
        assert_eq!(tail(&state, at, &[])["status"], "not_selected");
    }
}
