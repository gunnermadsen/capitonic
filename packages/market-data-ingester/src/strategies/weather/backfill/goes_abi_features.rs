use std::{
    collections::{btree_map::Entry, BTreeMap},
    path::{Path, PathBuf},
};

use async_trait::async_trait;
use chrono::{DateTime, Duration, NaiveDateTime, Timelike, Utc};
use chrono_tz::America::New_York;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::{Postgres, QueryBuilder};
use tokio::io::AsyncReadExt;

use crate::{
    domain::{
        BackfillContext, BackfillExecutionError, BackfillFailureKind, BackfillOutcome,
        BackfillRequest, BackfillShard, BackfillWorkerStrategy, StrategyDescriptor,
        ValidatedBackfillRequest,
    },
    strategies::{
        backfill_support,
        raw_archive::{self, RawObject},
        weather::raw_support,
    },
};

use super::goes_abi_decode::{self, Patch, Summary};

pub const STRATEGY_KEY: &str = "goes_abi_features_backfill";
const PROCESS_ID: &str = "d48e2b47-18df-4c8c-95b9-0f12e6ca7d41";
const STATION_ID: &str = "KLGA";
const SCHEMA_VERSION: &str = "goes-abi-klga-v2";
const RADII: [i32; 3] = [25, 50, 100];
const SECTORS: [&str; 5] = ["all", "north", "south", "east", "west"];
const OFFSETS: [i32; 3] = [180, 60, 15];
const TRANSITION: &str = "2025-04-07T15:10:00Z";

type CoverageReceipt = (
    String,
    Option<DateTime<Utc>>,
    Option<DateTime<Utc>>,
    Option<f64>,
    Option<uuid::Uuid>,
    Option<String>,
);

struct Product {
    key: &'static str,
    archive: &'static str,
    channel: Option<&'static str>,
    variables: &'static [&'static str],
    required: bool,
    noon_only: bool,
}

const PRODUCTS: [Product; 7] = [
    Product {
        key: "infrared_c13",
        archive: "ABI-L2-CMIPC",
        channel: Some("C13"),
        variables: &["CMI", "Rad"],
        required: true,
        noon_only: false,
    },
    Product {
        key: "clear_sky_mask",
        archive: "ABI-L2-ACMC",
        channel: None,
        variables: &["BCM", "ACM"],
        required: true,
        noon_only: false,
    },
    Product {
        key: "cloud_top_temperature",
        archive: "ABI-L2-ACHTF",
        channel: None,
        variables: &["TEMP"],
        required: true,
        noon_only: false,
    },
    Product {
        key: "cloud_top_height",
        archive: "ABI-L2-ACHAC",
        channel: None,
        variables: &["HT"],
        required: true,
        noon_only: false,
    },
    Product {
        key: "visible_c02",
        archive: "ABI-L2-CMIPC",
        channel: Some("C02"),
        variables: &["CMI", "Rad"],
        required: true,
        noon_only: true,
    },
    Product {
        key: "cloud_optical_depth",
        archive: "ABI-L2-CODC",
        channel: None,
        variables: &["COD"],
        required: false,
        noon_only: false,
    },
    Product {
        key: "water_vapor_c08",
        archive: "ABI-L2-CMIPC",
        channel: Some("C08"),
        variables: &["CMI", "Rad"],
        required: false,
        noon_only: false,
    },
];

pub struct GoesAbiFeaturesBackfill {
    descriptor: StrategyDescriptor,
    client: reqwest::Client,
}

impl GoesAbiFeaturesBackfill {
    pub fn new() -> Result<Self, BackfillExecutionError> {
        Ok(Self {
            descriptor: backfill_support::descriptor(
                STRATEGY_KEY,
                "GOES ABI KLGA features",
                "Collects causal GOES ABI scans and persists the existing KLGA feature schema",
            )?,
            client: raw_archive::client()?,
        })
    }
}

#[async_trait]
impl BackfillWorkerStrategy for GoesAbiFeaturesBackfill {
    fn descriptor(&self) -> &StrategyDescriptor {
        &self.descriptor
    }

    fn validate_request(
        &self,
        request: &BackfillRequest,
    ) -> Result<ValidatedBackfillRequest, BackfillExecutionError> {
        backfill_support::validate_empty_request(&self.descriptor, request)
    }

    fn plan_shards(
        &self,
        request: &ValidatedBackfillRequest,
    ) -> Result<Vec<BackfillShard>, BackfillExecutionError> {
        backfill_support::daily_shards(request, self.descriptor.maximum_shards)
    }

    async fn execute_backfill(
        &self,
        context: BackfillContext,
        shard: BackfillShard,
    ) -> Result<BackfillOutcome, BackfillExecutionError> {
        let mut count = 0i64;
        let mut decision = shard
            .range_start
            .with_minute(0)
            .and_then(|v| v.with_second(0))
            .and_then(|v| v.with_nanosecond(0))
            .ok_or_else(|| invalid("invalid decision timestamp"))?;
        if decision < shard.range_start {
            decision += Duration::hours(1);
        }
        while decision < shard.range_end {
            if matches!(decision.with_timezone(&New_York).hour(), 0 | 12) {
                count += self.process_decision(&context, decision).await?;
            }
            decision += Duration::hours(1);
        }
        Ok(backfill_support::outcome(
            count,
            &shard,
            json!({"station_id":STATION_ID,"feature_schema_version":SCHEMA_VERSION,"rows_verified":count}),
        ))
    }
}

struct ProductResult {
    key: &'static str,
    status: &'static str,
    scan_start: Option<DateTime<Utc>>,
    scan_end: Option<DateTime<Utc>>,
    artifact_id: Option<uuid::Uuid>,
    patch_path: Option<PathBuf>,
    patch_sha: Option<String>,
    patch: Option<Patch>,
    valid_fraction: Option<f64>,
}

impl GoesAbiFeaturesBackfill {
    async fn process_decision(
        &self,
        context: &BackfillContext,
        decision: DateTime<Utc>,
    ) -> Result<i64, BackfillExecutionError> {
        let satellite = if decision < transition() {
            "G16"
        } else {
            "G19"
        };
        let noon = decision.with_timezone(&New_York).hour() == 12;
        let mut catalog = BTreeMap::<DateTime<Utc>, Vec<RawObject>>::new();
        let mut prior_infrared = BTreeMap::<(i32, i32, &'static str), f64>::new();
        let mut verified = 0i64;
        for offset in OFFSETS {
            let target = decision - Duration::minutes(i64::from(offset));
            for hour in [
                target - Duration::hours(1),
                target,
                target + Duration::hours(1),
            ] {
                let start = hour
                    .with_minute(0)
                    .and_then(|v| v.with_second(0))
                    .and_then(|v| v.with_nanosecond(0))
                    .ok_or_else(|| invalid("invalid GOES catalog hour"))?;
                if let Entry::Vacant(entry) = catalog.entry(start) {
                    let shard = BackfillShard {
                        shard_key: start.to_rfc3339(),
                        range_start: start,
                        range_end: start + Duration::hours(1),
                        parameters: json!({}),
                    };
                    let objects = raw_support::goes_objects(&self.client, &shard).await?;
                    entry.insert(objects);
                }
            }
            let mut results = Vec::new();
            for product in PRODUCTS.iter().filter(|product| !product.noon_only || noon) {
                let selected = select_scan(
                    catalog.values().flatten(),
                    product,
                    target,
                    decision,
                    satellite,
                );
                let Some((object, scan_start, scan_end)) = selected else {
                    results.push(ProductResult {
                        key: product.key,
                        status: if (decision - transition()).num_hours().abs() < 24 {
                            "satellite_transition_issue"
                        } else {
                            "missing_source"
                        },
                        scan_start: None,
                        scan_end: None,
                        artifact_id: None,
                        patch_path: None,
                        patch_sha: None,
                        patch: None,
                        valid_fraction: None,
                    });
                    continue;
                };
                raw_archive::store(context, STRATEGY_KEY, &self.client, object).await?;
                let path = raw_archive::root()?.join(&object.relative_path);
                let artifact: (uuid::Uuid, String, i64) = sqlx::query_as("SELECT artifact_id,checksum,byte_size FROM ingester.backfill_artifacts WHERE strategy_key=$1 AND logical_key=$2 AND status='completed'")
                    .bind(STRATEGY_KEY).bind(&object.logical_key)
                    .fetch_one(&context.pool).await.map_err(backfill_support::database_error)?;
                verify_raw_artifact(&path, &artifact.1, artifact.2).await?;
                let product_key = product.key;
                let variables = product.variables.to_vec();
                let decoded_path = path.clone();
                let patch = tokio::task::spawn_blocking(move || {
                    goes_abi_decode::decode(
                        &decoded_path,
                        &variables,
                        product_key == "clear_sky_mask",
                    )
                })
                .await
                .map_err(|error| integrity(error.to_string()))?
                .map_err(integrity)?;
                let patch_path = patch_path(satellite, decision, offset, product.key)?;
                let patch_sha =
                    write_patch(&patch_path, &patch, product.key, product.archive).await?;
                let valid_fraction = patch
                    .summaries
                    .values()
                    .map(|summary| summary.valid_pixel_fraction)
                    .fold(0.0, f64::max);
                let status = if valid_fraction < 0.5 {
                    "insufficient_valid_pixels"
                } else if patch
                    .summaries
                    .values()
                    .filter_map(|summary| summary.mean)
                    .all(|value| value == 0.0)
                {
                    "valid_zero"
                } else {
                    "complete"
                };
                results.push(ProductResult {
                    key: product.key,
                    status,
                    scan_start: Some(scan_start),
                    scan_end: Some(scan_end),
                    artifact_id: Some(artifact.0),
                    patch_path: Some(patch_path),
                    patch_sha: Some(patch_sha),
                    patch: Some(patch),
                    valid_fraction: Some(valid_fraction),
                });
            }
            let cloudy_fraction = results
                .iter()
                .find(|result| result.key == "clear_sky_mask")
                .and_then(|result| result.patch.as_ref())
                .and_then(|patch| patch.summaries.get(&(100, "all")))
                .and_then(|summary| summary.cloudy_fraction);
            for result in &mut results {
                if !matches!(result.key, "cloud_top_temperature" | "cloud_top_height")
                    || !matches!(
                        result.status,
                        "complete" | "valid_zero" | "insufficient_valid_pixels"
                    )
                {
                    continue;
                }
                match cloudy_fraction {
                    Some(0.0) => {
                        result.valid_fraction = Some(1.0);
                        result.status = "valid_zero";
                    }
                    Some(fraction) if fraction > 0.0 => {
                        let raw = result
                            .patch
                            .as_ref()
                            .and_then(|patch| patch.summaries.get(&(100, "all")))
                            .map_or(0.0, |summary| summary.valid_pixel_fraction);
                        let adjusted = (raw / fraction).min(1.0);
                        result.valid_fraction = Some(adjusted);
                        result.status = if adjusted >= 0.5 {
                            "complete"
                        } else {
                            "insufficient_valid_pixels"
                        };
                    }
                    _ => {}
                }
            }
            let required_ready = PRODUCTS
                .iter()
                .filter(|product| product.required && (!product.noon_only || noon))
                .all(|product| {
                    results.iter().any(|result| {
                        result.key == product.key
                            && matches!(result.status, "complete" | "valid_zero")
                    })
                });
            verified += persist_offset(
                context,
                decision,
                offset,
                satellite,
                &results,
                required_ready,
                &mut prior_infrared,
            )
            .await?;
        }
        Ok(verified)
    }
}

fn transition() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(TRANSITION)
        .unwrap()
        .with_timezone(&Utc)
}

fn parse_scan_times(source_uri: &str) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
    let file = source_uri.rsplit('/').next()?;
    let start = file.split("_s").nth(1)?.get(..13)?;
    let end = file.split("_e").nth(1)?.get(..13)?;
    let parse = |value: &str| {
        NaiveDateTime::parse_from_str(value, "%Y%j%H%M%S")
            .ok()
            .map(|value| value.and_utc())
    };
    Some((parse(start)?, parse(end)?))
}

fn select_scan<'a>(
    objects: impl Iterator<Item = &'a RawObject>,
    product: &Product,
    target: DateTime<Utc>,
    decision: DateTime<Utc>,
    satellite: &str,
) -> Option<(&'a RawObject, DateTime<Utc>, DateTime<Utc>)> {
    objects
        .filter(|object| {
            object.source_uri.contains(product.archive)
                && object.source_uri.contains(&format!("_{satellite}_"))
                && product.channel.is_none_or(|channel| {
                    object
                        .source_uri
                        .contains(&format!("{channel}_{satellite}_"))
                })
        })
        .filter_map(|object| {
            parse_scan_times(&object.source_uri).map(|(start, end)| (object, start, end))
        })
        .filter(|(_, _, end)| {
            *end <= decision - Duration::minutes(15) && (*end - target).num_seconds().abs() <= 1800
        })
        .min_by_key(|(_, _, end)| (((*end - target).num_seconds().abs()), -end.timestamp()))
}

fn patch_path(
    satellite: &str,
    decision: DateTime<Utc>,
    offset: i32,
    product: &str,
) -> Result<PathBuf, BackfillExecutionError> {
    let root = PathBuf::from(
        std::env::var("INGESTER_GOES_PATCH_ROOT")
            .unwrap_or_else(|_| "/var/lib/weather/satellite/goes".into()),
    );
    if !root.is_absolute() {
        return Err(invalid("GOES patch root must be absolute"));
    }
    Ok(root
        .join(satellite.to_lowercase())
        .join(decision.format("%Y/%m/%d/%Y%m%dT%H%M%SZ").to_string())
        .join(format!("offset-{offset:03}-{product}.nc")))
}

async fn write_patch(
    path: &Path,
    patch: &Patch,
    product: &str,
    archive: &str,
) -> Result<String, BackfillExecutionError> {
    let bytes = patch.netcdf_bytes(product, archive).map_err(integrity)?;
    let hash = hex::encode(Sha256::digest(&bytes));
    let parent = path
        .parent()
        .ok_or_else(|| invalid("GOES patch path has no parent"))?;
    tokio::fs::create_dir_all(parent).await.map_err(source)?;
    if path.exists() {
        let existing = tokio::fs::read(path).await.map_err(source)?;
        if Sha256::digest(&existing) != Sha256::digest(&bytes)
            && !patch
                .matches_netcdf(path, product, archive)
                .map_err(integrity)?
        {
            return Err(integrity(format!(
                "existing GOES patch differs: {}",
                path.display()
            )));
        }
        return Ok(hex::encode(Sha256::digest(&existing)));
    }
    let partial = path.with_extension(format!("{}.partial", uuid::Uuid::new_v4()));
    tokio::fs::write(&partial, bytes).await.map_err(source)?;
    tokio::fs::rename(&partial, path).await.map_err(source)?;
    Ok(hash)
}

async fn verify_raw_artifact(
    path: &Path,
    checksum: &str,
    expected_bytes: i64,
) -> Result<(), BackfillExecutionError> {
    let mut file = tokio::fs::File::open(path).await.map_err(source)?;
    let mut hash = Sha256::new();
    let mut actual_bytes = 0i64;
    let mut buffer = [0u8; 128 * 1024];
    loop {
        let read = file.read(&mut buffer).await.map_err(source)?;
        if read == 0 {
            break;
        }
        actual_bytes += read as i64;
        hash.update(&buffer[..read]);
    }
    if actual_bytes != expected_bytes || hex::encode(hash.finalize()) != checksum {
        return Err(integrity(format!(
            "GOES raw artifact differs from its receipt: {}",
            path.display()
        )));
    }
    Ok(())
}

fn source(error: impl std::fmt::Display) -> BackfillExecutionError {
    BackfillExecutionError::new(
        BackfillFailureKind::TransientSource,
        "goes_source",
        error.to_string(),
    )
}
fn invalid(error: impl std::fmt::Display) -> BackfillExecutionError {
    BackfillExecutionError::invalid("goes_invalid", error.to_string())
}
fn integrity(error: impl std::fmt::Display) -> BackfillExecutionError {
    BackfillExecutionError::new(
        BackfillFailureKind::Integrity,
        "goes_integrity",
        error.to_string(),
    )
}

async fn persist_offset(
    context: &BackfillContext,
    decision: DateTime<Utc>,
    offset: i32,
    satellite: &str,
    results: &[ProductResult],
    required_ready: bool,
    prior_infrared: &mut BTreeMap<(i32, i32, &'static str), f64>,
) -> Result<i64, BackfillExecutionError> {
    let process_id = uuid::Uuid::parse_str(PROCESS_ID).map_err(invalid)?;
    let mut tx = context
        .pool
        .begin()
        .await
        .map_err(backfill_support::database_error)?;
    backfill_support::require_lease(&mut tx, context).await?;
    for result in results {
        let quality = result
            .patch
            .as_ref()
            .map(|patch| {
                Value::Object(
                    patch
                        .summaries
                        .iter()
                        .map(|((radius, sector), summary)| {
                            (format!("({radius}, '{sector}')"), quality_json(summary))
                        })
                        .collect(),
                )
            })
            .unwrap_or_else(|| json!({}));
        let metadata = result
            .patch
            .as_ref()
            .map(|patch| json!({"variable":patch.variable,"units":patch.units}))
            .unwrap_or_else(|| json!({}));
        sqlx::query(
            "INSERT INTO weather.goes_abi_window_coverage (process_id,station_id,decision_time,requested_offset_minutes,product,feature_schema_version,status,satellite,scan_start,scan_end,valid_pixel_fraction,source_artifact_id,cropped_artifact_path,cropped_artifact_sha256,quality_flags,source_metadata) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16) ON CONFLICT (process_id,station_id,decision_time,requested_offset_minutes,product,feature_schema_version) DO UPDATE SET status=EXCLUDED.status,satellite=EXCLUDED.satellite,scan_start=EXCLUDED.scan_start,scan_end=EXCLUDED.scan_end,valid_pixel_fraction=EXCLUDED.valid_pixel_fraction,source_artifact_id=EXCLUDED.source_artifact_id,cropped_artifact_path=EXCLUDED.cropped_artifact_path,cropped_artifact_sha256=EXCLUDED.cropped_artifact_sha256,quality_flags=EXCLUDED.quality_flags,source_metadata=EXCLUDED.source_metadata,checked_at=now() WHERE weather.goes_abi_window_coverage.status IN ('missing_source','satellite_transition_issue','download_failure','processing_failure')",
        )
        .bind(process_id).bind(STATION_ID).bind(decision).bind(offset).bind(result.key)
        .bind(SCHEMA_VERSION).bind(result.status).bind(satellite).bind(result.scan_start)
        .bind(result.scan_end).bind(result.valid_fraction).bind(result.artifact_id)
        .bind(result.patch_path.as_ref().map(|path| path.to_string_lossy().to_string()))
        .bind(&result.patch_sha).bind(quality).bind(metadata)
        .execute(&mut *tx).await.map_err(backfill_support::database_error)?;
        let stored: CoverageReceipt = sqlx::query_as(
            "SELECT status,scan_start,scan_end,valid_pixel_fraction,source_artifact_id,cropped_artifact_sha256 FROM weather.goes_abi_window_coverage WHERE process_id=$1 AND station_id=$2 AND decision_time=$3 AND requested_offset_minutes=$4 AND product=$5 AND feature_schema_version=$6",
        )
        .bind(process_id).bind(STATION_ID).bind(decision).bind(offset).bind(result.key).bind(SCHEMA_VERSION)
        .fetch_one(&mut *tx).await.map_err(backfill_support::database_error)?;
        if stored.0 != result.status
            || stored.1 != result.scan_start
            || stored.2 != result.scan_end
            || stored.3 != result.valid_fraction
            || stored.4 != result.artifact_id
            || stored.5 != result.patch_sha
        {
            return Err(integrity(format!(
                "GOES coverage receipt differs from selected source: {} {decision} offset={offset}",
                result.key
            )));
        }
    }
    if !required_ready {
        tx.commit()
            .await
            .map_err(backfill_support::database_error)?;
        return Ok(0);
    }
    let scan_end = results
        .iter()
        .filter_map(|result| result.scan_end)
        .max()
        .ok_or_else(|| integrity("ready GOES offset lacked scan end"))?;
    let source_metadata = Value::Object(
        results
            .iter()
            .filter_map(|result| {
                Some((
                    result.key.into(),
                    json!({
                        "source_artifact_id":result.artifact_id?,
                        "scan_end":result.scan_end?,
                        "cropped_artifact_sha256":result.patch_sha,
                    }),
                ))
            })
            .collect(),
    );
    let mut verified = 0;
    for radius in RADII {
        for sector in SECTORS {
            let summary = |key: &str| -> Option<&Summary> {
                results
                    .iter()
                    .find(|result| result.key == key)
                    .and_then(|result| result.patch.as_ref())
                    .and_then(|patch| patch.summaries.get(&(radius as u32, sector)))
            };
            let ir = summary("infrared_c13")
                .ok_or_else(|| integrity("ready GOES offset lacked infrared summary"))?;
            let clear = summary("clear_sky_mask");
            let cloud_temp = summary("cloud_top_temperature");
            let cloud_height = summary("cloud_top_height");
            let visible = summary("visible_c02");
            let optical = summary("cloud_optical_depth");
            let vapor = summary("water_vapor_c08");
            let delta_45 = if offset == 15 {
                ir.mean
                    .zip(prior_infrared.get(&(60, radius, sector)).copied())
                    .map(|(current, previous)| current - previous)
            } else {
                None
            };
            let delta_165 = if offset == 15 {
                ir.mean
                    .zip(prior_infrared.get(&(180, radius, sector)).copied())
                    .map(|(current, previous)| current - previous)
            } else {
                None
            };
            let difference = |key: &str,
                              first: &'static str,
                              second: &'static str,
                              cloudy: bool|
             -> Option<f64> {
                let get = |sector| {
                    results
                        .iter()
                        .find(|result| result.key == key)
                        .and_then(|result| result.patch.as_ref())
                        .and_then(|patch| patch.summaries.get(&(radius as u32, sector)))
                        .and_then(|value| {
                            if cloudy {
                                value.cloudy_fraction
                            } else {
                                value.mean
                            }
                        })
                };
                get(first)
                    .zip(get(second))
                    .map(|(first, second)| first - second)
            };
            let quality = Value::Object(
                results
                    .iter()
                    .filter_map(|result| {
                        result
                            .patch
                            .as_ref()
                            .and_then(|patch| patch.summaries.get(&(radius as u32, sector)))
                            .map(|summary| (result.key.into(), quality_json(summary)))
                    })
                    .collect(),
            );
            let mut query = QueryBuilder::<Postgres>::new(
                "INSERT INTO weather.goes_abi_features (process_id,station_id,decision_time,requested_offset_minutes,spatial_radius_km,sector,feature_schema_version,satellite,scan_end,clear_pixel_fraction,cloudy_pixel_fraction,infrared_brightness_temperature_mean_k,infrared_brightness_temperature_stddev_k,infrared_brightness_temperature_p10_k,infrared_brightness_temperature_p50_k,infrared_brightness_temperature_p90_k,cloud_top_temperature_mean_k,cloud_top_temperature_p10_k,cloud_top_temperature_p50_k,cloud_top_temperature_p90_k,cloud_top_height_mean_m,cloud_top_height_p10_m,cloud_top_height_p50_m,cloud_top_height_p90_m,visible_reflectance_mean,visible_reflectance_stddev,visible_reflectance_p10,visible_reflectance_p50,visible_reflectance_p90,cloud_optical_depth_mean,cloud_optical_depth_p50,cloud_optical_depth_p90,water_vapor_brightness_temperature_mean_k,infrared_change_45m_k,infrared_change_165m_k,infrared_north_south_gradient_k,infrared_east_west_gradient_k,cloudy_north_south_gradient,cloudy_east_west_gradient,valid_pixel_fraction,quality_flags,source_metadata) VALUES (",
            );
            query
                .push_bind(process_id)
                .push(",")
                .push_bind(STATION_ID)
                .push(",")
                .push_bind(decision)
                .push(",")
                .push_bind(offset)
                .push(",")
                .push_bind(radius)
                .push(",")
                .push_bind(sector)
                .push(",")
                .push_bind(SCHEMA_VERSION)
                .push(",")
                .push_bind(satellite)
                .push(",")
                .push_bind(scan_end)
                .push(",")
                .push_bind(clear.and_then(|v| v.clear_fraction))
                .push(",")
                .push_bind(clear.and_then(|v| v.cloudy_fraction))
                .push(",")
                .push_bind(ir.mean)
                .push(",")
                .push_bind(ir.stddev)
                .push(",")
                .push_bind(ir.p10)
                .push(",")
                .push_bind(ir.p50)
                .push(",")
                .push_bind(ir.p90)
                .push(",")
                .push_bind(cloud_temp.and_then(|v| v.mean))
                .push(",")
                .push_bind(cloud_temp.and_then(|v| v.p10))
                .push(",")
                .push_bind(cloud_temp.and_then(|v| v.p50))
                .push(",")
                .push_bind(cloud_temp.and_then(|v| v.p90))
                .push(",")
                .push_bind(cloud_height.and_then(|v| v.mean))
                .push(",")
                .push_bind(cloud_height.and_then(|v| v.p10))
                .push(",")
                .push_bind(cloud_height.and_then(|v| v.p50))
                .push(",")
                .push_bind(cloud_height.and_then(|v| v.p90))
                .push(",")
                .push_bind(visible.and_then(|v| v.mean))
                .push(",")
                .push_bind(visible.and_then(|v| v.stddev))
                .push(",")
                .push_bind(visible.and_then(|v| v.p10))
                .push(",")
                .push_bind(visible.and_then(|v| v.p50))
                .push(",")
                .push_bind(visible.and_then(|v| v.p90))
                .push(",")
                .push_bind(optical.and_then(|v| v.mean))
                .push(",")
                .push_bind(optical.and_then(|v| v.p50))
                .push(",")
                .push_bind(optical.and_then(|v| v.p90))
                .push(",")
                .push_bind(vapor.and_then(|v| v.mean))
                .push(",")
                .push_bind(delta_45)
                .push(",")
                .push_bind(delta_165)
                .push(",")
                .push_bind(difference("infrared_c13", "north", "south", false))
                .push(",")
                .push_bind(difference("infrared_c13", "east", "west", false))
                .push(",")
                .push_bind(difference("clear_sky_mask", "north", "south", true))
                .push(",")
                .push_bind(difference("clear_sky_mask", "east", "west", true))
                .push(",")
                .push_bind(ir.valid_pixel_fraction)
                .push(",")
                .push_bind(quality.clone())
                .push(",")
                .push_bind(source_metadata.clone())
                .push(") ON CONFLICT DO NOTHING");
            query
                .build()
                .execute(&mut *tx)
                .await
                .map_err(backfill_support::database_error)?;
            let stored: (Option<f64>, f64, DateTime<Utc>, Value, Value) = sqlx::query_as(
                "SELECT infrared_brightness_temperature_mean_k,valid_pixel_fraction,scan_end,quality_flags,source_metadata FROM weather.goes_abi_features WHERE process_id=$1 AND station_id=$2 AND decision_time=$3 AND requested_offset_minutes=$4 AND spatial_radius_km=$5 AND sector=$6 AND feature_schema_version=$7",
            )
            .bind(process_id).bind(STATION_ID).bind(decision).bind(offset).bind(radius).bind(sector).bind(SCHEMA_VERSION)
            .fetch_one(&mut *tx).await.map_err(backfill_support::database_error)?;
            if stored.0 != ir.mean
                || stored.1 != ir.valid_pixel_fraction
                || stored.2 != scan_end
                || stored.3 != quality
                || stored.4 != source_metadata
            {
                return Err(integrity(format!("GOES feature row differs from selected source: {decision} offset={offset} radius={radius} sector={sector}")));
            }
            verified += 1;
            if let Some(mean) = ir.mean {
                prior_infrared.insert((offset, radius, sector), mean);
            }
        }
    }
    tx.commit()
        .await
        .map_err(backfill_support::database_error)?;
    Ok(verified)
}

fn quality_json(summary: &Summary) -> Value {
    json!({"valid":summary.quality_valid,"counts":summary.quality_counts.iter().map(|(key,value)| (key.to_string(), *value)).collect::<BTreeMap<_,_>>()})
}

#[cfg(test)]
mod tests {
    use super::*;

    fn object(satellite: &str, start: &str, end: &str) -> RawObject {
        let file = format!("OR_ABI-L2-CMIPC-M6C13_{satellite}_s{start}_e{end}_c20262431255000.nc");
        RawObject {
            logical_key: file.clone(),
            provider: "noaa_goes_open_data",
            source_uri: format!(
                "https://noaa-goes19.s3.amazonaws.com/ABI-L2-CMIPC/2026/243/12/{file}"
            ),
            relative_path: PathBuf::from(file),
            media_type: "application/x-netcdf",
            minimum: Utc::now(),
            maximum: Utc::now(),
        }
    }

    #[test]
    fn scan_selection_is_causal_and_prefers_nearest_end() {
        let decision = DateTime::parse_from_rfc3339("2026-08-31T13:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let target = decision - Duration::minutes(60);
        let objects = [
            object("G19", "20262431150100", "20262431155500"),
            object("G19", "20262431210100", "20262431215500"),
            object("G19", "20262431250100", "20262431255500"),
        ];
        let selected = select_scan(objects.iter(), &PRODUCTS[0], target, decision, "G19").unwrap();
        assert!(selected.0.source_uri.contains("31150100"));
        assert!(selected.2 <= decision - Duration::minutes(15));
    }
}
