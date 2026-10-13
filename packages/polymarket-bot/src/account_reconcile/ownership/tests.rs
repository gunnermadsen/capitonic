use super::*;
use rust_decimal_macros::dec;

fn request() -> OwnershipRequest {
    let now = Utc::now();
    OwnershipRequest {
        request_id: Uuid::new_v4(),
        account_address: format!("0x{}", "1".repeat(40)),
        account_ref: "polymarket-primary".into(),
        chain_id: 137,
        window_start: now - chrono::Duration::hours(1),
        window_end: now,
        venue_order_ids: vec!["venue-peer".into()],
        transaction_hashes: vec![format!("0x{}", "a".repeat(64))],
        token_ids: vec!["shared-token".into()],
    }
}
fn evidence(request: &OwnershipRequest, environment: &str) -> OwnershipEvidence {
    let process = Uuid::from_u128(42);
    OwnershipEvidence {
        request_id: request.request_id,
        environment: environment.into(),
        account_address: request.account_address.clone(),
        account_ref: request.account_ref.clone(),
        chain_id: request.chain_id,
        observed_at: Utc::now(),
        process_ids: vec![process],
        orders: HashMap::from([("venue-peer".into(), "peer-order".into())]),
        order_process_ids: HashMap::from([("peer-order".into(), process)]),
        fills: vec![AccountLiveFillEvidence {
            transaction_hash: request.transaction_hashes[0].clone(),
            order_id: "peer-order".into(),
            token_id: "shared-token".into(),
            side: "buy".into(),
            price: dec!(0.46),
            size: dec!(8.695653),
        }],
        positions: HashMap::new(),
        token_lineage: HashSet::from(["shared-token".into()]),
    }
}
fn trade(request: &OwnershipRequest) -> AccountTrade {
    let activity = serde_json::from_value(serde_json::json!({
        "type": "TRADE", "timestamp": request.window_end.timestamp(),
        "asset": "shared-token", "side": "BUY", "price": "0.46", "size": "8.695653",
        "transactionHash": request.transaction_hashes[0],
    }))
    .unwrap();
    crate::account_reconcile::account_trade_from_activity(
        &request.account_address,
        &activity,
        "poll",
    )
    .unwrap()
}
fn position(size: Decimal) -> LiveAccountPositionEvidence {
    LiveAccountPositionEvidence {
        size,
        credited_zero_payout_size: Decimal::ZERO,
        oldest_fill_at: Utc::now() - chrono::Duration::minutes(10),
        market_window_end: Utc::now() + chrono::Duration::minutes(5),
    }
}

#[test]
fn exact_foreign_evidence_is_symmetric_and_never_links_local_accounting() {
    let request = request();
    for (local, peer) in [("development", "production"), ("production", "development")] {
        let evidence = evidence(&request, peer);
        assert_ne!(local, peer);
        evidence
            .ensure_disjoint(&[Uuid::from_u128(1)], &HashMap::new())
            .unwrap();
        let trade = trade(&request);
        let before = serde_json::to_value(&trade).unwrap();
        let foreign = evidence
            .foreign_trades(&[trade.clone(), trade.clone()])
            .unwrap();
        assert_eq!(foreign, HashSet::from([trade.account_trade_id]));
        assert_eq!(serde_json::to_value(&trade).unwrap(), before);
        assert!(trade.linked_order_id.is_none());
        let mut by_order = trade;
        by_order.venue_order_id = Some("venue-peer".into());
        by_order.transaction_hash = None;
        assert_eq!(evidence.foreign_trades(&[by_order]).unwrap().len(), 1);
    }
}

#[test]
fn same_token_or_economics_does_not_establish_foreign_ownership() {
    let request = request();
    let evidence = evidence(&request, "production");
    let mut unknown = trade(&request);
    unknown.transaction_hash = Some(format!("0x{}", "b".repeat(64)));
    assert!(evidence.foreign_trades(&[unknown]).unwrap().is_empty());
    let mut conflicting = trade(&request);
    conflicting.linked_order_id = Some("local-order".into());
    assert!(evidence
        .foreign_trades(&[conflicting])
        .unwrap_err()
        .to_string()
        .contains("ownership_conflict"));
    assert!(evidence
        .ensure_disjoint(&evidence.process_ids, &HashMap::new())
        .is_err());
    assert!(evidence
        .ensure_disjoint(
            &[],
            &HashMap::from([("venue-peer".into(), "local-order".into())])
        )
        .is_err());
}

#[test]
fn peer_evidence_requires_matching_identity_freshness_and_process_ownership() {
    let request = request();
    let now = Utc::now();
    let valid = evidence(&request, "production");
    valid.validate(&request, "production", now, now).unwrap();
    for kind in 0..9 {
        let mut invalid = valid.clone();
        match kind {
            0 => invalid.environment = "development".into(),
            1 => invalid.account_address = format!("0x{}", "2".repeat(40)),
            2 => invalid.chain_id = 1,
            3 => invalid.request_id = Uuid::new_v4(),
            4 => invalid.observed_at = now - chrono::Duration::minutes(1),
            5 => invalid.order_process_ids.clear(),
            6 => invalid.process_ids.push(invalid.process_ids[0]),
            7 => invalid
                .orders
                .insert("unrequested".into(), "peer-order".into())
                .map(|_| ())
                .unwrap_or(()),
            _ => invalid
                .positions
                .insert("shared-token".into(), position(dec!(-1)))
                .map(|_| ())
                .unwrap_or(()),
        }
        assert!(
            invalid.validate(&request, "production", now, now).is_err(),
            "case {kind}"
        );
    }
}

#[test]
fn combined_custody_preserves_local_positions_and_detects_real_discrepancies() {
    let request = request();
    let mut peer = evidence(&request, "production");
    peer.positions
        .insert("shared-token".into(), position(dec!(2)));
    let local = HashMap::from([("shared-token".into(), position(dec!(3)))]);
    let mut combined = local.clone();
    peer.merge_positions(&mut combined).unwrap();
    assert_eq!(local["shared-token"].size, dec!(3));
    assert_eq!(combined["shared-token"].size, dec!(5));
    let snapshot = crate::store::AccountPositionSnapshot {
        account_address: request.account_address.clone(),
        token_id: "shared-token".into(),
        market_id: None,
        size: dec!(5),
        avg_price: None,
        snapshot_id: Uuid::new_v4(),
        current_price: None,
        current_value: None,
        cash_pnl: None,
        percent_pnl: None,
        snapshot_at: Utc::now(),
        source: "poll".into(),
        raw_payload: serde_json::json!({}),
    };
    let (proof, mismatches, _) = crate::account_reconcile::process_accounting_proof(
        &combined,
        std::slice::from_ref(&snapshot),
        0,
        &[],
        Utc::now(),
    )
    .unwrap();
    assert_eq!(proof.status, "proven");
    assert!(mismatches.is_empty());
    let mut missing = snapshot;
    missing.size = dec!(4);
    let (proof, mismatches, _) = crate::account_reconcile::process_accounting_proof(
        &combined,
        &[missing],
        0,
        &[],
        Utc::now(),
    )
    .unwrap();
    assert_eq!(proof.status, "unproven");
    assert_eq!(mismatches.len(), 1);
}

#[test]
fn settled_peer_lineage_remains_ambiguous_for_manual_accounting() {
    let request = request();
    let evidence = evidence(&request, "production");
    assert!(evidence.positions.is_empty());
    assert!(evidence.has_token_lineage("shared-token"));
    assert!(!evidence.has_token_lineage("unrelated-token"));
}

#[test]
fn configuration_is_optional_bounded_and_redacts_secrets() {
    assert!(OwnershipClient::new(&OwnershipConfig::default())
        .unwrap()
        .is_none());
    let mut config = OwnershipConfig {
        environment: Some("development".into()),
        peer_environment: Some("production".into()),
        peer_url: Some("https://example.invalid/admin/live/reconciliation-ownership".into()),
        peer_token: Some("secret-never-logged".into()),
    };
    assert!(config.validate().is_ok());
    assert!(!format!("{config:?}").contains("secret-never-logged"));
    config.peer_environment = config.environment.clone();
    assert!(config.validate().is_err());
    config.peer_environment = Some("production".into());
    config.peer_token = None;
    assert!(config.validate().is_err());
    let mut request = request();
    request.venue_order_ids = vec!["order".into(); 4001];
    assert!(request.validate().is_err());
}

#[tokio::test]
async fn peer_outage_recovers_on_next_read_without_retaining_absence() {
    use axum::{
        extract::State,
        http::{HeaderMap, StatusCode},
        routing::post,
        Json, Router,
    };
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    async fn handler(
        State(attempt): State<Arc<AtomicUsize>>,
        headers: HeaderMap,
        Json(request): Json<OwnershipRequest>,
    ) -> Result<Json<OwnershipEvidence>, StatusCode> {
        assert_eq!(headers["authorization"], "Bearer test-only-token");
        if attempt.fetch_add(1, Ordering::SeqCst) == 0 {
            return Err(StatusCode::SERVICE_UNAVAILABLE);
        }
        Ok(Json(evidence(&request, "production")))
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/evidence", listener.local_addr().unwrap());
    let app = Router::new()
        .route("/evidence", post(handler))
        .with_state(Arc::new(AtomicUsize::new(0)));
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let client = OwnershipClient::new(&OwnershipConfig {
        environment: Some("development".into()),
        peer_environment: Some("production".into()),
        peer_url: Some(url),
        peer_token: Some("test-only-token".into()),
    })
    .unwrap()
    .unwrap();
    let request = request();
    assert!(client
        .fetch(&request)
        .await
        .unwrap_err()
        .to_string()
        .contains("peer_unavailable"));
    let recovered = client.fetch(&request).await.unwrap();
    assert_eq!(
        recovered.foreign_trades(&[trade(&request)]).unwrap().len(),
        1
    );
    task.abort();
}

#[test]
fn two_settled_foreign_trades_clear_only_the_false_local_blocker() {
    let mut request = request();
    request
        .transaction_hashes
        .push(format!("0x{}", "b".repeat(64)));
    let mut peer = evidence(&request, "production");
    let mut second_fill = peer.fills[0].clone();
    second_fill.transaction_hash = request.transaction_hashes[1].clone();
    second_fill.order_id = "second-peer-order".into();
    second_fill.size = dec!(8.173914);
    peer.order_process_ids
        .insert(second_fill.order_id.clone(), peer.process_ids[0]);
    peer.fills.push(second_fill);
    let first = trade(&request);
    let mut second_request = request.clone();
    second_request.transaction_hashes.swap(0, 1);
    let second = trade(&second_request);
    let trades = vec![first, second];
    let foreign = peer.foreign_trades(&trades).unwrap();
    assert_eq!(foreign.len(), 2);
    let unmatched = trades
        .iter()
        .filter(|trade| {
            trade.linked_order_id.is_none() && !foreign.contains(&trade.account_trade_id)
        })
        .count() as u64;
    let (proof, _, _) = crate::account_reconcile::process_accounting_proof(
        &HashMap::new(),
        &[],
        unmatched,
        &[],
        Utc::now(),
    )
    .unwrap();
    assert_eq!(proof.status, "proven");
    let (proof, _, _) = crate::account_reconcile::process_accounting_proof(
        &HashMap::new(),
        &[],
        unmatched + 1,
        &[],
        Utc::now(),
    )
    .unwrap();
    assert_eq!(proof.reason, "unmatched_account_trades");
    assert!(trades.iter().all(|trade| trade.linked_order_id.is_none()));
}
