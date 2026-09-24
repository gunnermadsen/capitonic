use std::{
    collections::BTreeMap,
    fmt::Write as _,
    sync::{Mutex, OnceLock},
    time::Duration,
};

#[derive(Clone, Copy)]
pub enum FillEconomicsSite {
    WebsocketBeforeInsert,
    WebsocketAfterInsert,
    WebsocketCancellation,
    RestBackfill,
}

impl FillEconomicsSite {
    const fn as_str(self) -> &'static str {
        match self {
            Self::WebsocketBeforeInsert => "websocket_before_insert",
            Self::WebsocketAfterInsert => "websocket_after_insert",
            Self::WebsocketCancellation => "websocket_cancellation",
            Self::RestBackfill => "rest_backfill",
        }
    }
}

#[derive(Clone, Copy)]
pub enum FillIdentitySite {
    WebsocketLegacyTrade,
    RestBackfill,
}

impl FillIdentitySite {
    const fn as_str(self) -> &'static str {
        match self {
            Self::WebsocketLegacyTrade => "websocket_legacy_trade",
            Self::RestBackfill => "rest_backfill",
        }
    }
}

const BUCKETS: [f64; 11] = [
    0.001, 0.002, 0.005, 0.01, 0.02, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5,
];

#[derive(Default)]
struct Histogram {
    buckets: [u64; BUCKETS.len()],
    count: u64,
    sum: f64,
}

impl Histogram {
    fn observe(&mut self, elapsed: Duration) {
        let seconds = elapsed.as_secs_f64();
        self.count = self.count.saturating_add(1);
        self.sum += seconds;
        for (count, bound) in self.buckets.iter_mut().zip(BUCKETS) {
            if seconds <= bound {
                *count = count.saturating_add(1);
            }
        }
    }
}

type SeriesKey = (&'static str, &'static str, &'static str);
static READS: OnceLock<Mutex<BTreeMap<SeriesKey, Histogram>>> = OnceLock::new();

fn observe(operation: &'static str, site: &'static str, result: &'static str, elapsed: Duration) {
    let mut reads = READS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    reads
        .entry((operation, site, result))
        .or_default()
        .observe(elapsed);
}

pub(super) fn observe_economics(site: FillEconomicsSite, success: bool, elapsed: Duration) {
    observe(
        "filled_economics",
        site.as_str(),
        if success { "success" } else { "error" },
        elapsed,
    );
}

pub(super) fn observe_identity(
    site: FillIdentitySite,
    result: &Result<Option<String>, sqlx::Error>,
    elapsed: Duration,
) {
    let outcome = match result {
        Ok(Some(_)) => "found",
        Ok(None) => "missing",
        Err(_) => "error",
    };
    observe("fill_identity", site.as_str(), outcome, elapsed);
}

pub(super) fn prometheus_metrics() -> String {
    let mut output = String::from(
        "# HELP polymarket_live_fill_read_duration_seconds Client-observed time from starting the store read through SQLx returning; includes pool wait, database execution, transfer, and decoding. Results describe the read only, not downstream execution or persistence.\n\
# TYPE polymarket_live_fill_read_duration_seconds histogram\n",
    );
    let Some(reads) = READS.get() else {
        return output;
    };
    let reads = reads.lock().unwrap_or_else(|error| error.into_inner());
    for ((operation, site, result), histogram) in reads.iter() {
        let labels = format!("operation=\"{operation}\",site=\"{site}\",result=\"{result}\"");
        for (bound, count) in BUCKETS.iter().zip(histogram.buckets) {
            let _ = writeln!(output, "polymarket_live_fill_read_duration_seconds_bucket{{{labels},le=\"{bound}\"}} {count}");
        }
        let _ = writeln!(
            output,
            "polymarket_live_fill_read_duration_seconds_bucket{{{labels},le=\"+Inf\"}} {}",
            histogram.count
        );
        let _ = writeln!(
            output,
            "polymarket_live_fill_read_duration_seconds_sum{{{labels}}} {}",
            histogram.sum
        );
        let _ = writeln!(
            output,
            "polymarket_live_fill_read_duration_seconds_count{{{labels}}} {}",
            histogram.count
        );
    }
    output
}

#[cfg(test)]
mod tests {
    use super::{
        observe_economics, observe_identity, prometheus_metrics, FillEconomicsSite,
        FillIdentitySite,
    };
    use std::time::Duration;

    #[test]
    fn metrics_distinguish_query_errors_from_identity_misses() {
        observe_economics(
            FillEconomicsSite::WebsocketBeforeInsert,
            true,
            Duration::from_millis(3),
        );
        observe_identity(
            FillIdentitySite::RestBackfill,
            &Ok(None),
            Duration::from_millis(7),
        );
        observe_identity(
            FillIdentitySite::RestBackfill,
            &Err(sqlx::Error::RowNotFound),
            Duration::from_millis(11),
        );
        let metrics = prometheus_metrics();
        assert!(metrics.contains(
            "operation=\"filled_economics\",site=\"websocket_before_insert\",result=\"success\""
        ));
        assert!(metrics
            .contains("operation=\"fill_identity\",site=\"rest_backfill\",result=\"missing\""));
        assert!(
            metrics.contains("operation=\"fill_identity\",site=\"rest_backfill\",result=\"error\"")
        );
        assert!(metrics.contains("le=\"0.005\"} 1"));
        assert!(metrics.contains("le=\"0.005\"} 0"));
    }
}
