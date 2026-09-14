from __future__ import annotations

from datetime import UTC, datetime
from pathlib import Path

import polars as pl

from btc_directional_model.time_bucket_source_preparation import _tail_labels


def test_tail_labels_accept_only_unique_causal_twap_reports(tmp_path: Path) -> None:
    source = tmp_path / "source"
    labels = source / "labels"
    labels.mkdir(parents=True)
    start = datetime(2026, 9, 1, tzinfo=UTC)
    frame = pl.DataFrame(
        {
            "market_id": ["valid", "ambiguous"],
            "window_start": [start, start],
            "window_end": [start, start],
            "official_outcome": ["up", "up"],
            "twap_open_price": [100.0, 100.0],
            "twap_close_price": [101.0, 101.0],
            "twap_open_effective_timestamp_rows": [1, 2],
            "twap_close_effective_timestamp_rows": [1, 1],
            "twap_open_valid_from_timestamp": [start, start],
            "twap_open_source_timestamp": [start, start],
            "twap_close_valid_from_timestamp": [start, start],
            "twap_close_source_timestamp": [start, start],
        }
    )
    path = labels / "2026-09-01.parquet"
    frame.write_parquet(path)
    import hashlib
    import json

    digest = hashlib.sha256(path.read_bytes()).hexdigest()
    (source / "source-manifest.json").write_text(
        json.dumps({"partitions": {"labels": [{"path": "labels/2026-09-01.parquet", "sha256": digest}]}})
    )
    result = _tail_labels(source, start)
    assert result["market_id"].to_list() == ["valid"]
    assert result["label_source"].to_list() == ["authentic_official_twap60"]
    assert result["label_up"].to_list() == [1]
