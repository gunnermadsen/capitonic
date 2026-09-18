use std::collections::BTreeSet;

use chrono::{DateTime, Duration, Utc};
use serde_json::{json, Value};

use crate::domain::{BackfillRequest, BackfillWorkerStrategy, StrategyCapability};

use super::{
    support::{
        normalize_reference_value, parse_gamma_btc_interval_event, reference_fact_matches,
        EXECUTION_SNAPSHOTS_BACKFILL_KEY, MARKET_CONTRACTS_BACKFILL_KEY,
        ORDERBOOK_EVENTS_BACKFILL_KEY, RESOLUTIONS_BACKFILL_KEY,
    },
    PolymarketBtcExecutionSnapshotsBackfill, PolymarketBtcMarketContractsBackfill,
    PolymarketBtcOrderbookEventsBackfill, PolymarketBtcResolutionsBackfill,
};
use rust_decimal::Decimal;

fn request(key: &str, start: DateTime<Utc>, end: DateTime<Utc>) -> BackfillRequest {
    serde_json::from_value(json!({
        "strategy_key": key,
        "range": {"start": start, "end": end},
        "parameters": {},
        "execution": {},
    }))
    .unwrap()
}

#[test]
fn four_strategies_have_unique_backfill_only_contracts() {
    let strategies: Vec<Box<dyn BackfillWorkerStrategy>> = vec![
        Box::new(PolymarketBtcMarketContractsBackfill::new().unwrap()),
        Box::new(PolymarketBtcResolutionsBackfill::new().unwrap()),
        Box::new(PolymarketBtcOrderbookEventsBackfill::new().unwrap()),
        Box::new(PolymarketBtcExecutionSnapshotsBackfill::new().unwrap()),
    ];
    let keys = strategies
        .iter()
        .map(|strategy| strategy.descriptor().strategy_key.to_string())
        .collect::<BTreeSet<_>>();
    assert_eq!(keys.len(), 4);
    assert_eq!(
        keys,
        BTreeSet::from([
            MARKET_CONTRACTS_BACKFILL_KEY.to_owned(),
            RESOLUTIONS_BACKFILL_KEY.to_owned(),
            ORDERBOOK_EVENTS_BACKFILL_KEY.to_owned(),
            EXECUTION_SNAPSHOTS_BACKFILL_KEY.to_owned(),
        ])
    );
    assert!(strategies.iter().all(|strategy| {
        strategy.descriptor().capabilities == vec![StrategyCapability::Backfill]
    }));
}

#[test]
fn one_hour_requests_produce_exactly_one_deterministic_shard() {
    let start: DateTime<Utc> = "2026-07-22T00:00:00Z".parse().unwrap();
    for strategy in [
        Box::new(PolymarketBtcMarketContractsBackfill::new().unwrap())
            as Box<dyn BackfillWorkerStrategy>,
        Box::new(PolymarketBtcResolutionsBackfill::new().unwrap()),
        Box::new(PolymarketBtcOrderbookEventsBackfill::new().unwrap()),
        Box::new(PolymarketBtcExecutionSnapshotsBackfill::new().unwrap()),
    ] {
        let key = strategy.descriptor().strategy_key.as_ref();
        let validated = strategy
            .validate_request(&request(key, start, start + Duration::hours(1)))
            .unwrap();
        let first = strategy.plan_shards(&validated).unwrap();
        let second = strategy.plan_shards(&validated).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].range_start, start);
        assert_eq!(first[0].range_end, start + Duration::hours(1));
    }
}

#[test]
fn pmxt_rejects_partial_hour_ranges() {
    let start: DateTime<Utc> = "2026-07-22T00:05:00Z".parse().unwrap();
    let strategy = PolymarketBtcOrderbookEventsBackfill::new().unwrap();
    let error = strategy
        .validate_request(&request(
            ORDERBOOK_EVENTS_BACKFILL_KEY,
            start,
            start + Duration::hours(1),
        ))
        .unwrap_err();
    assert_eq!(error.code, "range_alignment_invalid");
}

#[test]
fn original_gamma_contract_fixture_parses_without_contract_drift() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/polymarket/gamma_btc_five_minute_event_v1.json"
    ))
    .unwrap();
    let start: DateTime<Utc> = "2026-07-13T00:30:00Z".parse().unwrap();
    let market = parse_gamma_btc_interval_event(&fixture, start).unwrap();
    assert_eq!(market.window_start, start);
    assert_eq!(market.window_end, start + Duration::minutes(5));
    assert_ne!(market.up_token_id, market.down_token_id);
}

#[test]
fn gamma_reference_values_match_the_durable_numeric_scale() {
    assert_eq!(
        normalize_reference_value("66134.39593420082".parse::<Decimal>().unwrap()),
        "66134.3959342008".parse::<Decimal>().unwrap()
    );
}

#[test]
fn gamma_reference_midpoints_match_existing_postgres_rounding() {
    for (source, durable) in [
        ("70636.27506812985", "70636.2750681299"),
        ("68953.73318371625", "68953.7331837163"),
    ] {
        assert_eq!(
            normalize_reference_value(source.parse::<Decimal>().unwrap()),
            durable.parse::<Decimal>().unwrap()
        );
    }
}

#[test]
fn gamma_reference_normalization_preserves_real_conflicts() {
    assert_ne!(
        normalize_reference_value("70636.27506812984".parse::<Decimal>().unwrap()),
        "70636.2750681299".parse::<Decimal>().unwrap()
    );
}

#[test]
fn gamma_reference_legacy_midpoint_matches_exact_stored_evidence() {
    let effective_at = "2026-08-17T02:30:00Z".parse().unwrap();
    let source = "63116.58653999865".parse::<Decimal>().unwrap();
    let evidence = json!({"eventMetadata": {"finalPrice": 63116.58653999865}});
    assert!(reference_fact_matches(
        "final_price",
        source,
        effective_at,
        "63116.5865399986".parse().unwrap(),
        effective_at,
        &evidence,
    ));
}

#[test]
fn gamma_reference_exact_evidence_rejects_real_source_conflicts() {
    let effective_at = "2026-08-17T02:30:00Z".parse().unwrap();
    let evidence = json!({"eventMetadata": {"finalPrice": 63116.58653999865}});
    assert!(!reference_fact_matches(
        "final_price",
        "63116.58653999866".parse().unwrap(),
        effective_at,
        "63116.5865399986".parse().unwrap(),
        effective_at,
        &evidence,
    ));
    assert!(!reference_fact_matches(
        "final_price",
        "63116.58653999865".parse().unwrap(),
        effective_at + Duration::minutes(5),
        "63116.5865399986".parse().unwrap(),
        effective_at,
        &evidence,
    ));
}

#[test]
fn gamma_opening_boundary_reads_price_to_beat_evidence() {
    let effective_at = "2026-08-17T02:25:00Z".parse().unwrap();
    let source = "63053.18232987998".parse::<Decimal>().unwrap();
    let evidence = json!({"eventMetadata": {"priceToBeat": 63053.18232987998}});
    assert!(reference_fact_matches(
        "opening_boundary",
        source,
        effective_at,
        "63053.1823298800".parse().unwrap(),
        effective_at,
        &evidence,
    ));
}

#[test]
fn gamma_reference_missing_evidence_accepts_only_historical_rounding_contracts() {
    let effective_at = "2026-08-17T02:30:00Z".parse().unwrap();
    let source = "63116.58653999865".parse::<Decimal>().unwrap();
    assert!(reference_fact_matches(
        "final_price",
        source,
        effective_at,
        "63116.5865399986".parse().unwrap(),
        effective_at,
        &json!({}),
    ));
    assert!(reference_fact_matches(
        "final_price",
        source,
        effective_at,
        "63116.5865399987".parse().unwrap(),
        effective_at,
        &json!({}),
    ));
    assert!(!reference_fact_matches(
        "final_price",
        source,
        effective_at,
        "63116.5865399988".parse().unwrap(),
        effective_at,
        &json!({}),
    ));
}
