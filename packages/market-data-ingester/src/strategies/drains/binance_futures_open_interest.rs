use super::{
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
use arrow_array::{ArrayRef, RecordBatch, StringArray};
use arrow_schema::{DataType, Field, Schema};
use async_trait::async_trait;
use futures_util::TryStreamExt;
use sqlx::{postgres::PgRow, Row};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::fs;
use uuid::Uuid;

const SPEC: RetainedDrainSpec = RetainedDrainSpec {
    key: "binance_futures_btcusdt_open_interest",
    relation: "market_data.binance_futures_btcusdt_open_interest",
    schema: "market_data",
    table: "binance_futures_btcusdt_open_interest",
    retention_days: Some(14),
};
const COLUMNS: [&str; 14] = [
    "source",
    "source_timestamp",
    "symbol",
    "period_seconds",
    "sum_open_interest",
    "sum_open_interest_value",
    "cmc_circulating_supply",
    "provider_available_at",
    "received_at",
    "source_payload",
    "payload_sha256",
    "strategy_key",
    "capture_artifact_id",
    "ingested_at",
];
const BATCH_ROWS: usize = 500;

pub struct BinanceFuturesOpenInterestDrain {
    descriptor: DrainDescriptor,
    root: PathBuf,
}
impl BinanceFuturesOpenInterestDrain {
    pub fn from_environment() -> Result<Self, DrainExecutionError> {
        let root = PathBuf::from(
            std::env::var("INGESTER_BINANCE_FUTURES_OPEN_INTEREST_LAKE_ROOT")
                .unwrap_or_else(|_| "/var/lib/binance-l2/drains/futures-open-interest".into()),
        );
        if !root.is_absolute() {
            return Err(invalid(
                "drain_root_invalid",
                "Binance open-interest lake root must be absolute",
            ));
        }
        Ok(Self {
            descriptor: retained::descriptor(&SPEC),
            root,
        })
    }
}
fn schema() -> Arc<Schema> {
    Arc::new(Schema::new(
        COLUMNS
            .iter()
            .map(|name| Field::new(*name, DataType::Utf8, true))
            .collect::<Vec<_>>(),
    ))
}
fn batch(rows: Vec<PgRow>) -> Result<RecordBatch, DrainExecutionError> {
    let arrays = COLUMNS
        .iter()
        .enumerate()
        .map(|(index, _)| -> Result<ArrayRef, DrainExecutionError> {
            let values = rows
                .iter()
                .map(|row| row.try_get::<Option<String>, _>(index).map_err(db_error))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Arc::new(StringArray::from(values)))
        })
        .collect::<Result<Vec<_>, _>>()?;
    RecordBatch::try_new(schema(), arrays).map_err(|error| io_error(error.to_string()))
}
#[async_trait]
impl RetainedDrainAdapter for BinanceFuturesOpenInterestDrain {
    fn spec(&self) -> &RetainedDrainSpec {
        &SPEC
    }
    fn root(&self) -> &Path {
        &self.root
    }
    async fn export_chunk(
        &self,
        context: &DrainContext,
        chunk: &Chunk,
    ) -> Result<Publication, DrainExecutionError> {
        let id = create_object(context, SPEC.key, SPEC.relation, chunk).await?;
        let staging = self.root.join(".staging").join(format!("{id}.parquet.tmp"));
        let _ = fs::remove_file(&staging).await;
        let (sender, writer) = start_writer(staging.clone(), schema());
        let mut stream = sqlx::query("SELECT source,source_timestamp::text,symbol,period_seconds::text,sum_open_interest::text,sum_open_interest_value::text,cmc_circulating_supply::text,provider_available_at::text,received_at::text,source_payload::text,payload_sha256,strategy_key,capture_artifact_id::text,ingested_at::text FROM market_data.binance_futures_btcusdt_open_interest WHERE source_timestamp >= $1 AND source_timestamp < $2 ORDER BY source_timestamp,symbol,period_seconds").bind(chunk.range_start).bind(chunk.range_end).fetch(&context.pool);
        let mut rows = Vec::with_capacity(BATCH_ROWS);
        let mut count = 0i64;
        while let Some(row) = stream.try_next().await.map_err(db_error)? {
            rows.push(row);
            count += 1;
            if rows.len() == BATCH_ROWS {
                sender
                    .send(batch(std::mem::take(&mut rows))?)
                    .await
                    .map_err(|_| io_error("Parquet writer stopped"))?;
            }
        }
        if !rows.is_empty() {
            sender
                .send(batch(rows)?)
                .await
                .map_err(|_| io_error("Parquet writer stopped"))?;
        }
        finish_writer(sender, writer).await?;
        publish(context, &self.root, chunk, id, staging, count).await
    }
}
async fn publish(
    context: &DrainContext,
    root: &Path,
    chunk: &Chunk,
    id: Uuid,
    staging: PathBuf,
    count: i64,
) -> Result<Publication, DrainExecutionError> {
    let relative = format!(
        "verified-chunks-v1/year={}/month={}/day={}/{id}.parquet",
        chunk.range_start.format("%Y"),
        chunk.range_start.format("%m"),
        chunk.range_start.format("%d")
    );
    let (sha, size) = publish_file(&staging, root, &relative, count).await?;
    sqlx::query_as("UPDATE ingester.drain_objects SET row_count=$2,relative_path=$3,sha256=$4,byte_size=$5,status='published',published_at=clock_timestamp(),updated_at=clock_timestamp() WHERE object_id=$1 AND status='staging' RETURNING object_id,row_count,relative_path,sha256::text,byte_size,status").bind(id).bind(count).bind(relative).bind(sha).bind(size).fetch_one(&context.pool).await.map_err(db_error)
}
#[async_trait]
impl DrainWorkerStrategy for BinanceFuturesOpenInterestDrain {
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
#[cfg(test)]
#[path = "tests/binance_futures_open_interest.rs"]
mod tests;
