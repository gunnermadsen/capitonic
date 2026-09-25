use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicI32, Ordering},
        Arc,
    },
};

use arrow_array::{Array, ArrayRef, RecordBatch, StringArray};
use arrow_schema::{DataType, Field, Schema};
use async_trait::async_trait;
use chrono::{DateTime, Duration, TimeZone, Utc};
use futures_util::TryStreamExt;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::Row;
use tokio::fs;

use crate::domain::{
    DrainContext, DrainDescriptor, DrainExecutionError, DrainMode, DrainOutcome, DrainRequest,
    DrainWorkerStrategy,
};

use super::common::{
    create_object, db_error, existing_publication, finish_writer, invalid, io_error,
    preflight_lake_root, publish_file, reset_publication, send_batch, start_writer,
    verify_existing, Chunk, Publication,
};

const BATCH_ROWS: usize = 1_000;

const GOES_COLUMNS: &[&str] = &[
    "process_id",
    "station_id",
    "decision_time",
    "requested_offset_minutes",
    "spatial_radius_km",
    "sector",
    "feature_schema_version",
    "satellite",
    "scan_end",
    "clear_pixel_fraction",
    "cloudy_pixel_fraction",
    "infrared_brightness_temperature_mean_k",
    "infrared_brightness_temperature_stddev_k",
    "infrared_brightness_temperature_p10_k",
    "infrared_brightness_temperature_p50_k",
    "infrared_brightness_temperature_p90_k",
    "cloud_top_temperature_mean_k",
    "cloud_top_temperature_p10_k",
    "cloud_top_temperature_p50_k",
    "cloud_top_temperature_p90_k",
    "cloud_top_height_mean_m",
    "cloud_top_height_p10_m",
    "cloud_top_height_p50_m",
    "cloud_top_height_p90_m",
    "visible_reflectance_mean",
    "visible_reflectance_stddev",
    "visible_reflectance_p10",
    "visible_reflectance_p50",
    "visible_reflectance_p90",
    "cloud_optical_depth_mean",
    "cloud_optical_depth_p50",
    "cloud_optical_depth_p90",
    "water_vapor_brightness_temperature_mean_k",
    "infrared_change_45m_k",
    "infrared_change_165m_k",
    "valid_pixel_fraction",
    "quality_flags",
    "source_metadata",
    "created_at",
    "infrared_north_south_gradient_k",
    "infrared_east_west_gradient_k",
    "cloudy_north_south_gradient",
    "cloudy_east_west_gradient",
];

const HRRR_COLUMNS: &[&str] = &[
    "process_id",
    "station_id",
    "decision_time",
    "model_run",
    "valid_at",
    "lead_hours",
    "spatial_radius_km",
    "sector",
    "feature_schema_version",
    "temperature_2m_mean_k",
    "temperature_2m_stddev_k",
    "dew_point_2m_mean_k",
    "dew_point_2m_stddev_k",
    "total_cloud_cover_mean_fraction",
    "total_cloud_cover_stddev_fraction",
    "downward_shortwave_radiation_mean_w_m2",
    "wind_u_10m_mean_m_s",
    "wind_v_10m_mean_m_s",
    "wind_speed_10m_mean_m_s",
    "boundary_layer_height_mean_m",
    "accumulated_precipitation_mean_mm",
    "composite_reflectivity_mean_dbz",
    "composite_reflectivity_max_dbz",
    "valid_pixel_fraction",
    "quality_flags",
    "source_metadata",
    "created_at",
    "temperature_2m_north_south_gradient_k",
    "temperature_2m_east_west_gradient_k",
];

const MARKET_COLUMNS: &[&str] = &["market_id", "raw_payload", "validation_errors"];
const FACT_COLUMNS: &[&str] = &["fact_id", "evidence"];

#[derive(Clone, Copy)]
pub enum VerifiedRowKind {
    Goes,
    Hrrr,
    MarketPayload,
    ReferenceEvidence,
}

impl VerifiedRowKind {
    fn key(self) -> &'static str {
        match self {
            Self::Goes => "goes_abi_features",
            Self::Hrrr => "hrrr_environment_features",
            Self::MarketPayload => "polymarket_btc_interval_market_payload",
            Self::ReferenceEvidence => "polymarket_btc_market_reference_fact_evidence",
        }
    }

    fn relation(self) -> &'static str {
        match self {
            Self::Goes => "weather.goes_abi_features",
            Self::Hrrr => "weather.hrrr_environment_features",
            Self::MarketPayload => "polymarket.btc_interval_markets",
            Self::ReferenceEvidence => "polymarket.btc_market_reference_facts",
        }
    }

    fn schema(self) -> &'static str {
        match self {
            Self::Goes | Self::Hrrr => "weather",
            Self::MarketPayload | Self::ReferenceEvidence => "polymarket",
        }
    }

    fn columns(self) -> &'static [&'static str] {
        match self {
            Self::Goes => GOES_COLUMNS,
            Self::Hrrr => HRRR_COLUMNS,
            Self::MarketPayload => MARKET_COLUMNS,
            Self::ReferenceEvidence => FACT_COLUMNS,
        }
    }

    fn retention_days(self) -> i64 {
        match self {
            Self::Goes | Self::Hrrr => 0,
            Self::MarketPayload => 5,
            Self::ReferenceEvidence => 3,
        }
    }

    fn root_env(self) -> &'static str {
        match self {
            Self::Goes => "INGESTER_GOES_ABI_FEATURES_LAKE_ROOT",
            Self::Hrrr => "INGESTER_HRRR_ENVIRONMENT_FEATURES_LAKE_ROOT",
            Self::MarketPayload => "INGESTER_BTC_INTERVAL_MARKET_PAYLOAD_LAKE_ROOT",
            Self::ReferenceEvidence => "INGESTER_BTC_MARKET_REFERENCE_EVIDENCE_LAKE_ROOT",
        }
    }

    fn default_root(self) -> &'static str {
        match self {
            Self::Goes => "/var/lib/weather/curated/drains/goes-abi-features",
            Self::Hrrr => "/var/lib/weather/curated/drains/hrrr-environment-features",
            Self::MarketPayload => {
                "/var/lib/verified-drains/polymarket/btc-interval-market-payload"
            }
            Self::ReferenceEvidence => {
                "/var/lib/verified-drains/polymarket/btc-market-reference-evidence"
            }
        }
    }

    fn row_json_sql(self) -> &'static str {
        match self {
            Self::Goes | Self::Hrrr => "to_jsonb(t)::text",
            Self::MarketPayload => "jsonb_build_object('market_id',t.market_id,'raw_payload',t.raw_payload,'validation_errors',t.validation_errors)::text",
            Self::ReferenceEvidence => "jsonb_build_object('fact_id',t.fact_id,'evidence',t.evidence)::text",
        }
    }

    fn where_sql(self) -> &'static str {
        match self {
            Self::Goes | Self::Hrrr => "t.decision_time >= $1 AND t.decision_time < $2",
            Self::MarketPayload => "t.window_start >= $1 AND t.window_start < $2 AND t.official_outcome IS NOT NULL AND NOT EXISTS (SELECT 1 FROM polymarket.btc_official_resolution_watches watch WHERE watch.market_id=t.market_id AND watch.status IN ('pending','expired')) AND (t.raw_payload <> '{}'::jsonb OR t.validation_errors <> '[]'::jsonb)",
            Self::ReferenceEvidence => "t.source_effective_at >= $1 AND t.source_effective_at < $2 AND market.official_outcome IS NOT NULL AND t.evidence <> '{}'::jsonb",
        }
    }

    fn join_sql(self) -> &'static str {
        match self {
            Self::ReferenceEvidence => {
                "JOIN polymarket.btc_interval_markets market ON market.market_id=t.market_id"
            }
            _ => "",
        }
    }

    fn time_column(self) -> &'static str {
        match self {
            Self::Goes | Self::Hrrr => "decision_time",
            Self::MarketPayload => "window_start",
            Self::ReferenceEvidence => "source_effective_at",
        }
    }
}

pub struct VerifiedRowsDrain {
    kind: VerifiedRowKind,
    descriptor: DrainDescriptor,
    root: PathBuf,
}

struct ActiveReadGuard(Arc<AtomicI32>);

impl Drop for ActiveReadGuard {
    fn drop(&mut self) {
        self.0.store(0, Ordering::SeqCst);
    }
}

impl VerifiedRowsDrain {
    pub fn new(kind: VerifiedRowKind) -> Result<Self, DrainExecutionError> {
        let root = PathBuf::from(
            std::env::var(kind.root_env()).unwrap_or_else(|_| kind.default_root().into()),
        );
        if !root.is_absolute() {
            return Err(invalid(
                "drain_root_invalid",
                "verified row drain root must be absolute",
            ));
        }
        Ok(Self {
            kind,
            descriptor: DrainDescriptor {
                strategy_key: Arc::from(kind.key()),
                relation: Arc::from(kind.relation()),
                contract_version: 1,
            },
            root,
        })
    }

    fn schema(&self) -> Arc<Schema> {
        let mut fields = vec![Field::new("source_row_json", DataType::Utf8, false)];
        fields.extend(
            self.kind
                .columns()
                .iter()
                .map(|name| Field::new(*name, DataType::Utf8, true)),
        );
        Arc::new(Schema::new(fields))
    }

    fn row_query(&self) -> String {
        format!(
            "SELECT {} AS source_row_json FROM {} t {} WHERE {} ORDER BY source_row_json",
            self.kind.row_json_sql(),
            self.kind.relation(),
            self.kind.join_sql(),
            self.kind.where_sql(),
        )
    }

    fn days_query(&self) -> String {
        format!(
            "SELECT (date_trunc('day',t.{} AT TIME ZONE 'UTC') AT TIME ZONE 'UTC') AS day, sum(pg_column_size(t))::bigint AS size_bytes FROM {} t {} WHERE {} GROUP BY day ORDER BY day",
            self.kind.time_column(), self.kind.relation(), self.kind.join_sql(),
            self.kind.where_sql(),
        )
    }

    async fn export_day(
        &self,
        context: &DrainContext,
        chunk: &Chunk,
    ) -> Result<Publication, DrainExecutionError> {
        let id = create_object(context, self.kind.key(), self.kind.relation(), chunk).await?;
        let staging = self.root.join(".staging").join(format!("{id}.parquet.tmp"));
        let _ = fs::remove_file(&staging).await;
        let (sender, mut writer) = start_writer(staging.clone(), self.schema());
        let query = self.row_query();
        let mut connection = context.pool.acquire().await.map_err(db_error)?;
        let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *connection)
            .await
            .map_err(db_error)?;
        context.active_read_pid.store(pid, Ordering::SeqCst);
        let read_guard = ActiveReadGuard(context.active_read_pid.clone());
        let mut stream = sqlx::query(&query)
            .bind(chunk.range_start)
            .bind(chunk.range_end)
            .fetch(&mut *connection);
        let mut source_digest = Sha256::new();
        let mut count = 0i64;
        let mut pending = Vec::with_capacity(BATCH_ROWS);
        while let Some(row) = stream.try_next().await.map_err(db_error)? {
            if context.shutdown.is_cancelled() {
                return Err(cancelled());
            }
            let source_json: String = row.try_get("source_row_json").map_err(db_error)?;
            if count > 0 {
                source_digest.update(b"\n");
            }
            source_digest.update(source_json.as_bytes());
            pending.push(source_json);
            count += 1;
            if pending.len() == BATCH_ROWS {
                send_batch(
                    &sender,
                    &mut writer,
                    self.batch(std::mem::take(&mut pending))?,
                )
                .await?;
            }
        }
        if !pending.is_empty() {
            send_batch(&sender, &mut writer, self.batch(pending)?).await?;
        }
        drop(stream);
        drop(read_guard);
        drop(connection);
        finish_writer(sender, writer).await?;
        if count == 0 {
            return Err(invalid(
                "drain_source_empty",
                "eligible source rows disappeared before publication",
            ));
        }
        if context.shutdown.is_cancelled() {
            return Err(cancelled());
        }
        let source_sha = format!("{:x}", source_digest.finalize());
        let (db_count, db_sha): (i64, String) = sqlx::query_as(
            "SELECT row_count,source_rows_sha256 FROM ingester.verified_row_source_fingerprint($1,$2,$3)",
        )
        .bind(self.kind.key()).bind(chunk.range_start).bind(chunk.range_end)
        .fetch_one(&context.pool).await.map_err(db_error)?;
        if count != db_count || source_sha != db_sha {
            return Err(invalid(
                "drain_source_payload_mismatch",
                "source rows changed during row drain export",
            ));
        }
        let relative = format!(
            "verified-rows-v1/year={}/month={}/day={}/{id}.parquet",
            chunk.range_start.format("%Y"),
            chunk.range_start.format("%m"),
            chunk.range_start.format("%d"),
        );
        let (sha, size) = publish_file(&staging, &self.root, &relative, count).await?;
        verify_source_rows(&self.root.join(&relative), self.kind.columns(), &source_sha).await?;
        sqlx::query_as("UPDATE ingester.drain_objects SET row_count=$2,relative_path=$3,sha256=$4,byte_size=$5,source_rows_sha256=$6,status='published',published_at=clock_timestamp(),updated_at=clock_timestamp() WHERE object_id=$1 AND status='staging' RETURNING object_id,row_count,relative_path,sha256::text,byte_size,status")
            .bind(id).bind(count).bind(relative).bind(sha).bind(size).bind(source_sha)
            .fetch_one(&context.pool).await.map_err(db_error)
    }

    fn batch(&self, rows: Vec<String>) -> Result<RecordBatch, DrainExecutionError> {
        let parsed = rows
            .iter()
            .map(|row| {
                serde_json::from_str::<Value>(row)
                    .map_err(|error| invalid("drain_source_json_invalid", error.to_string()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        for row in &parsed {
            require_source_columns(row, self.kind.columns())?;
        }
        let mut arrays: Vec<ArrayRef> = vec![Arc::new(StringArray::from(rows))];
        for name in self.kind.columns() {
            let values = parsed
                .iter()
                .map(|row| field_value(row.get(*name)))
                .collect::<Vec<_>>();
            arrays.push(Arc::new(StringArray::from(values)));
        }
        RecordBatch::try_new(self.schema(), arrays)
            .map_err(|error| invalid("drain_batch_invalid", error.to_string()))
    }

    async fn remove_day(
        &self,
        context: &DrainContext,
        publication: &Publication,
    ) -> Result<i64, DrainExecutionError> {
        let mut transaction = context.pool.begin().await.map_err(db_error)?;
        sqlx::query("SELECT set_config('lock_timeout','500ms',true),set_config('statement_timeout','30s',true)")
            .execute(&mut *transaction).await.map_err(db_error)?;
        let rows =
            sqlx::query_scalar::<_, i64>("SELECT ingester.remove_verified_drain_rows($1,$2,$3)")
                .bind(publication.object_id)
                .bind(&publication.sha256)
                .bind(context.job_id)
                .fetch_one(&mut *transaction)
                .await
                .map_err(db_error)?;
        sqlx::query_scalar::<_, i64>("UPDATE ingester.drain_jobs SET rows_removed=rows_removed+$3,updated_at=clock_timestamp() WHERE job_id=$1 AND lease_token=$2 AND status='running' AND cancel_requested_at IS NULL RETURNING rows_removed")
            .bind(context.job_id).bind(context.lease_token).bind(rows)
            .fetch_one(&mut *transaction).await.map_err(db_error)?;
        transaction.commit().await.map_err(db_error)?;
        Ok(rows)
    }
}

fn field_value(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::Null => None,
        Value::String(value) => Some(value.clone()),
        value => Some(value.to_string()),
    }
}

fn cancelled() -> DrainExecutionError {
    DrainExecutionError::new("drain_cancelled", "row drain was cancelled", true)
}

fn require_source_columns(source: &Value, columns: &[&str]) -> Result<(), DrainExecutionError> {
    let fields = source.as_object().ok_or_else(|| {
        invalid(
            "drain_source_schema_changed",
            "source row is not a JSON object",
        )
    })?;
    if fields.len() != columns.len() || columns.iter().any(|name| !fields.contains_key(*name)) {
        return Err(invalid(
            "drain_source_schema_changed",
            "source row columns differ from the registered Parquet schema",
        ));
    }
    Ok(())
}

async fn verify_source_rows(
    path: &Path,
    columns: &'static [&'static str],
    expected_sha: &str,
) -> Result<(), DrainExecutionError> {
    let path = path.to_owned();
    let expected_sha = expected_sha.to_owned();
    tokio::task::spawn_blocking(move || -> Result<(), DrainExecutionError> {
        let builder =
            ParquetRecordBatchReaderBuilder::try_new(std::fs::File::open(path).map_err(io_error)?)
                .map_err(|error| invalid("drain_parquet_invalid", error.to_string()))?;
        let mut digest = Sha256::new();
        let mut count = 0u64;
        for batch in builder
            .with_batch_size(1_024)
            .build()
            .map_err(|error| invalid("drain_parquet_invalid", error.to_string()))?
        {
            let batch =
                batch.map_err(|error| invalid("drain_parquet_invalid", error.to_string()))?;
            let source = batch
                .column(0)
                .as_any()
                .downcast_ref::<StringArray>()
                .ok_or_else(|| invalid("drain_schema_invalid", "source row column is not UTF-8"))?;
            for row in 0..batch.num_rows() {
                let source_json = source.value(row);
                let parsed: Value = serde_json::from_str(source_json)
                    .map_err(|error| invalid("drain_source_json_invalid", error.to_string()))?;
                require_source_columns(&parsed, columns)?;
                for (index, name) in columns.iter().enumerate() {
                    let value = batch
                        .column(index + 1)
                        .as_any()
                        .downcast_ref::<StringArray>()
                        .ok_or_else(|| {
                            invalid("drain_schema_invalid", "source field column is not UTF-8")
                        })?;
                    let expected = field_value(parsed.get(*name));
                    if expected.as_deref() != (!value.is_null(row)).then(|| value.value(row)) {
                        return Err(invalid(
                            "drain_column_parity_failed",
                            format!("Parquet source field {name} differs from source row"),
                        ));
                    }
                }
                if count > 0 {
                    digest.update(b"\n");
                }
                digest.update(source_json.as_bytes());
                count += 1;
            }
        }
        if format!("{:x}", digest.finalize()) != expected_sha {
            return Err(invalid(
                "drain_source_payload_mismatch",
                "Parquet rows differ from the source fingerprint",
            ));
        }
        Ok(())
    })
    .await
    .map_err(|error| io_error(error.to_string()))?
}

#[async_trait]
impl DrainWorkerStrategy for VerifiedRowsDrain {
    fn descriptor(&self) -> &DrainDescriptor {
        &self.descriptor
    }

    fn validate_request(&self, request: &DrainRequest) -> Result<(), DrainExecutionError> {
        if request.strategy_key != self.kind.key() {
            return Err(invalid(
                "drain_strategy_mismatch",
                "row drain strategy key mismatch",
            ));
        }
        request
            .execution
            .validate()
            .map_err(|error| invalid("drain_execution_selector_invalid", error.to_string()))?;
        if request.mode.removes_source_data()
            && request.cutoff > Utc::now() - Duration::days(self.kind.retention_days())
        {
            return Err(invalid(
                "drain_retention_violation",
                "cutoff violates row drain buffer",
            ));
        }
        Ok(())
    }

    async fn execute_drain(
        &self,
        context: DrainContext,
        request: DrainRequest,
    ) -> Result<DrainOutcome, DrainExecutionError> {
        self.validate_request(&request)?;
        let requested_at: DateTime<Utc> =
            sqlx::query_scalar("SELECT requested_at FROM ingester.drain_jobs WHERE job_id=$1")
                .bind(context.job_id)
                .fetch_one(&context.pool)
                .await
                .map_err(db_error)?;
        let cutoff = request.cutoff.min(requested_at);
        let closed = Utc.from_utc_datetime(&cutoff.date_naive().and_hms_opt(0, 0, 0).unwrap());
        let query = self.days_query();
        let days: Vec<(DateTime<Utc>, i64)> = sqlx::query_as(&query)
            .bind(DateTime::<Utc>::from_timestamp(0, 0).unwrap())
            .bind(closed)
            .fetch_all(&context.pool)
            .await
            .map_err(db_error)?;
        if request.dry_run {
            return Ok(DrainOutcome {
                rows_exported: 0,
                rows_removed: 0,
                objects_published: 0,
                bytes_written: 0,
                summary: json!({"eligible_days":days.len(),"days":days.iter().map(|(day,_)|day).collect::<Vec<_>>(),"relation":self.kind.relation(),"cutoff":request.cutoff,"retention_days":self.kind.retention_days(),"mode":request.mode}),
            });
        }
        preflight_lake_root(&self.root, 0).await?;
        fs::create_dir_all(self.root.join(".staging"))
            .await
            .map_err(io_error)?;
        let mut outcome = DrainOutcome {
            rows_exported: 0,
            rows_removed: 0,
            objects_published: 0,
            bytes_written: 0,
            summary: json!({}),
        };
        for (day, source_size) in days {
            if context.shutdown.is_cancelled() {
                return Err(cancelled());
            }
            let chunk = Chunk {
                chunk_schema: self.kind.schema().into(),
                chunk_name: day.format("%Y-%m-%d").to_string(),
                range_start: day,
                range_end: day + Duration::days(1),
                size_bytes: source_size,
            };
            let publication = match existing_publication(&context, self.kind.key(), &chunk).await? {
                Some(existing) => {
                    let stored_sha: Option<String> = sqlx::query_scalar(
                        "SELECT source_rows_sha256 FROM ingester.drain_objects WHERE object_id=$1",
                    )
                    .bind(existing.object_id)
                    .fetch_one(&context.pool)
                    .await
                    .map_err(db_error)?;
                    let (count, source_sha): (i64,String) = sqlx::query_as("SELECT row_count,source_rows_sha256 FROM ingester.verified_row_source_fingerprint($1,$2,$3)")
                        .bind(self.kind.key()).bind(day).bind(chunk.range_end)
                        .fetch_one(&context.pool).await.map_err(db_error)?;
                    if count != existing.row_count
                        || stored_sha.as_deref() != Some(source_sha.as_str())
                        || verify_existing(&self.root, &existing).await.is_err()
                        || verify_source_rows(
                            &self.root.join(&existing.relative_path),
                            self.kind.columns(),
                            &source_sha,
                        )
                        .await
                        .is_err()
                    {
                        preflight_lake_root(&self.root, source_size.saturating_mul(2)).await?;
                        reset_publication(&context, &existing).await?;
                        self.export_day(&context, &chunk).await?
                    } else {
                        existing
                    }
                }
                None => {
                    preflight_lake_root(&self.root, source_size.saturating_mul(2)).await?;
                    self.export_day(&context, &chunk).await?
                }
            };
            outcome.rows_exported += publication.row_count;
            outcome.objects_published += 1;
            outcome.bytes_written += publication.byte_size;
            if request.mode == DrainMode::Drain {
                outcome.rows_removed += self.remove_day(&context, &publication).await?;
            }
        }
        outcome.summary = json!({"relation":self.kind.relation(),"cutoff":request.cutoff,
            "days_processed":outcome.objects_published,"retention_days":self.kind.retention_days(),
            "mode":request.mode});
        Ok(outcome)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn row_schema_keeps_the_source_and_each_named_field() {
        assert_eq!(GOES_COLUMNS.len(), 43);
        assert_eq!(HRRR_COLUMNS.len(), 29);
        for kind in [
            VerifiedRowKind::Goes,
            VerifiedRowKind::Hrrr,
            VerifiedRowKind::MarketPayload,
            VerifiedRowKind::ReferenceEvidence,
        ] {
            let drain = VerifiedRowsDrain::new(kind).unwrap();
            let schema = drain.schema();
            assert_eq!(schema.fields().len(), kind.columns().len() + 1);
            assert_eq!(schema.field(0).name(), "source_row_json");
            assert!(drain.row_query().contains("ORDER BY source_row_json"));
        }
    }

    #[tokio::test]
    async fn parquet_recheck_rejects_a_changed_source_field() {
        let drain = VerifiedRowsDrain::new(VerifiedRowKind::MarketPayload).unwrap();
        let directory = tempdir().unwrap();
        let path = directory.path().join("market.parquet");
        let source = r#"{"market_id":"market-1","raw_payload":{"price":1},"validation_errors":[]}"#;
        let mut digest = Sha256::new();
        digest.update(source.as_bytes());
        let source_sha = format!("{:x}", digest.finalize());
        let (sender, writer) = start_writer(path.clone(), drain.schema());
        let mut batch = drain.batch(vec![source.to_owned()]).unwrap();
        batch = RecordBatch::try_new(
            drain.schema(),
            vec![
                batch.column(0).clone(),
                Arc::new(StringArray::from(vec!["wrong-market"])),
                batch.column(2).clone(),
                batch.column(3).clone(),
            ],
        )
        .unwrap();
        sender.send(batch).await.unwrap();
        finish_writer(sender, writer).await.unwrap();
        let failure = verify_source_rows(&path, MARKET_COLUMNS, &source_sha)
            .await
            .unwrap_err();
        assert_eq!(failure.code, "drain_column_parity_failed");
    }

    #[tokio::test]
    async fn parquet_recheck_accepts_exact_source_fields() {
        let drain = VerifiedRowsDrain::new(VerifiedRowKind::ReferenceEvidence).unwrap();
        let directory = tempdir().unwrap();
        let path = directory.path().join("fact.parquet");
        let source = r#"{"fact_id":"1234","evidence":{"price":1.25}}"#;
        let mut digest = Sha256::new();
        digest.update(source.as_bytes());
        let source_sha = format!("{:x}", digest.finalize());
        let (sender, writer) = start_writer(path.clone(), drain.schema());
        sender
            .send(drain.batch(vec![source.to_owned()]).unwrap())
            .await
            .unwrap();
        finish_writer(sender, writer).await.unwrap();
        verify_source_rows(&path, FACT_COLUMNS, &source_sha)
            .await
            .unwrap();
        let failure = verify_source_rows(&path, FACT_COLUMNS, &"0".repeat(64))
            .await
            .unwrap_err();
        assert_eq!(failure.code, "drain_source_payload_mismatch");
    }

    #[test]
    fn new_source_column_requires_an_explicit_parquet_schema_update() {
        let drain = VerifiedRowsDrain::new(VerifiedRowKind::ReferenceEvidence).unwrap();
        let source = r#"{"fact_id":"1234","evidence":{},"unexpected":1}"#;
        let error = drain.batch(vec![source.to_owned()]).unwrap_err();
        assert_eq!(error.code, "drain_source_schema_changed");
    }
}
