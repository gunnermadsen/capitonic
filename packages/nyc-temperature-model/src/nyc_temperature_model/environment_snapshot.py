from __future__ import annotations

import hashlib
import json
import os
from collections.abc import Iterator
from datetime import UTC, date, datetime, timedelta
from pathlib import Path
from typing import Any
from uuid import UUID
from zoneinfo import ZoneInfo

import pyarrow as pa
import pyarrow.parquet as pq

from . import PROCESS_ID, STATION_ID
from .config import Settings
from .database import connection
from .goes_ingestion import FEATURE_SCHEMA_VERSION as GOES_VERSION
from .hrrr_environment_ingestion import FEATURE_SCHEMA_VERSION as HRRR_VERSION
from .sources import file_sha256

_WEATHER_DRAIN_ROOT = Path(
    os.environ.get(
        "WEATHER_DATA_ROOT", "/Volumes/docker-data/polymarket-bot/temperature-expectancy"
    )
) / "curated/drains"
_WEATHER_DRAINS = {
    "goes_abi_features": ("goes-abi-features", "weather.goes_abi_features"),
    "hrrr_environment_features": ("hrrr-environment-features", "weather.hrrr_environment_features"),
}
_NYC = ZoneInfo("America/New_York")


def _removed_weather_objects(
    conn, strategy: str, start_at: datetime, end_at: datetime
) -> list[dict]:
    return list(conn.execute(
        """
        SELECT source_start, source_end, relative_path, sha256, byte_size, row_count,
               source_rows_sha256
        FROM ingester.drain_objects
        WHERE strategy_key=%s AND status='removed'
          AND source_end > %s AND source_start < %s
        ORDER BY source_start, object_id
        """,
        (strategy, start_at, end_at),
    ).fetchall())


def _archive_rows(object_row: dict, strategy: str) -> Iterator[dict]:
    directory, _ = _WEATHER_DRAINS[strategy]
    root = (_WEATHER_DRAIN_ROOT / directory).resolve()
    path = (root / object_row["relative_path"]).resolve()
    if not path.is_relative_to(root) or not path.is_file():
        raise RuntimeError(f"verified weather drain file is missing or outside its root: {path}")
    digest, size = file_sha256(path)
    if digest != object_row["sha256"].strip() or size != object_row["byte_size"]:
        raise RuntimeError(f"verified weather drain file hash or size changed: {path}")
    parquet = pq.ParquetFile(path)
    if parquet.metadata.num_rows != object_row["row_count"]:
        raise RuntimeError(f"verified weather drain row count changed: {path}")
    expected_fields = set(parquet.schema_arrow.names) - {"source_row_json"}
    if "source_row_json" not in parquet.schema_arrow.names:
        raise RuntimeError(f"verified weather drain lacks source rows: {path}")
    source_digest = hashlib.sha256()
    count = 0
    for batch in parquet.iter_batches(batch_size=10_000):
        for encoded in batch.column(batch.schema.get_field_index("source_row_json")).to_pylist():
            if count:
                source_digest.update(b"\n")
            source_digest.update(encoded.encode())
            count += 1
            row = json.loads(encoded)
            if set(row) != expected_fields:
                raise RuntimeError(f"verified weather drain source schema changed: {path}")
            yield row
    if (
        count != object_row["row_count"]
        or source_digest.hexdigest() != object_row["source_rows_sha256"]
    ):
        raise RuntimeError(f"verified weather drain source fingerprint changed: {path}")


def _snapshot_value(value: Any) -> Any:
    if isinstance(value, UUID):
        return str(value)
    if isinstance(value, (dict, list)):
        return json.dumps(value, sort_keys=True, separators=(",", ":"))
    return value


def _snapshot_schema(description) -> pa.Schema:
    types = {
        16: pa.bool_(), 23: pa.int32(), 25: pa.string(), 701: pa.float64(),
        1184: pa.timestamp("us", tz="UTC"), 1700: pa.decimal128(8, 3),
        2950: pa.string(), 3802: pa.string(),
    }
    return pa.schema([pa.field(field.name, types[field.type_code]) for field in description])


def _write_weather_snapshot(
    conn, strategy: str, *, start_at: datetime, end_at: datetime,
    version: str, destination: Path, order_fields: tuple[str, ...]
) -> int:
    _, relation = _WEATHER_DRAINS[strategy]
    objects = _removed_weather_objects(conn, strategy, start_at, end_at)
    objects_by_day = {row["source_start"].date(): row for row in objects}
    if len(objects_by_day) != len(objects):
        raise RuntimeError(f"weather drain has overlapping publications for {strategy}")
    labels = {
        row["event_date"]: row
        for row in conn.execute(
            """SELECT event_date, station_daily_max_f, station_rounded_max_f,
                      winner_matches_station
               FROM weather.label_reconciliation
               WHERE process_id=%s AND event_date >= %s AND event_date <= %s""",
            (PROCESS_ID, start_at.astimezone(_NYC).date(), end_at.astimezone(_NYC).date()),
        ).fetchall()
    }
    query = f"""SELECT t.*, l.station_daily_max_f, l.station_rounded_max_f,
                   l.winner_matches_station
            FROM {relation} t
            LEFT JOIN weather.label_reconciliation l
              ON l.process_id=t.process_id
             AND l.event_date=(t.decision_time AT TIME ZONE 'America/New_York')::date
            WHERE t.process_id=%s AND t.station_id=%s
              AND t.decision_time >= %s AND t.decision_time < %s
              AND t.feature_schema_version=%s
            ORDER BY {','.join('t.' + field for field in order_fields)}"""
    writer = None
    rows_written = 0
    day = start_at.date()
    try:
        while day < end_at.date():
            day_start = datetime.combine(day, datetime.min.time(), UTC)
            day_end = day_start + timedelta(days=1)
            cursor = conn.cursor(name=f"snapshot_{destination.stem}_{day:%Y%m%d}")
            retained: list[dict] = []
            try:
                cursor.execute(query, (PROCESS_ID, STATION_ID, day_start, day_end, version))
                schema = _snapshot_schema(cursor.description)
                while rows := cursor.fetchmany(10_000):
                    retained.extend(
                        {key: _snapshot_value(value) for key, value in row.items()}
                        for row in rows
                    )
            finally:
                cursor.close()
            archived: list[dict] = []
            if object_row := objects_by_day.get(day):
                for row in _archive_rows(object_row, strategy):
                    row = row.copy()
                    decision_time = datetime.fromisoformat(row["decision_time"])
                    if (start_at <= decision_time < end_at and row["process_id"] == str(PROCESS_ID)
                            and row["station_id"] == STATION_ID
                            and row["feature_schema_version"] == version):
                        for field in (
                            "decision_time", "created_at", "scan_end", "model_run", "valid_at"
                        ):
                            if field in row and row[field] is not None:
                                row[field] = datetime.fromisoformat(row[field])
                        label = labels.get(row["decision_time"].astimezone(_NYC).date(), {})
                        row.update({
                            name: label.get(name) for name in (
                                "station_daily_max_f", "station_rounded_max_f",
                                "winner_matches_station",
                            )
                        })
                        archived.append({key: _snapshot_value(value) for key, value in row.items()})
            primary_key = ("process_id", "station_id", *order_fields, "feature_schema_version")
            seen = {tuple(str(row[field]) for field in primary_key) for row in retained}
            for row in archived:
                key = tuple(str(row[field]) for field in primary_key)
                if key in seen:
                    raise RuntimeError(
                        f"weather drain and PostgreSQL overlap for {strategy}: {key}"
                    )
                seen.add(key)
            result = retained + archived
            if result:
                result.sort(key=lambda row: tuple(row[field] for field in order_fields))
                if writer is None:
                    writer = pq.ParquetWriter(destination, schema, compression="zstd")
                writer.write_table(pa.Table.from_pylist(result, schema=schema))
                rows_written += len(result)
            day += timedelta(days=1)
    finally:
        if writer is not None:
            writer.close()
    if not rows_written:
        raise ValueError(f"snapshot query produced no rows for {destination.name}")
    return rows_written


def _manifest_query() -> str:
    return """
      WITH coverage AS (
        SELECT 'goes'::text AS source_family, decision_time, product AS dimension,
               status, source_artifact_id, cropped_artifact_path, cropped_artifact_sha256
        FROM weather.goes_abi_window_coverage
        WHERE process_id=%s AND station_id=%s AND decision_time >= %s AND decision_time < %s
          AND feature_schema_version=%s
        UNION ALL
        SELECT 'hrrr_environment', decision_time, valid_at::text, status,
               source_artifact_id, cropped_artifact_path, cropped_artifact_sha256
        FROM weather.hrrr_environment_window_coverage
        WHERE process_id=%s AND station_id=%s AND decision_time >= %s AND decision_time < %s
          AND feature_schema_version=%s
      )
      SELECT c.source_family,c.decision_time,c.dimension,c.status,
             c.cropped_artifact_path,c.cropped_artifact_sha256,
             a.provider,a.logical_key,a.source_uri,a.sha256 AS source_sha256,
             a.compressed_bytes,a.source_start,a.source_end,a.metadata::text AS source_metadata
      FROM coverage c
      LEFT JOIN weather.source_artifacts a ON a.artifact_id=c.source_artifact_id
      ORDER BY c.source_family,c.decision_time,c.dimension
    """


def _verify_cropped_artifacts(rows: list[dict[str, Any]]) -> dict[str, int]:
    verified = missing = mismatched = 0
    seen: set[tuple[str, str]] = set()
    for row in rows:
        path_value = row.get("cropped_artifact_path")
        expected = row.get("cropped_artifact_sha256")
        if not path_value or not expected or (path_value, expected) in seen:
            continue
        seen.add((path_value, expected))
        path = Path(path_value)
        if not path.is_file():
            missing += 1
            continue
        actual, _ = file_sha256(path)
        if actual != expected:
            mismatched += 1
        else:
            verified += 1
    if missing or mismatched:
        raise RuntimeError(
            f"cropped artifact verification failed: missing={missing} mismatched={mismatched}"
        )
    return {"verified": verified, "missing": missing, "mismatched": mismatched}


def export_environment_snapshot(
    settings: Settings, *, start: date, end: date, snapshot_id: str
) -> dict[str, Any]:
    if not start < end:
        raise ValueError("snapshot end must be after start")
    if not snapshot_id or any(character not in "abcdefghijklmnopqrstuvwxyz0123456789-_" for character in snapshot_id):
        raise ValueError("snapshot_id must contain lowercase letters, digits, hyphens, or underscores")
    destination = settings.training_snapshot_directory / snapshot_id
    if destination.exists():
        raise FileExistsError(f"immutable snapshot already exists: {destination}")
    partial = destination.with_name(f".{destination.name}.{os.getpid()}.partial")
    partial.mkdir(parents=True, exist_ok=False)
    start_at = datetime.combine(start, datetime.min.time(), UTC)
    end_at = datetime.combine(end, datetime.min.time(), UTC)
    try:
        with connection(settings.database_url) as conn:
            satellite_rows = _write_weather_snapshot(
                conn, "goes_abi_features", start_at=start_at, end_at=end_at,
                version=GOES_VERSION, destination=partial / "satellite_training_matrix.parquet",
                order_fields=(
                    "decision_time", "requested_offset_minutes", "spatial_radius_km", "sector"
                ),
            )
            hrrr_rows = _write_weather_snapshot(
                conn, "hrrr_environment_features", start_at=start_at, end_at=end_at,
                version=HRRR_VERSION,
                destination=partial / "hrrr_environment_training_matrix.parquet",
                order_fields=("decision_time", "valid_at", "spatial_radius_km", "sector"),
            )
            manifest_rows = list(
                conn.execute(
                    _manifest_query(),
                    (
                        PROCESS_ID, STATION_ID, start_at, end_at, GOES_VERSION,
                        PROCESS_ID, STATION_ID, start_at, end_at, HRRR_VERSION,
                    ),
                ).fetchall()
            )
        artifact_verification = _verify_cropped_artifacts(manifest_rows)
        manifest_table = pa.Table.from_pylist([dict(row) for row in manifest_rows])
        pq.write_table(manifest_table, partial / "artifact_manifest.parquet", compression="zstd")
        file_manifest = {}
        for path in sorted(partial.glob("*.parquet")):
            digest, size = file_sha256(path)
            file_manifest[path.name] = {"sha256": digest, "bytes": size}
        manifest = {
            "snapshot_id": snapshot_id,
            "created_at": datetime.now(UTC).isoformat(),
            "process_id": PROCESS_ID,
            "station_id": STATION_ID,
            "range": {"start": start.isoformat(), "end_exclusive": end.isoformat()},
            "feature_schema_versions": {"goes": GOES_VERSION, "hrrr_environment": HRRR_VERSION},
            "rows": {
                "satellite_training_matrix": satellite_rows,
                "hrrr_environment_training_matrix": hrrr_rows,
                "artifact_manifest": len(manifest_rows),
            },
            "cropped_artifact_verification": artifact_verification,
            "files": file_manifest,
            "training_performed": False,
        }
        manifest_bytes = json.dumps(manifest, indent=2, sort_keys=True).encode()
        (partial / "snapshot-manifest.json").write_bytes(manifest_bytes)
        (partial / "snapshot.sha256").write_text(
            hashlib.sha256(manifest_bytes).hexdigest() + "  snapshot-manifest.json\n"
        )
        os.replace(partial, destination)
        return {**manifest, "path": str(destination)}
    except Exception:  # noqa: TRY203 - preserve a diagnostic partial snapshot.
        # Keep a failed partial snapshot for diagnosis; it can never be mistaken for immutable output.
        raise


def audit_environment_coverage(settings: Settings, *, start: date, end: date) -> dict[str, Any]:
    start_at = datetime.combine(start, datetime.min.time(), UTC)
    end_at = datetime.combine(end, datetime.min.time(), UTC)
    with connection(settings.database_url) as conn:
        satellite_coverage = list(
            conn.execute(
                """
                SELECT EXTRACT(HOUR FROM decision_time AT TIME ZONE 'America/New_York')::int AS decision_hour,
                       product,count(*)::int AS expected,
                       count(*) FILTER (
                         WHERE status IN ('complete','valid_zero','insufficient_valid_pixels')
                       )::int AS available,
                       count(*) FILTER (
                         WHERE status IN ('complete','valid_zero')
                       )::int AS quality_qualified
                FROM weather.goes_abi_window_coverage
                WHERE process_id=%s AND station_id=%s AND decision_time >= %s AND decision_time < %s
                  AND feature_schema_version=%s
                  AND product IN ('infrared_c13','clear_sky_mask','cloud_top_temperature',
                                  'cloud_top_height','visible_c02')
                GROUP BY 1,2 ORDER BY 1,2
                """,
                (PROCESS_ID, STATION_ID, start_at, end_at, GOES_VERSION),
            ).fetchall()
        )
        checks = conn.execute(
            """
            SELECT
              (SELECT count(*)::int FROM weather.goes_abi_features
                WHERE process_id=%s AND decision_time >= %s AND decision_time < %s
                  AND scan_end > decision_time - interval '15 minutes') AS causal_violations,
              (SELECT count(*)::int FROM weather.goes_abi_features
                WHERE process_id=%s AND decision_time >= %s AND decision_time < %s
                  AND (valid_pixel_fraction NOT BETWEEN 0 AND 1
                    OR infrared_brightness_temperature_mean_k NOT BETWEEN 100 AND 400))
                AS satellite_physical_bound_violations,
              (SELECT count(*)::int FROM weather.hrrr_environment_features
                WHERE process_id=%s AND decision_time >= %s AND decision_time < %s
                  AND (valid_pixel_fraction NOT BETWEEN 0 AND 1
                    OR temperature_2m_mean_k NOT BETWEEN 180 AND 340
                    OR total_cloud_cover_mean_fraction NOT BETWEEN 0 AND 1))
                AS hrrr_physical_bound_violations
            """,
            (
                PROCESS_ID, start_at, end_at,
                PROCESS_ID, start_at, end_at,
                PROCESS_ID, start_at, end_at,
            ),
        ).fetchone()
        transition = list(
            conn.execute(
                """
                SELECT satellite,
                       EXTRACT(HOUR FROM decision_time AT TIME ZONE 'America/New_York')::int AS decision_hour,
                       count(*)::int AS samples,
                       avg(infrared_brightness_temperature_mean_k) AS infrared_mean_k,
                       stddev_samp(infrared_brightness_temperature_mean_k) AS infrared_stddev_k
                FROM weather.goes_abi_features
                WHERE process_id=%s AND station_id=%s AND spatial_radius_km=100 AND sector='all'
                  AND requested_offset_minutes=15
                  AND decision_time >= timestamptz '2025-03-08 15:10:00+00'
                  AND decision_time < timestamptz '2025-05-08 15:10:00+00'
                  AND feature_schema_version=%s
                GROUP BY 1,2 ORDER BY 1,2
                """,
                (PROCESS_ID, STATION_ID, GOES_VERSION),
            ).fetchall()
        )
    coverage = []
    for row in satellite_coverage:
        item = dict(row)
        item["coverage_fraction"] = item["available"] / item["expected"] if item["expected"] else 0
        item["quality_qualified_fraction"] = (
            item["quality_qualified"] / item["expected"] if item["expected"] else 0
        )
        coverage.append(item)
    return {
        "range": {"start": start.isoformat(), "end_exclusive": end.isoformat()},
        "satellite_required_coverage": coverage,
        "minimum_required_coverage_met": bool(coverage) and all(
            row["coverage_fraction"] >= 0.95 for row in coverage
        ),
        "checks": dict(checks),
        "transition_distribution_audit": [dict(row) for row in transition],
    }
