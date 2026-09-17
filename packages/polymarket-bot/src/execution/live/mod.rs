use std::{
    collections::{HashMap, HashSet},
    str::FromStr,
    sync::Arc,
    time::Duration,
};

use alloy_signer_local::PrivateKeySigner;
use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use base64::{engine::general_purpose::URL_SAFE, Engine as _};
use chrono::{DateTime, Utc};
use futures_util::{SinkExt, StreamExt};
use hmac::{Hmac, Mac as _};
use polymarket_client_sdk_v2::{
    auth::{state::Authenticated, Credentials, LocalSigner, Normal, Signer as _},
    clob::{
        types::{
            request::{
                BalanceAllowanceRequest, OrdersRequest, TradesRequest,
                UpdateBalanceAllowanceRequest,
            },
            response::{OpenOrderResponse, Page, PostOrderResponse, TradeResponse},
            AssetType, OrderStatusType, OrderType as SdkOrderType, Side as SdkSide, SignatureType,
            TradeStatusType,
        },
        Client as SdkClient, Config as SdkConfig,
    },
    derive_proxy_wallet, derive_safe_wallet,
    error::{Kind as SdkErrorKind, Status as SdkStatus},
    types::{Address, Decimal as SdkDecimal, U256},
    POLYGON,
};
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::Sha256;
use tokio::sync::{Mutex, OnceCell};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{debug, warn};
use uuid::Uuid;

use crate::{
    account_reconcile::{
        account_trade_from_live_event, reconcile_account_positions, AccountReconcileReport,
        AccountReconcileRequest,
    },
    config::LiveExecutionConfig,
    data_api::DataApiClient,
    execution::{
        live_execution_gate_closed_order, ExecutionVenue, LiveExecutionGateReason,
        LiveIdentityDiagnostics, LiveOrderDryRunDiagnostics, LiveOrderDryRunRequest,
        LivePoly1271FunderProbeCandidate, LivePoly1271FunderProbeRequest,
        LivePoly1271FunderProbeResponse, LivePrePostGuard, LiveVenueStatus,
        LiveWalletAddressDiagnostics, LiveWalletCandidateAddressDiagnostics,
        LiveWalletTokenBalances, ReconciliationReport, LIVE_FILL_RECONCILIATION_SKEW,
    },
    fees::{dynamic_crypto_taker_fee, sealed_dynamic_fee_rate},
    idempotency::event_hash,
    models::{EffectiveProcessExecutionConfig, FillRecord, OrderRecord, OrderRequest},
    models::{FillSource, OrderSide, OrderState, OrderType},
    store::{validate_live_cumulative_fill_economics, Store},
};

mod diagnostics;
mod execution_venue;
mod fill_reconciliation;
mod normalization;
mod reconciliation;
mod user_stream;
mod venue;

#[cfg(test)]
mod tests;

use diagnostics::*;
use fill_reconciliation::*;
use normalization::*;
use reconciliation::*;
use user_stream::*;

type AuthenticatedClient = SdkClient<Authenticated<Normal>>;

const DEFAULT_POLYGON_RPC_URL: &str = "https://polygon-bor-rpc.publicnode.com";
const DEFAULT_RELAYER_BASE_URL: &str = "https://relayer-v2.polymarket.com";
const PUSD_ADDRESS: &str = "0xC011a7E12a19f7B1f670d46F03B03f3342E82DFB";
const CTF_EXCHANGE_V2_ADDRESS: &str = "0xE111180000d2663C0091e4f400237545B87B996B";
const NEG_RISK_CTF_EXCHANGE_V2_ADDRESS: &str = "0xe2222d279d744050d28e00520010520000310F59";
const USDC_E_ADDRESS: &str = "0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174";
const NATIVE_USDC_ADDRESS: &str = "0x3c499c542cef5e3811e1192ce70d8cc03d5c3359";
const MAX_PROCESS_NONTERMINAL_ORDERS: usize = 256;
const CLOB_TERMINAL_CURSOR: &str = "LTE=";
const MAX_CLOB_RECONCILIATION_PAGES: usize = 32;
const MAX_CLOB_RECONCILIATION_ROWS: usize = 4_096;
const MAX_CLOB_RECONCILIATION_ORDER_IDS: usize = 8_192;
const MAX_CLOB_CURSOR_BYTES: usize = 256;
const CLOB_ORDER_ID_QUERY_CHUNK: usize = 500;
const USER_WS_MAX_TRANSPORT_OPERATION_TIMEOUT: Duration = Duration::from_secs(10);
const USER_WS_RECONNECT_INITIAL_DELAY: Duration = Duration::from_secs(1);
const USER_WS_RECONNECT_MAX_DELAY: Duration = Duration::from_secs(30);

#[derive(Debug, Deserialize)]
struct RpcResponse {
    result: Option<String>,
    error: Option<Value>,
}

#[derive(Clone)]
pub struct LiveVenue {
    config: LiveExecutionConfig,
    clob_base_url: String,
    http_client: reqwest::Client,
    signer: Option<Arc<PrivateKeySigner>>,
    authenticated_client_cache: Arc<OnceCell<AuthenticatedClient>>,
    store: Option<Store>,
    data_api: Option<DataApiClient>,
    bound_process_id: Option<Uuid>,
    bound_account_ref: Option<String>,
    bound_execution: Option<EffectiveProcessExecutionConfig>,
    transport_state: Arc<Mutex<LiveTransportState>>,
    readiness_state: Arc<Mutex<LiveVenueState>>,
    global_entry_gate: Arc<Mutex<GlobalLiveEntryGate>>,
    submit_guard: Arc<Mutex<()>>,
    reconcile_guard: Arc<Mutex<()>>,
}

#[derive(Debug, Clone)]
struct LiveTransportState {
    user_ws_connected: bool,
    last_user_ws_pong_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
struct LiveVenueState {
    last_rest_reconcile_at: Option<DateTime<Utc>>,
    idempotency_clean: bool,
    unresolved_live_order_count: usize,
    manual_entries_enabled: bool,
    manual_entries_reason: Option<String>,
    process_accounting_proven: bool,
    process_accounting_entry_safe: bool,
    process_accounting_status: String,
    credential_account_fingerprint_sha256: Option<String>,
    reconciled_safety_generation: Option<u64>,
    pending_settlement_count: usize,
    reconciliation_error: Option<String>,
}

#[derive(Debug, Clone)]
struct CanonicalLiveAccountIdentity {
    account_address: String,
    signer_address: String,
    signature_type: SignatureType,
    fingerprint_sha256: String,
}

#[derive(Debug, Clone)]
struct GlobalLiveEntryGate {
    halted: bool,
    reason: String,
    safety_generation: u64,
}

impl LiveTransportState {
    fn initial() -> Self {
        Self {
            user_ws_connected: false,
            last_user_ws_pong_at: None,
        }
    }
}

impl LiveVenueState {
    fn fail_closed() -> Self {
        Self {
            last_rest_reconcile_at: None,
            idempotency_clean: false,
            unresolved_live_order_count: 0,
            manual_entries_enabled: false,
            manual_entries_reason: Some("manual_enable_required".to_string()),
            process_accounting_proven: false,
            process_accounting_entry_safe: false,
            process_accounting_status: "unproven".to_string(),
            credential_account_fingerprint_sha256: None,
            reconciled_safety_generation: None,
            pending_settlement_count: 0,
            reconciliation_error: None,
        }
    }
}

impl GlobalLiveEntryGate {
    fn fail_closed() -> Self {
        Self {
            halted: true,
            reason: "global_enable_required".to_string(),
            safety_generation: 0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LiveVenueEvent {
    pub source: String,
    pub event_type: String,
    pub venue_event_id: Option<String>,
    pub venue_order_id: Option<String>,
    pub venue_trade_id: Option<String>,
    pub event_status: Option<String>,
    pub raw_payload: serde_json::Value,
}

impl LiveVenueEvent {
    pub fn hash(&self) -> String {
        event_hash(&json!({
            "source": self.source,
            "event_type": self.event_type,
            "venue_event_id": self.venue_event_id,
            "venue_order_id": self.venue_order_id,
            "venue_trade_id": self.venue_trade_id,
            "event_status": self.event_status,
            "raw_payload": self.raw_payload
        }))
    }
}
