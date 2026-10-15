//! Typed, process-owned order policy parameters. No gate evaluation or venue IO lives here.
use std::collections::BTreeMap;

use anyhow::{ensure, Result};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::btc::admission::{DailyRealizedPnlHighWaterMarkConfig, LossRegimeConfidenceFloorConfig};
use crate::btc::BtcDirectionalModelEntryPolicy;
use crate::models::OrderType;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrderPolicy {
    pub name: String,
    pub version: u32,
    pub params: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SizingMode {
    Fixed,
    Dynamic,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SizingPolicy {
    pub mode: SizingMode,
    #[serde(with = "shares")]
    pub lower_share_bound: Decimal,
    #[serde(with = "shares")]
    pub upper_share_bound: Decimal,
    #[serde(with = "shares")]
    pub default_share: Decimal,
    #[serde(with = "decimal")]
    pub min_entry_price: Decimal,
    #[serde(with = "decimal")]
    pub max_entry_price: Decimal,
    #[serde(with = "decimal")]
    pub max_depth_participation: Decimal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntryTimingPolicy {
    pub min_seconds_after_open: i64,
    pub min_seconds_before_close: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidencePolicy {
    pub max_reference_age_ms: i64,
    pub max_chainlink_open_delay_ms: i64,
    pub max_book_age_ms: i64,
    pub max_source_skew_ms: i64,
    pub max_fee_age_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiquidityPolicy {
    pub require_exit_book: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionCostPolicy {
    #[serde(with = "decimal")]
    pub spread_reserve_fraction: Decimal,
    #[serde(with = "decimal")]
    pub slippage_reserve_bps: Decimal,
    #[serde(with = "decimal")]
    pub latency_reserve_per_share: Decimal,
    #[serde(with = "decimal")]
    pub max_fee_rate: Decimal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EdgePolicy {
    pub directional_model_entry_policy: BtcDirectionalModelEntryPolicy,
    #[serde(with = "decimal")]
    pub min_net_edge_per_share: Decimal,
    #[serde(with = "decimal")]
    pub min_net_edge_usd: Decimal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrderExecutionPolicy {
    pub member_order_types: BTreeMap<String, OrderType>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExposurePolicy {
    pub max_open_positions: Option<usize>,
    #[serde(with = "optional_decimal")]
    pub max_open_notional_usd: Option<Decimal>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LossLimitPolicy {
    #[serde(with = "optional_decimal")]
    pub max_daily_loss_usd: Option<Decimal>,
    pub daily_realized_pnl_high_water_mark: Option<DailyRealizedPnlHighWaterMarkConfig>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PaperPreview {
    pub scenario_key: String,
    pub arrival_latency_ms: u64,
    #[serde(with = "decimal")]
    pub visible_depth_haircut: Decimal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PaperExecutionPolicy {
    pub arrival_latency_ms: u64,
    #[serde(with = "decimal")]
    pub visible_depth_haircut: Decimal,
    #[serde(with = "decimal")]
    pub starting_collateral_usd: Decimal,
    pub stress_previews: Vec<PaperPreview>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OrderPolicies {
    pub sizing: SizingPolicy,
    pub entry_timing: EntryTimingPolicy,
    pub evidence: EvidencePolicy,
    pub liquidity: LiquidityPolicy,
    pub execution_cost: ExecutionCostPolicy,
    pub edge: EdgePolicy,
    pub order_execution: OrderExecutionPolicy,
    pub exposure: ExposurePolicy,
    pub loss_limit: LossLimitPolicy,
    pub loss_regime: Option<LossRegimeConfidenceFloorConfig>,
    pub paper_execution: Option<PaperExecutionPolicy>,
}

impl OrderPolicies {
    pub fn compile(policies: &[OrderPolicy], paper: bool) -> Result<Self> {
        use anyhow::Context;
        let mut values = BTreeMap::new();
        for policy in policies {
            ensure!(
                policy.version == 1,
                "unsupported policy version: {}",
                policy.name
            );
            ensure!(
                values
                    .insert(policy.name.as_str(), policy.params.clone())
                    .is_none(),
                "duplicate order policy: {}",
                policy.name
            );
        }
        fn take<T: serde::de::DeserializeOwned>(
            values: &mut BTreeMap<&str, Value>,
            name: &str,
        ) -> Result<T> {
            serde_json::from_value(
                values
                    .remove(name)
                    .with_context(|| format!("missing order policy: {name}"))?,
            )
            .with_context(|| format!("invalid parameters for {name}"))
        }
        let loss_regime = values
            .remove("loss_regime_policy")
            .map(serde_json::from_value)
            .transpose()?;
        let paper_execution = values
            .remove("paper_execution_policy")
            .map(serde_json::from_value)
            .transpose()?;
        let result = Self {
            sizing: take(&mut values, "sizing_policy")?,
            entry_timing: take(&mut values, "entry_timing_policy")?,
            evidence: take(&mut values, "evidence_policy")?,
            liquidity: take(&mut values, "liquidity_policy")?,
            execution_cost: take(&mut values, "execution_cost_policy")?,
            edge: take(&mut values, "edge_policy")?,
            order_execution: take(&mut values, "order_execution_policy")?,
            exposure: take(&mut values, "exposure_policy")?,
            loss_limit: take(&mut values, "loss_limit_policy")?,
            loss_regime,
            paper_execution,
        };
        ensure!(
            values.is_empty(),
            "unknown order policies: {:?}",
            values.keys().collect::<Vec<_>>()
        );
        result.validate(paper)?;
        Ok(result)
    }

    fn validate(&self, paper: bool) -> Result<()> {
        let s = &self.sizing;
        ensure!(
            s.mode == SizingMode::Fixed,
            "dynamic sizing decision contract is not implemented"
        );
        ensure!(
            s.lower_share_bound > Decimal::ZERO
                && s.lower_share_bound <= s.default_share
                && s.default_share <= s.upper_share_bound,
            "invalid share bounds"
        );
        ensure!(
            s.min_entry_price > Decimal::ZERO
                && s.min_entry_price < s.max_entry_price
                && s.max_entry_price < Decimal::ONE,
            "invalid entry price bounds"
        );
        ensure!(
            s.max_depth_participation > Decimal::ZERO && s.max_depth_participation <= Decimal::ONE,
            "invalid depth participation"
        );
        let t = &self.entry_timing;
        ensure!(
            t.min_seconds_after_open >= 0
                && t.min_seconds_before_close > 0
                && t.min_seconds_after_open
                    .checked_add(t.min_seconds_before_close)
                    .is_some_and(|sum| sum < 300
                        || (t.min_seconds_after_open == 120 && t.min_seconds_before_close == 180)),
            "entry timing leaves no supported opportunity"
        );
        let e = &self.evidence;
        ensure!(
            [
                e.max_reference_age_ms,
                e.max_chainlink_open_delay_ms,
                e.max_book_age_ms,
                e.max_fee_age_ms
            ]
            .iter()
            .all(|v| *v > 0)
                && e.max_source_skew_ms >= 0,
            "invalid evidence age bounds"
        );
        let c = &self.execution_cost;
        ensure!(
            [
                c.spread_reserve_fraction,
                c.slippage_reserve_bps,
                c.latency_reserve_per_share,
                c.max_fee_rate,
                self.edge.min_net_edge_per_share,
                self.edge.min_net_edge_usd
            ]
            .iter()
            .all(|v| *v >= Decimal::ZERO),
            "negative cost or edge parameter"
        );
        ensure!(
            self.order_execution
                .member_order_types
                .values()
                .all(|v| matches!(v, OrderType::Fok | OrderType::Fak)),
            "only FOK and FAK are supported"
        );
        ensure!(
            self.exposure
                .max_open_positions
                .is_none_or(|v| (1..=6).contains(&v)),
            "max_open_positions must be 1..=6"
        );
        ensure!(
            self.exposure
                .max_open_notional_usd
                .is_none_or(|v| v > Decimal::ZERO && v <= Decimal::from(30)),
            "exposure value exceeds existing configuration bounds"
        );
        ensure!(
            self.loss_limit
                .max_daily_loss_usd
                .is_none_or(|v| v > Decimal::ZERO),
            "max_daily_loss_usd must be greater than zero"
        );
        if let Some(p) = &self.loss_regime {
            p.validate()?;
        }
        if let Some(p) = &self.loss_limit.daily_realized_pnl_high_water_mark {
            p.validate()?;
        }
        ensure!(
            self.paper_execution.is_some() == paper,
            "paper_execution_policy is required only for paper mode"
        );
        if let Some(p) = &self.paper_execution {
            ensure!(
                p.starting_collateral_usd > Decimal::ZERO
                    && p.starting_collateral_usd <= Decimal::from(1_000_000),
                "invalid paper starting collateral"
            );
            let mut keys = std::collections::HashSet::new();
            for (latency, depth) in std::iter::once((p.arrival_latency_ms, p.visible_depth_haircut))
                .chain(
                    p.stress_previews
                        .iter()
                        .map(|v| (v.arrival_latency_ms, v.visible_depth_haircut)),
                )
            {
                ensure!(
                    (1..=60_000).contains(&latency)
                        && depth > Decimal::ZERO
                        && depth <= Decimal::ONE,
                    "invalid paper latency or visible depth"
                );
            }
            for preview in &p.stress_previews {
                ensure!(
                    !preview.scenario_key.trim().is_empty() && keys.insert(&preview.scenario_key),
                    "invalid or duplicate stress scenario"
                );
            }
            if let Some(h) = &self.loss_limit.daily_realized_pnl_high_water_mark {
                h.validate_against_starting_collateral(p.starting_collateral_usd)?;
            }
        }
        Ok(())
    }
}

mod decimal {
    use rust_decimal::Decimal;
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Decimal, D::Error> {
        Decimal::from_str_exact(&String::deserialize(d)?).map_err(serde::de::Error::custom)
    }
    pub fn serialize<S: Serializer>(v: &Decimal, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&v.normalize().to_string())
    }
}
mod optional_decimal {
    use rust_decimal::Decimal;
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Decimal>, D::Error> {
        Option::<String>::deserialize(d)?
            .map(|v| Decimal::from_str_exact(&v).map_err(serde::de::Error::custom))
            .transpose()
    }
    pub fn serialize<S: Serializer>(v: &Option<Decimal>, s: S) -> Result<S::Ok, S::Error> {
        match v {
            Some(v) => s.serialize_some(&v.normalize().to_string()),
            None => s.serialize_none(),
        }
    }
}
mod shares {
    use rust_decimal::Decimal;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::str::FromStr;
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Decimal, D::Error> {
        Decimal::from_str_exact(&serde_json::Number::deserialize(d)?.to_string())
            .map_err(serde::de::Error::custom)
    }
    pub fn serialize<S: Serializer>(v: &Decimal, s: S) -> Result<S::Ok, S::Error> {
        serde_json::Number::from_str(&v.normalize().to_string())
            .map_err(serde::ser::Error::custom)?
            .serialize(s)
    }
}
