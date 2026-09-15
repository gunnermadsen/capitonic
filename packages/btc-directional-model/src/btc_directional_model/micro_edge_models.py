"""Compact model recipes and chronological fitting for the micro-edge tournament."""

from __future__ import annotations

import json
from dataclasses import dataclass
from pathlib import Path

import numpy as np
import polars as pl
from scipy.special import logit
from sklearn.ensemble import HistGradientBoostingClassifier, HistGradientBoostingRegressor
from sklearn.impute import SimpleImputer
from sklearn.linear_model import LogisticRegression
from sklearn.pipeline import make_pipeline
from sklearn.preprocessing import StandardScaler


@dataclass(frozen=True)
class Recipe:
    name: str
    groups: tuple[str, ...]
    rtds: bool
    kind: str = "tree"
    hypothesis: str = ""
    baseline: dict | None = None


def recipes() -> list[Recipe]:
    groups = {
        "path_persistence": ("core",),
        "reversal_exhaustion": ("reversal",),
        "refprice_dislocation": ("core", "refprice", "oracle"),
        "partial_twap_bridge": ("core", "twap", "refprice", "oracle"),
        "crossvenue_leadlag": ("core", "kraken"),
        "flow_persistence": ("core", "binance_prints", "kraken"),
        "positioning": ("core", "open_interest", "binance_prints"),
        "liquidity_response": ("core", "spot_l2", "kraken_l2"),
        "q5_payoff": ("core", "oracle", "refprice", "execution"),
        "market_residual": ("core", "kraken", "execution"),
    }
    result = []
    for name, gs in groups.items():
        for rtds in (False, True):
            result.append(
                Recipe(
                    name + ("__rtds" if rtds else "__no_rtds"),
                    gs,
                    rtds,
                    "payoff" if name == "q5_payoff" else "tree",
                    name,
                )
            )
    all_groups = (
        "core",
        "oracle",
        "refprice",
        "twap",
        "open_interest",
        "binance_prints",
        "kraken",
        "spot_l2",
        "kraken_l2",
        "execution",
    )
    for rtds in (False, True):
        for name, gs, kind in [
            ("path_linear", ("core",), "linear"),
            ("crossvenue_linear", ("core", "kraken", "oracle"), "linear"),
            ("all_dimensions", all_groups, "tree"),
            ("normalized_boundary", all_groups, "normalized"),
            ("loss_aware", all_groups, "loss"),
            ("within_bucket_wait", all_groups, "wait"),
        ]:
            result.append(Recipe(name + ("__rtds" if rtds else "__no_rtds"), gs, rtds, kind, name))
    references = json.loads(
        (Path(__file__).resolve().parents[2] / "config/btc-micro-edge-baselines.json").read_text()
    )
    unique = {}
    for reference in references:
        params = {k: v for k, v in reference["estimator_parameters"].items() if k != "random_state"}
        key = json.dumps(
            [reference["features"], params, reference["start_second"], reference["end_second"]],
            sort_keys=True,
        )
        if key in unique:
            unique[key].baseline["equivalent_origins"].append(reference["run"])
            continue
        reference["equivalent_origins"] = [reference["run"]]
        recipe = Recipe(
            reference["name"], (), False, "legacy", "matched_prior_recipe_refit", reference
        )
        unique[key] = recipe
        result.append(recipe)
    return result


def feature_names(recipe: Recipe, groups: dict) -> list[str]:
    if recipe.baseline is not None:
        return recipe.baseline["features"]
    selected = []
    for group in recipe.groups:
        if group == "reversal":
            selected.extend(
                c
                for c in groups["core"]
                if any(
                    s in c
                    for s in (
                        "reversal",
                        "pullback",
                        "acceleration",
                        "cross",
                        "return",
                        "seconds_",
                        "volatility",
                    )
                )
            )
        else:
            selected.extend(groups[group])
        if group != "reversal":
            selected.append("has_" + group)
    if recipe.rtds:
        selected.extend(groups["candles"])
        selected.append("has_candles")
    selected += ["seconds_elapsed_scaled", "seconds_remaining_scaled", "settlement_regime"]
    if recipe.kind == "normalized":
        selected += ["normalized_boundary_gap", "normalized_twap_gap"]
    return list(dict.fromkeys(selected))


def fit_indices(frame: pl.DataFrame, test_start) -> np.ndarray:
    # Five-minute conservative label-availability embargo beyond market end.
    return np.flatnonzero(
        (frame["window_end"].to_numpy().astype("datetime64[us]") + np.timedelta64(5, "m"))
        < np.datetime64(test_start.replace(tzinfo=None), "us")
    )


def matrix(frame, features):
    x = frame.select([pl.col(c).cast(pl.Float32) for c in features]).to_numpy().copy()
    x[~np.isfinite(x)] = np.nan
    return x


def market_weights(frame):
    return (
        1.0
        / frame["market_id"]
        .value_counts()
        .rename({"count": "_count"})
        .join(frame.select("market_id").with_row_index("_i"), on="market_id")
        .sort("_i")["_count"]
        .to_numpy()
    ).astype(float)


def fit_estimator(x, y, w, kind, seed=20260915, parameters=None):
    usable = np.flatnonzero(np.sum(np.isfinite(x), axis=0) > 0)
    if not len(usable) or len(np.unique(y)) < 2:
        return {"constant": float(np.average(y, weights=w)), "columns": usable, "kind": kind}
    xx = x[:, usable]
    if kind == "legacy":
        model = HistGradientBoostingRegressor(**{**parameters, "random_state": seed})
        model.fit(xx, y, sample_weight=w * len(w) / w.sum())
    elif kind == "linear":
        model = make_pipeline(
            SimpleImputer(strategy="median", add_indicator=True),
            StandardScaler(),
            LogisticRegression(C=0.1, max_iter=160, solver="lbfgs", random_state=seed),
        )
        model.fit(xx, y, logisticregression__sample_weight=w * len(w) / w.sum())
    else:
        cls = (
            HistGradientBoostingRegressor
            if kind in ("payoff", "wait", "legacy")
            else HistGradientBoostingClassifier
        )
        model = cls(
            max_iter=60,
            max_leaf_nodes=15,
            min_samples_leaf=100,
            learning_rate=0.07,
            l2_regularization=3.0,
            max_bins=63,
            early_stopping=False,
            random_state=seed,
        )
        model.fit(xx, y, sample_weight=w * len(w) / w.sum())
    return {"estimator": model, "columns": usable, "kind": kind}


def predict_estimator(model, x):
    if "constant" in model:
        return np.full(len(x), model["constant"])
    obj = model["estimator"]
    xx = x[:, model["columns"]]
    return (
        obj.predict(xx)
        if model["kind"] in ("payoff", "wait", "legacy")
        else obj.predict_proba(xx)[:, 1]
    )


def fit_calibrator(p, y, regime=None):
    if regime is not None:
        return {
            "pooled": fit_calibrator(p, y),
            "regimes": {
                str(int(r)): fit_calibrator(p[regime == r], y[regime == r])
                for r in np.unique(regime)
            },
        }
    valid = np.isfinite(p) & np.isfinite(y)
    if valid.sum() < 30 or len(np.unique(y[valid])) < 2:
        return None
    c = LogisticRegression(C=1.0, max_iter=120)
    c.fit(logit(np.clip(p[valid], 1e-5, 1 - 1e-5)).reshape(-1, 1), y[valid])
    return c


def calibrate(p, c, regime=None):
    if isinstance(c, dict):
        result = calibrate(p, c["pooled"])
        if regime is not None:
            for r in np.unique(regime):
                specific = c["regimes"].get(str(int(r)))
                if specific is not None:
                    result[regime == r] = calibrate(p[regime == r], specific)
        return result
    p = np.clip(p, 1e-5, 1 - 1e-5)
    if c is not None:
        valid = np.isfinite(p)
        p = p.copy()
        if valid.any():
            p[valid] = c.predict_proba(logit(p[valid]).reshape(-1, 1))[:, 1]
    return p
