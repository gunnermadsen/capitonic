use super::{profile, AsyncDecisionProvider, EvaluationRequest, EvaluationResult, OpenAiProvider};
use anyhow::Result;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

/// One bounded request per process; rollover and shutdown cancel outstanding work.
#[derive(Default)]
pub struct AgentSession {
    market: String,
    attempts: usize,
    accepted: bool,
    pending: Option<JoinHandle<EvaluationResult>>,
    completed: Option<EvaluationResult>,
    latest: Option<EvaluationResult>,
    pub restored: bool,
    cancellation: CancellationToken,
    usage_process: Option<uuid::Uuid>,
    created_at: Option<chrono::DateTime<chrono::Utc>>,
}
impl AgentSession {
    pub fn observe_market(&mut self, market: &str) {
        self.created_at.get_or_insert_with(chrono::Utc::now);
        if self.market != market {
            self.cancel();
            self.market = market.into();
            self.attempts = 0;
            self.accepted = false;
            self.restored = false;
            self.cancellation = CancellationToken::new();
        }
    }
    pub fn due(&self, seconds: i64) -> bool {
        !self.accepted
            && self.pending.is_none()
            && self.completed.is_none()
            && profile()
                .attempts_seconds
                .get(self.attempts)
                .is_some_and(|at| seconds >= *at)
            && seconds < profile().decision_ceiling_seconds
    }
    pub fn start(&mut self, request: EvaluationRequest) -> Result<()> {
        let provider = OpenAiProvider::new()?;
        super::super::telemetry::agent_usage_start(
            &request,
            self.created_at.is_some_and(|at| at > request.window_start),
        );
        self.usage_process = Some(request.process_id);
        let cancellation = self.cancellation.child_token();
        self.pending = Some(tokio::spawn(async move {
            provider.evaluate(request, cancellation).await
        }));
        self.attempts += 1;
        Ok(())
    }
    pub async fn poll(&mut self) -> Option<EvaluationResult> {
        if self.completed.is_some() {
            return self.completed.take();
        }
        if !self.pending.as_ref().is_some_and(|task| task.is_finished()) {
            return None;
        }
        self.pending.take()?.await.ok()
    }
    pub fn accept(&mut self) {
        self.cancel();
        self.accepted = true;
    }
    pub fn finished(&self) -> bool {
        self.accepted
    }
    pub fn pending(&self) -> bool {
        self.pending.is_some()
    }
    pub fn latest(&self) -> Option<&EvaluationResult> {
        self.latest.as_ref()
    }
    pub fn retain(&mut self, result: EvaluationResult) {
        self.attempts = self.attempts.max(
            profile()
                .attempts_seconds
                .iter()
                .filter(|second| **second <= result.request.attempt as i64)
                .count(),
        );
        self.latest = Some(result);
    }
    pub fn defer(&mut self, result: EvaluationResult) {
        self.completed = Some(result);
    }
    pub fn cancel(&mut self) {
        if let Some(id) = self.usage_process.take() {
            super::super::telemetry::agent_usage_finish(id);
        }
        self.cancellation.cancel();
        self.completed = None;
        self.latest = None;
        if let Some(task) = self.pending.take() {
            task.abort();
        }
    }
}
impl Drop for AgentSession {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn schedule_and_rollover_are_process_scoped() {
        let mut session = AgentSession::default();
        session.observe_market("a");
        assert!(!session.due(44));
        assert!(session.due(45));
        assert!(!session.due(120));
        session.accept();
        assert!(!session.due(75));
        session.observe_market("b");
        assert!(session.due(45));
    }

    fn forecast() -> EvaluationResult {
        use super::super::{AgentSelection, Prediction, BRIDGE_VERSION, PROFILE_KEY};
        use chrono::Utc;
        use uuid::Uuid;
        let request_id = Uuid::new_v4();
        EvaluationResult {
            failure_diagnostic: None,
            request: EvaluationRequest {
                version: BRIDGE_VERSION.into(),
                request_id,
                process_id: Uuid::new_v4(),
                run_id: Uuid::new_v4(),
                config_hash: "a".repeat(64),
                selection: AgentSelection {
                    profile_key: PROFILE_KEY.into(),
                    profile_sha256: super::super::hash(&profile()).unwrap(),
                },
                market_id: "a".into(),
                window_start: Utc::now(),
                snapshot_id: Uuid::new_v4(),
                observed_at: Utc::now(),
                deadline: Utc::now(),
                attempt: 45,
                input_sha256: "b".repeat(64),
                context: serde_json::json!({}),
            },
            completed_at: Utc::now(),
            inference_seconds: 1.0,
            response_id: None,
            model: None,
            usage: super::super::UsageReport::Missing,
            prediction: Ok(Prediction {
                direction: crate::btc::types::BtcOutcome::Up,
                probability_up: 0.6,
                confidence: 0.4,
                reason_codes: vec!["momentum".into()],
            }),
        }
    }

    #[test]
    fn saved_evaluations_without_diagnostics_remain_readable() {
        let old = serde_json::to_value(forecast()).unwrap();
        assert!(old.get("failure_diagnostic").is_none());
        let restored: EvaluationResult = serde_json::from_value(old).unwrap();
        assert!(restored.failure_diagnostic.is_none());
        let mut failed = forecast();
        failed.failure_diagnostic = Some(super::super::FailureDiagnostic {
            stage: "validation".into(),
            reason: "reason_codes".into(),
            ..Default::default()
        });
        let restored: EvaluationResult =
            serde_json::from_value(serde_json::to_value(failed).unwrap()).unwrap();
        assert_eq!(restored.failure_diagnostic.unwrap().reason, "reason_codes");
    }

    #[tokio::test]
    async fn deferred_result_is_bounded_and_cleared_on_rollover() {
        let mut session = AgentSession::default();
        session.observe_market("a");
        let result = forecast();
        let request_id = result.request.request_id;
        session.defer(result.clone());
        assert!(!session.due(75));
        assert_eq!(session.poll().await.unwrap().request.request_id, request_id);
        assert!(session.poll().await.is_none());
        session.defer(result);
        session.observe_market("b");
        assert!(session.poll().await.is_none());
        assert!(session.due(45));
    }

    #[tokio::test]
    async fn retained_forecast_survives_non_fills_and_refreshes_until_fill() {
        let mut session = AgentSession::default();
        session.observe_market("a");
        let first = forecast();
        session.retain(first.clone());
        assert!(!session.due(74));
        assert!(session.due(75));
        assert!(!session.finished());
        assert_eq!(
            session.latest().unwrap().request.request_id,
            first.request.request_id
        );
        let mut updated = forecast();
        updated.request.attempt = 75;
        updated.prediction.as_mut().unwrap().direction = crate::btc::types::BtcOutcome::Down;
        updated.prediction.as_mut().unwrap().probability_up = 0.3;
        session.retain(updated.clone());
        assert!(!session.due(104));
        assert!(session.due(105));
        assert_eq!(
            session
                .latest()
                .unwrap()
                .prediction
                .as_ref()
                .unwrap()
                .probability_up,
            0.3
        );
        // A fill cancels even an in-flight refresh and discards its late answer.
        session.pending = Some(tokio::spawn(async move {
            std::future::pending::<()>().await;
            updated
        }));
        assert!(!session.due(105));
        session.accept();
        assert!(session.finished());
        assert!(session.latest().is_none());
        assert!(session.poll().await.is_none());
        assert!(!session.due(105));
        session.observe_market("b");
        assert!(!session.finished());
        assert!(session.due(45));
    }

    #[test]
    fn persisted_forecast_restores_remaining_schedule_without_token_replay() {
        let original = forecast();
        let mut wire = serde_json::to_value(&original).unwrap();
        // Earlier deployed evidence predates token reporting.
        wire.as_object_mut().unwrap().remove("usage");
        let restored: EvaluationResult = serde_json::from_value(wire).unwrap();
        let mut session = AgentSession::default();
        session.observe_market("a");
        session.retain(restored);
        assert!(!session.due(60));
        assert!(session.due(75));
        assert!(!session.due(120));
        assert!(session.usage_process.is_none());
        assert_eq!(
            session.latest().unwrap().request.request_id,
            original.request.request_id
        );
    }
}
