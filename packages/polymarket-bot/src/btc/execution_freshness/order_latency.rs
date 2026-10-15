//! Monotonic, bounded order timings. Recording never waits for a scrape or allocates.
use super::*;
use crate::execution::polymarket::{oms::Origin, AdmissionRejection};
use std::{
    future::Future,
    time::{Duration, Instant},
};

const BOUNDS_NS: [u64; 19] = [
    10_000,
    25_000,
    50_000,
    100_000,
    250_000,
    500_000,
    1_000_000,
    2_500_000,
    5_000_000,
    10_000_000,
    25_000_000,
    50_000_000,
    100_000_000,
    250_000_000,
    500_000_000,
    1_000_000_000,
    2_500_000_000,
    5_000_000_000,
    30_000_000_000,
];
#[derive(Clone, Copy)]
#[repr(usize)]
pub enum OrderLatencyStage {
    Evidence,
    Preview,
    Preparation,
    Execution,
    ReservationWait,
    Admission,
    Accounting,
    FinalValidation,
    Adapter,
    PaperArrival,
    Persistence,
    Reconciliation,
    Observation,
    Transport,
}
const STAGES: [&str; 14] = [
    "evidence",
    "preview",
    "preparation",
    "execution",
    "reservation_wait",
    "admission",
    "accounting",
    "final_validation",
    "adapter",
    "paper_arrival",
    "persistence",
    "reconciliation",
    "observation",
    "transport",
];
#[derive(Clone, Copy)]
#[repr(usize)]
pub enum LatencyOutcome {
    Completed,
    Rejected,
    Error,
    Cancelled,
}
const OUTCOMES: [&str; 4] = ["completed", "rejected", "error", "cancelled"];

#[derive(Debug, Default)]
struct Histogram {
    // Exclusive bins make observation two atomic additions plus count/max; the scrape
    // constructs cumulative buckets. Scrapes are approximate while writes are in flight.
    bins: [AtomicU64; BOUNDS_NS.len() + 1],
    sum_ns: AtomicU64,
    max_ns: AtomicU64,
}
impl Histogram {
    fn observe(&self, elapsed: Duration) {
        let ns = elapsed.as_nanos().min(u64::MAX as u128) as u64;
        let bin = BOUNDS_NS.partition_point(|bound| *bound < ns);
        self.sum_ns.fetch_add(ns, Relaxed);
        self.max_ns.fetch_max(ns, Relaxed);
        self.bins[bin].fetch_add(1, Relaxed);
    }
}
#[derive(Debug)]
pub(super) struct OrderLatencyMetrics(Box<[Histogram]>);
impl Default for OrderLatencyMetrics {
    fn default() -> Self {
        Self(
            (0..2 * STAGES.len() * OUTCOMES.len())
                .map(|_| Histogram::default())
                .collect(),
        )
    }
}
impl OrderLatencyMetrics {
    fn histogram(
        &self,
        origin: Origin,
        stage: OrderLatencyStage,
        outcome: LatencyOutcome,
    ) -> &Histogram {
        let origin = match origin {
            Origin::Api => 0,
            Origin::Umr => 1,
        };
        &self.0[(origin * STAGES.len() + stage as usize) * OUTCOMES.len() + outcome as usize]
    }
    pub(super) fn write(&self, out: &mut String, process_id: Uuid, mode: &str) {
        for (index, histogram) in self.0.iter().enumerate() {
            let bins = histogram.bins.each_ref().map(|v| v.load(Relaxed));
            let count: u64 = bins.iter().sum();
            if count == 0 {
                continue;
            }
            let outcome = OUTCOMES[index % OUTCOMES.len()];
            let stage = STAGES[(index / OUTCOMES.len()) % STAGES.len()];
            let origin = ["api", "umr"][index / (OUTCOMES.len() * STAGES.len())];
            let labels = format!("process_id=\"{process_id}\",execution_mode=\"{mode}\",origin=\"{origin}\",stage=\"{stage}\",outcome=\"{outcome}\"");
            let mut cumulative = 0;
            for (i, ns) in BOUNDS_NS.iter().enumerate() {
                cumulative += bins[i];
                let bound = *ns as f64 / 1e9;
                let _ = writeln!(out, "polymarket_order_stage_duration_seconds_bucket{{{labels},le=\"{bound}\"}} {cumulative}");
            }
            let _ = writeln!(
                out,
                "polymarket_order_stage_duration_seconds_bucket{{{labels},le=\"+Inf\"}} {count}"
            );
            let _ = writeln!(
                out,
                "polymarket_order_stage_duration_seconds_count{{{labels}}} {count}"
            );
            let _ = writeln!(
                out,
                "polymarket_order_stage_duration_seconds_sum{{{labels}}} {}",
                histogram.sum_ns.load(Relaxed) as f64 / 1e9
            );
            let _ = writeln!(
                out,
                "polymarket_order_stage_duration_max_seconds{{{labels}}} {}",
                histogram.max_ns.load(Relaxed) as f64 / 1e9
            );
        }
    }
}

pub struct OrderTimer<'a> {
    metrics: &'a ExecutionFreshnessMetrics,
    origin: Origin,
    stage: OrderLatencyStage,
    started: Instant,
    outcome: LatencyOutcome,
}
impl OrderTimer<'_> {
    pub fn finish(mut self, outcome: LatencyOutcome) {
        self.outcome = outcome;
    }
}
impl Drop for OrderTimer<'_> {
    fn drop(&mut self) {
        self.metrics.order_duration(
            self.origin,
            self.stage,
            self.outcome,
            self.started.elapsed(),
        );
    }
}
impl ExecutionFreshnessMetrics {
    pub fn order_timer(&self, origin: Origin, stage: OrderLatencyStage) -> OrderTimer<'_> {
        OrderTimer {
            metrics: self,
            origin,
            stage,
            started: Instant::now(),
            outcome: LatencyOutcome::Cancelled,
        }
    }
    pub fn order_duration(
        &self,
        origin: Origin,
        stage: OrderLatencyStage,
        outcome: LatencyOutcome,
        elapsed: Duration,
    ) {
        self.order_latency
            .histogram(origin, stage, outcome)
            .observe(elapsed);
    }
    pub async fn time_order<T>(
        &self,
        origin: Origin,
        stage: OrderLatencyStage,
        future: impl Future<Output = anyhow::Result<T>>,
    ) -> anyhow::Result<T> {
        self.time_order_outcome(origin, stage, future, |_| LatencyOutcome::Completed)
            .await
    }
    pub async fn time_order_outcome<T>(
        &self,
        origin: Origin,
        stage: OrderLatencyStage,
        future: impl Future<Output = anyhow::Result<T>>,
        classify: impl FnOnce(&T) -> LatencyOutcome,
    ) -> anyhow::Result<T> {
        let timer = self.order_timer(origin, stage);
        let result = future.await;
        let outcome = match &result {
            Ok(value) => classify(value),
            Err(error) if error.is::<AdmissionRejection>() => LatencyOutcome::Rejected,
            Err(_) => LatencyOutcome::Error,
        };
        timer.finish(outcome);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn shared_timing_preserves_results_and_records_errors_rejections_and_cancellation() {
        for mode in ["paper", "live"] {
            let m = ExecutionFreshnessMetrics::new(Uuid::new_v4(), mode, 100);
            let result = m
                .time_order(Origin::Api, OrderLatencyStage::Admission, async { Ok(7) })
                .await
                .unwrap();
            assert_eq!(result, 7);
            let error = m
                .time_order::<()>(Origin::Api, OrderLatencyStage::Admission, async {
                    anyhow::bail!("failure")
                })
                .await
                .unwrap_err();
            assert_eq!(error.to_string(), "failure");
            assert!(m
                .time_order::<()>(Origin::Api, OrderLatencyStage::Admission, async {
                    Err(AdmissionRejection {
                        reason: "insufficient_collateral".into(),
                        policy_results: serde_json::json!([]),
                    }
                    .into())
                })
                .await
                .is_err());
            let reason = m
                .time_order_outcome(
                    Origin::Api,
                    OrderLatencyStage::FinalValidation,
                    async { Ok(Some(7)) },
                    |_| LatencyOutcome::Rejected,
                )
                .await
                .unwrap();
            assert_eq!(reason, Some(7));
            {
                let future = m.time_order::<()>(
                    Origin::Umr,
                    OrderLatencyStage::Execution,
                    std::future::pending(),
                );
                tokio::pin!(future);
                assert!(
                    tokio::time::timeout(Duration::from_millis(1), future.as_mut())
                        .await
                        .is_err()
                );
            }
            // A dropped timer explicitly records cancellation, never a fabricated success.
            drop(m.order_timer(Origin::Api, OrderLatencyStage::Adapter));
            let mut output = String::new();
            m.order_latency.write(&mut output, m.process_id, m.mode);
            for outcome in ["completed", "rejected", "error", "cancelled"] {
                assert!(output.contains(&format!("outcome=\"{outcome}\"")));
            }
            assert!(output.contains(&format!("execution_mode=\"{mode}\"")));
            assert!(!output.contains("order_id"));
            assert!(output.lines().any(|l| l
                .contains("origin=\"umr\",stage=\"execution\",outcome=\"cancelled\"")
                && l.ends_with(" 1")));
        }
    }
    #[test]
    fn histogram_buckets_sum_max_and_origin_isolation_are_exact_after_recording() {
        let m = ExecutionFreshnessMetrics::new(Uuid::new_v4(), "paper", 100);
        for us in [10, 20, 2000] {
            m.order_duration(
                Origin::Api,
                OrderLatencyStage::Admission,
                LatencyOutcome::Completed,
                Duration::from_micros(us),
            );
        }
        let mut output = String::new();
        m.order_latency.write(&mut output, m.process_id, m.mode);
        assert!(output.contains("le=\"0.00001\"} 1"));
        assert!(output.contains("le=\"0.000025\"} 2"));
        assert!(output.contains("le=\"+Inf\"} 3"));
        assert!(output.lines().any(|l| l
            .starts_with("polymarket_order_stage_duration_seconds_sum")
            && l.ends_with(" 0.00203")));
        assert!(output.lines().any(|l| l
            .starts_with("polymarket_order_stage_duration_max_seconds")
            && l.ends_with(" 0.002")));
        assert!(!output.contains("origin=\"umr\""));
    }
}
