from __future__ import annotations

import hashlib
from datetime import UTC, datetime, timedelta

import pyarrow as pa
import pyarrow.parquet as pq
import pytest

from btc_directional_model.drained_sources import removed_source_rows


class _Cursor:
    def __init__(self, publication: tuple, source_count: int) -> None:
        self.publication = publication
        self.source_count = source_count
        self.query = ""

    def __enter__(self):
        return self

    def __exit__(self, *_):
        return False

    def execute(self, query, _parameters):
        self.query = query

    def fetchall(self):
        return [self.publication]

    def fetchone(self):
        return (self.source_count,)


class _Connection:
    def __init__(self, publication: tuple, source_count: int = 0) -> None:
        self.publication = publication
        self.source_count = source_count

    def cursor(self, **_):
        return _Cursor(self.publication, self.source_count)


def test_removed_source_rows_verifies_file_and_rejects_live_overlap(tmp_path):
    path = tmp_path / "verified-chunks-v1" / "part.parquet"
    path.parent.mkdir()
    pq.write_table(
        pa.table(
            {
                "source_timestamp": ["2026-07-03 00:00:00+00", "2026-07-03 00:05:00+00"],
                "value": ["12", "13"],
            }
        ),
        path,
    )
    start = datetime(2026, 7, 2, tzinfo=UTC)
    end = start + timedelta(days=7)
    publication = (
        "market_data.example",
        start,
        end,
        "verified-chunks-v1/part.parquet",
        hashlib.sha256(path.read_bytes()).hexdigest(),
        path.stat().st_size,
        2,
    )
    arguments = {
        "strategy_key": "example",
        "relation": "market_data.example",
        "time_column": "source_timestamp",
        "root": tmp_path,
        "range_start": start,
        "range_end": end,
        "columns": ("source_timestamp", "value"),
    }
    frame = removed_source_rows(_Connection(publication), **arguments)
    assert frame.height == 2
    assert frame["value"].to_list() == ["12", "13"]
    with pytest.raises(RuntimeError, match="overlaps live source rows"):
        removed_source_rows(_Connection(publication, source_count=1), **arguments)
    with pytest.raises(RuntimeError, match="hash or size mismatch"):
        removed_source_rows(
            _Connection(publication[:4] + ("0" * 64,) + publication[5:]), **arguments
        )
