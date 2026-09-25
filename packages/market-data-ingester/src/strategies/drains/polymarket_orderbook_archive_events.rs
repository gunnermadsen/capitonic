use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use arrow_array::{ArrayRef, RecordBatch, StringArray};
use arrow_schema::{DataType, Field, Schema};
use async_trait::async_trait;
use futures_util::TryStreamExt;
use sqlx::{postgres::PgRow, Row};
use tokio::fs;

use crate::domain::{
    DrainContext, DrainDescriptor, DrainExecutionError, DrainOutcome, DrainRequest,
    DrainWorkerStrategy,
};

use super::{
    common::{
        create_object, db_error, finish_writer, invalid, io_error, publish_file, start_writer,
        Chunk, Publication,
    },
    retained::{self, RetainedDrainAdapter, RetainedDrainSpec},
};

const SPEC: RetainedDrainSpec = RetainedDrainSpec {
    key: "polymarket_btc_orderbook_archive_events",
    relation: "polymarket.btc_orderbook_archive_events",
    schema: "polymarket",
    table: "btc_orderbook_archive_events",
    time_column: "provider_received_at",
    retention_days: Some(0),
};
const BACKFILL_KEY: &str = "polymarket_btc_five_minute_orderbook_events_backfill";
const BATCH_ROWS: usize = 2_000;
const COLUMNS: [&str; 19] = [
    "artifact_id",
    "source_row_number",
    "provider_received_at",
    "source_timestamp",
    "condition_id",
    "asset_id",
    "event_type",
    "bids",
    "asks",
    "price",
    "size",
    "side",
    "best_bid",
    "best_ask",
    "fee_rate_bps",
    "transaction_hash",
    "old_tick_size",
    "new_tick_size",
    "ingested_at",
];

pub struct PolymarketOrderbookArchiveEventsDrain {
    descriptor: DrainDescriptor,
    root: PathBuf,
}

impl PolymarketOrderbookArchiveEventsDrain {
    pub fn from_environment() -> Result<Self, DrainExecutionError> {
        let mount = PathBuf::from(
            std::env::var("INGESTER_POLYMARKET_ORDERBOOK_LAKE_ROOT")
                .unwrap_or_else(|_| "/var/lib/polymarket-orderbooks".into()),
        );
        if !mount.is_absolute() {
            return Err(invalid(
                "drain_root_invalid",
                "Polymarket orderbook lake root must be absolute",
            ));
        }
        Ok(Self {
            descriptor: retained::descriptor(&SPEC),
            root: mount.join("archive-events"),
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
    let arrays = (0..COLUMNS.len())
        .map(|index| -> Result<ArrayRef, DrainExecutionError> {
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
impl RetainedDrainAdapter for PolymarketOrderbookArchiveEventsDrain {
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
        let object_id = create_object(context, SPEC.key, SPEC.relation, chunk).await?;
        let staging = self
            .root
            .join(".staging")
            .join(format!("{object_id}.parquet.tmp"));
        let _ = fs::remove_file(&staging).await;
        let (sender, writer) = start_writer(staging.clone(), schema());
        let mut source = sqlx::query(
            "SELECT artifact_id::text,source_row_number::text,provider_received_at::text,\
             source_timestamp::text,condition_id,asset_id,event_type,bids::text,asks::text,\
             price::text,size::text,side,best_bid::text,best_ask::text,fee_rate_bps::text,\
             transaction_hash,old_tick_size::text,new_tick_size::text,ingested_at::text \
             FROM polymarket.btc_orderbook_archive_events \
             WHERE provider_received_at >= $1 AND provider_received_at < $2 \
             ORDER BY provider_received_at,artifact_id,source_row_number",
        )
        .bind(chunk.range_start)
        .bind(chunk.range_end)
        .fetch(&context.pool);
        let mut pending = Vec::with_capacity(BATCH_ROWS);
        let mut count = 0i64;
        loop {
            let row = tokio::select! {
                _ = context.shutdown.cancelled() => return Err(DrainExecutionError::new(
                    "drain_cancelled", "drain was cancelled", true,
                )),
                result = source.try_next() => result.map_err(db_error)?,
            };
            let Some(row) = row else { break };
            pending.push(row);
            count += 1;
            if pending.len() == BATCH_ROWS {
                sender
                    .send(batch(std::mem::take(&mut pending))?)
                    .await
                    .map_err(|_| io_error("Parquet writer stopped"))?;
                tokio::time::sleep(std::time::Duration::from_millis(15)).await;
            }
        }
        drop(source);
        if !pending.is_empty() {
            sender
                .send(batch(pending)?)
                .await
                .map_err(|_| io_error("Parquet writer stopped"))?;
        }
        finish_writer(sender, writer).await?;
        let relative_path = format!(
            "verified-chunks-v1/year={}/month={}/day={}/{object_id}.parquet",
            chunk.range_start.format("%Y"),
            chunk.range_start.format("%m"),
            chunk.range_start.format("%d"),
        );
        let (sha256, byte_size) = publish_file(&staging, &self.root, &relative_path, count).await?;
        sqlx::query_as::<_, Publication>(
            "UPDATE ingester.drain_objects SET row_count=$2,relative_path=$3,sha256=$4,\
             byte_size=$5,status='published',published_at=clock_timestamp(),\
             updated_at=clock_timestamp() WHERE object_id=$1 AND status='staging' \
             RETURNING object_id,row_count,relative_path,sha256::text,byte_size,status",
        )
        .bind(object_id)
        .bind(count)
        .bind(relative_path)
        .bind(sha256)
        .bind(byte_size)
        .fetch_one(&context.pool)
        .await
        .map_err(db_error)
    }
}

#[async_trait]
impl DrainWorkerStrategy for PolymarketOrderbookArchiveEventsDrain {
    fn descriptor(&self) -> &DrainDescriptor {
        &self.descriptor
    }

    fn validate_request(&self, request: &DrainRequest) -> Result<(), DrainExecutionError> {
        retained::validate(self, request)
    }

    async fn execute_drain(
        &self,
        context: DrainContext,
        request: DrainRequest,
    ) -> Result<DrainOutcome, DrainExecutionError> {
        if !request.dry_run {
            let active = sqlx::query_scalar::<_, i64>(
                "SELECT count(*)::bigint FROM ingester.backfill_jobs \
                 WHERE strategy_key=$1 AND status IN ('queued','running')",
            )
            .bind(BACKFILL_KEY)
            .fetch_one(&context.pool)
            .await
            .map_err(db_error)?;
            if active > 0 {
                return Err(DrainExecutionError::new(
                    "drain_backfill_active",
                    "orderbook archive drain is blocked while its backfill is active",
                    true,
                ));
            }
        }
        retained::execute(self, context, request).await
    }
}

#[cfg(test)]
#[path = "tests/polymarket_orderbook_archive_events.rs"]
mod tests;
