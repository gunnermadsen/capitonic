use super::*;
use futures_util::StreamExt;
use std::time::Instant;

pub struct OpenAiProvider {
    client: reqwest::Client,
}
impl OpenAiProvider {
    pub fn new() -> Result<Self> {
        Ok(Self {
            client: reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(5))
                .timeout(std::time::Duration::from_secs(profile().timeout_seconds))
                .build()?,
        })
    }
    async fn infer(
        &self,
        request: &EvaluationRequest,
        usage: &mut UsageReport,
        diagnostic: &mut FailureDiagnostic,
    ) -> Result<(Prediction, String, String), ProviderFailure> {
        diagnostic.mark("request", "request_preparation");
        let selected = profile_for_key(&request.selection.profile_key)
            .map_err(|_| ProviderFailure::InvalidResponse)?;
        let token = auth::access_token(&self.client)
            .await
            .map_err(|_| ProviderFailure::Authentication)?;
        let response = self.client.post("https://api.openai.com/v1/responses").bearer_auth(token)
            .json(&serde_json::json!({
                "model": selected.model, "instructions": selected.instructions,
                "store": false, "stream": true,
                "input": [{"role":"user", "content":serde_json::to_string(&request.context).map_err(|_| ProviderFailure::InvalidResponse)?}],
                "text": {"format": {"type":"json_schema", "name":"btc_prediction", "strict":true,
                    "schema": {"type":"object","additionalProperties":false,
                        "properties": {"direction":{"type":"string","enum":["up","down"]},
                            "probability_up":{"type":"number"}, "confidence":{"type":"number"},
                            "reason_codes":{"type":"array","items":{"type":"string"}}},
                        "required":["direction","probability_up","confidence","reason_codes"]}}}
            })).send().await.map_err(|_| ProviderFailure::Transport)?;
        diagnostic.http_status = Some(response.status().as_u16());
        diagnostic.provider_request_id = super::diagnostics::identifier(
            response
                .headers()
                .get("x-request-id")
                .and_then(|v| v.to_str().ok()),
        );
        diagnostic.mark("http", "http_status");
        match response.status().as_u16() {
            200 => {}
            401 | 403 => return Err(ProviderFailure::Authentication),
            429 | 503 => return Err(ProviderFailure::Capacity),
            _ => return Err(ProviderFailure::Transport),
        }
        let mut stream = response.bytes_stream();
        let mut buffer = Vec::new();
        let mut streamed_text = String::new();
        let mut received_bytes = 0usize;
        diagnostic.mark("stream", "stream_read");
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| ProviderFailure::Transport)?;
            received_bytes += chunk.len();
            diagnostic.received_bytes = received_bytes;
            if received_bytes > 1_048_576 {
                return Err(diagnostic.invalid("stream", "response_size_limit"));
            }
            buffer.extend_from_slice(&chunk);
            while let Some(end) = buffer.iter().position(|b| *b == b'\n') {
                let line: Vec<u8> = buffer.drain(..=end).collect();
                let line = std::str::from_utf8(&line)
                    .map_err(|_| diagnostic.invalid("stream", "invalid_utf8"))?
                    .trim();
                let Some(data) = line.strip_prefix("data:") else {
                    continue;
                };
                if data.trim() == "[DONE]" {
                    continue;
                }
                let event: Value = serde_json::from_str(data.trim())
                    .map_err(|_| diagnostic.invalid("stream", "event_json"))?;
                match event["type"].as_str() {
                    Some("response.created") => {
                        diagnostic.response_id = super::diagnostics::identifier(
                            event.pointer("/response/id").and_then(Value::as_str),
                        );
                        diagnostic.model = super::diagnostics::identifier(
                            event.pointer("/response/model").and_then(Value::as_str),
                        );
                    }
                    Some("response.output_text.delta") => {
                        append_text_delta(&mut streamed_text, &event).inspect_err(|_| {
                            diagnostic.mark(
                                "stream",
                                if event["delta"].is_string() {
                                    "output_size_limit"
                                } else {
                                    "missing_delta"
                                },
                            );
                        })?;
                        diagnostic.output_bytes = streamed_text.len();
                    }
                    Some("response.completed") => {
                        *usage = UsageReport::from_event(&event);
                        diagnostic.capture(&event);
                        return parse_completed_diagnostic(&event, &streamed_text, diagnostic);
                    }
                    Some("response.failed") => {
                        diagnostic.capture(&event);
                        diagnostic.mark("provider", "provider_failed");
                        *usage = UsageReport::from_event(&event);
                        let code = event
                            .pointer("/response/error/code")
                            .and_then(Value::as_str)
                            .unwrap_or("");
                        return Err(if code.starts_with("subscription_sharing_usage") {
                            ProviderFailure::Capacity
                        } else {
                            ProviderFailure::InvalidResponse
                        });
                    }
                    Some("response.incomplete" | "error") => {
                        diagnostic.capture(&event);
                        diagnostic.mark(
                            "provider",
                            if event["type"] == "response.incomplete" {
                                "provider_incomplete"
                            } else {
                                "provider_error"
                            },
                        );
                        *usage = UsageReport::from_event(&event);
                        return Err(ProviderFailure::InvalidResponse);
                    }
                    _ => {}
                }
            }
        }
        Err(diagnostic.invalid("stream", "missing_completed_event"))
    }
}
fn append_text_delta(text: &mut String, event: &Value) -> Result<(), ProviderFailure> {
    let delta = event["delta"]
        .as_str()
        .ok_or(ProviderFailure::InvalidResponse)?;
    if text.len() + delta.len() > 8192 {
        return Err(ProviderFailure::InvalidResponse);
    }
    text.push_str(delta);
    Ok(())
}
#[cfg(test)]
fn parse_completed(
    event: &Value,
    streamed_text: &str,
) -> Result<(Prediction, String, String), ProviderFailure> {
    parse_completed_diagnostic(event, streamed_text, &mut FailureDiagnostic::default())
}
fn parse_completed_diagnostic(
    event: &Value,
    streamed_text: &str,
    diagnostic: &mut FailureDiagnostic,
) -> Result<(Prediction, String, String), ProviderFailure> {
    let response = &event["response"];
    let output = response["output"]
        .as_array()
        .ok_or_else(|| diagnostic.invalid("output", "missing_output"))?;
    let mut text = String::new();
    for item in output {
        if item["type"] != "message" {
            continue;
        }
        for content in item["content"]
            .as_array()
            .ok_or_else(|| diagnostic.invalid("output", "missing_content"))?
        {
            if content["type"] == "output_text" {
                text.push_str(
                    content["text"]
                        .as_str()
                        .ok_or_else(|| diagnostic.invalid("output", "missing_text"))?,
                );
            }
        }
    }
    // Subscription streams may omit messages from the completed response's output array.
    // Deltas become eligible only after the provider sends response.completed.
    if text.is_empty() {
        text.push_str(streamed_text);
    }
    diagnostic.output_bytes = text.len();
    if text.len() > 8192 {
        return Err(diagnostic.invalid("output", "output_size_limit"));
    }
    let prediction: Prediction =
        serde_json::from_str(&text).map_err(|_| diagnostic.invalid("output", "prediction_json"))?;
    prediction.validate().map_err(|error| {
        diagnostic.invalid(
            "validation",
            match error.to_string().as_str() {
                "invalid probability" => "probability_range",
                "invalid confidence" => "confidence_range",
                "direction contradicts probability" => "direction_probability_mismatch",
                "invalid reason codes" => "reason_codes",
                _ => "prediction_validation",
            },
        )
    })?;
    let id = response["id"]
        .as_str()
        .filter(|s| s.len() <= 256)
        .ok_or_else(|| diagnostic.invalid("metadata", "response_id"))?
        .into();
    let model = response["model"]
        .as_str()
        .filter(|s| s.len() <= 256)
        .ok_or_else(|| diagnostic.invalid("metadata", "response_model"))?
        .into();
    Ok((prediction, id, model))
}
#[async_trait]
impl AsyncDecisionProvider for OpenAiProvider {
    async fn evaluate(
        &self,
        request: EvaluationRequest,
        cancellation: CancellationToken,
    ) -> EvaluationResult {
        let started = Instant::now();
        let remaining = (request.deadline - Utc::now()).to_std().unwrap_or_default();
        let mut usage = UsageReport::Missing;
        let mut diagnostic = FailureDiagnostic::default();
        let result = tokio::select! {
            _ = cancellation.cancelled() => Err(ProviderFailure::Cancelled),
            result = tokio::time::timeout(remaining, self.infer(&request, &mut usage, &mut diagnostic)) => result.unwrap_or(Err(ProviderFailure::Timeout)),
        };
        let failure_diagnostic = result.as_ref().err().map(|failure| {
            if !matches!(failure, ProviderFailure::InvalidResponse) {
                diagnostic.mark("request", failure.code());
            }
            super::super::telemetry::event(request.process_id, "agent_response_failures", &diagnostic.reason);
            tracing::warn!(event="umr_agent_response_failed", process_id=%request.process_id,
                run_id=%request.run_id, market_id=%request.market_id, request_id=%request.request_id,
                failure=failure.code(), stage=%diagnostic.stage, reason=%diagnostic.reason,
                provider_request_id=?diagnostic.provider_request_id, response_id=?diagnostic.response_id,
                model=?diagnostic.model, http_status=?diagnostic.http_status,
                terminal_event=?diagnostic.terminal_event, provider_error_code=?diagnostic.provider_error_code,
                incomplete_reason=?diagnostic.incomplete_reason,
                received_bytes=diagnostic.received_bytes, output_bytes=diagnostic.output_bytes,
                duration_seconds=started.elapsed().as_secs_f64(), usage_status=usage.status(),
                "Agent response failed; bounded diagnostic evidence");
            diagnostic.clone()
        });
        super::super::telemetry::agent_usage_report(request.process_id, request.request_id, &usage);
        let (prediction, response_id, model) = match result {
            Ok((p, id, model)) => (Ok(p), Some(id), Some(model)),
            Err(error) => (
                Err(error),
                diagnostic.response_id.clone(),
                diagnostic.model.clone(),
            ),
        };
        EvaluationResult {
            failure_diagnostic,
            request,
            completed_at: Utc::now(),
            inference_seconds: started.elapsed().as_secs_f64(),
            response_id,
            model,
            usage,
            prediction,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn diagnostics_distinguish_parsing_validation_and_metadata_without_changing_acceptance() {
        let valid = serde_json::json!({"direction":"up","probability_up":0.6,"confidence":0.2,"reason_codes":["momentum"]});
        for (text, expected) in [
            ("not json".into(), "prediction_json"),
            (serde_json::json!({"direction":"up","probability_up":1.5,"confidence":0.2,"reason_codes":["momentum"]}).to_string(), "probability_range"),
        ] {
            let mut diagnostic = FailureDiagnostic::default();
            let event = serde_json::json!({"response":{"id":"resp_test","model":"test","output":[]}});
            assert!(parse_completed_diagnostic(&event, &text, &mut diagnostic).is_err());
            assert_eq!(diagnostic.reason, expected);
        }
        for (field, value, expected) in [
            (
                "probability_up",
                serde_json::json!(0.3),
                "direction_probability_mismatch",
            ),
            ("confidence", serde_json::json!(-1), "confidence_range"),
            (
                "reason_codes",
                serde_json::json!(["Invalid reason"]),
                "reason_codes",
            ),
        ] {
            let mut prediction = valid.clone();
            prediction[field] = value;
            let mut diagnostic = FailureDiagnostic::default();
            let event =
                serde_json::json!({"response":{"id":"resp_test","model":"test","output":[]}});
            assert!(
                parse_completed_diagnostic(&event, &prediction.to_string(), &mut diagnostic)
                    .is_err()
            );
            assert_eq!(diagnostic.reason, expected);
        }
        let mut diagnostic = FailureDiagnostic::default();
        assert!(parse_completed_diagnostic(
            &serde_json::json!({"response":{"output":[]}}),
            &valid.to_string(),
            &mut diagnostic
        )
        .is_err());
        assert_eq!(diagnostic.reason, "response_id");
    }
    #[test]
    fn only_completed_valid_output_becomes_prediction() {
        let event = serde_json::json!({"response":{"id":"resp_test","model":"test","output":[{"type":"message","content":[{"type":"output_text","text":"{\"direction\":\"up\",\"probability_up\":0.6,\"confidence\":0.2,\"reason_codes\":[\"momentum\"]}"}]}]}});
        assert!(parse_completed(&event, "").is_ok());
        assert!(parse_completed(&serde_json::json!({"response":{"output":[]}}), "").is_err());
    }
    #[test]
    fn completed_subscription_response_accepts_bounded_streamed_prediction() {
        let mut text = String::new();
        for delta in [
            "{\"direction\":\"up\",\"probability_up\":0.6,",
            "\"confidence\":0.2,\"reason_codes\":[\"momentum\"]}",
        ] {
            append_text_delta(&mut text, &serde_json::json!({"delta":delta})).unwrap();
        }
        let event =
            serde_json::json!({"response":{"id":"resp_test","model":"gpt-6-astra","output":[]}});
        assert!(parse_completed(&event, &text).is_ok());
        assert!(parse_completed(&event, "{\"direction\":\"up\"}").is_err());
    }
    #[test]
    fn streamed_prediction_rejects_missing_or_oversized_delta() {
        let mut text = "x".repeat(8192);
        assert!(append_text_delta(&mut text, &serde_json::json!({"delta":"x"})).is_err());
        assert!(append_text_delta(&mut String::new(), &serde_json::json!({})).is_err());
    }
}
