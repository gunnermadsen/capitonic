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
        if !self.pending.as_ref().is_some_and(|task| task.is_finished()) {
            return None;
        }
        self.pending.take()?.await.ok()
    }
    pub fn accept(&mut self) {
        self.accepted = true;
    }
    pub fn cancel(&mut self) {
        self.cancellation.cancel();
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
}
