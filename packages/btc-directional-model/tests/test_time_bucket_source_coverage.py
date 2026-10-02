from datetime import UTC, datetime

import polars as pl

from btc_directional_model.time_bucket_market_sources import joined_coverage_by_day_bucket
from btc_directional_model.time_bucket_source_audit import PRODUCTS


def test_matched_coverage_requires_each_product_and_both_twap_windows():
    data = {
        "decision_at": [datetime(2026, 8, 21, tzinfo=UTC)] * 4,
        "bucket_start": [0, 0, 0, 0],
    }
    for product in PRODUCTS:
        for window in [30, 60] if product.endswith("twap") else [None]:
            suffix = f"{product}_{window}" if window else product
            data[f"{suffix}_source_age_seconds"] = [1.0, 1.0, 1.0, 1.0]
    data["polymarket_chainlink_btcusd_twap_60_source_age_seconds"] = [1.0, None, 6.0, -1.0]
    result = joined_coverage_by_day_bucket(pl.DataFrame(data))
    matched = result.filter(pl.col("product") == "all_four_products")
    assert matched["observed_market_decisions"].to_list() == [4, 4, 4]
    assert matched["causal_fresh_decisions"].to_list() == [1, 1, 2]
    direct = result.filter(pl.col("product") == "chainlink_btcusd_reference_prices")
    assert direct["causal_fresh_decisions"].to_list() == [4, 4, 4]
