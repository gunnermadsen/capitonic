use super::*;

pub(super) const BTC_PIPELINE_VERSION: &str = "btc_realtime_paper_pipeline_v11";
pub(super) const BTC_PROCESS_SCHEMA_VERSION: &str = "btc_realtime_paper_process_v2";
pub(super) const SELECTABLE_BTC_PIPELINE_VERSION: &str = "btc_realtime_paper_pipeline_v12";
pub(super) const SELECTABLE_BTC_PROCESS_SCHEMA_VERSION: &str = "btc_realtime_paper_process_v3";
pub(super) const ROUTER_BTC_PIPELINE_VERSION: &str = "btc_realtime_paper_pipeline_v13";
pub(super) const AGENT_PROCESS_SCHEMA_VERSION: &str = "btc_realtime_paper_process_v5";
pub(super) const AGENT_BTC_PIPELINE_VERSION: &str = "btc_realtime_paper_pipeline_v14";
pub(super) const LEGACY_BTC_PROCESS_SCHEMA_VERSION: &str = "btc_realtime_paper_process_v1";
pub(super) const BTC_PROCESS_TYPE: &str = "btc_5m";
pub(super) const BTC_PROCESS_SCOPE: &str = "realtime_paper";
pub(super) const BTC_RUNTIME_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(45);
pub(super) const BTC_LIVE_QUIESCE_TIMEOUT: Duration = Duration::from_secs(10);
pub(super) const BTC_LIVE_CONTROL_TIMEOUT: Duration = Duration::from_secs(20);
pub(super) const GRAFANA_STATUS_READ_TIMEOUT: Duration = Duration::from_millis(25);
pub(super) const COMPILED_SOURCE_IDENTITY: &str = env!("POLYMARKET_COMPILED_SOURCE_ID");

pub(super) fn should_resume_configured_live_entries(
    process: &TradingProcess,
    execution: &EffectiveProcessExecutionConfig,
) -> bool {
    process.enabled
        && process.status == "running"
        && execution.mode == "live"
        && execution.execute_signals
        && execution.live_capital
}

pub(super) async fn preflight_btc_run_identity(
    repository: &BtcRepository,
    run_key: &str,
    run_id: uuid::Uuid,
) -> Result<()> {
    let identity_exists = repository
        .run_manifest_exists(run_id, run_key)
        .await
        .context("failed to preflight immutable BTC run identity")?;
    if identity_exists {
        bail!(
            "BTC run identity {run_key} already exists; every new explicit start requires a globally unique run key"
        );
    }
    Ok(())
}

pub(super) async fn mark_btc_process_terminal(
    pool: &PgPool,
    process_id: uuid::Uuid,
    status: &str,
    reason: &str,
    allow_inactive_process: bool,
) -> Result<()> {
    validate_btc_process_terminal_request(status, reason)?;
    let mut tx = pool
        .begin()
        .await
        .context("failed to begin BTC process terminal transaction")?;
    let process_update = sqlx::query(
        r#"
        UPDATE polymarket.trading_processes
        SET status = $2,
            enabled = false,
            stopped_at = now(),
            stop_reason = $3,
            last_error = CASE WHEN $2 = 'failed' THEN $3 ELSE last_error END,
            updated_at = now()
        WHERE process_id = $1
          AND status IN ('starting','running','stopping')
        "#,
    )
    .bind(process_id)
    .bind(status)
    .bind(reason)
    .execute(&mut *tx)
    .await
    .context("failed to mark BTC process terminal")?;
    if process_update.rows_affected() == 0 {
        let existing_status = sqlx::query_scalar::<_, String>(
            r#"
            SELECT status
            FROM polymarket.trading_processes
            WHERE process_id = $1
            "#,
        )
        .bind(process_id)
        .fetch_optional(&mut *tx)
        .await
        .context("failed to inspect existing BTC process terminal state")?;
        match existing_status {
            Some(existing_status) if existing_status == status => {}
            Some(existing_status)
                if allow_inactive_process
                    && matches!(
                        existing_status.as_str(),
                        "created" | "stopped" | "failed" | "completed" | "expired"
                    ) => {}
            Some(existing_status) => {
                bail!("cannot overwrite BTC process terminal state {existing_status} with {status}")
            }
            None => bail!("BTC process disappeared during terminal transition"),
        }
    }
    tx.commit()
        .await
        .context("failed to commit BTC process terminal transaction")?;
    Ok(())
}

pub(super) fn validate_btc_process_terminal_request(status: &str, reason: &str) -> Result<()> {
    if !matches!(status, "stopped" | "failed" | "completed") || reason.trim().is_empty() {
        bail!("invalid BTC process terminal status or reason");
    }
    Ok(())
}

#[derive(Clone)]
pub(super) struct BtcProcessManagerConfig {
    pub(super) live_venue: Option<Arc<LiveVenue>>,
    pub(super) live_reconcile_interval: Duration,
    pub(super) grafana_live_enabled: bool,
}

pub(super) fn shared_market_data_config_compatible(
    left: &BtcRuntimeConfig,
    right: &BtcRuntimeConfig,
) -> bool {
    let _ = (left, right);
    true
}

pub(super) fn shared_runtime_recovery_required(
    active_process_count: usize,
    shared_running: bool,
) -> bool {
    active_process_count > 0 && !shared_running
}

pub(super) async fn invalidate_shared_market_data_evidence(
    state: &Arc<tokio::sync::RwLock<polymarket_bot::btc::RealtimeState>>,
    books: &Arc<tokio::sync::RwLock<BookRegistry>>,
) {
    *state.write().await = polymarket_bot::btc::RealtimeState::default();
    *books.write().await = BookRegistry::new(uuid::Uuid::new_v4());
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct BtcRealtimePaperControlConfig {
    pub(super) schema_version: String,
    #[serde(default)]
    pub(super) playbook_version: Option<String>,
    #[serde(default)]
    pub(super) sources: Vec<SourceSelector>,
    pub(super) next_experiment_key: String,
    pub(super) preregistration_sha256: String,
    pub(super) strategy: serde_json::Value,
    pub(super) entry_admission: Option<BtcEntryAdmissionConfig>,
    #[serde(default)]
    pub(super) risk_strategies: Vec<RiskStrategySelection>,
    pub(super) runtime: BtcProcessRuntimeControl,
    pub(super) paper: BtcProcessPaperControl,
}

impl Default for BtcRealtimePaperControlConfig {
    fn default() -> Self {
        Self {
            schema_version: String::new(),
            playbook_version: None,
            sources: Vec::new(),
            next_experiment_key: String::new(),
            preregistration_sha256: String::new(),
            strategy: serde_json::json!({}),
            entry_admission: None,
            risk_strategies: Vec::new(),
            runtime: BtcProcessRuntimeControl::default(),
            paper: BtcProcessPaperControl::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BtcDefinitionUse {
    InactiveDefinition,
    ExplicitStart,
    DurableResume,
}

pub(super) fn parse_btc_process_control(
    mut value: serde_json::Value,
    definition_use: BtcDefinitionUse,
) -> Result<BtcRealtimePaperControlConfig, HttpError> {
    let schema_version = value
        .get("schema_version")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| HttpError::bad_request("BTC process schema_version is required"))?;

    if schema_version == LEGACY_BTC_PROCESS_SCHEMA_VERSION {
        if definition_use != BtcDefinitionUse::DurableResume {
            return Err(HttpError::bad_request(format!(
                "BTC process schema_version {LEGACY_BTC_PROCESS_SCHEMA_VERSION} is resume-only"
            )));
        }
        let object = value.as_object_mut().ok_or_else(|| {
            HttpError::bad_request("BTC process control configuration must be an object")
        })?;
        if let Some(retired) = object.remove("ml_shadow") {
            let retired = retired.as_object().ok_or_else(|| {
                HttpError::bad_request("legacy ml_shadow compatibility value must be an object")
            })?;
            if retired.keys().any(|key| key != "enabled")
                || retired
                    .get("enabled")
                    .is_some_and(|enabled| !enabled.is_boolean())
            {
                return Err(HttpError::bad_request(
                    "legacy ml_shadow compatibility value may contain only a boolean enabled field",
                ));
            }
        }
        object.insert(
            "schema_version".into(),
            serde_json::Value::String(BTC_PROCESS_SCHEMA_VERSION.into()),
        );
    } else if !matches!(
        schema_version,
        BTC_PROCESS_SCHEMA_VERSION
            | SELECTABLE_BTC_PROCESS_SCHEMA_VERSION
            | ROUTER_PROCESS_SCHEMA_VERSION
            | AGENT_PROCESS_SCHEMA_VERSION
    ) {
        return Err(HttpError::bad_request(format!(
            "unsupported BTC process schema_version {schema_version}"
        )));
    }
    let mut control: BtcRealtimePaperControlConfig = serde_json::from_value(value)
        .map_err(|e| HttpError::bad_request(format!("invalid BTC process configuration: {e}")))?;
    if control.sources.is_empty() {
        if definition_use == BtcDefinitionUse::DurableResume {
            control.sources = polymarket_bot::market_data_stream::legacy_default_sources();
        } else {
            return Err(HttpError::bad_request(
                "BTC process sources are required; update the playbook to version v1.2",
            ));
        }
    } else if control.playbook_version.as_deref() != Some("v1.2") {
        return Err(HttpError::bad_request(
            "BTC process sources require playbook_version v1.2",
        ));
    }
    for source in &control.sources {
        source
            .validate()
            .map_err(|error| HttpError::bad_request(error.to_string()))?;
    }
    let unique = control
        .sources
        .iter()
        .map(|source| source.key.as_str())
        .collect::<HashSet<_>>();
    if unique.len() != control.sources.len() {
        return Err(HttpError::bad_request("BTC process sources must be unique"));
    }
    Ok(control)
}

pub(super) fn resolve_legacy_btc_strategy(
    control: &BtcRealtimePaperControlConfig,
) -> Result<BtcStrategyConfig, HttpError> {
    let overrides = control
        .strategy
        .as_object()
        .ok_or_else(|| HttpError::bad_request("strategy must be an object"))?;
    if !overrides.contains_key("target_size") {
        return Err(HttpError::bad_request("strategy.target_size is required"));
    }
    let selectable = control.schema_version == SELECTABLE_BTC_PROCESS_SCHEMA_VERSION;
    if selectable && !overrides.contains_key("decision_strategy") {
        return Err(HttpError::bad_request(format!(
            "{SELECTABLE_BTC_PROCESS_SCHEMA_VERSION} requires strategy.decision_strategy"
        )));
    }
    if !selectable && overrides.contains_key("decision_strategy") {
        return Err(HttpError::bad_request(format!(
            "strategy.decision_strategy requires {SELECTABLE_BTC_PROCESS_SCHEMA_VERSION}"
        )));
    }
    for key in ["strategy_version", "feature_schema_version"] {
        if selectable && overrides.contains_key(key) {
            return Err(HttpError::bad_request(format!(
                "strategy.{key} is a compiled identity"
            )));
        }
    }

    let mut value = serde_json::to_value(BtcStrategyConfig::default())
        .map_err(|error| HttpError::internal(error.to_string()))?;
    let object = value.as_object_mut().expect("strategy object");
    object.insert("decision_strategy".into(), serde_json::Value::Null);
    object.insert("unified_model".into(), serde_json::Value::Null);
    object.insert(
        "max_directional_feature_age_ms".into(),
        serde_json::Value::Null,
    );
    object.insert("required_model_feeds".into(), serde_json::json!([]));
    for (key, override_value) in overrides {
        let slot = object
            .get_mut(key)
            .ok_or_else(|| HttpError::bad_request(format!("unsupported strategy setting {key}")))?;
        *slot = override_value.clone();
    }

    if selectable {
        let selection: BtcDecisionStrategyConfig = serde_json::from_value(
            overrides
                .get("decision_strategy")
                .cloned()
                .expect("selectable contract requires a decision strategy"),
        )
        .map_err(|error| HttpError::bad_request(format!("invalid strategy selection: {error}")))?;
        let (strategy_version, feature_schema_version) = match &selection {
            BtcDecisionStrategyConfig::OpenaiAgent { .. } => {
                return Err(HttpError::bad_request(
                    "openai_agent requires process schema v5",
                ))
            }
            BtcDecisionStrategyConfig::BtcDirectionalModel {
                model_key,
                artifact_sha256,
                feature_schema_sha256,
            } => {
                let model = runtime_model(&RuntimeModelSelection {
                    model_key: model_key.clone(),
                    artifact_sha256: artifact_sha256.clone(),
                    feature_schema_sha256: feature_schema_sha256.clone(),
                })
                .map_err(|error| HttpError::bad_request(error.to_string()))?;
                (
                    polymarket_bot::btc::BTC_DIRECTIONAL_MODEL_STRATEGY_VERSION.to_string(),
                    model.feature_schema_version().to_string(),
                )
            }
            BtcDecisionStrategyConfig::BtcAsymmetricValueModel {
                model_key,
                artifact_sha256,
                feature_schema_sha256,
            } => {
                let model = runtime_model(&RuntimeModelSelection {
                    model_key: model_key.clone(),
                    artifact_sha256: artifact_sha256.clone(),
                    feature_schema_sha256: feature_schema_sha256.clone(),
                })
                .map_err(|error| HttpError::bad_request(error.to_string()))?;
                if !model.is_asymmetric_value() {
                    return Err(HttpError::bad_request(
                        "selected model is not an asymmetric value artifact",
                    ));
                }
                (
                    polymarket_bot::btc::BTC_ASYMMETRIC_VALUE_MODEL_STRATEGY_VERSION.to_string(),
                    model.feature_schema_version().to_string(),
                )
            }
        };
        object.insert(
            "strategy_version".into(),
            serde_json::Value::String(strategy_version),
        );
        object.insert(
            "feature_schema_version".into(),
            serde_json::Value::String(feature_schema_version),
        );
    }

    let strategy: BtcStrategyConfig = serde_json::from_value(value)
        .map_err(|error| HttpError::bad_request(format!("invalid strategy settings: {error}")))?;
    strategy
        .validate()
        .map_err(|error| HttpError::bad_request(error.to_string()))?;
    Ok(strategy)
}

pub(super) fn resolve_btc_members(
    control: &BtcRealtimePaperControlConfig,
) -> Result<Vec<(String, BtcStrategyConfig, polymarket_bot::models::OrderType)>, HttpError> {
    if control.schema_version != ROUTER_PROCESS_SCHEMA_VERSION
        && control.schema_version != AGENT_PROCESS_SCHEMA_VERSION
    {
        return Ok(vec![(
            "legacy_primary".into(),
            resolve_legacy_btc_strategy(control)?,
            polymarket_bot::models::OrderType::Fok,
        )]);
    }
    let overrides = control
        .strategy
        .as_object()
        .ok_or_else(|| HttpError::bad_request("strategy must be an object"))?;
    if !overrides.contains_key("target_size") {
        return Err(HttpError::bad_request("strategy.target_size is required"));
    }
    for forbidden in [
        "unified_model",
        "strategy_version",
        "feature_schema_version",
    ] {
        if overrides.contains_key(forbidden) {
            return Err(HttpError::bad_request(format!(
                "strategy.{forbidden} is not configurable"
            )));
        }
    }
    let router: RouterDefinition = serde_json::from_value(
        overrides
            .get("decision_strategy")
            .cloned()
            .ok_or_else(|| HttpError::bad_request("missing router selection"))?,
    )
    .map_err(|e| HttpError::bad_request(format!("invalid router: {e}")))?;
    if router.models.iter().any(|m| {
        matches!(
            m.selection,
            polymarket_bot::btc::unified_model_runtime::router::Selection::OpenaiAgent { .. }
        )
    }) && control.schema_version != AGENT_PROCESS_SCHEMA_VERSION
    {
        return Err(HttpError::bad_request(
            "openai_agent requires process schema v5",
        ));
    }
    let mut value = serde_json::to_value(BtcStrategyConfig::default())
        .map_err(|e| HttpError::internal(e.to_string()))?;
    let object = value.as_object_mut().expect("strategy object");
    object.insert(
        "max_directional_feature_age_ms".into(),
        serde_json::Value::Null,
    );
    object.insert("required_model_feeds".into(), serde_json::json!([]));
    for (key, v) in overrides {
        if key == "decision_strategy" {
            continue;
        }
        let slot = object
            .get_mut(key)
            .ok_or_else(|| HttpError::bad_request(format!("unsupported strategy setting {key}")))?;
        *slot = v.clone();
    }
    let base: BtcStrategyConfig =
        serde_json::from_value(value).map_err(|e| HttpError::bad_request(e.to_string()))?;
    let sources = control.sources.iter().map(|s| s.key.as_str()).collect();
    router
        .compile_members(&base, &sources)
        .map_err(|e| HttpError::bad_request(e.to_string()))
}

#[cfg(test)]
pub(super) fn resolve_btc_strategy(
    control: &BtcRealtimePaperControlConfig,
) -> Result<BtcStrategyConfig, HttpError> {
    Ok(resolve_btc_members(control)?.remove(0).1)
}
pub(super) fn pin_router_config(config: &mut TradingProcessConfig) -> Result<(), HttpError> {
    let Some(control) = config.raw.get_mut("btc_realtime_paper") else {
        return Ok(());
    };
    if !matches!(
        control["schema_version"].as_str(),
        Some(ROUTER_PROCESS_SCHEMA_VERSION | AGENT_PROCESS_SCHEMA_VERSION)
    ) {
        return Ok(());
    }
    let selected = control
        .pointer_mut("/strategy/decision_strategy")
        .ok_or_else(|| HttpError::bad_request("missing router"))?;
    let mut router: RouterDefinition = serde_json::from_value(selected.clone())
        .map_err(|e| HttpError::bad_request(e.to_string()))?;
    router
        .resolve_pins()
        .map_err(|e| HttpError::bad_request(e.to_string()))?;
    *selected = serde_json::to_value(router).map_err(|e| HttpError::internal(e.to_string()))?;
    Ok(())
}

pub(super) fn validate_directional_model_entry_policy(
    strategy: &BtcStrategyConfig,
    entry_policy: BtcDirectionalModelEntryPolicy,
) -> Result<(), HttpError> {
    if entry_policy == BtcDirectionalModelEntryPolicy::ExecuteDirectionalPrediction
        && !matches!(
            strategy.decision_strategy.as_ref(),
            Some(
                BtcDecisionStrategyConfig::BtcDirectionalModel { .. }
                    | BtcDecisionStrategyConfig::OpenaiAgent { .. }
            )
        )
    {
        return Err(HttpError::bad_request(
            "paper.directional_model_entry_policy execute_directional_prediction requires the BTC directional-model strategy",
        ));
    }
    Ok(())
}

pub(super) fn validate_btc_entry_timing(strategy: &BtcStrategyConfig) -> Result<(), HttpError> {
    let fixed_120_directional_model = matches!(
        strategy.decision_strategy.as_ref(),
        Some(BtcDecisionStrategyConfig::BtcDirectionalModel { .. })
    ) && strategy.min_seconds_after_open == 120
        && strategy.min_seconds_before_close == 180;
    let timing_valid = strategy.min_seconds_after_open >= 0
        && strategy.min_seconds_before_close > 0
        && strategy
            .min_seconds_after_open
            .checked_add(strategy.min_seconds_before_close)
            .is_some_and(|entry_gate_seconds| {
                entry_gate_seconds < 300
                    || (entry_gate_seconds == 300 && fixed_120_directional_model)
            });
    if !timing_valid {
        return Err(HttpError::bad_request(
            "BTC entry timing gates leave no tradable portion of a five-minute window",
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct BtcProcessRuntimeControl {
    pub(super) strategy_interval_ms: u64,
}

impl Default for BtcProcessRuntimeControl {
    fn default() -> Self {
        Self {
            strategy_interval_ms: 1_000,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct BtcProcessPaperControl {
    pub(super) arrival_latency_ms: u64,
    pub(super) visible_depth_haircut: rust_decimal::Decimal,
    pub(super) starting_collateral_usd: rust_decimal::Decimal,
    pub(super) directional_model_entry_policy: BtcDirectionalModelEntryPolicy,
    pub(super) stress_previews: Vec<BtcProcessPaperPreviewControl>,
}

impl Default for BtcProcessPaperControl {
    fn default() -> Self {
        Self {
            arrival_latency_ms: 150,
            visible_depth_haircut: dec!(0.80),
            starting_collateral_usd: dec!(1000),
            directional_model_entry_policy:
                BtcDirectionalModelEntryPolicy::RequirePositiveDirectEdge,
            stress_previews: vec![
                BtcProcessPaperPreviewControl {
                    scenario_key: "latency_300ms_depth_65pct".to_string(),
                    arrival_latency_ms: 300,
                    visible_depth_haircut: dec!(0.65),
                },
                BtcProcessPaperPreviewControl {
                    scenario_key: "latency_600ms_depth_50pct".to_string(),
                    arrival_latency_ms: 600,
                    visible_depth_haircut: dec!(0.50),
                },
            ],
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct BtcProcessPaperPreviewControl {
    pub(super) scenario_key: String,
    pub(super) arrival_latency_ms: u64,
    pub(super) visible_depth_haircut: rust_decimal::Decimal,
}

#[derive(Clone)]
pub(super) struct ResolvedBtcProcessDefinition {
    pub(super) control: BtcRealtimePaperControlConfig,
    pub(super) strategy: BtcStrategyConfig,
    pub(super) entry_admission: Option<BtcEntryAdmissionConfig>,
    pub(super) risk_strategies: Vec<RiskStrategySelection>,
    pub(super) runtime: BtcRuntimeConfig,
    pub(super) paper_venue: PaperVenueConfig,
    pub(super) paper_stress_previews: Vec<PaperPreviewConfig>,
}

pub(super) struct PreparedBtcStartDefinition {
    pub(super) run_id: uuid::Uuid,
    pub(super) run_key: String,
    pub(super) preregistration_sha256: String,
    pub(super) strategy: BtcStrategyConfig,
    pub(super) sources: Vec<SourceSelector>,
    pub(super) entry_admission: Option<BtcEntryAdmissionConfig>,
    pub(super) risk_strategies: Vec<RiskStrategySelection>,
    pub(super) directional_model_entry_policy: BtcDirectionalModelEntryPolicy,
    pub(super) runtime: BtcRuntimeConfig,
    pub(super) paper_venue: PaperVenueConfig,
    pub(super) paper_stress_previews: Vec<PaperPreviewConfig>,
    pub(super) execution_mode: BtcExecutionMode,
    pub(super) execution: EffectiveProcessExecutionConfig,
    pub(super) frozen_process_config: TradingProcessConfig,
    pub(super) config_hash: String,
}

pub(super) fn merge_source_selectors(
    selectors: impl IntoIterator<Item = SourceSelector>,
) -> Result<Vec<SourceSelector>, HttpError> {
    let mut by_key: BTreeMap<String, SourceSelector> = BTreeMap::new();
    for selector in selectors {
        if let Some(existing) = by_key.get(&selector.key) {
            // An optional consumer must not change an established required
            // subscription. Adapter freshness checks remain process-local.
            if existing.contract_version == selector.contract_version
                && existing.required != selector.required
            {
                if selector.required {
                    by_key.insert(selector.key.clone(), selector);
                }
                continue;
            }
            if existing != &selector {
                return Err(HttpError::conflict(format!(
                    "BTC source {} has incompatible selector settings across active processes",
                    selector.key
                )));
            }
            continue;
        }
        by_key.insert(selector.key.clone(), selector);
    }
    Ok(by_key.into_values().collect())
}

pub(super) fn merge_runtime_source_selectors(
    selectors: impl IntoIterator<Item = SourceSelector>,
    grafana_live_enabled: bool,
) -> Result<Vec<SourceSelector>, HttpError> {
    merge_source_selectors(selectors.into_iter().chain(grafana_live_enabled.then(|| {
        // Observability owns this optional subscription. It must stay independent
        // of playbook inputs and must never become a trading-readiness gate.
        SourceSelector {
            key: polymarket_bot::market_data_stream::PRODUCT_TWAP.to_string(),
            contract_version: polymarket_bot::market_data_stream::CONTRACT_VERSION,
            required: false,
            maximum_age_ms: Some(120_000),
            require_sequence_integrity: false,
        }
    })))
}

pub(super) const BTC_LIVE_EXECUTION_FRESHNESS_LIMIT_MS: i64 = 2_000;

pub(super) fn validate_btc_start_eligibility(process: &TradingProcess) -> Result<(), HttpError> {
    validate_btc_process_capability(process)?;
    validate_btc_execution_activation(process)?;
    if process.enabled || matches!(process.status.as_str(), "starting" | "running" | "stopping") {
        return Err(HttpError::conflict(
            "BTC trading process already claims an active lifecycle",
        ));
    }
    if !matches!(
        process.status.as_str(),
        "created" | "stopped" | "failed" | "completed"
    ) {
        return Err(HttpError::bad_request(format!(
            "BTC trading process cannot start from status {}",
            process.status
        )));
    }
    Ok(())
}

pub(super) fn validate_btc_process_capability(process: &TradingProcess) -> Result<(), HttpError> {
    if !BtcProcessManager::is_managed_process(process) {
        return Err(HttpError::bad_request(
            "process is not a managed BTC realtime execution process",
        ));
    }
    let execution = process.effective_execution();
    validate_optional_execution_controls(&execution)?;
    match execution.mode.as_str() {
        "paper" => {
            if execution.live_capital {
                return Err(HttpError::bad_request(
                    "BTC paper execution cannot enable live capital",
                ));
            }
            if !execution.execute_signals {
                return Err(HttpError::bad_request(
                    "BTC paper execution requires execution.execute_signals=true",
                ));
            }
            if execution.account_ref.is_some() {
                return Err(HttpError::bad_request(
                    "BTC paper execution cannot select a live account_ref",
                ));
            }
        }
        "live" => {
            let account_ref = execution
                .account_ref
                .as_deref()
                .map(str::trim)
                .unwrap_or("");
            if account_ref.is_empty()
                || account_ref.len() > 128
                || !account_ref.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b':')
                })
            {
                return Err(HttpError::bad_request(
                    "BTC live execution requires a bounded account_ref slug",
                ));
            }
            if execution.execute_signals != execution.live_capital {
                return Err(HttpError::bad_request(
                    "BTC live execution must enable execute_signals and live_capital together",
                ));
            }
        }
        _ => {
            return Err(HttpError::bad_request(
                "BTC execution.mode must be paper or live",
            ));
        }
    }
    Ok(())
}

pub(super) fn validate_optional_execution_controls(
    execution: &EffectiveProcessExecutionConfig,
) -> Result<(), HttpError> {
    for (name, value, maximum) in [
        (
            "max_order_notional_usd",
            execution.max_order_notional_usd,
            dec!(5),
        ),
        (
            "max_open_notional_usd",
            execution.max_open_notional_usd,
            dec!(30),
        ),
        ("max_daily_loss_usd", execution.max_daily_loss_usd, dec!(10)),
    ] {
        if value.is_some_and(|value| value <= Decimal::ZERO || value > maximum) {
            return Err(HttpError::bad_request(format!(
                "BTC execution.{name} must be greater than zero and at most {maximum}"
            )));
        }
    }
    if execution
        .max_open_positions
        .is_some_and(|value| value == 0 || value > 6)
    {
        return Err(HttpError::bad_request(
            "BTC execution.max_open_positions must be between 1 and 6",
        ));
    }
    Ok(())
}

pub(super) fn validate_btc_execution_activation(process: &TradingProcess) -> Result<(), HttpError> {
    let execution = process.effective_execution();
    match execution.mode.as_str() {
        "paper" if execution.execute_signals && !execution.live_capital => Ok(()),
        "live" if execution.execute_signals && execution.live_capital => Ok(()),
        "paper" => Err(HttpError::bad_request(
            "BTC paper start requires execution.execute_signals=true",
        )),
        "live" => Err(HttpError::bad_request(
            "BTC live start requires execution.execute_signals=true and execution.live_capital=true; credential-only definitions cannot start",
        )),
        _ => Err(HttpError::bad_request(
            "BTC execution.mode must be paper or live",
        )),
    }
}

pub(super) fn validate_btc_live_model_authorization(
    strategy: &BtcStrategyConfig,
) -> Result<(), HttpError> {
    let (model_key, artifact_sha256, feature_schema_sha256, require_asymmetric_value) =
        match strategy.decision_strategy.as_ref() {
            Some(BtcDecisionStrategyConfig::BtcDirectionalModel {
                model_key,
                artifact_sha256,
                feature_schema_sha256,
            }) => (model_key, artifact_sha256, feature_schema_sha256, false),
            Some(BtcDecisionStrategyConfig::BtcAsymmetricValueModel {
                model_key,
                artifact_sha256,
                feature_schema_sha256,
            }) => (model_key, artifact_sha256, feature_schema_sha256, true),
            _ => {
                return Err(HttpError::bad_request(
                    "BTC live execution currently requires an immutable model artifact",
                ));
            }
        };
    let model = runtime_model(&RuntimeModelSelection {
        model_key: model_key.clone(),
        artifact_sha256: artifact_sha256.clone(),
        feature_schema_sha256: feature_schema_sha256.clone(),
    })
    .map_err(|error| HttpError::bad_request(format!("invalid live model artifact: {error}")))?;
    if require_asymmetric_value && !model.is_asymmetric_value() {
        return Err(HttpError::bad_request(
            "BTC asymmetric-value live execution requires an asymmetric-value artifact",
        ));
    }
    if !model.live_capital_allowed() {
        return Err(HttpError::conflict(format!(
            "model {} is not authorized for live capital (deployment_scope={}, production_qualified={})",
            model.model_key(),
            model.deployment_scope().unwrap_or("unspecified"),
            model.production_qualified()
        )));
    }
    Ok(())
}

pub(super) fn validate_btc_live_execution_freshness(
    strategy: &BtcStrategyConfig,
) -> Result<(), HttpError> {
    if strategy.max_reference_age_ms > BTC_LIVE_EXECUTION_FRESHNESS_LIMIT_MS
        || strategy.max_book_age_ms > BTC_LIVE_EXECUTION_FRESHNESS_LIMIT_MS
    {
        return Err(HttpError::bad_request(format!(
            "BTC live execution requires max_reference_age_ms and max_book_age_ms at or below {BTC_LIVE_EXECUTION_FRESHNESS_LIMIT_MS}"
        )));
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn prepare_btc_start_definition(
    resolved: ResolvedBtcProcessDefinition,
) -> Result<PreparedBtcStartDefinition, HttpError> {
    prepare_btc_start_definition_for_execution(
        resolved,
        &EffectiveProcessExecutionConfig {
            mode: "paper".to_string(),
            execute_signals: true,
            live_capital: false,
            account_ref: None,
            taker_fee_rate: dec!(0.03),
            max_order_notional_usd: None,
            max_open_notional_usd: None,
            max_open_positions: None,
            max_daily_loss_usd: None,
            require_exit_book: None,
        },
    )
}

pub(super) fn prepare_btc_start_definition_for_execution(
    resolved: ResolvedBtcProcessDefinition,
    execution: &EffectiveProcessExecutionConfig,
) -> Result<PreparedBtcStartDefinition, HttpError> {
    let ResolvedBtcProcessDefinition {
        control,
        strategy,
        entry_admission,
        risk_strategies,
        runtime,
        paper_venue,
        paper_stress_previews,
    } = resolved;
    let directional_model_entry_policy = control.paper.directional_model_entry_policy;
    let sources = control.sources.clone();
    risk_runtime::validate_selections(&risk_strategies)
        .map_err(|error| HttpError::bad_request(error.to_string()))?;
    if let Some(binding) = &strategy.unified_model {
        for input in &binding.sources {
            if !sources.iter().any(|source| source.key == input.product) {
                return Err(HttpError::bad_request(format!(
                    "UMR binding {} requires process stream {}",
                    input.slot, input.product
                )));
            }
        }
    }
    let (pipeline_version, process_schema_version) = match control.schema_version.as_str() {
        BTC_PROCESS_SCHEMA_VERSION => (BTC_PIPELINE_VERSION, BTC_PROCESS_SCHEMA_VERSION),
        SELECTABLE_BTC_PROCESS_SCHEMA_VERSION => (
            SELECTABLE_BTC_PIPELINE_VERSION,
            SELECTABLE_BTC_PROCESS_SCHEMA_VERSION,
        ),
        ROUTER_PROCESS_SCHEMA_VERSION => {
            (ROUTER_BTC_PIPELINE_VERSION, ROUTER_PROCESS_SCHEMA_VERSION)
        }
        AGENT_PROCESS_SCHEMA_VERSION => (AGENT_BTC_PIPELINE_VERSION, AGENT_PROCESS_SCHEMA_VERSION),
        schema_version => {
            return Err(HttpError::internal(format!(
                "resolved unsupported BTC process schema {schema_version}"
            )))
        }
    };
    let run_key = control.next_experiment_key;
    let preregistration_sha256 = control.preregistration_sha256;
    let execution_mode = match execution.mode.as_str() {
        "paper" => BtcExecutionMode::Paper,
        "live" => BtcExecutionMode::Live,
        _ => {
            return Err(HttpError::bad_request(
                "BTC execution.mode must be paper or live",
            ))
        }
    };
    if strategy.agent_selection().is_some() && execution_mode != BtcExecutionMode::Paper {
        return Err(HttpError::bad_request(
            "openai_agent supports paper execution only",
        ));
    }
    if execution_mode == BtcExecutionMode::Live {
        validate_btc_live_execution_freshness(&strategy)?;
    }
    let run_namespace = match execution_mode {
        BtcExecutionMode::Paper => "polymarket-bot/btc-paper",
        BtcExecutionMode::Live => "polymarket-bot/btc-live",
    };
    let run_id = uuid::Uuid::new_v5(
        &uuid::Uuid::NAMESPACE_URL,
        format!("{run_namespace}/{run_key}").as_bytes(),
    );
    let frozen_strategy = if matches!(
        process_schema_version,
        ROUTER_PROCESS_SCHEMA_VERSION | AGENT_PROCESS_SCHEMA_VERSION
    ) {
        control.strategy.clone()
    } else {
        serde_json::to_value(&strategy).map_err(|error| HttpError::internal(error.to_string()))?
    };
    let mut frozen_raw = serde_json::json!({
        "pipeline_version": pipeline_version,
        "process_schema_version": process_schema_version,
        "preregistration_sha256": &preregistration_sha256,
        "build": {
            "package_version": env!("CARGO_PKG_VERSION"),
            "compiled_source_identity": COMPILED_SOURCE_IDENTITY,
        },
        "strategy": frozen_strategy,
        "playbook_version": "v1.2",
        "sources": &sources,
        "runtime": &runtime,
        "paper": {
            "execution_enabled": true,
            "venue": &paper_venue,
            "stress_previews": &paper_stress_previews,
        }
    });
    if directional_model_entry_policy != BtcDirectionalModelEntryPolicy::RequirePositiveDirectEdge {
        frozen_raw["paper"]
            .as_object_mut()
            .expect("BTC frozen paper config is an object")
            .insert(
                "directional_model_entry_policy".to_string(),
                serde_json::to_value(directional_model_entry_policy)
                    .map_err(|error| HttpError::internal(error.to_string()))?,
            );
    }
    if let Some(entry_admission) = entry_admission.as_ref() {
        frozen_raw
            .as_object_mut()
            .expect("BTC frozen process config is an object")
            .insert(
                "entry_admission".to_string(),
                serde_json::to_value(entry_admission)
                    .map_err(|error| HttpError::internal(error.to_string()))?,
            );
    }
    if !risk_strategies.is_empty() {
        frozen_raw
            .as_object_mut()
            .expect("BTC frozen process config is an object")
            .insert(
                "risk_strategies".to_string(),
                serde_json::to_value(&risk_strategies)
                    .map_err(|error| HttpError::internal(error.to_string()))?,
            );
    }
    if execution_mode == BtcExecutionMode::Live {
        frozen_raw["paper"]["execution_enabled"] = serde_json::Value::Bool(false);
        frozen_raw
            .as_object_mut()
            .expect("BTC frozen process config is an object")
            .insert(
                "live".to_string(),
                serde_json::json!({
                    "execution_enabled": true,
                    "account_ref": execution.account_ref,
                }),
            );
    }
    let frozen_process_config = TradingProcessConfig {
        execution: Some(ProcessExecutionConfig {
            mode: Some(execution_mode.as_str().to_string()),
            execute_signals: execution.execute_signals,
            live_capital: execution.live_capital,
            account_ref: execution.account_ref.clone(),
            taker_fee_rate: None,
            max_order_notional_usd: execution.max_order_notional_usd,
            max_open_notional_usd: execution.max_open_notional_usd,
            max_open_positions: execution.max_open_positions,
            max_daily_loss_usd: execution.max_daily_loss_usd,
            require_exit_book: execution.require_exit_book,
        }),
        raw: frozen_raw,
    };
    let config_hash = hash_btc_frozen_process_config(&frozen_process_config)?;
    Ok(PreparedBtcStartDefinition {
        run_id,
        run_key,
        preregistration_sha256,
        strategy,
        sources,
        entry_admission,
        risk_strategies,
        directional_model_entry_policy,
        runtime,
        paper_venue,
        paper_stress_previews,
        execution_mode,
        execution: execution.clone(),
        frozen_process_config,
        config_hash,
    })
}

pub(super) fn hash_btc_frozen_process_config(
    frozen_process_config: &TradingProcessConfig,
) -> Result<String, HttpError> {
    Ok(format!(
        "{:x}",
        Sha256::digest(
            serde_json::to_vec(frozen_process_config)
                .map_err(|error| HttpError::internal(error.to_string()))?
        )
    ))
}

pub(super) fn resume_process_contract_projection(
    mut config: serde_json::Value,
) -> serde_json::Value {
    if let Some(raw) = config
        .get_mut("raw")
        .and_then(serde_json::Value::as_object_mut)
    {
        if let Some(build) = raw
            .get_mut("build")
            .and_then(serde_json::Value::as_object_mut)
        {
            build.remove("compiled_source_identity");
        }
        if let Some(runtime) = raw
            .get_mut("runtime")
            .and_then(serde_json::Value::as_object_mut)
        {
            for retired_system_field in [
                "gamma_base_url",
                "clob_rest_base_url",
                "clob_ws_url",
                "rtds_ws_url",
                "binance_ws_url",
                "binance_rest_base_url",
                "discovery_interval",
                "reconnect_initial_delay",
                "reconnect_max_delay",
                "checkpoint_interval",
                "boundary_tick_max_delay",
                "official_resolution_audit_grace",
                "official_resolution_watch_retention",
                "writer_capacity",
                "clob_heartbeat_interval",
                "rtds_heartbeat_interval",
                "binance_heartbeat_interval",
                "binance_spot_l2_enabled",
                "binance_spot_l2_ws_url",
            ] {
                runtime.remove(retired_system_field);
            }
        }
        raw.remove("playbook_version");
        raw.remove("sources");
        if raw
            .get("process_schema_version")
            .and_then(serde_json::Value::as_str)
            == Some(LEGACY_BTC_PROCESS_SCHEMA_VERSION)
        {
            raw.insert(
                "process_schema_version".into(),
                serde_json::Value::String(BTC_PROCESS_SCHEMA_VERSION.into()),
            );
            raw.remove("ml_shadow");
        }
    }
    config
}

pub(super) fn selector_only_config_change(
    current: &TradingProcessConfig,
    candidate: &TradingProcessConfig,
) -> Result<bool, HttpError> {
    fn remove_selectors(value: &mut serde_json::Value) {
        if let Some(control) = value
            .get_mut("raw")
            .and_then(|raw| raw.get_mut("btc_realtime_paper"))
            .and_then(serde_json::Value::as_object_mut)
        {
            control.remove("playbook_version");
            control.remove("sources");
        }
    }
    let mut current =
        serde_json::to_value(current).map_err(|error| HttpError::internal(error.to_string()))?;
    let mut candidate =
        serde_json::to_value(candidate).map_err(|error| HttpError::internal(error.to_string()))?;
    remove_selectors(&mut current);
    remove_selectors(&mut candidate);
    Ok(current == candidate)
}
