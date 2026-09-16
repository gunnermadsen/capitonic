//! Stream-only Kraken spot trades. No market-data rows or durable trade checkpoint.
use super::spot_support::KrakenSpotTrade;
use rust_decimal::Decimal;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tokio::{
    sync::{mpsc, watch},
    task::JoinSet,
};
use tokio_tungstenite::{
    connect_async_with_config,
    tungstenite::{protocol::WebSocketConfig, Message},
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{
    domain::{
        IngesterProfile, IngesterStrategyKey, RealtimeWorkerStrategy, StrategyError,
        StrategyErrorKind,
    },
    persistence::ProfileRepository,
    runtime::{StrategyFactory, StrategyFactoryError},
    streaming,
};

const PRODUCT: &str = "kraken_spot_btcusd_trades";
const ENDPOINT: &str = "wss://ws.kraken.com/v2";
const FRAME_CAPACITY: usize = 1024;

pub struct KrakenSpotTradesFactory;

impl StrategyFactory for KrakenSpotTradesFactory {
    fn key(&self) -> IngesterStrategyKey {
        IngesterStrategyKey::KrakenSpotBtcusdTrades
    }
    fn config_schema_version(&self) -> i32 {
        1
    }
    fn validate_config(&self, config: &Value) -> Result<(), StrategyFactoryError> {
        if config.as_object().is_some_and(|value| value.is_empty()) {
            Ok(())
        } else {
            Err(StrategyFactoryError::InvalidConfiguration(
                "Kraken BTC/USD trade stream accepts an empty configuration object".into(),
            ))
        }
    }
    fn build(
        &self,
        profile: &IngesterProfile,
        pool: PgPool,
    ) -> Result<Box<dyn RealtimeWorkerStrategy>, StrategyFactoryError> {
        if profile.strategy_key != self.key()
            || profile.config_schema_version != self.config_schema_version()
        {
            return Err(StrategyFactoryError::Construction(
                "Kraken profile identity or schema mismatch".into(),
            ));
        }
        self.validate_config(&profile.config)?;
        Ok(Box::new(KrakenSpotTrades {
            profiles: ProfileRepository::new(pool),
            owner: profile.lease_owner.clone().ok_or_else(|| {
                StrategyFactoryError::Construction("missing Kraken lease owner".into())
            })?,
            token: profile.lease_token.ok_or_else(|| {
                StrategyFactoryError::Construction("missing Kraken lease token".into())
            })?,
            generation: profile.desired_generation,
        }))
    }
}

struct SourceReadinessGuard;
impl Drop for SourceReadinessGuard {
    fn drop(&mut self) {
        streaming::set_source_connection_ready(PRODUCT, false);
    }
}

struct KrakenSpotTrades {
    profiles: ProfileRepository,
    owner: String,
    token: Uuid,
    generation: i64,
}

#[derive(Debug, Deserialize, Serialize)]
struct Trade {
    symbol: String,
    trade_id: u64,
    timestamp: DateTime<Utc>,
    price: Decimal,
    qty: Decimal,
    ord_type: String,
    side: String,
}

#[derive(Serialize)]
struct TradeBatch {
    /// A connection reset invalidates the consumer's rolling history.
    source_epoch: Uuid,
    trades: Vec<KrakenSpotTrade>,
}

fn source(code: &'static str, message: impl Into<String>) -> StrategyError {
    StrategyError::new(StrategyErrorKind::TransientSource, code, message)
}

fn decode(bytes: &[u8]) -> Result<Vec<Trade>, StrategyError> {
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|error| source("kraken_invalid_json", error.to_string()))?;
    if value.get("success") == Some(&Value::Bool(false)) {
        return Err(source("kraken_subscription_rejected", value.to_string()));
    }
    if value.get("channel").and_then(Value::as_str) != Some("trade") {
        return Ok(Vec::new());
    }
    // A last-50 snapshot is not a complete history window; never use it as one.
    if value.get("type").and_then(Value::as_str) != Some("update") {
        return Ok(Vec::new());
    }
    let trades: Vec<Trade> =
        serde_json::from_value(value.get("data").cloned().unwrap_or(Value::Null))
            .map_err(|error| source("kraken_invalid_trade", error.to_string()))?;
    for trade in &trades {
        if trade.symbol != "BTC/USD"
            || trade.price <= Decimal::ZERO
            || trade.qty <= Decimal::ZERO
            || trade.trade_id > i64::MAX as u64
            || trade.timestamp.timestamp_nanos_opt().is_none()
            || !matches!(trade.side.as_str(), "buy" | "sell")
        {
            return Err(source(
                "kraken_invalid_trade",
                "trade has invalid symbol, price, quantity or side",
            ));
        }
    }
    Ok(trades)
}

#[async_trait]
impl RealtimeWorkerStrategy for KrakenSpotTrades {
    fn key(&self) -> IngesterStrategyKey {
        IngesterStrategyKey::KrakenSpotBtcusdTrades
    }
    async fn run(&self, shutdown: CancellationToken) -> Result<(), StrategyError> {
        let _readiness = SourceReadinessGuard;
        let (progress, observed) = watch::channel(None);
        let capture = self.capture(shutdown.clone(), progress);
        tokio::pin!(capture);
        let mut report = tokio::time::interval(Duration::from_secs(5));
        loop {
            tokio::select! {
                result = &mut capture => return result,
                _ = report.tick() => {
                    let source_at = *observed.borrow();
                    let current = self.profiles.record_stream_progress(self.key(), &self.owner, self.token, self.generation, source_at)
                        .await.map_err(|error| StrategyError::new(StrategyErrorKind::TransientDatabase, "kraken_health_report_failed", error.to_string()))?;
                    if !current { return Err(StrategyError::new(StrategyErrorKind::LeaseLost, "kraken_lease_lost", "stream profile lease no longer current")); }
                }
            }
        }
    }
}

impl KrakenSpotTrades {
    async fn capture(
        &self,
        shutdown: CancellationToken,
        progress: watch::Sender<Option<DateTime<Utc>>>,
    ) -> Result<(), StrategyError> {
        loop {
            streaming::observe_source_connection_attempt(PRODUCT, ENDPOINT);
            let result = tokio::select! {
                _ = shutdown.cancelled() => { streaming::set_source_connection_ready(PRODUCT, false); return Ok(()); },
                result = capture_session(&progress) => result,
            };
            progress.send_replace(None);
            streaming::set_source_connection_ready(PRODUCT, false);
            if let Err(error) = result {
                if error.code == "kraken_frame_overflow" {
                    streaming::observe_websocket_queue_overflow(PRODUCT);
                }
                streaming::observe_source_connection_failure(PRODUCT, ENDPOINT, error.code);
                streaming::observe_source_reconnect(PRODUCT, error.code);
                tracing::warn!(product_key=PRODUCT, code=error.code, %error, "Kraken stream reconnecting; consumers must warm a new history window");
            }
            tokio::select! {
                _ = shutdown.cancelled() => { streaming::set_source_connection_ready(PRODUCT, false); return Ok(()); },
                _ = tokio::time::sleep(Duration::from_secs(2)) => {}
            }
        }
    }
}

async fn capture_session(
    progress: &watch::Sender<Option<DateTime<Utc>>>,
) -> Result<(), StrategyError> {
    let config = WebSocketConfig::default()
        .max_message_size(Some(1024 * 1024))
        .max_frame_size(Some(1024 * 1024));
    let (socket, _) = tokio::time::timeout(
        Duration::from_secs(10),
        connect_async_with_config(ENDPOINT, Some(config), true),
    )
    .await
    .map_err(|_| source("kraken_connect_timeout", "connection timed out"))?
    .map_err(|error| source("kraken_connect_failed", error.to_string()))?;
    let (mut sink, mut stream) = socket.split();
    let (commands, mut writes) = mpsc::channel::<Message>(16);
    let (frames, mut reads) = mpsc::channel::<Message>(FRAME_CAPACITY);
    let mut tasks = JoinSet::new();
    commands.try_send(Message::Text(json!({"method":"subscribe","params":{"channel":"trade","symbol":["BTC/USD"],"snapshot":false}}).to_string().into()))
        .map_err(|_| source("kraken_writer_unavailable", "subscription queue unavailable"))?;
    tasks.spawn(async move {
        while let Some(message) = writes.recv().await {
            tokio::time::timeout(Duration::from_secs(5), sink.send(message))
                .await
                .map_err(|_| source("kraken_write_timeout", "control write timed out"))?
                .map_err(|error| source("kraken_write_failed", error.to_string()))?;
        }
        Ok::<_, StrategyError>(())
    });
    tasks.spawn(async move {
        while let Some(frame) = stream.next().await {
            match frame {
                Ok(message @ (Message::Text(_) | Message::Binary(_))) => {
                    frames.try_send(message).map_err(|_| {
                        source(
                            "kraken_frame_overflow",
                            "bounded frame queue unavailable; history continuity lost",
                        )
                    })?;
                }
                Ok(Message::Ping(payload)) => {
                    commands.try_send(Message::Pong(payload)).map_err(|_| {
                        source(
                            "kraken_control_overflow",
                            "bounded control queue unavailable",
                        )
                    })?;
                }
                Ok(Message::Close(_)) => {
                    return Err(source("kraken_closed", "provider closed connection"))
                }
                Ok(_) => {}
                Err(error) => return Err(source("kraken_read_failed", error.to_string())),
            }
        }
        Err(source("kraken_eof", "provider stream ended"))
    });
    // JoinSet aborts both socket tasks on cancellation, error, overflow or timeout.
    let epoch = Uuid::new_v4();
    loop {
        let frame = tokio::select! {
            result = tasks.join_next() => return match result {
                Some(Ok(Err(error))) => Err(error),
                _ => Err(source("kraken_io_stopped", "socket task stopped")),
            },
            frame = tokio::time::timeout(Duration::from_secs(30), reads.recv()) => frame
                .map_err(|_| source("kraken_stale_socket", "no provider frames for 30 seconds"))?
                .ok_or_else(|| source("kraken_queue_closed", "frame queue closed"))?,
        };
        let received_at = Utc::now();
        let bytes = frame.into_data();
        streaming::observe_websocket_frame(PRODUCT, bytes.len(), None);
        streaming::set_websocket_queue_depth(PRODUCT, reads.len(), FRAME_CAPACITY);
        let trades = decode(&bytes)?;
        streaming::observe_websocket_frame_processed(PRODUCT);
        if trades.is_empty() {
            continue;
        }
        let source_at = trades
            .iter()
            .map(|trade| trade.timestamp)
            .max()
            .expect("nonempty batch");
        let event_id = format!(
            "{}:{}",
            epoch,
            trades.last().expect("nonempty batch").trade_id
        );
        let checksum = hex::encode(Sha256::digest(&bytes));
        progress.send_replace(Some(source_at));
        streaming::observe_source_event(PRODUCT, source_at);
        streaming::set_source_connection_ready(PRODUCT, true);
        streaming::publish(
            PRODUCT,
            event_id,
            source_at,
            received_at,
            received_at,
            checksum,
            false,
            &TradeBatch {
                source_epoch: epoch,
                trades: trades
                    .into_iter()
                    .map(|trade| KrakenSpotTrade {
                        trade_id: trade.trade_id as i64,
                        timestamp_ns: trade
                            .timestamp
                            .timestamp_nanos_opt()
                            .expect("validated timestamp"),
                        price: trade.price,
                        base_volume: trade.qty,
                        side: trade.side,
                        order_type: trade.ord_type,
                    })
                    .collect(),
            },
        )
        .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_provider_trade_and_does_not_treat_snapshot_as_history() {
        let update = br#"{"channel":"trade","type":"update","data":[{"symbol":"BTC/USD","trade_id":42,"timestamp":"2026-09-16T00:00:00.123456Z","price":60000.5,"qty":0.25,"side":"sell","ord_type":"market"}]}"#;
        let trades = decode(update).unwrap();
        assert_eq!(trades.len(), 1);
        assert_eq!(trades[0].trade_id, 42);
        assert_eq!(trades[0].timestamp.timestamp_subsec_micros(), 123456);
        assert!(decode(
            String::from_utf8(update.to_vec())
                .unwrap()
                .replace("update", "snapshot")
                .as_bytes()
        )
        .unwrap()
        .is_empty());
        assert!(decode(
            String::from_utf8(update.to_vec())
                .unwrap()
                .replace("sell", "invalid")
                .as_bytes()
        )
        .is_err());
        assert!(decode(br#"{"method":"subscribe","success":false,"error":"rejected"}"#).is_err());
    }
}
