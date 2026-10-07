//! Compact comparison of existing causal request snapshots; no new history or execution policy.
use super::{EvaluationRequest, EvaluationResult, REASSESSMENT_CONTEXT_VERSION};
use crate::btc::types::BtcOutcome;
use rust_decimal::Decimal;
use serde_json::{json, Value};

pub fn add_reassessment_context(
    request: &mut EvaluationRequest,
    previous: Option<&EvaluationResult>,
) {
    request.context["version"] = json!(REASSESSMENT_CONTEXT_VERSION);
    request.context["previous_prediction_comparison"] = previous
        .and_then(|previous| comparison(request, previous))
        .unwrap_or(Value::Null);
}

fn comparison(current: &EvaluationRequest, previous: &EvaluationResult) -> Option<Value> {
    let prior = &previous.request;
    if prior.process_id != current.process_id
        || prior.run_id != current.run_id
        || prior.config_hash != current.config_hash
        || prior.selection != current.selection
        || prior.market_id != current.market_id
        || prior.window_start != current.window_start
        || prior.observed_at < current.window_start
        || prior.observed_at >= current.observed_at
        || previous.completed_at < prior.observed_at
        || previous.completed_at > current.observed_at
        || super::hash(&prior.context).ok().as_ref() != Some(&prior.input_sha256)
    {
        return None;
    }
    let prediction = previous.prediction.as_ref().ok()?;
    prediction.validate().ok()?;
    let before = &prior.context;
    let now = &current.context;
    let side = match prediction.direction {
        BtcOutcome::Up => "up_book",
        BtcOutcome::Down => "down_book",
    };
    let price_path = format!("/execution_snapshot/{side}/executable_ask_vwap");
    let reference_path = "/execution_snapshot/chainlink_price";
    Some(json!({
        "previous_direction": prediction.direction,
        "previous_probability_up": prediction.probability_up,
        "previous_observed_at": prior.observed_at,
        "current_observed_at": current.observed_at,
        "previous_observation_age_ms": (current.observed_at-prior.observed_at).num_milliseconds(),
        "meaning": "Changes between two observations in this same market, not a previous market. RTDS reference is a supporting midpoint indicator, not the settlement TWAP. Prices are USD; relative changes are basis points. Executable prices exclude fees. Null means unknown. The existing rolling 60-second history remains separate.",
        "twap_distance_from_open_bps": pair(twap_distance(before), twap_distance(now)),
        "rtds_reference_price": pair(positive(before, reference_path), positive(now, reference_path)),
        "rtds_reference_change_bps": relative_change(positive(before, reference_path), positive(now, reference_path)),
        "previous_side_executable_price": pair(positive(before, &price_path), positive(now, &price_path)),
    }))
}

fn positive(context: &Value, path: &str) -> Option<Decimal> {
    let value: Decimal = serde_json::from_value(context.pointer(path)?.clone()).ok()?;
    (value > Decimal::ZERO).then_some(value)
}

fn relative_change(before: Option<Decimal>, now: Option<Decimal>) -> Option<Decimal> {
    let before = before?;
    now?.checked_sub(before)?
        .checked_div(before)?
        .checked_mul(Decimal::from(10_000))
        .map(|value| value.normalize())
}

fn twap_distance(context: &Value) -> Option<Decimal> {
    relative_change(
        positive(context, "/twap_context/opening/price"),
        positive(context, "/twap_context/current/price"),
    )
}

fn pair(before: Option<Decimal>, now: Option<Decimal>) -> Value {
    json!({"previous":before,"current":now,"change":before.zip(now).and_then(|(a,b)| b.checked_sub(a)).map(|value| value.normalize())})
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::btc::unified_model_runtime::agent::*;
    use chrono::{Duration, TimeZone, Utc};
    use uuid::Uuid;

    fn fixture() -> (EvaluationRequest, EvaluationResult) {
        let start = Utc.timestamp_opt(1_790_000_000, 0).unwrap();
        let profile = profile_for_key(REASSESSMENT_PROFILE_KEY).unwrap();
        let context = json!({"version":VOLATILITY_CONTEXT_VERSION,
            "twap_context":{"opening":{"price":"100"},"current":{"price":"101"}},
            "execution_snapshot":{"chainlink_price":"100", "up_book":{"executable_ask_vwap":"0.55"},"down_book":{"executable_ask_vwap":"0.46"}},
            "rtds_last_60_seconds":[{"price":"100"}],"rtds_volatility":{"observation":null}});
        let request = EvaluationRequest {
            version: BRIDGE_VERSION.into(),
            request_id: Uuid::new_v4(),
            process_id: Uuid::new_v4(),
            run_id: Uuid::new_v4(),
            config_hash: "config".into(),
            selection: AgentSelection {
                profile_key: profile.key.into(),
                profile_sha256: hash(&profile).unwrap(),
            },
            market_id: "market".into(),
            window_start: start,
            snapshot_id: Uuid::new_v4(),
            observed_at: start + Duration::seconds(45),
            deadline: start + Duration::seconds(120),
            attempt: 45,
            input_sha256: hash(&context).unwrap(),
            context,
        };
        let previous = EvaluationResult {
            request: request.clone(),
            completed_at: start + Duration::seconds(52),
            inference_seconds: 7.0,
            response_id: None,
            model: None,
            usage: UsageReport::Missing,
            prediction: Ok(Prediction {
                direction: BtcOutcome::Up,
                probability_up: 0.65,
                confidence: 0.4,
                reason_codes: vec!["momentum".into()],
            }),
        };
        let mut current = request;
        current.observed_at = start + Duration::seconds(75);
        current.context["twap_context"]["current"]["price"] = json!("99");
        current.context["execution_snapshot"]["chainlink_price"] = json!("102");
        current.context["execution_snapshot"]["up_book"]["executable_ask_vwap"] = json!("0.60");
        (current, previous)
    }

    #[test]
    fn comparison_preserves_history_and_uses_previous_side() {
        let (mut current, mut prior) = fixture();
        let original = current.context.clone();
        add_reassessment_context(&mut current, Some(&prior));
        let c = &current.context["previous_prediction_comparison"];
        assert_eq!(c["previous_observation_age_ms"], 30000);
        assert_eq!(c["previous_probability_up"], 0.65);
        assert_eq!(
            c["twap_distance_from_open_bps"]["previous"],
            json!(Decimal::from(100))
        );
        assert_eq!(
            c["twap_distance_from_open_bps"]["current"],
            json!(Decimal::from(-100))
        );
        assert_eq!(
            c["twap_distance_from_open_bps"]["change"],
            json!(Decimal::from(-200))
        );
        assert_eq!(c["rtds_reference_change_bps"], json!(Decimal::from(200)));
        assert_eq!(
            c["previous_side_executable_price"]["change"],
            json!(Decimal::new(5, 2))
        );
        current
            .context
            .as_object_mut()
            .unwrap()
            .remove("previous_prediction_comparison");
        current.context["version"] = original["version"].clone();
        assert_eq!(current.context, original);
        prior.prediction.as_mut().unwrap().direction = BtcOutcome::Down;
        prior.prediction.as_mut().unwrap().probability_up = 0.35;
        add_reassessment_context(&mut current, Some(&prior));
        assert_eq!(
            current.context["previous_prediction_comparison"]["previous_side_executable_price"]
                ["change"],
            json!(Decimal::ZERO)
        );
    }

    #[test]
    fn absent_or_out_of_scope_forecast_is_unknown_not_a_gate() {
        let (current, prior) = fixture();
        let mut first = current.clone();
        add_reassessment_context(&mut first, None);
        assert!(first.context["previous_prediction_comparison"].is_null());
        for case in 0..8 {
            let mut invalid = prior.clone();
            match case {
                0 => invalid.request.process_id = Uuid::new_v4(),
                1 => invalid.request.run_id = Uuid::new_v4(),
                2 => invalid.request.market_id = "other".into(),
                3 => invalid.request.config_hash = "other".into(),
                4 => invalid.completed_at = current.observed_at + Duration::seconds(1),
                5 => invalid.request.observed_at = current.observed_at,
                6 => invalid.request.window_start -= Duration::seconds(300),
                _ => invalid.request.input_sha256 = "invalid".into(),
            }
            assert!(comparison(&current, &invalid).is_none());
        }
    }

    #[test]
    fn missing_values_do_not_substitute_reference_for_twap() {
        let (mut current, prior) = fixture();
        current.context["twap_context"] = Value::Null;
        current.context["execution_snapshot"]["up_book"]["executable_ask_vwap"] = Value::Null;
        let c = comparison(&current, &prior).unwrap();
        assert!(c["twap_distance_from_open_bps"]["current"].is_null());
        assert!(c["twap_distance_from_open_bps"]["change"].is_null());
        assert!(c["previous_side_executable_price"]["change"].is_null());
        assert_eq!(c["rtds_reference_change_bps"], json!(Decimal::from(200)));
    }
}
