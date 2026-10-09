use std::sync::Arc;

use async_trait::async_trait;
use axum::{
    body::{to_bytes, Body},
    http::{header::AUTHORIZATION, Request, StatusCode},
};
use chrono::Utc;
use polymarket_bot::{
    execution::{
        LiveIdentityDiagnostics, LiveOrderDryRunDiagnostics, LiveOrderDryRunRequest,
        LivePoly1271FunderProbeCandidate, LivePoly1271FunderProbeRequest,
        LivePoly1271FunderProbeResponse, LiveVenueStatus, LiveWalletAddressDiagnostics,
        LiveWalletCandidateAddressDiagnostics, LiveWalletTokenBalances, ReconciliationReport,
    },
    grafana_live::{
        EntryPermission, EntryStatusSelection, ProcessEntryPermission, TradingEntryStatusSnapshot,
    },
    http::{self, ControlApi, HttpError, MetricsResponse},
    models::{ProcessExecutionConfig, TradingProcess, TradingProcessConfig},
};
use rust_decimal::Decimal;
use serde_json::Value;
use tower::ServiceExt;
use uuid::Uuid;

#[derive(Debug, Default)]
struct FakeControlApi;

#[async_trait]
impl ControlApi for FakeControlApi {
    fn orders(
        &self,
    ) -> Result<&dyn polymarket_bot::execution::orders::OrderApplication, HttpError> {
        Ok(&FixtureOrders)
    }

    async fn metrics(&self) -> Result<MetricsResponse, HttpError> {
        Ok(MetricsResponse {
            service: "polymarket-bot".to_string(),
            captured_at: Utc::now(),
            counters: serde_json::json!({"scans": 1}),
            gauges: serde_json::json!({}),
        })
    }

    async fn prometheus_metrics(&self) -> Result<String, HttpError> {
        Ok("# TYPE polymarket_btc_polygon_oracle_ready gauge\npolymarket_btc_polygon_oracle_ready 1\n# TYPE polymarket_btc_clob_source_to_receive_lag_milliseconds gauge\npolymarket_btc_clob_source_to_receive_lag_milliseconds 2417\n# TYPE polymarket_btc_clob_source_to_receive_lag_unavailable_transitions_total counter\npolymarket_btc_clob_source_to_receive_lag_unavailable_transitions_total 9\n".to_string())
    }

    async fn btc_realtime_status(&self) -> Result<Value, HttpError> {
        Ok(serde_json::json!({"enabled": true, "readiness": {"ready": true}}))
    }

    async fn btc_entry_status(
        &self,
        request: http::EntryStatusRequest,
    ) -> Result<EntryStatusSelection, HttpError> {
        let process_id = request.process_id.unwrap_or_else(|| Uuid::from_u128(1));
        let snapshot = TradingEntryStatusSnapshot::new(
            Utc::now(),
            vec![
                ProcessEntryPermission {
                    process_id,
                    permission: EntryPermission::enabled(),
                },
                ProcessEntryPermission {
                    process_id: Uuid::from_u128(2),
                    permission: EntryPermission::disabled(),
                },
            ],
        );
        Ok(snapshot.select(&request.scope, request.process_id))
    }

    async fn live_status(&self) -> Result<LiveVenueStatus, HttpError> {
        Ok(LiveVenueStatus {
            mode: "live".to_string(),
            live_confirmed: false,
            geoblock_readable: true,
            geoblock_blocked: Some(false),
            geoblock_country: Some("PE".to_string()),
            geoblock_region: Some("LMA".to_string()),
            last_geoblock_check_age_secs: Some(1),
            order_submit_enabled: false,
            user_ws_enabled: false,
            user_ws_connected: false,
            last_user_ws_pong_age_secs: None,
            last_rest_reconcile_age_secs: None,
            idempotency_clean: true,
            unresolved_live_order_count: 0,
            process_accounting_proven: false,
            process_accounting_status: "unproven".to_string(),
            max_order_notional_usd: Decimal::ZERO,
            max_open_notional_usd: Decimal::ZERO,
            entries_enabled: false,
            reason: Some("test_fixture_disabled".to_string()),
        })
    }

    async fn live_identity_diagnostics(&self) -> Result<LiveIdentityDiagnostics, HttpError> {
        Ok(LiveIdentityDiagnostics {
            mode: "live".to_string(),
            clob_api_base_url: "https://clob.polymarket.com".to_string(),
            geoblock_readable: true,
            geoblock_blocked: Some(false),
            geoblock_country: Some("PE".to_string()),
            geoblock_region: Some("LMA".to_string()),
            geoblock_error: None,
            signer_address: Some("0x0000000000000000000000000000000000000001".to_string()),
            configured_funder_address: Some(
                "0x0000000000000000000000000000000000000002".to_string(),
            ),
            configured_signature_type: Some("3".to_string()),
            resolved_signature_type: Some("Poly1271".to_string()),
            authenticated_client_address: Some(
                "0x0000000000000000000000000000000000000001".to_string(),
            ),
            account_identity_valid: true,
            account_identity_fingerprint_sha256: Some("a".repeat(64)),
            credentials_present: true,
            api_keys_readable: true,
            api_keys_error: None,
            balance_allowance_readable: true,
            balance_allowance_error: None,
            collateral_balance: Some("20".to_string()),
            open_orders_readable: true,
            open_orders_error: None,
            open_orders_count: Some(0),
            checked_at: Utc::now(),
        })
    }

    async fn live_wallet_address_diagnostics(
        &self,
        candidate_addresses: Vec<String>,
    ) -> Result<LiveWalletAddressDiagnostics, HttpError> {
        Ok(LiveWalletAddressDiagnostics {
            mode: "live".to_string(),
            signer_address: Some("0x0000000000000000000000000000000000000001".to_string()),
            configured_funder_address: Some(
                "0x0000000000000000000000000000000000000002".to_string(),
            ),
            configured_signature_type: Some("3".to_string()),
            resolved_signature_type: Some("Poly1271".to_string()),
            authenticated_client_address: Some(
                "0x0000000000000000000000000000000000000001".to_string(),
            ),
            derived_proxy_wallet_address: Some(
                "0x0000000000000000000000000000000000000003".to_string(),
            ),
            derived_safe_wallet_address: Some(
                "0x0000000000000000000000000000000000000004".to_string(),
            ),
            expected_order_maker_address: Some(
                "0x0000000000000000000000000000000000000002".to_string(),
            ),
            expected_order_signer_field: Some(
                "0x0000000000000000000000000000000000000002".to_string(),
            ),
            configured_funder_matches_signer: Some(false),
            configured_funder_matches_proxy_wallet: Some(false),
            configured_funder_matches_safe_wallet: Some(false),
            configured_funder_deployed_as_deposit_wallet: Some(true),
            configured_funder_deployed_as_deposit_wallet_error: None,
            relayer_base_url: Some("https://relayer-v2.polymarket.com".to_string()),
            relayer_deployment_check_url: Some(
                "https://relayer-v2.polymarket.com/deployed?address=0x0000000000000000000000000000000000000002&type=WALLET"
                    .to_string(),
            ),
            signer_balances: Some(LiveWalletTokenBalances {
                address: "0x0000000000000000000000000000000000000001".to_string(),
                pol_wei: Some("1".to_string()),
                pusd: Some("20".to_string()),
                usdc_e: Some("0".to_string()),
                native_usdc: Some("0".to_string()),
                error: None,
            }),
            configured_funder_balances: Some(LiveWalletTokenBalances {
                address: "0x0000000000000000000000000000000000000002".to_string(),
                pol_wei: Some("0".to_string()),
                pusd: Some("30".to_string()),
                usdc_e: Some("0".to_string()),
                native_usdc: Some("0".to_string()),
                error: None,
            }),
            candidate_addresses: candidate_addresses
                .into_iter()
                .map(|address| LiveWalletCandidateAddressDiagnostics {
                    address: address.clone(),
                    matches_signer: Some(false),
                    matches_configured_funder: Some(false),
                    matches_authenticated_client: Some(false),
                    matches_proxy_wallet: Some(false),
                    matches_safe_wallet: Some(false),
                    deployed_as_deposit_wallet: Some(false),
                    deployed_as_deposit_wallet_error: None,
                    deposit_wallet_deployment_check_url: Some(
                        "https://relayer-v2.polymarket.com/deployed?address=0x0000000000000000000000000000000000000005&type=WALLET"
                            .to_string(),
                    ),
                    deployed_as_safe_wallet: Some(false),
                    deployed_as_safe_wallet_error: None,
                    safe_wallet_deployment_check_url: Some(
                        "https://relayer-v2.polymarket.com/deployed?address=0x0000000000000000000000000000000000000005&type=SAFE"
                            .to_string(),
                    ),
                    balances: None,
                    poly1271_authenticated_client_address: Some(address),
                    poly1271_api_keys_readable: true,
                    poly1271_api_keys_error: None,
                    poly1271_balance_allowance_readable: true,
                    poly1271_balance_allowance_error: None,
                    poly1271_collateral_balance: Some("0".to_string()),
                    poly1271_open_orders_readable: true,
                    poly1271_open_orders_error: None,
                    poly1271_open_orders_count: Some(0),
                })
                .collect(),
            verified_deposit_wallet_address: Some(
                "0x0000000000000000000000000000000000000002".to_string(),
            ),
            verified_deposit_wallet_candidates_count: 1,
            checked_at: Utc::now(),
        })
    }

    async fn live_order_dry_run(
        &self,
        _request: LiveOrderDryRunRequest,
    ) -> Result<LiveOrderDryRunDiagnostics, HttpError> {
        Ok(LiveOrderDryRunDiagnostics {
            mode: "live".to_string(),
            clob_api_base_url: "https://clob.polymarket.com".to_string(),
            signer_address: Some("0x0000000000000000000000000000000000000001".to_string()),
            configured_funder_address: Some(
                "0x0000000000000000000000000000000000000002".to_string(),
            ),
            configured_signature_type: Some("3".to_string()),
            resolved_signature_type: Some("Poly1271".to_string()),
            authenticated_client_address: Some(
                "0x0000000000000000000000000000000000000001".to_string(),
            ),
            order_signer: Some("0x0000000000000000000000000000000000000002".to_string()),
            order_maker: Some("0x0000000000000000000000000000000000000002".to_string()),
            order_signature_type: Some("3".to_string()),
            order_signer_matches_authenticated_client: Some(false),
            order_signer_matches_configured_funder: Some(true),
            order_maker_matches_configured_funder: Some(true),
            owner_redacted: true,
            signature_redacted: true,
            signed_order: serde_json::json!({
                "owner": "<redacted>",
                "order": {
                    "maker": "0x0000000000000000000000000000000000000002",
                    "signer": "0x0000000000000000000000000000000000000002",
                    "signatureType": "3",
                    "signature": "<redacted>"
                }
            }),
            checked_at: Utc::now(),
        })
    }

    async fn live_poly1271_funder_probe(
        &self,
        request: LivePoly1271FunderProbeRequest,
    ) -> Result<LivePoly1271FunderProbeResponse, HttpError> {
        let order_type = request
            .order_type
            .unwrap_or(polymarket_bot::models::OrderType::Fok);
        let verified = request.addresses.first().cloned();
        Ok(LivePoly1271FunderProbeResponse {
            mode: "live".to_string(),
            clob_api_base_url: "https://clob.polymarket.com".to_string(),
            signer_address: Some("0x0000000000000000000000000000000000000001".to_string()),
            token_id: request.token_id,
            side: request.side,
            order_type,
            price: request.price,
            size: request.size,
            candidates: request
                .addresses
                .into_iter()
                .map(|address| LivePoly1271FunderProbeCandidate {
                    address: address.clone(),
                    address_valid: true,
                    derive_credentials_ok: true,
                    derive_credentials_error: None,
                    authenticated_client_address: Some(
                        "0x0000000000000000000000000000000000000001".to_string(),
                    ),
                    api_keys_readable: true,
                    api_keys_error: None,
                    update_balance_allowance_ok: true,
                    update_balance_allowance_error: None,
                    balance_allowance_readable: true,
                    balance_allowance_error: None,
                    collateral_balance: Some("20".to_string()),
                    open_orders_readable: true,
                    open_orders_error: None,
                    open_orders_count: Some(0),
                    signed_order_build_ok: true,
                    signed_order_error: None,
                    signed_order_maker: Some(address.clone()),
                    signed_order_signer: Some(address.clone()),
                    signed_order_signature_type: Some("3".to_string()),
                    maker_matches_candidate: Some(true),
                    signer_matches_candidate: Some(true),
                    signature_type_is_poly1271: Some(true),
                    ready_for_live_canary: true,
                    signed_order: serde_json::json!({
                        "owner": "<redacted>",
                        "order": {
                            "maker": address,
                            "signer": address,
                            "signatureType": "3",
                            "signature": "<redacted>"
                        }
                    }),
                })
                .collect(),
            verified_funder_address: verified,
            verified_funder_candidates_count: 1,
            checked_at: Utc::now(),
        })
    }

    async fn live_halt(&self) -> Result<Value, HttpError> {
        Ok(serde_json::json!({"halted": true}))
    }

    async fn live_reconcile(&self) -> Result<Value, HttpError> {
        Ok(serde_json::json!({"open_orders": 0, "unresolved_count": 0}))
    }

    async fn live_set_entries_enabled(&self, enabled: bool) -> Result<LiveVenueStatus, HttpError> {
        Ok(LiveVenueStatus {
            mode: "live".to_string(),
            live_confirmed: true,
            geoblock_readable: true,
            geoblock_blocked: Some(false),
            geoblock_country: Some("PE".to_string()),
            geoblock_region: Some("LMA".to_string()),
            last_geoblock_check_age_secs: Some(1),
            order_submit_enabled: true,
            user_ws_enabled: true,
            user_ws_connected: true,
            last_user_ws_pong_age_secs: Some(1),
            last_rest_reconcile_age_secs: Some(1),
            idempotency_clean: true,
            unresolved_live_order_count: 0,
            process_accounting_proven: true,
            process_accounting_status: "proven".to_string(),
            max_order_notional_usd: Decimal::from(2),
            max_open_notional_usd: Decimal::from(30),
            entries_enabled: enabled,
            reason: (!enabled).then(|| "manual_disable".to_string()),
        })
    }

    async fn trading_process_live_preflight(
        &self,
        process_id: Uuid,
    ) -> Result<http::TradingProcessLivePreflightResponse, HttpError> {
        Ok(http::TradingProcessLivePreflightResponse {
            process_id,
            account_ref: "polymarket-primary".to_string(),
            credential_connectivity_ready: true,
            reconciliation_ready: true,
            trading_disabled: true,
            ready: true,
            reasons: Vec::new(),
            identity: self.live_identity_diagnostics().await?,
            status: self.live_status().await?,
            reconciliation: Some(ReconciliationReport {
                open_orders: 0,
                balances_checked: true,
                mismatches_found: 0,
                unresolved_count: 0,
                checked_at: Utc::now(),
            }),
            reconciliation_error: None,
            checked_at: Utc::now(),
        })
    }

    async fn set_trading_process_live_entries_enabled(
        &self,
        _process_id: Uuid,
        enabled: bool,
    ) -> Result<LiveVenueStatus, HttpError> {
        self.live_set_entries_enabled(enabled).await
    }

    async fn list_trading_processes(
        &self,
        _request: http::ListTradingProcessesRequest,
    ) -> Result<http::TradingProcessesResponse, HttpError> {
        Ok(http::TradingProcessesResponse {
            processes: vec![test_process(
                Uuid::new_v4(),
                "btc-paper-process".to_string(),
                "btc_5m".to_string(),
                "realtime_paper".to_string(),
                Some("btc-paper-process".to_string()),
                "running",
                true,
                TradingProcessConfig::default(),
            )],
        })
    }

    async fn get_trading_process(
        &self,
        process_id: Uuid,
    ) -> Result<http::TradingProcessResponse, HttpError> {
        Ok(http::TradingProcessResponse {
            process: test_process(
                process_id,
                "btc-paper-canary".to_string(),
                "btc_5m".to_string(),
                "realtime_paper".to_string(),
                Some("btc-paper-canary".to_string()),
                "running",
                true,
                TradingProcessConfig::default(),
            ),
        })
    }

    async fn update_trading_process(
        &self,
        process_id: Uuid,
        request: http::UpdateTradingProcessRequest,
    ) -> Result<http::TradingProcessResponse, HttpError> {
        Ok(http::TradingProcessResponse {
            process: test_process(
                process_id,
                request
                    .name
                    .unwrap_or_else(|| "btc-paper-canary".to_string()),
                "btc_5m".to_string(),
                "realtime_paper".to_string(),
                Some("btc-paper-canary".to_string()),
                "created",
                false,
                request.config.unwrap_or_default(),
            ),
        })
    }

    async fn start_trading_process(
        &self,
        process_id: Uuid,
    ) -> Result<http::TradingProcessResponse, HttpError> {
        Ok(http::TradingProcessResponse {
            process: test_process(
                process_id,
                "btc-paper-canary".to_string(),
                "btc_5m".to_string(),
                "realtime_paper".to_string(),
                Some("btc-paper-canary".to_string()),
                "running",
                true,
                TradingProcessConfig::default(),
            ),
        })
    }

    async fn stop_trading_process(
        &self,
        process_id: Uuid,
    ) -> Result<http::TradingProcessResponse, HttpError> {
        Ok(http::TradingProcessResponse {
            process: test_process(
                process_id,
                "btc-paper-canary".to_string(),
                "btc_5m".to_string(),
                "realtime_paper".to_string(),
                Some("btc-paper-canary".to_string()),
                "stopped",
                false,
                TradingProcessConfig::default(),
            ),
        })
    }

    async fn complete_trading_process(
        &self,
        process_id: Uuid,
    ) -> Result<http::TradingProcessResponse, HttpError> {
        Ok(http::TradingProcessResponse {
            process: test_process(
                process_id,
                "btc-paper-canary".to_string(),
                "btc_5m".to_string(),
                "realtime_paper".to_string(),
                Some("btc-paper-canary".to_string()),
                "completed",
                false,
                TradingProcessConfig::default(),
            ),
        })
    }

    async fn upsert_trading_process_by_key(
        &self,
        process_key: String,
        request: http::UpsertTradingProcessByKeyRequest,
    ) -> Result<http::TradingProcessResponse, HttpError> {
        Ok(http::TradingProcessResponse {
            process: test_process(
                Uuid::new_v4(),
                request.name,
                request.process_type,
                request.process_scope,
                Some(process_key),
                request.status.as_str(),
                request.enabled,
                request.config,
            ),
        })
    }

    async fn get_trading_process_status(
        &self,
        process_id: Uuid,
    ) -> Result<http::TradingProcessStatusResponse, HttpError> {
        Ok(http::TradingProcessStatusResponse {
            process_id,
            status: serde_json::json!({
                "orders": {"total": 1},
                "fills": {"total": 1},
                "btc_runtime": {
                    "active": true,
                    "execution_mode": "live",
                    "live_status": {
                        "entries_enabled": false,
                        "user_ws_enabled": true,
                        "user_ws_connected": true,
                        "last_user_ws_pong_age_secs": 1,
                        "last_rest_reconcile_age_secs": 1,
                        "process_accounting_proven": true,
                        "process_accounting_status": "proven",
                        "idempotency_clean": true,
                        "unresolved_live_order_count": 0,
                        "geoblock_readable": true,
                        "geoblock_blocked": false,
                        "geoblock_country": "PE",
                        "geoblock_region": "LMA",
                        "last_geoblock_check_age_secs": 1,
                        "reason": "manual_enable_required"
                    }
                }
            }),
        })
    }

    async fn preview_trading_process_start(
        &self,
        process_id: Uuid,
    ) -> Result<http::TradingProcessStartPreviewResponse, HttpError> {
        let run_key = "btc-5m-run-preview-test".to_string();
        let run_id = Uuid::new_v5(
            &Uuid::NAMESPACE_URL,
            format!("polymarket-bot/run/{run_key}").as_bytes(),
        );
        Ok(http::TradingProcessStartPreviewResponse {
            process_id,
            run_id,
            run_key,
            preregistration_sha256: "b".repeat(64),
            config_hash: "c".repeat(64),
            frozen_process_config: TradingProcessConfig {
                execution: Some(ProcessExecutionConfig {
                    rail: None,
                    order_policies: None,
                    mode: Some("paper".to_string()),
                    execute_signals: true,
                    live_capital: false,
                    account_ref: None,
                    taker_fee_rate: None,
                    max_order_notional_usd: None,
                    max_open_notional_usd: None,
                    max_open_positions: None,
                    max_daily_loss_usd: None,
                    require_exit_book: None,
                }),
                raw: serde_json::json!({
                    "pipeline_version": "btc_realtime_paper_pipeline_v11",
                    "process_schema_version": "btc_realtime_paper_process_v2",
                    "preregistration_sha256": "b".repeat(64),
                }),
            },
        })
    }
}

#[tokio::test]
async fn health_is_public_and_admin_routes_require_bearer() {
    let app = http::router(Arc::new(FakeControlApi), "secret");

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let process_id = Uuid::new_v4();
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/admin/trading-processes/{process_id}/start-preview"
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/admin/backfill/jobs")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn prometheus_metrics_are_public_text_exposition() {
    let app = http::router(Arc::new(FakeControlApi), "secret");
    let response = app
        .oneshot(
            Request::builder()
                .uri("/prometheus/metrics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get("content-type").unwrap(),
        "text/plain; version=0.0.4; charset=utf-8"
    );
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body = String::from_utf8(body.to_vec()).unwrap();
    assert!(body.contains("polymarket_btc_polygon_oracle_ready 1"));
    assert!(body.contains("# TYPE polymarket_btc_clob_source_to_receive_lag_milliseconds gauge\n"));
    assert!(body.contains("polymarket_btc_clob_source_to_receive_lag_milliseconds 2417\n"));
    assert!(body.contains(
        "# TYPE polymarket_btc_clob_source_to_receive_lag_unavailable_transitions_total counter\n"
    ));
    assert!(body
        .contains("polymarket_btc_clob_source_to_receive_lag_unavailable_transitions_total 9\n"));
}

#[tokio::test]
async fn generic_ingestion_admin_routes_require_bearer() {
    let app = http::router(Arc::new(FakeControlApi), "secret");
    let job_id = Uuid::new_v4();
    let cases = vec![
        ("GET", "/admin/backfill/ingesters".to_string(), "", StatusCode::NOT_FOUND),
        ("GET", "/admin/backfill/jobs".to_string(), "", StatusCode::NOT_FOUND),
        (
            "POST",
            "/admin/backfill/jobs".to_string(),
            r#"{"ingester":"btc_five_minute_markets"}"#,
            StatusCode::NOT_FOUND,
        ),
        ("GET", format!("/admin/backfill/jobs/{job_id}"), "", StatusCode::NOT_FOUND),
        (
            "GET",
            format!("/admin/backfill/jobs/{job_id}/events"),
            "",
            StatusCode::NOT_FOUND,
        ),
        (
            "POST",
            format!("/admin/backfill/jobs/{job_id}/cancel"),
            "",
            StatusCode::NOT_FOUND,
        ),
        (
            "GET",
            "/admin/backfill/readiness/btc-five-minute-training?range_start=2026-01-01T00%3A00%3A00Z&range_end=2026-01-02T00%3A00%3A00Z"
                .to_string(),
            "",
            StatusCode::NOT_FOUND,
        ),
    ];

    for (method, uri, body, expected) in cases {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(&uri)
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), expected, "{uri}");
    }
}

#[tokio::test]
async fn retired_backfill_routes_are_not_found_for_authenticated_admin() {
    let app = http::router(Arc::new(FakeControlApi), "secret");
    for uri in ["/admin/backfill/ingesters", "/admin/backfill/jobs"] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(uri)
                    .header(AUTHORIZATION, "Bearer secret")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{uri}");
    }
}

#[tokio::test]
async fn authenticated_admin_can_read_btc_realtime_status() {
    let app = http::router(Arc::new(FakeControlApi), "secret");
    let response = app
        .oneshot(
            Request::builder()
                .uri("/admin/strategy/btc-5m/readiness")
                .header(AUTHORIZATION, "Bearer secret")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn authenticated_admin_can_read_alertable_btc_entry_status() {
    let process_id = Uuid::new_v4();
    let app = http::router(Arc::new(FakeControlApi), "secret");
    let response = app
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/admin/strategy/btc-5m/entry-status?scope=Selected%20process&process_id={process_id}"
                ))
                .header(AUTHORIZATION, "Bearer secret")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["display"], "Enabled");
    assert_eq!(json["alert_enabled"], 1);
    assert!(json.get("enabled_count").is_none());
    assert!(json.get("total_count").is_none());
    assert!(json["observed_at_epoch_seconds"].as_i64().is_some());
}

#[tokio::test]
async fn authenticated_admin_can_read_all_process_entry_counts_without_changing_alert_status() {
    let app = http::router(Arc::new(FakeControlApi), "secret");
    let response = app
        .oneshot(
            Request::builder()
                .uri("/admin/strategy/btc-5m/entry-status?scope=All%20processes")
                .header(AUTHORIZATION, "Bearer secret")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["enabled_count"], 1);
    assert_eq!(json["total_count"], 2);
    assert_eq!(json["alert_enabled"], 0);
    assert_eq!(json["display"], "Blocked — 1 of 2");
}

#[tokio::test]
async fn retired_btc_paper_experiment_route_is_not_found_for_authenticated_admin() {
    let app = http::router(Arc::new(FakeControlApi), "secret");
    let response = app
        .oneshot(
            Request::builder()
                .uri("/admin/strategy/btc-5m/paper-experiment")
                .header(AUTHORIZATION, "Bearer secret")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn retired_whale_copy_routes_are_not_found() {
    let app = http::router(Arc::new(FakeControlApi), "secret");
    for uri in [
        "/admin/backfill/whales",
        "/admin/pnl/stats",
        "/admin/trades/pnl/summary",
        "/admin/trades/pnl/wallets",
        "/admin/trades/pnl/open-positions",
        "/admin/trades/pnl/mark-health",
        "/admin/trades/pnl/recent-exits",
        "/admin/copy-trade/expectancy-flow/cells",
        "/admin/copy-trade/expectancy-flow/wallet-cells",
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(uri)
                    .header(AUTHORIZATION, "Bearer secret")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{uri}");
    }
}

#[tokio::test]
async fn retired_wallet_analytics_routes_are_not_found() {
    let app = http::router(Arc::new(FakeControlApi), "secret");
    for uri in [
        "/admin/wallets/mrs/recompute",
        "/admin/wallets/scoring/refresh/enqueue",
        "/admin/wallets/scoring/refresh/process",
        "/admin/wallets/scoring/refresh/jobs",
        "/admin/wallets/mrs/segments",
        "/admin/wallets/mrs/segments/recompute",
        "/admin/gamma/taxonomy/status",
        "/admin/gamma/taxonomy/backfill",
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(uri)
                    .header(AUTHORIZATION, "Bearer secret")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{uri}");
    }
}

#[tokio::test]
async fn retired_replay_and_backtest_routes_are_not_found() {
    let app = http::router(Arc::new(FakeControlApi), "secret");
    for uri in [
        "/admin/copy-trade/backtest",
        "/admin/copy-trade/calibration",
        "/admin/copy-trade/replay-existing",
        "/admin/backtests/replay",
        "/admin/backtests",
        "/admin/backtests/00000000-0000-0000-0000-000000000000",
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(uri)
                    .header(AUTHORIZATION, "Bearer secret")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{uri}");
    }
}

#[tokio::test]
async fn retired_legacy_simulator_reset_route_is_not_found() {
    let app = http::router(Arc::new(FakeControlApi), "secret");
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(
                    "/admin/trading-processes/00000000-0000-0000-0000-000000000000/reset-simulation",
                )
                .header(AUTHORIZATION, "Bearer secret")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn authenticated_admin_can_read_live_status_and_halt() {
    let app = http::router(Arc::new(FakeControlApi), "secret");
    let status_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/live/status")
                .header(AUTHORIZATION, "Bearer secret")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(status_response.status(), StatusCode::OK);
    let status_body = to_bytes(status_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let status_json: Value = serde_json::from_slice(&status_body).unwrap();
    assert_eq!(status_json["mode"], "live");
    assert_eq!(status_json["entries_enabled"], false);

    let reconcile_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/admin/live/reconcile")
                .header(AUTHORIZATION, "Bearer secret")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(reconcile_response.status(), StatusCode::OK);

    let diagnostics_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/admin/live/diagnostics")
                .header(AUTHORIZATION, "Bearer secret")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(diagnostics_response.status(), StatusCode::OK);
    let diagnostics_body = to_bytes(diagnostics_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let diagnostics_json: Value = serde_json::from_slice(&diagnostics_body).unwrap();
    assert_eq!(diagnostics_json["credentials_present"], true);
    assert_eq!(diagnostics_json["api_keys_readable"], true);

    let wallet_diagnostics_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/admin/live/wallet-diagnostics?address=0x0000000000000000000000000000000000000005")
                .header(AUTHORIZATION, "Bearer secret")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(wallet_diagnostics_response.status(), StatusCode::OK);
    let wallet_diagnostics_body = to_bytes(wallet_diagnostics_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let wallet_diagnostics_json: Value = serde_json::from_slice(&wallet_diagnostics_body).unwrap();
    assert_eq!(
        wallet_diagnostics_json["configured_funder_deployed_as_deposit_wallet"],
        true
    );
    assert_eq!(wallet_diagnostics_json["signer_balances"]["pusd"], "20");
    assert_eq!(
        wallet_diagnostics_json["candidate_addresses"][0]["address"],
        "0x0000000000000000000000000000000000000005"
    );

    let dry_run_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/admin/live/order-dry-run")
                .header(AUTHORIZATION, "Bearer secret")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "token_id": "123",
                        "side": "buy",
                        "order_type": "fok",
                        "price": "0.39",
                        "size": "5.12"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(dry_run_response.status(), StatusCode::OK);
    let dry_run_body = to_bytes(dry_run_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let dry_run_json: Value = serde_json::from_slice(&dry_run_body).unwrap();
    assert_eq!(
        dry_run_json["order_signer_matches_authenticated_client"],
        false
    );
    assert_eq!(dry_run_json["signature_redacted"], true);

    let funder_probe_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/admin/live/poly1271-funder-probe")
                .header(AUTHORIZATION, "Bearer secret")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "addresses": ["0x0000000000000000000000000000000000000002"],
                        "token_id": "123",
                        "side": "buy",
                        "order_type": "fok",
                        "price": "0.39",
                        "size": "5.12"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(funder_probe_response.status(), StatusCode::OK);
    let funder_probe_body = to_bytes(funder_probe_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let funder_probe_json: Value = serde_json::from_slice(&funder_probe_body).unwrap();
    assert_eq!(funder_probe_json["verified_funder_candidates_count"], 1);
    assert_eq!(
        funder_probe_json["candidates"][0]["maker_matches_candidate"],
        true
    );

    let enable_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/admin/live/entries/enable")
                .header(AUTHORIZATION, "Bearer secret")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(enable_response.status(), StatusCode::OK);

    let halt_response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/admin/live/halt")
                .header(AUTHORIZATION, "Bearer secret")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(halt_response.status(), StatusCode::OK);
}

#[tokio::test]
async fn authenticated_admin_can_preflight_and_control_process_scoped_live_entries() {
    let app = http::router(Arc::new(FakeControlApi), "secret");
    let process_id = Uuid::from_u128(0xeffa3e5e2f5a4f1898ba06e4c0da74ef);
    let preflight_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/admin/trading-processes/{process_id}/live-preflight"
                ))
                .header(AUTHORIZATION, "Bearer secret")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(preflight_response.status(), StatusCode::OK);
    let body = to_bytes(preflight_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let report: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(report["process_id"], process_id.to_string());
    assert_eq!(report["account_ref"], "polymarket-primary");
    assert_eq!(report["ready"], true);

    let enable_response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/admin/trading-processes/{process_id}/live/entries/enable"
                ))
                .header(AUTHORIZATION, "Bearer secret")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(enable_response.status(), StatusCode::OK);
}

#[tokio::test]
async fn authenticated_admin_can_manage_trading_processes() {
    let app = http::router(Arc::new(FakeControlApi), "secret");
    let retired_create_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/admin/trading-processes")
                .header(AUTHORIZATION, "Bearer secret")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"name":"generic-process","process_type":"generic","enabled":true,"config":{}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        retired_create_response.status(),
        StatusCode::METHOD_NOT_ALLOWED
    );
    let process_id = Uuid::new_v4().to_string();

    let list_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/trading-processes?limit=5")
                .header(AUTHORIZATION, "Bearer secret")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(list_response.status(), StatusCode::OK);
    let list_body = to_bytes(list_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let list_json: Value = serde_json::from_slice(&list_body).unwrap();
    assert_eq!(list_json["processes"][0]["status"], "running");

    let upsert_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/admin/trading-processes/by-key/btc-paper-canary")
                .header(AUTHORIZATION, "Bearer secret")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"name":"btc-paper-canary","process_type":"btc_5m","process_scope":"realtime_paper","enabled":false,"status":"created","config":{}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(upsert_response.status(), StatusCode::OK);
    let upsert_body = to_bytes(upsert_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let upsert_json: Value = serde_json::from_slice(&upsert_body).unwrap();
    assert_eq!(upsert_json["process"]["process_key"], "btc-paper-canary");
    assert_eq!(upsert_json["process"]["process_scope"], "realtime_paper");

    let status_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/admin/trading-processes/{process_id}/status"))
                .header(AUTHORIZATION, "Bearer secret")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(status_response.status(), StatusCode::OK);
    let status_body = to_bytes(status_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let status_json: Value = serde_json::from_slice(&status_body).unwrap();
    assert_eq!(
        status_json["status"]["btc_runtime"]["execution_mode"],
        "live"
    );
    assert_eq!(
        status_json["status"]["btc_runtime"]["live_status"]["entries_enabled"],
        false
    );
    assert_eq!(
        status_json["status"]["btc_runtime"]["live_status"]["user_ws_connected"],
        true
    );
    assert_eq!(
        status_json["status"]["btc_runtime"]["live_status"]["process_accounting_status"],
        "proven"
    );
    assert_eq!(
        status_json["status"]["btc_runtime"]["live_status"]["geoblock_blocked"],
        false
    );

    let update_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!("/admin/trading-processes/{process_id}"))
                .header(AUTHORIZATION, "Bearer secret")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"name":"btc-paper-canary-updated"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(update_response.status(), StatusCode::OK);

    let lifecycle_update_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!("/admin/trading-processes/{process_id}"))
                .header(AUTHORIZATION, "Bearer secret")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"status":"running"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        lifecycle_update_response.status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );

    let preview_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/admin/trading-processes/{process_id}/start-preview"
                ))
                .header(AUTHORIZATION, "Bearer secret")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(preview_response.status(), StatusCode::OK);
    let preview_body = to_bytes(preview_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let preview_json: Value = serde_json::from_slice(&preview_body).unwrap();
    assert_eq!(preview_json["process_id"], process_id);
    assert!(preview_json["run_id"].is_string());
    assert_eq!(preview_json["run_key"], "btc-5m-run-preview-test");
    assert!(preview_json.get("experiment_id").is_none());
    assert!(preview_json.get("experiment_key").is_none());
    assert_eq!(
        preview_json["preregistration_sha256"]
            .as_str()
            .unwrap()
            .len(),
        64
    );
    assert_eq!(preview_json["config_hash"].as_str().unwrap().len(), 64);
    assert_eq!(
        preview_json["frozen_process_config"]["execution"]["mode"],
        "paper"
    );

    let start_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/admin/trading-processes/{process_id}/start"))
                .header(AUTHORIZATION, "Bearer secret")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(start_response.status(), StatusCode::OK);

    let stop_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/admin/trading-processes/{process_id}/stop"))
                .header(AUTHORIZATION, "Bearer secret")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(stop_response.status(), StatusCode::OK);

    let complete_response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/admin/trading-processes/{process_id}/complete"))
                .header(AUTHORIZATION, "Bearer secret")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(complete_response.status(), StatusCode::OK);
    let complete_body = to_bytes(complete_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let complete_json: Value = serde_json::from_slice(&complete_body).unwrap();
    assert_eq!(complete_json["process"]["status"], "completed");
    assert_eq!(complete_json["process"]["enabled"], false);
}

#[allow(clippy::too_many_arguments)]
fn test_process(
    process_id: Uuid,
    name: String,
    process_type: String,
    process_scope: String,
    process_key: Option<String>,
    status: &str,
    enabled: bool,
    config: TradingProcessConfig,
) -> TradingProcess {
    TradingProcess {
        process_id,
        name,
        process_type,
        process_scope,
        process_key,
        status: status.to_string(),
        enabled,
        config,
        metadata: serde_json::json!({}),
        created_at: Utc::now(),
        updated_at: Utc::now(),
        started_at: None,
        stopped_at: None,
        last_error: None,
    }
}

// An independently registered application proves that the HTTP layer does not select an adapter.
struct FixtureOrders;
#[async_trait]
impl polymarket_bot::execution::orders::OrderApplication for FixtureOrders {
    async fn preview(
        &self,
        id: Uuid,
        request: polymarket_bot::execution::orders::EvaluationRequest,
        prepare: bool,
    ) -> Result<polymarket_bot::execution::orders::Receipt, HttpError> {
        match request.candidate.market_id.as_str() {
            "missing" => return Err(HttpError::not_found("missing")),
            "unsupported" => return Err(HttpError::bad_request("unsupported")),
            "unavailable" => return Err(HttpError::unavailable("unavailable")),
            "unimplemented" => return Err(HttpError::not_implemented("unimplemented")),
            _ => {}
        }
        Ok(polymarket_bot::execution::orders::Receipt {
            version: 1,
            process_id: id,
            operation_id: Uuid::from_u128(41),
            order_id: None,
            origin: "api".into(),
            config_sha256: "fixture".into(),
            status: if request.candidate.market_id == "reject" {
                "rejected"
            } else {
                "evaluated"
            }
            .into(),
            policy_results: serde_json::json!([]),
            evidence: serde_json::json!({"economic_effects":false}),
            preparation: prepare.then(|| serde_json::json!({"status":"unavailable"})),
        })
    }
    async fn execute(
        &self,
        id: Uuid,
        request: polymarket_bot::execution::orders::ExecuteRequest,
    ) -> Result<polymarket_bot::execution::orders::Receipt, HttpError> {
        if request.candidate.market_id == "conflict" {
            return Err(HttpError::conflict("conflict"));
        }
        let mut receipt = self
            .preview(
                id,
                polymarket_bot::execution::orders::EvaluationRequest {
                    version: request.version,
                    candidate: request.candidate,
                },
                false,
            )
            .await?;
        receipt.operation_id = request.idempotency_key;
        receipt.status = "acknowledged".into();
        receipt.order_id = Some("fixture-order".into());
        Ok(receipt)
    }
    async fn order(
        &self,
        id: Uuid,
        order_id: String,
    ) -> Result<polymarket_bot::execution::orders::Receipt, HttpError> {
        if id != Uuid::from_u128(1) || order_id != "fixture-order" {
            return Err(HttpError::not_found("order not found"));
        }
        let input = order_candidate("market");
        self.preview(id, serde_json::from_value(input).unwrap(), false)
            .await
    }
}
fn order_candidate(market: &str) -> Value {
    serde_json::json!({"version":1,"candidate":{"member_id":"member","market_id":market,"outcome":"up","synthetic_prediction":{"probability_up":"0.80"}}})
}
async fn order_http(method: &str, path: &str, body: Value, auth: bool) -> (StatusCode, Value) {
    let app = http::router(Arc::new(FakeControlApi), "admin-secret");
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json");
    if auth {
        request = request.header(AUTHORIZATION, "Bearer admin-secret");
    }
    let response = app
        .oneshot(request.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}
#[tokio::test]
async fn generic_order_routes_authenticate_and_delegate_without_adapter_names() {
    let id = Uuid::from_u128(1);
    for route in ["preview", "dry-run", "execute"] {
        let mut body = order_candidate("market");
        if route == "execute" {
            body["idempotency_key"] = serde_json::json!(Uuid::new_v4());
        }
        let path = format!("/orders/{id}/{route}");
        assert_eq!(
            order_http("POST", &path, body.clone(), false).await.0,
            StatusCode::UNAUTHORIZED
        );
        let (status, response) = order_http("POST", &path, body, true).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(response["process_id"], id.to_string());
        if route == "execute" {
            assert_eq!(response["status"], "acknowledged");
        } else {
            assert!(response["order_id"].is_null());
            assert_eq!(response["evidence"]["economic_effects"], false);
        }
        if route == "dry-run" {
            assert_eq!(response["preparation"]["status"], "unavailable");
        }
    }
}
#[tokio::test]
async fn generic_order_routes_reject_overrides_and_malformed_contracts() {
    let path = format!("/orders/{}/preview", Uuid::new_v4());
    for field in ["quantity", "price", "mode", "account_ref", "rail", "policy"] {
        let mut body = order_candidate("market");
        body["candidate"][field] = serde_json::json!("override");
        assert_eq!(
            order_http("POST", &path, body, true).await.0,
            StatusCode::BAD_REQUEST
        );
    }
    for version in [0, 2] {
        let mut body = order_candidate("market");
        body["version"] = serde_json::json!(version);
        assert_eq!(
            order_http("POST", &path, body, true).await.0,
            StatusCode::BAD_REQUEST
        );
    }
    let mut body = order_candidate("market");
    body["candidate"]["synthetic_prediction"]["probability_up"] = serde_json::json!("1.1");
    assert_eq!(
        order_http("POST", &path, body, true).await.0,
        StatusCode::BAD_REQUEST
    );
    let path = path.replace("preview", "execute");
    assert_eq!(
        order_http("POST", &path, order_candidate("market"), true)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
}
#[tokio::test]
async fn generic_order_routes_preserve_application_error_and_ownership_semantics() {
    let path = format!("/orders/{}/preview", Uuid::from_u128(1));
    for (market, status) in [
        ("missing", StatusCode::NOT_FOUND),
        ("unsupported", StatusCode::BAD_REQUEST),
        ("unavailable", StatusCode::SERVICE_UNAVAILABLE),
        ("unimplemented", StatusCode::NOT_IMPLEMENTED),
        ("reject", StatusCode::OK),
    ] {
        assert_eq!(
            order_http("POST", &path, order_candidate(market), true)
                .await
                .0,
            status
        );
    }
    let mut body = order_candidate("conflict");
    body["idempotency_key"] = serde_json::json!(Uuid::new_v4());
    assert_eq!(
        order_http("POST", &path.replace("preview", "execute"), body, true)
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        order_http(
            "GET",
            &format!("/orders/{}/order/fixture-order", Uuid::from_u128(2)),
            Value::Null,
            true
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        order_http(
            "GET",
            &format!("/orders/{}/order/fixture-order", Uuid::from_u128(1)),
            Value::Null,
            true
        )
        .await
        .0,
        StatusCode::OK
    );
}
