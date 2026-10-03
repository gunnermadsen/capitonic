use std::collections::BTreeSet;

use chrono::{DateTime, Duration, Utc};
use serde_json::{json, Value};

use crate::domain::{
    BackfillFailureKind, BackfillRequest, BackfillShard, BackfillWorkerStrategy, StrategyCapability,
};

use super::{
    support::{
        classify_database_message, normalize_reference_value, parse_gamma_btc_interval_event,
        reference_fact_matches, removed_drain_outcome, require_execution_source_records,
        RemovedDrainCoverage, EXECUTION_SNAPSHOTS_BACKFILL_KEY, MARKET_CONTRACTS_BACKFILL_KEY,
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
fn drained_execution_snapshot_coverage_completes_without_claiming_hourly_rows() {
    let start: DateTime<Utc> = "2026-08-01T00:00:00Z".parse().unwrap();
    let shard = BackfillShard {
        shard_key: "1785542400-1785546000".to_owned(),
        range_start: start,
        range_end: start + Duration::hours(1),
        parameters: json!({}),
    };
    let coverage = RemovedDrainCoverage {
        object_id: "dc820676-a0b5-4356-968a-b2d39c30e092".parse().unwrap(),
        source_start: start,
        source_end: start + Duration::days(1),
        row_count: 27_648,
        relative_path: "verified-chunks-v1/year=2026/month=08/day=01/object.parquet".to_owned(),
        sha256: "c6faa46a8796556a745da4d98eac62d1c4624d0878383569e101d285d717a584".to_owned(),
        byte_size: 4_469_203,
    };

    let outcome = removed_drain_outcome(&shard, &coverage);

    assert_eq!(outcome.records_verified, 0);
    assert_eq!(outcome.summary["already_archived"], true);
    assert_eq!(outcome.summary["source_download_skipped"], true);
    assert_eq!(outcome.summary["drain_object_row_count"], 27_648);
    assert_eq!(outcome.verified_coverage["durable_drain_coverage"], true);
}

#[test]
fn empty_pmxt_execution_archives_do_not_claim_verified_coverage() {
    let error = require_execution_source_records(0).unwrap_err();
    assert_eq!(error.kind, BackfillFailureKind::Integrity);
    assert_eq!(error.code, "pmxt_execution_source_empty");

    require_execution_source_records(1).unwrap();
}

#[test]
fn drained_history_trigger_is_a_permanent_backfill_failure() {
    let error = classify_database_message(
        "database error: historical source chunk has been drained".to_owned(),
    );
    assert_eq!(error.kind, BackfillFailureKind::InvalidRequest);
    assert_eq!(error.code, "drained_history_already_archived");

    let transient = classify_database_message("database connection closed".to_owned());
    assert_eq!(transient.kind, BackfillFailureKind::TransientDatabase);
    assert_eq!(transient.code, "database_error");
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
