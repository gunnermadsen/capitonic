//! Frozen conservative directional probability and paper-only admission.
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
    reliability_penalty: f64,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Policy {
    start_second: i64,
    end_second: i64,
    minimum_confidence: f64,
    minimum_stressed_edge: f64,
    maximum_share_cost: f64,
    execution_reserve_per_share: f64,
    stress_slippage_per_share: f64,
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
            contract.adapter == "conservative_selective"
                && contract.missing_policy == "native_missing_branch"
                && contract.qualified_trade_size == Some(5.0),
            "invalid conservative selective contract"
        );
        ensure!(
            calibration.slope.is_finite()
                && calibration.intercept.is_finite()
                && calibration.reliability_penalty.is_finite()
                && (0.0..=0.5).contains(&calibration.reliability_penalty),
            "invalid conservative calibration"
        );
        ensure!(
            (30..=210).contains(&policy.start_second)
                && (policy.start_second..=210).contains(&policy.end_second)
                && (0.5..=1.0).contains(&policy.minimum_confidence)
                && policy.minimum_stressed_edge.is_finite()
                && (0.0..=1.0).contains(&policy.maximum_share_cost)
                && policy.execution_reserve_per_share.is_finite()
                && policy.execution_reserve_per_share >= 0.0
                && policy.stress_slippage_per_share.is_finite()
                && policy.stress_slippage_per_share >= 0.0,
            "invalid conservative paper policy"
        );
        super::time_bucket_features::validate_contract(&contract, names)?;
        let position = |name: &str| {
            names
                .iter()
                .position(|n| n == name)
                .ok_or_else(|| anyhow::anyhow!("missing execution feature {name}"))
        };
        Ok(Self {
            up_cost: position("up_ask_vwap_5")?,
            down_cost: position("down_ask_vwap_5")?,
            width: names.len(),
            contract,
            outcome: compile_submodel(outcome, names.len())?,
            calibration,
            policy,
        })
    }

    fn probability(&self, values: &[f64]) -> Result<f64> {
        ensure!(
            values.len() == self.width,
            "conservative feature width mismatch"
        );
        let raw = self.outcome.score(values)?.clamp(1e-6, 1.0 - 1e-6);
        let logit = (raw / (1.0 - raw)).ln() * self.calibration.slope + self.calibration.intercept;
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
        serde_json::to_value(&self.policy).expect("finite conservative policy")
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
            "outside conservative candidate window"
        );
        let p = self.probability(values)?;
        let up = p >= 0.5;
        let confidence = p.max(1.0 - p);
        let conservative_confidence = (confidence - self.calibration.reliability_penalty).max(0.5);
        let cost = values[if up { self.up_cost } else { self.down_cost }];
        let stressed_edge = conservative_confidence
            - cost
            - self.policy.execution_reserve_per_share
            - self.policy.stress_slippage_per_share;
        let accepted = cost.is_finite()
            && cost > 0.0
            && cost <= self.policy.maximum_share_cost
            && conservative_confidence >= self.policy.minimum_confidence
            && stressed_edge >= self.policy.minimum_stressed_edge;
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
                "conservative_paper_policy"
            },
            admission_probability: None,
            predicted_stress_edge: Some(stressed_edge),
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
