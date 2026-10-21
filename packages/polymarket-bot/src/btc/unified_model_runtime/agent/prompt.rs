//! Render configured source references from one existing UMR evaluation snapshot.
use anyhow::{bail, Context, Result};
use serde_json::{Map, Value};

use crate::market_data_stream::{
    PRODUCT_BINANCE_1S, PRODUCT_BINANCE_OPEN_INTEREST, PRODUCT_BOOKS, PRODUCT_CHAINLINK,
    PRODUCT_MARKETS, PRODUCT_POLYGON_ORACLE, PRODUCT_RESOLUTIONS, PRODUCT_TWAP,
};

pub(super) fn render(template: &str, context: &Value) -> Result<String> {
    let source_keys = context["source_keys"]
        .as_array()
        .context("agent context is missing configured source keys")?;
    let products = partition(context)?;
    let mut output = String::with_capacity(template.len());
    let mut remaining = template;
    while let Some(start) = remaining.find("#{") {
        output.push_str(&remaining[..start]);
        let token = &remaining[start + 2..];
        let end = token
            .find('}')
            .context("unterminated prompt product reference")?;
        let key = &token[..end];
        if !source_keys
            .iter()
            .any(|source| source.as_str() == Some(key))
        {
            bail!("prompt product is not configured in sources: {key}");
        }
        let value = products
            .get(key)
            .context("source has no agent prompt binding")?;
        output.push_str(&serde_json::to_string(value)?);
        remaining = &token[end + 1..];
    }
    output.push_str(remaining);
    Ok(output)
}

// Partition the already prepared context; do not fetch, recompute, or forward-fill data.
// Internal context paths remain intact for retained predictions and reassessments.
fn partition(context: &Value) -> Result<Map<String, Value>> {
    let mut remaining = context
        .as_object()
        .context("agent context must be an object")?
        .clone();
    remaining.remove("source_keys");
    let mut snapshot = remaining
        .remove("execution_snapshot")
        .context("agent context is missing execution snapshot")?
        .as_object()
        .context("execution snapshot must be an object")?
        .clone();
    let mut products = Map::new();
    for (key, fields, snapshot_fields) in [
        (
            PRODUCT_BOOKS,
            &["market_reference"][..],
            &[
                "up_book",
                "down_book",
                "fees_enabled",
                "fee_rate",
                "fee_rate_observed_at",
            ][..],
        ),
        (
            PRODUCT_RESOLUTIONS,
            &[
                "resolved_outcome",
                "historical_calibration",
                "entry_calibration",
            ][..],
            &[][..],
        ),
        (
            PRODUCT_CHAINLINK,
            &[
                "rtds_last_60_seconds",
                "rtds_volatility",
                "rtds_contiguous_tail",
                "market_horizons",
            ][..],
            &[
                "chainlink_open_price",
                "chainlink_price",
                "chainlink_gap_bps",
                "chainlink_return_5s",
                "chainlink_return_15s",
                "chainlink_return_30s",
                "chainlink_path_efficiency_30s",
                "chainlink_path_tick_count_30s",
                "chainlink_realized_volatility_5s",
                "chainlink_realized_volatility_30s",
                "chainlink_age_ms",
                "chainlink_quality_ok",
            ][..],
        ),
        (
            PRODUCT_BINANCE_1S,
            &["binance_closed_one_second_candles"][..],
            &[
                "binance_price",
                "binance_return_1s",
                "binance_return_5s",
                "binance_return_30s",
                "realized_volatility",
                "binance_age_ms",
                "binance_quality_ok",
            ][..],
        ),
        (PRODUCT_POLYGON_ORACLE, &["polygon_oracle"][..], &[][..]),
        (
            PRODUCT_BINANCE_OPEN_INTEREST,
            &["binance_futures_open_interest"][..],
            &[][..],
        ),
        (
            PRODUCT_TWAP,
            &[
                "twap_context",
                "twap_context_status",
                "settlement_uncertainty",
            ][..],
            &[][..],
        ),
    ] {
        let mut product = take(&mut remaining, fields);
        let features = take(&mut snapshot, snapshot_fields);
        if !features.is_empty() {
            product.insert("execution_snapshot".into(), Value::Object(features));
        }
        products.insert(key.into(), Value::Object(product));
    }
    // Market-scoped evaluation metadata and cross-source comparisons belong here.
    // This includes version, resolution rule, evaluation slot, entry reassessment,
    // previous comparison, market contract fields, lineage, and cross-source basis/skew.
    remaining.insert("execution_snapshot".into(), Value::Object(snapshot));
    products.insert(PRODUCT_MARKETS.into(), Value::Object(remaining));
    Ok(products)
}

fn take(object: &mut Map<String, Value>, fields: &[&str]) -> Map<String, Value> {
    fields
        .iter()
        .filter_map(|field| object.remove(*field).map(|value| ((*field).into(), value)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn context() -> Value {
        json!({
            "source_keys": [PRODUCT_MARKETS, PRODUCT_BOOKS, PRODUCT_RESOLUTIONS,
                PRODUCT_CHAINLINK, PRODUCT_BINANCE_1S, PRODUCT_POLYGON_ORACLE,
                PRODUCT_BINANCE_OPEN_INTEREST, PRODUCT_TWAP],
            "execution_snapshot": {"market_id":"market", "observed_at":"snapshot-time",
                "up_book":{"price":"0.4"}, "down_book":{"price":"0.6"},
                "chainlink_price":"100", "binance_price":"101"},
            "resolved_outcome":null,
            "historical_calibration":{"status":"unavailable"},
            "rtds_last_60_seconds":[{"price":"100"}],
            "binance_closed_one_second_candles":[],
            "polygon_oracle":null,
            "binance_futures_open_interest":null,
            "twap_context":{"opening":null,"current":null},
            "previous_prediction_comparison":null
        })
    }

    #[test]
    fn exact_keys_render_only_at_template_locations_in_one_pass() {
        let mut context = context();
        context["rtds_last_60_seconds"] = json!([{"text":"#{uninterpreted}"}]);
        let template = format!("Before #{{{PRODUCT_CHAINLINK}}} after");
        let rendered = render(&template, &context).unwrap();
        assert!(rendered.starts_with("Before {"));
        assert!(rendered.ends_with("} after"));
        assert!(rendered.contains("#{uninterpreted}"));
        assert!(!rendered.contains("up_book"));
        assert!(!rendered.contains("binance_closed_one_second_candles"));
    }

    #[test]
    fn partition_preserves_each_context_and_snapshot_field_once() {
        let context = context();
        let products = partition(&context).unwrap();
        let mut restored = Map::new();
        let mut snapshot = Map::new();
        for value in products.values() {
            for (key, value) in value.as_object().unwrap() {
                if key == "execution_snapshot" {
                    for (key, value) in value.as_object().unwrap() {
                        assert!(snapshot.insert(key.clone(), value.clone()).is_none());
                    }
                } else {
                    assert!(restored.insert(key.clone(), value.clone()).is_none());
                }
            }
        }
        restored.insert("execution_snapshot".into(), Value::Object(snapshot));
        let mut expected = context.as_object().unwrap().clone();
        expected.remove("source_keys");
        assert_eq!(restored, expected);
        assert_eq!(
            products[PRODUCT_RESOLUTIONS]["resolved_outcome"],
            Value::Null
        );
        assert_eq!(
            products[PRODUCT_MARKETS]["execution_snapshot"]["market_id"],
            "market"
        );
    }

    #[test]
    fn unconfigured_unknown_and_unterminated_references_fail_at_render_time() {
        let mut context = context();
        assert!(render("#{unknown}", &context).is_err());
        assert!(render("#{", &context).is_err());
        context["source_keys"] = json!(["unknown"]);
        assert!(render("#{unknown}", &context).is_err());
        assert!(render(&format!("#{{{PRODUCT_CHAINLINK}}}"), &context).is_err());
    }
}
