use super::{
    configuration::compile,
    lifecycle::{self, Resolution},
    oms::*,
};
use crate::{
    btc::{
        process_runner::book_features,
        strategy::{BtcFeatureLineage, BtcFeatureSnapshot},
        types::{BtcOutcome, FeedIntegrityStatus, OrderbookCheckpoint, OrderbookLevel},
    },
    models::{FillRecord, FillSource, OrderRecord, OrderState, OrderType, TradingProcessConfig},
};
use chrono::{Duration, Utc};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use uuid::Uuid;

fn definition(mode: &str) -> super::configuration::CompiledDefinition {
    let json = if mode == "paper" {
        include_str!("../../../../../docs/polymarket-execution-rail/examples/paper.json")
    } else {
        include_str!("../../../../../docs/polymarket-execution-rail/examples/live.json")
    };
    compile(&serde_json::from_str::<TradingProcessConfig>(json).unwrap()).unwrap()
}
fn book(token: &str) -> OrderbookCheckpoint {
    let now = Utc::now();
    OrderbookCheckpoint {
        checkpoint_id: Uuid::new_v4(),
        market_id: "market".into(),
        token_id: token.into(),
        source_timestamp: now,
        received_at: now,
        observed_at: now,
        connection_id: Uuid::new_v4(),
        ingest_sequence: 1,
        source_hash: None,
        tick_size: dec!(0.01),
        best_bid: Some(dec!(0.39)),
        best_ask: Some(dec!(0.40)),
        bids: vec![OrderbookLevel {
            price: dec!(0.39),
            size: dec!(100),
        }],
        asks: vec![OrderbookLevel {
            price: dec!(0.40),
            size: dec!(100),
        }],
        integrity_status: FeedIntegrityStatus::Ok,
    }
}
pub(super) fn fixture(mode: &str) -> (Oms, Candidate, [OrderbookCheckpoint; 2], Accounting) {
    let now = Utc::now();
    let process_id = Uuid::new_v4();
    let books = [book("up"), book("down")];
    let snapshot:BtcFeatureSnapshot=serde_json::from_value(serde_json::json!({
        "snapshot_id":Uuid::new_v4(),"process_id":process_id,"observed_at":now,"feature_schema_version":"synthetic_market_evidence",
        "market_id":"market","event_slug":"market","window_start":now-Duration::seconds(60),"window_end":now+Duration::seconds(240),
        "market_active":true,"market_closed":false,"accepting_orders":true,"tick_size":"0.01","minimum_order_size":"1","resolution_source":"chainlink",
        "chainlink_quality_ok":true,"binance_quality_ok":true,
        "up_book":book_features(BtcOutcome::Up,"up",Some(&books[0]),now,dec!(5)),
        "down_book":book_features(BtcOutcome::Down,"down",Some(&books[1]),now,dec!(5)),
        "fees_enabled":true,"fee_rate":"0.25","fee_rate_observed_at":now,"lineage":BtcFeatureLineage::default()
    })).unwrap();
    let candidate = Candidate::synthetic(
        Uuid::new_v4(),
        "conservative".into(),
        snapshot,
        BtcOutcome::Up,
        dec!(0.90),
    )
    .unwrap();
    (
        Oms::new(
            definition(mode),
            Ownership {
                process_id,
                run_id: Uuid::new_v4(),
            },
        )
        .unwrap(),
        candidate,
        books,
        Accounting {
            available: dec!(100),
            open_notional: Decimal::ZERO,
            open_markets: vec![],
            daily_pnl: Decimal::ZERO,
            high_water: None,
            loss_regime: None,
        },
    )
}

#[test]
fn current_run_identity_is_preserved() {
    let compiled = definition("paper");
    let config_hash = compiled.config_sha256.clone();
    let oms = Oms::new_for_run(
        compiled,
        Ownership {
            process_id: Uuid::new_v4(),
            run_id: Uuid::new_v4(),
        },
        config_hash.clone(),
    )
    .unwrap();
    assert_eq!(oms.run_config_sha256, config_hash);
}

fn admit(
    oms: &Oms,
    candidate: &Candidate,
    books: &[OrderbookCheckpoint; 2],
    account: &Accounting,
) -> anyhow::Result<ExecutionIntent> {
    oms.admit(
        candidate,
        [Some(&books[0]), Some(&books[1])],
        account,
        Utc::now(),
    )
}
#[test]
fn identical_shared_evidence_produces_identical_paper_live_orders() {
    let (paper, candidate, books, account) = fixture("paper");
    let live = Oms::new(definition("live"), paper.owner.clone()).unwrap();
    let a = admit(&paper, &candidate, &books, &account).unwrap();
    let b = admit(&live, &candidate, &books, &account).unwrap();
    assert_eq!(
        (a.request.price, a.request.size, a.request.order_type),
        (b.request.price, b.request.size, b.request.order_type)
    );
    assert_eq!(a.origin, Origin::Api);
    assert!(a
        .request
        .metadata
        .get("reference_execution_guard")
        .is_none());
}
#[test]
fn quantity_is_resolved_before_depth_walk_and_fee_pricing() {
    let (mut oms, candidate, mut books, account) = fixture("paper");
    books[0].asks = vec![
        OrderbookLevel {
            price: dec!(0.40),
            size: dec!(4),
        },
        OrderbookLevel {
            price: dec!(0.50),
            size: dec!(100),
        },
    ];
    oms.definition
        .execution
        .policies
        .exposure
        .max_open_notional_usd = Some(dec!(4));
    oms.definition.execution.policies.sizing.default_share = dec!(5);
    let a = admit(&oms, &candidate, &books, &account).unwrap();
    assert_eq!(a.request.price, dec!(0.50));
    assert_eq!(a.request.size, dec!(5));
    oms.definition.execution.policies.sizing.default_share = dec!(1);
    let b = admit(&oms, &candidate, &books, &account).unwrap();
    assert_eq!(b.request.price, dec!(0.40));
    assert_eq!(b.request.size, dec!(1));
}
#[test]
fn fok_and_fak_do_not_clip_process_quantity_to_pass_admission() {
    let (mut oms, candidate, mut books, account) = fixture("paper");
    books[0].asks[0].size = dec!(10);
    for kind in [OrderType::Fok, OrderType::Fak] {
        oms.definition
            .execution
            .policies
            .order_execution
            .member_order_types
            .insert("conservative".into(), kind);
        assert!(admit(&oms, &candidate, &books, &account)
            .unwrap_err()
            .to_string()
            .contains("depth"));
    }
}
#[test]
fn both_modes_enforce_opposite_book_and_automatically_accept_fresh_reconnect() {
    for mode in ["paper", "live"] {
        let (oms, candidate, mut books, account) = fixture(mode);
        books[1].integrity_status = FeedIntegrityStatus::PreSnapshot;
        assert!(admit(&oms, &candidate, &books, &account).is_err());
        books[1] = book("down");
        assert!(admit(&oms, &candidate, &books, &account).is_ok());
        books[0].received_at -= Duration::seconds(5);
        assert!(admit(&oms, &candidate, &books, &account).is_err());
        books[0] = book("up");
        assert!(admit(&oms, &candidate, &books, &account).is_ok());
    }
}
#[test]
fn shared_accounting_enforces_funds_exposure_positions_and_loss_for_both_modes() {
    for mode in ["paper", "live"] {
        let (oms, candidate, books, mut account) = fixture(mode);
        account.available = dec!(1);
        assert!(admit(&oms, &candidate, &books, &account).is_err());
        account.available = dec!(100);
        account.open_notional = dec!(2);
        assert!(admit(&oms, &candidate, &books, &account).is_err());
        account.open_notional = Decimal::ZERO;
        account.open_markets = vec!["other".into()];
        assert!(admit(&oms, &candidate, &books, &account).is_err());
        account.open_markets.clear();
        account.daily_pnl = dec!(-10);
        assert!(admit(&oms, &candidate, &books, &account).is_err());
        account.daily_pnl = Decimal::ZERO;
        assert!(admit(&oms, &candidate, &books, &account).is_ok());
    }
}
#[test]
fn stale_fee_future_book_and_identity_mismatch_reject_before_submission() {
    let (oms, mut candidate, mut books, account) = fixture("paper");
    candidate.snapshot.fee_rate_observed_at = Some(Utc::now() - Duration::hours(2));
    assert!(admit(&oms, &candidate, &books, &account).is_err());
    candidate.snapshot.fee_rate_observed_at = Some(Utc::now());
    books[0].source_timestamp = Utc::now()
        + Duration::milliseconds(oms.definition.execution.policies.evidence.max_book_age_ms + 1);
    assert!(admit(&oms, &candidate, &books, &account).is_err());
    books[0] = book("different");
    assert!(admit(&oms, &candidate, &books, &account).is_err());
}
#[test]
fn execution_book_clock_semantics_match_readiness_in_both_modes() {
    use crate::btc::types::{BookReadiness, BtcIntervalMarket, RealtimeState};
    for mode in ["paper", "live"] {
        let (oms, candidate, original, account) = fixture(mode);
        let now = Utc::now();
        let bound = oms.definition.execution.policies.evidence.max_book_age_ms;
        let market: BtcIntervalMarket = serde_json::from_value(serde_json::json!({
            "event_id":"event","event_slug":"market","series_slug":"btc","market_id":"market",
            "condition_id":"condition","window_start":candidate.snapshot.window_start,
            "window_end":candidate.snapshot.window_end,"up_token_id":"up","down_token_id":"down",
            "tick_size":"0.01","minimum_order_size":"1","resolution_source":"chainlink",
            "active":true,"closed":false,"accepting_orders":true,"fees_enabled":false,
            "fee_schedule":{},"raw_payload":{}
        }))
        .unwrap();
        // Exercise selected and opposite books, exact bounds and sub-ms causality.
        for side in 0..2 {
            for (source_offset, receipt_offset, accepted) in [
                (50_000, -1_000, true),
                (bound * 1000, 0, true),
                (bound * 1000 + 1, 0, false),
                (0, 1, false),
                (-bound * 1000 - 1000, 0, false),
                (50_000, -bound * 1000 - 1000, false),
            ] {
                let mut books = original.clone();
                books[side].source_timestamp = now + Duration::microseconds(source_offset);
                books[side].received_at = now + Duration::microseconds(receipt_offset);
                let state = RealtimeState {
                    current_market: Some(market.clone()),
                    books: books
                        .iter()
                        .map(|b| {
                            (
                                b.token_id.clone(),
                                BookReadiness {
                                    market_id: b.market_id.clone(),
                                    token_id: b.token_id.clone(),
                                    connection_id: b.connection_id,
                                    bootstrapped: true,
                                    integrity_status: b.integrity_status,
                                    source_timestamp: Some(b.source_timestamp),
                                    received_at: Some(b.received_at),
                                    best_bid: b.best_bid,
                                    best_ask: b.best_ask,
                                },
                            )
                        })
                        .collect(),
                    ..Default::default()
                };
                let readiness =
                    state.readiness(now, Duration::milliseconds(bound), Duration::seconds(2));
                let books_ready = !readiness.reasons.iter().any(|r| r.contains("book"));
                assert_eq!(
                    books_ready, accepted,
                    "{mode} side {side} {source_offset}/{receipt_offset}"
                );
                let result = oms.admit(
                    &candidate,
                    [Some(&books[0]), Some(&books[1])],
                    &account,
                    now,
                );
                assert_eq!(result.is_ok(), accepted, "{mode} side {side}: {result:?}");
                if accepted && side == 0 {
                    let mut snapshot = candidate.snapshot.clone();
                    snapshot.observed_at = now;
                    let feature =
                        book_features(BtcOutcome::Up, "up", Some(&books[0]), now, dec!(5));
                    assert_eq!(
                        crate::btc::strategy::validate_book(
                            &oms.strategy(),
                            &snapshot,
                            &feature,
                            crate::btc::strategy::BtcRejectReason::MissingUpBook
                        ),
                        Err(crate::btc::strategy::BtcRejectReason::FutureInputTimestamp)
                    );
                }
            }
        }
        // No durable latch: the same OMS accepts fresh evidence after rejection.
        assert!(oms
            .admit(
                &candidate,
                [Some(&original[0]), Some(&original[1])],
                &account,
                now
            )
            .is_ok());
    }
}

#[test]
fn changed_candidate_identity_cannot_reuse_the_same_execution_identity() {
    let (_, mut candidate, _, _) = fixture("paper");
    let original = candidate_identity(&candidate).unwrap();
    candidate.probability = dec!(0.8);
    assert_ne!(original, candidate_identity(&candidate).unwrap());
}
fn order_and_fill() -> (OrderRecord, FillRecord) {
    let (oms, candidate, books, account) = fixture("paper");
    let intent = admit(&oms, &candidate, &books, &account).unwrap();
    let now = Utc::now();
    let order = OrderRecord {
        order_id: "order".into(),
        request: intent.request,
        state: OrderState::Cancelled,
        created_at: now,
        updated_at: now,
    };
    let fill = FillRecord {
        fill_id: Uuid::new_v4(),
        process_id: order.request.process_id,
        order_id: order.order_id.clone(),
        token_id: order.request.token_id.clone(),
        price: dec!(0.39),
        size: dec!(2),
        fee: dec!(0.01),
        source: FillSource::Paper,
        filled_at: now,
    };
    (order, fill)
}
#[test]
fn partial_fill_cancel_and_reordered_duplicate_evidence_do_not_double_count() {
    let (order, fill) = order_and_fill();
    assert_eq!(
        lifecycle::validate_fills(&order, &[fill.clone(), fill.clone()]).unwrap(),
        dec!(2)
    );
    assert_eq!(
        lifecycle::resolution(&order, std::slice::from_ref(&fill), false).unwrap(),
        Resolution::Pending
    );
    assert_eq!(
        lifecycle::resolution(&order, &[fill], true).unwrap(),
        Resolution::Executed
    );
}
#[test]
fn lost_submission_response_and_acknowledgement_are_never_final_execution() {
    let (mut order, _) = order_and_fill();
    for state in [
        OrderState::Unknown,
        OrderState::Submitted,
        OrderState::Acknowledged,
        OrderState::CancelRequested,
    ] {
        order.state = state;
        assert_eq!(
            lifecycle::resolution(&order, &[], true).unwrap(),
            Resolution::Pending
        );
    }
}
#[test]
fn explicit_rejection_differs_from_ambiguity_and_zero_fill_cancel_requires_reconcile() {
    let (mut order, _) = order_and_fill();
    assert_eq!(
        lifecycle::resolution(&order, &[], false).unwrap(),
        Resolution::Pending
    );
    assert_eq!(
        lifecycle::resolution(&order, &[], true).unwrap(),
        Resolution::DefinitiveNonexecution
    );
    order.state = OrderState::Rejected;
    assert_eq!(
        lifecycle::resolution(&order, &[], false).unwrap(),
        Resolution::DefinitiveNonexecution
    );
}
#[test]
fn price_improvement_uses_cash_authorization_without_fabricating_a_share_cap() {
    let (order, mut fill) = order_and_fill();
    fill.price = dec!(0.10);
    fill.size = dec!(6);
    assert!(lifecycle::validate_fills(&order, std::slice::from_ref(&fill)).is_ok());
    fill.size = dec!(21);
    assert!(lifecycle::validate_fills(&order, &[fill]).is_err());
}

#[test]
fn conflicting_fills_cross_process_and_premature_finality_are_rejected() {
    let (mut order, fill) = order_and_fill();
    let mut conflict = fill.clone();
    conflict.size = dec!(1);
    assert!(lifecycle::validate_fills(&order, &[fill.clone(), conflict.clone()]).is_err());
    conflict.process_id = Some(Uuid::new_v4());
    assert!(lifecycle::validate_fills(&order, &[conflict]).is_err());
    order.state = OrderState::Filled;
    assert!(lifecycle::validate_fills(&order, std::slice::from_ref(&fill)).is_err());
    order.state = OrderState::Cancelled;
    order.request.order_type = OrderType::Fok;
    assert!(lifecycle::validate_fills(&order, &[fill]).is_err());
}

#[test]
fn compiled_policy_mutation_is_detected_before_adapter_construction() {
    let (oms, _, _, _) = fixture("paper");
    let mut definition = oms.definition.clone();
    definition.execution.policies.sizing.default_share = dec!(7);
    assert!(Oms::new(definition, oms.owner).is_err());
}
#[test]
fn cash_budget_reservation_covers_improved_price_fees_and_matches_durable_replay() {
    let (oms, candidate, books, account) = fixture("paper");
    let intent = admit(&oms, &candidate, &books, &account).unwrap();
    let exposure = crate::btc::ProposedEntryExposure::for_buy_cash_budget(
        intent.request.size,
        intent.request.price,
        dec!(0.25),
    )
    .unwrap();
    let replay = crate::store::Store::conservative_live_request_exposure(&intent.request).unwrap();
    assert_eq!(exposure.entry_debit_usd(), replay.total_exposure_usd);
    let budget = intent.request.size * intent.request.price;
    for price in [dec!(0.001), dec!(0.10), dec!(0.30), dec!(0.40)] {
        let size = budget / price;
        let fee = crate::fees::dynamic_crypto_taker_fee(size, dec!(0.25), price);
        assert!(budget + fee <= replay.total_exposure_usd);
    }
}
#[test]
fn far_away_depth_cannot_satisfy_participation_at_the_executable_limit() {
    let (oms, candidate, mut books, account) = fixture("paper");
    books[0].asks = vec![
        OrderbookLevel {
            price: dec!(0.4),
            size: dec!(5),
        },
        OrderbookLevel {
            price: dec!(0.99),
            size: dec!(1000),
        },
    ];
    assert!(admit(&oms, &candidate, &books, &account)
        .unwrap_err()
        .to_string()
        .contains("marketability"));
}

#[test]
#[ignore = "manual host timing; correctness does not depend on scheduler load"]
fn warmed_shared_admission_latency() {
    let (oms, candidate, books, account) = fixture("paper");
    let mut times = Vec::new();
    for _ in 0..100 {
        let start = std::time::Instant::now();
        admit(&oms, &candidate, &books, &account).unwrap();
        times.push(start.elapsed());
    }
    times.sort();
    eprintln!(
        "shared admission: median {:?}, p95 {:?}",
        times[50], times[95]
    );
    assert!(times[50] < std::time::Duration::from_millis(2));
    assert!(times[95] < std::time::Duration::from_millis(10));
}

#[test]
fn admission_report_records_actual_checks_and_stops_at_rejection() {
    let (oms, candidate, books, account) = fixture("paper");
    let (intent, report) = oms.admit_report(
        &candidate,
        [Some(&books[0]), Some(&books[1])],
        &account,
        Utc::now(),
    );
    let intent = intent.unwrap();
    assert_eq!(
        intent.request.metadata["polymarket_execution"]["policy_results"],
        report
    );
    assert!(report
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p["name"] == "model_qualification" && p["status"] == "not_applicable"));
    let mut invalid = account;
    invalid.available = Decimal::ZERO;
    let (result, report) = oms.admit_report(
        &candidate,
        [Some(&books[0]), Some(&books[1])],
        &invalid,
        Utc::now(),
    );
    assert!(result.is_err());
    let last = report.as_array().unwrap().last().unwrap();
    assert_eq!(last["name"], "collateral");
    assert_eq!(last["status"], "rejected");
    assert_eq!(last["reason"], "insufficient_collateral");
    assert!(!report
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p["name"] == "marketable_depth"));
}
#[test]
fn api_replay_uses_original_payload_and_owner_without_current_configuration() {
    use crate::execution::orders::{validate_replay, EvaluationRequest};
    let input = serde_json::json!({"version":1,"candidate":{"member_id":"m","market_id":"market","outcome":"up","synthetic_prediction":{"probability_up":"0.80"}}});
    let request: EvaluationRequest = serde_json::from_value(input.clone()).unwrap();
    let (mut order, _) = order_and_fill();
    let owner = order.request.process_id.unwrap();
    order.request.metadata["polymarket_execution"]["api_request_sha256"] =
        serde_json::json!(request.identity().unwrap());
    order.request.metadata["polymarket_execution"]["config_sha256"] =
        serde_json::json!("historical_configuration");
    validate_replay(&order, owner, &request).unwrap();
    assert!(validate_replay(&order, Uuid::new_v4(), &request).is_err());
    let mut equivalent: EvaluationRequest = serde_json::from_value(input).unwrap();
    equivalent.candidate.synthetic_prediction.probability_up = dec!(0.8000);
    validate_replay(&order, owner, &equivalent).unwrap();
    equivalent.candidate.synthetic_prediction.probability_up = dec!(0.7);
    assert!(validate_replay(&order, owner, &equivalent).is_err());
}
#[test]
fn shared_order_metrics_keep_process_mode_origin_and_recovery_independent() {
    use crate::btc::execution_freshness::{
        prometheus_metrics, ExecutionFreshnessMetrics, OrderOutcome, OrderStage,
    };
    let a = Uuid::new_v4();
    let b = Uuid::new_v4();
    let paper = ExecutionFreshnessMetrics::new(a, "paper", 2000);
    let live = ExecutionFreshnessMetrics::new(b, "live", 2000);
    paper.order_event(Origin::Api, OrderStage::Preview, OrderOutcome::Rejected);
    paper.order_recovery(false, Some(1));
    live.order_recovery(true, Some(0));
    let output = prometheus_metrics(&serde_json::json!({}));
    assert!(output.contains(&format!("process_id=\"{a}\",execution_mode=\"paper\",origin=\"api\",stage=\"preview\",outcome=\"rejected\"")));
    assert!(output.contains(&format!(
        "polymarket_order_recovery_healthy{{process_id=\"{a}\",execution_mode=\"paper\"}} 0"
    )));
    assert!(output.contains(&format!(
        "polymarket_order_recovery_healthy{{process_id=\"{b}\",execution_mode=\"live\"}} 1"
    )));
    assert!(!output.contains("order_id="));
}

#[test]
fn configured_daily_loss_threshold_is_enforced_identically_in_both_modes() {
    for mode in ["paper", "live"] {
        let (mut oms, candidate, books, mut account) = fixture(mode);
        for limit in [dec!(25), dec!(1000000), Decimal::MAX] {
            oms.definition
                .execution
                .policies
                .loss_limit
                .max_daily_loss_usd = Some(limit);
            account.daily_pnl = dec!(-10);
            assert!(admit(&oms, &candidate, &books, &account).is_ok());
            account.daily_pnl = -limit;
            assert!(admit(&oms, &candidate, &books, &account)
                .unwrap_err()
                .to_string()
                .contains("daily_loss_limit"));
        }
        oms.definition
            .execution
            .policies
            .loss_limit
            .max_daily_loss_usd = Some(dec!(25));
        account.daily_pnl = dec!(-24.99);
        assert!(admit(&oms, &candidate, &books, &account).is_ok());
        account.daily_pnl = dec!(-25.01);
        assert!(admit(&oms, &candidate, &books, &account).is_err());
        oms.definition
            .execution
            .policies
            .loss_limit
            .max_daily_loss_usd = None;
        assert!(admit(&oms, &candidate, &books, &account).is_ok());
    }
}
