from datetime import UTC, datetime, timedelta

import numpy as np
import polars as pl

from btc_directional_model.micro_edge_data import attach_vwap5, causal_twap
from btc_directional_model.micro_edge_evaluation import (
    apply_policies,
    fit_policies,
    metrics,
    opportunities,
    router,
    select,
)
from btc_directional_model.micro_edge_models import (
    calibrate,
    fit_calibrator,
    fit_estimator,
    fit_indices,
    predict_estimator,
    recipes,
)


def frame():
    start = datetime(2026, 7, 1, tzinfo=UTC)
    return pl.DataFrame(
        {
            "market_id": ["a", "a", "b", "b"],
            "window_start": [
                start,
                start,
                start + timedelta(minutes=5),
                start + timedelta(minutes=5),
            ],
            "seconds_elapsed": [60, 110, 60, 110],
            "label_up": [1, 1, 0, 0],
        }
    ).with_columns(
        (pl.col("window_start") + pl.duration(minutes=5)).alias("window_end"),
        (pl.col("window_start") + pl.duration(seconds=pl.col("seconds_elapsed"))).alias(
            "observed_at"
        ),
        pl.lit(0.4).alias("up_ask_vwap_5"),
        pl.lit(0.6).alias("down_ask_vwap_5"),
        pl.lit(0.0).alias("fee_rate"),
        pl.lit(1.0).alias("pm_up_book_age_seconds"),
        pl.lit(1.0).alias("pm_down_book_age_seconds"),
        pl.lit(0.0).alias("settlement_regime"),
    )


def test_five_shares_accounting():
    f = frame()
    o = opportunities(f, np.array([0.8, 0.8, 0.2, 0.2]), "test")
    assert np.allclose(o["net_pnl"], [2.975, 2.975, 1.975, 1.975])
    assert np.allclose(o["net_pnl"] - o["stress_net_pnl"], 0.05)
    assert o.height == 4


def test_later_bucket_not_suppressed_in_independent_replay():
    o = opportunities(frame(), np.array([0.8, 0.8, 0.2, 0.2]), "test")
    assert select(o, (0.5, 0.01, 0.95, "both"), True).height == 4
    assert select(o, (0.5, 0.01, 0.95, "both")).height == 2
    policies = fit_policies(o)
    assert set(policies) == {60, 110}
    assert apply_policies(o, policies, True).height == 4
    assert apply_policies(o, policies).height == 2


def test_missing_price_or_fee_cannot_make_money():
    f = frame().with_columns(pl.lit(None, dtype=pl.Float64).alias("fee_rate"))
    assert opportunities(f, np.full(4, 0.8), "test").height == 0


def test_future_or_stale_book_rejected():
    f = frame().with_columns(pl.Series("pm_up_book_age_seconds", [-0.1, 2.1, 0.0, 2.0]))
    assert opportunities(f, np.full(4, 0.8), "test")["seconds_elapsed"].to_list() == [60, 110]


def test_vwap5_does_not_require_larger_depth():
    f = frame().drop(
        "up_ask_vwap_5",
        "down_ask_vwap_5",
        "pm_up_book_age_seconds",
        "pm_down_book_age_seconds",
        "fee_rate",
    )
    books = f.select("market_id", "observed_at").with_columns(
        pl.lit(0.4).alias("up_ask_vwap_5"),
        pl.lit(0.6).alias("down_ask_vwap_5"),
        pl.lit(0).alias("quality_flags"),
        pl.lit(0.0).alias("fee_rate"),
        pl.lit(5.0).alias("up_ask_depth"),
        pl.lit(5.0).alias("down_ask_depth"),
        pl.lit(0.4).alias("up_best_ask"),
        pl.lit(0.6).alias("down_best_ask"),
        pl.col("observed_at").alias("up_provider_received_at"),
        pl.col("observed_at").alias("down_provider_received_at"),
    )
    result = attach_vwap5(f, books)
    assert result["up_ask_vwap_5"].null_count() == 0
    bad = books.with_columns(
        (pl.col("observed_at") + pl.duration(seconds=1)).alias("up_provider_received_at")
    )
    result = attach_vwap5(f, bad)
    assert result["up_ask_vwap_5"].null_count() == 4
    assert result["down_ask_vwap_5"].null_count() == 0


def test_market_split_and_label_embargo():
    f = frame()
    start = datetime(2026, 7, 1, 0, 10, tzinfo=UTC)
    assert len(fit_indices(f, start)) == 0
    assert fit_indices(f, start + timedelta(seconds=1)).tolist() == [0, 1]


def test_compensation_and_drawdown():
    o = opportunities(frame(), np.array([0.8, 0.8, 0.8, 0.8]), "test")
    t = select(o, (0.5, 0.01, 0.95, "both"))
    m = metrics(t, 2)
    assert m["wins"] == 1 and m["losses"] == 1
    assert np.isclose(m["recovery_wins_per_loss"], 2.025 / 2.975)
    assert np.isclose(m["maximum_drawdown"], 2.025)


def test_router_one_order_and_deterministic_tie():
    o = opportunities(frame(), np.array([0.8, 0.8, 0.2, 0.2]), "b")
    other = o.with_columns(pl.lit("a").alias("candidate"))
    r = router(pl.concat([o, other]))
    assert r.height == 2
    assert r["candidate"].unique().to_list() == ["a"]


def test_models_accept_optional_missing_values():
    rng = np.random.default_rng(1)
    x = rng.normal(size=(300, 3))
    x[:150, 1] = np.nan
    x[:, 2] = np.nan
    y = (x[:, 0] > 0).astype(int)
    for kind in ("tree", "linear"):
        m = fit_estimator(x, y, np.ones(300), kind)
        p = predict_estimator(m, x)
        assert np.isfinite(p).all()
        assert 2 not in m["columns"]
        assert np.isfinite(calibrate(p, fit_calibrator(p, y))).all()


def test_unique_recipes_and_no_rtds_leak():
    rs = recipes()
    assert len({r.name for r in rs}) == len(rs)
    for r in rs:
        if not r.rtds:
            assert "candles" not in r.groups


def test_empty_sensor_preserves_prediction_rows():
    f = frame()
    result = causal_twap(f, pl.DataFrame())
    assert result.height == f.height
    assert result["twap60_margin_bps"].null_count() == f.height


def test_calibration_preserves_unavailable_predictions():
    p = np.linspace(0.1, 0.9, 100)
    y = (p > 0.5).astype(int)
    c = fit_calibrator(p, y)
    result = calibrate(np.array([0.2, np.nan, 0.8]), c)
    assert np.isnan(result[1]) and np.isfinite(result[[0, 2]]).all()


def test_future_reference_revisions_cannot_change_past_twap():
    f = frame().head(1)
    decision = f["observed_at"][0]
    times = [decision - timedelta(seconds=i) for i in range(180, 0, -1)]
    source = pl.DataFrame(
        {
            "source_timestamp": times,
            "provider_available_at": times,
            "price": np.linspace(60000, 60030, len(times)),
            "archive_row_number": list(range(len(times))),
        }
    )
    original = causal_twap(f, source)
    revision = source.tail(1).with_columns(
        pl.lit(decision + timedelta(seconds=10)).alias("provider_available_at"),
        pl.lit(100000.0).alias("price"),
    )
    appended = causal_twap(f, pl.concat([source, revision]))
    np.testing.assert_allclose(original["twap60_margin_bps"], appended["twap60_margin_bps"])


def test_checkpoint_resume_and_future_labels_do_not_change_first_fold(tmp_path, monkeypatch):
    import joblib
    import pytest

    from btc_directional_model import micro_edge_tournament as runner
    from btc_directional_model.micro_edge_models import Recipe

    start = datetime(2026, 6, 7, tzinfo=UTC)
    data = []
    for day in range(17):
        for market in range(12):
            at = start + timedelta(days=day, minutes=5 * market)
            for second in (60, 65):
                data.append(
                    {
                        "market_id": f"{day}-{market}",
                        "window_start": at,
                        "seconds_elapsed": second,
                        "label_up": market % 2,
                        "signal": (-1.0) ** market,
                    }
                )
    f = (
        pl.DataFrame(data)
        .with_columns(
            (pl.col("window_start") + pl.duration(minutes=5)).alias("window_end"),
            (pl.col("window_start") + pl.duration(seconds=pl.col("seconds_elapsed"))).alias(
                "observed_at"
            ),
            pl.lit(0.4).alias("up_ask_vwap_5"),
            pl.lit(0.6).alias("down_ask_vwap_5"),
            pl.lit(0.0).alias("fee_rate"),
            pl.lit(1.0).alias("pm_up_book_age_seconds"),
            pl.lit(1.0).alias("pm_down_book_age_seconds"),
            pl.lit(0.0).alias("settlement_regime"),
            pl.lit(0.2).alias("seconds_elapsed_scaled"),
            pl.lit(0.8).alias("seconds_remaining_scaled"),
            pl.lit(True).alias("has_core"),
        )
        .with_row_index("row_id")
    )
    boundary = start + timedelta(days=3)
    monkeypatch.setattr(runner, "START", start)
    monkeypatch.setattr(runner, "END", start + timedelta(days=17))
    monkeypatch.setattr(runner, "OOF_START", boundary)
    monkeypatch.setattr(runner, "EVAL_START", boundary)
    recipe = Recipe("checkpoint_test", ("core",), False)
    identity = {"source_commit": "test"}
    manifest = {"feature_groups": {"core": ["signal"]}}
    out = tmp_path / "original"
    runner.run_model(recipe, f, manifest, identity, out)
    final = out / "models/checkpoint_test.joblib"
    original = final.read_bytes()
    mtime = final.stat().st_mtime_ns
    runner.run_model(recipe, f, manifest, identity, out)
    assert final.stat().st_mtime_ns == mtime
    changed = f.with_columns(
        pl.when(pl.col("window_start") >= boundary)
        .then(1 - pl.col("label_up"))
        .otherwise(pl.col("label_up"))
        .alias("label_up")
    )
    runner.run_model(recipe, changed, manifest, identity, tmp_path / "perturbed")
    a = pl.read_parquet(out / "checkpoints/checkpoint_test/20260610/predictions.parquet")
    b = pl.read_parquet(
        tmp_path / "perturbed/checkpoints/checkpoint_test/20260610/predictions.parquet"
    )
    np.testing.assert_array_equal(a["raw_probability"], b["raw_probability"])
    assert joblib.load(final)["quantity"] == 5
    final.write_bytes(original + b"corrupt")
    with pytest.raises(RuntimeError, match="final artifact checkpoint mismatch"):
        runner.run_model(recipe, f, manifest, identity, out)


def test_unavailable_teacher_warmup_abstains():
    model = fit_estimator(np.empty((0, 2)), np.array([]), np.array([]), "wait")
    probability = predict_estimator(model, np.ones((3, 2)))
    assert model["untrained"]
    assert not (probability >= 0).any()
