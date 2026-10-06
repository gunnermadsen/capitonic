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
    ) -> Result<(Prediction, String, String), ProviderFailure> {
        let token = auth::access_token(&self.client)
            .await
            .map_err(|_| ProviderFailure::Authentication)?;
        let response = self.client.post("https://api.openai.com/v1/responses").bearer_auth(token)
            .json(&serde_json::json!({
                "model": profile().model, "instructions": profile().instructions,
                "store": false, "stream": true,
                "input": [{"role":"user", "content":serde_json::to_string(&request.context).map_err(|_| ProviderFailure::InvalidResponse)?}],
                "text": {"format": {"type":"json_schema", "name":"btc_prediction", "strict":true,
                    "schema": {"type":"object","additionalProperties":false,
                        "properties": {"direction":{"type":"string","enum":["up","down"]},
                            "probability_up":{"type":"number"}, "confidence":{"type":"number"},
                            "reason_codes":{"type":"array","items":{"type":"string"}}},
                        "required":["direction","probability_up","confidence","reason_codes"]}}}
            })).send().await.map_err(|_| ProviderFailure::Transport)?;
        match response.status().as_u16() {
            200 => {}
            401 | 403 => return Err(ProviderFailure::Authentication),
            429 | 503 => return Err(ProviderFailure::Capacity),
            _ => return Err(ProviderFailure::Transport),
        }
        let mut stream = response.bytes_stream();
        let mut buffer = Vec::new();
        while let Some(chunk) = stream.next().await {
            buffer.extend_from_slice(&chunk.map_err(|_| ProviderFailure::Transport)?);
            if buffer.len() > 1_048_576 {
                return Err(ProviderFailure::InvalidResponse);
            }
            while let Some(end) = buffer.iter().position(|b| *b == b'\n') {
                let line: Vec<u8> = buffer.drain(..=end).collect();
                let line = std::str::from_utf8(&line)
                    .map_err(|_| ProviderFailure::InvalidResponse)?
                    .trim();
                let Some(data) = line.strip_prefix("data:") else {
                    continue;
                };
                if data.trim() == "[DONE]" {
                    continue;
                }
                let event: Value = serde_json::from_str(data.trim())
                    .map_err(|_| ProviderFailure::InvalidResponse)?;
                match event["type"].as_str() {
                    Some("response.completed") => return parse_completed(&event),
                    Some("response.failed") => {
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
                        return Err(ProviderFailure::InvalidResponse)
                    }
                    _ => {}
                }
            }
        }
        Err(ProviderFailure::InvalidResponse)
    }
}
fn parse_completed(event: &Value) -> Result<(Prediction, String, String), ProviderFailure> {
    let response = &event["response"];
    let output = response["output"]
        .as_array()
        .ok_or(ProviderFailure::InvalidResponse)?;
    let mut text = String::new();
    for item in output {
        if item["type"] != "message" {
            continue;
        }
        for content in item["content"]
            .as_array()
            .ok_or(ProviderFailure::InvalidResponse)?
        {
            if content["type"] == "output_text" {
                text.push_str(
                    content["text"]
                        .as_str()
                        .ok_or(ProviderFailure::InvalidResponse)?,
                );
            }
        }
    }
    if text.len() > 8192 {
        return Err(ProviderFailure::InvalidResponse);
    }
    let prediction: Prediction =
        serde_json::from_str(&text).map_err(|_| ProviderFailure::InvalidResponse)?;
    prediction
        .validate()
        .map_err(|_| ProviderFailure::InvalidResponse)?;
    let id = response["id"]
        .as_str()
        .filter(|s| s.len() <= 256)
        .ok_or(ProviderFailure::InvalidResponse)?
        .into();
    let model = response["model"]
        .as_str()
        .filter(|s| s.len() <= 256)
        .ok_or(ProviderFailure::InvalidResponse)?
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
        let result = tokio::select! {
            _ = cancellation.cancelled() => Err(ProviderFailure::Cancelled),
            result = tokio::time::timeout(remaining, self.infer(&request)) => result.unwrap_or(Err(ProviderFailure::Timeout)),
        };
        let (prediction, response_id, model) = match result {
            Ok((p, id, model)) => (Ok(p), Some(id), Some(model)),
            Err(error) => (Err(error), None, None),
        };
        EvaluationResult {
            request,
            completed_at: Utc::now(),
            inference_seconds: started.elapsed().as_secs_f64(),
            response_id,
            model,
            prediction,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_completed_valid_output_becomes_prediction() {
        let event = serde_json::json!({"response":{"id":"resp_test","model":"test","output":[{"type":"message","content":[{"type":"output_text","text":"{\"direction\":\"up\",\"probability_up\":0.6,\"confidence\":0.2,\"reason_codes\":[\"momentum\"]}"}]}]}});
        assert!(parse_completed(&event).is_ok());
        assert!(parse_completed(&serde_json::json!({"response":{"output":[]}})).is_err());
    }
}
