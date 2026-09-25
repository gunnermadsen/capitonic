use std::{
    fs::File,
    path::{Path, PathBuf},
    sync::Arc,
};

use arrow_array::{Array, RecordBatch, TimestampMicrosecondArray};
use arrow_cast::display::array_value_to_string;
use arrow_schema::{DataType, Schema, TimeUnit};
use chrono::{DateTime, Utc};
use parquet::{
    arrow::{arrow_reader::ParquetRecordBatchReaderBuilder, ArrowWriter},
    basic::{Compression, ZstdLevel},
    file::{
        metadata::KeyValue,
        properties::WriterProperties,
        reader::{FileReader, SerializedFileReader},
    },
};
use sha2::{Digest, Sha256};
use sqlx::FromRow;
use tokio::{fs, io::AsyncReadExt, sync::mpsc};
use uuid::Uuid;

use crate::domain::{DrainContext, DrainExecutionError};

#[derive(Debug, Clone, FromRow)]
pub struct Chunk {
    pub chunk_schema: String,
    pub chunk_name: String,
    pub range_start: DateTime<Utc>,
    pub range_end: DateTime<Utc>,
    pub size_bytes: i64,
}

const LAKE_FREE_SPACE_RESERVE: u64 = 256 * 1024 * 1024;
const CONTENT_DIGEST_KEY: &str = "ingester_source_rows_sha256";
const MISSING_PARITY_PROOF: &str = "Parquet source-row parity proof is missing";

pub async fn preflight_lake_root(
    root: &Path,
    source_bytes: i64,
) -> Result<(), DrainExecutionError> {
    fs::create_dir_all(root).await.map_err(io_error)?;
    #[cfg(target_os = "linux")]
    {
        let mountinfo = fs::read_to_string("/proc/self/mountinfo")
            .await
            .map_err(io_error)?;
        if !has_persistent_mount(root, &mountinfo) {
            return Err(invalid(
                "drain_lake_mount_missing",
                format!(
                    "drain lake root {} is not on a persistent mount",
                    root.display()
                ),
            ));
        }
    }
    let statistics = rustix::fs::statvfs(root).map_err(|error| {
        invalid(
            "drain_lake_space_unavailable",
            format!("cannot inspect free space at {}: {error}", root.display()),
        )
    })?;
    let available = statistics.f_bavail.saturating_mul(statistics.f_frsize);
    if !has_enough_space(available, source_bytes) {
        let needed = u64::try_from(source_bytes.max(0))
            .unwrap_or(u64::MAX)
            .saturating_add(LAKE_FREE_SPACE_RESERVE);
        return Err(invalid(
            "drain_lake_space_insufficient",
            format!(
                "drain lake root {} has {available} free bytes; at least {needed} are required",
                root.display()
            ),
        ));
    }
    Ok(())
}

fn has_enough_space(available: u64, source_bytes: i64) -> bool {
    let needed = u64::try_from(source_bytes.max(0))
        .unwrap_or(u64::MAX)
        .saturating_add(LAKE_FREE_SPACE_RESERVE);
    available >= needed
}

#[cfg(any(target_os = "linux", test))]
fn has_persistent_mount(root: &Path, mountinfo: &str) -> bool {
    mountinfo
        .lines()
        .filter_map(|line| {
            let (mount, filesystem) = line.split_once(" - ")?;
            let path = mount.split_whitespace().nth(4)?;
            let kind = filesystem.split_whitespace().next()?;
            let mountpoint = Path::new(path);
            (mountpoint != Path::new("/") && root.starts_with(mountpoint))
                .then_some((mountpoint.components().count(), kind))
        })
        .max_by_key(|(depth, _)| *depth)
        .is_some_and(|(_, kind)| kind != "overlay")
}

#[derive(Debug, Clone, FromRow)]
pub struct Publication {
    pub object_id: Uuid,
    pub row_count: i64,
    pub relative_path: String,
    pub sha256: String,
    pub byte_size: i64,
    pub status: String,
}

#[derive(Debug, Clone, FromRow)]
pub struct ArchivedPublication {
    pub source_start: DateTime<Utc>,
    pub source_end: DateTime<Utc>,
}

pub async fn archived_publications(
    context: &DrainContext,
    strategy_key: &str,
) -> Result<Vec<ArchivedPublication>, DrainExecutionError> {
    sqlx::query_as(
        "SELECT source_start,source_end FROM ingester.drain_objects WHERE strategy_key=$1 \
         AND status IN ('published','removed') ORDER BY source_start,source_end,object_id",
    )
    .bind(strategy_key)
    .fetch_all(&context.pool)
    .await
    .map_err(db_error)
}

pub async fn existing_publication(
    context: &DrainContext,
    strategy_key: &str,
    chunk: &Chunk,
) -> Result<Option<Publication>, DrainExecutionError> {
    let value = sqlx::query_as::<_, Publication>(
        "SELECT object_id,row_count,relative_path,sha256::text,byte_size,status \
         FROM ingester.drain_objects WHERE strategy_key=$1 AND source_chunk_schema=$2 \
         AND source_chunk_name=$3 AND status IN ('published','removed')",
    )
    .bind(strategy_key)
    .bind(&chunk.chunk_schema)
    .bind(&chunk.chunk_name)
    .fetch_optional(&context.pool)
    .await
    .map_err(db_error)?;
    if value.as_ref().is_some_and(|item| item.status == "removed") {
        return Err(invalid(
            "drain_chunk_already_removed",
            "removed drain object still has a source chunk",
        ));
    }
    Ok(value)
}

pub async fn create_object(
    context: &DrainContext,
    strategy_key: &str,
    relation: &str,
    chunk: &Chunk,
) -> Result<Uuid, DrainExecutionError> {
    sqlx::query_scalar(
        "INSERT INTO ingester.drain_objects(job_id,strategy_key,source_relation,\
         source_chunk_schema,source_chunk_name,source_start,source_end) \
         VALUES($1,$2,$3,$4,$5,$6,$7) ON CONFLICT(strategy_key,source_chunk_schema,\
         source_chunk_name) DO UPDATE SET job_id=EXCLUDED.job_id,updated_at=clock_timestamp() \
         WHERE ingester.drain_objects.status='staging' RETURNING object_id",
    )
    .bind(context.job_id)
    .bind(strategy_key)
    .bind(relation)
    .bind(&chunk.chunk_schema)
    .bind(&chunk.chunk_name)
    .bind(chunk.range_start)
    .bind(chunk.range_end)
    .fetch_one(&context.pool)
    .await
    .map_err(db_error)
}

pub async fn reset_publication(
    context: &DrainContext,
    publication: &Publication,
) -> Result<(), DrainExecutionError> {
    let changed = sqlx::query(
        "UPDATE ingester.drain_objects SET status='staging',job_id=$2,row_count=NULL,\
         relative_path=NULL,sha256=NULL,byte_size=NULL,published_at=NULL,\
         updated_at=clock_timestamp() WHERE object_id=$1 AND status='published'",
    )
    .bind(publication.object_id)
    .bind(context.job_id)
    .execute(&context.pool)
    .await
    .map_err(db_error)?
    .rows_affected();
    if changed != 1 {
        return Err(invalid(
            "drain_publication_changed",
            "published chunk changed before it could be exported again",
        ));
    }
    Ok(())
}

pub fn start_writer(
    staging: PathBuf,
    schema: Arc<Schema>,
) -> (
    mpsc::Sender<RecordBatch>,
    tokio::task::JoinHandle<Result<(), String>>,
) {
    let (sender, mut receiver) = mpsc::channel::<RecordBatch>(2);
    let writer = tokio::task::spawn_blocking(move || -> Result<(), String> {
        let mut content_digest = content_hasher(&schema);
        let file = File::create(&staging).map_err(|error| error.to_string())?;
        let properties = WriterProperties::builder()
            .set_compression(Compression::ZSTD(
                ZstdLevel::try_new(6).map_err(|error| error.to_string())?,
            ))
            .set_max_row_group_size(5_000)
            .build();
        let mut writer = ArrowWriter::try_new(file, schema, Some(properties))
            .map_err(|error| error.to_string())?;
        while let Some(batch) = receiver.blocking_recv() {
            hash_batch_rows(&mut content_digest, &batch)?;
            writer.write(&batch).map_err(|error| error.to_string())?;
        }
        writer.append_key_value_metadata(KeyValue::new(
            CONTENT_DIGEST_KEY.to_owned(),
            format!("{:x}", content_digest.finalize()),
        ));
        let mut file = writer.into_inner().map_err(|error| error.to_string())?;
        use std::io::Write;
        file.flush().map_err(|error| error.to_string())?;
        file.sync_all().map_err(|error| error.to_string())?;
        Ok(())
    });
    (sender, writer)
}

pub async fn finish_writer(
    sender: mpsc::Sender<RecordBatch>,
    writer: tokio::task::JoinHandle<Result<(), String>>,
) -> Result<(), DrainExecutionError> {
    drop(sender);
    match writer.await.map_err(|error| io_error(error.to_string()))? {
        Ok(()) => Ok(()),
        Err(error)
            if error.starts_with("Parquet row ") && error.contains("contains no source values") =>
        {
            Err(invalid("drain_empty_source_row", error))
        }
        Err(error) => Err(io_error(error)),
    }
}

pub async fn send_batch(
    sender: &mpsc::Sender<RecordBatch>,
    writer: &mut tokio::task::JoinHandle<Result<(), String>>,
    batch: RecordBatch,
) -> Result<(), DrainExecutionError> {
    if sender.send(batch).await.is_ok() {
        return Ok(());
    }
    match writer.await.map_err(|error| io_error(error.to_string()))? {
        Ok(()) => Err(io_error(
            "Parquet writer stopped before accepting all source rows",
        )),
        Err(error)
            if error.starts_with("Parquet row ") && error.contains("contains no source values") =>
        {
            Err(invalid("drain_empty_source_row", error))
        }
        Err(error) => Err(io_error(error)),
    }
}

pub async fn publish_file(
    staging: &Path,
    root: &Path,
    relative: &str,
    expected_rows: i64,
) -> Result<(String, i64), DrainExecutionError> {
    let (sha256, byte_size) = hash_file(staging).await?;
    verify_parquet(staging, expected_rows).await?;
    let final_path = root.join(relative);
    let directory = final_path.parent().ok_or_else(|| {
        invalid(
            "drain_path_invalid",
            "published drain object has no parent directory",
        )
    })?;
    fs::create_dir_all(directory).await.map_err(io_error)?;
    install_verified_file(staging, &final_path, &sha256, byte_size).await?;
    Ok((sha256, byte_size))
}

async fn install_verified_file(
    staging: &Path,
    final_path: &Path,
    sha256: &str,
    byte_size: i64,
) -> Result<(), DrainExecutionError> {
    if final_path.exists() {
        match verify_hash(final_path, sha256, byte_size).await {
            Ok(()) => return fs::remove_file(staging).await.map_err(io_error),
            Err(error) if error.code == "drain_object_conflict" => {}
            Err(error) => return Err(error),
        }
    }
    // The new Parquet was verified before this atomic replacement; source removal
    // still requires a matching publication and an unchanged source row count.
    fs::rename(staging, final_path).await.map_err(io_error)?;
    sync_directory(final_path.parent().unwrap().to_owned()).await
}

pub async fn verify_existing(
    root: &Path,
    publication: &Publication,
) -> Result<(), DrainExecutionError> {
    let path = root.join(&publication.relative_path);
    verify_hash(&path, &publication.sha256, publication.byte_size).await?;
    verify_parquet(&path, publication.row_count).await
}

async fn hash_file(path: &Path) -> Result<(String, i64), DrainExecutionError> {
    let mut file = fs::File::open(path).await.map_err(io_error)?;
    let mut hash = Sha256::new();
    let mut bytes = 0i64;
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        let count = file.read(&mut buffer).await.map_err(io_error)?;
        if count == 0 {
            break;
        }
        bytes += count as i64;
        hash.update(&buffer[..count]);
    }
    Ok((format!("{:x}", hash.finalize()), bytes))
}

async fn verify_hash(path: &Path, expected: &str, size: i64) -> Result<(), DrainExecutionError> {
    let (actual, actual_size) = hash_file(path).await?;
    if actual != expected || actual_size != size {
        return Err(invalid(
            "drain_object_conflict",
            "existing Parquet object hash or size differs",
        ));
    }
    Ok(())
}

async fn verify_parquet(path: &Path, expected: i64) -> Result<(), DrainExecutionError> {
    let path = path.to_owned();
    let (rows, expected_digest, actual_digest) = tokio::task::spawn_blocking(move || {
        let reader =
            SerializedFileReader::new(File::open(&path).map_err(|error| error.to_string())?)
                .map_err(|error| error.to_string())?;
        let metadata = reader.metadata().file_metadata();
        let proof = metadata
            .key_value_metadata()
            .and_then(|items| items.iter().find(|item| item.key == CONTENT_DIGEST_KEY))
            .and_then(|item| item.value.clone())
            .ok_or_else(|| MISSING_PARITY_PROOF.to_owned())?;
        let builder = ParquetRecordBatchReaderBuilder::try_new(
            File::open(&path).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        let mut digest = content_hasher(builder.schema());
        let mut observed_rows = 0i64;
        for batch in builder
            .with_batch_size(2048)
            .build()
            .map_err(|error| error.to_string())?
        {
            let batch = batch.map_err(|error| error.to_string())?;
            observed_rows += batch.num_rows() as i64;
            hash_batch_rows(&mut digest, &batch)?;
        }
        if observed_rows != metadata.num_rows() {
            return Err(format!(
                "Parquet metadata reports {} rows but payload has {observed_rows}",
                metadata.num_rows()
            ));
        }
        Ok::<_, String>((observed_rows, proof, format!("{:x}", digest.finalize())))
    })
    .await
    .map_err(|error| io_error(error.to_string()))?
    .map_err(|error| invalid("drain_parity_unverified", error))?;
    if rows != expected {
        return Err(invalid(
            "drain_row_count_mismatch",
            format!("Parquet has {rows} rows, expected {expected}"),
        ));
    }
    if actual_digest != expected_digest {
        return Err(invalid(
            "drain_payload_mismatch",
            "Parquet row values differ from the exported source rows",
        ));
    }
    Ok(())
}

fn content_hasher(schema: &Schema) -> Sha256 {
    let mut digest = Sha256::new();
    digest.update((schema.fields().len() as u64).to_le_bytes());
    for field in schema.fields() {
        hash_value(&mut digest, field.name().as_bytes());
        hash_value(&mut digest, format!("{:?}", field.data_type()).as_bytes());
        digest.update([u8::from(field.is_nullable())]);
    }
    digest
}

fn hash_batch_rows(digest: &mut Sha256, batch: &RecordBatch) -> Result<(), String> {
    for row in 0..batch.num_rows() {
        if batch.columns().iter().all(|column| column.is_null(row)) {
            return Err(format!("Parquet row {row} contains no source values"));
        }
        digest.update([0x52]);
        for column in batch.columns() {
            if column.is_null(row) {
                digest.update([0]);
            } else {
                digest.update([1]);
                if matches!(
                    column.data_type(),
                    DataType::Timestamp(TimeUnit::Microsecond, Some(timezone)) if timezone.as_ref() == "UTC"
                ) {
                    let timestamps = column
                        .as_any()
                        .downcast_ref::<TimestampMicrosecondArray>()
                        .ok_or_else(|| {
                            "UTC timestamp column has an unexpected array type".to_owned()
                        })?;
                    hash_value(digest, &timestamps.value(row).to_le_bytes());
                    continue;
                }
                let value = array_value_to_string(column.as_ref(), row)
                    .map_err(|error| error.to_string())?;
                hash_value(digest, value.as_bytes());
            }
        }
    }
    Ok(())
}

fn hash_value(digest: &mut Sha256, value: &[u8]) {
    digest.update((value.len() as u64).to_le_bytes());
    digest.update(value);
}

async fn sync_directory(path: PathBuf) -> Result<(), DrainExecutionError> {
    tokio::task::spawn_blocking(move || File::open(path)?.sync_all())
        .await
        .map_err(|error| io_error(error.to_string()))?
        .map_err(io_error)
}

pub fn invalid(code: &'static str, message: impl Into<String>) -> DrainExecutionError {
    DrainExecutionError::new(code, message, false)
}

pub fn io_error(error: impl std::fmt::Display) -> DrainExecutionError {
    DrainExecutionError::new("drain_io_failed", error.to_string(), true)
}

pub fn db_error(error: impl std::fmt::Display) -> DrainExecutionError {
    DrainExecutionError::new("drain_database_failed", error.to_string(), true)
}

#[cfg(test)]
mod lake_preflight_tests {
    use super::*;

    #[test]
    fn drain_root_requires_a_non_overlay_mount() {
        let mountinfo = "1 0 0:1 / / rw - overlay overlay rw\n\
            2 1 0:2 /lake /var/lib/lake rw - virtiofs none rw\n\
            3 2 0:3 /nested /var/lib/lake/overlay rw - overlay overlay rw\n";
        assert!(has_persistent_mount(
            Path::new("/var/lib/lake/drains"),
            mountinfo
        ));
        assert!(!has_persistent_mount(
            Path::new("/var/lib/unmounted"),
            mountinfo
        ));
        assert!(!has_persistent_mount(
            Path::new("/var/lib/lake/overlay"),
            mountinfo
        ));
    }

    #[test]
    fn drain_root_reserves_space_for_a_source_chunk() {
        assert!(!has_enough_space(LAKE_FREE_SPACE_RESERVE, 1));
        assert!(has_enough_space(LAKE_FREE_SPACE_RESERVE + 1, 1));
        assert!(!has_enough_space(LAKE_FREE_SPACE_RESERVE, i64::MAX));
    }
}

#[cfg(test)]
mod verified_file_tests {
    use super::*;
    use arrow_array::StringArray;
    use arrow_schema::{DataType, Field};

    #[tokio::test]
    async fn utc_timestamp_rows_receive_a_verified_parquet_proof() {
        let path = std::env::temp_dir().join(format!("utc-drain-row-{}.parquet", Uuid::new_v4()));
        let schema = Arc::new(Schema::new(vec![Field::new(
            "observed_at",
            DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
            false,
        )]));
        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![Arc::new(
                TimestampMicrosecondArray::from(vec![1_790_304_000_000_000, 1_790_304_000_000_001])
                    .with_timezone("UTC"),
            )],
        )
        .unwrap();
        let (sender, writer) = start_writer(path.clone(), schema);
        sender.send(batch).await.unwrap();
        finish_writer(sender, writer).await.unwrap();
        verify_parquet(&path, 2).await.unwrap();
        fs::remove_file(path).await.unwrap();
    }

    #[tokio::test]
    async fn closed_writer_reports_its_original_error() {
        let (sender, receiver) = mpsc::channel(1);
        drop(receiver);
        let mut writer =
            tokio::spawn(async { Err("Parquet schema rejected source value".to_owned()) });
        let schema = Arc::new(Schema::new(vec![Field::new(
            "value",
            DataType::Utf8,
            false,
        )]));
        let batch =
            RecordBatch::try_new(schema, vec![Arc::new(StringArray::from(vec!["value"]))]).unwrap();
        let error = send_batch(&sender, &mut writer, batch).await.unwrap_err();
        assert_eq!(error.code, "drain_io_failed");
        assert_eq!(error.message, "Parquet schema rejected source value");
    }

    #[tokio::test]
    async fn publication_requires_every_parquet_value_to_match_exported_rows() {
        let root = std::env::temp_dir().join(format!("drain-parity-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).await.unwrap();
        let schema = Arc::new(Schema::new(vec![
            Field::new("id", DataType::Utf8, false),
            Field::new("value", DataType::Utf8, true),
        ]));
        let source = RecordBatch::try_new(
            schema.clone(),
            vec![
                Arc::new(StringArray::from(vec!["a", "b"])),
                Arc::new(StringArray::from(vec![Some("source"), None])),
            ],
        )
        .unwrap();
        let valid_path = root.join("valid.parquet");
        let (sender, writer) = start_writer(valid_path.clone(), schema.clone());
        sender.send(source.clone()).await.unwrap();
        finish_writer(sender, writer).await.unwrap();
        verify_parquet(&valid_path, 2).await.unwrap();

        let mut source_digest = content_hasher(&schema);
        hash_batch_rows(&mut source_digest, &source).unwrap();
        let changed = RecordBatch::try_new(
            schema.clone(),
            vec![
                Arc::new(StringArray::from(vec!["a", "b"])),
                Arc::new(StringArray::from(vec![Some("changed"), None])),
            ],
        )
        .unwrap();
        let changed_path = root.join("changed.parquet");
        let mut writer =
            ArrowWriter::try_new(File::create(&changed_path).unwrap(), schema, None).unwrap();
        writer.write(&changed).unwrap();
        writer.append_key_value_metadata(KeyValue::new(
            CONTENT_DIGEST_KEY.to_owned(),
            format!("{:x}", source_digest.finalize()),
        ));
        writer.close().unwrap();
        assert_eq!(
            verify_parquet(&changed_path, 2).await.unwrap_err().code,
            "drain_payload_mismatch"
        );
        fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn empty_row_cannot_receive_a_parity_proof() {
        let schema = Arc::new(Schema::new(vec![Field::new("value", DataType::Utf8, true)]));
        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![Arc::new(StringArray::from(vec![None::<&str>]))],
        )
        .unwrap();
        assert!(hash_batch_rows(&mut content_hasher(&schema), &batch)
            .unwrap_err()
            .contains("contains no source values"));
        let path = std::env::temp_dir().join(format!("empty-drain-row-{}.parquet", Uuid::new_v4()));
        let (sender, writer) = start_writer(path.clone(), schema);
        sender.send(batch).await.unwrap();
        let error = finish_writer(sender, writer).await.unwrap_err();
        assert_eq!(error.code, "drain_empty_source_row");
        assert!(!error.retryable);
        fs::remove_file(path).await.unwrap();
    }

    #[tokio::test]
    async fn stale_object_path_is_atomically_replaced_after_new_file_verification() {
        let root = std::env::temp_dir().join(format!("stale-drain-object-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).await.unwrap();
        let final_path = root.join("object.parquet");
        let staging = root.join("object.tmp");
        fs::write(&final_path, b"old publication").await.unwrap();
        fs::write(&staging, b"verified replacement").await.unwrap();
        let (sha256, byte_size) = hash_file(&staging).await.unwrap();

        install_verified_file(&staging, &final_path, &sha256, byte_size)
            .await
            .unwrap();

        assert_eq!(
            fs::read(&final_path).await.unwrap(),
            b"verified replacement"
        );
        assert!(!staging.exists());
        fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn matching_object_path_is_reused() {
        let root = std::env::temp_dir().join(format!("matching-drain-object-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).await.unwrap();
        let final_path = root.join("object.parquet");
        let staging = root.join("object.tmp");
        fs::write(&final_path, b"same publication").await.unwrap();
        fs::write(&staging, b"same publication").await.unwrap();
        let (sha256, byte_size) = hash_file(&staging).await.unwrap();

        install_verified_file(&staging, &final_path, &sha256, byte_size)
            .await
            .unwrap();

        assert_eq!(fs::read(&final_path).await.unwrap(), b"same publication");
        assert!(!staging.exists());
        fs::remove_dir_all(root).await.unwrap();
    }
}
