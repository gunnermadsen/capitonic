use std::{
    collections::BTreeMap,
    fmt::Write as _,
    sync::{Arc, Mutex, OnceLock, Weak},
};

use anyhow::Result;
use polymarket_client_sdk_v2::error::{Kind as SdkErrorKind, Status as SdkStatus};
use uuid::Uuid;

use crate::execution::ReconciliationReport;

static REGISTRY: OnceLock<Mutex<Vec<Weak<LiveReconciliationMetrics>>>> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum ReconciliationOutcome {
    Clean,
    Unsafe,
    TransientFailure,
    HardFailure,
}

impl ReconciliationOutcome {
    const ALL: [Self; 4] = [
        Self::Clean,
        Self::Unsafe,
        Self::TransientFailure,
        Self::HardFailure,
    ];

    const fn as_str(self) -> &'static str {
        match self {
            Self::Clean => "clean",
            Self::Unsafe => "unsafe",
            Self::TransientFailure => "transient_failure",
            Self::HardFailure => "hard_failure",
        }
    }

    const fn index(self) -> usize {
        match self {
            Self::Clean => 0,
            Self::Unsafe => 1,
            Self::TransientFailure => 2,
            Self::HardFailure => 3,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum ReconciliationStage {
    UserEventBackfill,
    OpenOrders,
    LocalOrders,
    Trades,
    Balances,
    OrderOwnership,
    RestFillBackfill,
    AccountReconciliation,
    SafetyGeneration,
    Persistence,
    Unknown,
}

impl ReconciliationStage {
    const fn as_str(self) -> &'static str {
        match self {
            Self::UserEventBackfill => "user_event_backfill",
            Self::OpenOrders => "open_orders",
            Self::LocalOrders => "local_orders",
            Self::Trades => "trades",
            Self::Balances => "balances",
            Self::OrderOwnership => "order_ownership",
            Self::RestFillBackfill => "rest_fill_backfill",
            Self::AccountReconciliation => "account_reconciliation",
            Self::SafetyGeneration => "safety_generation",
            Self::Persistence => "persistence",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum ReconciliationFailureClass {
    Timeout,
    Connectivity,
    Tls,
    RateLimit,
    UpstreamServer,
    Authentication,
    AccountIntegrity,
    InvalidResponse,
    Persistence,
    SafetyGeneration,
    Unknown,
}

impl ReconciliationFailureClass {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Timeout => "timeout",
            Self::Connectivity => "connectivity",
            Self::Tls => "tls",
            Self::RateLimit => "rate_limit",
            Self::UpstreamServer => "upstream_server",
            Self::Authentication => "authentication",
            Self::AccountIntegrity => "account_integrity",
            Self::InvalidResponse => "invalid_response",
            Self::Persistence => "persistence",
            Self::SafetyGeneration => "safety_generation",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug)]
pub(super) struct StagedReconciliationError {
    pub(super) stage: ReconciliationStage,
    pub(super) error: anyhow::Error,
}

pub(super) fn at_stage<T>(
    stage: ReconciliationStage,
    result: Result<T>,
) -> std::result::Result<T, StagedReconciliationError> {
    result.map_err(|error| StagedReconciliationError { stage, error })
}

pub(super) fn failure_class(
    failure_stage: ReconciliationStage,
    error: &anyhow::Error,
) -> ReconciliationFailureClass {
    if failure_stage == ReconciliationStage::SafetyGeneration {
        return ReconciliationFailureClass::SafetyGeneration;
    }

    let mut saw_connectivity = false;
    for cause in error.chain() {
        if cause.downcast_ref::<sqlx::Error>().is_some() {
            return ReconciliationFailureClass::Persistence;
        }
        if cause.downcast_ref::<rustls::Error>().is_some() {
            return ReconciliationFailureClass::Tls;
        }
        if let Some(status) = cause.downcast_ref::<SdkStatus>() {
            let code = status.status_code.as_u16();
            if code == 429 {
                return ReconciliationFailureClass::RateLimit;
            }
            if status.status_code.is_server_error() {
                return ReconciliationFailureClass::UpstreamServer;
            }
            if matches!(code, 401 | 403) {
                return ReconciliationFailureClass::Authentication;
            }
        }
        if let Some(request_error) = cause.downcast_ref::<reqwest::Error>() {
            if request_error.is_timeout() {
                return ReconciliationFailureClass::Timeout;
            }
            if let Some(status) = request_error.status() {
                if status.as_u16() == 429 {
                    return ReconciliationFailureClass::RateLimit;
                }
                if status.is_server_error() {
                    return ReconciliationFailureClass::UpstreamServer;
                }
                if matches!(status.as_u16(), 401 | 403) {
                    return ReconciliationFailureClass::Authentication;
                }
            }
            saw_connectivity |= request_error.is_connect();
        }
        if let Some(io_error) = cause.downcast_ref::<std::io::Error>() {
            match io_error.kind() {
                std::io::ErrorKind::TimedOut => return ReconciliationFailureClass::Timeout,
                std::io::ErrorKind::ConnectionReset
                | std::io::ErrorKind::ConnectionAborted
                | std::io::ErrorKind::NotConnected
                | std::io::ErrorKind::BrokenPipe
                | std::io::ErrorKind::WouldBlock
                | std::io::ErrorKind::Interrupted
                | std::io::ErrorKind::UnexpectedEof => saw_connectivity = true,
                _ => {}
            }
        }
        if let Some(sdk_error) = cause.downcast_ref::<polymarket_client_sdk_v2::error::Error>() {
            match sdk_error.kind() {
                SdkErrorKind::Synchronization => saw_connectivity = true,
                SdkErrorKind::Validation | SdkErrorKind::Geoblock => {
                    return ReconciliationFailureClass::InvalidResponse;
                }
                _ => {}
            }
        }
    }
    if saw_connectivity {
        return ReconciliationFailureClass::Connectivity;
    }
    match failure_stage {
        ReconciliationStage::AccountReconciliation => ReconciliationFailureClass::AccountIntegrity,
        ReconciliationStage::Persistence => ReconciliationFailureClass::Persistence,
        ReconciliationStage::Unknown => ReconciliationFailureClass::Unknown,
        _ => ReconciliationFailureClass::InvalidResponse,
    }
}

pub(super) fn is_transient_failure(error: &anyhow::Error) -> bool {
    matches!(
        failure_class(ReconciliationStage::Unknown, error),
        ReconciliationFailureClass::Timeout
            | ReconciliationFailureClass::Connectivity
            | ReconciliationFailureClass::Tls
            | ReconciliationFailureClass::RateLimit
            | ReconciliationFailureClass::UpstreamServer
    )
}

#[derive(Default)]
struct MetricsState {
    attempts: [u64; 4],
    failures: BTreeMap<(ReconciliationStage, ReconciliationFailureClass), u64>,
    last_attempt_timestamp: i64,
    last_success_timestamp: i64,
    reconciliation_entry_safe: bool,
    mismatches: usize,
    unresolved_orders: usize,
}

pub(super) struct LiveReconciliationMetrics {
    process_id: Uuid,
    state: Mutex<MetricsState>,
}

impl LiveReconciliationMetrics {
    pub(super) fn new(process_id: Uuid) -> Arc<Self> {
        let metrics = Arc::new(Self {
            process_id,
            state: Mutex::new(MetricsState::default()),
        });
        let mut registry = REGISTRY
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        registry.retain(|entry| entry.strong_count() > 0);
        registry.push(Arc::downgrade(&metrics));
        metrics
    }

    pub(super) fn record_report(&self, report: &ReconciliationReport) {
        let outcome = if report.balances_checked
            && report.mismatches_found == 0
            && report.unresolved_count == 0
        {
            ReconciliationOutcome::Clean
        } else {
            ReconciliationOutcome::Unsafe
        };
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.attempts[outcome.index()] = state.attempts[outcome.index()].saturating_add(1);
        state.last_attempt_timestamp = report.checked_at.timestamp();
        state.mismatches = report.mismatches_found;
        state.unresolved_orders = report.unresolved_count;
        state.reconciliation_entry_safe = outcome == ReconciliationOutcome::Clean;
        if outcome == ReconciliationOutcome::Clean {
            state.last_success_timestamp = report.checked_at.timestamp();
        }
    }

    pub(super) fn record_failure(&self, failure_stage: ReconciliationStage, error: &anyhow::Error) {
        let class = failure_class(failure_stage, error);
        let transient = is_transient_failure(error);
        let outcome = if transient {
            ReconciliationOutcome::TransientFailure
        } else {
            ReconciliationOutcome::HardFailure
        };
        let now = chrono::Utc::now().timestamp();
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.attempts[outcome.index()] = state.attempts[outcome.index()].saturating_add(1);
        *state.failures.entry((failure_stage, class)).or_default() += 1;
        state.last_attempt_timestamp = now;
        if !transient {
            state.reconciliation_entry_safe = false;
        }
    }

    pub(super) fn set_entry_safe(&self, entry_safe: bool) {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .reconciliation_entry_safe = entry_safe;
    }

    fn write(&self, output: &mut String) {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        let labels = format!("process_id=\"{}\"", self.process_id);
        for outcome in ReconciliationOutcome::ALL {
            let _ = writeln!(
                output,
                "polymarket_live_reconciliation_attempts_total{{{labels},outcome=\"{}\"}} {}",
                outcome.as_str(),
                state.attempts[outcome.index()]
            );
        }
        for ((stage, class), count) in &state.failures {
            let _ = writeln!(
                output,
                "polymarket_live_reconciliation_failures_total{{{labels},stage=\"{}\",failure_class=\"{}\"}} {count}",
                stage.as_str(),
                class.as_str()
            );
        }
        for (metric, value) in [
            (
                "last_attempt_timestamp_seconds",
                state.last_attempt_timestamp,
            ),
            (
                "last_success_timestamp_seconds",
                state.last_success_timestamp,
            ),
            ("entry_safe", i64::from(state.reconciliation_entry_safe)),
            ("mismatches", state.mismatches as i64),
            ("unresolved_orders", state.unresolved_orders as i64),
        ] {
            let _ = writeln!(
                output,
                "polymarket_live_reconciliation_{metric}{{{labels}}} {value}"
            );
        }
    }
}

pub fn prometheus_metrics() -> String {
    let mut output = String::new();
    for (name, help, metric_type) in [
        ("attempts_total", "Live reconciliation attempts by terminal outcome.", "counter"),
        ("failures_total", "Live reconciliation failures by bounded stage and failure class.", "counter"),
        ("last_attempt_timestamp_seconds", "Unix timestamp of the latest live reconciliation attempt.", "gauge"),
        ("last_success_timestamp_seconds", "Unix timestamp of the latest clean live reconciliation.", "gauge"),
        ("entry_safe", "Whether the latest reconciliation evidence is safe for entry; transient failures preserve the prior value.", "gauge"),
        ("mismatches", "Mismatches in the latest completed live reconciliation report.", "gauge"),
        ("unresolved_orders", "Unresolved orders in the latest completed live reconciliation report.", "gauge"),
    ] {
        let _ = writeln!(
            output,
            "# HELP polymarket_live_reconciliation_{name} {help}\n# TYPE polymarket_live_reconciliation_{name} {metric_type}"
        );
    }

    let mut registry = REGISTRY
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let mut current = BTreeMap::new();
    registry.retain(|entry| {
        if let Some(metrics) = entry.upgrade() {
            current.insert(metrics.process_id, metrics);
            true
        } else {
            false
        }
    });
    drop(registry);
    for metrics in current.values() {
        metrics.write(&mut output);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use polymarket_client_sdk_v2::error::{Method, StatusCode};

    #[test]
    fn transient_failure_preserves_entry_safety_and_uses_bounded_labels() {
        let process_id = Uuid::new_v4();
        let metrics = LiveReconciliationMetrics::new(process_id);
        metrics.record_report(&ReconciliationReport {
            open_orders: 0,
            balances_checked: true,
            mismatches_found: 0,
            unresolved_count: 0,
            checked_at: Utc::now(),
        });
        let failure = at_stage(
            ReconciliationStage::OpenOrders,
            Err::<(), _>(anyhow::Error::new(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "secret upstream detail",
            ))),
        )
        .unwrap_err();
        metrics.record_failure(failure.stage, &failure.error);

        let rendered = prometheus_metrics();
        assert!(rendered.contains(&format!(
            "polymarket_live_reconciliation_entry_safe{{process_id=\"{process_id}\"}} 1"
        )));
        assert!(rendered.contains("stage=\"open_orders\",failure_class=\"timeout\""));
        assert!(!rendered.contains("secret upstream detail"));
    }

    #[test]
    fn transient_failure_does_not_open_fail_closed_startup_gate() {
        let process_id = Uuid::new_v4();
        let metrics = LiveReconciliationMetrics::new(process_id);
        let failure = at_stage(
            ReconciliationStage::OpenOrders,
            Err::<(), _>(anyhow::Error::new(std::io::Error::new(
                std::io::ErrorKind::ConnectionReset,
                "connection reset",
            ))),
        )
        .unwrap_err();
        metrics.record_failure(failure.stage, &failure.error);

        let rendered = prometheus_metrics();
        assert!(rendered.contains(&format!(
            "polymarket_live_reconciliation_entry_safe{{process_id=\"{process_id}\"}} 0"
        )));
        assert!(rendered.contains(&format!(
            "polymarket_live_reconciliation_attempts_total{{process_id=\"{process_id}\",outcome=\"transient_failure\"}} 1"
        )));
    }

    #[test]
    fn venue_status_failures_classify_retryable_and_auth_boundaries() {
        let rate_limit = anyhow::Error::new(polymarket_client_sdk_v2::error::Error::status(
            StatusCode::TOO_MANY_REQUESTS,
            Method::GET,
            "/orders".to_string(),
            "rate limited",
        ));
        assert!(is_transient_failure(&rate_limit));
        assert_eq!(
            failure_class(ReconciliationStage::OpenOrders, &rate_limit),
            ReconciliationFailureClass::RateLimit
        );

        let authentication = anyhow::Error::new(polymarket_client_sdk_v2::error::Error::status(
            StatusCode::UNAUTHORIZED,
            Method::GET,
            "/orders".to_string(),
            "unauthorized",
        ));
        assert!(!is_transient_failure(&authentication));
        assert_eq!(
            failure_class(ReconciliationStage::OpenOrders, &authentication),
            ReconciliationFailureClass::Authentication
        );
    }

    #[test]
    fn unsafe_report_and_hard_failure_clear_entry_safety() {
        let process_id = Uuid::new_v4();
        let metrics = LiveReconciliationMetrics::new(process_id);
        metrics.record_report(&ReconciliationReport {
            open_orders: 1,
            balances_checked: true,
            mismatches_found: 1,
            unresolved_count: 1,
            checked_at: Utc::now(),
        });
        let rendered = prometheus_metrics();
        assert!(rendered.contains(&format!(
            "polymarket_live_reconciliation_entry_safe{{process_id=\"{process_id}\"}} 0"
        )));
        assert!(rendered.contains(&format!(
            "polymarket_live_reconciliation_mismatches{{process_id=\"{process_id}\"}} 1"
        )));

        let failure = at_stage(
            ReconciliationStage::Persistence,
            Err::<(), _>(anyhow::anyhow!("write failed")),
        )
        .unwrap_err();
        metrics.record_failure(failure.stage, &failure.error);
        assert!(!is_transient_failure(&failure.error));
        let rendered = prometheus_metrics();
        assert!(rendered.contains(&format!(
            "polymarket_live_reconciliation_attempts_total{{process_id=\"{process_id}\",outcome=\"hard_failure\"}} 1"
        )));
        assert!(rendered.contains(&format!(
            "polymarket_live_reconciliation_entry_safe{{process_id=\"{process_id}\"}} 0"
        )));
    }
}
