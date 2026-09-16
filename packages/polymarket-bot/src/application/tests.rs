use super::*;

#[cfg(test)]
mod lifecycle_tests {
    #[test]
    fn optional_model_source_preserves_required_subscription_in_either_order() {
        let required: super::SourceSelector =
            serde_json::from_value(serde_json::json!("polygon_chainlink_btcusd_oracle")).unwrap();
        let optional: super::SourceSelector = serde_json::from_value(serde_json::json!({
            "key": "polygon_chainlink_btcusd_oracle", "required": false,
            "maximum_age_ms": 600000, "require_sequence_integrity": false
        }))
        .unwrap();
        for selectors in [
            vec![required.clone(), optional.clone()],
            vec![optional.clone(), required.clone()],
        ] {
            assert_eq!(
                super::merge_source_selectors(selectors).unwrap(),
                vec![required.clone()]
            );
        }
        let mut conflicting = optional.clone();
        conflicting.contract_version = 2;
        assert!(super::merge_source_selectors([required.clone(), conflicting]).is_err());
        let mut conflicting = required.clone();
        conflicting.maximum_age_ms = Some(1000);
        assert!(super::merge_source_selectors([required, conflicting]).is_err());
    }
    use super::*;
    use polymarket_bot::btc::BTC_DIRECTIONAL_MODEL_FEATURE_SCHEMA_VERSION;

    #[tokio::test]
    async fn inactive_runtime_status_reports_configured_mode_without_unbound_live_status() {
        let pool = PgPoolOptions::new()
            .connect_lazy("postgres://postgres:postgres@localhost/polymarket")
            .unwrap();
        let manager = BtcProcessManager::new(
            Store::from_pool(pool.clone()),
            pool.clone(),
            BtcRepository::from_pool(pool),
            BtcProcessManagerConfig {
                live_venue: None,
                live_reconcile_interval: Duration::from_secs(1),
            },
        );

        let status = manager
            .runtime_status_for_process(uuid::Uuid::new_v4(), Some("live"))
            .await;

        assert_eq!(status["active"], false);
        assert_eq!(status["execution_mode"], "live");
        assert!(status.get("live_status").is_none());
    }

    #[test]
    fn btc_process_terminal_validation_accepts_all_supported_terminal_states() {
        for status in ["stopped", "failed", "completed"] {
            validate_btc_process_terminal_request(status, "api_transition").unwrap();
        }
        assert!(validate_btc_process_terminal_request("running", "api_transition").is_err());
        assert!(validate_btc_process_terminal_request("completed", " ").is_err());
    }

    fn eligible_btc_process() -> TradingProcess {
        let now = Utc::now();
        TradingProcess {
            process_id: uuid::Uuid::new_v4(),
            name: "BTC realtime paper".to_string(),
            process_type: "btc_5m".to_string(),
            process_scope: "realtime_paper".to_string(),
            process_key: Some("btc-5m-realtime-paper".to_string()),
            status: "created".to_string(),
            enabled: false,
            config: TradingProcessConfig {
                execution: Some(ProcessExecutionConfig {
                    mode: Some("paper".to_string()),
                    execute_signals: true,
                    live_capital: false,
                    account_ref: None,
                    taker_fee_rate: None,
                    ..ProcessExecutionConfig::default()
                }),
                ..TradingProcessConfig::default()
            },
            metadata: serde_json::json!({}),
            created_at: now,
            updated_at: now,
            started_at: None,
            stopped_at: None,
            last_error: None,
        }
    }

    #[test]
    fn durable_resume_uses_only_explicit_running_live_capital_authorization() {
        let mut process = eligible_btc_process();
        process.enabled = true;
        process.status = "running".to_string();
        let configured = process.config.execution.as_mut().unwrap();
        configured.mode = Some("live".to_string());
        configured.live_capital = true;
        configured.account_ref = Some("polymarket-primary".to_string());
        let execution = process.effective_execution();
        assert!(should_resume_configured_live_entries(&process, &execution));

        process.enabled = false;
        assert!(!should_resume_configured_live_entries(&process, &execution));
        process.enabled = true;
        process.status = "stopping".to_string();
        assert!(!should_resume_configured_live_entries(&process, &execution));
    }

    fn prepared_btc_definition_with_default_runtime() -> PreparedBtcStartDefinition {
        prepare_btc_start_definition(ResolvedBtcProcessDefinition {
            control: BtcRealtimePaperControlConfig {
                schema_version: BTC_PROCESS_SCHEMA_VERSION.to_string(),
                next_experiment_key: "btc-5m-heartbeat-resume-test".to_string(),
                preregistration_sha256: "a".repeat(64),
                ..BtcRealtimePaperControlConfig::default()
            },
            strategy: BtcStrategyConfig::default(),
            entry_admission: None,
            risk_strategies: Vec::new(),
            runtime: BtcRuntimeConfig {
                enabled: true,
                ..BtcRuntimeConfig::default()
            },
            paper_venue: PaperVenueConfig::default(),
            paper_stress_previews: Vec::new(),
        })
        .unwrap()
    }

    #[test]
    fn btc_run_identity_is_owned_by_process_control_config() {
        let control: BtcRealtimePaperControlConfig = serde_json::from_value(serde_json::json!({
            "schema_version": BTC_PROCESS_SCHEMA_VERSION,
            "next_experiment_key": "btc-5m-paper-20260713-c",
            "preregistration_sha256": "a".repeat(64),
        }))
        .unwrap();
        assert_eq!(control.schema_version, BTC_PROCESS_SCHEMA_VERSION);
        assert_eq!(control.next_experiment_key, "btc-5m-paper-20260713-c");
        assert_eq!(control.preregistration_sha256.len(), 64);
        assert_eq!(control.runtime.strategy_interval_ms, 1_000);
        assert_eq!(control.paper.arrival_latency_ms, 150);
        assert_eq!(control.paper.visible_depth_haircut, dec!(0.80));
        assert_eq!(
            control.paper.directional_model_entry_policy,
            BtcDirectionalModelEntryPolicy::RequirePositiveDirectEdge
        );
        assert!(control.entry_admission.is_none());
        assert!(control.risk_strategies.is_empty());
    }

    #[test]
    fn btc_process_control_accepts_one_explicit_risk_strategy() {
        let control: BtcRealtimePaperControlConfig = serde_json::from_value(serde_json::json!({
            "schema_version": BTC_PROCESS_SCHEMA_VERSION,
            "next_experiment_key": "btc-5m-risk-contract",
            "preregistration_sha256": "a".repeat(64),
            "risk_strategies": [{
                "version": "capitonic-risk-strategy-v1",
                "model_key": "risk-model",
                "artifact_sha256": "b".repeat(64)
            }]
        }))
        .unwrap();
        risk_runtime::validate_selections(&control.risk_strategies).unwrap();
        let mut duplicated = control.risk_strategies.clone();
        duplicated.push(duplicated[0].clone());
        assert!(risk_runtime::validate_selections(&duplicated).is_err());
    }

    #[test]
    fn btc_process_control_rejects_silently_ignored_fields() {
        let result = serde_json::from_value::<BtcRealtimePaperControlConfig>(serde_json::json!({
            "schema_version": BTC_PROCESS_SCHEMA_VERSION,
            "next_experiment_key": "btc-5m-paper-20260713-c",
            "preregistration_sha256": "a".repeat(64),
            "unknown_setting": true,
        }));
        assert!(result.is_err());

        let retired_ml =
            serde_json::from_value::<BtcRealtimePaperControlConfig>(serde_json::json!({
                    "schema_version": BTC_PROCESS_SCHEMA_VERSION,
                    "next_experiment_key": "btc-5m-paper-20260713-c",
                    "preregistration_sha256": "a".repeat(64),
                    "ml_shadow": {"enabled": true},
            }));
        assert!(retired_ml.is_err());

        for field in [
            "clob_heartbeat_interval",
            "rtds_heartbeat_interval",
            "binance_heartbeat_interval",
        ] {
            let mut process_control = serde_json::json!({
                "schema_version": BTC_PROCESS_SCHEMA_VERSION,
                "next_experiment_key": "btc-5m-paper-20260713-c",
                "preregistration_sha256": "a".repeat(64),
                "runtime": {},
            });
            process_control["runtime"][field] = serde_json::json!(5);
            assert!(
                serde_json::from_value::<BtcRealtimePaperControlConfig>(process_control).is_err()
            );
        }
    }

    #[test]
    fn retired_v1_ml_field_is_accepted_only_for_durable_resume() {
        let legacy = serde_json::json!({
            "schema_version": LEGACY_BTC_PROCESS_SCHEMA_VERSION,
            "next_experiment_key": "btc-5m-paper-20260713-c",
            "preregistration_sha256": "a".repeat(64),
            "ml_shadow": {"enabled": true},
        });
        assert!(
            parse_btc_process_control(legacy.clone(), BtcDefinitionUse::ExplicitStart).is_err()
        );
        let resumed = parse_btc_process_control(legacy, BtcDefinitionUse::DurableResume).unwrap();
        assert_eq!(resumed.schema_version, BTC_PROCESS_SCHEMA_VERSION);
    }

    #[test]
    fn selectable_v3_resolves_native_directional_model_as_the_only_strategy() {
        let control = BtcRealtimePaperControlConfig {
            schema_version: SELECTABLE_BTC_PROCESS_SCHEMA_VERSION.to_string(),
            strategy: serde_json::json!({
                "decision_strategy": {
                    "type": "btc_directional_model",
                    "model_key": polymarket_bot::btc::BTC_DIRECTIONAL_MODEL_V1_KEY,
                    "artifact_sha256":
                        polymarket_bot::btc::BTC_DIRECTIONAL_MODEL_V1_ARTIFACT_SHA256,
                    "feature_schema_sha256":
                        polymarket_bot::btc::BTC_DIRECTIONAL_MODEL_V1_FEATURE_SCHEMA_SHA256
                },
                "min_seconds_after_open": 60,
                "min_seconds_before_close": 60,
                "max_directional_feature_age_ms": 5000
            }),
            ..BtcRealtimePaperControlConfig::default()
        };

        let strategy = resolve_btc_strategy(&control).unwrap();

        assert_eq!(
            strategy.strategy_version,
            BTC_DIRECTIONAL_MODEL_STRATEGY_VERSION
        );
        assert_eq!(
            strategy.feature_schema_version,
            BTC_DIRECTIONAL_MODEL_FEATURE_SCHEMA_VERSION
        );
        assert!(matches!(
            strategy.decision_strategy,
            Some(BtcDecisionStrategyConfig::BtcDirectionalModel { .. })
        ));
        assert_eq!(strategy.max_directional_feature_age_ms, Some(5_000));
    }

    #[test]
    fn selectable_v3_resolves_asymmetric_value_model_without_directional_entry_policy() {
        let mut control = BtcRealtimePaperControlConfig {
            schema_version: SELECTABLE_BTC_PROCESS_SCHEMA_VERSION.to_string(),
            strategy: serde_json::json!({
                "decision_strategy": {
                    "type": "btc_asymmetric_value_model",
                    "model_key": "btc-5m-asymmetric-core-paper-20260805-v1",
                    "artifact_sha256":
                        "4379aee1ab04b382425b76f2c8f32e80c86299a149cd9994c2515b8138de9813",
                    "feature_schema_sha256":
                        "633033efb069dfb54a5f7834ab355ea380bd01c5774b800fddb528322e1e73dd"
                },
                "required_model_feeds": [
                    {"feed": "binance_btcusdt_one_second_v1", "maximum_age_ms": 1000},
                    {"feed": "polymarket_btc5m_clob_execution_v1", "maximum_age_ms": 2000}
                ],
                "min_seconds_after_open": 1,
                "min_seconds_before_close": 244,
                "max_directional_feature_age_ms": 1000,
                "min_entry_price": "0.20",
                "max_entry_price": "0.30",
                "spread_reserve_fraction": "0",
                "slippage_reserve_bps": "0",
                "latency_reserve_per_share": "0.01",
                "min_net_edge_per_share": "0.03",
                "min_net_edge_usd": "0.15"
            }),
            ..BtcRealtimePaperControlConfig::default()
        };

        let strategy = resolve_btc_strategy(&control).unwrap();

        assert_eq!(
            strategy.strategy_version,
            BTC_ASYMMETRIC_VALUE_MODEL_STRATEGY_VERSION
        );
        assert!(matches!(
            strategy.decision_strategy,
            Some(BtcDecisionStrategyConfig::BtcAsymmetricValueModel { .. })
        ));
        assert_eq!(strategy.required_model_feeds.len(), 2);

        control.strategy["decision_strategy"] = serde_json::json!({
            "type": "btc_asymmetric_value_model",
            "model_key": "btc-5m-asymmetric-core-oracle-live-pilot-20260814",
            "artifact_sha256":
                "c87dd4ca07903000f5ccff2c3691b9541aee38f389cceee7c9432383ec0e0a0b",
            "feature_schema_sha256":
                "fe2a5aaee3df1ef899d2553712555091aa29b7481b3fed7805ba140dc8aa5014"
        });
        control.strategy["required_model_feeds"] = serde_json::json!([
            {"feed": "binance_btcusdt_one_second_v1", "maximum_age_ms": 1000},
            {"feed": "polymarket_btc5m_clob_execution_v1", "maximum_age_ms": 2000},
            {"feed": "chainlink_btcusd_oracle_v1", "maximum_age_ms": 300000}
        ]);
        let live_strategy = resolve_btc_strategy(&control).unwrap();
        validate_btc_live_model_authorization(&live_strategy).unwrap();
    }

    #[test]
    fn live_model_authorization_accepts_only_the_promoted_asymmetric_artifact() {
        let strategy = |model_key: &str, artifact_sha256: &str| BtcStrategyConfig {
            decision_strategy: Some(BtcDecisionStrategyConfig::BtcAsymmetricValueModel {
                model_key: model_key.to_string(),
                artifact_sha256: artifact_sha256.to_string(),
                feature_schema_sha256:
                    "fe2a5aaee3df1ef899d2553712555091aa29b7481b3fed7805ba140dc8aa5014".to_string(),
            }),
            ..BtcStrategyConfig::default()
        };

        validate_btc_live_model_authorization(&strategy(
            "btc-5m-asymmetric-core-oracle-live-pilot-20260814",
            "c87dd4ca07903000f5ccff2c3691b9541aee38f389cceee7c9432383ec0e0a0b",
        ))
        .unwrap();

        assert!(validate_btc_live_model_authorization(&strategy(
            "btc-5m-asymmetric-core-oracle-paper-20260805-v1",
            "2c91e894356f6fee7fe9514e24c39da6e11602ffcb7f961f64848bde72418db9",
        ))
        .is_err());
    }

    #[test]
    fn directional_model_validation_entry_policy_is_paper_only_and_frozen() {
        let mut control = BtcRealtimePaperControlConfig {
            schema_version: SELECTABLE_BTC_PROCESS_SCHEMA_VERSION.to_string(),
            next_experiment_key: "btc-5m-directional-model-validation-test".to_string(),
            preregistration_sha256: "e".repeat(64),
            strategy: serde_json::json!({
                "decision_strategy": {
                    "type": "btc_directional_model",
                    "model_key": polymarket_bot::btc::BTC_DIRECTIONAL_MODEL_V1_KEY,
                    "artifact_sha256":
                        polymarket_bot::btc::BTC_DIRECTIONAL_MODEL_V1_ARTIFACT_SHA256,
                    "feature_schema_sha256":
                        polymarket_bot::btc::BTC_DIRECTIONAL_MODEL_V1_FEATURE_SCHEMA_SHA256
                },
                "min_seconds_after_open": 60,
                "min_seconds_before_close": 60
            }),
            ..BtcRealtimePaperControlConfig::default()
        };
        control.paper.directional_model_entry_policy =
            BtcDirectionalModelEntryPolicy::ExecuteDirectionalPrediction;
        let strategy = resolve_btc_strategy(&control).unwrap();
        validate_directional_model_entry_policy(
            &strategy,
            BtcDirectionalModelEntryPolicy::ExecuteDirectionalPrediction,
        )
        .unwrap();
        assert!(validate_directional_model_entry_policy(
            &BtcStrategyConfig::default(),
            BtcDirectionalModelEntryPolicy::ExecuteDirectionalPrediction,
        )
        .is_err());

        let resolved = |control: BtcRealtimePaperControlConfig| ResolvedBtcProcessDefinition {
            control,
            strategy: strategy.clone(),
            entry_admission: None,
            risk_strategies: Vec::new(),
            runtime: BtcRuntimeConfig {
                enabled: true,
                ..BtcRuntimeConfig::default()
            },
            paper_venue: PaperVenueConfig::default(),
            paper_stress_previews: Vec::new(),
        };
        let prepared = prepare_btc_start_definition(resolved(control.clone())).unwrap();
        assert_eq!(
            prepared.directional_model_entry_policy,
            BtcDirectionalModelEntryPolicy::ExecuteDirectionalPrediction
        );
        assert_eq!(
            prepared.frozen_process_config.raw["paper"]["directional_model_entry_policy"],
            serde_json::json!("execute_directional_prediction")
        );

        control.paper.directional_model_entry_policy =
            BtcDirectionalModelEntryPolicy::RequirePositiveDirectEdge;
        let default_prepared = prepare_btc_start_definition(resolved(control)).unwrap();
        assert!(default_prepared.frozen_process_config.raw["paper"]
            .get("directional_model_entry_policy")
            .is_none());
        assert_ne!(prepared.config_hash, default_prepared.config_hash);

        let mut live_capital = eligible_btc_process();
        live_capital.config.execution.as_mut().unwrap().live_capital = true;
        assert!(validate_btc_process_capability(&live_capital).is_err());

        let mut live_mode = eligible_btc_process();
        live_mode.config.execution.as_mut().unwrap().mode = Some("live".to_string());
        assert!(validate_btc_process_capability(&live_mode).is_err());
    }

    #[test]
    fn optional_execution_controls_are_mode_independent_and_validated_when_present() {
        let mut process = eligible_btc_process();
        let execution = process.config.execution.as_mut().unwrap();
        execution.max_order_notional_usd = Some(dec!(5));
        execution.max_open_notional_usd = Some(dec!(20));
        execution.max_open_positions = Some(6);
        execution.max_daily_loss_usd = Some(dec!(10));
        execution.require_exit_book = Some(true);
        validate_btc_process_capability(&process).unwrap();

        let execution = process.config.execution.as_mut().unwrap();
        execution.mode = Some("live".to_string());
        execution.execute_signals = true;
        execution.live_capital = true;
        execution.account_ref = Some("polymarket-primary".to_string());
        validate_btc_process_capability(&process).unwrap();

        process
            .config
            .execution
            .as_mut()
            .unwrap()
            .max_order_notional_usd = Some(dec!(5.01));
        assert!(validate_btc_process_capability(&process).is_err());
    }

    #[test]
    fn entry_timing_accepts_only_the_fixed_120_zero_width_model_window() {
        let mut fixed_120 = BtcStrategyConfig {
            min_seconds_after_open: 120,
            min_seconds_before_close: 180,
            decision_strategy: Some(BtcDecisionStrategyConfig::BtcDirectionalModel {
                model_key: "btc-fixed-120".to_string(),
                artifact_sha256: "a".repeat(64),
                feature_schema_sha256: "b".repeat(64),
            }),
            ..BtcStrategyConfig::default()
        };
        validate_btc_entry_timing(&fixed_120).unwrap();

        fixed_120.min_seconds_after_open = 125;
        fixed_120.min_seconds_before_close = 175;
        assert!(validate_btc_entry_timing(&fixed_120).is_err());

        fixed_120.min_seconds_after_open = 121;
        fixed_120.min_seconds_before_close = 180;
        assert!(validate_btc_entry_timing(&fixed_120).is_err());

        fixed_120.min_seconds_after_open = 120;
        fixed_120.min_seconds_before_close = 180;
        fixed_120.decision_strategy = None;
        assert!(validate_btc_entry_timing(&fixed_120).is_err());
    }

    #[test]
    fn btc_start_eligibility_rejects_generic_active_and_invalid_definitions() {
        let mut generic = eligible_btc_process();
        generic.process_type = "copy_trade".to_string();
        assert!(validate_btc_start_eligibility(&generic).is_err());

        let mut active = eligible_btc_process();
        active.status = "running".to_string();
        active.enabled = true;
        assert!(validate_btc_start_eligibility(&active).is_err());

        let mut invalid = eligible_btc_process();
        invalid.config.execution.as_mut().unwrap().mode = Some("live".to_string());
        assert!(validate_btc_start_eligibility(&invalid).is_err());

        assert!(validate_btc_start_eligibility(&eligible_btc_process()).is_ok());
    }

    #[test]
    fn live_credential_only_definition_is_valid_but_cannot_start() {
        let mut process = eligible_btc_process();
        process.config.execution = Some(ProcessExecutionConfig {
            mode: Some("live".to_string()),
            execute_signals: false,
            live_capital: false,
            account_ref: Some("polymarket-primary".to_string()),
            taker_fee_rate: None,
            ..ProcessExecutionConfig::default()
        });

        validate_btc_process_capability(&process).unwrap();
        assert!(validate_btc_start_eligibility(&process).is_err());

        let execution = process.config.execution.as_mut().unwrap();
        execution.execute_signals = true;
        execution.live_capital = true;
        validate_btc_start_eligibility(&process).unwrap();
    }

    #[test]
    fn live_start_preparation_has_a_distinct_venue_identity_and_frozen_seam() {
        let run_key = "btc-5m-live-cutover-preview";
        let resolved = ResolvedBtcProcessDefinition {
            control: BtcRealtimePaperControlConfig {
                schema_version: BTC_PROCESS_SCHEMA_VERSION.to_string(),
                next_experiment_key: run_key.to_string(),
                preregistration_sha256: "c".repeat(64),
                ..BtcRealtimePaperControlConfig::default()
            },
            strategy: BtcStrategyConfig::default(),
            entry_admission: None,
            risk_strategies: Vec::new(),
            runtime: BtcRuntimeConfig {
                enabled: true,
                ..BtcRuntimeConfig::default()
            },
            paper_venue: PaperVenueConfig::default(),
            paper_stress_previews: Vec::new(),
        };
        let execution = EffectiveProcessExecutionConfig {
            mode: "live".to_string(),
            execute_signals: true,
            live_capital: true,
            account_ref: Some("polymarket-primary".to_string()),
            taker_fee_rate: dec!(0.03),
            ..EffectiveProcessExecutionConfig::default()
        };

        let prepared = prepare_btc_start_definition_for_execution(resolved, &execution).unwrap();
        assert_eq!(prepared.execution_mode, BtcExecutionMode::Live);
        assert_eq!(
            prepared.execution.account_ref.as_deref(),
            Some("polymarket-primary")
        );
        assert_eq!(
            prepared.run_id,
            uuid::Uuid::new_v5(
                &uuid::Uuid::NAMESPACE_URL,
                format!("polymarket-bot/btc-live/{run_key}").as_bytes(),
            )
        );
        assert_eq!(
            prepared
                .frozen_process_config
                .execution
                .as_ref()
                .unwrap()
                .mode
                .as_deref(),
            Some("live")
        );
        assert_eq!(
            prepared.frozen_process_config.raw["paper"]["execution_enabled"],
            false
        );
        assert_eq!(
            prepared.frozen_process_config.raw["live"],
            serde_json::json!({
                "execution_enabled": true,
                "account_ref": "polymarket-primary"
            })
        );
    }

    #[test]
    fn live_start_cannot_relax_the_two_second_execution_freshness_contract() {
        let execution = EffectiveProcessExecutionConfig {
            mode: "live".to_string(),
            execute_signals: true,
            live_capital: true,
            account_ref: Some("polymarket-primary".to_string()),
            taker_fee_rate: dec!(0.03),
            ..EffectiveProcessExecutionConfig::default()
        };
        let resolved = |strategy: BtcStrategyConfig| ResolvedBtcProcessDefinition {
            control: BtcRealtimePaperControlConfig {
                schema_version: BTC_PROCESS_SCHEMA_VERSION.to_string(),
                next_experiment_key: "btc-live-freshness-contract".to_string(),
                preregistration_sha256: "d".repeat(64),
                ..BtcRealtimePaperControlConfig::default()
            },
            strategy,
            entry_admission: None,
            risk_strategies: Vec::new(),
            runtime: BtcRuntimeConfig {
                enabled: true,
                ..BtcRuntimeConfig::default()
            },
            paper_venue: PaperVenueConfig::default(),
            paper_stress_previews: Vec::new(),
        };

        let mut relaxed_reference = BtcStrategyConfig::default();
        relaxed_reference.max_reference_age_ms = BTC_LIVE_EXECUTION_FRESHNESS_LIMIT_MS + 1;
        assert!(prepare_btc_start_definition_for_execution(
            resolved(relaxed_reference.clone()),
            &execution,
        )
        .is_err());
        assert!(
            prepare_btc_start_definition(resolved(relaxed_reference)).is_ok(),
            "the live-only ceiling must not break existing paper definitions"
        );

        let mut relaxed_book = BtcStrategyConfig::default();
        relaxed_book.max_book_age_ms = BTC_LIVE_EXECUTION_FRESHNESS_LIMIT_MS + 1;
        assert!(
            prepare_btc_start_definition_for_execution(resolved(relaxed_book), &execution,)
                .is_err()
        );

        assert!(prepare_btc_start_definition_for_execution(
            resolved(BtcStrategyConfig::default()),
            &execution,
        )
        .is_ok());
    }

    #[test]
    fn btc_start_preparation_is_deterministic_and_freezes_exact_execution_config() {
        let run_key = "btc-5m-paper-20260713-preview";
        let preregistration_sha256 = "b".repeat(64);
        let resolved = ResolvedBtcProcessDefinition {
            control: BtcRealtimePaperControlConfig {
                schema_version: BTC_PROCESS_SCHEMA_VERSION.to_string(),
                next_experiment_key: run_key.to_string(),
                preregistration_sha256: preregistration_sha256.clone(),
                ..BtcRealtimePaperControlConfig::default()
            },
            strategy: BtcStrategyConfig::default(),
            entry_admission: None,
            risk_strategies: Vec::new(),
            runtime: BtcRuntimeConfig {
                enabled: true,
                ..BtcRuntimeConfig::default()
            },
            paper_venue: PaperVenueConfig::default(),
            paper_stress_previews: Vec::new(),
        };

        let first = prepare_btc_start_definition(resolved.clone()).unwrap();
        let second = prepare_btc_start_definition(resolved).unwrap();
        let expected_run_id = uuid::Uuid::new_v5(
            &uuid::Uuid::NAMESPACE_URL,
            format!("polymarket-bot/btc-paper/{run_key}").as_bytes(),
        );
        let serialized_config = serde_json::to_vec(&first.frozen_process_config).unwrap();
        let expected_config_hash = format!("{:x}", Sha256::digest(serialized_config));

        assert_eq!(first.run_id, expected_run_id);
        assert_eq!(first.run_id, second.run_id);
        assert_eq!(first.config_hash, expected_config_hash);
        assert_eq!(first.config_hash, second.config_hash);
        let frozen_runtime = first.frozen_process_config.raw["runtime"]
            .as_object()
            .unwrap();
        assert!(!frozen_runtime.contains_key("clob_heartbeat_interval"));
        assert!(!frozen_runtime.contains_key("rtds_heartbeat_interval"));
        assert!(!frozen_runtime.contains_key("binance_heartbeat_interval"));
        assert_eq!(
            serde_json::to_value(&first.frozen_process_config).unwrap(),
            serde_json::to_value(&second.frozen_process_config).unwrap()
        );
        assert_eq!(
            first.frozen_process_config.raw["preregistration_sha256"],
            preregistration_sha256
        );
        assert_eq!(
            first.frozen_process_config.raw["process_schema_version"],
            BTC_PROCESS_SCHEMA_VERSION
        );
        assert_eq!(
            first.frozen_process_config.raw["pipeline_version"],
            BTC_PIPELINE_VERSION
        );
        assert!(first.frozen_process_config.raw["strategy"]
            .get("decision_strategy")
            .is_none());
        assert!(first.frozen_process_config.raw.get("ml_shadow").is_none());
        assert!(first
            .frozen_process_config
            .raw
            .get("entry_admission")
            .is_none());
        assert_eq!(
            first
                .frozen_process_config
                .execution
                .as_ref()
                .and_then(|execution| execution.mode.as_deref()),
            Some("paper")
        );
    }

    #[test]
    fn shared_market_data_accepts_playbook_runtime_differences() {
        let shared = BtcRuntimeConfig::default();
        let mut playbook = shared.clone();
        playbook.strategy_interval = Duration::from_millis(500);
        playbook.max_book_age = Duration::from_secs(3);
        playbook.max_reference_age = Duration::from_secs(4);

        assert!(shared_market_data_config_compatible(&shared, &playbook));

        playbook.strategy_interval += Duration::from_millis(1);
        assert!(shared_market_data_config_compatible(&shared, &playbook));
    }

    #[test]
    fn shared_feed_loss_requests_recovery_only_for_active_processes() {
        assert!(shared_runtime_recovery_required(17, false));
        assert!(!shared_runtime_recovery_required(17, true));
        assert!(!shared_runtime_recovery_required(0, false));
    }

    #[tokio::test]
    async fn shared_feed_recovery_invalidates_evidence_without_rebinding_consumers() {
        let state = Arc::new(tokio::sync::RwLock::new(
            polymarket_bot::btc::RealtimeState {
                primary_persistence_degraded: true,
                last_updated_at: Some(Utc::now()),
                ..polymarket_bot::btc::RealtimeState::default()
            },
        ));
        let original_connection_id = uuid::Uuid::new_v4();
        let books = Arc::new(tokio::sync::RwLock::new(BookRegistry::new(
            original_connection_id,
        )));
        let state_consumer = state.clone();
        let books_consumer = books.clone();

        invalidate_shared_market_data_evidence(&state, &books).await;

        assert!(Arc::ptr_eq(&state, &state_consumer));
        assert!(Arc::ptr_eq(&books, &books_consumer));
        assert_eq!(
            *state.read().await,
            polymarket_bot::btc::RealtimeState::default()
        );
        assert_ne!(books.read().await.connection_id(), original_connection_id);
    }

    #[test]
    fn btc_resume_process_contract_ignores_system_feed_transport_metadata() {
        let prepared = prepared_btc_definition_with_default_runtime();
        let current = serde_json::to_value(&prepared.frozen_process_config).unwrap();
        assert_eq!(
            resume_process_contract_projection(current.clone()),
            resume_process_contract_projection(current.clone())
        );

        for historical_value in [
            serde_json::Value::Null,
            serde_json::json!("10s"),
            serde_json::to_value(Duration::ZERO).unwrap(),
            serde_json::to_value(Duration::from_secs(5)).unwrap(),
            serde_json::to_value(Duration::from_secs(10)).unwrap(),
            serde_json::to_value(Duration::from_secs(6)).unwrap(),
            serde_json::to_value(Duration::from_secs(30)).unwrap(),
            serde_json::to_value(Duration::new(10, 1)).unwrap(),
        ] {
            let mut durable = current.clone();
            for field in [
                "gamma_base_url",
                "clob_rest_base_url",
                "clob_ws_url",
                "rtds_ws_url",
                "clob_heartbeat_interval",
                "rtds_heartbeat_interval",
                "binance_heartbeat_interval",
                "binance_ws_url",
                "binance_spot_l2_enabled",
                "binance_spot_l2_ws_url",
                "binance_rest_base_url",
                "discovery_interval",
                "reconnect_initial_delay",
                "reconnect_max_delay",
                "checkpoint_interval",
                "boundary_tick_max_delay",
                "official_resolution_audit_grace",
                "official_resolution_watch_retention",
                "writer_capacity",
            ] {
                durable["raw"]["runtime"][field] = historical_value.clone();
            }
            assert_eq!(
                resume_process_contract_projection(current.clone()),
                resume_process_contract_projection(durable)
            );
        }
    }

    #[test]
    fn btc_resume_system_heartbeat_metadata_does_not_relax_process_parameters() {
        let prepared = prepared_btc_definition_with_default_runtime();
        let current = serde_json::to_value(&prepared.frozen_process_config).unwrap();
        let mut durable = serde_json::to_value(&prepared.frozen_process_config).unwrap();
        durable["raw"]["runtime"]["clob_heartbeat_interval"] = serde_json::json!("ignored");
        durable["raw"]["runtime"]["rtds_heartbeat_interval"] = serde_json::json!(5);
        durable["raw"]["runtime"]["binance_heartbeat_interval"] = serde_json::json!(20);
        durable["raw"]["runtime"]["strategy_interval"] =
            serde_json::to_value(Duration::from_secs(1)).unwrap();

        assert_ne!(
            resume_process_contract_projection(current),
            resume_process_contract_projection(durable)
        );
    }

    #[test]
    fn btc_resume_preserves_frozen_parameters_across_service_rebuilds() {
        let durable = serde_json::json!({
            "raw": {
                "process_schema_version": LEGACY_BTC_PROCESS_SCHEMA_VERSION,
                "ml_shadow": {
                    "ml_a_enabled": true,
                    "ml_b_enabled": true,
                    "execution_authority": false
                },
                "build": {
                    "package_version": "0.1.0",
                    "compiled_source_identity": "tree-sha256:old"
                },
                "strategy": {"threshold": "0.03"}
            }
        });
        let rebuilt = serde_json::json!({
            "raw": {
                "process_schema_version": BTC_PROCESS_SCHEMA_VERSION,
                "build": {
                    "package_version": "0.1.0",
                    "compiled_source_identity": "tree-sha256:new"
                },
                "strategy": {"threshold": "0.03"}
            }
        });
        assert_eq!(
            resume_process_contract_projection(durable.clone()),
            resume_process_contract_projection(rebuilt)
        );

        let changed_parameters = serde_json::json!({
            "raw": {
                "process_schema_version": BTC_PROCESS_SCHEMA_VERSION,
                "build": {
                    "package_version": "0.1.0",
                    "compiled_source_identity": "tree-sha256:new"
                },
                "strategy": {"threshold": "0.04"}
            }
        });
        assert_ne!(
            resume_process_contract_projection(durable),
            resume_process_contract_projection(changed_parameters)
        );
    }

    #[test]
    fn readme_btc_process_contract_matches_v2_parser() {
        let readme = include_str!("../../../../README.md");
        let contract = readme
            .split("<!-- btc-5m-process-v2:start -->")
            .nth(1)
            .and_then(|tail| tail.split("<!-- btc-5m-process-v2:end -->").next())
            .expect("README must contain the BTC v2 process contract example")
            .trim()
            .strip_prefix("```json")
            .and_then(|json| json.trim().strip_suffix("```"))
            .expect("README BTC process contract must be a JSON code block");
        let request: control_http::UpsertTradingProcessByKeyRequest =
            serde_json::from_str(contract).unwrap();
        let control = request
            .config
            .raw
            .get("btc_realtime_paper")
            .cloned()
            .unwrap();
        parse_btc_process_control(control, BtcDefinitionUse::ExplicitStart).unwrap();

        let now = Utc::now();
        let process = TradingProcess {
            process_id: uuid::Uuid::nil(),
            name: request.name,
            process_type: request.process_type,
            process_scope: request.process_scope,
            process_key: Some("btc-5m-chainlink-paper".to_string()),
            status: request.status,
            enabled: request.enabled,
            config: request.config,
            metadata: request.metadata,
            created_at: now,
            updated_at: now,
            started_at: None,
            stopped_at: None,
            last_error: None,
        };
        validate_btc_start_eligibility(&process).unwrap();
    }
    #[test]
    fn umr_paper_templates_use_existing_process_resolution_and_start_contract() {
        let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("infra/processes");
        let mut checked = 0;
        for entry in std::fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if !path.to_string_lossy().ends_with("-umr-20260902.json") {
                continue;
            }
            let process: serde_json::Value =
                serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
            let control = parse_btc_process_control(
                process["config"]["raw"]["btc_realtime_paper"].clone(),
                BtcDefinitionUse::InactiveDefinition,
            )
            .unwrap();
            let strategy = resolve_btc_strategy(&control).unwrap();
            strategy.validate().unwrap();
            validate_btc_entry_timing(&strategy).unwrap();
            validate_directional_model_entry_policy(
                &strategy,
                control.paper.directional_model_entry_policy,
            )
            .unwrap();
            assert!(validate_btc_live_model_authorization(&strategy).is_err());
            let resolved = ResolvedBtcProcessDefinition {
                control,
                strategy,
                entry_admission: None,
                risk_strategies: Vec::new(),
                runtime: BtcRuntimeConfig {
                    enabled: true,
                    ..BtcRuntimeConfig::default()
                },
                paper_venue: PaperVenueConfig::default(),
                paper_stress_previews: Vec::new(),
            };
            let first = prepare_btc_start_definition(resolved.clone()).unwrap();
            let resumed = prepare_btc_start_definition(resolved).unwrap();
            assert_eq!(first.run_id, resumed.run_id);
            assert_eq!(first.config_hash, resumed.config_hash);
            checked += 1;
        }
        assert_eq!(checked, 5);
    }
}
