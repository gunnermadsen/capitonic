//! Optional summaries of the existing shared RTDS and TWAP context. No feed or execution owner.
use super::{reassessment::positive, EvaluationRequest, EvaluationResult};
use crate::{
    btc::types::RealtimeState,
    market_data_stream::{SourceSelector, PRODUCT_CHAINLINK},
};
use chrono::{DateTime, Duration, Utc};
use rust_decimal::{prelude::ToPrimitive, Decimal};
use serde::Serialize;
use serde_json::{json, Value};

#[derive(Serialize)]
struct PathSummary {
    source_start: DateTime<Utc>,
    source_end: DateTime<Utc>,
    available_at: DateTime<Utc>,
    sample_interval_seconds: i64,
    return_count: usize,
    log_return_bps: f64,
    sample_volatility_bps: f64,
    path_efficiency: Option<f64>,
    reversal_count: usize,
}

// Descriptive arithmetic over supplied shared observations, never a candle builder.
fn summarize(
    samples: &[(DateTime<Utc>, DateTime<Utc>, Decimal)],
    interval: i64,
) -> Option<PathSummary> {
    if samples.len() < 3
        || samples
            .windows(2)
            .any(|p| p[1].0 - p[0].0 != Duration::seconds(interval))
    {
        return None;
    }
    let logs: Option<Vec<_>> = samples
        .iter()
        .map(|p| {
            p.2.to_f64()
                .filter(|v| v.is_finite() && *v > 0.0)
                .map(f64::ln)
        })
        .collect();
    let logs = logs?;
    let returns: Vec<_> = logs.windows(2).map(|p| p[1] - p[0]).collect();
    let net = logs.last()? - logs.first()?;
    let absolute: f64 = returns.iter().map(|v| v.abs()).sum();
    let nonzero: Vec<_> = returns.iter().filter(|v| **v != 0.0).collect();
    Some(PathSummary {
        source_start: samples.first()?.0,
        source_end: samples.last()?.0,
        available_at: samples.iter().map(|p| p.1).max()?,
        sample_interval_seconds: interval,
        return_count: returns.len(),
        log_return_bps: net * 10_000.0,
        sample_volatility_bps: crate::btc::directional_features::rolling_volatility(
            &returns,
            returns.len(),
            returns.len(),
            returns.len(),
        )? * 10_000.0,
        path_efficiency: (absolute > 0.0).then(|| (net.abs() / absolute).min(1.0)),
        reversal_count: nonzero
            .windows(2)
            .filter(|p| p[0].is_sign_positive() != p[1].is_sign_positive())
            .count(),
    })
}

fn horizon(window_seconds: i64, summary: &Option<PathSummary>, missing: &str) -> Value {
    json!({"window_seconds":window_seconds,"status":if summary.is_some(){"available"}else{missing},"observation":summary})
}

/// Extend the existing versioned agent context immediately before its normal input hash.
pub fn add_market_context(
    request: &mut EvaluationRequest,
    state: &RealtimeState,
    sources: &[SourceSelector],
    previous: Option<&EvaluationResult>,
) {
    let at = request.observed_at;
    let selector = sources.iter().find(|s| s.key == PRODUCT_CHAINLINK);
    let repository = state.directional_external.rtds();
    let last = repository.points_as_of(at).last();
    let fresh = selector.zip(last).is_some_and(|(s, p)| {
        (at - p.source_timestamp).num_milliseconds() <= s.effective_maximum_age_ms() as i64
    });
    let missing = if selector.is_none() {
        "not_selected"
    } else if !fresh {
        "missing_or_stale_reference"
    } else {
        "incomplete_history"
    };
    let (mut short, mut five, mut fifteen) = (None, None, None);
    if fresh {
        let end = last.unwrap().source_timestamp;
        let samples: Vec<_> = repository
            .points_as_of(at)
            .filter(|p| p.source_timestamp >= end - Duration::seconds(15))
            .map(|p| (p.source_timestamp, p.available_at, p.price))
            .collect();
        if samples.len() == 16 {
            short = summarize(&samples, 1);
        }
        // Reuse the repository's closed-candle semantics and gap handling verbatim.
        if let Ok(candles) = repository.closed_candles(at) {
            let samples: Vec<_> = candles
                .iter()
                .map(|c| (c.close_timestamp, c.available_at, c.close_price))
                .collect();
            if samples.len() >= 16 {
                five = summarize(&samples[samples.len() - 6..], 60);
                fifteen = summarize(&samples[samples.len() - 16..], 60);
            }
        }
    }
    request.context["market_horizons"] = json!({
        "source":PRODUCT_CHAINLINK,"observed_at":at,
        "meaning":"Supporting midpoint observations, not settlement TWAP. Log returns and sample standard deviations are basis points. Volatility is per stated sampling interval, not annualized. Path efficiency is absolute net log return / summed absolute log returns (null on a flat path). Reversals count sign changes between nonzero sampled returns. Short history requires contiguous one-second points; longer histories use existing shared closed-minute candles, whose availability does not assert every second was present. No gap filling. Null means unknown.",
        "windows":[horizon(15,&short,missing),horizon(300,&five,missing),horizon(900,&fifteen,missing)]
    });
    let price = |path| positive(&request.context, path).and_then(|p| p.to_f64());
    let opening = price("/twap_context/opening/price");
    let current = price("/twap_context/current/price");
    let log_gap =
        |a: Option<f64>, b: Option<f64>| a.zip(b).map(|(a, b)| (b.ln() - a.ln()) * 10_000.0);
    let distance = log_gap(opening, current);
    let reference = last.filter(|_| fresh).and_then(|p| p.price.to_f64());
    let gap = log_gap(current, reference);
    let window_end = request
        .context
        .pointer("/execution_snapshot/window_end")
        .and_then(|v| serde_json::from_value::<DateTime<Utc>>(v.clone()).ok());
    let remaining = window_end.map(|end| (end - at).num_milliseconds().max(0) as f64 / 1000.0);
    let scale = fifteen
        .as_ref()
        .zip(remaining)
        .map(|(s, t)| s.sample_volatility_bps * (t / 60.0).sqrt());
    let normalized = distance
        .zip(scale)
        .filter(|(_, scale)| *scale > 0.0)
        .map(|(d, s)| d / s)
        .filter(|v| v.is_finite());
    // The existing comparison validates process/run/profile/market identity, causality and hash.
    let prior_gap = request
        .context
        .get("previous_prediction_comparison")
        .filter(|v| !v.is_null())
        .and(previous)
        .and_then(|p| {
            p.request
                .context
                .pointer("/settlement_uncertainty/reference_to_twap_log_gap_bps")
        })
        .and_then(Value::as_f64);
    request.context["settlement_uncertainty"] = json!({
        "twap_status":request.context["twap_context_status"],
        "remaining_seconds":remaining,"twap_log_distance_from_open_bps":distance,
        "variability_source":PRODUCT_CHAINLINK,"variability_window_seconds":900,
        "variability_sample_interval_seconds":60,"variability_return_count":fifteen.as_ref().map(|s|s.return_count),
        "remaining_midpoint_variability_scale_bps":scale,
        "twap_distance_in_midpoint_scale_units":normalized,
        "reference_to_twap_log_gap_bps":gap,
        "reference_source_timestamp":last.filter(|_|fresh).map(|p|p.source_timestamp),
        "twap_source_timestamp":request.context.pointer("/twap_context/current/source_timestamp"),
        "absolute_reference_twap_gap_change_bps":gap.zip(prior_gap).map(|(now,old)|now.abs()-old.abs()),
        "meaning":"TWAP distance uses only the existing opening/current official TWAP context. Remaining variability = the 15-minute sample standard deviation of one-minute midpoint log returns times sqrt(remaining_seconds/60). This stationary independent-return, zero-drift scale ignores TWAP averaging; it is neither settlement-price volatility nor a probability or confidence interval. A zero scale yields null normalized distance, never certainty. Reference/TWAP gaps compare distinct products at the displayed times; a negative absolute gap change since the verified previous request indicates narrowing, not guaranteed convergence. Missing inputs remain null."
    });
    request.context["version"] = json!(super::MARKET_CONTEXT_VERSION);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::btc::unified_model_runtime::agent::{self, AgentSelection, Prediction, UsageReport};
    use crate::btc::{rtds_repository::RtdsPoint, types::BtcOutcome};
    use chrono::TimeZone;
    use uuid::Uuid;

    fn fixture() -> (EvaluationRequest, RealtimeState, Vec<SourceSelector>) {
        let at = Utc.with_ymd_and_hms(2026, 10, 9, 0, 6, 0).unwrap();
        let profile = agent::profile_for_key(agent::MARKET_CONTEXT_PROFILE_KEY).unwrap();
        let request = EvaluationRequest {
            version: agent::BRIDGE_VERSION.into(),
            request_id: Uuid::new_v4(),
            process_id: Uuid::new_v4(),
            run_id: Uuid::new_v4(),
            config_hash: "config".into(),
            selection: AgentSelection {
                profile_key: profile.key.into(),
                profile_sha256: agent::hash(&profile).unwrap(),
            },
            market_id: "market".into(),
            window_start: at - Duration::seconds(60),
            snapshot_id: Uuid::new_v4(),
            observed_at: at,
            deadline: at + Duration::seconds(60),
            attempt: 60,
            input_sha256: String::new(),
            context: json!({
                "execution_snapshot":{"window_end":at+Duration::seconds(240)},
                "twap_context":{"opening":{"price":"100"},"current":{"price":"101","source_timestamp":at}},
                "twap_context_status":"available","historical_calibration":{"status":"available","groups":[]},
                "rtds_volatility":{"observation":null}}),
        };
        let mut state = RealtimeState::default();
        for second in 0..=3660 {
            let timestamp = at - Duration::seconds(3660 - second);
            std::sync::Arc::make_mut(&mut state.directional_external.rtds).insert_fixture(
                RtdsPoint {
                    source_timestamp: timestamp,
                    available_at: timestamp,
                    price: Decimal::from(100 + (second / 60) % 2),
                },
            );
        }
        (
            request,
            state,
            vec![serde_json::from_value(json!(PRODUCT_CHAINLINK)).unwrap()],
        )
    }

    #[test]
    fn horizons_use_shared_candles_and_preserve_settlement_units_and_existing_context() {
        let (mut r, state, sources) = fixture();
        let original = r.context.clone();
        add_market_context(&mut r, &state, &sources, None);
        for key in [
            "historical_calibration",
            "twap_context",
            "rtds_volatility",
            "execution_snapshot",
        ] {
            assert_eq!(r.context[key], original[key]);
        }
        let w = &r.context["market_horizons"]["windows"];
        for (i, count, interval) in [(0, 15, 1), (1, 5, 60), (2, 15, 60)] {
            assert_eq!(w[i]["status"], "available");
            assert_eq!(w[i]["observation"]["return_count"], count);
            assert_eq!(w[i]["observation"]["sample_interval_seconds"], interval);
        }
        let expected = 1.01_f64.ln() * (16.0_f64 / 15.0).sqrt() * 10_000.0;
        assert!(
            (w[2]["observation"]["sample_volatility_bps"]
                .as_f64()
                .unwrap()
                - expected)
                .abs()
                < 1e-8
        );
        let u = &r.context["settlement_uncertainty"];
        assert_eq!(u["remaining_seconds"], 240.0);
        assert!(
            (u["remaining_midpoint_variability_scale_bps"]
                .as_f64()
                .unwrap()
                - 2.0 * expected)
                .abs()
                < 1e-8
        );
        assert!(
            (u["twap_distance_in_midpoint_scale_units"].as_f64().unwrap()
                - 1.0 / (2.0 * (16.0_f64 / 15.0).sqrt()))
            .abs()
                < 1e-8
        );
        assert!(u["absolute_reference_twap_gap_change_bps"].is_null());
        r.input_sha256 = agent::hash(&r.context).unwrap();
        let restored: EvaluationRequest =
            serde_json::from_slice(&serde_json::to_vec(&r).unwrap()).unwrap();
        assert_eq!(
            restored.input_sha256,
            agent::hash(&restored.context).unwrap()
        );
    }

    #[test]
    fn future_and_late_points_do_not_leak_and_gaps_remain_unknown() {
        let (mut r, mut state, sources) = fixture();
        add_market_context(&mut r, &state, &sources, None);
        let expected = r.context.clone();
        for (source_timestamp, available_at) in [
            (r.observed_at + Duration::seconds(1), r.observed_at),
            (
                r.observed_at - Duration::milliseconds(500),
                r.observed_at + Duration::seconds(1),
            ),
        ] {
            std::sync::Arc::make_mut(&mut state.directional_external.rtds).insert_fixture(
                RtdsPoint {
                    source_timestamp,
                    available_at,
                    price: Decimal::from(999),
                },
            );
        }
        add_market_context(&mut r, &state, &sources, None);
        assert_eq!(r.context, expected);
        let mut gap = RealtimeState::default();
        for p in state
            .directional_external
            .rtds()
            .points_as_of(r.observed_at)
        {
            if p.source_timestamp == r.observed_at - Duration::seconds(5) {
                continue;
            }
            std::sync::Arc::make_mut(&mut gap.directional_external.rtds).insert_fixture(p.clone());
        }
        add_market_context(&mut r, &gap, &sources, None);
        assert_eq!(
            r.context["market_horizons"]["windows"][0]["status"],
            "incomplete_history"
        );
        // A missing second does not change the shared closed-minute candle contract.
        assert_eq!(
            r.context["market_horizons"]["windows"][2]["status"],
            "available"
        );
        add_market_context(&mut r, &state, &[], None);
        assert_eq!(
            r.context["market_horizons"]["windows"][2]["status"],
            "not_selected"
        );
        assert!(
            r.context["settlement_uncertainty"]["remaining_midpoint_variability_scale_bps"]
                .is_null()
        );
        r.observed_at += Duration::seconds(30);
        add_market_context(&mut r, &state, &sources, None);
        assert_eq!(
            r.context["market_horizons"]["windows"][2]["status"],
            "missing_or_stale_reference"
        );
    }

    #[test]
    fn flat_paths_and_absent_twap_never_imply_settlement_certainty() {
        let (mut r, state, sources) = fixture();
        let mut flat = RealtimeState::default();
        for p in state
            .directional_external
            .rtds()
            .points_as_of(r.observed_at)
        {
            let mut p = p.clone();
            p.price = Decimal::from(100);
            std::sync::Arc::make_mut(&mut flat.directional_external.rtds).insert_fixture(p);
        }
        add_market_context(&mut r, &flat, &sources, None);
        assert_eq!(
            r.context["settlement_uncertainty"]["remaining_midpoint_variability_scale_bps"],
            0.0
        );
        assert!(
            r.context["settlement_uncertainty"]["twap_distance_in_midpoint_scale_units"].is_null()
        );
        assert!(
            r.context["market_horizons"]["windows"][2]["observation"]["path_efficiency"].is_null()
        );
        r.context["twap_context"]["opening"] = Value::Null;
        add_market_context(&mut r, &state, &sources, None);
        assert!(r.context["settlement_uncertainty"]["twap_log_distance_from_open_bps"].is_null());
        assert!(
            r.context["settlement_uncertainty"]["twap_distance_in_midpoint_scale_units"].is_null()
        );
    }

    #[test]
    fn reference_twap_gap_change_requires_existing_verified_comparison() {
        let (mut request, state, sources) = fixture();
        add_market_context(&mut request, &state, &sources, None);
        request.context["settlement_uncertainty"]["reference_to_twap_log_gap_bps"] = json!(10.0);
        request.input_sha256 = agent::hash(&request.context).unwrap();
        let previous = EvaluationResult {
            request: request.clone(),
            completed_at: request.observed_at,
            inference_seconds: 0.0,
            response_id: None,
            model: None,
            usage: UsageReport::Missing,
            failure_diagnostic: None,
            prediction: Ok(Prediction {
                direction: BtcOutcome::Up,
                probability_up: 0.6,
                confidence: 0.5,
                reason_codes: vec!["test".into()],
            }),
        };
        request.observed_at += Duration::seconds(1);
        agent::add_reassessment_context(&mut request, Some(&previous));
        add_market_context(&mut request, &state, &sources, Some(&previous));
        assert_eq!(
            request.context["settlement_uncertainty"]["absolute_reference_twap_gap_change_bps"],
            -10.0
        );
        request.process_id = Uuid::new_v4();
        agent::add_reassessment_context(&mut request, Some(&previous));
        add_market_context(&mut request, &state, &sources, Some(&previous));
        assert!(request.context["settlement_uncertainty"]
            ["absolute_reference_twap_gap_change_bps"]
            .is_null());
    }
}
