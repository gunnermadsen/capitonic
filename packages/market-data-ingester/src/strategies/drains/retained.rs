use std::{path::Path, sync::Arc, time::Duration as StdDuration};

use async_trait::async_trait;
use chrono::{Duration, Utc};
use serde_json::json;
use tokio::fs;

use crate::domain::{
    DrainContext, DrainDescriptor, DrainExecutionError, DrainMode, DrainOutcome, DrainRequest,
};

use super::common::{
    archived_publications, db_error, existing_publication, invalid, preflight_lake_root,
    reset_publication, verify_existing, Chunk, Publication,
};

pub struct RetainedDrainSpec {
    pub key: &'static str,
    pub relation: &'static str,
    pub schema: &'static str,
    pub table: &'static str,
    pub retention_days: Option<i64>,
}

const CHUNK_REMOVAL_LOCK_TIMEOUT: &str = "500ms";
const CHUNK_REMOVAL_STATEMENT_TIMEOUT: &str = "120s";
const CHUNK_REMOVAL_RETRY_LIMIT: usize = 20;
const CHUNK_REMOVAL_RETRY_INITIAL: StdDuration = StdDuration::from_millis(250);
const CHUNK_REMOVAL_RETRY_MAX: StdDuration = StdDuration::from_secs(5);
const CHUNK_REMOVAL_PACING: StdDuration = StdDuration::from_millis(100);

#[async_trait]
pub trait RetainedDrainAdapter: Send + Sync {
    fn spec(&self) -> &RetainedDrainSpec;
    fn root(&self) -> &Path;
    async fn source_count(
        &self,
        _context: &DrainContext,
        _chunk: &Chunk,
    ) -> Result<Option<i64>, DrainExecutionError> {
        Ok(None)
    }
    async fn export_chunk(
        &self,
        context: &DrainContext,
        chunk: &Chunk,
    ) -> Result<Publication, DrainExecutionError>;
}

pub fn descriptor(spec: &RetainedDrainSpec) -> DrainDescriptor {
    DrainDescriptor {
        strategy_key: Arc::from(spec.key),
        relation: Arc::from(spec.relation),
        contract_version: 1,
    }
}

pub fn validate(
    adapter: &dyn RetainedDrainAdapter,
    request: &DrainRequest,
) -> Result<(), DrainExecutionError> {
    let spec = adapter.spec();
    if request.strategy_key != spec.key {
        return Err(invalid(
            "drain_strategy_mismatch",
            format!("request does not target the {} drain", spec.key),
        ));
    }
    request
        .execution
        .validate()
        .map_err(|error| invalid("drain_execution_selector_invalid", error.to_string()))?;
    if request.mode.removes_source_data() {
        let Some(days) = spec.retention_days else {
            return Ok(());
        };
        if request.cutoff > Utc::now() - Duration::days(days) {
            return Err(invalid(
                "drain_retention_violation",
                format!("cutoff must retain at least {days} days of data"),
            ));
        }
    }
    Ok(())
}

pub async fn execute(
    adapter: &dyn RetainedDrainAdapter,
    context: DrainContext,
    request: DrainRequest,
) -> Result<DrainOutcome, DrainExecutionError> {
    validate(adapter, &request)?;
    let spec = adapter.spec();
    let snapshot_at = sqlx::query_scalar::<_, chrono::DateTime<Utc>>(
        "SELECT requested_at FROM ingester.drain_jobs WHERE job_id=$1",
    )
    .bind(context.job_id)
    .fetch_one(&context.pool)
    .await
    .map_err(db_error)?;
    let chunks = sqlx::query_as::<_, Chunk>(
        "SELECT chunk.chunk_schema,chunk.chunk_name,chunk.range_start,chunk.range_end, \
         size.total_bytes::bigint AS size_bytes \
         FROM timescaledb_information.chunks chunk \
         JOIN chunks_detailed_size($3::regclass) size \
         ON size.chunk_schema=chunk.chunk_schema AND size.chunk_name=chunk.chunk_name \
         WHERE chunk.hypertable_schema=$1 AND chunk.hypertable_name=$2 \
         ORDER BY chunk.range_start,chunk.chunk_name",
    )
    .bind(spec.schema)
    .bind(spec.table)
    .bind(spec.relation)
    .fetch_all(&context.pool)
    .await
    .map_err(db_error)?;
    let copyable: Vec<_> = chunks
        .iter()
        .filter(|chunk| chunk.range_end <= snapshot_at)
        .cloned()
        .collect();
    let eligible = copyable
        .iter()
        .filter(|chunk| chunk.range_end <= request.cutoff)
        .count();
    let eligible_chunks = copyable
        .iter()
        .filter(|chunk| chunk.range_end <= request.cutoff)
        .map(|chunk| {
            json!({
                "schema": chunk.chunk_schema,
                "name": chunk.chunk_name,
                "start": chunk.range_start,
                "end": chunk.range_end,
            })
        })
        .collect::<Vec<_>>();
    let removable = if request.mode.removes_source_data() {
        eligible
    } else {
        0
    };
    if request.dry_run {
        return Ok(DrainOutcome {
            rows_exported: 0,
            rows_removed: 0,
            objects_published: 0,
            bytes_written: 0,
            summary: json!({
                "copyable_closed_chunks": copyable.len(),
                "cutoff": request.cutoff,
                "eligible_chunks": eligible,
                "eligible_chunk_ranges": eligible_chunks,
                "mode": request.mode,
                "open_chunks": chunks.len() - copyable.len(),
                "relation": spec.relation,
                "retained_chunks": chunks.len() - removable,
                "retention_days": spec.retention_days,
                "snapshot_at": snapshot_at,
            }),
        });
    }
    let work_chunks = chunks_to_process(copyable, request.mode, request.cutoff);
    preflight_lake_root(
        adapter.root(),
        work_chunks
            .iter()
            .map(|chunk| chunk.size_bytes)
            .max()
            .unwrap_or(0),
    )
    .await?;
    fs::create_dir_all(adapter.root().join(".staging"))
        .await
        .map_err(super::common::io_error)?;
    let mut outcome = DrainOutcome {
        rows_exported: 0,
        rows_removed: 0,
        objects_published: 0,
        bytes_written: 0,
        summary: json!({}),
    };
    let archived = archived_publications(&context, spec.key).await?;
    let mut verified_existing_objects = 0usize;
    let mut objects_created = 0usize;
    let mut chunks_removed = 0usize;
    for chunk in work_chunks {
        if context.shutdown.is_cancelled() {
            return Err(DrainExecutionError::new(
                "drain_cancelled",
                "drain was cancelled",
                true,
            ));
        }
        let publication = match existing_publication(&context, spec.key, &chunk).await? {
            Some(publication) => {
                let source_count = adapter.source_count(&context, &chunk).await?;
                let verified = verify_existing(adapter.root(), &publication).await;
                if should_republish(source_count, publication.row_count, verified.is_ok()) {
                    preflight_lake_root(adapter.root(), chunk.size_bytes).await?;
                    reset_publication(&context, &publication).await?;
                    objects_created += 1;
                    adapter.export_chunk(&context, &chunk).await?
                } else {
                    verified?;
                    verified_existing_objects += 1;
                    publication
                }
            }
            None => {
                preflight_lake_root(adapter.root(), chunk.size_bytes).await?;
                objects_created += 1;
                adapter.export_chunk(&context, &chunk).await?
            }
        };
        outcome.rows_exported += publication.row_count;
        outcome.objects_published += 1;
        outcome.bytes_written += publication.byte_size;
        if request.mode.removes_source_data() && chunk.range_end <= request.cutoff {
            outcome.rows_removed += remove_verified_chunk(&context, &publication).await?;
            chunks_removed += 1;
            tokio::select! {
                _ = context.shutdown.cancelled() => return Err(cancelled()),
                _ = tokio::time::sleep(CHUNK_REMOVAL_PACING) => {}
            }
        }
    }
    outcome.summary = json!({
        "cutoff": request.cutoff,
        "database_retained_from": chunks.iter().filter(|chunk| !request.mode.removes_source_data() || chunk.range_end > request.cutoff).map(|chunk| chunk.range_start).min(),
        "live_tail_from": chunks.iter().filter(|chunk| chunk.range_end > snapshot_at).map(|chunk| chunk.range_start).min(),
        "mode": request.mode,
        "objects_created": objects_created,
        "chunks_removed": chunks_removed,
        "relation": spec.relation,
        "retention_days": spec.retention_days,
        "snapshot_at": snapshot_at,
        "ssd_complete_from": archived.iter().map(|item| item.source_start).min().or_else(|| chunks.iter().filter(|chunk| chunk.range_end <= snapshot_at).map(|chunk| chunk.range_start).min()),
        "ssd_complete_through": chunks.iter().filter(|chunk| chunk.range_end <= snapshot_at).map(|chunk| chunk.range_end).max().or_else(|| archived.iter().map(|item| item.source_end).max()),
        "verified_existing_objects": verified_existing_objects,
    });
    Ok(outcome)
}

fn should_republish(source_count: Option<i64>, published_count: i64, file_valid: bool) -> bool {
    source_count.is_some_and(|count| count != published_count || !file_valid)
}

fn chunks_to_process(
    chunks: Vec<Chunk>,
    mode: DrainMode,
    cutoff: chrono::DateTime<Utc>,
) -> Vec<Chunk> {
    chunks
        .into_iter()
        .filter(|chunk| !mode.removes_source_data() || chunk.range_end <= cutoff)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{chunks_to_process, should_republish, Chunk, DrainMode};
    use chrono::{Duration, TimeZone, Utc};

    #[test]
    fn stale_or_missing_publication_must_be_exported_again() {
        assert!(should_republish(Some(101), 100, true));
        assert!(should_republish(Some(100), 100, false));
        assert!(!should_republish(Some(100), 100, true));
        assert!(!should_republish(None, 100, true));
    }

    #[test]
    fn drain_stops_at_cutoff_while_reconcile_keeps_closed_chunks() {
        let cutoff = Utc.with_ymd_and_hms(2026, 9, 11, 0, 0, 0).unwrap();
        let chunks = (0..3)
            .map(|day| Chunk {
                chunk_schema: "polymarket".into(),
                chunk_name: format!("chunk_{day}"),
                range_start: cutoff + Duration::days(day - 1),
                range_end: cutoff + Duration::days(day),
                size_bytes: 1,
            })
            .collect::<Vec<_>>();

        let drain = chunks_to_process(chunks.clone(), DrainMode::Drain, cutoff);
        assert_eq!(drain.len(), 1);
        assert_eq!(drain[0].range_end, cutoff);
        assert_eq!(
            chunks_to_process(chunks, DrainMode::Reconcile, cutoff).len(),
            3
        );
    }
}

async fn remove_verified_chunk(
    context: &DrainContext,
    publication: &Publication,
) -> Result<i64, DrainExecutionError> {
    let mut delay = CHUNK_REMOVAL_RETRY_INITIAL;
    for attempt in 0..CHUNK_REMOVAL_RETRY_LIMIT {
        let removal = remove_verified_chunk_once(context, publication);
        let result = tokio::select! {
            _ = context.shutdown.cancelled() => return Err(cancelled()),
            result = removal => result,
        };
        match result {
            Ok(rows) => return Ok(rows),
            Err(error) if is_lock_contention(&error) && attempt + 1 < CHUNK_REMOVAL_RETRY_LIMIT => {
                tokio::select! {
                    _ = context.shutdown.cancelled() => return Err(cancelled()),
                    _ = tokio::time::sleep(delay) => {}
                }
                delay = (delay * 2).min(CHUNK_REMOVAL_RETRY_MAX);
            }
            Err(error) if is_lock_contention(&error) => {
                return Err(DrainExecutionError::new(
                    "drain_chunk_lock_contended",
                    "realtime database activity prevented safe chunk removal; retrying the drain later",
                    true,
                ));
            }
            Err(error) => return Err(db_error(error)),
        }
    }
    unreachable!("chunk removal retry loop returns on its final attempt")
}

async fn remove_verified_chunk_once(
    context: &DrainContext,
    publication: &Publication,
) -> Result<i64, sqlx::Error> {
    let mut transaction = context.pool.begin().await?;
    sqlx::query(
        "SELECT set_config('lock_timeout',$1,true),set_config('statement_timeout',$2,true)",
    )
    .bind(CHUNK_REMOVAL_LOCK_TIMEOUT)
    .bind(CHUNK_REMOVAL_STATEMENT_TIMEOUT)
    .execute(&mut *transaction)
    .await?;
    let rows =
        sqlx::query_scalar::<_, i64>("SELECT ingester.remove_verified_drain_chunk($1,$2,$3)")
            .bind(publication.object_id)
            .bind(&publication.sha256)
            .bind(context.job_id)
            .fetch_one(&mut *transaction)
            .await?;
    sqlx::query_scalar::<_, i64>(
        "UPDATE ingester.drain_jobs SET rows_removed=rows_removed+$3,updated_at=clock_timestamp() WHERE job_id=$1 AND lease_token=$2 AND status='running' AND cancel_requested_at IS NULL RETURNING rows_removed",
    )
    .bind(context.job_id)
    .bind(context.lease_token)
    .bind(rows)
    .fetch_one(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(rows)
}

fn is_lock_contention(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .and_then(|database| database.code())
        .is_some_and(|code| code == "55P03" || code == "57014")
}

fn cancelled() -> DrainExecutionError {
    DrainExecutionError::new("drain_cancelled", "drain was cancelled", true)
}

pub async fn execute_strategy(
    adapter: &dyn RetainedDrainAdapter,
    context: DrainContext,
    request: DrainRequest,
) -> Result<DrainOutcome, DrainExecutionError> {
    execute(adapter, context, request).await
}

pub fn validate_strategy(
    adapter: &dyn RetainedDrainAdapter,
    request: &DrainRequest,
) -> Result<(), DrainExecutionError> {
    validate(adapter, request)
}
