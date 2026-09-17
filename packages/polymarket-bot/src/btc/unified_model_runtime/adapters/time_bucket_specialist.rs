//! Frozen tournament regressors, Platt calibration and bucket admission.
use super::super::contract::ModelContract;
use super::{Evaluation, FeatureContext, FeatureSession, ModelAdapter};
use crate::btc::directional_model::{
    compile_submodel, RuntimeModelAction, RuntimeModelScore, RuntimeSubmodel, RuntimeSubmodelFile,
};
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Definition {
    contract: ModelContract,
    outcome: RuntimeSubmodelFile,
    calibration: Calibration,
    policy: Policy,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Calibration {
    slope: f64,
    intercept: f64,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Policy {
    start_second: i64,
    end_second: i64,
    side: String,
    minimum_confidence: f64,
    minimum_edge: f64,
    maximum_share_cost: f64,
    execution_reserve_per_share: f64,
}
#[derive(Debug)]
pub struct Adapter {
    contract: ModelContract,
    outcome: RuntimeSubmodel,
    calibration: Calibration,
    policy: Policy,
    width: usize,
    up_cost: usize,
    down_cost: usize,
    fee: usize,
}
impl Adapter {
    pub(crate) fn compile(definition: Definition, names: &[String]) -> Result<Self> {
        let Definition {
            contract,
            outcome,
            calibration,
            policy,
        } = definition;
        contract.validate()?;
        ensure!(
            contract.adapter == "time_bucket_specialist"
                && contract.missing_policy == "native_missing_branch"
                && contract.qualified_trade_size == Some(5.0),
            "invalid bucket specialist contract"
        );
        ensure!(
            calibration.slope.is_finite() && calibration.intercept.is_finite(),
            "invalid bucket calibration"
        );
        ensure!(
            (0..300).contains(&policy.start_second)
                && (policy.start_second..300).contains(&policy.end_second)
                && matches!(policy.side.as_str(), "up" | "down" | "both")
                && (0.5..=1.0).contains(&policy.minimum_confidence)
                && policy.minimum_edge.is_finite()
                && (0.0..=1.0).contains(&policy.maximum_share_cost)
                && policy.execution_reserve_per_share.is_finite()
                && policy.execution_reserve_per_share >= 0.0,
            "invalid frozen bucket policy"
        );
        super::time_bucket_features::validate_contract(&contract, names)?;
        let position = |name: &str| {
            names
                .iter()
                .position(|n| n == name)
                .ok_or_else(|| anyhow::anyhow!("missing bucket execution feature {name}"))
        };
        Ok(Self {
            up_cost: position("up_ask_vwap_5")?,
            down_cost: position("down_ask_vwap_5")?,
            fee: position("fee_rate")?,
            width: names.len(),
            contract,
            outcome: compile_submodel(outcome, names.len())?,
            calibration,
            policy,
        })
    }
    fn probability(&self, values: &[f64]) -> Result<f64> {
        ensure!(values.len() == self.width, "bucket feature width mismatch");
        let raw = self.outcome.score(values)?;
        ensure!(raw.is_finite(), "nonfinite bucket prediction");
        let clipped = raw.clamp(1e-6, 1.0 - 1e-6);
        let logit =
            (clipped / (1.0 - clipped)).ln() * self.calibration.slope + self.calibration.intercept;
        // Stable logistic, without introducing a calibration clip absent in training.
        Ok(if logit >= 0.0 {
            1.0 / (1.0 + (-logit).exp())
        } else {
            let exp = logit.exp();
            exp / (1.0 + exp)
        })
    }
}
impl ModelAdapter for Adapter {
    fn contract(&self) -> &ModelContract {
        &self.contract
    }
    fn supported_products(&self) -> &'static [&'static str] {
        super::time_bucket_features::PRODUCTS
    }
    fn policy(&self) -> serde_json::Value {
        serde_json::to_value(&self.policy).expect("finite policy")
    }
    fn directional_probability(&self, values: &[f64]) -> Result<f64> {
        self.probability(values)
    }
    fn requires_history(&self) -> bool {
        false
    }
    fn new_session(&self) -> Box<dyn FeatureSession> {
        Box::new(Session)
    }
    fn evaluate(&self, values: &[f64], seconds: i64) -> Result<Evaluation> {
        ensure!(
            (self.policy.start_second..=self.policy.end_second).contains(&seconds),
            "outside frozen bucket"
        );
        let p = self.probability(values)?;
        let up = p >= 0.5;
        let confidence = p.max(1.0 - p);
        let cost = values[if up { self.up_cost } else { self.down_cost }];
        let fee = values[self.fee];
        ensure!(fee.is_finite() && fee >= 0.0, "invalid bucket fee");
        let side_allowed =
            self.policy.side == "both" || self.policy.side == if up { "up" } else { "down" };
        let edge =
            confidence - cost - fee * cost * (1.0 - cost) - self.policy.execution_reserve_per_share;
        let accepted = side_allowed
            && cost.is_finite()
            && cost > 0.0
            && cost <= self.policy.maximum_share_cost
            && confidence >= self.policy.minimum_confidence
            && edge >= self.policy.minimum_edge;
        Ok(Evaluation {
            score: RuntimeModelScore {
                raw_logit: (p / (1.0 - p)).ln(),
                probability_up: p,
                confidence,
                action: if !accepted {
                    RuntimeModelAction::NoTrade
                } else if up {
                    RuntimeModelAction::Up
                } else {
                    RuntimeModelAction::Down
                },
                accepted,
            },
            reason: if accepted {
                "qualified"
            } else {
                "frozen_bucket_policy"
            },
            admission_probability: None,
            predicted_stress_edge: None,
            predicted_loss: None,
            temporal_std: None,
            temporal_agreement: None,
        })
    }
}
struct Session;
impl FeatureSession for Session {
    fn observe_slot(&mut self, _market: &str, _seconds: i64) {}
    fn prepare(
        &mut self,
        _adapter: &dyn ModelAdapter,
        context: &FeatureContext<'_>,
    ) -> Result<Vec<f64>> {
        super::time_bucket_features::build(context)
    }
}
