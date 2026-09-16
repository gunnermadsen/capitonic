//! Shared, bounded inference-only Kraken history; never persisted.
use anyhow::{bail, Result};
use chrono::{DateTime, Utc};
use rust_decimal::{prelude::ToPrimitive, Decimal};
use serde::Deserialize;
use std::collections::{BTreeMap, VecDeque};
use uuid::Uuid;

#[derive(Debug, Deserialize)]
pub struct KrakenTradeBatch {
    source_epoch: Uuid,
    trades: Vec<KrakenTrade>,
}

#[derive(Debug, Deserialize)]
struct KrakenTrade {
    trade_id: u64,
    timestamp_ns: i64,
    price: Decimal,
    base_volume: Decimal,
    side: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct KrakenTradeSecond {
    pub second: i64,
    pub close: f64,
    pub quote_volume: f64,
    pub signed_quote_volume: f64,
    pub trade_count: u64,
    pub available_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct KrakenTradeWindow {
    epoch: Option<Uuid>,
    last_trade_id: Option<u64>,
    pub continuous_since: Option<DateTime<Utc>>,
    seconds: VecDeque<KrakenTradeSecond>,
}

impl KrakenTradeWindow {
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn observe(&mut self, batch: KrakenTradeBatch, received_at: DateTime<Utc>) -> Result<()> {
        // Validate the complete frame before mutating shared history.
        for trade in &batch.trades {
            if trade.trade_id > i64::MAX as u64
                || trade.price <= Decimal::ZERO
                || trade.base_volume <= Decimal::ZERO
                || !(trade.price.to_f64().unwrap_or(f64::NAN)
                    * trade.base_volume.to_f64().unwrap_or(f64::NAN))
                .is_finite()
                || !matches!(trade.side.as_str(), "buy" | "sell")
            {
                bail!("invalid Kraken trade stream payload");
            }
        }
        if self.epoch != Some(batch.source_epoch) {
            self.clear();
            self.epoch = Some(batch.source_epoch);
            self.continuous_since = Some(received_at);
        }
        for trade in batch.trades {
            if self.last_trade_id.is_some_and(|id| trade.trade_id <= id) {
                continue;
            }
            let second = trade.timestamp_ns.div_euclid(1_000_000_000);
            if self
                .seconds
                .back()
                .is_some_and(|previous| second < previous.second)
            {
                self.clear();
                bail!("Kraken trade timestamp regressed; history invalidated");
            }
            let price = trade.price.to_f64().expect("validated price");
            let quote = price * trade.base_volume.to_f64().expect("validated volume");
            if self
                .seconds
                .back()
                .is_none_or(|previous| second != previous.second)
            {
                self.seconds.push_back(KrakenTradeSecond {
                    second,
                    close: price,
                    quote_volume: 0.0,
                    signed_quote_volume: 0.0,
                    trade_count: 0,
                    available_at: received_at,
                });
            }
            let current = self.seconds.back_mut().expect("second inserted");
            current.close = price;
            current.quote_volume += quote;
            current.signed_quote_volume += if trade.side == "buy" { quote } else { -quote };
            current.trade_count += 1;
            current.available_at = received_at;
            self.last_trade_id = Some(trade.trade_id);
            while self
                .seconds
                .front()
                .is_some_and(|row| row.second < second - 240)
            {
                self.seconds.pop_front();
            }
        }
        Ok(())
    }

    /// Mirrors the tournament's sparse one-second trade aggregation and rolling
    /// row windows. Empty seconds are not invented or forward-filled.
    pub fn feature_values(
        &self,
        as_of: DateTime<Utc>,
        binance_close: f64,
        binance_return_30s: f64,
        binance_flow_30s: f64,
    ) -> BTreeMap<String, f64> {
        let rows: Vec<_> = self.completed_at(as_of).collect();
        let mut values = BTreeMap::new();
        let fresh = rows
            .last()
            .is_some_and(|row| (0..=2).contains(&(as_of.timestamp() - row.second - 1)));
        for seconds in [5_usize, 15, 30, 60, 120] {
            let value = if fresh && rows.len() > seconds {
                let latest = rows[rows.len() - 1];
                let previous = rows[rows.len() - 1 - seconds];
                if latest.second - previous.second == seconds as i64 {
                    (latest.close / previous.close).ln() * 10_000.0
                } else {
                    f64::NAN
                }
            } else {
                f64::NAN
            };
            values.insert(format!("kraken_return_{seconds}s_bps"), value);
        }
        for seconds in [30_usize, 60] {
            let (volatility, volume, count) = if fresh && rows.len() > seconds {
                let start = rows.len() - seconds;
                let returns: Vec<_> = (start..rows.len())
                    .map(|i| (rows[i].close / rows[i - 1].close).ln() * 10_000.0)
                    .collect();
                let mean = returns.iter().sum::<f64>() / seconds as f64;
                let std = (returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>()
                    / (seconds - 1) as f64)
                    .sqrt();
                (
                    std,
                    rows[start..]
                        .iter()
                        .map(|r| r.quote_volume)
                        .sum::<f64>()
                        .ln_1p(),
                    rows[start..]
                        .iter()
                        .map(|r| r.trade_count as f64)
                        .sum::<f64>()
                        .ln_1p(),
                )
            } else if fresh && rows.len() == seconds {
                (
                    f64::NAN,
                    rows.iter().map(|r| r.quote_volume).sum::<f64>().ln_1p(),
                    rows.iter()
                        .map(|r| r.trade_count as f64)
                        .sum::<f64>()
                        .ln_1p(),
                )
            } else {
                (f64::NAN, f64::NAN, f64::NAN)
            };
            values.insert(
                format!("kraken_realized_volatility_{seconds}s_bps"),
                volatility,
            );
            values.insert(format!("kraken_log_quote_volume_{seconds}s"), volume);
            values.insert(format!("kraken_log_trade_count_{seconds}s"), count);
        }
        for seconds in [5_usize, 30, 60] {
            let value = if fresh && rows.len() >= seconds {
                let window = &rows[rows.len() - seconds..];
                window.iter().map(|r| r.signed_quote_volume).sum::<f64>()
                    / (window.iter().map(|r| r.quote_volume).sum::<f64>() + 1e-9)
            } else {
                f64::NAN
            };
            values.insert(format!("kraken_print_signed_share_{seconds}s"), value);
        }
        // Rust signum(0) is +1; the training expression sign(0) is zero.
        let sign = |value: f64| if value == 0.0 { 0.0 } else { value.signum() };
        values.insert(
            "kraken_binance_basis_bps".into(),
            if fresh && binance_close > 0.0 {
                (rows.last().expect("fresh row").close / binance_close).ln() * 10_000.0
            } else {
                f64::NAN
            },
        );
        values.insert(
            "kraken_binance_return_agreement_30s".into(),
            sign(values["kraken_return_30s_bps"]) * sign(binance_return_30s),
        );
        values.insert(
            "kraken_binance_flow_agreement_30s".into(),
            sign(values["kraken_print_signed_share_30s"]) * sign(binance_flow_30s),
        );
        values
    }

    /// Only completed, causally available seconds may enter a feature vector.
    pub fn completed_at(&self, as_of: DateTime<Utc>) -> impl Iterator<Item = &KrakenTradeSecond> {
        self.seconds
            .iter()
            .filter(move |row| row.second < as_of.timestamp() && row.available_at <= as_of)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn batch(epoch: Uuid, id: u64, second: i64) -> KrakenTradeBatch {
        KrakenTradeBatch {
            source_epoch: epoch,
            trades: vec![KrakenTrade {
                trade_id: id,
                timestamp_ns: second * 1_000_000_000,
                price: Decimal::from(100),
                base_volume: Decimal::from(2),
                side: "sell".into(),
            }],
        }
    }
    #[test]
    fn rolling_features_match_constant_price_and_signed_volume() {
        let epoch = Uuid::new_v4();
        let mut window = KrakenTradeWindow::default();
        for i in 0..=120 {
            let at = DateTime::from_timestamp(1000 + i, 0).unwrap();
            window
                .observe(batch(epoch, (i + 1) as u64, 1000 + i), at)
                .unwrap();
        }
        let values =
            window.feature_values(DateTime::from_timestamp(1121, 0).unwrap(), 100.0, 0.0, -1.0);
        assert_eq!(values["kraken_return_120s_bps"], 0.0);
        assert_eq!(values["kraken_realized_volatility_60s_bps"], 0.0);
        assert_eq!(values["kraken_log_quote_volume_30s"], 6000.0_f64.ln_1p());
        assert_eq!(values["kraken_log_trade_count_60s"], 60.0_f64.ln_1p());
        assert_eq!(values["kraken_binance_return_agreement_30s"], 0.0);
        assert_eq!(values["kraken_binance_flow_agreement_30s"], 1.0);
        let stale =
            window.feature_values(DateTime::from_timestamp(1124, 0).unwrap(), 100.0, 0.0, -1.0);
        assert!(stale.values().all(|value| value.is_nan()));
    }

    #[test]
    fn replay_does_not_duplicate_and_reconnect_invalidates_history() {
        let mut window = KrakenTradeWindow::default();
        let epoch = Uuid::new_v4();
        let at = DateTime::from_timestamp(1000, 0).unwrap();
        window.observe(batch(epoch, 1, 1000), at).unwrap();
        window.observe(batch(epoch, 1, 1000), at).unwrap();
        assert_eq!(window.seconds[0].trade_count, 1);
        assert_eq!(window.seconds[0].signed_quote_volume, -200.0);
        assert_eq!(window.completed_at(at).count(), 0);
        assert_eq!(
            window
                .completed_at(at + chrono::Duration::seconds(1))
                .count(),
            1
        );
        window
            .observe(
                batch(Uuid::new_v4(), 2, 1001),
                at + chrono::Duration::seconds(1),
            )
            .unwrap();
        assert_eq!(window.seconds.len(), 1);
        assert_eq!(window.seconds[0].second, 1001);
    }
}
