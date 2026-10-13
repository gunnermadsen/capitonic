//! Shared execution stages extend the existing process metrics owner.
use super::*;
use crate::execution::polymarket::oms::Origin;
#[derive(Clone, Copy)]
pub enum OrderStage {
    Candidate,
    Preview,
    Preparation,
    Admission,
    Submission,
    Observation,
    Replay,
}
impl OrderStage {
    fn label(self) -> &'static str {
        match self {
            Self::Candidate => "candidate",
            Self::Preview => "preview",
            Self::Preparation => "preparation",
            Self::Admission => "admission",
            Self::Submission => "submission",
            Self::Observation => "observation",
            Self::Replay => "replay",
        }
    }
}
#[derive(Clone, Copy)]
pub enum OrderOutcome {
    Accepted,
    Rejected,
    Pending,
    Executed,
    Nonexecution,
    Error,
    Inapplicable,
}
impl OrderOutcome {
    fn label(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Rejected => "rejected",
            Self::Pending => "pending",
            Self::Executed => "executed",
            Self::Nonexecution => "nonexecution",
            Self::Error => "error",
            Self::Inapplicable => "inapplicable",
        }
    }
}
impl ExecutionFreshnessMetrics {
    pub fn expect_order_recovery(&self) {
        self.recovery_healthy.store(0, Relaxed);
        self.last_recovery_at.store(0, Relaxed);
    }

    pub fn order_event(&self, origin: Origin, stage: OrderStage, outcome: OrderOutcome) {
        let labels = (origin.as_str(), stage.label(), outcome.label());
        *self
            .order_events
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(labels)
            .or_default() += 1;
        tracing::info!(event="order_stage",process_id=%self.process_id,execution_mode=self.mode,origin=labels.0,stage=labels.1,outcome=labels.2,"Order execution stage");
    }
    pub fn order_recovery(&self, healthy: bool, unresolved: Option<usize>) {
        self.recovery_healthy.store(i64::from(healthy), Relaxed);
        self.last_recovery_at.store(Utc::now().timestamp(), Relaxed);
        if let Some(count) = unresolved {
            self.unresolved_orders.store(count as i64, Relaxed);
        }
        tracing::info!(event="order_recovery",process_id=%self.process_id,execution_mode=self.mode,healthy,unresolved,"Order recovery pass");
    }
    pub(super) fn write_order_events(&self, out: &mut String) {
        for ((origin, stage, outcome), value) in self
            .order_events
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
        {
            let _=writeln!(out,"polymarket_order_stage_events_total{{process_id=\"{}\",execution_mode=\"{}\",origin=\"{}\",stage=\"{}\",outcome=\"{}\"}} {}",self.process_id,self.mode,origin,stage,outcome,value);
        }
        for (name, value) in [
            ("recovery_healthy", self.recovery_healthy.load(Relaxed)),
            (
                "last_recovery_timestamp_seconds",
                self.last_recovery_at.load(Relaxed),
            ),
            ("unresolved_orders", self.unresolved_orders.load(Relaxed)),
        ] {
            if value >= 0 {
                let _ = writeln!(
                    out,
                    "polymarket_order_{name}{{process_id=\"{}\",execution_mode=\"{}\"}} {value}",
                    self.process_id, self.mode
                );
            }
        }
    }
}
