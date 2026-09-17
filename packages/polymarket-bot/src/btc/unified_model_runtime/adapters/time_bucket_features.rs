//! Tournament feature binding over market data already delivered to the bot.
use super::super::contract::{InputContract, ModelContract};
use super::FeatureContext;
use crate::btc::directional_features::{
    build_payoff_feature_values_with_policy, DirectionalExternalFeatureInputs,
    DirectionalOracleRound,
};
use anyhow::{ensure, Context, Result};
use chrono::Duration;

pub const PRODUCTS: &[&str] = &[
    "binance_spot_btcusdt_one_second_ohlcv",
    "polymarket_btc_five_minute_orderbooks",
    "polygon_chainlink_btcusd_oracle",
];

fn requires_unsupported_realtime_source(name: &str) -> bool {
    name.starts_with("chainlink_ref_")
        || name.starts_with("chainlink_candle_")
        || name.starts_with("binance_oi_")
        || name.starts_with("kraken_")
        || matches!(name, "refprice_margin_bps" | "sensor_source_age_seconds")
}

pub(crate) fn validate_contract(contract: &ModelContract, names: &[String]) -> Result<()> {
    ensure!(
        names
            .iter()
            .all(|name| super::time_bucket_feature_names::SUPPORTED.contains(&name.as_str())),
        "unsupported tournament feature"
    );
    ensure!(
        names
            .iter()
            .all(|name| !requires_unsupported_realtime_source(name)),
        "tournament model requires an unavailable realtime source"
    );

    let uses_oracle = names
        .iter()
        .any(|name| name.starts_with("oracle_") || name.starts_with("binance_oracle_"));
    let mut expected = vec![
        InputContract {
            slot: "btc_seconds".into(),
            product: PRODUCTS[0].into(),
            semantics: "binance_closed_seconds_prewindow_open_v1".into(),
            required: true,
            lookback_seconds: 301,
            maximum_age_ms: 5000,
        },
        InputContract {
            slot: "execution_book".into(),
            product: PRODUCTS[1].into(),
            semantics: "causal_vwap_five_shares_v1".into(),
            required: true,
            lookback_seconds: 2,
            maximum_age_ms: 2000,
        },
    ];
    if uses_oracle {
        expected.push(InputContract {
            slot: "oracle".into(),
            product: PRODUCTS[2].into(),
            semantics: "causal_oracle_rounds_v1".into(),
            required: false,
            lookback_seconds: 600,
            maximum_age_ms: 600000,
        });
    }
    ensure!(
        contract.inputs == expected,
        "bucket source recipe differs from supported training semantics"
    );
    Ok(())
}

pub fn build(context: &FeatureContext<'_>) -> Result<Vec<f64>> {
    let at = context.feature_as_of;
    let anchor = context
        .state
        .binance_one_second_window
        .completed()
        .iter()
        .find(|row| row.open_timestamp == context.window_start - Duration::seconds(1))
        .context("bucket Binance opening boundary unavailable")?;
    let up = context
        .state
        .unified_book_history
        .at(
            &context.up.market_id,
            &context.up.token_id,
            context.up.connection_id,
            at,
        )
        .context("bucket causal UP book unavailable")?;
    let down = context
        .state
        .unified_book_history
        .at(
            &context.down.market_id,
            &context.down.token_id,
            context.down.connection_id,
            at,
        )
        .context("bucket causal DOWN book unavailable")?;
    for book in [up, down] {
        ensure!(
            (0..=2000).contains(&(at - book.received_at).num_milliseconds()),
            "bucket book stale"
        );
    }

    let uses_oracle = context
        .binding
        .sources
        .iter()
        .any(|source| source.slot == "oracle");
    let rounds: Vec<_> = context
        .state
        .directional_external
        .oracle
        .iter()
        .filter(|round| uses_oracle && round.available_at <= at)
        .map(|round| DirectionalOracleRound {
            phase_id: i32::from(round.phase_id),
            aggregator_round_id: round.round_id as i64,
            source_timestamp: round.source_timestamp,
            block_timestamp: round.block_timestamp,
            block_number: None,
            log_index: None,
            price: round.price,
            available_at: round.available_at,
        })
        .collect();
    let inputs = DirectionalExternalFeatureInputs {
        oracle_rounds: &rounds,
        ..Default::default()
    };
    Ok(build_payoff_feature_values_with_policy(
        &context.state.binance_one_second_window,
        context.window_start,
        at,
        anchor.open_price,
        &inputs,
        up,
        down,
        context.fee_rate,
        context.names,
        true,
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_realtime_feature_families_are_rejected() {
        for name in [
            "chainlink_ref_return_5s_bps",
            "chainlink_candle_return_5m_bps",
            "binance_oi_change_5m_bps",
            "kraken_return_5s_bps",
            "refprice_margin_bps",
            "sensor_source_age_seconds",
        ] {
            assert!(requires_unsupported_realtime_source(name), "{name}");
        }
        assert!(!requires_unsupported_realtime_source("btc_return_5s_bps"));
        assert!(!requires_unsupported_realtime_source(
            "oracle_return_from_window_open_bps"
        ));
    }
}
