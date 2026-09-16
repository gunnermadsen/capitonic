//! Tournament feature binding over existing shared runtime observations.
use super::super::contract::{InputContract, ModelContract};
use super::FeatureContext;
use crate::btc::directional_features::{
    build_payoff_feature_values_with_policy, DirectionalBinanceOpenInterest,
    DirectionalExternalFeatureInputs, DirectionalOracleRound,
};
use anyhow::{ensure, Context, Result};
use chrono::{DateTime, Duration, Utc};
use rust_decimal::prelude::ToPrimitive;
use std::collections::BTreeMap;

pub const PRODUCTS: &[&str] = &[
    "binance_spot_btcusdt_one_second_ohlcv",
    "polymarket_btc_five_minute_orderbooks",
    "polygon_chainlink_btcusd_oracle",
    "chainlink_btcusd_reference_price",
    "chainlink_btcusd_one_minute_ohlc",
    "binance_futures_btcusdt_open_interest",
    "kraken_spot_btcusd_trades",
];
pub(crate) fn validate_contract(contract: &ModelContract, names: &[String]) -> Result<()> {
    ensure!(
        names
            .iter()
            .all(|n| super::time_bucket_feature_names::SUPPORTED.contains(&n.as_str())),
        "unsupported tournament feature"
    );
    let recipes = [
        (
            "btc_seconds",
            PRODUCTS[0],
            "binance_closed_seconds_prewindow_open_v1",
            true,
            301,
            5000,
            true,
        ),
        (
            "execution_book",
            PRODUCTS[1],
            "causal_vwap_five_shares_v1",
            true,
            2,
            2000,
            true,
        ),
        (
            "oracle",
            PRODUCTS[2],
            "causal_oracle_rounds_v1",
            false,
            600,
            600000,
            names
                .iter()
                .any(|n| n.starts_with("oracle_") || n.starts_with("binance_oracle_")),
        ),
        (
            "refprice",
            PRODUCTS[3],
            "causal_signed_refprice_tournament_v1",
            true,
            125,
            5000,
            names.iter().any(|n| n.starts_with("chainlink_ref_")),
        ),
        (
            "candles",
            PRODUCTS[4],
            "chainlink_ohlc_close_available_120s_v1",
            true,
            3660,
            120000,
            names.iter().any(|n| n.starts_with("chainlink_candle_")),
        ),
        (
            "open_interest",
            PRODUCTS[5],
            "binance_futures_five_minute_open_interest_v1",
            true,
            3900,
            360000,
            names.iter().any(|n| n.starts_with("binance_oi_")),
        ),
        (
            "kraken",
            PRODUCTS[6],
            "kraken_spot_sparse_trade_seconds_v1",
            true,
            121,
            2000,
            names.iter().any(|n| n.starts_with("kraken_")),
        ),
    ];
    let expected: Vec<_> = recipes
        .into_iter()
        .filter(|r| r.6)
        .map(
            |(slot, product, semantics, required, lookback_seconds, maximum_age_ms, _)| {
                InputContract {
                    slot: slot.into(),
                    product: product.into(),
                    semantics: semantics.into(),
                    required,
                    lookback_seconds,
                    maximum_age_ms,
                }
            },
        )
        .collect();
    ensure!(
        contract.inputs == expected,
        "bucket source recipe differs from supported training semantics"
    );
    Ok(())
}
pub fn build(c: &FeatureContext<'_>) -> Result<Vec<f64>> {
    let at = c.feature_as_of;
    let anchor = c
        .state
        .binance_one_second_window
        .completed()
        .iter()
        .find(|row| row.open_timestamp == c.window_start - Duration::seconds(1))
        .context("bucket Binance opening boundary unavailable")?;
    let up = c
        .state
        .unified_book_history
        .at(&c.up.market_id, &c.up.token_id, c.up.connection_id, at)
        .context("bucket causal UP book unavailable")?;
    let down = c
        .state
        .unified_book_history
        .at(
            &c.down.market_id,
            &c.down.token_id,
            c.down.connection_id,
            at,
        )
        .context("bucket causal DOWN book unavailable")?;
    for book in [up, down] {
        ensure!(
            (0..=2000).contains(&(at - book.received_at).num_milliseconds()),
            "bucket book stale"
        );
    }
    let external = &c.state.directional_external;
    let rounds: Vec<_> = external
        .oracle
        .iter()
        .filter(|r| {
            r.available_at <= at
                && c.binding
                    .sources
                    .iter()
                    .any(|source| source.slot == "oracle")
        })
        .map(|r| DirectionalOracleRound {
            phase_id: i32::from(r.phase_id),
            aggregator_round_id: r.round_id as i64,
            source_timestamp: r.source_timestamp,
            block_timestamp: r.block_timestamp,
            block_number: None,
            log_index: None,
            price: r.price,
            available_at: r.available_at,
        })
        .collect();
    let oi: Vec<_> = external
        .open_interest
        .iter()
        .filter(|r| r.available_at <= at)
        .map(|r| DirectionalBinanceOpenInterest {
            source_timestamp: r.source_timestamp,
            period_seconds: 300,
            sum_open_interest: r.sum_open_interest,
            sum_open_interest_value: r.sum_open_interest_value,
            available_at: r.available_at,
        })
        .collect();
    let candles: Vec<_> = external
        .canonical_candles
        .iter()
        .filter(|r| r.available_at <= at)
        .cloned()
        .collect();
    let inputs = DirectionalExternalFeatureInputs {
        oracle_rounds: &rounds,
        chainlink_candles: &candles,
        open_interest: &oi,
        ..Default::default()
    };
    let mut base_names: Vec<_> = c
        .names
        .iter()
        .filter(|n| !n.starts_with("chainlink_ref_") && !n.starts_with("kraken_"))
        .cloned()
        .collect();
    for name in [
        "btc_return_5s_bps",
        "btc_return_30s_bps",
        "btc_signed_flow_30s",
    ] {
        if !base_names.iter().any(|n| n == name) {
            base_names.push(name.into());
        }
    }
    let base = build_payoff_feature_values_with_policy(
        &c.state.binance_one_second_window,
        c.window_start,
        at,
        anchor.open_price,
        &inputs,
        up,
        down,
        c.fee_rate,
        &base_names,
        true,
    )?;
    let mut values: BTreeMap<String, f64> = base_names.into_iter().zip(base).collect();
    let close = c
        .state
        .binance_one_second_window
        .completed()
        .iter()
        .rfind(|r| r.close_timestamp <= at)
        .context("bucket Binance close unavailable")?
        .close_price
        .to_f64()
        .context("invalid Binance close")?;
    if c.names.iter().any(|n| n.starts_with("chainlink_ref_")) {
        values.extend(refprice_features(
            external,
            at,
            anchor.open_price.to_f64().context("invalid boundary")?,
            close,
            values["btc_return_5s_bps"],
            values["btc_return_30s_bps"],
        )?);
    }
    if c.names.iter().any(|n| n.starts_with("kraken_")) {
        ensure!(
            c.state
                .kraken_trades
                .continuous_since
                .is_some_and(|start| (at - start).num_seconds() >= 121),
            "Kraken history warmup incomplete"
        );
        let kraken = c.state.kraken_trades.feature_values(
            at,
            close,
            values["btc_return_30s_bps"],
            values["btc_signed_flow_30s"],
        );
        ensure!(
            kraken
                .get("kraken_binance_basis_bps")
                .is_some_and(|v| v.is_finite()),
            "Kraken source stale"
        );
        values.extend(kraken);
    }
    c.names
        .iter()
        .map(|n| {
            values
                .get(n)
                .copied()
                .with_context(|| format!("unsupported bucket feature {n}"))
        })
        .collect()
}
fn sign(x: f64) -> f64 {
    if x == 0.0 {
        0.0
    } else {
        x.signum()
    }
}
fn refprice_features(
    external: &crate::btc::DirectionalExternalState,
    at: DateTime<Utc>,
    boundary: f64,
    close: f64,
    btc5: f64,
    btc30: f64,
) -> Result<BTreeMap<String, f64>> {
    // Select only reports known at this observation, retaining the latest report per
    // effective timestamp as in the training canonical path.
    let mut canonical = BTreeMap::new();
    for row in external
        .refprice
        .iter()
        .filter(|r| r.available_at <= at && r.source_timestamp <= at)
    {
        canonical.insert(row.valid_from_timestamp, row);
    }
    let mut rows: Vec<_> = canonical.into_values().collect();
    rows.sort_by_key(|r| (r.source_timestamp, r.available_at));
    let last = *rows
        .last()
        .context("direct Chainlink RefPrice history unavailable")?;
    let age = (at - last.source_timestamp)
        .num_microseconds()
        .unwrap_or(i64::MAX) as f64
        / 1e6;
    ensure!(age > 0.0 && age <= 5.0, "direct Chainlink RefPrice stale");
    let prices: Vec<f64> = rows
        .iter()
        .map(|r| r.price.to_f64().context("invalid RefPrice"))
        .collect::<Result<_>>()?;
    ensure!(
        prices.iter().all(|p| p.is_finite() && *p > 0.0),
        "invalid RefPrice"
    );
    let current = *prices.last().expect("nonempty");
    let spread = |i: usize| -> Result<f64> {
        Ok((rows[i].ask - rows[i].bid)
            .to_f64()
            .context("invalid RefPrice spread")?
            / prices[i]
            * 1e4)
    };
    let mut out = BTreeMap::new();
    let mut anchors = BTreeMap::new();
    for seconds in [1, 5, 15, 30, 60, 90, 120] {
        let target = at - Duration::seconds(seconds);
        let i = rows
            .iter()
            .rposition(|r| r.source_timestamp <= target)
            .context("direct RefPrice anchor history unavailable")?;
        ensure!(
            (target - rows[i].source_timestamp).num_milliseconds() <= 5000,
            "direct RefPrice anchor stale"
        );
        anchors.insert(seconds, i);
        out.insert(
            format!("chainlink_ref_return_{seconds}s_bps"),
            (current / prices[i]).ln() * 1e4,
        );
    }
    out.insert("chainlink_ref_age_seconds".into(), age);
    out.insert(
        "chainlink_ref_source_skew_seconds".into(),
        (last.source_timestamp - last.valid_from_timestamp)
            .num_microseconds()
            .unwrap_or(0) as f64
            / 1e6,
    );
    out.insert(
        "chainlink_ref_binance_basis_bps".into(),
        (current / close).ln() * 1e4,
    );
    out.insert(
        "chainlink_ref_boundary_gap_bps".into(),
        (current / boundary).ln() * 1e4,
    );
    out.insert("chainlink_ref_spread_bps".into(), spread(rows.len() - 1)?);
    out.insert(
        "chainlink_ref_spread_change_30s_bps".into(),
        spread(rows.len() - 1)? - spread(anchors[&30])?,
    );
    let agreement = sign(btc30) * sign(out["chainlink_ref_return_30s_bps"]);
    let alignment =
        sign(out["chainlink_ref_return_5s_bps"]) * sign(out["chainlink_ref_return_30s_bps"]);
    out.insert(
        "chainlink_ref_binance_direction_agreement_30s".into(),
        agreement,
    );
    out.insert(
        "chainlink_ref_binance_disagreement_30s".into(),
        f64::from(agreement < 0.0),
    );
    out.insert("chainlink_ref_momentum_alignment_5_30".into(), alignment);
    out.insert(
        "chainlink_ref_reversal_5_vs_30".into(),
        f64::from(alignment < 0.0),
    );
    out.insert(
        "chainlink_ref_reversal_15_vs_60".into(),
        f64::from(
            sign(out["chainlink_ref_return_15s_bps"]) * sign(out["chainlink_ref_return_60s_bps"])
                < 0.0,
        ),
    );
    out.insert(
        "chainlink_ref_boundary_velocity_5s_bps".into(),
        out["chainlink_ref_return_5s_bps"],
    );
    out.insert(
        "chainlink_ref_binance_basis_velocity_5s_bps".into(),
        out["chainlink_ref_binance_basis_bps"]
            - (prices[anchors[&5]] / (close / (btc5 / 1e4).exp())).ln() * 1e4,
    );
    for horizon in [5, 15, 30, 60, 90, 120] {
        let start = rows.partition_point(|r| r.source_timestamp < at - Duration::seconds(horizon));
        let sample = &prices[start..];
        ensure!(sample.len() >= 2, "RefPrice rolling history incomplete");
        let returns: Vec<_> = sample
            .windows(2)
            .map(|p| (p[1].ln() - p[0].ln()) * 1e4)
            .collect();
        let min = sample.iter().copied().fold(f64::INFINITY, f64::min);
        let max = sample.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        out.insert(
            format!("chainlink_ref_realized_volatility_{horizon}s_bps"),
            returns.iter().map(|r| r * r).sum::<f64>().sqrt(),
        );
        if horizon >= 15 {
            out.insert(
                format!("chainlink_ref_range_{horizon}s_bps"),
                (max / min).ln() * 1e4,
            );
        }
        if horizon == 30 || horizon == 60 {
            out.insert(
                format!("chainlink_ref_path_efficiency_{horizon}s"),
                (sample[sample.len() - 1] / sample[0]).ln().abs() * 1e4
                    / returns.iter().map(|r| r.abs()).sum::<f64>().max(1e-9),
            );
        }
        if horizon == 60 {
            out.insert(
                "chainlink_ref_range_position_60s".into(),
                (sample[sample.len() - 1] - min) / (max - min).max(1e-9),
            );
            out.insert(
                "chainlink_ref_boundary_cross_count_60s".into(),
                sample
                    .windows(2)
                    .filter(|p| (p[0] >= boundary) != (p[1] >= boundary))
                    .count() as f64,
            );
            out.insert(
                "chainlink_ref_direction_changes_60s".into(),
                returns
                    .windows(2)
                    .filter(|p| sign(p[0]) != sign(p[1]))
                    .count() as f64,
            );
            out.insert("chainlink_ref_reports_60s".into(), sample.len() as f64);
            out.insert(
                "chainlink_ref_max_gap_60s".into(),
                rows[start..]
                    .windows(2)
                    .map(|r| {
                        (r[1].source_timestamp - r[0].source_timestamp)
                            .num_microseconds()
                            .unwrap_or(0) as f64
                            / 1e6
                    })
                    .fold(0.0, f64::max),
            );
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::btc::{
        directional_external_runtime::ChainlinkRefPricePoint, DirectionalExternalState,
    };
    use rust_decimal::Decimal;
    #[test]
    fn direct_refprice_matches_training_recipe_and_rejects_stale_history() {
        let at = DateTime::parse_from_rfc3339("2026-09-16T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let mut state = DirectionalExternalState::default();
        for i in 0..126 {
            let source = at - Duration::seconds(126 - i);
            let price = Decimal::from(60000 + (i % 11) * 3 + (i / 11));
            state.merge_refprice(ChainlinkRefPricePoint {
                source_timestamp: source,
                valid_from_timestamp: source - Duration::seconds(1),
                available_at: source + Duration::milliseconds(200),
                price,
                bid: price - Decimal::new(5, 1),
                ask: price + Decimal::new(5, 1),
            });
        }
        // Frozen output of attach_causal_refprice_features on the same nonmonotonic price path.
        let expected:BTreeMap<String,f64>=serde_json::from_str(r#"{"chainlink_ref_return_1s_bps":0.0,"chainlink_ref_return_5s_bps":1.9994335005035029,"chainlink_ref_return_15s_bps":1.6661668204753404,"chainlink_ref_return_30s_bps":-1.4993128177683541,"chainlink_ref_return_60s_bps":2.8326487954861386,"chainlink_ref_binance_basis_bps":0.49982089761490045,"chainlink_ref_boundary_gap_bps":1.1662876245102844,"chainlink_ref_spread_bps":0.166602802259134,"chainlink_ref_spread_change_30s_bps":2.497709921928104e-05,"chainlink_ref_binance_direction_agreement_30s":1.0,"chainlink_ref_return_90s_bps":1.8327987721567194,"chainlink_ref_return_120s_bps":0.8330487088403492,"chainlink_ref_momentum_alignment_5_30":-1.0,"chainlink_ref_reversal_5_vs_30":1.0,"chainlink_ref_reversal_15_vs_60":0.0,"chainlink_ref_boundary_velocity_5s_bps":1.9994335005035029,"chainlink_ref_realized_volatility_5s_bps":0.9997167518151814,"chainlink_ref_realized_volatility_15s_bps":5.15642465661469,"chainlink_ref_realized_volatility_30s_bps":8.747622914340525,"chainlink_ref_realized_volatility_60s_bps":11.410695547986487,"chainlink_ref_realized_volatility_90s_bps":14.387050330231062,"chainlink_ref_realized_volatility_120s_bps":16.845720163839708,"chainlink_ref_range_15s_bps":4.831280133290141,"chainlink_ref_range_30s_bps":5.164557920356182,"chainlink_ref_range_60s_bps":5.664495428271579,"chainlink_ref_range_90s_bps":5.997800986173149,"chainlink_ref_range_120s_bps":6.497780153812002,"chainlink_ref_path_efficiency_30s":0.0545425788160635,"chainlink_ref_path_efficiency_60s":0.05538243641318962,"chainlink_ref_range_position_60s":0.5,"chainlink_ref_boundary_cross_count_60s":11.0,"chainlink_ref_direction_changes_60s":10.0,"chainlink_ref_age_seconds":1.0,"chainlink_ref_reports_60s":60.0,"chainlink_ref_max_gap_60s":1.0,"chainlink_ref_binance_basis_velocity_5s_bps":0.7994335005024331,"chainlink_ref_binance_disagreement_30s":0.0,"chainlink_ref_source_skew_seconds":1.0}"#).unwrap();
        let actual = refprice_features(&state, at, 60016.0, 60020.0, 1.2, -2.3).unwrap();
        assert_eq!(actual.len(), expected.len());
        for (name, value) in expected {
            assert!(
                (actual[&name] - value).abs() < 1e-8,
                "{name}: {} != {value}",
                actual[&name]
            );
        }
        assert!(refprice_features(
            &state,
            at + Duration::seconds(6),
            60016.0,
            60020.0,
            1.2,
            -2.3
        )
        .is_err());
    }
}
