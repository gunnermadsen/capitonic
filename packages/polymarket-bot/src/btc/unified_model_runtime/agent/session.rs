use super::{profile, AsyncDecisionProvider, EvaluationRequest, EvaluationResult, OpenAiProvider};
use anyhow::Result;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

/// One bounded request per process; rollover and shutdown cancel outstanding work.
#[derive(Default)]
pub struct AgentSession {
    market: String,
    pub calibration: Option<serde_json::Value>,
    attempts: usize,
    accepted: bool,
    entry_reassessment: Option<uuid::Uuid>,
    entry_reassessment_started: Option<uuid::Uuid>,
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
    pub fn evaluation_slot(&self) -> usize {
        self.attempts + 1
    }

    /// Latch only the affected forecast; a bounce above the cap cannot release it.
    pub fn hold_entry(
        &mut self,
        result: &EvaluationResult,
        up_ask: Option<rust_decimal::Decimal>,
        down_ask: Option<rust_decimal::Decimal>,
        cap: rust_decimal::Decimal,
        price_change: Option<rust_decimal::Decimal>,
    ) -> bool {
        if !super::uses_entry_reassessment(&result.request.selection.profile_key) {
            return false;
        }
        let current =
            result
                .prediction
                .as_ref()
                .ok()
                .and_then(|prediction| match prediction.direction {
                    crate::btc::types::BtcOutcome::Up => up_ask,
                    crate::btc::types::BtcOutcome::Down => down_ask,
                });
        let observed = super::reassessment::selected_ask(result);
        let became_affordable = observed.is_some_and(|price| price > cap)
            && current.is_some_and(|price| price > rust_decimal::Decimal::ZERO && price <= cap);
        let changed = result.request.selection.profile_key == super::FRESH_EVIDENCE_PROFILE_KEY
            && observed
                .zip(current)
                .zip(price_change)
                .is_some_and(|((before, now), threshold)| {
                    now > rust_decimal::Decimal::ZERO
                        && threshold > rust_decimal::Decimal::ZERO
                        && (now - before).abs() >= threshold
                });
        if became_affordable || changed {
            self.entry_reassessment = Some(result.request.request_id);
        }
        self.entry_reassessment == Some(result.request.request_id)
    }

    pub fn entry_reassessment_due(&self, seconds: i64) -> bool {
        self.entry_reassessment.is_some()
            && self.entry_reassessment_started != self.entry_reassessment
            && !self.accepted
            && self.pending.is_none()
            && self.completed.is_none()
            && self.attempts < profile().attempts_seconds.len()
            && seconds >= profile().attempts_seconds[0]
            && seconds < profile().decision_ceiling_seconds
    }

    /// Request ordinals include failed and interrupted calls in the existing ledger.
    pub fn restore_request(&mut self, request: &EvaluationRequest) {
        if super::uses_calibration(&request.selection.profile_key) {
            self.calibration = request.context.get("historical_calibration").cloned();
        }
        if super::uses_entry_reassessment(&request.selection.profile_key) {
            if let Some(slot) = request.context["evaluation_slot"].as_u64() {
                self.attempts = self
                    .attempts
                    .max(slot.min(profile().attempts_seconds.len() as u64) as usize);
            }
            if matches!(
                request.context["entry_reassessment"]["trigger"].as_str(),
                Some("forecast_became_affordable" | "forecast_evidence_changed")
            ) {
                self.entry_reassessment_started = request.context["entry_reassessment"]
                    ["previous_request_id"]
                    .as_str()
                    .and_then(|id| id.parse().ok());
            }
        }
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
        self.entry_reassessment_started = self.entry_reassessment;
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
        if self
            .entry_reassessment
            .is_some_and(|id| id != result.request.request_id)
        {
            self.entry_reassessment = None;
            self.entry_reassessment_started = None;
        }
        if super::uses_entry_reassessment(&result.request.selection.profile_key) {
            if let Some(slot) = result.request.context["evaluation_slot"].as_u64() {
                self.attempts = self
                    .attempts
                    .max(slot.min(profile().attempts_seconds.len() as u64) as usize);
            }
        } else {
            self.attempts = self.attempts.max(
                profile()
                    .attempts_seconds
                    .iter()
                    .filter(|second| **second <= result.request.attempt as i64)
                    .count(),
            );
        }
        if super::uses_calibration(&result.request.selection.profile_key) {
            self.calibration = result
                .request
                .context
                .get("historical_calibration")
                .cloned();
        }
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
        self.calibration = None;
        self.entry_reassessment = None;
        self.entry_reassessment_started = None;
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
    fn entry_forecast(price: &str, slot: usize) -> EvaluationResult {
        let mut result = forecast();
        let selected =
            super::super::profile_for_key(super::super::ENTRY_REASSESSMENT_PROFILE_KEY).unwrap();
        result.request.selection.profile_key = selected.key.into();
        result.request.selection.profile_sha256 = super::super::hash(&selected).unwrap();
        result.request.context = serde_json::json!({"evaluation_slot":slot,
            "execution_snapshot":{"up_book":{"executable_ask_vwap":price},
                                  "down_book":{"executable_ask_vwap":"0.40"}}});
        result.request.input_sha256 = super::super::hash(&result.request.context).unwrap();
        result
    }

    #[test]
    fn affordability_holds_only_the_affected_side_and_profile() {
        use rust_decimal_macros::dec;
        let mut session = AgentSession::default();
        session.observe_market("a");
        let above = entry_forecast("0.70", 1);
        session.retain(above.clone());
        assert!(!session.hold_entry(&above, Some(dec!(0.61)), Some(dec!(0.30)), dec!(0.60), None));
        assert!(!session.hold_entry(&above, None, Some(dec!(0.30)), dec!(0.60), None));
        assert!(session.hold_entry(&above, Some(dec!(0.60)), Some(dec!(0.30)), dec!(0.60), None));
        assert!(session.entry_reassessment_due(60));
        // A price bounce cannot release the invalidated forecast.
        assert!(session.hold_entry(&above, Some(dec!(0.65)), None, dec!(0.60), None));
        let mut independent = AgentSession::default();
        independent.observe_market("a");
        independent.retain(above.clone());
        assert!(!independent.entry_reassessment_due(60));
        let mut down = entry_forecast("0.70", 1);
        down.prediction.as_mut().unwrap().direction = crate::btc::types::BtcOutcome::Down;
        down.prediction.as_mut().unwrap().probability_up = 0.3;
        independent.retain(down.clone());
        assert!(!independent.hold_entry(
            &down,
            Some(dec!(0.55)),
            Some(dec!(0.35)),
            dec!(0.60),
            None
        ));
        down.request.context["execution_snapshot"]["down_book"]["executable_ask_vwap"] =
            serde_json::json!("0.80");
        assert!(independent.hold_entry(&down, None, Some(dec!(0.59)), dec!(0.60), None));
        let affordable = entry_forecast("0.60", 1);
        session.retain(affordable.clone());
        assert!(!session.hold_entry(&affordable, Some(dec!(0.55)), None, dec!(0.60), None));
        let legacy = forecast();
        assert!(!session.hold_entry(&legacy, Some(dec!(0.55)), None, dec!(0.60), None));
    }

    #[tokio::test]
    async fn one_refresh_uses_next_slot_and_failure_does_not_release_old_forecast() {
        use rust_decimal_macros::dec;
        let mut session = AgentSession::default();
        session.observe_market("a");
        let old = entry_forecast("0.70", 1);
        session.retain(old.clone());
        assert!(session.hold_entry(&old, Some(dec!(0.59)), None, dec!(0.60), None));
        assert!(session.entry_reassessment_due(60));
        let mut refreshed = entry_forecast("0.59", 2);
        refreshed.request.attempt = 60;
        super::super::add_entry_reassessment_context(
            &mut refreshed.request,
            Some(&old),
            dec!(0.60),
            2,
            true,
        );
        // Model the pending provider without network access.
        session.attempts += 1;
        session.entry_reassessment_started = session.entry_reassessment;
        session.pending = Some(tokio::spawn(std::future::pending()));
        assert!(!session.entry_reassessment_due(60));
        assert!(!session.due(75));
        session.pending.take().unwrap().abort();
        // A failed result consumes its slot but is not retained as a valid forecast.
        assert!(session.hold_entry(&old, Some(dec!(0.58)), None, dec!(0.60), None));
        assert!(!session.entry_reassessment_due(70));
        assert!(!session.due(104));
        assert!(session.due(105));
        // A valid refreshed prediction clears the affected hold and resumes retries.
        session.retain(refreshed.clone());
        assert!(!session.hold_entry(&refreshed, Some(dec!(0.58)), None, dec!(0.60), None));
        refreshed.request.context["evaluation_slot"] = serde_json::json!(3);
        session.retain(refreshed.clone());
        assert!(!session.due(119));
        assert!(!session.entry_reassessment_due(119));
        session.accept();
        assert!(!session.due(119));
        session.observe_market("b");
        assert!(session.due(45));
        assert!(!session.entry_reassessment_due(45));
    }

    #[test]
    fn restart_restores_interrupted_refresh_budget_without_releasing_forecast() {
        use rust_decimal_macros::dec;
        let old = entry_forecast("0.70", 1);
        let mut request = entry_forecast("0.59", 2).request;
        request.attempt = 60;
        super::super::add_entry_reassessment_context(&mut request, Some(&old), dec!(0.60), 2, true);
        let mut restarted = AgentSession::default();
        restarted.observe_market("a");
        restarted.retain(old.clone());
        restarted.restore_request(&request);
        assert!(restarted.hold_entry(&old, Some(dec!(0.58)), None, dec!(0.60), None));
        assert!(!restarted.entry_reassessment_due(60));
        assert!(!restarted.due(75));
        assert!(restarted.due(105));
        request.context["evaluation_slot"] = serde_json::json!(3);
        restarted.restore_request(&request);
        assert!(!restarted.due(119));
        assert!(!restarted.entry_reassessment_due(119));
        assert!(!restarted.entry_reassessment_due(120));
        restarted.observe_market("b");
        assert!(restarted.due(45));
    }

    #[test]
    fn a_pending_scheduled_result_must_review_the_affordable_evidence() {
        use rust_decimal_macros::dec;
        let mut session = AgentSession::default();
        session.observe_market("a");
        let old = entry_forecast("0.70", 1);
        session.retain(old.clone());
        assert!(session.hold_entry(&old, Some(dec!(0.59)), None, dec!(0.60), None));
        let still_expensive = entry_forecast("0.65", 2);
        session.retain(still_expensive.clone());
        assert!(session.hold_entry(&still_expensive, Some(dec!(0.59)), None, dec!(0.60), None));
        assert!(session.entry_reassessment_due(90));
        let mut revised = entry_forecast("0.70", 3);
        revised.prediction.as_mut().unwrap().direction = crate::btc::types::BtcOutcome::Down;
        revised.prediction.as_mut().unwrap().probability_up = 0.3;
        session.retain(revised.clone());
        assert!(!session.hold_entry(
            &revised,
            Some(dec!(0.59)),
            Some(dec!(0.45)),
            dec!(0.60),
            None
        ));
        assert!(!session.entry_reassessment_due(110));
    }
    #[test]
    fn calibration_evidence_and_entry_budget_survive_restart_and_reset_on_rollover() {
        for profile_key in [
            super::super::CALIBRATION_PROFILE_KEY,
            super::super::MARKET_CONTEXT_PROFILE_KEY,
            super::super::EXECUTION_EVIDENCE_PROFILE_KEY,
            super::super::MARKET_REFERENCE_PROFILE_KEY,
        ] {
            use rust_decimal_macros::dec;
            let mut result = entry_forecast("0.70", 2);
            let profile = super::super::profile_for_key(profile_key).unwrap();
            result.request.selection.profile_key = profile.key.into();
            result.request.selection.profile_sha256 = super::super::hash(&profile).unwrap();
            let evidence = super::super::calibration::unavailable(result.request.window_start);
            result.request.context["historical_calibration"] = evidence.clone();
            result.request.input_sha256 = super::super::hash(&result.request.context).unwrap();
            let wire = serde_json::to_vec(&result.request).unwrap();
            let restored: EvaluationRequest = serde_json::from_slice(&wire).unwrap();
            let mut session = AgentSession::default();
            session.observe_market("a");
            session.restore_request(&restored);
            assert_eq!(session.calibration, Some(evidence.clone()));
            assert_eq!(session.evaluation_slot(), 3);
            session.retain(result.clone());
            assert!(session.hold_entry(&result, Some(dec!(0.55)), None, dec!(0.60), None));
            assert!(session.entry_reassessment_due(100));
            assert!(!session.entry_reassessment_due(120));
            assert_eq!(
                super::super::hash(&restored.context).unwrap(),
                restored.input_sha256
            );
            session.observe_market("b");
            assert!(session.calibration.is_none());
            assert_eq!(session.evaluation_slot(), 1);
            assert!(session.due(45));
        }
    }
    fn fresh_forecast(price: &str, slot: usize) -> EvaluationResult {
        let mut result = entry_forecast(price, slot);
        let profile =
            super::super::profile_for_key(super::super::FRESH_EVIDENCE_PROFILE_KEY).unwrap();
        result.request.selection.profile_key = profile.key.into();
        result.request.selection.profile_sha256 = super::super::hash(&profile).unwrap();
        result
    }

    #[test]
    fn absent_price_threshold_preserves_forecasts_and_original_affordability_check() {
        use rust_decimal_macros::dec;
        let mut session = AgentSession::default();
        session.observe_market("a");
        let result = fresh_forecast("0.49", 3);
        session.retain(result.clone());
        for price in [dec!(0.12), dec!(0.44), dec!(0.90)] {
            assert!(!session.hold_entry(&result, Some(price), None, dec!(0.50), None));
        }
        assert!(!session.entry_reassessment_due(110));
        let above_cap = fresh_forecast("0.70", 1);
        session.retain(above_cap.clone());
        assert!(session.hold_entry(&above_cap, Some(dec!(0.49)), None, dec!(0.50), None));
    }

    #[test]
    fn material_changes_below_cap_hold_only_new_profile_and_selected_side() {
        use rust_decimal_macros::dec;
        let mut session = AgentSession::default();
        session.observe_market("a");
        let old = fresh_forecast("0.49", 1);
        session.retain(old.clone());
        // Small moves, missing books, and unrelated-side moves do not invalidate it.
        assert!(!session.hold_entry(
            &old,
            Some(dec!(0.45)),
            Some(dec!(0.10)),
            dec!(0.50),
            Some(dec!(0.05))
        ));
        assert!(!session.hold_entry(&old, None, Some(dec!(0.10)), dec!(0.50), Some(dec!(0.05))));
        assert!(!session.hold_entry(&old, Some(dec!(0.12)), None, dec!(0.50), None));
        assert!(session.hold_entry(&old, Some(dec!(0.44)), None, dec!(0.50), Some(dec!(0.05))));
        assert!(session.entry_reassessment_due(60));
        assert!(session.hold_entry(&old, Some(dec!(0.49)), None, dec!(0.50), Some(dec!(0.05))));
        let mut independent = AgentSession::default();
        independent.observe_market("a");
        assert!(!independent.entry_reassessment_due(60));
        let legacy = entry_forecast("0.49", 1);
        assert!(!independent.hold_entry(
            &legacy,
            Some(dec!(0.12)),
            None,
            dec!(0.50),
            Some(dec!(0.05))
        ));
        // A successful refresh releases only the previous forecast's hold.
        let refreshed = fresh_forecast("0.44", 2);
        session.retain(refreshed.clone());
        assert!(!session.hold_entry(
            &refreshed,
            Some(dec!(0.43)),
            None,
            dec!(0.50),
            Some(dec!(0.05))
        ));
        assert!(session.hold_entry(
            &refreshed,
            Some(dec!(0.49)),
            None,
            dec!(0.50),
            Some(dec!(0.05))
        ));
        // DOWN uses the DOWN book, not the UP book.
        let mut down = fresh_forecast("0.49", 1);
        down.prediction.as_mut().unwrap().direction = crate::btc::types::BtcOutcome::Down;
        down.prediction.as_mut().unwrap().probability_up = 0.3;
        assert!(independent.hold_entry(
            &down,
            Some(dec!(0.49)),
            Some(dec!(0.35)),
            dec!(0.50),
            Some(dec!(0.05))
        ));
    }

    #[test]
    fn below_cap_collapse_during_inference_and_restart_preserve_budget_and_recovery() {
        use rust_decimal_macros::dec;
        let old = fresh_forecast("0.49", 1);
        let mut session = AgentSession::default();
        session.observe_market("a");
        // Completion-time check catches the observed 49c -> 12c regression.
        session.retain(old.clone());
        assert!(session.hold_entry(&old, Some(dec!(0.12)), None, dec!(0.50), Some(dec!(0.05))));
        let mut request = fresh_forecast("0.12", 2).request;
        super::super::add_entry_reassessment_context(&mut request, Some(&old), dec!(0.50), 2, true);
        let restored: EvaluationRequest =
            serde_json::from_value(serde_json::to_value(&request).unwrap()).unwrap();
        let mut restarted = AgentSession::default();
        restarted.observe_market("a");
        restarted.retain(old.clone());
        restarted.restore_request(&restored);
        assert!(restarted.hold_entry(&old, Some(dec!(0.12)), None, dec!(0.50), Some(dec!(0.05))));
        assert!(!restarted.entry_reassessment_due(70));
        assert!(!restarted.due(75));
        assert!(restarted.due(105));
        // No fourth request or release of invalid evidence after budget exhaustion.
        request.context["evaluation_slot"] = serde_json::json!(3);
        restarted.restore_request(&request);
        assert!(!restarted.entry_reassessment_due(119));
        assert!(!restarted.due(119));
        assert!(!restarted.due(120));
        restarted.observe_market("b");
        assert!(restarted.due(45));
        restarted.accept();
        assert!(!restarted.due(75));
    }
}
