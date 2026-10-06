use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TokenUsage {
    pub input: u64,
    pub output: u64,
    pub total: u64,
    pub cached_input: Option<u64>,
    pub reasoning: Option<u64>,
}
impl TokenUsage {
    pub fn values(&self) -> [(&'static str, Option<u64>); 5] {
        [
            ("input", Some(self.input)),
            ("output", Some(self.output)),
            ("total", Some(self.total)),
            ("cached_input", self.cached_input),
            ("reasoning", self.reasoning),
        ]
    }
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(tag = "status", content = "tokens", rename_all = "snake_case")]
pub enum UsageReport {
    Reported(TokenUsage),
    #[default]
    Missing,
    Invalid,
}
impl UsageReport {
    pub fn status(&self) -> &'static str {
        match self {
            Self::Reported(_) => "reported",
            Self::Missing => "missing",
            Self::Invalid => "invalid",
        }
    }
    pub fn from_event(event: &Value) -> Self {
        let Some(value) = event.pointer("/response/usage").filter(|v| !v.is_null()) else {
            return Self::Missing;
        };
        fn parse(value: &Value) -> Option<TokenUsage> {
            let number = |v: &Value| v.as_u64().filter(|n| *n <= 9_007_199_254_740_991);
            let input = number(&value["input_tokens"])?;
            let output = number(&value["output_tokens"])?;
            let total = number(&value["total_tokens"])?;
            if input.checked_add(output)? != total {
                return None;
            }
            let optional = |path: &str| -> Option<Option<u64>> {
                match value.pointer(path) {
                    None | Some(Value::Null) => Some(None),
                    Some(v) => Some(Some(number(v)?)),
                }
            };
            let cached_input = optional("/input_tokens_details/cached_tokens")?;
            let reasoning = optional("/output_tokens_details/reasoning_tokens")?;
            if cached_input.is_some_and(|n| n > input) || reasoning.is_some_and(|n| n > output) {
                return None;
            }
            Some(TokenUsage {
                input,
                output,
                total,
                cached_input,
                reasoning,
            })
        }
        parse(value).map(Self::Reported).unwrap_or(Self::Invalid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reported_usage_preserves_subset_and_unknown_semantics() {
        let event = serde_json::json!({"response":{"usage":{"input_tokens":100,"output_tokens":20,"total_tokens":120,"input_tokens_details":{"cached_tokens":60},"output_tokens_details":{"reasoning_tokens":10}}}});
        let UsageReport::Reported(usage) = UsageReport::from_event(&event) else {
            panic!("missing usage");
        };
        assert_eq!(usage.total, 120);
        assert_eq!(usage.cached_input, Some(60));
        assert_eq!(
            UsageReport::from_event(&serde_json::json!({})),
            UsageReport::Missing
        );
        assert_eq!(
            UsageReport::from_event(
                &serde_json::json!({"response":{"usage":{"input_tokens":100,"output_tokens":20,"total_tokens":121}}})
            ),
            UsageReport::Invalid
        );
        assert_eq!(
            UsageReport::from_event(
                &serde_json::json!({"response":{"usage":{"input_tokens":0,"output_tokens":0,"total_tokens":0}}})
            ),
            UsageReport::Reported(TokenUsage {
                input: 0,
                output: 0,
                total: 0,
                cached_input: None,
                reasoning: None
            })
        );
    }
}
