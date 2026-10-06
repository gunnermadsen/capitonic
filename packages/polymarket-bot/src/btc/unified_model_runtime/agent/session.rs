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
    cancellation: CancellationToken,
}
impl AgentSession {
    pub fn observe_market(&mut self, market: &str) {
        if self.market != market {
            self.cancel();
            self.market = market.into();
            self.attempts = 0;
            self.accepted = false;
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
        self.accepted = true;
    }
    pub fn defer(&mut self, result: EvaluationResult) {
        self.completed = Some(result);
    }
    pub fn cancel(&mut self) {
        self.cancellation.cancel();
        self.completed = None;
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

    #[tokio::test]
    async fn deferred_result_is_bounded_and_cleared_on_rollover() {
        use super::super::{AgentSelection, ProviderFailure, BRIDGE_VERSION, PROFILE_KEY};
        use chrono::Utc;
        use uuid::Uuid;
        let mut session = AgentSession::default();
        session.observe_market("a");
        let request_id = Uuid::new_v4();
        let result = EvaluationResult {
            request: EvaluationRequest {
                version: BRIDGE_VERSION,
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
            prediction: Err(ProviderFailure::Transport),
        };
        session.defer(result.clone());
        assert!(!session.due(75));
        assert_eq!(session.poll().await.unwrap().request.request_id, request_id);
        assert!(session.poll().await.is_none());
        session.defer(result);
        session.observe_market("b");
        assert!(session.poll().await.is_none());
        assert!(session.due(45));
    }
}
