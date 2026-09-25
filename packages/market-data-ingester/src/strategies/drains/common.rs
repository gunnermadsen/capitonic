use std::{
    fs::File,
    path::{Path, PathBuf},
    sync::Arc,
};

use arrow_array::RecordBatch;
use arrow_schema::Schema;
use chrono::{DateTime, Utc};
use parquet::{
    arrow::ArrowWriter,
    basic::{Compression, ZstdLevel},
    file::{
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
            writer.write(&batch).map_err(|error| error.to_string())?;
        }
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
    writer
        .await
        .map_err(|error| io_error(error.to_string()))?
        .map_err(io_error)
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
    let rows = tokio::task::spawn_blocking(move || {
        SerializedFileReader::new(File::open(path).map_err(|error| error.to_string())?)
            .map(|reader| reader.metadata().file_metadata().num_rows())
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| io_error(error.to_string()))?
    .map_err(io_error)?;
    if rows != expected {
        return Err(invalid(
            "drain_row_count_mismatch",
            format!("Parquet has {rows} rows, expected {expected}"),
        ));
    }
    Ok(())
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
