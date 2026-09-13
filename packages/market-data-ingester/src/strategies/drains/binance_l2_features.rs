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

pub const COLUMNS: [&str; 50] = [
    "symbol",
    "second_start",
    "source_event_timestamp",
    "provider_received_at",
    "available_at",
    "source_update_id",
    "feature_schema_version",
    "quality_status",
    "artifact_id",
    "midpoint",
    "microprice",
    "spread_bps",
    "bid_depth_5",
    "ask_depth_5",
    "imbalance_5",
    "bid_depth_10",
    "ask_depth_10",
    "imbalance_10",
    "bid_depth_20",
    "ask_depth_20",
    "imbalance_20",
    "bid_depth_slope_20",
    "ask_depth_slope_20",
    "bid_depth_concentration_20",
    "ask_depth_concentration_20",
    "bid_quote_replenishment_1s",
    "ask_quote_replenishment_1s",
    "bid_quote_churn_1s",
    "ask_quote_churn_1s",
    "midpoint_change_bps_1s",
    "spread_bps_delta_1s",
    "depth_20_change_bps_1s",
    "imbalance_20_delta_1s",
    "midpoint_change_bps_5s",
    "spread_bps_delta_5s",
    "depth_20_change_bps_5s",
    "imbalance_20_delta_5s",
    "midpoint_change_bps_15s",
    "spread_bps_delta_15s",
    "depth_20_change_bps_15s",
    "imbalance_20_delta_15s",
    "midpoint_change_bps_30s",
    "spread_bps_delta_30s",
    "depth_20_change_bps_30s",
    "imbalance_20_delta_30s",
    "midpoint_change_bps_60s",
    "spread_bps_delta_60s",
    "depth_20_change_bps_60s",
    "imbalance_20_delta_60s",
    "ingested_at",
];
const SELECT_COLUMNS: &str = "symbol,second_start::text,source_event_timestamp::text,provider_received_at::text,available_at::text,source_update_id::text,feature_schema_version,quality_status,artifact_id::text,midpoint::text,microprice::text,spread_bps::text,bid_depth_5::text,ask_depth_5::text,imbalance_5::text,bid_depth_10::text,ask_depth_10::text,imbalance_10::text,bid_depth_20::text,ask_depth_20::text,imbalance_20::text,bid_depth_slope_20::text,ask_depth_slope_20::text,bid_depth_concentration_20::text,ask_depth_concentration_20::text,bid_quote_replenishment_1s::text,ask_quote_replenishment_1s::text,bid_quote_churn_1s::text,ask_quote_churn_1s::text,midpoint_change_bps_1s::text,spread_bps_delta_1s::text,depth_20_change_bps_1s::text,imbalance_20_delta_1s::text,midpoint_change_bps_5s::text,spread_bps_delta_5s::text,depth_20_change_bps_5s::text,imbalance_20_delta_5s::text,midpoint_change_bps_15s::text,spread_bps_delta_15s::text,depth_20_change_bps_15s::text,imbalance_20_delta_15s::text,midpoint_change_bps_30s::text,spread_bps_delta_30s::text,depth_20_change_bps_30s::text,imbalance_20_delta_30s::text,midpoint_change_bps_60s::text,spread_bps_delta_60s::text,depth_20_change_bps_60s::text,imbalance_20_delta_60s::text,ingested_at::text";
const BATCH_ROWS: usize = 250;

pub struct BinanceL2FeaturesDrain {
    descriptor: DrainDescriptor,
    root: PathBuf,
    spec: &'static RetainedDrainSpec,
}
impl BinanceL2FeaturesDrain {
    pub fn from_environment(
        spec: &'static RetainedDrainSpec,
        environment: &str,
        default_root: &str,
        label: &str,
    ) -> Result<Self, DrainExecutionError> {
        let root =
            PathBuf::from(std::env::var(environment).unwrap_or_else(|_| default_root.into()));
        if !root.is_absolute() {
            return Err(invalid(
                "drain_root_invalid",
                format!("{label} lake root must be absolute"),
            ));
        }
        Ok(Self {
            descriptor: retained::descriptor(spec),
            root,
            spec,
        })
    }
}
pub fn schema() -> Arc<Schema> {
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
impl RetainedDrainAdapter for BinanceL2FeaturesDrain {
    fn spec(&self) -> &RetainedDrainSpec {
        self.spec
    }
    fn root(&self) -> &Path {
        &self.root
    }
    async fn export_chunk(
        &self,
        context: &DrainContext,
        chunk: &Chunk,
    ) -> Result<Publication, DrainExecutionError> {
        let id = create_object(context, self.spec.key, self.spec.relation, chunk).await?;
        let staging = self.root.join(".staging").join(format!("{id}.parquet.tmp"));
        let _ = fs::remove_file(&staging).await;
        let (sender, writer) = start_writer(staging.clone(), schema());
        let query = format!("SELECT {SELECT_COLUMNS} FROM {}.{} WHERE second_start >= $1 AND second_start < $2 ORDER BY second_start,symbol,artifact_id", self.spec.schema, self.spec.table);
        let mut stream = sqlx::query(&query)
            .bind(chunk.range_start)
            .bind(chunk.range_end)
            .fetch(&context.pool);
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
        let relative = format!(
            "verified-chunks-v1/year={}/month={}/day={}/{id}.parquet",
            chunk.range_start.format("%Y"),
            chunk.range_start.format("%m"),
            chunk.range_start.format("%d")
        );
        let (sha, size) = publish_file(&staging, &self.root, &relative, count).await?;
        sqlx::query_as("UPDATE ingester.drain_objects SET row_count=$2,relative_path=$3,sha256=$4,byte_size=$5,status='published',published_at=clock_timestamp(),updated_at=clock_timestamp() WHERE object_id=$1 AND status='staging' RETURNING object_id,row_count,relative_path,sha256::text,byte_size,status").bind(id).bind(count).bind(relative).bind(sha).bind(size).fetch_one(&context.pool).await.map_err(db_error)
    }
}
#[async_trait]
impl DrainWorkerStrategy for BinanceL2FeaturesDrain {
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
