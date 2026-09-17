use super::*;

#[cfg(test)]
mod tests {
    use rust_decimal_macros::dec;

    use crate::config::LiveExecutionConfig;

    use super::*;

    #[test]
    fn collateral_allowances_require_both_current_v2_exchanges_to_be_positive() {
        let exchange = CTF_EXCHANGE_V2_ADDRESS.parse::<Address>().unwrap();
        let neg_risk_exchange = NEG_RISK_CTF_EXCHANGE_V2_ADDRESS.parse::<Address>().unwrap();

        assert!(!collateral_allowances_positive(&HashMap::new()));
        assert!(!collateral_allowances_positive(&HashMap::from([(
            exchange,
            "0".to_string(),
        )])));
        assert!(!collateral_allowances_positive(&HashMap::from([(
            exchange,
            "invalid".to_string(),
        )])));
        assert!(!collateral_allowances_positive(&HashMap::from([
            (exchange, U256::MAX.to_string()),
            (neg_risk_exchange, "0".to_string()),
        ])));
        assert!(collateral_allowances_positive(&HashMap::from([
            (exchange, U256::MAX.to_string()),
            (neg_risk_exchange, "1".to_string()),
            (Address::ZERO, "0".to_string()),
        ])));
    }

    struct AlwaysReadyPrePostGuard;

    impl crate::execution::live_pre_post_guard_sealed::Sealed for AlwaysReadyPrePostGuard {}

    #[async_trait]
    impl LivePrePostGuard for AlwaysReadyPrePostGuard {
        async fn validate_pre_post(
            &self,
            _request: &OrderRequest,
        ) -> Result<Option<LiveExecutionGateReason>> {
            Ok(None)
        }
    }

    async fn submit_with_test_guard(
        venue: &LiveVenue,
        request: OrderRequest,
    ) -> Result<OrderRecord> {
        venue
            .submit_order_with_pre_post_guard(request, Some(Arc::new(AlwaysReadyPrePostGuard)))
            .await
    }

    fn live_config() -> LiveExecutionConfig {
        LiveExecutionConfig {
            user_ws_url: "wss://ws-subscriptions-clob.polymarket.com/ws/user".to_string(),
            clob_api_base_url: "https://clob-v2.polymarket.com".to_string(),
            user_ws_stale: std::time::Duration::from_secs(20),
            reconcile_interval: std::time::Duration::from_secs(30),
            stale_reconcile: std::time::Duration::from_secs(60),
            clob_api_key: Some("00000000-0000-0000-0000-000000000001".to_string()),
            clob_secret: Some("secret".to_string()),
            clob_passphrase: Some("pass".to_string()),
            private_key: Some(format!("0x{:064x}", 1)),
            funder_address: Some("0x7E5F4552091A69125d5DfCb7b8C2659029395Bdf".to_string()),
            signature_type: Some("0".to_string()),
        }
    }

    #[tokio::test]
    async fn process_bound_venues_share_the_initialized_live_authentication_cache() {
        let root = LiveVenue::new_for_test(live_config()).unwrap();
        let first = root
            .bind_process(Uuid::new_v4(), &live_execution())
            .unwrap();
        let second = root
            .bind_process(Uuid::new_v4(), &live_execution())
            .unwrap();

        assert!(Arc::ptr_eq(
            &root.authenticated_client_cache,
            &first.authenticated_client_cache
        ));
        assert!(Arc::ptr_eq(
            &first.authenticated_client_cache,
            &second.authenticated_client_cache
        ));
        assert!(Arc::ptr_eq(
            first.signer.as_ref().unwrap(),
            second.signer.as_ref().unwrap()
        ));
        assert!(root.authenticated_client_cache.get().is_none());

        first.authenticated_client().await.unwrap();

        assert!(root.authenticated_client_cache.get().is_some());
        assert!(second.authenticated_client_cache.get().is_some());
    }

    fn live_execution() -> EffectiveProcessExecutionConfig {
        EffectiveProcessExecutionConfig {
            mode: "live".to_string(),
            execute_signals: true,
            live_capital: true,
            account_ref: Some("polymarket-test".to_string()),
            taker_fee_rate: dec!(0.03),
            max_order_notional_usd: Some(dec!(2)),
            max_open_notional_usd: Some(dec!(30)),
            max_open_positions: Some(6),
            max_daily_loss_usd: Some(dec!(10)),
            require_exit_book: Some(true),
        }
    }

    fn assert_live_gate_rejection(order: &OrderRecord, reason: LiveExecutionGateReason) {
        assert_eq!(order.state, OrderState::Rejected);
        assert_eq!(
            order.request.metadata["reject_reason"],
            crate::execution::LIVE_EXECUTION_GATE_CLOSED_REASON
        );
        assert_eq!(
            order.request.metadata["live_execution_gate"]["gate_reason"],
            reason.as_str()
        );
        assert_eq!(
            order.request.metadata["live_execution_gate"]["post_attempted"],
            false
        );
    }

    fn rest_backfill_order(
        process_id: Uuid,
        order_id: &str,
        side: OrderSide,
        price: Decimal,
        size: Decimal,
        created_at: DateTime<Utc>,
    ) -> OrderRecord {
        OrderRecord {
            order_id: order_id.to_string(),
            request: OrderRequest {
                client_order_id: Uuid::new_v4(),
                process_id: Some(process_id),
                market_id: "market".to_string(),
                token_id: "1".to_string(),
                side,
                order_type: OrderType::Fok,
                price,
                size,
                metadata: json!({
                    "execution_intent": "entry",
                    "dynamic_fee_rate": "0.25"
                }),
            },
            state: OrderState::Acknowledged,
            created_at,
            updated_at: created_at,
        }
    }

    fn taker_trade(
        trade_id: &str,
        order_id: &str,
        price: Decimal,
        size: Decimal,
        fee_rate_bps: Decimal,
        match_time: DateTime<Utc>,
    ) -> TradeResponse {
        TradeResponse::builder()
            .id(trade_id)
            .taker_order_id(order_id)
            .market(polymarket_client_sdk_v2::types::B256::ZERO)
            .asset_id(U256::from(1))
            .side(SdkSide::Buy)
            .size(size)
            .fee_rate_bps(fee_rate_bps)
            .price(price)
            .status(TradeStatusType::Matched)
            .match_time(match_time)
            .last_update(match_time)
            .outcome("YES")
            .bucket_index(0)
            .owner(Uuid::max())
            .maker_address(Address::ZERO)
            .maker_orders(Vec::new())
            .transaction_hash(polymarket_client_sdk_v2::types::B256::ZERO)
            .trader_side(polymarket_client_sdk_v2::clob::types::TraderSide::Taker)
            .build()
    }

    #[test]
    fn rest_fill_backfill_recovers_complete_owned_trade_after_hour_long_gap() {
        let process_id = Uuid::new_v4();
        let checked_at = Utc::now();
        let created_at = checked_at - chrono::Duration::minutes(90);
        let order = rest_backfill_order(
            process_id,
            "venue-order-1",
            OrderSide::Buy,
            dec!(0.50),
            dec!(2),
            created_at,
        );
        let trade = taker_trade(
            "trade-1",
            "venue-order-1",
            dec!(0.49),
            dec!(2),
            dec!(25),
            created_at + chrono::Duration::seconds(1),
        );
        let owned_orders =
            HashMap::from([("venue-order-1".to_string(), "venue-order-1".to_string())]);

        let window_start =
            reconciliation_trade_window_start(std::slice::from_ref(&order), checked_at).unwrap();
        assert_eq!(window_start, created_at - LIVE_FILL_RECONCILIATION_SKEW);
        assert!(window_start < checked_at - chrono::Duration::hours(1));

        let fills = rest_fill_backfill_plan(
            process_id,
            std::slice::from_ref(&order),
            &owned_orders,
            std::slice::from_ref(&trade),
            checked_at,
        )
        .unwrap();
        assert_eq!(fills.len(), 1);
        assert_eq!(fills[0].process_id, Some(process_id));
        assert_eq!(fills[0].order_id, order.order_id);
        assert_eq!(fills[0].token_id, "1");
        assert_eq!(fills[0].price, dec!(0.49));
        assert_eq!(fills[0].size, order.request.size);
        assert_eq!(fills[0].fee, dec!(0.124950));
        assert_eq!(
            fills[0].fill_id,
            Uuid::new_v5(
                &Uuid::NAMESPACE_URL,
                b"polymarket:trade-order:trade-1:venue-order-1"
            )
        );
    }

    #[test]
    fn rest_fill_backfill_accepts_price_improved_buy_shares_within_authorized_notional() {
        let process_id = Uuid::new_v4();
        let checked_at = Utc::now();
        let order = rest_backfill_order(
            process_id,
            "venue-order-price-improved",
            OrderSide::Buy,
            dec!(0.82),
            dec!(5),
            checked_at - chrono::Duration::seconds(1),
        );
        let trade = taker_trade(
            "trade-price-improved",
            "venue-order-price-improved",
            dec!(0.80),
            dec!(5.125),
            Decimal::ZERO,
            checked_at,
        );
        let owned_orders = HashMap::from([(
            "venue-order-price-improved".to_string(),
            "venue-order-price-improved".to_string(),
        )]);

        let fills = rest_fill_backfill_plan(
            process_id,
            std::slice::from_ref(&order),
            &owned_orders,
            &[trade],
            checked_at,
        )
        .unwrap();

        assert_eq!(fills[0].price, dec!(0.80));
        assert_eq!(fills[0].size, dec!(5.125));
        validate_live_cumulative_fill_economics(
            &order.request,
            fills[0].size,
            fills[0].price * fills[0].size,
        )
        .unwrap();
    }

    #[test]
    fn live_fill_fee_uses_sealed_rate_for_websocket_and_rest_economics() {
        let order = rest_backfill_order(
            Uuid::new_v4(),
            "venue-order-fee",
            OrderSide::Buy,
            dec!(0.50),
            dec!(10),
            Utc::now(),
        );

        assert_eq!(
            live_fill_fee(&order, dec!(0.40), dec!(10)).unwrap(),
            dec!(0.60)
        );
        let mut missing = order;
        missing.request.metadata = json!({"execution_intent": "entry"});
        assert!(live_fill_fee(&missing, dec!(0.40), dec!(10)).is_err());
    }

    #[test]
    fn rest_fill_backfill_uses_exact_maker_order_economics() {
        use polymarket_client_sdk_v2::clob::types::response::MakerOrder;

        let process_id = Uuid::new_v4();
        let checked_at = Utc::now();
        let order = rest_backfill_order(
            process_id,
            "maker-order-1",
            OrderSide::Sell,
            dec!(0.40),
            dec!(1),
            checked_at - chrono::Duration::seconds(2),
        );
        let trade = TradeResponse::builder()
            .id("maker-trade-1")
            .taker_order_id("foreign-taker")
            .market(polymarket_client_sdk_v2::types::B256::ZERO)
            .asset_id(U256::from(999))
            .side(SdkSide::Buy)
            .size(dec!(9))
            .fee_rate_bps(dec!(99))
            .price(dec!(0.90))
            .status(TradeStatusType::Confirmed)
            .match_time(checked_at - chrono::Duration::seconds(1))
            .last_update(checked_at)
            .outcome("YES")
            .bucket_index(0)
            .owner(Uuid::max())
            .maker_address(Address::ZERO)
            .maker_orders(vec![MakerOrder::builder()
                .order_id("maker-order-1")
                .owner(Uuid::max())
                .maker_address(Address::ZERO)
                .matched_amount(dec!(1))
                .price(dec!(0.42))
                .fee_rate_bps(dec!(7))
                .asset_id(U256::from(1))
                .outcome("YES")
                .side(SdkSide::Sell)
                .build()])
            .transaction_hash(polymarket_client_sdk_v2::types::B256::ZERO)
            .trader_side(polymarket_client_sdk_v2::clob::types::TraderSide::Maker)
            .build();
        let owned_orders =
            HashMap::from([("maker-order-1".to_string(), "maker-order-1".to_string())]);

        let fills = rest_fill_backfill_plan(
            process_id,
            std::slice::from_ref(&order),
            &owned_orders,
            std::slice::from_ref(&trade),
            checked_at,
        )
        .unwrap();

        assert_eq!(fills.len(), 1);
        assert_eq!(fills[0].token_id, "1");
        assert_eq!(fills[0].price, dec!(0.42));
        assert_eq!(fills[0].size, dec!(1));
        assert_eq!(fills[0].fee, dec!(0.060900));
    }

    #[tokio::test]
    async fn user_ws_health_is_diagnostic_and_rest_freshness_controls_backup_readiness() {
        let venue = LiveVenue::new_for_test(live_config())
            .unwrap()
            .bind_process(Uuid::new_v4(), &live_execution())
            .unwrap();
        {
            let mut transport = venue.transport_state.lock().await;
            transport.user_ws_connected = true;
            transport.last_user_ws_pong_at = Some(Utc::now() + chrono::Duration::minutes(1));
        }
        {
            let mut state = venue.readiness_state.lock().await;
            state.last_rest_reconcile_at = Some(Utc::now());
            state.idempotency_clean = true;
            state.unresolved_live_order_count = 0;
            state.manual_entries_enabled = true;
            state.manual_entries_reason = None;
            state.process_accounting_proven = true;
            state.process_accounting_entry_safe = true;
            state.process_accounting_status = "proven".to_string();
        }
        {
            let mut global = venue.global_entry_gate.lock().await;
            global.halted = false;
            global.reason = "test_checked_enable".to_string();
        }

        let status = venue.live_status().await.unwrap();
        assert!(status.entries_enabled);
        assert!(status.live_confirmed);
        assert!(!status.geoblock_readable);
        assert_eq!(status.geoblock_blocked, None);
        assert_eq!(status.geoblock_country, None);
        assert_eq!(status.geoblock_region, None);
        assert_eq!(status.last_geoblock_check_age_secs, None);
        assert_eq!(status.last_user_ws_pong_age_secs, None);
        assert_eq!(status.reason, None);

        {
            let mut transport = venue.transport_state.lock().await;
            transport.user_ws_connected = false;
            transport.last_user_ws_pong_at = None;
        }
        let status = venue.live_status().await.unwrap();
        assert!(status.entries_enabled);
        assert!(!status.user_ws_connected);

        venue.readiness_state.lock().await.last_rest_reconcile_at =
            Some(Utc::now() + chrono::Duration::minutes(1));
        let status = venue.live_status().await.unwrap();
        assert!(!status.entries_enabled);
        assert_eq!(status.last_rest_reconcile_age_secs, None);
        assert_eq!(status.reason.as_deref(), Some("live_rest_reconcile_stale"));
    }

    #[tokio::test]
    async fn reconciliation_failure_gate_recovers_without_mutating_manual_authorization() {
        let venue = LiveVenue::new_for_test(live_config())
            .unwrap()
            .bind_process(Uuid::new_v4(), &live_execution())
            .unwrap();
        {
            let mut state = venue.readiness_state.lock().await;
            state.last_rest_reconcile_at = Some(Utc::now());
            state.idempotency_clean = true;
            state.unresolved_live_order_count = 0;
            state.manual_entries_enabled = true;
            state.manual_entries_reason = None;
            state.process_accounting_proven = true;
            state.process_accounting_entry_safe = true;
            state.process_accounting_status = "proven".to_string();
        }
        {
            let mut global = venue.global_entry_gate.lock().await;
            global.halted = false;
            global.reason = "configured_resume_authorization".to_string();
        }

        venue
            .update_live_reconciliation_health(
                1,
                Some("temporary settlement discovery failure".to_string()),
            )
            .await
            .unwrap();
        let degraded = venue.live_status().await.unwrap();
        assert!(!degraded.entries_enabled);
        assert!(venue.readiness_state.lock().await.manual_entries_enabled);

        venue
            .update_live_reconciliation_health(0, None)
            .await
            .unwrap();
        let recovered = venue.live_status().await.unwrap();
        assert!(recovered.entries_enabled);
        assert!(venue.readiness_state.lock().await.manual_entries_enabled);

        venue
            .update_live_reconciliation_health(1, None)
            .await
            .unwrap();
        let settlement_pending = venue.live_status().await.unwrap();
        assert!(settlement_pending.entries_enabled);
        assert_eq!(
            venue.readiness_state.lock().await.pending_settlement_count,
            1
        );
    }

    #[tokio::test]
    async fn clean_reconciliation_restores_readiness_without_manual_reenable() {
        let venue = LiveVenue::new_for_test(live_config())
            .unwrap()
            .bind_process(Uuid::new_v4(), &live_execution())
            .unwrap();
        {
            let mut state = venue.readiness_state.lock().await;
            state.last_rest_reconcile_at = Some(Utc::now());
            state.idempotency_clean = false;
            state.unresolved_live_order_count = 1;
            state.manual_entries_enabled = true;
            state.manual_entries_reason = None;
            state.process_accounting_proven = true;
            state.process_accounting_entry_safe = true;
            state.process_accounting_status = "proven".to_string();
        }
        {
            let mut global = venue.global_entry_gate.lock().await;
            global.halted = false;
            global.reason = "process_checked_enable".to_string();
        }

        let status = venue.live_status().await.unwrap();
        assert!(!status.entries_enabled);
        assert_eq!(status.reason.as_deref(), Some("live_idempotency_not_clean"));

        {
            let mut state = venue.readiness_state.lock().await;
            state.last_rest_reconcile_at = Some(Utc::now());
            state.idempotency_clean = true;
            state.unresolved_live_order_count = 0;
        }
        let status = venue.live_status().await.unwrap();
        assert!(status.entries_enabled);
        assert_eq!(status.reason, None);
        assert!(venue.readiness_state.lock().await.manual_entries_enabled);
        assert!(!venue.global_entry_gate.lock().await.halted);
    }

    #[test]
    fn user_event_hash_is_stable() {
        let raw = json!({"event_type":"trade","id":"t1","status":"CONFIRMED"});
        let event = LiveVenue::parse_user_event(raw);
        assert_eq!(event.hash(), event.hash());
        assert_eq!(event.event_type, "trade");
        assert_eq!(event.venue_trade_id.as_deref(), Some("t1"));

        let cancellation = LiveVenue::parse_user_event(json!({
            "type": "CANCELLATION",
            "id": "order-1"
        }));
        assert_eq!(cancellation.event_type, "cancellation");
        assert!(is_cancelled_order_status(
            cancellation.event_status.as_deref()
        ));
        for status in ["CANCELED", "CANCELLED", " cancellation "] {
            assert!(is_cancelled_order_status(Some(status)));
        }
        assert!(!is_cancelled_order_status(Some("CANCELED_BY_SYSTEM")));
        assert!(!is_cancelled_order_status(Some("CANCEL")));
    }

    #[test]
    fn user_ws_omits_empty_market_filter_and_strictly_classifies_health() {
        let config = live_config();
        let subscription = user_ws_subscription_payload(&config).unwrap();
        assert!(subscription.get("markets").is_none());
        assert!(matches!(
            classify_user_ws_text("PONG", &[]).unwrap(),
            UserWsText::HeartbeatPong
        ));
        assert!(classify_user_ws_text("not-json", &[]).is_err());
        assert!(classify_user_ws_text(r#"{"status":"unauthorized"}"#, &[]).is_err());

        let event = json!({
            "event_type": "trade",
            "id": "trade-1",
            "market": "condition-1",
            "asset_id": "token-1",
            "status": "CONFIRMED"
        });
        assert!(matches!(
            classify_user_ws_text(&event.to_string(), &[]).unwrap(),
            UserWsText::UserEvent(_)
        ));
        assert!(
            classify_user_ws_text(&event.to_string(), &["different-condition".to_string()])
                .is_err()
        );
    }

    #[test]
    fn user_ws_heartbeat_requires_an_acknowledgement_within_the_bound() {
        let timeout = Duration::from_secs(20);
        assert!(!user_ws_heartbeat_ack_timed_out(None, timeout));
        assert!(!user_ws_heartbeat_ack_timed_out(
            Some(Duration::from_secs(19)),
            timeout
        ));
        assert!(user_ws_heartbeat_ack_timed_out(
            Some(Duration::from_secs(20)),
            timeout
        ));
    }

    #[test]
    fn user_ws_reconnect_backoff_is_bounded() {
        assert_eq!(
            next_user_ws_reconnect_delay(USER_WS_RECONNECT_INITIAL_DELAY),
            Duration::from_secs(2)
        );
        assert_eq!(
            next_user_ws_reconnect_delay(Duration::from_secs(16)),
            USER_WS_RECONNECT_MAX_DELAY
        );
        assert_eq!(
            next_user_ws_reconnect_delay(USER_WS_RECONNECT_MAX_DELAY),
            USER_WS_RECONNECT_MAX_DELAY
        );
    }

    #[test]
    fn clob_pagination_fails_closed_on_bad_cursors_metadata_and_overflow() {
        let mut seen = HashSet::new();
        assert_eq!(
            next_clob_reconciliation_cursor(CLOB_TERMINAL_CURSOR, &mut seen, "trades").unwrap(),
            None
        );
        assert_eq!(
            next_clob_reconciliation_cursor("YWJj", &mut seen, "trades").unwrap(),
            Some("YWJj".to_string())
        );
        assert!(next_clob_reconciliation_cursor("YWJj", &mut seen, "trades").is_err());
        assert!(next_clob_reconciliation_cursor("", &mut seen, "trades").is_err());
        assert!(next_clob_reconciliation_cursor(
            &"A".repeat(MAX_CLOB_CURSOR_BYTES + 1),
            &mut seen,
            "trades"
        )
        .is_err());

        assert!(validate_clob_page_metadata("trades", 2, 2, 100).is_ok());
        assert!(validate_clob_page_metadata("trades", 2, 1, 100).is_err());
        assert!(validate_clob_page_metadata("trades", 0, 0, 0).is_err());
        assert!(validate_clob_page_metadata(
            "trades",
            1,
            1,
            MAX_CLOB_RECONCILIATION_ROWS as u64 + 1
        )
        .is_err());
        assert!(checked_clob_row_count(MAX_CLOB_RECONCILIATION_ROWS, 1, "trades").is_err());
        assert!(checked_clob_row_count(usize::MAX, 1, "trades").is_err());
    }

    #[test]
    fn generation_compare_prevents_enable_and_post_races() {
        let mut gate = GlobalLiveEntryGate {
            halted: true,
            reason: "checked_enable_revalidation".to_string(),
            safety_generation: 11,
        };
        let mut state = LiveVenueState::fail_closed();
        state.reconciled_safety_generation = Some(11);
        record_global_entry_halt(&mut gate, "account_reconciliation_failed");
        assert!(!commit_checked_live_enable(&mut gate, &mut state, 11));
        assert!(gate.halted);
        assert!(!state.manual_entries_enabled);

        gate.halted = false;
        gate.reason = "process_checked_enable".to_string();
        state.manual_entries_enabled = true;
        state.reconciled_safety_generation = Some(gate.safety_generation);
        let admitted_generation = gate.safety_generation;
        record_global_entry_halt(&mut gate, "account_transport_unavailable");
        assert_eq!(
            commit_live_post_attempt(&mut gate, &mut state, admitted_generation),
            Some(LiveExecutionGateReason::GlobalHalt)
        );

        gate.halted = false;
        gate.reason = "process_checked_enable".to_string();
        state.manual_entries_enabled = true;
        state.reconciled_safety_generation = Some(gate.safety_generation);
        let admitted_generation = gate.safety_generation;
        assert_eq!(
            commit_live_post_attempt(&mut gate, &mut state, admitted_generation),
            None
        );
        assert!(!gate.halted);
        assert!(state.manual_entries_enabled);
        assert_eq!(
            state.reconciled_safety_generation,
            Some(admitted_generation)
        );
    }

    #[test]
    fn cumulative_capital_exposure_enforces_the_daily_hard_cap() {
        assert_eq!(
            live_capital_exposure_gate(dec!(10), Some(dec!(10)), Some(dec!(30))),
            None
        );
        assert_eq!(
            live_capital_exposure_gate(dec!(10.01), Some(dec!(10)), Some(dec!(30))),
            Some(LiveExecutionGateReason::DailyLossLimit)
        );
        assert_eq!(
            live_capital_exposure_gate(dec!(5.01), Some(dec!(10)), Some(dec!(5))),
            Some(LiveExecutionGateReason::OpenNotionalLimit)
        );
    }

    #[tokio::test]
    async fn unbound_admin_reconcile_cannot_supply_process_scope() {
        let venue = LiveVenue::new_for_test(live_config()).unwrap();
        for request in [
            AccountReconcileRequest {
                account_address: None,
                lookback_hours: Some(1),
                process_id: Some(Uuid::new_v4()),
                account_ref: None,
                credential_account_fingerprint_sha256: None,
                dry_run: true,
                token_id: None,
                source: Some("admin".to_string()),
            },
            AccountReconcileRequest {
                account_address: None,
                lookback_hours: Some(1),
                process_id: None,
                account_ref: Some("polymarket-test".to_string()),
                credential_account_fingerprint_sha256: None,
                dry_run: true,
                token_id: None,
                source: Some("admin".to_string()),
            },
            AccountReconcileRequest {
                account_address: None,
                lookback_hours: Some(1),
                process_id: None,
                account_ref: None,
                credential_account_fingerprint_sha256: Some("a".repeat(64)),
                dry_run: true,
                token_id: None,
                source: Some("admin".to_string()),
            },
        ] {
            let error = venue
                .run_account_reconcile(request)
                .await
                .unwrap_err()
                .to_string();
            assert!(error.contains("rejects caller-supplied process scope"));
        }
    }

    #[tokio::test]
    async fn live_status_blocks_entries_when_submit_disabled() {
        let venue = LiveVenue::new_for_test(live_config()).unwrap();
        let status = venue.live_status().await.unwrap();
        assert!(!status.entries_enabled);
        assert_eq!(status.reason.as_deref(), Some("live_order_submit_disabled"));
    }

    #[tokio::test]
    async fn disabled_entries_block_entry_and_metadata_labeled_exit_intents() {
        let process_id = uuid::Uuid::new_v4();
        let venue = LiveVenue::new_for_test(live_config())
            .unwrap()
            .bind_process(process_id, &live_execution())
            .unwrap();
        venue.global_entry_gate.lock().await.halted = false;
        let base = OrderRequest {
            client_order_id: uuid::Uuid::new_v4(),
            process_id: Some(process_id),
            market_id: "market".to_string(),
            token_id: "1".to_string(),
            side: OrderSide::Buy,
            order_type: OrderType::Fok,
            price: dec!(0.50),
            size: dec!(2),
            metadata: json!({"purpose": "entry"}),
        };

        let entry = submit_with_test_guard(&venue, base.clone()).await.unwrap();
        assert_live_gate_rejection(&entry, LiveExecutionGateReason::ManualEnableRequired);

        let mut exit = base;
        exit.metadata = json!({"purpose": "exit"});
        let exit = submit_with_test_guard(&venue, exit).await.unwrap();
        assert_live_gate_rejection(&exit, LiveExecutionGateReason::ManualEnableRequired);
    }

    #[tokio::test]
    async fn disabled_entries_block_non_exit_intents() {
        let process_id = uuid::Uuid::new_v4();
        let venue = LiveVenue::new_for_test(live_config())
            .unwrap()
            .bind_process(process_id, &live_execution())
            .unwrap();
        venue.global_entry_gate.lock().await.halted = false;
        let mut request = OrderRequest {
            client_order_id: uuid::Uuid::new_v4(),
            process_id: Some(process_id),
            market_id: "market".to_string(),
            token_id: "1".to_string(),
            side: OrderSide::Buy,
            order_type: OrderType::Fok,
            price: dec!(0.50),
            size: dec!(2),
            metadata: json!({"execution_intent": "risk_reduction"}),
        };

        for intent in ["risk_reduction", "admin_manual"] {
            request.client_order_id = uuid::Uuid::new_v4();
            request.metadata = json!({"execution_intent": intent});
            let order = submit_with_test_guard(&venue, request.clone())
                .await
                .unwrap();
            assert_live_gate_rejection(&order, LiveExecutionGateReason::ManualEnableRequired);
        }
    }

    #[tokio::test]
    async fn per_order_risk_denial_is_a_zero_post_nonfatal_record() {
        let process_id = Uuid::new_v4();
        let venue = LiveVenue::new_for_test(live_config())
            .unwrap()
            .bind_process(process_id, &live_execution())
            .unwrap();
        let request = OrderRequest {
            client_order_id: Uuid::new_v4(),
            process_id: Some(process_id),
            market_id: "market".to_string(),
            token_id: "1".to_string(),
            side: OrderSide::Buy,
            order_type: OrderType::Fok,
            price: dec!(0.75),
            size: dec!(3),
            metadata: json!({"execution_intent": "entry"}),
        };

        let order = submit_with_test_guard(&venue, request).await.unwrap();

        assert_live_gate_rejection(&order, LiveExecutionGateReason::PerOrderNotionalLimit);
    }

    #[tokio::test]
    async fn omitted_per_order_limit_does_not_create_a_risk_gate() {
        let process_id = Uuid::new_v4();
        let mut execution = live_execution();
        execution.max_order_notional_usd = None;
        let venue = LiveVenue::new_for_test(live_config())
            .unwrap()
            .bind_process(process_id, &execution)
            .unwrap();
        let request = OrderRequest {
            client_order_id: Uuid::new_v4(),
            process_id: Some(process_id),
            market_id: "market".to_string(),
            token_id: "1".to_string(),
            side: OrderSide::Buy,
            order_type: OrderType::Fok,
            price: dec!(0.75),
            size: dec!(3),
            metadata: json!({"execution_intent": "entry"}),
        };

        let order = submit_with_test_guard(&venue, request).await.unwrap();

        assert_live_gate_rejection(&order, LiveExecutionGateReason::GlobalHalt);
    }

    #[tokio::test]
    async fn process_bound_venue_rejects_missing_or_mismatched_process_identity() {
        let process_id = uuid::Uuid::new_v4();
        let venue = LiveVenue::new_for_test(live_config())
            .unwrap()
            .bind_process(process_id, &live_execution())
            .unwrap();
        let request = OrderRequest {
            client_order_id: uuid::Uuid::new_v4(),
            process_id: Some(uuid::Uuid::new_v4()),
            market_id: "market".to_string(),
            token_id: "1".to_string(),
            side: OrderSide::Buy,
            order_type: OrderType::Fok,
            price: dec!(0.50),
            size: dec!(2),
            metadata: json!({"purpose": "entry"}),
        };
        let error = venue.submit_order(request).await.unwrap_err().to_string();
        assert!(error.contains("must match bound process"));
    }

    #[tokio::test]
    async fn manual_enable_remains_fail_closed_before_first_successful_reconcile() {
        let venue = LiveVenue::new_for_test(live_config())
            .unwrap()
            .bind_process(uuid::Uuid::new_v4(), &live_execution())
            .unwrap();
        let error = venue
            .set_live_entries_enabled(true, None)
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("live persistence store is not configured"));
        let status = venue.live_status().await.unwrap();
        assert!(!status.entries_enabled);
        assert!(!status.idempotency_clean);
        assert_eq!(status.last_rest_reconcile_age_secs, None);
        assert_eq!(
            status.reason.as_deref(),
            Some("live_global_halt:global_enable_required")
        );
    }

    #[tokio::test]
    async fn configured_restart_authorization_does_not_bypass_reconciliation_readiness() {
        let venue = LiveVenue::new_for_test(live_config())
            .unwrap()
            .bind_process(uuid::Uuid::new_v4(), &live_execution())
            .unwrap();

        venue
            .restore_configured_entries_after_restart()
            .await
            .unwrap();

        let status = venue.live_status().await.unwrap();
        assert!(!status.entries_enabled);
        assert!(!status.process_accounting_proven);
        assert!(!status.idempotency_clean);
        assert_eq!(
            status.reason.as_deref(),
            Some("live_process_accounting_not_proven:unproven")
        );
        assert!(venue.readiness_state.lock().await.manual_entries_enabled);
        assert!(!venue.global_entry_gate.lock().await.halted);
    }

    #[tokio::test]
    async fn wallet_wide_halt_closes_every_bound_submit_path() {
        let root = LiveVenue::new_for_test(live_config()).unwrap();
        let process_id = Uuid::new_v4();
        let bound = root.bind_process(process_id, &live_execution()).unwrap();
        let identity =
            canonical_configured_account_identity(&bound.config, "polymarket-test").unwrap();
        {
            let mut transport = bound.transport_state.lock().await;
            transport.user_ws_connected = true;
            transport.last_user_ws_pong_at = Some(Utc::now());
        }
        {
            let mut state = bound.readiness_state.lock().await;
            state.last_rest_reconcile_at = Some(Utc::now());
            state.idempotency_clean = true;
            state.unresolved_live_order_count = 0;
            state.process_accounting_proven = true;
            state.process_accounting_entry_safe = true;
            state.process_accounting_status = "proven".to_string();
            state.credential_account_fingerprint_sha256 = Some(identity.fingerprint_sha256);
            state.reconciled_safety_generation = Some(0);
            state.manual_entries_enabled = true;
            state.manual_entries_reason = None;
        }
        {
            let mut global = bound.global_entry_gate.lock().await;
            global.halted = false;
            global.reason = "process_checked_enable".to_string();
        }
        assert!(bound.live_status().await.unwrap().entries_enabled);

        root.set_live_entries_enabled(false, Some("test_global_halt".to_string()))
            .await
            .unwrap();
        let status = bound.live_status().await.unwrap();
        assert!(!status.entries_enabled);
        assert_eq!(
            status.reason.as_deref(),
            Some("live_global_halt:test_global_halt")
        );

        let exit = OrderRequest {
            client_order_id: Uuid::new_v4(),
            process_id: Some(process_id),
            market_id: "market".to_string(),
            token_id: "1".to_string(),
            side: OrderSide::Sell,
            order_type: OrderType::Fok,
            price: dec!(0.50),
            size: dec!(1),
            metadata: json!({"purpose": "exit"}),
        };
        let order = submit_with_test_guard(&bound, exit).await.unwrap();
        assert_live_gate_rejection(&order, LiveExecutionGateReason::GlobalHalt);
    }

    #[tokio::test]
    async fn process_bound_live_submit_cannot_bypass_adjacent_guard() {
        let process_id = Uuid::new_v4();
        let venue = LiveVenue::new_for_test(live_config())
            .unwrap()
            .bind_process(process_id, &live_execution())
            .unwrap();
        let request = OrderRequest {
            client_order_id: Uuid::new_v4(),
            process_id: Some(process_id),
            market_id: "market".to_string(),
            token_id: "1".to_string(),
            side: OrderSide::Buy,
            order_type: OrderType::Fok,
            price: dec!(0.50),
            size: dec!(1),
            metadata: json!({"purpose": "entry"}),
        };

        let error = venue.submit_order(request).await.unwrap_err().to_string();

        assert!(error.contains("requires an adjacent pre-POST guard"));
    }

    #[tokio::test]
    async fn process_bound_live_submit_rejects_an_explicitly_missing_guard_before_io() {
        let process_id = Uuid::new_v4();
        let venue = LiveVenue::new_for_test(live_config())
            .unwrap()
            .bind_process(process_id, &live_execution())
            .unwrap();
        let request = OrderRequest {
            client_order_id: Uuid::new_v4(),
            process_id: Some(process_id),
            market_id: "market".to_string(),
            token_id: "1".to_string(),
            side: OrderSide::Buy,
            order_type: OrderType::Fok,
            price: dec!(0.50),
            size: dec!(1),
            metadata: json!({"purpose": "entry"}),
        };

        let error = venue
            .submit_order_with_pre_post_guard(request, None)
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("requires an adjacent pre-POST guard"));
    }

    #[tokio::test]
    async fn repeated_metadata_labeled_exit_attempts_remain_zero_post_while_halted() {
        let process_id = Uuid::new_v4();
        let venue = LiveVenue::new_for_test(live_config())
            .unwrap()
            .bind_process(process_id, &live_execution())
            .unwrap();
        let request = OrderRequest {
            client_order_id: Uuid::new_v4(),
            process_id: Some(process_id),
            market_id: "market".to_string(),
            token_id: "1".to_string(),
            side: OrderSide::Sell,
            order_type: OrderType::Fok,
            price: dec!(0.50),
            size: dec!(1),
            metadata: json!({"purpose": "exit"}),
        };

        for _ in 0..2 {
            let order = submit_with_test_guard(&venue, request.clone())
                .await
                .unwrap();
            assert_live_gate_rejection(&order, LiveExecutionGateReason::GlobalHalt);
        }
    }

    #[test]
    fn canonical_identity_is_signature_aware_and_fingerprint_is_bounded() {
        let eoa = canonical_configured_account_identity(&live_config(), "polymarket-test").unwrap();
        assert_eq!(eoa.account_address, eoa.signer_address);
        assert_eq!(eoa.fingerprint_sha256.len(), 64);

        let mut poly1271 = live_config();
        poly1271.signature_type = Some("3".to_string());
        poly1271.funder_address = Some("0x0000000000000000000000000000000000000002".to_string());
        let poly1271 = canonical_configured_account_identity(&poly1271, "polymarket-test").unwrap();
        assert_ne!(poly1271.account_address, poly1271.signer_address);
        assert_eq!(
            poly1271.account_address,
            "0x0000000000000000000000000000000000000002"
        );
        assert_eq!(poly1271.fingerprint_sha256.len(), 64);
    }

    #[test]
    fn canonical_identity_fingerprint_survives_api_key_rotation_but_not_account_rotation() {
        let config = live_config();
        let baseline = canonical_configured_account_identity(&config, "polymarket-test")
            .unwrap()
            .fingerprint_sha256;

        let mut rotated_api = config.clone();
        rotated_api.clob_api_key = Some("00000000-0000-0000-0000-000000000002".to_string());
        rotated_api.clob_secret = Some("rotated-secret".to_string());
        rotated_api.clob_passphrase = Some("rotated-passphrase".to_string());
        assert_eq!(
            canonical_configured_account_identity(&rotated_api, "polymarket-test")
                .unwrap()
                .fingerprint_sha256,
            baseline
        );

        assert_ne!(
            canonical_configured_account_identity(&config, "polymarket-other")
                .unwrap()
                .fingerprint_sha256,
            baseline
        );
    }

    #[test]
    fn definitive_client_rejections_are_safe_nonfatal_order_outcomes() {
        use polymarket_client_sdk_v2::error::{Error as SdkError, Method, StatusCode};

        let rejected = anyhow::Error::new(SdkError::status(
            StatusCode::BAD_REQUEST,
            Method::POST,
            "/order".to_string(),
            "maker address not allowed, please use the deposit wallet flow",
        ));
        assert!(is_definitive_live_submit_error(&rejected));

        let duplicate_or_ambiguous = anyhow::Error::new(SdkError::status(
            StatusCode::CONFLICT,
            Method::POST,
            "/order".to_string(),
            "conflict",
        ));
        assert!(!is_definitive_live_submit_error(&duplicate_or_ambiguous));
        assert!(!is_definitive_live_submit_error(&anyhow::anyhow!(
            "connection reset after write"
        )));
    }

    #[test]
    fn definitive_fok_liquidity_rejection_has_stable_decision_reason() {
        assert_eq!(
            definitive_live_venue_reject_reason(
                OrderType::Fok,
                Some("order couldn't be fully filled. FOK orders are fully filled or killed."),
            ),
            LIVE_VENUE_FOK_UNFILLED_REASON
        );
        assert_eq!(
            definitive_live_venue_reject_reason(OrderType::Fok, Some("invalid signature")),
            LIVE_VENUE_REJECTED_REASON
        );
        assert_eq!(
            definitive_live_venue_reject_reason(
                OrderType::Gtc,
                Some("order couldn't be fully filled")
            ),
            LIVE_VENUE_REJECTED_REASON
        );
    }

    #[test]
    fn only_retryable_pre_submit_transport_failures_preserve_liveness() {
        use polymarket_client_sdk_v2::error::{Error as SdkError, Method, StatusCode};

        let unavailable = anyhow::Error::new(SdkError::status(
            StatusCode::SERVICE_UNAVAILABLE,
            Method::GET,
            "/tick-size".to_string(),
            "temporarily unavailable",
        ));
        assert!(is_retryable_live_pre_submit_error(&unavailable));

        let rate_limited = anyhow::Error::new(SdkError::status(
            StatusCode::TOO_MANY_REQUESTS,
            Method::GET,
            "/tick-size".to_string(),
            "rate limited",
        ));
        assert!(is_retryable_live_pre_submit_error(&rate_limited));

        let unauthorized = anyhow::Error::new(SdkError::status(
            StatusCode::UNAUTHORIZED,
            Method::GET,
            "/auth/api-key".to_string(),
            "unauthorized",
        ));
        assert!(!is_retryable_live_pre_submit_error(&unauthorized));
        assert!(is_retryable_live_pre_submit_error(&anyhow::Error::new(
            std::io::Error::new(std::io::ErrorKind::TimedOut, "request timed out")
        )));
    }

    #[test]
    fn transient_pre_submit_failure_is_a_durable_zero_post_gate_outcome() {
        let process_id = Uuid::new_v4();
        let request = OrderRequest {
            client_order_id: Uuid::new_v4(),
            process_id: Some(process_id),
            market_id: "market".to_string(),
            token_id: "1".to_string(),
            side: OrderSide::Buy,
            order_type: OrderType::Fok,
            price: dec!(0.50),
            size: dec!(1),
            metadata: json!({"purpose": "entry"}),
        };
        let error = anyhow::anyhow!("tick-size request timed out");

        let order = live_pre_submit_transient_gate_order(request, "order_build", &error).unwrap();

        assert_live_gate_rejection(&order, LiveExecutionGateReason::VenueReadiness);
        assert_eq!(
            order.request.metadata["live_pre_submit_error"],
            json!({
                "stage": "order_build",
                "error_chain": "tick-size request timed out",
                "post_attempted": false,
                "retryable": true,
            })
        );
    }

    #[test]
    fn blank_counterparty_fee_is_tolerated_only_for_authenticated_taker_trades() {
        let trade = |trader_side: &str| {
            serde_json::json!({
                "id": "32f00293-931f-4ffe-826c-f46d2124a82a",
                "taker_order_id": "0xe4fc3623f06ec47301f3feb30fb894137850d3d6bd9161c545d842cf6e610f91",
                "market": "0x21582805dbfc8aea9dc1cbbe7171f6a60c477726681588c98a97410d4f194fec",
                "asset_id": "64891112840096581114786599417318199598343837807127888883355968643722878718210",
                "side": "BUY",
                "size": "5",
                "fee_rate_bps": "0",
                "price": "0.94",
                "status": "MATCHED",
                "match_time": "1786677391",
                "last_update": "1786677391",
                "outcome": "Up",
                "bucket_index": 0,
                "owner": "25188274-5ec9-6ed4-3f3d-44965847a9f5",
                "maker_address": "0x74D0dA822ba46c7325bB78E74C915976e76159af",
                "maker_orders": [{
                    "order_id": "0xa6a02b11ed2d49b8d64a844782983d6fb786651ee29ef75be6e8b036e9f1776a",
                    "owner": "0356ef53-9f23-82e0-6e8c-3d9aa95d8b9e",
                    "maker_address": "0x6A9CEA200E4bBFd93d9Fa9b01563e0936Ff10F59",
                    "matched_amount": "5",
                    "price": "0.06",
                    "fee_rate_bps": "",
                    "asset_id": "113949045949963031393883453961344691338965896091858872621221708954515291442606",
                    "outcome": "Down",
                    "side": "BUY"
                }],
                "transaction_hash": "0x6daa5009f990979e5d369d40d1f245c4d7219d805965e0dddcd9876f9764ce6f",
                "trader_side": trader_side,
                "error_msg": null
            })
        };
        let mut taker_page = serde_json::json!({
            "data": [trade("TAKER")],
            "next_cursor": CLOB_TERMINAL_CURSOR,
            "limit": 500,
            "count": 1
        });
        assert_eq!(normalize_blank_taker_counterparty_fees(&mut taker_page), 1);
        let decoded: Page<TradeResponse> = serde_json::from_value(taker_page).unwrap();
        assert_eq!(
            decoded.data[0].maker_orders[0].fee_rate_bps,
            SdkDecimal::ZERO
        );

        let mut maker_page = serde_json::json!({"data": [trade("MAKER")]});
        assert_eq!(normalize_blank_taker_counterparty_fees(&mut maker_page), 0);
        assert_eq!(maker_page["data"][0]["maker_orders"][0]["fee_rate_bps"], "");
    }

    #[test]
    fn authenticated_trade_read_signature_matches_the_sdk_contract() {
        let signature =
            clob_l2_signature("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=", "1GET/").unwrap();
        assert_eq!(signature, "eHaylCwqRSOa2LFD77Nt_SaTpbsxzN8eTEI3LryhEj4=");
    }

    #[test]
    fn process_binding_rejects_nil_identity() {
        let venue = LiveVenue::new_for_test(live_config()).unwrap();
        assert!(venue.bind_process(Uuid::nil(), &live_execution()).is_err());
        let mut blank_account = live_execution();
        blank_account.account_ref = Some("   ".to_string());
        assert!(venue.bind_process(Uuid::new_v4(), &blank_account).is_err());

        let process_id = Uuid::new_v4();
        let mut padded_account = live_execution();
        padded_account.account_ref = Some("  polymarket-test  ".to_string());
        let bound = venue.bind_process(process_id, &padded_account).unwrap();
        assert_eq!(bound.bound_process_id(), Some(process_id));
        assert_eq!(bound.bound_account_ref(), Some("polymarket-test"));
    }

    #[test]
    fn live_config_requires_transport_endpoints() {
        let mut config = live_config();
        config.user_ws_url.clear();
        assert!(config.validate_for_live().is_err());

        let mut config = live_config();
        config.clob_api_base_url.clear();
        assert!(config.validate_for_live().is_err());
    }
}
