use std::path::PathBuf;

use async_trait::async_trait;
use futures_util::TryStreamExt;
use tokio::fs;

use super::{
    binance_schema::{self, TradeRow},
    common::{
        create_object, db_error, finish_writer, invalid, io_error, publish_file, start_writer,
        Chunk, Publication,
    },
    retained::{self, RetainedDrainAdapter, RetainedDrainSpec},
};
use crate::domain::{
    DrainContext, DrainDescriptor, DrainExecutionError, DrainOutcome, DrainRequest,
    DrainWorkerStrategy,
};

const KEY: &str = "binance_spot_btcusdt_aggregate_trades";
const RELATION: &str = "market_data.binance_spot_btcusdt_aggregate_trades";
const BATCH_ROWS: usize = 50_000;
const SPEC: RetainedDrainSpec = RetainedDrainSpec {
    key: KEY,
    relation: RELATION,
    schema: "market_data",
    table: "binance_spot_btcusdt_aggregate_trades",
    time_column: "trade_timestamp",
    retention_days: None,
};

pub struct BinanceAggregateTradesDrain {
    descriptor: DrainDescriptor,
    root: PathBuf,
}

impl BinanceAggregateTradesDrain {
    pub fn from_environment() -> Result<Self, DrainExecutionError> {
        let root = PathBuf::from(
            std::env::var("INGESTER_BINANCE_AGG_TRADE_LAKE_ROOT")
                .unwrap_or_else(|_| "/var/lib/binance-aggregate-trades".into()),
        );
        if !root.is_absolute() {
            return Err(invalid(
                "drain_root_invalid",
                "Binance aggregate-trade lake root must be absolute",
            ));
        }
        Ok(Self {
            descriptor: retained::descriptor(&SPEC),
            root,
        })
    }
}

#[async_trait]
impl DrainWorkerStrategy for BinanceAggregateTradesDrain {
    fn descriptor(&self) -> &DrainDescriptor {
        &self.descriptor
    }

    fn validate_request(&self, request: &DrainRequest) -> Result<(), DrainExecutionError> {
        retained::validate_strategy(self, request)
    }

    async fn execute_drain(
        &self,
        context: DrainContext,
        request: DrainRequest,
    ) -> Result<DrainOutcome, DrainExecutionError> {
        retained::execute_strategy(self, context, request).await
    }
}

#[async_trait]
impl RetainedDrainAdapter for BinanceAggregateTradesDrain {
    fn spec(&self) -> &RetainedDrainSpec {
        &SPEC
    }

    fn root(&self) -> &std::path::Path {
        &self.root
    }

    async fn source_count(
        &self,
        context: &DrainContext,
        chunk: &Chunk,
    ) -> Result<Option<i64>, DrainExecutionError> {
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM market_data.binance_spot_btcusdt_aggregate_trades WHERE trade_timestamp >= $1 AND trade_timestamp < $2",
        )
        .bind(chunk.range_start)
        .bind(chunk.range_end)
        .fetch_one(&context.pool)
        .await
        .map(Some)
        .map_err(db_error)
    }

    async fn export_chunk(
        &self,
        context: &DrainContext,
        chunk: &Chunk,
    ) -> Result<Publication, DrainExecutionError> {
        export_chunk(context, &self.root, chunk).await
    }
}

async fn export_chunk(
    context: &DrainContext,
    root: &std::path::Path,
    chunk: &Chunk,
) -> Result<Publication, DrainExecutionError> {
    let object_id = create_object(context, KEY, RELATION, chunk).await?;
    let staging = root
        .join(".staging")
        .join(format!("{object_id}.parquet.tmp"));
    let _ = fs::remove_file(&staging).await;
    let (sender, writer) = start_writer(staging.clone(), binance_schema::schema());
    let mut rows = sqlx::query_as::<_, TradeRow>("SELECT source,symbol,aggregate_trade_id,trade_timestamp,provider_available_at,received_at,price,quantity,first_trade_id,last_trade_id,buyer_maker,best_match,payload_sha256::text,strategy_key,capture_artifact_id,ingested_at FROM market_data.binance_spot_btcusdt_aggregate_trades WHERE trade_timestamp >= $1 AND trade_timestamp < $2 ORDER BY trade_timestamp,aggregate_trade_id")
        .bind(chunk.range_start).bind(chunk.range_end).fetch(&context.pool);
    let mut buffer = Vec::with_capacity(BATCH_ROWS);
    let mut count = 0i64;
    let mut minimum_id = None;
    let mut maximum_id = None;
    while let Some(row) = rows.try_next().await.map_err(db_error)? {
        minimum_id = Some(minimum_id.map_or(row.aggregate_trade_id, |value: i64| {
            value.min(row.aggregate_trade_id)
        }));
        maximum_id = Some(maximum_id.map_or(row.aggregate_trade_id, |value: i64| {
            value.max(row.aggregate_trade_id)
        }));
        buffer.push(row);
        count += 1;
        if buffer.len() == BATCH_ROWS {
            sender
                .send(binance_schema::to_batch(std::mem::take(&mut buffer))?)
                .await
                .map_err(|_| io_error("Parquet writer stopped"))?;
        }
    }
    if !buffer.is_empty() {
        sender
            .send(binance_schema::to_batch(buffer)?)
            .await
            .map_err(|_| io_error("Parquet writer stopped"))?;
    }
    finish_writer(sender, writer).await?;
    let partition = format!(
        "provider=binance_spot/dataset=aggregate_trades/symbol=BTCUSDT/year={}/month={}/day={}",
        chunk.range_start.format("%Y"),
        chunk.range_start.format("%m"),
        chunk.range_start.format("%d")
    );
    let relative = format!("{partition}/{object_id}.parquet");
    let (sha256, byte_size) = publish_file(&staging, root, &relative, count).await?;
    sqlx::query_as::<_, Publication>("UPDATE ingester.drain_objects SET row_count=$2,minimum_aggregate_trade_id=$3,maximum_aggregate_trade_id=$4,relative_path=$5,sha256=$6,byte_size=$7,status='published',published_at=clock_timestamp(),updated_at=clock_timestamp() WHERE object_id=$1 AND status='staging' RETURNING object_id,row_count,relative_path,sha256::text,byte_size,status")
        .bind(object_id).bind(count).bind(minimum_id).bind(maximum_id).bind(relative).bind(sha256).bind(byte_size).fetch_one(&context.pool).await.map_err(db_error)
}

#[cfg(test)]
#[path = "tests/binance_aggregate_trades.rs"]
mod tests;
