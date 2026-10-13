use super::*;

pub(crate) const FAILURE_REASONS: &[&str] = &[
    "request_preparation",
    "response_size_limit",
    "invalid_utf8",
    "event_json",
    "output_size_limit",
    "missing_delta",
    "provider_failed",
    "provider_incomplete",
    "provider_error",
    "missing_completed_event",
    "missing_output",
    "missing_content",
    "missing_text",
    "prediction_json",
    "probability_range",
    "confidence_range",
    "direction_probability_mismatch",
    "reason_codes",
    "prediction_validation",
    "response_id",
    "response_model",
    "authentication",
    "capacity",
    "transport",
    "timeout",
    "cancelled",
    "superseded",
];

/// Bounded failure evidence, carried by the existing evaluation metadata.
/// No raw prompts, provider messages, credentials, or response bodies are retained.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FailureDiagnostic {
    pub stage: String,
    pub reason: String,
    pub http_status: Option<u16>,
    pub provider_request_id: Option<String>,
    pub response_id: Option<String>,
    pub model: Option<String>,
    pub terminal_event: Option<String>,
    pub provider_error_code: Option<String>,
    pub incomplete_reason: Option<String>,
    pub received_bytes: usize,
    pub output_bytes: usize,
}

impl FailureDiagnostic {
    pub(super) fn mark(&mut self, stage: &'static str, reason: &'static str) {
        self.stage = stage.into();
        self.reason = reason.into();
    }

    pub(super) fn invalid(&mut self, stage: &'static str, reason: &'static str) -> ProviderFailure {
        self.mark(stage, reason);
        ProviderFailure::InvalidResponse
    }

    pub(super) fn capture(&mut self, event: &Value) {
        self.terminal_event = identifier(event["type"].as_str());
        if let Some(id) = identifier(event.pointer("/response/id").and_then(Value::as_str)) {
            self.response_id = Some(id);
        }
        if let Some(model) = identifier(event.pointer("/response/model").and_then(Value::as_str)) {
            self.model = Some(model);
        }
        self.incomplete_reason = identifier(
            event
                .pointer("/response/incomplete_details/reason")
                .and_then(Value::as_str),
        );
        self.provider_error_code = identifier(
            event
                .pointer("/response/error/code")
                .or_else(|| event.get("code"))
                .and_then(Value::as_str),
        );
    }
}

pub(super) fn identifier(value: Option<&str>) -> Option<String> {
    value
        .filter(|s| {
            !s.is_empty()
                && s.len() <= 256
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-.:".contains(&b))
        })
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_bounded_identifiers_are_retained() {
        assert_eq!(identifier(Some("resp_123")), Some("resp_123".into()));
        assert!(identifier(Some("Bearer secret")).is_none());
        assert!(identifier(Some(&"x".repeat(257))).is_none());
        let mut diagnostic = FailureDiagnostic::default();
        diagnostic.capture(&serde_json::json!({"type":"response.failed","response":{"id":"resp_1","error":{"code":"server_error","message":"sensitive body"}}}));
        assert_eq!(
            diagnostic.provider_error_code.as_deref(),
            Some("server_error")
        );
        assert!(!serde_json::to_string(&diagnostic)
            .unwrap()
            .contains("sensitive"));
    }
}
