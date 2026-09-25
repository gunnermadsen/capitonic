from __future__ import annotations

import hashlib
import json
from datetime import UTC, datetime
from types import SimpleNamespace

import pyarrow as pa
import pyarrow.parquet as pq
import pytest

from nyc_temperature_model import PROCESS_ID, STATION_ID
from nyc_temperature_model import environment_snapshot as snapshot
from nyc_temperature_model.sources import file_sha256


def test_verified_weather_archive_checks_source_rows_and_file_hash(tmp_path, monkeypatch):
    monkeypatch.setattr(snapshot, "_WEATHER_DRAIN_ROOT", tmp_path)
    root = tmp_path / "goes-abi-features"
    root.mkdir()
    row = {"process_id": PROCESS_ID, "decision_time": "2026-01-01T00:00:00+00:00"}
    encoded = json.dumps(row, sort_keys=True)
    relative = "verified-rows-v1/year=2026/month=01/day=01/test.parquet"
    path = root / relative
    path.parent.mkdir(parents=True)
    pq.write_table(pa.table({"source_row_json": [encoded],
                             "process_id": [PROCESS_ID],
                             "decision_time": [row["decision_time"]]}), path)
    digest, size = file_sha256(path)
    object_row = {"relative_path": relative, "sha256": digest, "byte_size": size,
                  "row_count": 1, "source_rows_sha256": hashlib.sha256(encoded.encode()).hexdigest()}
    assert list(snapshot._archive_rows(object_row, "goes_abi_features")) == [row]
    with pytest.raises(RuntimeError, match="source fingerprint"):
        list(snapshot._archive_rows({**object_row, "source_rows_sha256": "0" * 64},
                                    "goes_abi_features"))
    with pytest.raises(RuntimeError, match="hash or size"):
        list(snapshot._archive_rows({**object_row, "sha256": "0" * 64},
                                    "goes_abi_features"))


def test_weather_snapshot_merges_removed_archive_with_retained_rows(tmp_path, monkeypatch):
    start = datetime(2026, 1, 1, tzinfo=UTC)
    fields = [
        ("process_id", 2950), ("station_id", 25), ("decision_time", 1184),
        ("requested_offset_minutes", 23), ("spatial_radius_km", 23),
        ("sector", 25), ("feature_schema_version", 25), ("source_metadata", 3802),
        ("station_daily_max_f", 1700), ("station_rounded_max_f", 23),
        ("winner_matches_station", 16),
    ]
    retained = {
        "process_id": PROCESS_ID, "station_id": STATION_ID, "decision_time": start,
        "requested_offset_minutes": 60, "spatial_radius_km": 25, "sector": "all",
        "feature_schema_version": "goes-abi-klga-v2", "source_metadata": {"source": "db"},
        "station_daily_max_f": None, "station_rounded_max_f": None,
        "winner_matches_station": None,
    }
    archived = {**retained, "requested_offset_minutes": 15,
                "decision_time": start.isoformat(), "source_metadata": {"source": "archive"}}
    for label in ("station_daily_max_f", "station_rounded_max_f", "winner_matches_station"):
        archived.pop(label)

    class Cursor:
        def __init__(self):
            self.read = False
            self.description = [
                SimpleNamespace(name=name, type_code=type_code)
                for name, type_code in fields
            ]

        def execute(self, query, parameters):
            assert "weather.goes_abi_features" in query

        def fetchmany(self, count):
            if self.read:
                return []
            self.read = True
            return [retained]

        def close(self):
            pass

    class Connection:
        def execute(self, query, parameters):
            return SimpleNamespace(fetchall=list)

        def cursor(self, name):
            return Cursor()

    monkeypatch.setattr(snapshot, "_removed_weather_objects", lambda *args: [
        {"source_start": start}])
    monkeypatch.setattr(snapshot, "_archive_rows", lambda *args: iter([archived]))
    output = tmp_path / "snapshot.parquet"
    assert snapshot._write_weather_snapshot(
        Connection(), "goes_abi_features", start_at=start,
        end_at=datetime(2026, 1, 2, tzinfo=UTC), version="goes-abi-klga-v2",
        destination=output,
        order_fields=("decision_time", "requested_offset_minutes", "spatial_radius_km", "sector"),
    ) == 2
    rows = pq.read_table(output).to_pylist()
    assert [row["requested_offset_minutes"] for row in rows] == [15, 60]
    assert [json.loads(row["source_metadata"])["source"] for row in rows] == ["archive", "db"]

    archived["requested_offset_minutes"] = 60
    with pytest.raises(RuntimeError, match="overlap"):
        snapshot._write_weather_snapshot(
            Connection(), "goes_abi_features", start_at=start,
            end_at=datetime(2026, 1, 2, tzinfo=UTC), version="goes-abi-klga-v2",
            destination=tmp_path / "overlap.parquet",
            order_fields=("decision_time", "requested_offset_minutes", "spatial_radius_km", "sector"),
        )
