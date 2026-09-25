//! Process model selection. Packages own mathematics and schedules; the process owns sources.
use std::collections::HashSet;

use anyhow::{ensure, Context, Result};
use rust_decimal::prelude::ToPrimitive;
use serde::{Deserialize, Serialize};

use super::{
    catalog,
    contract::{ProcessBinding, SourceBinding, CONTRACT_VERSION},
};
use crate::btc::{
    directional_model::{
        runtime_model, RuntimeModelSelection, BTC_DIRECTIONAL_MODEL_STRATEGY_VERSION,
    },
    strategy::{
        BtcDecisionStrategyConfig, BtcStrategyConfig, BTC_ASYMMETRIC_VALUE_MODEL_STRATEGY_VERSION,
    },
};
use crate::models::OrderType;

pub const PROCESS_SCHEMA_VERSION: &str = "btc_realtime_paper_process_v4";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouterDefinition {
    #[serde(rename = "type")]
    pub kind: RouterKind,
    pub version: u32,
    pub routing: Routing,
    pub models: Vec<Member>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouterKind {
    UnifiedModelRouter,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Routing {
    pub mode: RoutingMode,
    pub tie_break: TieBreak,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoutingMode {
    FirstQualified,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TieBreak {
    ArrayOrder,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Member {
    pub member_id: String,
    pub selection: Selection,
    #[serde(default, skip_serializing_if = "OrderType::is_fok")]
    pub entry_order_type: OrderType,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Selection {
    BtcDirectionalModel {
        model_key: String,
        #[serde(default)]
        artifact_sha256: String,
        #[serde(default)]
        feature_schema_sha256: String,
    },
    BtcAsymmetricValueModel {
        model_key: String,
        #[serde(default)]
        artifact_sha256: String,
        #[serde(default)]
        feature_schema_sha256: String,
    },
}
impl Selection {
    pub fn identity(&self) -> RuntimeModelSelection {
        match self {
            Self::BtcDirectionalModel {
                model_key,
                artifact_sha256,
                feature_schema_sha256,
            }
            | Self::BtcAsymmetricValueModel {
                model_key,
                artifact_sha256,
                feature_schema_sha256,
            } => RuntimeModelSelection {
                model_key: model_key.clone(),
                artifact_sha256: artifact_sha256.clone(),
                feature_schema_sha256: feature_schema_sha256.clone(),
            },
        }
    }
    fn pin(&mut self, identity: RuntimeModelSelection) {
        match self {
            Self::BtcDirectionalModel {
                artifact_sha256,
                feature_schema_sha256,
                ..
            }
            | Self::BtcAsymmetricValueModel {
                artifact_sha256,
                feature_schema_sha256,
                ..
            } => {
                *artifact_sha256 = identity.artifact_sha256;
                *feature_schema_sha256 = identity.feature_schema_sha256;
            }
        }
    }
    pub fn compiled(&self) -> BtcDecisionStrategyConfig {
        let RuntimeModelSelection {
            model_key,
            artifact_sha256,
            feature_schema_sha256,
        } = self.identity();
        match self {
            Self::BtcDirectionalModel { .. } => BtcDecisionStrategyConfig::BtcDirectionalModel {
                model_key,
                artifact_sha256,
                feature_schema_sha256,
            },
            Self::BtcAsymmetricValueModel { .. } => {
                BtcDecisionStrategyConfig::BtcAsymmetricValueModel {
                    model_key,
                    artifact_sha256,
                    feature_schema_sha256,
                }
            }
        }
    }
}
impl RouterDefinition {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.version == 1, "unsupported model router version");
        ensure!(
            !self.models.is_empty() && self.models.len() <= 32,
            "router requires 1–32 members"
        );
        let mut ids = HashSet::new();
        for member in &self.models {
            ensure!(
                matches!(member.entry_order_type, OrderType::Fok | OrderType::Fak),
                "router entry_order_type must be fok or fak"
            );
            ensure!(
                !member.member_id.is_empty()
                    && member.member_id.len() <= 64
                    && member
                        .member_id
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-'),
                "invalid router member_id"
            );
            ensure!(ids.insert(&member.member_id), "duplicate router member_id");
            ensure!(
                !member.selection.identity().model_key.is_empty(),
                "empty model key"
            );
        }
        Ok(())
    }
    /// Called only by definition writes. Runtime startup requires already pinned identities.
    pub fn resolve_pins(&mut self) -> Result<()> {
        self.validate()?;
        let needs_catalog = self.models.iter().any(|m| {
            let s = m.selection.identity();
            s.artifact_sha256.is_empty() && s.feature_schema_sha256.is_empty()
        });
        let entries = if needs_catalog {
            catalog::discover()?
        } else {
            Vec::new()
        };
        for member in &mut self.models {
            let identity = member.selection.identity();
            ensure!(
                identity.artifact_sha256.is_empty() == identity.feature_schema_sha256.is_empty(),
                "supply both model checksums or neither"
            );
            if identity.artifact_sha256.is_empty() {
                let entry = entries
                    .iter()
                    .find(|e| e.model_key == identity.model_key && e.compatible)
                    .context("model key is not compatible with the runtime catalog")?;
                member.selection.pin(
                    entry
                        .selection
                        .clone()
                        .context("selected catalog entry is not a trading model")?,
                );
            }
            let model = runtime_model(&member.selection.identity())?;
            ensure!(
                model.is_asymmetric_value()
                    == matches!(member.selection, Selection::BtcAsymmetricValueModel { .. }),
                "model selection type does not match package capability"
            );
        }
        Ok(())
    }
    /// Compile member views through the existing strategy and binding validators.
    /// These views are internal runtime data, never another editable playbook contract.
    pub fn compile_members(
        &self,
        base: &BtcStrategyConfig,
        source_keys: &HashSet<&str>,
    ) -> Result<Vec<(String, BtcStrategyConfig, OrderType)>> {
        self.validate()?;
        self.models
            .iter()
            .map(|member| {
                let selection = member.selection.identity();
                ensure!(
                    !selection.artifact_sha256.is_empty()
                        && !selection.feature_schema_sha256.is_empty(),
                    "persisted router selections must contain model checksums"
                );
                let model = runtime_model(&selection)?;
                ensure!(
                    model.is_asymmetric_value()
                        == matches!(member.selection, Selection::BtcAsymmetricValueModel { .. }),
                    "model selection type does not match package capability"
                );
                if model.unified_adapter().is_none() {
                    for input in super::adapters::legacy::contract(&model).inputs {
                        ensure!(
                            !input.required || source_keys.contains(input.product.as_str()),
                            "router member {} requires global source {}",
                            member.member_id,
                            input.product
                        );
                    }
                }
                let mut strategy = base.clone();
                strategy.decision_strategy = Some(member.selection.compiled());
                strategy.strategy_version = if model.is_asymmetric_value() {
                    BTC_ASYMMETRIC_VALUE_MODEL_STRATEGY_VERSION
                } else {
                    BTC_DIRECTIONAL_MODEL_STRATEGY_VERSION
                }
                .into();
                strategy.feature_schema_version = model.feature_schema_version().into();
                strategy.unified_model = if let Some(adapter) = model.unified_adapter() {
                    let sources = adapter
                        .contract()
                        .inputs
                        .iter()
                        .filter(|input| source_keys.contains(input.product.as_str()))
                        .map(|input| SourceBinding {
                            slot: input.slot.clone(),
                            product: input.product.clone(),
                            semantics: input.semantics.clone(),
                        })
                        .collect();
                    let binding = ProcessBinding {
                        version: CONTRACT_VERSION.into(),
                        sources,
                        policy: adapter.policy(),
                    };
                    binding.validate(
                        adapter,
                        strategy
                            .target_size
                            .to_f64()
                            .context("invalid model trade size")?,
                    )?;
                    Some(binding)
                } else {
                    None
                };
                let policy = model.prediction_policy();
                strategy.min_seconds_after_open = base
                    .min_seconds_after_open
                    .max(policy.minimum_seconds_after_open);
                strategy.min_seconds_before_close = base
                    .min_seconds_before_close
                    .max(300 - policy.maximum_seconds_after_open);
                ensure!(
                    strategy.min_seconds_after_open <= 300 - strategy.min_seconds_before_close,
                    "process window excludes router member {}",
                    member.member_id
                );
                strategy.validate()?;
                Ok((member.member_id.clone(), strategy, member.entry_order_type))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn definition() -> serde_json::Value {
        serde_json::json!({"type":"unified_model_router","version":1,"routing":{"mode":"first_qualified","tie_break":"array_order"},"models":[{"member_id":"first","selection":{"type":"btc_directional_model","model_key":"test"}}]})
    }
    #[test]
    fn strict_router_shape_and_member_identity() {
        let mut value = definition();
        let router: RouterDefinition = serde_json::from_value(value.clone()).unwrap();
        router.validate().unwrap();
        let duplicate = value["models"][0].clone();
        value["models"].as_array_mut().unwrap().push(duplicate);
        assert!(serde_json::from_value::<RouterDefinition>(value)
            .unwrap()
            .validate()
            .is_err());
        let mut value = definition();
        value["sources"] = serde_json::json!([]);
        assert!(serde_json::from_value::<RouterDefinition>(value).is_err());
        let mut value = definition();
        value["models"][0]["sources"] = serde_json::json!([]);
        assert!(serde_json::from_value::<RouterDefinition>(value).is_err());
        let mut value = definition();
        value["routing"]["mode"] = serde_json::json!("all_qualified");
        assert!(serde_json::from_value::<RouterDefinition>(value).is_err());
    }
    #[test]
    fn startup_never_resolves_unpinned_model() {
        let router: RouterDefinition = serde_json::from_value(definition()).unwrap();
        assert!(router
            .compile_members(&BtcStrategyConfig::default(), &HashSet::new())
            .unwrap_err()
            .to_string()
            .contains("checksums"));
    }
    #[test]
    fn entry_order_type_is_member_scoped_and_defaults_to_fok() {
        let mut value = definition();
        value["models"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "member_id": "second",
                "selection": {"type": "btc_directional_model", "model_key": "test"},
                "entry_order_type": "fak"
            }));
        let router: RouterDefinition = serde_json::from_value(value).unwrap();
        assert_eq!(router.models[0].entry_order_type, OrderType::Fok);
        assert_eq!(router.models[1].entry_order_type, OrderType::Fak);
        let saved = serde_json::to_value(&router).unwrap();
        assert!(saved["models"][0].get("entry_order_type").is_none());
        assert_eq!(saved["models"][1]["entry_order_type"], "fak");
        let mut invalid = saved;
        invalid["models"][1]["entry_order_type"] = "gtc".into();
        assert!(serde_json::from_value::<RouterDefinition>(invalid)
            .unwrap()
            .validate()
            .is_err());
    }
}
