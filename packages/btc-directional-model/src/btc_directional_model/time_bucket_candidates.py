"""Finite candidate and source-ablation identities for the offline tournament."""

from __future__ import annotations

from itertools import combinations

import polars as pl

from .time_bucket_panels import BOOK_FIELDS, CAPACITY_FIELDS, PRICE_FIELDS, TIME_FIELDS
from .time_bucket_source_audit import PRODUCTS

PRODUCT_NAMES = list(PRODUCTS)
REFERENCE_FIELDS = ["path_bps", "return_30_bps", "source_age_seconds", "latency_seconds"]
BASE = BOOK_FIELDS + TIME_FIELDS
CORE = BASE + [f"rtds_chainlink_{name}" for name in PRICE_FIELDS]
BINANCE = BASE + [f"direct_binance_{name}" for name in PRICE_FIELDS]
QUIET_FIELDS = ["reference_quiet", "reference_fills_300", "reference_fills_1800",
                "reference_no_admission_fraction", "reference_probability_up",
                "reference_seconds_since_fill_capped_1800"]


def prefixes(product: str) -> list[str]:
    return [f"{product}_{seconds}" for seconds in (30, 60)] if product.endswith("twap") else [product]


def reference_features(products: list[str]) -> list[str]:
    fields = [f"{prefix}_{name}" for product in products for prefix in prefixes(product)
              for name in REFERENCE_FIELDS]
    for product in products:
        if product.endswith("twap"):
            fields.extend([f"{product}_slope_30_60", f"{product}_convergence_30_60"])
    for left, right in combinations(products, 2):
        for lp, rp in zip(prefixes(left), prefixes(right), strict=False):
            fields.extend([f"{lp}__{rp}_basis_bps", f"{lp}__{rp}_convergence_bps"])
    return fields


def derived_features(frame: pl.DataFrame) -> pl.DataFrame:
    """Only contemporaneously observable differences; no outcome-based construction."""
    exprs = []
    for product in PRODUCT_NAMES:
        if product.endswith("twap"):
            a, b = prefixes(product)
            if f"{a}_price" in frame and f"{b}_price" in frame:
                exprs.extend([
                    ((pl.col(f"{a}_price") / pl.col(f"{b}_price") - 1) * 10000).alias(f"{product}_slope_30_60"),
                    (pl.col(f"{a}_return_30_bps") - pl.col(f"{b}_return_30_bps")).alias(f"{product}_convergence_30_60"),
                ])
    for left, right in combinations(PRODUCT_NAMES, 2):
        for lp, rp in zip(prefixes(left), prefixes(right), strict=False):
            if f"{lp}_price" in frame and f"{rp}_price" in frame:
                exprs.extend([
                    ((pl.col(f"{lp}_price") / pl.col(f"{rp}_price") - 1) * 10000).alias(f"{lp}__{rp}_basis_bps"),
                    (pl.col(f"{lp}_return_30_bps") - pl.col(f"{rp}_return_30_bps")).alias(f"{lp}__{rp}_convergence_bps"),
                ])
    for name in ["path_bps", "return_5_bps", "return_30_bps", "volatility_30_bps"]:
        source = f"rtds_chainlink_{name}"
        if source in frame:
            exprs.append((pl.col(source) * pl.col("seconds_elapsed_scaled")).alias(f"elapsed_x_{name}"))
    if "rtds_chainlink_price" in frame and "direct_binance_price" in frame:
        exprs.extend([
            ((pl.col("direct_binance_price") / pl.col("rtds_chainlink_price") - 1) * 10000)
            .alias("binance_usdt_chainlink_usd_apparent_basis_bps"),
            (pl.col("direct_binance_path_bps") - pl.col("rtds_chainlink_path_bps"))
            .alias("binance_chainlink_path_disagreement_bps"),
        ])
    spot, future = "binance_spot_l2_features_midpoint", "binance_futures_l2_features_midpoint"
    if spot in frame and future in frame:
        exprs.append(((pl.col(future) / pl.col(spot) - 1) * 10000).alias("binance_futures_spot_midpoint_basis_bps"))
    return frame.with_columns(exprs)


def registry(flow_groups: dict[str, list[str]]) -> list[dict]:
    """Eight challengers, with explicitly subordinate arms, never extra candidates."""
    arms: list[dict] = []

    def add(candidate: str, arm: str, features: list[str], hypothesis: str, **extra: object) -> None:
        arms.append({"candidate": candidate, "arm": arm, "features": features,
                     "hypothesis": hypothesis, "matched_products": [], "entry_offsets": [19],
                     "primary": arm == "primary", "head": "direction", **extra})

    add("conservative_selective_refresh", "primary", CORE,
        "Selective calibrated BTC direction can retain positive stressed expectancy on the expanded valid calendar.")
    add("conservative_selective_refresh", "direct_binance_core", BINANCE,
        "A separately identified direct Binance core tests independent broader causal price coverage.")
    add("conservative_selective_refresh", "capacity", CORE + CAPACITY_FIELDS,
        "Observed sweep-cost slopes can reject poor payoff capacity without truncating the primary core.")
    add("time_bucket_conditional_selector", "primary", CORE + [f"elapsed_x_{n}" for n in
        ["path_bps", "return_5_bps", "return_30_bps", "volatility_30_bps"]],
        "Elapsed-time interactions localize repeatable opportunity; calibration alone selects the time mask.")
    add("refprice_twap_consensus_residual", "primary", BASE + reference_features(PRODUCT_NAMES),
        "Separately identified reference and TWAP disagreement and convergence predict stressed value.")
    for count in range(1, 5):
        for product_subset in combinations(PRODUCT_NAMES, count):
            ids = [str(PRODUCT_NAMES.index(p) + 1) for p in product_subset]
            add("refprice_twap_consensus_residual", "matched_" + "_".join(ids), BASE + reference_features(list(product_subset)),
                "Matched-population source ablation isolates incremental information.", matched_products=PRODUCT_NAMES)
    for i, product in enumerate(PRODUCT_NAMES, 1):
        add("refprice_twap_consensus_residual", f"broader_product_{i}", BASE + reference_features([product]),
            "The individual product retains its entire causal history outside the all-four intersection.")
    # Each source has a separate complete-case arm. The primary is the broad candle pathway.
    for product, features in flow_groups.items():
        add("cross_venue_flow", "primary" if product == "binance_spot_one_second_ohlcv" else product,
            BASE + features, "Causal external price discovery and flow precede Polymarket adjustment.")
    all_flow = [name for names in flow_groups.values() for name in names]
    add("cross_venue_flow", "all_covered_flow", BASE + all_flow,
        "Joint cross-venue context is evaluated only on the exact all-source intersection.")
    add("cross_venue_flow", "binance_chainlink_basis", CORE + [
        "binance_usdt_chainlink_usd_apparent_basis_bps", "binance_chainlink_path_disagreement_bps"],
        "Apparent BTCUSDT/BTCUSD basis and path disagreement precede a Polymarket adjustment; this includes quote-currency effects.")
    add("cross_venue_flow", "spot_futures_basis", BASE + flow_groups["binance_spot_l2_features"]
        + flow_groups["binance_futures_l2_features"] + ["binance_futures_spot_midpoint_basis_bps"],
        "Contemporaneous spot-versus-futures order-book basis adds value where both original source states exist.")
    add("reversal_exhaustion_within_bucket_wait", "primary", CORE,
        "Causal exhaustion and first eligible within-bucket stopping improve entry payoff.", entry_offsets=[0, 9, 19])
    add("quiet_explorer", "primary", CORE + QUIET_FIELDS,
        "Known quiet under simulated refresh activity contains independently profitable opportunities.", quiet_only=True)
    add("quiet_explorer", "reference_disagreement", CORE + QUIET_FIELDS + reference_features(PRODUCT_NAMES),
        "Quiet disagreement has incremental value where all four mandatory products are observed.", quiet_only=True)
    add("two_sided_buy_sell", "primary", CORE,
        "A separately trained hold-versus-sell head improves stressed recovery after each admitted entry.", head="direction_and_exit")
    add("payoff_recovery_aware_selector", "primary", CORE,
        "Expected stressed value rejects high-confidence entries with poor payoff asymmetry.", head="direction_and_value")
    if len({a["candidate"] for a in arms}) != 8:
        raise ValueError("The tournament must contain exactly eight challenger identities")
    return arms


def complete_cases(frame: pl.DataFrame, arm: dict) -> pl.DataFrame:
    required = list(arm["features"])
    for product in arm["matched_products"]:
        required.extend(reference_features([product]))
    required = list(dict.fromkeys(required))
    missing = set(required) - set(frame.columns)
    if missing:
        raise ValueError(f"Panel schema lacks declared features: {sorted(missing)}")
    result = frame.filter(pl.all_horizontal(pl.col(name).is_not_null() & pl.col(name).is_finite()
                                           for name in required))
    if arm.get("quiet_only"):
        result = result.filter(pl.col("reference_quiet") == 1)
    return result
