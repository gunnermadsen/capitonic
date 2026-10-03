use std::{sync::Arc, time::Duration};

use anyhow::Result;
use prometheus_client::{
    encoding::{text::encode, EncodeLabelSet},
    metrics::{
        counter::Counter,
        family::Family,
        histogram::{exponential_buckets, Histogram},
    },
    registry::Registry,
};

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct BackfillLabels {
    strategy: String,
    outcome: String,
}

struct BackfillMetricsInner {
    registry: Registry,
    executions: Family<BackfillLabels, Counter>,
    duration: Family<BackfillLabels, Histogram>,
}

#[derive(Clone)]
pub(crate) struct BackfillMetrics(Arc<BackfillMetricsInner>);

impl Default for BackfillMetrics {
    fn default() -> Self {
        let mut registry = Registry::with_prefix("market_data_ingester");
        let executions = Family::<BackfillLabels, Counter>::default();
        let duration = Family::<BackfillLabels, Histogram>::new_with_constructor(|| {
            Histogram::new(exponential_buckets(0.5, 2.0, 17))
        });
        registry.register(
            "backfill_executions",
            "Backfill shard execution attempts by strategy and terminal outcome.",
            executions.clone(),
        );
        registry.register(
            "backfill_execution_duration_seconds",
            "Elapsed worker execution time for backfill shard attempts.",
            duration.clone(),
        );
        Self(Arc::new(BackfillMetricsInner {
            registry,
            executions,
            duration,
        }))
    }
}

impl BackfillMetrics {
    pub(crate) fn observe(&self, strategy: &str, outcome: &str, elapsed: Duration) {
        let labels = BackfillLabels {
            strategy: strategy.to_owned(),
            outcome: outcome.to_owned(),
        };
        self.0.executions.get_or_create(&labels).inc();
        self.0
            .duration
            .get_or_create(&labels)
            .observe(elapsed.as_secs_f64());
    }

    pub(crate) fn render(&self) -> Result<String> {
        let mut output = String::new();
        encode(&mut output, &self.0.registry)?;
        if output.ends_with("# EOF\n") {
            output.truncate(output.len() - "# EOF\n".len());
        }
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_backfill_count_and_duration_without_job_cardinality() {
        let metrics = BackfillMetrics::default();
        metrics.observe("pmxt", "completed", Duration::from_secs(3));
        let rendered = metrics.render().unwrap();
        assert!(rendered.contains("market_data_ingester_backfill_executions_total"));
        assert!(rendered.contains("market_data_ingester_backfill_execution_duration_seconds"));
        assert!(rendered.contains("strategy=\"pmxt\""));
        assert!(rendered.contains("outcome=\"completed\""));
        assert!(!rendered.contains("job_id"));
        assert!(!rendered.contains("# EOF"));
    }
}
