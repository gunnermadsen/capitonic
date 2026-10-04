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
use chrono::{DateTime, Duration, Utc};
use futures_util::TryStreamExt;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::Row;
use tokio::fs;

use crate::domain::{
    DrainContext, DrainDescriptor, DrainExecutionError, DrainMode, DrainOutcome, DrainRequest,
    DrainWorkerStrategy, GOES_COLUMNS, HRRR_COLUMNS,
};

use super::common::{
    create_object, db_error, existing_publication, finish_writer, invalid, io_error,
    preflight_lake_root, publish_file, reset_publication, send_batch, start_writer,
    verify_existing, Chunk, Publication,
};

const BATCH_ROWS: usize = 1_000;

const DECISION_COLUMNS: &[&str] = &[
    "decision_id",
    "decision_at",
    "run_id",
    "process_id",
    "market_id",
    "snapshot_id",
    "strategy_version",
    "config_hash",
    "action",
    "outcome",
    "token_id",
    "fair_probability",
    "executable_price",
    "gross_edge_per_share",
    "fee_per_share",
    "reserve_per_share",
    "net_edge_per_share",
    "size",
    "status",
    "reject_reason",
    "order_plan_id",
    "execution_mode",
    "metadata",
    "created_at",
];
const DECISION_FILTER: &str = "t.decision_at >= $1 AND t.decision_at < $2 AND t.action = 'no_trade' AND t.status = 'rejected' AND t.order_plan_id IS NULL";

const MARKET_COLUMNS: &[&str] = &["market_id", "raw_payload", "validation_errors"];
const FACT_COLUMNS: &[&str] = &["fact_id", "evidence"];
const CONTRACT_COLUMNS: &[&str] = &["market_id", "revision_sha256", "source_payload"];
const RESOLUTION_COLUMNS: &[&str] = &["market_id", "source", "payload_sha256", "source_payload"];

#[derive(Clone, Copy)]
pub enum VerifiedRowKind {
    Goes,
    Hrrr,
    MarketPayload,
    ReferenceEvidence,
    ContractPayload,
    ResolutionPayload,
    StrategyDecisions,
}

impl VerifiedRowKind {
    fn key(self) -> &'static str {
        match self {
            Self::StrategyDecisions => "polymarket_btc_strategy_decisions",
            Self::Goes => "goes_abi_features",
            Self::Hrrr => "hrrr_environment_features",
            Self::MarketPayload => "polymarket_btc_interval_market_payload",
            Self::ReferenceEvidence => "polymarket_btc_market_reference_fact_evidence",
            Self::ContractPayload => "polymarket_btc_five_minute_contract_payload",
            Self::ResolutionPayload => "polymarket_btc_five_minute_resolution_payload",
        }
    }

    fn relation(self) -> &'static str {
        match self {
            Self::StrategyDecisions => "polymarket.btc_strategy_decisions",
            Self::Goes => "weather.goes_abi_features",
            Self::Hrrr => "weather.hrrr_environment_features",
            Self::MarketPayload => "polymarket.btc_interval_markets",
            Self::ReferenceEvidence => "polymarket.btc_market_reference_facts",
            Self::ContractPayload => "market_data.polymarket_btc_five_minute_contracts",
            Self::ResolutionPayload => "market_data.polymarket_btc_five_minute_resolutions",
        }
    }

    fn schema(self) -> &'static str {
        match self {
            Self::Goes | Self::Hrrr => "weather",
            Self::MarketPayload | Self::ReferenceEvidence | Self::StrategyDecisions => "polymarket",
            Self::ContractPayload | Self::ResolutionPayload => "market_data",
        }
    }

    fn columns(self) -> &'static [&'static str] {
        match self {
            Self::StrategyDecisions => DECISION_COLUMNS,
            Self::Goes => GOES_COLUMNS,
            Self::Hrrr => HRRR_COLUMNS,
            Self::MarketPayload => MARKET_COLUMNS,
            Self::ReferenceEvidence => FACT_COLUMNS,
            Self::ContractPayload => CONTRACT_COLUMNS,
            Self::ResolutionPayload => RESOLUTION_COLUMNS,
        }
    }

    fn retention_days(self) -> i64 {
        match self {
            Self::StrategyDecisions => 0,
            Self::Goes | Self::Hrrr => 0,
            Self::MarketPayload | Self::ContractPayload | Self::ResolutionPayload => 5,
            Self::ReferenceEvidence => 3,
        }
    }

    fn retention(self) -> Duration {
        match self {
            Self::StrategyDecisions => Duration::hours(12),
            _ => Duration::days(self.retention_days()),
        }
    }

    fn window(self) -> Duration {
        match self {
            Self::StrategyDecisions => Duration::minutes(5),
            _ => Duration::days(1),
        }
    }

    fn closed_cutoff(self, cutoff: DateTime<Utc>) -> DateTime<Utc> {
        let seconds = self.window().num_seconds();
        DateTime::from_timestamp(cutoff.timestamp().div_euclid(seconds) * seconds, 0).unwrap()
    }

    fn window_name(self, start: DateTime<Utc>) -> String {
        match self {
            Self::StrategyDecisions => start.format("%Y-%m-%dT%H:%MZ").to_string(),
            _ => start.format("%Y-%m-%d").to_string(),
        }
    }

    fn root_env(self) -> &'static str {
        match self {
            Self::StrategyDecisions => "INGESTER_BTC_STRATEGY_DECISIONS_LAKE_ROOT",
            Self::Goes => "INGESTER_GOES_ABI_FEATURES_LAKE_ROOT",
            Self::Hrrr => "INGESTER_HRRR_ENVIRONMENT_FEATURES_LAKE_ROOT",
            Self::MarketPayload => "INGESTER_BTC_INTERVAL_MARKET_PAYLOAD_LAKE_ROOT",
            Self::ReferenceEvidence => "INGESTER_BTC_MARKET_REFERENCE_EVIDENCE_LAKE_ROOT",
            Self::ContractPayload => "INGESTER_BTC_FIVE_MINUTE_CONTRACT_PAYLOAD_LAKE_ROOT",
            Self::ResolutionPayload => "INGESTER_BTC_FIVE_MINUTE_RESOLUTION_PAYLOAD_LAKE_ROOT",
        }
    }

    fn default_root(self) -> &'static str {
        match self {
            Self::StrategyDecisions => "/var/lib/verified-drains/polymarket/btc-strategy-decisions",
            Self::Goes => "/var/lib/weather/curated/drains/goes-abi-features",
            Self::Hrrr => "/var/lib/weather/curated/drains/hrrr-environment-features",
            Self::MarketPayload => {
                "/var/lib/verified-drains/polymarket/btc-interval-market-payload"
            }
            Self::ReferenceEvidence => {
                "/var/lib/verified-drains/polymarket/btc-market-reference-evidence"
            }
            Self::ContractPayload => {
                "/var/lib/verified-drains/market-data/btc-five-minute-contract-payload"
            }
            Self::ResolutionPayload => {
                "/var/lib/verified-drains/market-data/btc-five-minute-resolution-payload"
            }
        }
    }

    fn row_json_sql(self) -> &'static str {
        match self {
            Self::StrategyDecisions => "to_jsonb(t)::text",
            Self::Goes | Self::Hrrr => "to_jsonb(t)::text",
            Self::MarketPayload => "jsonb_build_object('market_id',t.market_id,'raw_payload',t.raw_payload,'validation_errors',t.validation_errors)::text",
            Self::ReferenceEvidence => "jsonb_build_object('fact_id',t.fact_id,'evidence',t.evidence)::text",
            Self::ContractPayload => "jsonb_build_object('market_id',t.market_id,'revision_sha256',t.revision_sha256,'source_payload',t.source_payload)::text",
            Self::ResolutionPayload => "jsonb_build_object('market_id',t.market_id,'source',t.source,'payload_sha256',t.payload_sha256,'source_payload',t.source_payload)::text",
        }
    }

    fn where_sql(self) -> &'static str {
        match self {
            Self::StrategyDecisions => DECISION_FILTER,
            Self::Goes | Self::Hrrr => "t.decision_time >= $1 AND t.decision_time < $2",
            Self::MarketPayload => "t.window_start >= $1 AND t.window_start < $2 AND t.official_outcome IS NOT NULL AND NOT EXISTS (SELECT 1 FROM polymarket.btc_official_resolution_watches watch WHERE watch.market_id=t.market_id AND watch.status IN ('pending','expired')) AND (t.raw_payload <> '{}'::jsonb OR t.validation_errors <> '[]'::jsonb)",
            Self::ReferenceEvidence => "t.source_effective_at >= $1 AND t.source_effective_at < $2 AND market.official_outcome IS NOT NULL AND t.evidence <> '{}'::jsonb",
            Self::ContractPayload | Self::ResolutionPayload => "t.window_start >= $1 AND t.window_start < $2 AND t.source_payload IS NOT NULL AND EXISTS (SELECT 1 FROM polymarket.btc_interval_markets market WHERE market.market_id=t.market_id AND market.official_outcome IS NOT NULL) AND NOT EXISTS (SELECT 1 FROM polymarket.btc_official_resolution_watches watch WHERE watch.market_id=t.market_id AND watch.status IN ('pending','expired'))",
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
            Self::StrategyDecisions => "decision_at",
            Self::Goes | Self::Hrrr => "decision_time",
            Self::MarketPayload | Self::ContractPayload | Self::ResolutionPayload => "window_start",
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
        if matches!(self.kind, VerifiedRowKind::StrategyDecisions) {
            return format!(
                "WITH earliest AS (SELECT date_bin('5 minutes',t.decision_at,'1970-01-01'::timestamptz) AS start FROM {} t WHERE {} ORDER BY t.decision_at,t.decision_id LIMIT 1) SELECT date_bin('5 minutes',t.decision_at,'1970-01-01'::timestamptz) AS day, sum(pg_column_size(t))::bigint AS size_bytes FROM {} t CROSS JOIN earliest WHERE {} AND t.decision_at >= earliest.start AND t.decision_at < earliest.start + interval '1 hour' GROUP BY day ORDER BY day",
                self.kind.relation(), self.kind.where_sql(), self.kind.relation(), self.kind.where_sql(),
            );
        }
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
        if request.mode.removes_source_data() && request.cutoff > Utc::now() - self.kind.retention()
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
        let closed = self.kind.closed_cutoff(cutoff);
        let query = self.days_query();
        let mut days: Vec<(DateTime<Utc>, i64)> = sqlx::query_as(&query)
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
                summary: json!({"eligible_days":days.len(),"days":days.iter().map(|(day,_)|day).collect::<Vec<_>>(),"relation":self.kind.relation(),"cutoff":request.cutoff,"retention_days":self.kind.retention().num_hours() as f64 / 24.0,"retention_hours":self.kind.retention().num_hours(),"mode":request.mode}),
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
        loop {
            let next_start = days.last().map(|(start, _)| *start + self.kind.window());
            for (day, source_size) in days {
                if context.shutdown.is_cancelled() {
                    return Err(cancelled());
                }
                let chunk = Chunk {
                    chunk_schema: self.kind.schema().into(),
                    chunk_name: self.kind.window_name(day),
                    range_start: day,
                    range_end: day + self.kind.window(),
                    size_bytes: source_size,
                };
                let publication = match existing_publication(&context, self.kind.key(), &chunk)
                    .await?
                {
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
            if !matches!(self.kind, VerifiedRowKind::StrategyDecisions) {
                break;
            }
            let Some(next_start) = next_start else {
                break;
            };
            if context.shutdown.is_cancelled() {
                return Err(cancelled());
            }
            days = sqlx::query_as(&query)
                .bind(next_start)
                .bind(closed)
                .fetch_all(&context.pool)
                .await
                .map_err(db_error)?;
        }
        outcome.summary = json!({"relation":self.kind.relation(),"cutoff":request.cutoff,
            "days_processed":outcome.objects_published,"retention_days":self.kind.retention().num_hours() as f64 / 24.0,"retention_hours":self.kind.retention().num_hours(),
            "mode":request.mode});
        Ok(outcome)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn decisions_preserve_trading_evidence_and_bound_source_windows() {
        let kind = VerifiedRowKind::StrategyDecisions;
        assert_eq!(kind.retention(), Duration::hours(12));
        assert_eq!(kind.window(), Duration::minutes(5));
        assert_eq!(kind.columns().len(), 24);
        assert!(kind.where_sql().contains("t.action = 'no_trade'"));
        assert!(kind.where_sql().contains("t.status = 'rejected'"));
        assert!(kind.where_sql().contains("t.order_plan_id IS NULL"));
        let cutoff = DateTime::parse_from_rfc3339("2026-10-03T13:04:59Z")
            .unwrap()
            .with_timezone(&Utc);
        let closed = kind.closed_cutoff(cutoff);
        assert_eq!(kind.window_name(closed), "2026-10-03T13:00Z");
        let drain = VerifiedRowsDrain::new(kind).unwrap();
        assert!(drain.days_query().contains("interval '1 hour'"));
        assert!(drain
            .days_query()
            .contains("ORDER BY t.decision_at,t.decision_id LIMIT 1"));
        let mut request = DrainRequest {
            strategy_key: kind.key().into(),
            cutoff: Utc::now() - Duration::hours(11),
            dry_run: false,
            mode: DrainMode::Drain,
            execution: Default::default(),
        };
        assert_eq!(
            drain.validate_request(&request).unwrap_err().code,
            "drain_retention_violation"
        );
        request.cutoff = Utc::now() - Duration::hours(12) - Duration::seconds(1);
        drain.validate_request(&request).unwrap();
        let daily = VerifiedRowKind::MarketPayload;
        assert_eq!(daily.window_name(daily.closed_cutoff(cutoff)), "2026-10-03");
    }

    #[tokio::test]
    async fn decision_parquet_preserves_all_fields_and_rejects_corruption() {
        let drain = VerifiedRowsDrain::new(VerifiedRowKind::StrategyDecisions).unwrap();
        let mut row = serde_json::Map::new();
        for name in DECISION_COLUMNS {
            row.insert((*name).into(), Value::Null);
        }
        row.insert("decision_id".into(), json!("decision-1"));
        row.insert("process_id".into(), json!("process-1"));
        row.insert("action".into(), json!("no_trade"));
        row.insert(
            "metadata".into(),
            json!({"feature": 1.25, "nested": [null, true]}),
        );
        let source = Value::Object(row).to_string();
        let expected = format!("{:x}", Sha256::digest(source.as_bytes()));
        let directory = tempdir().unwrap();
        let path = directory.path().join("decisions.parquet");
        let (sender, writer) = start_writer(path.clone(), drain.schema());
        sender
            .send(drain.batch(vec![source]).unwrap())
            .await
            .unwrap();
        finish_writer(sender, writer).await.unwrap();
        verify_source_rows(&path, DECISION_COLUMNS, &expected)
            .await
            .unwrap();
        assert_eq!(
            verify_source_rows(&path, DECISION_COLUMNS, &"0".repeat(64))
                .await
                .unwrap_err()
                .code,
            "drain_source_payload_mismatch"
        );
        std::fs::write(&path, b"corrupt").unwrap();
        assert_eq!(
            verify_source_rows(&path, DECISION_COLUMNS, &expected)
                .await
                .unwrap_err()
                .code,
            "drain_parquet_invalid"
        );
    }

    #[test]
    fn row_schema_keeps_the_source_and_each_named_field() {
        assert_eq!(GOES_COLUMNS.len(), 43);
        assert_eq!(HRRR_COLUMNS.len(), 29);
        for kind in [
            VerifiedRowKind::Goes,
            VerifiedRowKind::Hrrr,
            VerifiedRowKind::MarketPayload,
            VerifiedRowKind::ReferenceEvidence,
            VerifiedRowKind::ContractPayload,
            VerifiedRowKind::ResolutionPayload,
            VerifiedRowKind::StrategyDecisions,
        ] {
            let drain = VerifiedRowsDrain::new(kind).unwrap();
            let schema = drain.schema();
            assert_eq!(schema.fields().len(), kind.columns().len() + 1);
            assert_eq!(schema.field(0).name(), "source_row_json");
            assert!(drain.row_query().contains("ORDER BY source_row_json"));
        }
    }

    #[test]
    fn five_minute_payloads_require_projected_outcomes_and_preserve_five_days() {
        for kind in [
            VerifiedRowKind::ContractPayload,
            VerifiedRowKind::ResolutionPayload,
        ] {
            assert_eq!(kind.retention_days(), 5);
            assert!(kind
                .where_sql()
                .contains("market.official_outcome IS NOT NULL"));
            assert!(kind.where_sql().contains("t.source_payload IS NOT NULL"));
            assert!(kind
                .where_sql()
                .contains("watch.status IN ('pending','expired')"));
            assert_eq!(kind.time_column(), "window_start");
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
