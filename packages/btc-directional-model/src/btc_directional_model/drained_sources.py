from __future__ import annotations

import hashlib
from datetime import datetime
from pathlib import Path

import polars as pl
import psycopg
import pyarrow.parquet as pq
from psycopg.rows import tuple_row


def completed_capture_ids(connection: psycopg.Connection, identifiers: list[str]) -> set[str]:
    completed: set[str] = set()
    with connection.cursor(row_factory=tuple_row) as cursor:
        for offset in range(0, len(identifiers), 1000):
            cursor.execute(
                "SELECT artifact_id::text FROM ingester.capture_artifacts "
                "WHERE artifact_id = ANY(%s::uuid[]) AND status='completed'",
                (identifiers[offset : offset + 1000],),
            )
            completed.update(row[0] for row in cursor.fetchall())
    return completed


def completed_backfill_providers(
    connection: psycopg.Connection, identifiers: list[str]
) -> dict[str, str]:
    completed: dict[str, str] = {}
    with connection.cursor(row_factory=tuple_row) as cursor:
        for offset in range(0, len(identifiers), 1000):
            cursor.execute(
                "SELECT artifact_id::text, provider FROM ingester.backfill_artifacts "
                "WHERE artifact_id = ANY(%s::uuid[]) AND status='completed'",
                (identifiers[offset : offset + 1000],),
            )
            completed.update(cursor.fetchall())
    return completed


def require_no_removed_chunks(
    connection: psycopg.Connection,
    *,
    strategy_keys: tuple[str, ...],
    range_start: datetime,
    range_end: datetime,
) -> None:
    """Prevent a PostgreSQL-only extractor from silently omitting removed rows."""
    with connection.cursor(row_factory=tuple_row) as cursor:
        cursor.execute(
            """
            SELECT strategy_key, source_start, source_end
            FROM ingester.drain_objects
            WHERE strategy_key = ANY(%s) AND status = 'removed'
              AND source_end > %s AND source_start < %s
            ORDER BY source_start
            LIMIT 1
            """,
            (list(strategy_keys), range_start, range_end),
        )
        overlap = cursor.fetchone()
    if overlap is not None:
        strategy_key, source_start, source_end = overlap
        raise RuntimeError(
            "PostgreSQL-only training extraction would omit verified drained rows: "
            f"{strategy_key} [{source_start}, {source_end}); "
            "use the canonical drain Parquet reader"
        )


def removed_source_rows(
    connection: psycopg.Connection,
    *,
    strategy_key: str,
    relation: str,
    time_column: str,
    root: Path,
    range_start: datetime,
    range_end: datetime,
    columns: tuple[str, ...],
) -> pl.DataFrame:
    """Read only removed chunks from their verified drain publications."""
    with connection.cursor(row_factory=tuple_row) as cursor:
        cursor.execute(
            """
            SELECT source_relation, source_start, source_end, relative_path,
                   sha256, byte_size, row_count
            FROM ingester.drain_objects
            WHERE strategy_key=%s AND status='removed'
              AND source_end > %s AND source_start < %s
            ORDER BY source_start
            """,
            (strategy_key, range_start, range_end),
        )
        publications = cursor.fetchall()
    frames: list[pl.DataFrame] = []
    if publications:
        root = root.resolve(strict=True)
    for (
        recorded_relation,
        source_start,
        source_end,
        relative_path,
        expected_hash,
        expected_size,
        expected_rows,
    ) in publications:
        if recorded_relation != relation:
            raise RuntimeError(f"drain publication source relation mismatch: {strategy_key}")
        path = (root / relative_path).resolve(strict=True)
        if not path.is_relative_to(root) or not path.is_file():
            raise RuntimeError(f"drain publication is outside its source root: {relative_path}")
        digest = hashlib.sha256()
        byte_size = 0
        with path.open("rb") as source:
            while block := source.read(1024 * 1024):
                digest.update(block)
                byte_size += len(block)
        if digest.hexdigest() != expected_hash or byte_size != expected_size:
            raise RuntimeError(f"drain publication hash or size mismatch: {path}")
        parquet = pq.ParquetFile(path)
        if parquet.metadata.num_rows != expected_rows:
            raise RuntimeError(f"drain publication row count mismatch: {path}")
        frame = pl.from_arrow(parquet.read(columns=list(columns)))
        if frame.height != expected_rows:
            raise RuntimeError(f"drain publication payload row count mismatch: {path}")
        with connection.cursor(row_factory=tuple_row) as cursor:
            cursor.execute(
                f"SELECT count(*) FROM {relation} WHERE {time_column} >= %s AND {time_column} < %s",
                (source_start, source_end),
            )
            if cursor.fetchone()[0] != 0:
                raise RuntimeError(f"drain publication overlaps live source rows: {path}")
        frames.append(frame)
    if not frames:
        return pl.DataFrame({column: [] for column in columns})
    return pl.concat(frames, how="vertical_relaxed", rechunk=True)
