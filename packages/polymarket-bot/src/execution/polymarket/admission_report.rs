//! Records the existing admission sequence without performing or duplicating any check.
use serde_json::{json, Value};
#[derive(Default)]
pub(super) struct AdmissionReport {
    pub results: Vec<Value>,
}
impl AdmissionReport {
    pub fn begin(&mut self, name: &'static str, owner: &'static str) {
        self.finish_current();
        self.results
            .push(json!({"name":name,"version":1,"status":"evaluating","effective_owner":owner}));
    }
    pub fn not_applicable(
        &mut self,
        name: &'static str,
        owner: &'static str,
        reason: &'static str,
    ) {
        self.finish_current();
        self.results.push(json!({"name":name,"version":1,"status":"not_applicable","effective_owner":owner,"reason":reason}));
    }
    fn finish_current(&mut self) {
        if let Some(last) = self.results.last_mut() {
            if last["status"] == "evaluating" {
                last["status"] = json!("applied");
            }
        }
    }
    pub fn finish(mut self, error: Option<&anyhow::Error>) -> Value {
        if let Some(error) = error {
            let reason = error.to_string();
            let bounded = if reason.len() <= 96
                && reason.bytes().all(|c| c.is_ascii_lowercase() || c == b'_')
            {
                reason.as_str()
            } else {
                "admission_evidence_unavailable"
            };
            if let Some(last) = self.results.last_mut() {
                last["status"] = json!("rejected");
                last["reason"] = json!(bounded);
            }
        } else {
            self.finish_current();
        }
        json!(self.results)
    }
}

/// A completed policy evaluation rejected this action. It is not a transport failure.
#[derive(Debug)]
pub struct AdmissionRejection {
    pub reason: String,
    pub policy_results: Value,
}
impl std::fmt::Display for AdmissionRejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.reason)
    }
}
impl std::error::Error for AdmissionRejection {}

impl AdmissionRejection {
    pub fn evidence_unavailable(&self) -> bool {
        [
            "missing",
            "stale",
            "unavailable",
            "future",
            "noncausal",
            "freshness",
        ]
        .iter()
        .any(|part| self.reason.contains(part))
    }
}
