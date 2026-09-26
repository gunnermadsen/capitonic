//! KLGA HRRR environmental features through the shared backfill worker.

use std::{path::PathBuf, sync::Arc, time::Duration};

use async_trait::async_trait;
use chrono::{DateTime, Datelike, Duration as ChronoDuration, NaiveDate, TimeZone, Timelike, Utc};
use chrono_tz::America::New_York;
use reqwest::{header::RANGE, Client, StatusCode};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::fs;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use crate::{
    domain::{
        BackfillContext, BackfillExecutionError, BackfillFailureKind, BackfillOutcome,
        BackfillRequest, BackfillShard, BackfillWorkerStrategy, StrategyDescriptor,
        ValidatedBackfillRequest,
    },
    strategies::{backfill_support, raw_archive},
};

use super::hrrr_grib::{self, DecodedPatch};

pub const STRATEGY_KEY: &str = "hrrr_environment_features_backfill";
const VERSION: &str = "hrrr-environment-klga-v2";
const PROCESS_ID: &str = "d48e2b47-18df-4c8c-95b9-0f12e6ca7d41";
const MAX_SUBSET_BYTES: usize = 64 * 1024 * 1024;

pub struct HrrrEnvironmentFeaturesBackfill {
    descriptor: StrategyDescriptor,
    client: Client,
}

impl HrrrEnvironmentFeaturesBackfill {
    pub fn new() -> Result<Self, BackfillExecutionError> {
        let descriptor = StrategyDescriptor {
            strategy_key: Arc::from(STRATEGY_KEY),
            name: Arc::from("HRRR KLGA environmental features"),
            description: Arc::from(
                "Decodes NOAA HRRR surface fields into the established KLGA v2 feature table",
            ),
            capabilities: vec![crate::domain::StrategyCapability::Backfill],
            strategy_contract_version: 1,
            request_schema_version: Some(1),
            shardable: true,
            maximum_shards: 17520,
        };
        descriptor.validate()?;
        Ok(Self {
            descriptor,
            client: raw_archive::client()?,
        })
    }
}

#[async_trait]
impl BackfillWorkerStrategy for HrrrEnvironmentFeaturesBackfill {
    fn descriptor(&self) -> &StrategyDescriptor {
        &self.descriptor
    }

    fn validate_request(
        &self,
        request: &BackfillRequest,
    ) -> Result<ValidatedBackfillRequest, BackfillExecutionError> {
        let valid = backfill_support::validate_empty_request(&self.descriptor, request)?;
        if valid.range_start < Utc.with_ymd_and_hms(2014, 7, 30, 0, 0, 0).unwrap() {
            return Err(invalid("HRRR archive starts on 2014-07-30"));
        }
        Ok(valid)
    }

    fn plan_shards(
        &self,
        request: &ValidatedBackfillRequest,
    ) -> Result<Vec<BackfillShard>, BackfillExecutionError> {
        let first = request.range_start.with_timezone(&New_York).date_naive();
        let last = (request.range_end - ChronoDuration::nanoseconds(1))
            .with_timezone(&New_York)
            .date_naive();
        let mut day = first;
        let mut shards = Vec::new();
        while day <= last {
            for hour in [0, 12] {
                let local = New_York
                    .with_ymd_and_hms(day.year(), day.month(), day.day(), hour, 0, 0)
                    .single()
                    .ok_or_else(|| invalid("NYC decision time was ambiguous"))?;
                let decision = local.with_timezone(&Utc);
                if decision < request.range_start || decision >= request.range_end {
                    continue;
                }
                let run = model_run(decision);
                let end = local_end(day)?;
                let mut valid = decision;
                while valid < end {
                    let lead = (valid - run).num_hours();
                    shards.push(BackfillShard {
                        shard_key: format!("decision-{}-valid-{}", decision.format("%Y%m%dT%H%MZ"), valid.format("%Y%m%dT%H%MZ")),
                        range_start: valid, range_end: valid + ChronoDuration::hours(1),
                        parameters: json!({"decision_time":decision,"model_run":run,"valid_at":valid,"lead_hours":lead}),
                    });
                    if shards.len() > self.descriptor.maximum_shards {
                        return Err(invalid("HRRR request exceeds the shard limit"));
                    }
                    valid += ChronoDuration::hours(1);
                }
            }
            day = day
                .succ_opt()
                .ok_or_else(|| invalid("HRRR date overflow"))?;
        }
        Ok(shards)
    }

    async fn execute_backfill(
        &self,
        context: BackfillContext,
        shard: BackfillShard,
    ) -> Result<BackfillOutcome, BackfillExecutionError> {
        let decision: DateTime<Utc> = parameter_time(&shard, "decision_time")?;
        let run: DateTime<Utc> = parameter_time(&shard, "model_run")?;
        let valid: DateTime<Utc> = parameter_time(&shard, "valid_at")?;
        let lead = (valid - run).num_hours();
        if valid != shard.range_start || !(0..=48).contains(&lead) || run != model_run(decision) {
            return Err(invalid(
                "HRRR shard parameters do not match the frozen window",
            ));
        }
        let logical_key = format!(
            "hrrr-environment:{}:f{lead:02}:{VERSION}",
            run.format("%Y%m%dT%H")
        );
        if let Some(outcome) =
            backfill_support::completed_outcome(&context, STRATEGY_KEY, &logical_key).await?
        {
            return Ok(outcome);
        }
        let process_id = Uuid::parse_str(PROCESS_ID).map_err(|e| integrity(e.to_string()))?;
        let existing_status: Option<String> = sqlx::query_scalar(
            "SELECT status FROM weather.hrrr_environment_window_coverage WHERE process_id=$1 AND station_id='KLGA' AND decision_time=$2 AND valid_at=$3 AND feature_schema_version=$4",
        )
        .bind(process_id).bind(decision).bind(valid).bind(VERSION)
        .fetch_optional(&context.pool).await.map_err(backfill_support::database_error)?;
        if existing_status.as_deref().is_some_and(|status| {
            !matches!(
                status,
                "missing_source" | "download_failure" | "processing_failure"
            )
        }) {
            return Ok(BackfillOutcome {
                records_verified: 0,
                verified_coverage: json!({"reused_coverage":true,"decision_time":decision,"valid_at":valid}),
                summary: json!({"status":"reused_coverage","source_status":existing_status}),
            });
        }
        let source_uri = source_uri(run, lead);
        let bytes = fetch_subset(&self.client, &source_uri, &context).await?;
        let (bytes, decoded) = tokio::task::spawn_blocking(move || {
            let decoded = hrrr_grib::decode(&bytes)?;
            Ok::<_, String>((bytes, decoded))
        })
        .await
        .map_err(|e| integrity(e.to_string()))?
        .map_err(integrity)?;
        if decoded.fields_present.len() != hrrr_grib::FIELDS.len() {
            return Err(integrity(format!(
                "HRRR subset missing fields: expected {:?}, got {:?}",
                hrrr_grib::FIELDS,
                decoded.fields_present
            )));
        }
        let fraction = decoded
            .summaries
            .get(&(100, "all"))
            .ok_or_else(|| integrity("missing HRRR 100 km summary"))?
            .values()
            .map(|s| s.valid_fraction)
            .fold(1.0, f64::min);
        if fraction < 0.5 {
            return Err(integrity(format!(
                "HRRR valid-pixel fraction {fraction} below 0.5"
            )));
        }
        let checksum = format!("{:x}", Sha256::digest(&bytes));
        let path = subset_path(run, lead)?;
        persist_subset(&path, &bytes, &checksum, &context).await?;
        let mut tx = context
            .pool
            .begin()
            .await
            .map_err(backfill_support::database_error)?;
        let artifact_id = backfill_support::create_artifact_in(
            &mut tx,
            &context,
            STRATEGY_KEY,
            STRATEGY_KEY,
            &logical_key,
            "noaa_hrrr_open_data",
            &source_uri,
            path.to_string_lossy().as_ref(),
        )
        .await?;
        let metadata = json!({
            "source_artifact_id":artifact_id,"units":decoded.units,
            "cropped_media_type":"application/x-grib2", "cropped_artifact_path":path,
            "cropped_artifact_sha256":checksum,
        });
        let status = if [
            "total_cloud_cover",
            "accumulated_precipitation",
            "composite_reflectivity",
        ]
        .iter()
        .all(|name| {
            decoded
                .summaries
                .get(&(25, "all"))
                .and_then(|m| m.get(name))
                .and_then(|s| s.mean)
                == Some(0.0)
        }) {
            "valid_zero"
        } else {
            "complete"
        };
        sqlx::query(r#"
            INSERT INTO weather.hrrr_environment_window_coverage (
              process_id,station_id,decision_time,model_run,valid_at,feature_schema_version,
              status,fields_present,fields_missing,source_artifact_id,cropped_artifact_path,
              cropped_artifact_sha256,valid_pixel_fraction,quality_flags,source_metadata
            ) VALUES ($1,'KLGA',$2,$3,$4,$5,$6,$7,ARRAY[]::text[],$8,$9,$10,$11,'{}'::jsonb,$12)
            ON CONFLICT (process_id,station_id,decision_time,valid_at,feature_schema_version)
            DO UPDATE SET status=EXCLUDED.status,fields_present=EXCLUDED.fields_present,
              fields_missing=EXCLUDED.fields_missing,source_artifact_id=EXCLUDED.source_artifact_id,
              cropped_artifact_path=EXCLUDED.cropped_artifact_path,
              cropped_artifact_sha256=EXCLUDED.cropped_artifact_sha256,
              valid_pixel_fraction=EXCLUDED.valid_pixel_fraction,
              source_metadata=EXCLUDED.source_metadata,checked_at=now()
            WHERE weather.hrrr_environment_window_coverage.status IN ('missing_source','download_failure','processing_failure')
        "#)
        .bind(process_id).bind(decision).bind(run).bind(valid).bind(VERSION).bind(status)
        .bind(&decoded.fields_present).bind(artifact_id).bind(path.to_string_lossy().as_ref())
        .bind(&checksum).bind(fraction).bind(&metadata)
        .execute(&mut *tx).await.map_err(backfill_support::database_error)?;
        insert_features(
            &mut tx,
            process_id,
            decision,
            run,
            valid,
            lead as i32,
            &decoded,
            &metadata,
        )
        .await?;
        let persisted: i64 = sqlx::query_scalar("SELECT count(*) FROM weather.hrrr_environment_features WHERE process_id=$1 AND station_id='KLGA' AND decision_time=$2 AND valid_at=$3 AND feature_schema_version=$4")
            .bind(process_id).bind(decision).bind(valid).bind(VERSION)
            .fetch_one(&mut *tx).await.map_err(backfill_support::database_error)?;
        if persisted != 16 {
            return Err(integrity(format!(
                "HRRR feature row count {persisted} != 16"
            )));
        }
        backfill_support::complete_artifact(
            &mut tx,
            &context,
            backfill_support::ArtifactCompletion {
                artifact_id,
                checksum: &checksum,
                byte_size: bytes.len() as u64,
                record_count: persisted,
                minimum: Some(valid),
                maximum: Some(valid + ChronoDuration::hours(1)),
                metadata: json!({"fields":decoded.fields_present,"units":decoded.units,
                "cropped_media_type":"application/x-grib2", "cropped_artifact_path":path,
                "cropped_artifact_sha256":checksum,"feature_schema_version":VERSION}),
            },
        )
        .await?;
        tx.commit()
            .await
            .map_err(backfill_support::database_error)?;
        Ok(BackfillOutcome {
            records_verified: persisted,
            verified_coverage: json!({"minimum_source_timestamp":valid,"maximum_source_timestamp":valid+ChronoDuration::hours(1),"records_verified":persisted}),
            summary: json!({"status":status,"source_sha256":checksum,"cropped_artifact_path":path,"fields_present":decoded.fields_present}),
        })
    }
}

fn model_run(decision: DateTime<Utc>) -> DateTime<Utc> {
    let available = decision - ChronoDuration::minutes(75);
    let hour = available.hour() - available.hour() % 6;
    available
        .with_hour(hour)
        .unwrap()
        .with_minute(0)
        .unwrap()
        .with_second(0)
        .unwrap()
        .with_nanosecond(0)
        .unwrap()
}

fn local_end(day: NaiveDate) -> Result<DateTime<Utc>, BackfillExecutionError> {
    let next = day
        .succ_opt()
        .ok_or_else(|| invalid("HRRR date overflow"))?;
    New_York
        .with_ymd_and_hms(next.year(), next.month(), next.day(), 0, 0, 0)
        .single()
        .map(|v| v.with_timezone(&Utc))
        .ok_or_else(|| invalid("NYC midnight was ambiguous"))
}

fn parameter_time(
    shard: &BackfillShard,
    key: &str,
) -> Result<DateTime<Utc>, BackfillExecutionError> {
    shard
        .parameters
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| invalid(format!("HRRR shard missing {key}")))?
        .parse()
        .map_err(|_| invalid(format!("HRRR shard has invalid {key}")))
}

fn source_uri(run: DateTime<Utc>, lead: i64) -> String {
    format!("https://noaa-hrrr-bdp-pds.s3.amazonaws.com/hrrr.{}/conus/hrrr.t{:02}z.wrfsfcf{lead:02}.grib2",
        run.format("%Y%m%d"),run.hour())
}

fn subset_path(run: DateTime<Utc>, lead: i64) -> Result<PathBuf, BackfillExecutionError> {
    Ok(raw_archive::root()?.join(format!(
        "noaa-hrrr/environment-subsets/{}/{:02}/hrrr-{}-f{lead:02}-{VERSION}.grib2",
        run.format("%Y%m%d"),
        run.hour(),
        run.format("%Y%m%dT%H%MZ")
    )))
}

fn select_ranges(index: &str) -> Result<Vec<(u64, u64)>, BackfillExecutionError> {
    let mut entries = Vec::new();
    for line in index.lines() {
        let mut parts = line.split(':');
        let _number = parts.next();
        let Some(offset) = parts.next().and_then(|v| v.parse::<u64>().ok()) else {
            continue;
        };
        entries.push((offset, line));
    }
    if entries.len() < 2 {
        return Err(integrity("HRRR index had too few offsets"));
    }
    let mut selected = Vec::new();
    for pair in entries.windows(2) {
        let (start, line) = pair[0];
        let end = pair[1]
            .0
            .checked_sub(1)
            .ok_or_else(|| integrity("HRRR index offsets reversed"))?;
        if [
            ":TMP:2 m above ground:",
            ":DPT:2 m above ground:",
            ":UGRD:10 m above ground:",
            ":VGRD:10 m above ground:",
            ":TCDC:entire atmosphere:",
            ":DSWRF:surface:",
            ":HPBL:surface:",
            ":APCP:surface:",
            ":REFC:entire atmosphere:",
        ]
        .iter()
        .any(|v| line.contains(v))
        {
            selected.push((start, end));
        }
    }
    if selected.len() < 9 {
        return Err(integrity("HRRR index omitted required feature fields"));
    }
    Ok(selected)
}

async fn fetch_subset(
    client: &Client,
    source_uri: &str,
    context: &BackfillContext,
) -> Result<Vec<u8>, BackfillExecutionError> {
    let index = tokio::select! {
        _ = context.shutdown.cancelled() => return Err(cancelled()),
        result = client.get(format!("{source_uri}.idx")).timeout(Duration::from_secs(30)).send() => result
    }.map_err(source_error)?.error_for_status().map_err(source_error)?.text().await.map_err(source_error)?;
    let ranges = select_ranges(&index)?;
    let total: u64 = ranges.iter().map(|(a, b)| b - a + 1).sum();
    if total > MAX_SUBSET_BYTES as u64 {
        return Err(integrity("HRRR selected subset exceeds 64 MiB"));
    }
    let mut bytes = Vec::with_capacity(total as usize);
    for (start, end) in ranges {
        let response = tokio::select! {
            _ = context.shutdown.cancelled() => return Err(cancelled()),
            result = client.get(source_uri).header(RANGE,format!("bytes={start}-{end}"))
                .timeout(Duration::from_secs(60)).send() => result
        }
        .map_err(source_error)?;
        if response.status() != StatusCode::PARTIAL_CONTENT {
            return Err(integrity("HRRR source ignored byte range"));
        }
        let part = response.bytes().await.map_err(source_error)?;
        if part.len() != (end - start + 1) as usize
            || !part.starts_with(b"GRIB")
            || !part.ends_with(b"7777")
        {
            return Err(integrity(
                "HRRR range response length or GRIB framing mismatch",
            ));
        }
        bytes.extend_from_slice(&part);
    }
    Ok(bytes)
}

async fn persist_subset(
    path: &PathBuf,
    bytes: &[u8],
    checksum: &str,
    context: &BackfillContext,
) -> Result<(), BackfillExecutionError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await.map_err(io_error)?;
    }
    if path.exists() {
        let existing = fs::read(path).await.map_err(io_error)?;
        if format!("{:x}", Sha256::digest(&existing)) != checksum {
            return Err(integrity(
                "HRRR source artifact path has conflicting content",
            ));
        }
        return Ok(());
    }
    let partial = path.with_extension(format!("{}.partial", context.lease_token));
    let mut file = fs::File::create(&partial).await.map_err(io_error)?;
    file.write_all(bytes).await.map_err(io_error)?;
    file.sync_all().await.map_err(io_error)?;
    drop(file);
    fs::rename(&partial, path).await.map_err(io_error)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn insert_features(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    process_id: Uuid,
    decision: DateTime<Utc>,
    run: DateTime<Utc>,
    valid: DateTime<Utc>,
    lead: i32,
    patch: &DecodedPatch,
    metadata: &Value,
) -> Result<i64, BackfillExecutionError> {
    let mut inserted = 0;
    for ((radius, sector), fields) in &patch.summaries {
        let get = |name: &str| fields.get(name);
        let mean = |name: &str| get(name).and_then(|v| v.mean);
        let std = |name: &str| get(name).and_then(|v| v.stddev);
        let u = mean("wind_u_10m");
        let v = mean("wind_v_10m");
        let wind = u.zip(v).map(|(a, b)| a.hypot(b));
        let gradient = |a: &str, b: &str| -> Option<f64> {
            if *radius == 0 {
                return None;
            }
            let first = patch
                .summaries
                .get(&(*radius, a))
                .and_then(|m| m.get("temperature_2m"))
                .and_then(|s| s.mean);
            let second = patch
                .summaries
                .get(&(*radius, b))
                .and_then(|m| m.get("temperature_2m"))
                .and_then(|s| s.mean);
            first.zip(second).map(|(x, y)| x - y)
        };
        let fraction = fields
            .values()
            .map(|s| s.valid_fraction)
            .fold(1.0, f64::min);
        let result = sqlx::query(r#"
            INSERT INTO weather.hrrr_environment_features (
              process_id,station_id,decision_time,model_run,valid_at,lead_hours,
              spatial_radius_km,sector,feature_schema_version,temperature_2m_mean_k,
              temperature_2m_stddev_k,dew_point_2m_mean_k,dew_point_2m_stddev_k,
              total_cloud_cover_mean_fraction,total_cloud_cover_stddev_fraction,
              downward_shortwave_radiation_mean_w_m2,wind_u_10m_mean_m_s,
              wind_v_10m_mean_m_s,wind_speed_10m_mean_m_s,boundary_layer_height_mean_m,
              accumulated_precipitation_mean_mm,composite_reflectivity_mean_dbz,
              composite_reflectivity_max_dbz,valid_pixel_fraction,
              temperature_2m_north_south_gradient_k,temperature_2m_east_west_gradient_k,
              quality_flags,source_metadata
            ) VALUES ($1,'KLGA',$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23,$24,$25,'{}'::jsonb,$26)
            ON CONFLICT DO NOTHING
        "#)
        .bind(process_id).bind(decision).bind(run).bind(valid).bind(lead)
        .bind(*radius as i32).bind(*sector).bind(VERSION)
        .bind(mean("temperature_2m")).bind(std("temperature_2m"))
        .bind(mean("dew_point_2m")).bind(std("dew_point_2m"))
        .bind(mean("total_cloud_cover")).bind(std("total_cloud_cover"))
        .bind(mean("downward_shortwave_radiation")).bind(u).bind(v).bind(wind)
        .bind(mean("boundary_layer_height")).bind(mean("accumulated_precipitation"))
        .bind(mean("composite_reflectivity")).bind(get("composite_reflectivity").and_then(|s| s.maximum))
        .bind(fraction).bind(gradient("north","south")).bind(gradient("east","west"))
        .bind(metadata).execute(&mut **tx).await.map_err(backfill_support::database_error)?;
        inserted += result.rows_affected() as i64;
    }
    Ok(inserted)
}

fn invalid(message: impl Into<String>) -> BackfillExecutionError {
    BackfillExecutionError::invalid("hrrr_environment_invalid", message)
}
fn integrity(message: impl Into<String>) -> BackfillExecutionError {
    BackfillExecutionError::new(
        BackfillFailureKind::Integrity,
        "hrrr_environment_integrity",
        message,
    )
}
fn source_error(error: impl std::fmt::Display) -> BackfillExecutionError {
    BackfillExecutionError::new(
        BackfillFailureKind::TransientSource,
        "hrrr_environment_source",
        error.to_string(),
    )
}
fn io_error(error: impl std::fmt::Display) -> BackfillExecutionError {
    BackfillExecutionError::new(
        BackfillFailureKind::TransientDatabase,
        "hrrr_environment_storage",
        error.to_string(),
    )
}
fn cancelled() -> BackfillExecutionError {
    BackfillExecutionError::new(
        BackfillFailureKind::LeaseLost,
        "hrrr_environment_cancelled",
        "HRRR backfill lease was cancelled",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_selects_only_bounded_feature_messages() {
        let index = "1:0:d=2024010100:REFC:entire atmosphere:15 hour fcst:\n2:50:d=2024010100:TMP:2 m above ground:15 hour fcst:\n3:100:d=2024010100:DPT:2 m above ground:15 hour fcst:\n4:150:d=2024010100:UGRD:10 m above ground:15 hour fcst:\n5:200:d=2024010100:VGRD:10 m above ground:15 hour fcst:\n6:250:d=2024010100:APCP:surface:14-15 hour acc fcst:\n7:300:d=2024010100:TCDC:entire atmosphere:15 hour fcst:\n8:350:d=2024010100:DSWRF:surface:15 hour fcst:\n9:400:d=2024010100:HPBL:surface:15 hour fcst:\n10:450:d=2024010100:OTHER:surface:15 hour fcst:\n";
        assert_eq!(
            select_ranges(index).unwrap(),
            (0..9).map(|n| (n * 50, n * 50 + 49)).collect::<Vec<_>>()
        );
    }

    #[test]
    fn frozen_decision_run_uses_seventy_five_minute_lag() {
        let decision = Utc.with_ymd_and_hms(2024, 1, 1, 17, 0, 0).unwrap();
        assert_eq!(
            model_run(decision),
            Utc.with_ymd_and_hms(2024, 1, 1, 12, 0, 0).unwrap()
        );
    }

    #[test]
    fn decision_windows_follow_new_york_day_and_dst() {
        let strategy = HrrrEnvironmentFeaturesBackfill::new().unwrap();
        for (start, end, expected) in [
            ("2024-01-01T05:00:00Z", "2024-01-02T05:00:00Z", 36),
            ("2024-03-10T05:00:00Z", "2024-03-11T04:00:00Z", 35),
        ] {
            let request = BackfillRequest {
                strategy_key: STRATEGY_KEY.into(),
                range: crate::domain::BackfillRange {
                    start: start.parse().unwrap(),
                    end: end.parse().unwrap(),
                },
                parameters: json!({}),
                execution: Default::default(),
            };
            let validated = strategy.validate_request(&request).unwrap();
            let shards = strategy.plan_shards(&validated).unwrap();
            assert_eq!(shards.len(), expected);
            assert_eq!(
                shards
                    .iter()
                    .filter(|s| s.parameters["decision_time"] == json!(request.range.start))
                    .count(),
                if expected == 36 { 24 } else { 23 }
            );
        }
    }
}
