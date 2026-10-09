//! Configuration admission tests require no database, account credentials, or network.
use polymarket_bot::{
    execution::polymarket::{
        configuration::{compile, is_selected, pin_definition, ExecutionMode, ExecutionTrigger},
        policies::OrderPolicies,
    },
    models::TradingProcessConfig,
};
use serde_json::{json, Value};

fn fixture(mode: &str) -> Value {
    serde_json::from_str(match mode {
        "paper" => include_str!("../../../docs/polymarket-execution-rail/examples/paper.json"),
        "live" => include_str!("../../../docs/polymarket-execution-rail/examples/live.json"),
        _ => unreachable!(),
    })
    .unwrap()
}
fn config(v: Value) -> TradingProcessConfig {
    serde_json::from_value(v).unwrap()
}
fn policy<'a>(v: &'a mut Value, name: &str) -> &'a mut Value {
    &mut v["execution"]["order_policies"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|p| p["name"] == name)
        .unwrap()["params"]
}
fn invalid(v: Value, reason: &str) {
    let error = compile(&config(v)).unwrap_err();
    assert!(
        format!("{error:#}").contains(reason),
        "unexpected rejection: {error:#}"
    );
}

#[test]
fn paper_and_live_compile_with_identical_shared_policies() {
    let paper = compile(&config(fixture("paper"))).unwrap();
    let live = compile(&config(fixture("live"))).unwrap();
    assert_eq!(paper.execution.mode, ExecutionMode::Paper);
    assert_eq!(live.execution.mode, ExecutionMode::Live);
    assert_eq!(paper.control.runtime.trigger, ExecutionTrigger::Api);
    let mut shared = paper.execution.policies.clone();
    shared.paper_execution = None;
    assert_eq!(shared, live.execution.policies);
    assert_ne!(paper.config_sha256, live.config_sha256);
}

#[test]
fn policy_and_source_order_do_not_change_compiled_identity() {
    let mut v = fixture("paper");
    let original = compile(&config(v.clone())).unwrap();
    v["execution"]["order_policies"]
        .as_array_mut()
        .unwrap()
        .reverse();
    v["raw"]["btc_realtime_paper"]["sources"]
        .as_array_mut()
        .unwrap()
        .reverse();
    policy(&mut v, "sizing_policy")["max_entry_price"] = json!("0.5500");
    let resolved = compile(&config(v)).unwrap();
    assert_eq!(original.config_sha256, resolved.config_sha256);
    assert_eq!(original.execution.policies, resolved.execution.policies);
}

#[test]
fn changing_policy_changes_identity_and_decimals_round_trip() {
    let mut v = fixture("paper");
    policy(&mut v, "sizing_policy")["min_entry_price"] = json!("0.0500000000000000000000000001");
    let a = config(v);
    let b: TradingProcessConfig = serde_json::from_slice(&serde_json::to_vec(&a).unwrap()).unwrap();
    assert_eq!(
        compile(&a).unwrap().config_sha256,
        compile(&b).unwrap().config_sha256
    );
    assert_ne!(
        compile(&a).unwrap().config_sha256,
        compile(&config(fixture("paper"))).unwrap().config_sha256
    );
}

#[test]
fn duplicate_unknown_missing_and_unsupported_policies_are_rejected() {
    let mut v = fixture("paper");
    let p = v["execution"]["order_policies"][0].clone();
    v["execution"]["order_policies"]
        .as_array_mut()
        .unwrap()
        .push(p);
    invalid(v, "duplicate order policy");
    let mut v = fixture("paper");
    v["execution"]["order_policies"]
        .as_array_mut()
        .unwrap()
        .push(json!({"name":"unknown","version":1,"params":{}}));
    invalid(v, "unknown order policies");
    let mut v = fixture("paper");
    v["execution"]["order_policies"][0]["version"] = json!(2);
    invalid(v, "unsupported policy version");
    let mut v = fixture("paper");
    v["execution"]["order_policies"]
        .as_array_mut()
        .unwrap()
        .remove(0);
    invalid(v, "missing order policy");
}

#[test]
fn parameters_are_strict_and_cannot_override_runtime_or_model_authority() {
    for (name, key, value) in [
        ("sizing_policy", "max_order_notional_usd", json!("5")),
        (
            "evidence_policy",
            "max_directional_feature_age_ms",
            json!(5000),
        ),
        (
            "paper_execution_policy",
            "strategy_interval_ms",
            json!(1000),
        ),
        ("edge_policy", "minimum_confidence", json!("0.75")),
    ] {
        let mut v = fixture("paper");
        policy(&mut v, name)[key] = value;
        invalid(v, "unknown field");
    }
    let mut v = fixture("paper");
    policy(&mut v, "sizing_policy")["min_entry_price"] = json!(0.05);
    invalid(v, "expected a string");
}

#[test]
fn legacy_fields_and_mixed_contract_markers_never_fall_back() {
    for name in [
        "max_order_notional_usd",
        "max_open_notional_usd",
        "max_daily_loss_usd",
        "taker_fee_rate",
    ] {
        let mut v = fixture("paper");
        v["execution"][name] = json!("1");
        invalid(v, "legacy flat order parameters");
    }
    for field in ["rail", "order_policies"] {
        let mut v = fixture("paper");
        v["execution"].as_object_mut().unwrap().remove(field);
        let cfg = config(v);
        assert!(is_selected(&cfg));
        assert!(compile(&cfg).is_err());
    }
    for field in ["target_size", "max_book_age_ms", "unified_model"] {
        let mut v = fixture("paper");
        v["raw"]["btc_realtime_paper"]["strategy"][field] = json!(5);
        invalid(v, "unknown field");
    }
    let mut v = fixture("paper");
    v["raw"]["btc_realtime_paper"]["schema_version"] = json!("btc_realtime_paper_process_v4");
    invalid(v, "unsupported process schema");
    let mut v = fixture("paper");
    v["execution"]["rail"]["name"] = json!("unknown");
    invalid(v, "unsupported execution rail");
}

#[test]
fn quantity_bounds_and_dynamic_availability_are_explicit() {
    for (key, value) in [
        ("lower_share_bound", json!(0)),
        ("default_share", json!(11)),
        ("max_entry_price", json!("1")),
        ("max_depth_participation", json!("1.1")),
    ] {
        let mut v = fixture("paper");
        policy(&mut v, "sizing_policy")[key] = value;
        assert!(compile(&config(v)).is_err());
    }
    let mut v = fixture("paper");
    policy(&mut v, "sizing_policy")["mode"] = json!("dynamic");
    invalid(v, "dynamic sizing decision contract is not implemented");
}

#[test]
fn order_types_preserve_member_specific_configuration() {
    let mut v = fixture("paper");
    let mut member =
        v["raw"]["btc_realtime_paper"]["strategy"]["decision_strategy"]["models"][0].clone();
    member["member_id"] = json!("second");
    v["raw"]["btc_realtime_paper"]["strategy"]["decision_strategy"]["models"]
        .as_array_mut()
        .unwrap()
        .push(member);
    invalid(v.clone(), "exactly match router members");
    policy(&mut v, "order_execution_policy")["member_order_types"]["second"] = json!("fok");
    let compiled = compile(&config(v.clone())).unwrap();
    assert_ne!(
        compiled
            .execution
            .policies
            .order_execution
            .member_order_types["conservative"],
        compiled
            .execution
            .policies
            .order_execution
            .member_order_types["second"]
    );
    v["raw"]["btc_realtime_paper"]["strategy"]["decision_strategy"]["models"][0]
        ["entry_order_type"] = json!("fok");
    invalid(v, "belongs only");
}

#[test]
fn authorization_and_adapter_configuration_cannot_be_confused() {
    let mut v = fixture("paper");
    v["execution"]["live_capital"] = json!(true);
    invalid(v, "paper execution authorization");
    let mut v = fixture("live");
    v["execution"]["account_ref"] = Value::Null;
    invalid(v, "account_ref");
    let mut v = fixture("live");
    v["raw"]["btc_realtime_paper"]["strategy"] =
        fixture("paper")["raw"]["btc_realtime_paper"]["strategy"].clone();
    invalid(v, "does not authorize live capital");
    let mut v = fixture("live");
    v["execution"]["order_policies"]
        .as_array_mut()
        .unwrap()
        .push(fixture("paper")["execution"]["order_policies"][9].clone());
    invalid(v, "required only for paper");
}

#[test]
fn missing_sources_and_incompatible_frozen_requirements_are_rejected() {
    let mut v = fixture("paper");
    v["raw"]["btc_realtime_paper"]["sources"]
        .as_array_mut()
        .unwrap()
        .pop();
    assert!(compile(&config(v)).is_err());
    let mut v = fixture("paper");
    v["raw"]["btc_realtime_paper"]["strategy"]["max_directional_feature_age_ms"] = json!(6000);
    assert!(compile(&config(v)).is_err());
}

#[test]
fn failed_pinning_does_not_mutate_the_candidate() {
    let mut v = fixture("paper");
    policy(&mut v, "sizing_policy")["default_share"] = json!(100);
    let mut c = config(v);
    let before = serde_json::to_value(&c).unwrap();
    assert!(pin_definition(&mut c).is_err());
    assert_eq!(before, serde_json::to_value(c).unwrap());
}

#[test]
fn optional_loss_controls_reuse_existing_admission_contracts() {
    let mut v = fixture("paper");
    v["execution"]["order_policies"].as_array_mut().unwrap().push(json!({"name":"loss_regime_policy","version":1,"params":{"schema_version":"loss_regime_confidence_floor_v1","activation_consecutive_candidate_losses":3,"min_conservative_probability":"0.90","release_consecutive_candidate_wins":2}}));
    policy(&mut v, "loss_limit_policy")["daily_realized_pnl_high_water_mark"] = json!({"schema_version":"daily_realized_pnl_high_water_mark_v1","activation_realized_pnl_usd":"10","max_drawdown_from_high_water_mark_usd":"5"});
    assert!(compile(&config(v.clone()))
        .unwrap()
        .execution
        .policies
        .loss_regime
        .is_some());
    policy(&mut v, "loss_regime_policy")["release_consecutive_candidate_wins"] = json!(0);
    invalid(v, "release_consecutive_candidate_wins");
}

#[test]
fn legacy_execution_serialization_and_effective_controls_are_unchanged() {
    let legacy = json!({"execution":{"mode":"live","execute_signals":true,"live_capital":true,"account_ref":"polymarket-primary","taker_fee_rate":null,"max_order_notional_usd":"3","max_open_notional_usd":"3","max_open_positions":1,"max_daily_loss_usd":"10","require_exit_book":true},"raw":{"btc_realtime_paper":{"schema_version":"btc_realtime_paper_process_v4"}}});
    let c = config(legacy.clone());
    assert!(!is_selected(&c));
    assert_eq!(serde_json::to_value(&c).unwrap(), legacy);
    let e = c.effective_execution();
    assert_eq!(e.mode, "live");
    assert_eq!(e.max_order_notional_usd.unwrap().to_string(), "3");
    assert_eq!(e.max_open_positions, Some(1));
    assert!(c.execution.unwrap().rail.is_none());
}

#[test]
fn paper_policy_never_changes_shared_policy_compilation() {
    let mut v = fixture("paper");
    let original = compile(&config(v.clone())).unwrap();
    policy(&mut v, "paper_execution_policy")["arrival_latency_ms"] = json!(600);
    let mut changed = compile(&config(v)).unwrap().execution.policies;
    changed.paper_execution = original.execution.policies.paper_execution.clone();
    assert_eq!(changed, original.execution.policies);
    let c = config(fixture("paper"));
    assert!(OrderPolicies::compile(
        c.execution
            .as_ref()
            .unwrap()
            .order_policies
            .as_ref()
            .unwrap(),
        false
    )
    .is_err());
}

#[test]
fn agent_configuration_reuses_existing_umr_contract_without_new_agent_machinery() {
    use polymarket_bot::btc::unified_model_runtime::agent;
    let mut v = fixture("paper");
    v["raw"]["btc_realtime_paper"]["sources"] = json!(agent::REQUIRED_PRODUCTS);
    v["raw"]["btc_realtime_paper"]["strategy"]["max_directional_feature_age_ms"] = Value::Null;
    v["raw"]["btc_realtime_paper"]["strategy"]["decision_strategy"]["models"][0]["selection"] = json!({"type":"openai_agent","profile_key":agent::PROFILE_KEY,"profile_sha256":agent::hash(&agent::profile()).unwrap()});
    let result = compile(&config(v)).unwrap();
    assert!(matches!(
        result.control.strategy.decision_strategy.models[0].selection,
        polymarket_bot::btc::unified_model_runtime::router::Selection::OpenaiAgent { .. }
    ));
}

#[test]
fn definition_pinning_uses_the_existing_catalog_and_keeps_order_types_in_policies() {
    let mut v = fixture("paper");
    let member = &mut v["raw"]["btc_realtime_paper"]["strategy"]["decision_strategy"]["models"][0];
    member["selection"]
        .as_object_mut()
        .unwrap()
        .remove("artifact_sha256");
    member["selection"]
        .as_object_mut()
        .unwrap()
        .remove("feature_schema_sha256");
    let mut c = config(v);
    pin_definition(&mut c).unwrap();
    assert_eq!(
        c.raw["btc_realtime_paper"]["strategy"]["decision_strategy"]["models"][0],
        fixture("paper")["raw"]["btc_realtime_paper"]["strategy"]["decision_strategy"]["models"][0]
    );
    compile(&c).unwrap();
}

#[test]
fn time_evidence_exposure_and_simulation_bounds_preserve_existing_constraints() {
    for (name, key, value) in [
        ("entry_timing_policy", "min_seconds_after_open", json!(300)),
        ("evidence_policy", "max_book_age_ms", json!(0)),
        ("evidence_policy", "max_source_skew_ms", json!(-1)),
        ("execution_cost_policy", "max_fee_rate", json!("-1")),
        ("edge_policy", "min_net_edge_usd", json!("-1")),
        ("exposure_policy", "max_open_positions", json!(7)),
        ("exposure_policy", "max_open_notional_usd", json!("31")),
        ("loss_limit_policy", "max_daily_loss_usd", json!("11")),
        ("paper_execution_policy", "arrival_latency_ms", json!(0)),
        (
            "paper_execution_policy",
            "starting_collateral_usd",
            json!("0"),
        ),
        (
            "paper_execution_policy",
            "visible_depth_haircut",
            json!("1.1"),
        ),
    ] {
        let mut v = fixture("paper");
        policy(&mut v, name)[key] = value;
        assert!(compile(&config(v)).is_err(), "accepted {name}.{key}");
    }
    let mut v = fixture("live");
    policy(&mut v, "evidence_policy")["max_reference_age_ms"] = json!(3000);
    policy(&mut v, "evidence_policy")["max_book_age_ms"] = json!(3000);
    invalid(v, "2000ms");
}

#[test]
#[ignore = "explicit host timing acceptance; not a hardware-sensitive CI gate"]
fn warmed_configuration_compilation_stays_within_admission_budget() {
    let c = config(fixture("paper"));
    compile(&c).unwrap();
    let mut elapsed = Vec::new();
    for _ in 0..100 {
        let now = std::time::Instant::now();
        compile(&c).unwrap();
        elapsed.push(now.elapsed());
    }
    elapsed.sort();
    eprintln!(
        "warmed configuration compile median={:?}, p95={:?}",
        elapsed[50], elapsed[95]
    );
    assert!(elapsed[50] < std::time::Duration::from_millis(25));
    assert!(elapsed[95] < std::time::Duration::from_millis(100));
}
