use std::{
    collections::HashSet,
    fs::File,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{bail, Context, Result};
use arrow_array::{
    Array, BinaryArray, Decimal128Array, FixedSizeBinaryArray, LargeBinaryArray, LargeStringArray,
    RecordBatch, StringArray, TimestampMicrosecondArray, TimestampMillisecondArray,
};
use chrono::{DateTime, Timelike, Utc};
use md5::Md5;
use parquet::{
    arrow::{arrow_reader::ParquetRecordBatchReaderBuilder, ProjectionMask},
    data_type::Decimal as ParquetDecimal,
    file::{
        metadata::RowGroupMetaData,
        reader::{FileReader, SerializedFileReader},
    },
    record::{Field, Row},
};
use rust_decimal::Decimal;
use sha2::{Digest, Sha256};
use tokio::{
    fs,
    io::{AsyncReadExt, AsyncWriteExt},
    sync::mpsc,
    task::JoinHandle,
    time::timeout,
};
use uuid::Uuid;

use super::reconstruction::ExecutionSnapshotReconstructor;
use super::types::{
    ArchiveCancellation, ArchiveDownloadLimits, ArchiveParseSummary, BtcExecutionSnapshot,
    BtcOrderbookArchiveEvent, BtcOrderbookMarketScope, DownloadedArchive,
};

pub const PMXT_ARCHIVE_PROVIDER: &str = "pmxt_v2";
pub const DEFAULT_PMXT_ARCHIVE_URL: &str = "https://r2v2.pmxt.dev";
pub const PMXT_COVERAGE_START_EPOCH: i64 = 1_776_106_800; // 2026-04-13T19:00:00Z
const PMXT_MULTIPART_CHUNK_BYTES: usize = 8 * 1024 * 1024;
const PMXT_DOWNLOAD_ATTEMPTS: usize = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
struct PmxtEtag {
    digest: String,
    parts: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PmxtObjectIdentity {
    content_length: Option<u64>,
    etag: Option<PmxtEtag>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PmxtArchiveDigest {
    sha256: String,
    bytes: u64,
    single_md5: Option<String>,
    multipart_md5: Option<String>,
    multipart_parts: usize,
}

struct PmxtDigestAccumulator {
    sha256: Sha256,
    whole_md5: Option<Md5>,
    part_md5: Option<Md5>,
    part_bytes: usize,
    part_digests: Vec<[u8; 16]>,
    bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PmxtArchiveSpec {
    pub hour: DateTime<Utc>,
    pub file_name: String,
    pub source_uri: String,
    pub logical_key: String,
}

impl PmxtArchiveSpec {
    pub fn new(base_url: &str, hour: DateTime<Utc>) -> Result<Self> {
        if hour.timestamp_subsec_nanos() != 0 || hour.minute() != 0 || hour.second() != 0 {
            bail!("PMXT archive timestamp must be UTC-hour aligned");
        }
        let stamp = hour.format("%Y-%m-%dT%H");
        let file_name = format!("polymarket_orderbook_{stamp}.parquet");
        Ok(Self {
            hour,
            source_uri: format!("{}/{}", base_url.trim_end_matches('/'), file_name),
            logical_key: format!("pmxt:v2:polymarket_orderbook:{stamp}"),
            file_name,
        })
    }
}

impl PmxtDigestAccumulator {
    fn new(etag: Option<&PmxtEtag>) -> Self {
        Self {
            sha256: Sha256::new(),
            whole_md5: etag
                .filter(|value| value.parts.is_none())
                .map(|_| Md5::new()),
            part_md5: etag
                .filter(|value| value.parts.is_some())
                .map(|_| Md5::new()),
            part_bytes: 0,
            part_digests: Vec::new(),
            bytes: 0,
        }
    }

    fn update(&mut self, bytes: &[u8]) -> Result<()> {
        self.bytes = self
            .bytes
            .checked_add(u64::try_from(bytes.len()).context("PMXT archive size overflow")?)
            .context("PMXT archive size overflow")?;
        self.sha256.update(bytes);
        if let Some(whole_md5) = &mut self.whole_md5 {
            whole_md5.update(bytes);
        }
        if self.part_md5.is_none() {
            return Ok(());
        }
        let mut remaining = bytes;
        while !remaining.is_empty() {
            let available = PMXT_MULTIPART_CHUNK_BYTES.saturating_sub(self.part_bytes);
            let take = available.min(remaining.len());
            self.part_md5
                .as_mut()
                .expect("multipart digest exists")
                .update(&remaining[..take]);
            self.part_bytes += take;
            remaining = &remaining[take..];
            if self.part_bytes == PMXT_MULTIPART_CHUNK_BYTES {
                self.finish_part();
            }
        }
        Ok(())
    }

    fn finish_part(&mut self) {
        let digest: [u8; 16] = self
            .part_md5
            .as_mut()
            .expect("multipart digest exists")
            .finalize_reset()
            .into();
        self.part_digests.push(digest);
        self.part_bytes = 0;
    }

    fn finish(mut self) -> PmxtArchiveDigest {
        if self.part_md5.is_some() && (self.part_bytes > 0 || self.part_digests.is_empty()) {
            self.finish_part();
        }
        let multipart_md5 = self.part_md5.map(|_| {
            let mut multipart = Md5::new();
            for digest in &self.part_digests {
                multipart.update(digest);
            }
            format!("{:x}", multipart.finalize())
        });
        PmxtArchiveDigest {
            sha256: format!("{:x}", self.sha256.finalize()),
            bytes: self.bytes,
            single_md5: self
                .whole_md5
                .map(|digest| format!("{:x}", digest.finalize())),
            multipart_md5,
            multipart_parts: self.part_digests.len(),
        }
    }
}

impl PmxtObjectIdentity {
    fn from_response(response: &reqwest::Response) -> Result<Self> {
        let etag = response
            .headers()
            .get(reqwest::header::ETAG)
            .map(|value| value.to_str().context("PMXT ETag was not ASCII"))
            .transpose()?
            .map(parse_etag)
            .transpose()?;
        Ok(Self {
            content_length: response.content_length(),
            etag,
        })
    }

    fn validate(&self, digest: &PmxtArchiveDigest) -> Result<()> {
        if self
            .content_length
            .is_some_and(|expected| expected != digest.bytes)
        {
            bail!(
                "PMXT archive length mismatch: expected {}, received {}",
                self.content_length.unwrap_or_default(),
                digest.bytes
            );
        }
        if let Some(expected) = &self.etag {
            let matches = match expected.parts {
                Some(parts) => {
                    parts == digest.multipart_parts
                        && digest.multipart_md5.as_deref() == Some(expected.digest.as_str())
                }
                None => digest.single_md5.as_deref() == Some(expected.digest.as_str()),
            };
            if !matches {
                bail!("PMXT archive ETag mismatch");
            }
        }
        Ok(())
    }
}

pub async fn archive_exists(
    client: &reqwest::Client,
    spec: &PmxtArchiveSpec,
    limits: &ArchiveDownloadLimits,
    cancellation: &ArchiveCancellation,
) -> Result<bool> {
    Ok(fetch_object_identity(client, spec, limits, cancellation)
        .await?
        .is_some())
}

pub async fn retain_execution_archives(
    cache_directory: &Path,
    archives: &mut [DownloadedArchive],
) -> Result<()> {
    let retained = archives
        .iter()
        .map(|archive| archive.path.clone())
        .collect::<HashSet<_>>();
    let mut entries = fs::read_dir(cache_directory).await?;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if name.starts_with("polymarket_orderbook_")
            && name.ends_with(".parquet")
            && !retained.contains(&path)
        {
            fs::remove_file(&path)
                .await
                .with_context(|| format!("failed to prune {}", path.display()))?;
        }
    }
    for archive in archives {
        archive.retain();
    }
    Ok(())
}

pub struct ExecutionParseOutput {
    pub summary: ArchiveParseSummary,
    pub snapshots: Vec<BtcExecutionSnapshot>,
}

pub fn spawn_execution_reconstruction(
    paths: Vec<PathBuf>,
    markets: Vec<BtcOrderbookMarketScope>,
    range_end: DateTime<Utc>,
    cancellation: ArchiveCancellation,
) -> JoinHandle<Result<ExecutionParseOutput>> {
    tokio::task::spawn_blocking(move || {
        let conditions = markets
            .iter()
            .map(|market| market.condition_id.clone())
            .collect::<HashSet<_>>();
        let assets = markets
            .iter()
            .flat_map(|market| [market.up_token_id.clone(), market.down_token_id.clone()])
            .collect::<HashSet<_>>();
        let mut reconstructor = ExecutionSnapshotReconstructor::new(markets)?;
        let mut summary = ArchiveParseSummary::default();
        for path in paths {
            parse_execution_archive(
                &path,
                &conditions,
                &assets,
                &cancellation,
                &mut reconstructor,
                &mut summary,
            )?;
        }
        let mut snapshots = Vec::new();
        reconstructor.finish_before(range_end, &mut snapshots);
        snapshots
            .retain(|row| row.up_source_timestamp.is_some() || row.down_source_timestamp.is_some());
        Ok(ExecutionParseOutput { summary, snapshots })
    })
}

fn parse_execution_archive(
    path: &Path,
    conditions: &HashSet<String>,
    assets: &HashSet<String>,
    cancellation: &ArchiveCancellation,
    reconstructor: &mut ExecutionSnapshotReconstructor,
    summary: &mut ArchiveParseSummary,
) -> Result<()> {
    let file = File::open(path)
        .with_context(|| format!("failed to open PMXT archive {}", path.display()))?;
    let builder = ParquetRecordBatchReaderBuilder::try_new(file)
        .context("failed to initialize PMXT Arrow reader")?;
    let fields = builder.schema().fields();
    let expected = [
        "timestamp_received",
        "timestamp",
        "market",
        "event_type",
        "asset_id",
        "bids",
        "asks",
        "price",
        "size",
        "side",
    ];
    if fields.len() < expected.len()
        || fields
            .iter()
            .take(expected.len())
            .zip(expected)
            .any(|(field, expected)| field.name() != expected)
    {
        bail!("PMXT execution archive did not expose the expected leading columns");
    }
    let condition_bytes = conditions
        .iter()
        .map(|condition| condition.as_bytes().to_vec())
        .collect::<Vec<_>>();
    let mut global_base = 0i64;
    let mut selected_groups = Vec::new();
    for row_group_index in 0..builder.metadata().num_row_groups() {
        let metadata = builder.metadata().row_group(row_group_index);
        let records = metadata.num_rows();
        if row_group_may_contain_condition(metadata, &condition_bytes) {
            selected_groups.push((row_group_index, global_base, records));
        }
        global_base = global_base
            .checked_add(records)
            .context("PMXT source row base overflow")?;
    }
    if selected_groups.is_empty() {
        return Ok(());
    }
    let projection = ProjectionMask::roots(builder.parquet_schema(), 0..expected.len());
    let row_groups = selected_groups
        .iter()
        .map(|(index, _, _)| *index)
        .collect::<Vec<_>>();
    let reader = builder
        .with_projection(projection)
        .with_row_groups(row_groups)
        .with_batch_size(8_192)
        .build()
        .context("failed to build PMXT Arrow reader")?;
    let mut selected_group = 0usize;
    let mut ordinal_in_group = 0i64;
    for batch in reader {
        let batch = batch.context("failed to decode PMXT Arrow batch")?;
        parse_execution_batch(
            &batch,
            &selected_groups,
            &mut selected_group,
            &mut ordinal_in_group,
            conditions,
            assets,
            cancellation,
            reconstructor,
            summary,
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn parse_execution_batch(
    batch: &RecordBatch,
    selected_groups: &[(usize, i64, i64)],
    selected_group: &mut usize,
    ordinal_in_group: &mut i64,
    conditions: &HashSet<String>,
    assets: &HashSet<String>,
    cancellation: &ArchiveCancellation,
    reconstructor: &mut ExecutionSnapshotReconstructor,
    summary: &mut ArchiveParseSummary,
) -> Result<()> {
    let schema = batch.schema();
    let received = TimestampColumn::new(
        batch.column(schema.index_of("timestamp_received")?),
        "timestamp_received",
    )?;
    let source = TimestampColumn::new(batch.column(schema.index_of("timestamp")?), "timestamp")?;
    let condition = TextColumn::new(batch.column(schema.index_of("market")?), "market")?;
    let event_type = TextColumn::new(batch.column(schema.index_of("event_type")?), "event_type")?;
    let asset = TextColumn::new(batch.column(schema.index_of("asset_id")?), "asset_id")?;
    let bids = TextColumn::new(batch.column(schema.index_of("bids")?), "bids")?;
    let asks = TextColumn::new(batch.column(schema.index_of("asks")?), "asks")?;
    let price = DecimalColumn::new(batch.column(schema.index_of("price")?), "price")?;
    let size = DecimalColumn::new(batch.column(schema.index_of("size")?), "size")?;
    let side = TextColumn::new(batch.column(schema.index_of("side")?), "side")?;

    for row in 0..batch.num_rows() {
        if cancellation.is_cancelled() {
            bail!("archive operation was cancelled");
        }
        while *selected_group < selected_groups.len()
            && *ordinal_in_group == selected_groups[*selected_group].2
        {
            *selected_group += 1;
            *ordinal_in_group = 0;
        }
        let Some((_, source_base, _)) = selected_groups.get(*selected_group) else {
            bail!("PMXT Arrow reader exceeded selected row-group coverage");
        };
        let source_row_number = source_base
            .checked_add(*ordinal_in_group)
            .context("PMXT source row number overflow")?;
        *ordinal_in_group = ordinal_in_group
            .checked_add(1)
            .context("PMXT row-group ordinal overflow")?;

        let condition_id = condition.required(row)?;
        let asset_id = asset.required(row)?;
        if !conditions.contains(condition_id) || !assets.contains(asset_id) {
            continue;
        }
        let event_type = event_type.required(row)?;
        if matches!(event_type, "last_trade_price" | "tick_size_change") {
            continue;
        }
        if !matches!(event_type, "book" | "price_change") {
            bail!("PMXT row had unsupported event type {event_type}");
        }
        let provider_received_at = received.required(row)?;
        let source_timestamp = source.required(row)?;
        let bids = json_value(&bids, row, "bids")?;
        let asks = json_value(&asks, row, "asks")?;
        if event_type == "book"
            && (!bids.as_ref().is_some_and(serde_json::Value::is_array)
                || !asks.as_ref().is_some_and(serde_json::Value::is_array))
        {
            bail!("PMXT book event did not contain bid and ask arrays");
        }
        reconstructor.apply_components(
            source_row_number,
            provider_received_at,
            source_timestamp,
            condition_id,
            asset_id,
            event_type,
            bids.as_ref(),
            asks.as_ref(),
            price.optional(row)?,
            size.optional(row)?,
            side.optional(row)?,
        )?;
        summary.records = summary.records.saturating_add(1);
        summary.minimum_timestamp = Some(
            summary
                .minimum_timestamp
                .map_or(source_timestamp, |value| value.min(source_timestamp)),
        );
        summary.maximum_timestamp = Some(
            summary
                .maximum_timestamp
                .map_or(source_timestamp, |value| value.max(source_timestamp)),
        );
    }
    summary.batches = summary.batches.saturating_add(1);
    summary.maximum_batch_records = summary.maximum_batch_records.max(batch.num_rows());
    Ok(())
}

enum TextColumn<'a> {
    Utf8(&'a StringArray),
    LargeUtf8(&'a LargeStringArray),
    Binary(&'a BinaryArray),
    LargeBinary(&'a LargeBinaryArray),
    FixedBinary(&'a FixedSizeBinaryArray),
}

impl<'a> TextColumn<'a> {
    fn new(array: &'a Arc<dyn Array>, name: &str) -> Result<Self> {
        if let Some(values) = array.as_any().downcast_ref::<StringArray>() {
            return Ok(Self::Utf8(values));
        }
        if let Some(values) = array.as_any().downcast_ref::<LargeStringArray>() {
            return Ok(Self::LargeUtf8(values));
        }
        if let Some(values) = array.as_any().downcast_ref::<BinaryArray>() {
            return Ok(Self::Binary(values));
        }
        if let Some(values) = array.as_any().downcast_ref::<LargeBinaryArray>() {
            return Ok(Self::LargeBinary(values));
        }
        if let Some(values) = array.as_any().downcast_ref::<FixedSizeBinaryArray>() {
            return Ok(Self::FixedBinary(values));
        }
        bail!("PMXT {name} column had an unsupported Arrow type")
    }

    fn optional(&self, row: usize) -> Result<Option<&str>> {
        let value = match self {
            Self::Utf8(values) if !values.is_null(row) => Some(values.value(row)),
            Self::LargeUtf8(values) if !values.is_null(row) => Some(values.value(row)),
            Self::Binary(values) if !values.is_null(row) => {
                Some(std::str::from_utf8(values.value(row))?)
            }
            Self::LargeBinary(values) if !values.is_null(row) => {
                Some(std::str::from_utf8(values.value(row))?)
            }
            Self::FixedBinary(values) if !values.is_null(row) => {
                Some(std::str::from_utf8(values.value(row))?)
            }
            _ => None,
        };
        Ok(value)
    }

    fn required(&self, row: usize) -> Result<&str> {
        self.optional(row)?
            .context("PMXT required text column contained null")
    }
}

enum TimestampColumn<'a> {
    Millis(&'a TimestampMillisecondArray),
    Micros(&'a TimestampMicrosecondArray),
}

impl<'a> TimestampColumn<'a> {
    fn new(array: &'a Arc<dyn Array>, name: &str) -> Result<Self> {
        if let Some(values) = array.as_any().downcast_ref::<TimestampMillisecondArray>() {
            return Ok(Self::Millis(values));
        }
        if let Some(values) = array.as_any().downcast_ref::<TimestampMicrosecondArray>() {
            return Ok(Self::Micros(values));
        }
        bail!("PMXT {name} column was not an Arrow timestamp")
    }

    fn required(&self, row: usize) -> Result<DateTime<Utc>> {
        match self {
            Self::Millis(values) if !values.is_null(row) => {
                DateTime::from_timestamp_millis(values.value(row))
                    .context("PMXT timestamp was out of range")
            }
            Self::Micros(values) if !values.is_null(row) => {
                DateTime::from_timestamp_micros(values.value(row))
                    .context("PMXT timestamp was out of range")
            }
            _ => bail!("PMXT required timestamp column contained null"),
        }
    }
}

struct DecimalColumn<'a> {
    values: &'a Decimal128Array,
    scale: u32,
}

impl<'a> DecimalColumn<'a> {
    fn new(array: &'a Arc<dyn Array>, name: &str) -> Result<Self> {
        let values = array
            .as_any()
            .downcast_ref::<Decimal128Array>()
            .with_context(|| format!("PMXT {name} column was not decimal128"))?;
        let arrow_schema::DataType::Decimal128(_, scale) = values.data_type() else {
            bail!("PMXT {name} column was not decimal128");
        };
        let scale = u32::try_from(*scale).context("PMXT decimal scale was negative")?;
        Ok(Self { values, scale })
    }

    fn optional(&self, row: usize) -> Result<Option<Decimal>> {
        Ok((!self.values.is_null(row))
            .then(|| Decimal::from_i128_with_scale(self.values.value(row), self.scale)))
    }
}

fn json_value(
    column: &TextColumn<'_>,
    row: usize,
    name: &str,
) -> Result<Option<serde_json::Value>> {
    column
        .optional(row)?
        .map(|value| {
            serde_json::from_str(value).with_context(|| format!("PMXT {name} JSON was invalid"))
        })
        .transpose()
}

fn parse_etag(value: &str) -> Result<PmxtEtag> {
    let value = value.trim().trim_start_matches("W/").trim_matches('"');
    let (digest, parts) = value
        .split_once('-')
        .map_or((value, None), |(digest, parts)| {
            (digest, parts.parse::<usize>().ok())
        });
    if digest.len() != 32
        || !digest.bytes().all(|value| value.is_ascii_hexdigit())
        || value.contains('-') && parts.is_none()
    {
        bail!("PMXT ETag had an unsupported format");
    }
    Ok(PmxtEtag {
        digest: digest.to_ascii_lowercase(),
        parts,
    })
}

pub async fn download_archive(
    client: &reqwest::Client,
    spec: &PmxtArchiveSpec,
    cache_directory: &Path,
    limits: &ArchiveDownloadLimits,
    cancellation: &ArchiveCancellation,
) -> Result<Option<DownloadedArchive>> {
    fs::create_dir_all(cache_directory)
        .await
        .with_context(|| format!("failed to create {}", cache_directory.display()))?;
    let final_path = cache_directory.join(&spec.file_name);
    if final_path.exists() {
        let Some(identity) = fetch_object_identity(client, spec, limits, cancellation).await?
        else {
            fs::remove_file(&final_path)
                .await
                .with_context(|| format!("failed to remove {}", final_path.display()))?;
            return Ok(None);
        };
        let digest = hash_file(
            &final_path,
            limits.maximum_compressed_bytes,
            cancellation,
            identity.etag.as_ref(),
        )
        .await?;
        if identity.validate(&digest).is_ok() {
            return Ok(Some(DownloadedArchive {
                path: final_path,
                sha256: digest.sha256,
                compressed_bytes: digest.bytes,
                cache_hit: true,
                retained: false,
            }));
        }
        fs::remove_file(&final_path)
            .await
            .with_context(|| format!("failed to remove invalid {}", final_path.display()))?;
    }

    let partial_path = cache_directory.join(format!("{}.{}.part", spec.file_name, Uuid::new_v4()));
    let _partial_cleanup = PartialArchiveCleanup(partial_path.clone());
    let mut last_error = None;
    for _ in 0..PMXT_DOWNLOAD_ATTEMPTS {
        match download_archive_once(client, spec, &partial_path, limits, cancellation).await {
            Ok(None) => return Ok(None),
            Ok(Some(digest)) => {
                fs::rename(&partial_path, &final_path)
                    .await
                    .context("failed to publish PMXT archive cache")?;
                return Ok(Some(DownloadedArchive {
                    path: final_path,
                    sha256: digest.sha256,
                    compressed_bytes: digest.bytes,
                    cache_hit: false,
                    retained: false,
                }));
            }
            Err(error) => {
                last_error = Some(error);
            }
        }
        if partial_path.exists() {
            let _ = fs::remove_file(&partial_path).await;
        }
    }
    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("PMXT archive download failed")))
}

struct PartialArchiveCleanup(PathBuf);

impl Drop for PartialArchiveCleanup {
    fn drop(&mut self) {
        match std::fs::remove_file(&self.0) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => tracing::warn!(
                event = "pmxt_archive_cleanup_failed",
                error_code = "pmxt_archive_partial_cleanup",
                path = %self.0.display(),
                %error,
                "failed to remove PMXT partial archive"
            ),
        }
    }
}

async fn fetch_object_identity(
    client: &reqwest::Client,
    spec: &PmxtArchiveSpec,
    limits: &ArchiveDownloadLimits,
    cancellation: &ArchiveCancellation,
) -> Result<Option<PmxtObjectIdentity>> {
    if cancellation.is_cancelled() {
        bail!("archive operation was cancelled");
    }
    let response = client
        .head(&spec.source_uri)
        .timeout(limits.request_timeout)
        .send()
        .await
        .with_context(|| format!("failed to inspect {}", spec.source_uri))?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let response = response
        .error_for_status()
        .with_context(|| format!("PMXT rejected {}", spec.source_uri))?;
    PmxtObjectIdentity::from_response(&response).map(Some)
}

async fn download_archive_once(
    client: &reqwest::Client,
    spec: &PmxtArchiveSpec,
    partial_path: &Path,
    limits: &ArchiveDownloadLimits,
    cancellation: &ArchiveCancellation,
) -> Result<Option<PmxtArchiveDigest>> {
    if cancellation.is_cancelled() {
        bail!("archive operation was cancelled");
    }
    let mut response = client
        .get(&spec.source_uri)
        .timeout(limits.request_timeout)
        .send()
        .await
        .with_context(|| format!("failed to request {}", spec.source_uri))?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    response = response
        .error_for_status()
        .with_context(|| format!("PMXT rejected {}", spec.source_uri))?;
    let identity = PmxtObjectIdentity::from_response(&response)?;
    if identity
        .content_length
        .is_some_and(|size| size > limits.maximum_compressed_bytes)
    {
        bail!("PMXT archive exceeded its compressed size limit");
    }
    let mut output = fs::File::create(partial_path)
        .await
        .with_context(|| format!("failed to create {}", partial_path.display()))?;
    let mut digest = PmxtDigestAccumulator::new(identity.etag.as_ref());
    loop {
        if cancellation.is_cancelled() {
            bail!("archive operation was cancelled");
        }
        let chunk = timeout(limits.chunk_idle_timeout, response.chunk())
            .await
            .context("PMXT archive response stalled")??;
        let Some(chunk) = chunk else { break };
        digest.update(&chunk)?;
        if digest.bytes > limits.maximum_compressed_bytes {
            bail!("PMXT archive exceeded its compressed size limit");
        }
        output
            .write_all(&chunk)
            .await
            .context("failed to write PMXT archive cache")?;
    }
    let digest = digest.finish();
    identity.validate(&digest)?;
    drop(output);
    Ok(Some(digest))
}

pub fn spawn_parser(
    path: PathBuf,
    markets: Vec<BtcOrderbookMarketScope>,
    batch_rows: usize,
    cancellation: ArchiveCancellation,
) -> (
    mpsc::Receiver<Result<Vec<BtcOrderbookArchiveEvent>>>,
    JoinHandle<Result<ArchiveParseSummary>>,
) {
    let (sender, receiver) = mpsc::channel(1);
    let handle = tokio::task::spawn_blocking(move || {
        let conditions = markets
            .iter()
            .map(|market| market.condition_id.as_str())
            .collect::<HashSet<_>>();
        let condition_bytes = conditions
            .iter()
            .map(|condition| condition.as_bytes().to_vec())
            .collect::<Vec<_>>();
        let assets = markets
            .iter()
            .flat_map(|market| [&market.up_token_id, &market.down_token_id])
            .map(String::as_str)
            .collect::<HashSet<_>>();
        let file = File::open(&path)
            .with_context(|| format!("failed to open PMXT archive {}", path.display()))?;
        let reader =
            SerializedFileReader::new(file).context("failed to initialize PMXT Parquet reader")?;
        let mut summary = ArchiveParseSummary::default();
        let mut batch = Vec::with_capacity(batch_rows);
        let mut source_row_base = 0i64;

        for row_group_index in 0..reader.num_row_groups() {
            let metadata = reader.metadata().row_group(row_group_index);
            let row_group_records = metadata.num_rows();
            if row_group_may_contain_condition(metadata, &condition_bytes) {
                let row_group = reader.get_row_group(row_group_index).with_context(|| {
                    format!("failed to initialize PMXT Parquet row group {row_group_index}")
                })?;
                let rows = row_group.get_row_iter(None).with_context(|| {
                    format!("failed to stream PMXT Parquet row group {row_group_index}")
                })?;
                for (row_group_ordinal, row) in rows.enumerate() {
                    if cancellation.is_cancelled() {
                        bail!("archive operation was cancelled");
                    }
                    let row_group_ordinal = i64::try_from(row_group_ordinal)
                        .context("PMXT row-group ordinal overflow")?;
                    let source_row_number = source_row_base
                        .checked_add(row_group_ordinal)
                        .context("PMXT source row number overflow")?;
                    let row = row.map_err(|error| {
                        anyhow::anyhow!(
                            "failed to decode PMXT Parquet row group {row_group_index}, \
                             source row {source_row_number}: {error}"
                        )
                    })?;
                    let record = parse_row(row, source_row_number, &conditions, &assets)?;
                    let Some(record) = record else {
                        continue;
                    };
                    summary.records = summary.records.saturating_add(1);
                    summary.minimum_timestamp = Some(
                        summary
                            .minimum_timestamp
                            .map_or(record.source_timestamp, |value| {
                                value.min(record.source_timestamp)
                            }),
                    );
                    summary.maximum_timestamp = Some(
                        summary
                            .maximum_timestamp
                            .map_or(record.source_timestamp, |value| {
                                value.max(record.source_timestamp)
                            }),
                    );
                    batch.push(record);
                    if batch.len() == batch_rows {
                        summary.batches = summary.batches.saturating_add(1);
                        summary.maximum_batch_records =
                            summary.maximum_batch_records.max(batch.len());
                        sender
                            .blocking_send(Ok(std::mem::take(&mut batch)))
                            .context("PMXT parser consumer stopped")?;
                        batch = Vec::with_capacity(batch_rows);
                    }
                }
            }
            source_row_base = source_row_base
                .checked_add(row_group_records)
                .context("PMXT source row base overflow")?;
        }
        if !batch.is_empty() {
            summary.batches = summary.batches.saturating_add(1);
            summary.maximum_batch_records = summary.maximum_batch_records.max(batch.len());
            sender
                .blocking_send(Ok(batch))
                .context("PMXT parser consumer stopped")?;
        }
        Ok(summary)
    });
    (receiver, handle)
}

fn row_group_may_contain_condition(metadata: &RowGroupMetaData, condition_ids: &[Vec<u8>]) -> bool {
    let Some(statistics) = metadata.column(2).statistics() else {
        return true;
    };
    if !statistics.min_is_exact() || !statistics.max_is_exact() {
        return true;
    }
    let (Some(minimum), Some(maximum)) = (statistics.min_bytes_opt(), statistics.max_bytes_opt())
    else {
        return true;
    };
    condition_range_overlaps(condition_ids, minimum, maximum)
}

fn condition_range_overlaps(condition_ids: &[Vec<u8>], minimum: &[u8], maximum: &[u8]) -> bool {
    condition_ids
        .iter()
        .any(|condition| condition.as_slice() >= minimum && condition.as_slice() <= maximum)
}

fn parse_row(
    row: Row,
    source_row_number: i64,
    conditions: &HashSet<&str>,
    assets: &HashSet<&str>,
) -> Result<Option<BtcOrderbookArchiveEvent>> {
    let columns = row.into_columns();
    if columns.len() != 16 {
        bail!("PMXT row did not have the documented 16-column schema");
    }
    let provider_received_at = timestamp_field(&columns[0].1, "timestamp_received")?;
    let source_timestamp = timestamp_field(&columns[1].1, "timestamp")?;
    let condition_id = bytes_field(&columns[2].1, "market")?;
    let event_type = string_field(&columns[3].1, "event_type")?;
    let asset_id = string_field(&columns[4].1, "asset_id")?;
    if !conditions.contains(condition_id.as_str()) || !assets.contains(asset_id.as_str()) {
        return Ok(None);
    }
    if !matches!(
        event_type.as_str(),
        "book" | "price_change" | "last_trade_price" | "tick_size_change"
    ) {
        bail!("PMXT row had unsupported event type {event_type}");
    }
    let bids = json_field(&columns[5].1, "bids")?;
    let asks = json_field(&columns[6].1, "asks")?;
    if event_type == "book"
        && (!bids.as_ref().is_some_and(serde_json::Value::is_array)
            || !asks.as_ref().is_some_and(serde_json::Value::is_array))
    {
        bail!("PMXT book event did not contain bid and ask arrays");
    }
    Ok(Some(BtcOrderbookArchiveEvent {
        source_row_number,
        provider_received_at,
        source_timestamp,
        condition_id,
        asset_id,
        event_type,
        bids,
        asks,
        price: decimal_field(&columns[7].1)?,
        size: decimal_field(&columns[8].1)?,
        side: optional_string_field(&columns[9].1, "side")?.map(|side| side.to_ascii_lowercase()),
        best_bid: decimal_field(&columns[10].1)?,
        best_ask: decimal_field(&columns[11].1)?,
        fee_rate_bps: optional_u16_field(&columns[12].1)?.map(i32::from),
        transaction_hash: optional_string_field(&columns[13].1, "transaction_hash")?,
        old_tick_size: decimal_field(&columns[14].1)?,
        new_tick_size: decimal_field(&columns[15].1)?,
    }))
}

fn timestamp_field(field: &Field, name: &str) -> Result<DateTime<Utc>> {
    let Field::TimestampMillis(value) = field else {
        bail!("PMXT {name} was not timestamp[ms]");
    };
    DateTime::from_timestamp_millis(*value).context("PMXT timestamp was out of range")
}

fn bytes_field(field: &Field, name: &str) -> Result<String> {
    let Field::Bytes(value) = field else {
        bail!("PMXT {name} was not fixed binary");
    };
    String::from_utf8(value.data().to_vec()).with_context(|| format!("PMXT {name} was not ASCII"))
}

fn string_field(field: &Field, name: &str) -> Result<String> {
    let Field::Str(value) = field else {
        bail!("PMXT {name} was not a string");
    };
    Ok(value.clone())
}

fn optional_string_field(field: &Field, name: &str) -> Result<Option<String>> {
    match field {
        Field::Null => Ok(None),
        Field::Str(value) => Ok(Some(value.clone())),
        _ => bail!("PMXT {name} was neither null nor a string"),
    }
}

fn json_field(field: &Field, name: &str) -> Result<Option<serde_json::Value>> {
    optional_string_field(field, name)?
        .map(|value| {
            serde_json::from_str(&value).with_context(|| format!("PMXT {name} was invalid JSON"))
        })
        .transpose()
}

fn decimal_field(field: &Field) -> Result<Option<Decimal>> {
    match field {
        Field::Null => Ok(None),
        Field::Decimal(value) => Ok(Some(parquet_decimal(value)?)),
        _ => bail!("PMXT decimal column had an unexpected type"),
    }
}

fn parquet_decimal(value: &ParquetDecimal) -> Result<Decimal> {
    let bytes = value.data();
    if bytes.is_empty() || bytes.len() > 16 {
        bail!("PMXT decimal exceeded supported i128 width");
    }
    let fill = if bytes[0] & 0x80 == 0 { 0 } else { 0xff };
    let mut extended = [fill; 16];
    extended[16 - bytes.len()..].copy_from_slice(bytes);
    let scale = u32::try_from(value.scale()).context("PMXT decimal scale was negative")?;
    Ok(Decimal::from_i128_with_scale(
        i128::from_be_bytes(extended),
        scale,
    ))
}

fn optional_u16_field(field: &Field) -> Result<Option<u16>> {
    match field {
        Field::Null => Ok(None),
        Field::UShort(value) => Ok(Some(*value)),
        _ => bail!("PMXT fee_rate_bps was neither null nor uint16"),
    }
}

async fn hash_file(
    path: &Path,
    maximum_bytes: u64,
    cancellation: &ArchiveCancellation,
    etag: Option<&PmxtEtag>,
) -> Result<PmxtArchiveDigest> {
    let mut input = fs::File::open(path).await?;
    let mut digest = PmxtDigestAccumulator::new(etag);
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        if cancellation.is_cancelled() {
            bail!("archive operation was cancelled");
        }
        let read = input.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read])?;
        if digest.bytes > maximum_bytes {
            bail!("PMXT cached archive exceeded its compressed size limit");
        }
    }
    Ok(digest.finish())
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use arrow_array::{Decimal128Array, RecordBatch, StringArray, TimestampMillisecondArray};
    use arrow_schema::{DataType, Field as ArrowField, Schema, TimeUnit};
    use chrono::TimeZone;
    use parquet::arrow::ArrowWriter;
    use parquet::data_type::ByteArray;
    use tempfile::TempDir;
    use tokio::{net::TcpListener, time::sleep};

    use super::*;

    #[test]
    fn archive_identity_is_hourly_and_stable() {
        let hour = Utc.with_ymd_and_hms(2026, 4, 17, 12, 0, 0).unwrap();
        let spec = PmxtArchiveSpec::new(DEFAULT_PMXT_ARCHIVE_URL, hour).unwrap();
        assert_eq!(
            spec.source_uri,
            "https://r2v2.pmxt.dev/polymarket_orderbook_2026-04-17T12.parquet"
        );
        assert_eq!(
            spec.logical_key,
            "pmxt:v2:polymarket_orderbook:2026-04-17T12"
        );
    }

    #[test]
    fn parquet_decimals_preserve_signed_scale() {
        let value = ParquetDecimal::from_bytes(ByteArray::from(vec![0x01, 0x86, 0xa0]), 9, 4);
        assert_eq!(parquet_decimal(&value).unwrap(), Decimal::new(100_000, 4));
        let negative =
            ParquetDecimal::from_bytes(ByteArray::from(vec![0xff, 0xff, 0xff, 0x9c]), 9, 2);
        assert_eq!(parquet_decimal(&negative).unwrap(), Decimal::new(-100, 2));
    }

    #[test]
    fn condition_ranges_prune_only_disjoint_row_groups() {
        let conditions = vec![b"0x20".to_vec(), b"0x80".to_vec()];
        assert!(condition_range_overlaps(&conditions, b"0x10", b"0x20"));
        assert!(condition_range_overlaps(&conditions, b"0x70", b"0x90"));
        assert!(!condition_range_overlaps(&conditions, b"0x21", b"0x79"));
        assert!(!condition_range_overlaps(&conditions, b"0x81", b"0xff"));
    }

    #[test]
    fn multipart_etag_validation_matches_r2_layout() {
        let etag = parse_etag("\"15c088024dc2b3017cad9ee6965f364a-2\"").unwrap();
        let mut accumulator = PmxtDigestAccumulator::new(Some(&etag));
        accumulator
            .update(&vec![b'a'; PMXT_MULTIPART_CHUNK_BYTES])
            .unwrap();
        accumulator.update(b"b").unwrap();
        let digest = accumulator.finish();
        assert_eq!(digest.bytes, 8_388_609);
        assert_eq!(digest.multipart_parts, 2);
        assert_eq!(
            digest.multipart_md5.as_deref(),
            Some("15c088024dc2b3017cad9ee6965f364a")
        );
        assert_eq!(digest.single_md5, None);
        let identity = PmxtObjectIdentity {
            content_length: Some(8_388_609),
            etag: Some(parse_etag("\"15c088024dc2b3017cad9ee6965f364a-2\"").unwrap()),
        };
        assert!(identity.validate(&digest).is_ok());
    }

    #[tokio::test]
    async fn archive_request_timeout_overrides_the_short_api_client_deadline() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = vec![0u8; 1024];
            let _ = stream.read(&mut request).await.unwrap();
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nConnection: close\r\n\r\n")
                .await
                .unwrap();
            for byte in b"abc" {
                sleep(Duration::from_millis(75)).await;
                stream.write_all(&[*byte]).await.unwrap();
            }
        });
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(100))
            .build()
            .unwrap();
        let hour = Utc.with_ymd_and_hms(2026, 4, 17, 12, 0, 0).unwrap();
        let spec = PmxtArchiveSpec::new(&format!("http://{address}"), hour).unwrap();
        let directory = TempDir::new().unwrap();
        let archive = download_archive(
            &client,
            &spec,
            directory.path(),
            &ArchiveDownloadLimits {
                maximum_compressed_bytes: 1024,
                request_timeout: Duration::from_secs(2),
                chunk_idle_timeout: Duration::from_millis(200),
            },
            &ArchiveCancellation::default(),
        )
        .await
        .unwrap()
        .unwrap();
        server.await.unwrap();
        assert_eq!(archive.compressed_bytes, 3);
        assert_eq!(tokio::fs::read(&archive.path).await.unwrap(), b"abc");
    }

    #[test]
    fn partial_archive_guard_removes_abandoned_downloads() {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("archive.parquet.lease.part");
        std::fs::write(&path, b"partial").unwrap();
        {
            let _cleanup = PartialArchiveCleanup(path.clone());
        }
        assert!(!path.exists());
    }

    #[test]
    fn downloaded_archive_removes_cache_file_on_drop() {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("archive.parquet");
        std::fs::write(&path, b"complete").unwrap();
        {
            let _archive = DownloadedArchive {
                path: path.clone(),
                sha256: "0".repeat(64),
                compressed_bytes: 8,
                cache_hit: false,
                retained: false,
            };
        }
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn retained_execution_archives_prune_older_cache_files() {
        let directory = TempDir::new().unwrap();
        let old = directory
            .path()
            .join("polymarket_orderbook_2026-04-17T10.parquet");
        let seed = directory
            .path()
            .join("polymarket_orderbook_2026-04-17T11.parquet");
        let current = directory
            .path()
            .join("polymarket_orderbook_2026-04-17T12.parquet");
        for path in [&old, &seed, &current] {
            std::fs::write(path, b"complete").unwrap();
        }
        let mut archives = vec![
            DownloadedArchive {
                path: seed.clone(),
                sha256: "0".repeat(64),
                compressed_bytes: 8,
                cache_hit: true,
                retained: false,
            },
            DownloadedArchive {
                path: current.clone(),
                sha256: "1".repeat(64),
                compressed_bytes: 8,
                cache_hit: false,
                retained: false,
            },
        ];
        retain_execution_archives(directory.path(), &mut archives)
            .await
            .unwrap();
        drop(archives);
        assert!(!old.exists());
        assert!(seed.exists());
        assert!(current.exists());
    }

    #[tokio::test]
    async fn arrow_execution_parser_reconstructs_matching_books_without_row_materialization() {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("execution.parquet");
        let schema = Arc::new(Schema::new(vec![
            ArrowField::new(
                "timestamp_received",
                DataType::Timestamp(TimeUnit::Millisecond, None),
                false,
            ),
            ArrowField::new(
                "timestamp",
                DataType::Timestamp(TimeUnit::Millisecond, None),
                false,
            ),
            ArrowField::new("market", DataType::Utf8, false),
            ArrowField::new("event_type", DataType::Utf8, false),
            ArrowField::new("asset_id", DataType::Utf8, false),
            ArrowField::new("bids", DataType::Utf8, true),
            ArrowField::new("asks", DataType::Utf8, true),
            ArrowField::new("price", DataType::Decimal128(10, 4), true),
            ArrowField::new("size", DataType::Decimal128(10, 4), true),
            ArrowField::new("side", DataType::Utf8, true),
        ]));
        let start = Utc.with_ymd_and_hms(2026, 4, 17, 12, 0, 0).unwrap();
        let received = start.timestamp_millis() + 900;
        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![
                Arc::new(TimestampMillisecondArray::from(vec![received; 3])),
                Arc::new(TimestampMillisecondArray::from(vec![received - 1; 3])),
                Arc::new(StringArray::from(vec![
                    "condition",
                    "condition",
                    "unrelated",
                ])),
                Arc::new(StringArray::from(vec!["book", "book", "book"])),
                Arc::new(StringArray::from(vec!["up", "down", "other"])),
                Arc::new(StringArray::from(vec![
                    Some("[[\"0.40\",\"10\"]]"),
                    Some("[[\"0.40\",\"10\"]]"),
                    Some("[]"),
                ])),
                Arc::new(StringArray::from(vec![
                    Some("[[\"0.45\",\"4\"],[\"0.50\",\"216\"]]"),
                    Some("[[\"0.45\",\"4\"],[\"0.50\",\"216\"]]"),
                    Some("[]"),
                ])),
                Arc::new(
                    Decimal128Array::from(vec![None, None, None])
                        .with_precision_and_scale(10, 4)
                        .unwrap(),
                ),
                Arc::new(
                    Decimal128Array::from(vec![None, None, None])
                        .with_precision_and_scale(10, 4)
                        .unwrap(),
                ),
                Arc::new(StringArray::from(vec![None::<&str>, None, None])),
            ],
        )
        .unwrap();
        let output = File::create(&path).unwrap();
        let mut writer = ArrowWriter::try_new(output, schema, None).unwrap();
        writer.write(&batch).unwrap();
        writer.close().unwrap();
        let scope = BtcOrderbookMarketScope {
            market_id: "market".to_owned(),
            condition_id: "condition".to_owned(),
            up_token_id: "up".to_owned(),
            down_token_id: "down".to_owned(),
            window_start: start,
            window_end: start + chrono::Duration::minutes(5),
        };
        let parsed = spawn_execution_reconstruction(
            vec![path],
            vec![scope],
            start + chrono::Duration::minutes(5),
            ArchiveCancellation::default(),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(parsed.summary.records, 2);
        assert_eq!(parsed.snapshots.len(), 96);
        assert_eq!(parsed.snapshots[0].quality_flags, 0);
        assert_eq!(parsed.snapshots[0].up_best_ask, Some(Decimal::new(45, 2)));
    }
}
