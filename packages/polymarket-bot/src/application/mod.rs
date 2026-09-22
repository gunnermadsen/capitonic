use polymarket_bot::btc::unified_model_runtime::router::{
    RouterDefinition, PROCESS_SCHEMA_VERSION as ROUTER_PROCESS_SCHEMA_VERSION,
};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use chrono::Utc;
use polymarket_bot::btc::unified_model_runtime::risk::{
    self as risk_runtime, RiskStrategySelection,
};
use polymarket_bot::grafana_live::MarketPathPublicationState;
use polymarket_bot::{
    btc::{
        process_runtime_readiness, runtime_model, runtime_status_from_inputs, BookRegistry,
        BtcDecisionStrategyConfig, BtcDirectionalModelEntryPolicy, BtcEntryAdmissionConfig,
        BtcExecutionLifecycle, BtcExecutionMode, BtcLiveExecutionAdapter, BtcPlaybookRuntimeHandle,
        BtcProcessConfig, BtcProcessRunner, BtcRepository, BtcRuntime, BtcRuntimeConfig,
        BtcRuntimeHandle, BtcStrategyConfig, LiveExecutionLifecycle, PaperExecutionLifecycle,
        PaperPreviewConfig, PaperVenue as BtcPaperVenue, PaperVenueConfig, RuntimeModelSelection,
    },
    config::AppConfig,
    data_api::DataApiClient,
    events::ServiceEvent,
    execution::{
        live::LiveVenue, ExecutionVenue, LiveIdentityDiagnostics, LiveOrderDryRunDiagnostics,
        LiveOrderDryRunRequest, LivePoly1271FunderProbeRequest, LivePoly1271FunderProbeResponse,
        LiveVenueStatus, LiveWalletAddressDiagnostics,
    },
    grafana_live::{
        CountdownSnapshot, EntryPermission, GrafanaLivePublisher, ProcessEntryPermission,
        TradingEntryStatusSnapshot,
    },
    http as control_http,
    http::{
        ControlApi, EntryStatusRequest, HealthResponse, HealthStatus, HttpError, MetricsResponse,
        TradingProcessLivePreflightResponse, TradingProcessResponse,
        TradingProcessStartPreviewResponse, TradingProcessStatusResponse, TradingProcessesResponse,
    },
    market_data_stream::SourceSelector,
    models::{
        EffectiveProcessExecutionConfig, ProcessExecutionConfig, TradingProcess,
        TradingProcessConfig,
    },
    store::Store,
};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use sqlx::{postgres::PgPoolOptions, PgPool};
use tokio::time::MissedTickBehavior;
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

#[cfg(test)]
use polymarket_bot::btc::{
    BTC_ASYMMETRIC_VALUE_MODEL_STRATEGY_VERSION, BTC_DIRECTIONAL_MODEL_STRATEGY_VERSION,
};

mod control_api;
mod grafana_publisher;
mod process_definition;
mod process_lifecycle;
mod process_manager;
mod process_status;
mod service;
mod shared_runtime;

#[cfg(test)]
mod tests;

use control_api::*;
use grafana_publisher::*;
use process_definition::*;
use process_manager::*;

pub(crate) use service::run;
