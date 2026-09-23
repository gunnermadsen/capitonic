use std::{
    collections::BTreeMap,
    fmt::Write as _,
    sync::{
        atomic::{AtomicI64, AtomicU64, Ordering::Relaxed},
        Arc, Mutex, OnceLock, Weak,
    },
    time::Instant,
};

use anyhow::Result;
use polymarket_client_sdk_v2::error::{Kind as SdkErrorKind, Status as SdkStatus};
use uuid::Uuid;

use crate::execution::ReconciliationReport;

static REGISTRY: OnceLock<Mutex<Vec<Weak<LiveReconciliationMetrics>>>> = OnceLock::new();
static USER_WS_EVENTS: AtomicU64 = AtomicU64::new(0);
static USER_WS_FILL_EVENTS: AtomicU64 = AtomicU64::new(0);
static USER_WS_RECONNECTS: AtomicU64 = AtomicU64::new(0);
static USER_WS_QUEUE_OVERFLOWS: AtomicU64 = AtomicU64::new(0);
static USER_WS_LAST_EVENT: AtomicI64 = AtomicI64::new(0);
static USER_WS_LAST_PONG: AtomicI64 = AtomicI64::new(0);

pub(super) fn record_user_ws_event(fill: bool) {
    USER_WS_EVENTS.fetch_add(1, Relaxed);
    if fill {
        USER_WS_FILL_EVENTS.fetch_add(1, Relaxed);
    }
    USER_WS_LAST_EVENT.store(chrono::Utc::now().timestamp(), Relaxed);
}

pub(super) fn record_user_ws_reconnect() {
    USER_WS_RECONNECTS.fetch_add(1, Relaxed);
}

pub(super) fn record_user_ws_pong() {
    USER_WS_LAST_PONG.store(chrono::Utc::now().timestamp(), Relaxed);
}

pub(super) fn record_user_ws_queue_overflow() {
    USER_WS_QUEUE_OVERFLOWS.fetch_add(1, Relaxed);
}

pub(super) fn record_process_fill_latency(
    process_id: Uuid,
    first_fill: bool,
    final_fill: bool,
    seconds: f64,
) {
    let mut registry = REGISTRY
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    registry.retain(|entry| entry.strong_count() > 0);
    for metrics in registry.iter().filter_map(Weak::upgrade) {
        if metrics.process_id == process_id {
            let mut state = metrics
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if first_fill {
                state
                    .execution_latencies
                    .entry("acknowledgement_to_first_fill")
                    .or_default()
                    .observe(seconds);
            }
            if final_fill {
                state
                    .execution_latencies
                    .entry("acknowledgement_to_final_fill")
                    .or_default()
                    .observe(seconds);
            }
        }
    }
}

pub(super) fn record_process_execution_outcome(process_id: Uuid, outcome: &'static str) {
    let mut registry = REGISTRY
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    registry.retain(|entry| entry.strong_count() > 0);
    for metrics in registry.iter().filter_map(Weak::upgrade) {
        if metrics.process_id == process_id {
            let mut state = metrics
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            *state.execution_outcomes.entry(outcome).or_default() += 1;
        }
    }
}

pub(crate) fn record_transport_runtime_termination(process_id: Uuid) {
    let mut registry = REGISTRY
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    registry.retain(|entry| entry.strong_count() > 0);
    for metrics in registry.iter().filter_map(Weak::upgrade) {
        if metrics.process_id == process_id {
            metrics.record_transport_runtime_outcome("terminated");
        }
    }
}

#[derive(Clone, Default)]
struct DurationHistogram {
    buckets: [u64; 8],
    count: u64,
    sum: f64,
}

const DURATION_BUCKETS: [f64; 8] = [0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0];

impl DurationHistogram {
    fn observe(&mut self, value: f64) {
        self.count = self.count.saturating_add(1);
        self.sum += value;
        for (index, bound) in DURATION_BUCKETS.iter().enumerate() {
            if value <= *bound {
                self.buckets[index] = self.buckets[index].saturating_add(1);
            }
        }
    }
}

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
    runs: BTreeMap<(&'static str, ReconciliationOutcome), u64>,
    durations: BTreeMap<&'static str, DurationHistogram>,
    current_source: &'static str,
    started_at: Option<Instant>,
    in_progress: bool,
    fallback_activations: BTreeMap<&'static str, u64>,
    fills_recovered_http: u64,
    recovery_started_at: Option<Instant>,
    last_recovery_duration_seconds: f64,
    execution_latencies: BTreeMap<&'static str, DurationHistogram>,
    execution_outcomes: BTreeMap<&'static str, u64>,
    order_path_events: BTreeMap<(&'static str, &'static str, &'static str), u64>,
    order_path_durations: BTreeMap<&'static str, DurationHistogram>,
    order_path_terminal_outcomes: BTreeMap<(&'static str, &'static str), u64>,
    order_path_in_flight: u64,
    order_path_last_activity_timestamp: i64,
    entry_eligibility_changes: BTreeMap<&'static str, u64>,
    clob_operation_durations: BTreeMap<&'static str, DurationHistogram>,
    clob_operation_timeouts: BTreeMap<&'static str, u64>,
    submit_guard_wait: DurationHistogram,
    transport_runtime_outcomes: BTreeMap<&'static str, u64>,
    unresolved_submit_unknown: usize,
    oldest_submit_unknown_age_seconds: f64,
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

    pub(super) fn record_started(
        &self,
        source: &'static str,
        fallback_reason: Option<&'static str>,
    ) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.current_source = source;
        state.started_at = Some(Instant::now());
        state.in_progress = true;
        if let Some(reason) = fallback_reason {
            *state.fallback_activations.entry(reason).or_default() += 1;
        }
    }

    pub(super) fn record_report(&self, report: &ReconciliationReport, fills_recovered: usize) {
        let outcome = if report.balances_checked
            && report.mismatches_found == 0
            && report.unresolved_count == 0
        {
            ReconciliationOutcome::Clean
        } else {
            ReconciliationOutcome::Unsafe
        };
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        let source = state.current_source;
        let elapsed = state
            .started_at
            .take()
            .map(|started| started.elapsed().as_secs_f64())
            .unwrap_or_default();
        state.in_progress = false;
        state.durations.entry(source).or_default().observe(elapsed);
        *state.runs.entry((source, outcome)).or_default() += 1;
        if source == "http_fallback" {
            state.fills_recovered_http = state
                .fills_recovered_http
                .saturating_add(fills_recovered as u64);
        }
        state.attempts[outcome.index()] = state.attempts[outcome.index()].saturating_add(1);
        state.last_attempt_timestamp = report.checked_at.timestamp();
        state.mismatches = report.mismatches_found;
        state.unresolved_orders = report.unresolved_count;
        let next_entry_safe = outcome == ReconciliationOutcome::Clean;
        if state.reconciliation_entry_safe != next_entry_safe {
            *state
                .entry_eligibility_changes
                .entry(if next_entry_safe {
                    "enabled"
                } else {
                    "disabled"
                })
                .or_default() += 1;
        }
        state.reconciliation_entry_safe = next_entry_safe;
        if outcome == ReconciliationOutcome::Clean {
            state.last_success_timestamp = report.checked_at.timestamp();
            if let Some(started) = state.recovery_started_at.take() {
                state.last_recovery_duration_seconds = started.elapsed().as_secs_f64();
            }
        } else {
            state.recovery_started_at.get_or_insert_with(Instant::now);
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
        let source = state.current_source;
        let elapsed = state
            .started_at
            .take()
            .map(|started| started.elapsed().as_secs_f64())
            .unwrap_or_default();
        state.in_progress = false;
        state.durations.entry(source).or_default().observe(elapsed);
        *state.runs.entry((source, outcome)).or_default() += 1;
        state.recovery_started_at.get_or_insert_with(Instant::now);
        state.attempts[outcome.index()] = state.attempts[outcome.index()].saturating_add(1);
        *state.failures.entry((failure_stage, class)).or_default() += 1;
        state.last_attempt_timestamp = now;
        if !transient {
            if state.reconciliation_entry_safe {
                *state
                    .entry_eligibility_changes
                    .entry("disabled")
                    .or_default() += 1;
            }
            state.reconciliation_entry_safe = false;
        }
    }

    pub(super) fn set_entry_safe(&self, entry_safe: bool) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if state.reconciliation_entry_safe != entry_safe {
            *state
                .entry_eligibility_changes
                .entry(if entry_safe { "enabled" } else { "disabled" })
                .or_default() += 1;
        }
        state.reconciliation_entry_safe = entry_safe;
    }

    pub(super) fn record_clob_operation(
        &self,
        stage: &'static str,
        elapsed: std::time::Duration,
        timed_out: bool,
    ) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state
            .clob_operation_durations
            .entry(stage)
            .or_default()
            .observe(elapsed.as_secs_f64());
        if timed_out {
            *state.clob_operation_timeouts.entry(stage).or_default() += 1;
        }
    }

    pub(super) fn record_submit_guard_wait(&self, elapsed: std::time::Duration) {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .submit_guard_wait
            .observe(elapsed.as_secs_f64());
    }

    pub(super) fn record_transport_runtime_outcome(&self, outcome: &'static str) {
        *self
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .transport_runtime_outcomes
            .entry(outcome)
            .or_default() += 1;
    }

    pub(super) fn record_submit_unknown_state(
        &self,
        count: usize,
        oldest_age_seconds: f64,
    ) -> bool {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        let resolved = count < state.unresolved_submit_unknown;
        state.unresolved_submit_unknown = count;
        state.oldest_submit_unknown_age_seconds = oldest_age_seconds;
        resolved
    }

    fn record_order_path_started(&self) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.order_path_in_flight = state.order_path_in_flight.saturating_add(1);
        state.order_path_last_activity_timestamp = chrono::Utc::now().timestamp();
        *state
            .order_path_events
            .entry(("submission", "started", "qualified_handoff"))
            .or_default() += 1;
    }

    fn record_order_path_event(
        &self,
        stage: &'static str,
        outcome: &'static str,
        reason: &'static str,
    ) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.order_path_last_activity_timestamp = chrono::Utc::now().timestamp();
        *state
            .order_path_events
            .entry((stage, outcome, reason))
            .or_default() += 1;
    }

    fn record_order_path_duration(&self, stage: &'static str, seconds: f64) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state
            .order_path_durations
            .entry(stage)
            .or_default()
            .observe(seconds);
    }

    fn record_order_path_terminal(&self, outcome: &'static str, reason: &'static str) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.order_path_in_flight = state.order_path_in_flight.saturating_sub(1);
        state.order_path_last_activity_timestamp = chrono::Utc::now().timestamp();
        *state
            .order_path_events
            .entry(("terminal", outcome, reason))
            .or_default() += 1;
        *state
            .order_path_terminal_outcomes
            .entry((outcome, reason))
            .or_default() += 1;
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
        for ((source, outcome), count) in &state.runs {
            let _ = writeln!(output, "polymarket_live_reconciliation_runs_total{{{labels},source=\"{source}\",outcome=\"{}\"}} {count}", outcome.as_str());
        }
        for (source, histogram) in &state.durations {
            for (index, bound) in DURATION_BUCKETS.iter().enumerate() {
                let _ = writeln!(output, "polymarket_live_reconciliation_duration_seconds_bucket{{{labels},source=\"{source}\",le=\"{bound}\"}} {}", histogram.buckets[index]);
            }
            let _ = writeln!(output, "polymarket_live_reconciliation_duration_seconds_bucket{{{labels},source=\"{source}\",le=\"+Inf\"}} {}", histogram.count);
            let _ = writeln!(output, "polymarket_live_reconciliation_duration_seconds_sum{{{labels},source=\"{source}\"}} {}", histogram.sum);
            let _ = writeln!(output, "polymarket_live_reconciliation_duration_seconds_count{{{labels},source=\"{source}\"}} {}", histogram.count);
        }
        for (stage, histogram) in &state.execution_latencies {
            for (index, bound) in DURATION_BUCKETS.iter().enumerate() {
                let _ = writeln!(output, "polymarket_live_execution_latency_seconds_bucket{{{labels},stage=\"{stage}\",le=\"{bound}\"}} {}", histogram.buckets[index]);
            }
            let _ = writeln!(output, "polymarket_live_execution_latency_seconds_bucket{{{labels},stage=\"{stage}\",le=\"+Inf\"}} {}", histogram.count);
            let _ = writeln!(
                output,
                "polymarket_live_execution_latency_seconds_sum{{{labels},stage=\"{stage}\"}} {}",
                histogram.sum
            );
            let _ = writeln!(
                output,
                "polymarket_live_execution_latency_seconds_count{{{labels},stage=\"{stage}\"}} {}",
                histogram.count
            );
        }
        for (outcome, count) in &state.execution_outcomes {
            let _ = writeln!(output, "polymarket_live_execution_outcomes_total{{{labels},outcome=\"{outcome}\"}} {count}");
        }
        for ((stage, outcome, reason), count) in &state.order_path_events {
            let _ = writeln!(output, "polymarket_live_order_path_events_total{{{labels},stage=\"{stage}\",outcome=\"{outcome}\",reason=\"{reason}\"}} {count}");
        }
        for (stage, histogram) in &state.order_path_durations {
            for (index, bound) in DURATION_BUCKETS.iter().enumerate() {
                let _ = writeln!(output, "polymarket_live_order_path_stage_duration_seconds_bucket{{{labels},stage=\"{stage}\",le=\"{bound}\"}} {}", histogram.buckets[index]);
            }
            let _ = writeln!(output, "polymarket_live_order_path_stage_duration_seconds_bucket{{{labels},stage=\"{stage}\",le=\"+Inf\"}} {}", histogram.count);
            let _ = writeln!(output, "polymarket_live_order_path_stage_duration_seconds_sum{{{labels},stage=\"{stage}\"}} {}", histogram.sum);
            let _ = writeln!(output, "polymarket_live_order_path_stage_duration_seconds_count{{{labels},stage=\"{stage}\"}} {}", histogram.count);
        }
        for ((outcome, reason), count) in &state.order_path_terminal_outcomes {
            let _ = writeln!(output, "polymarket_live_order_path_terminal_outcomes_total{{{labels},outcome=\"{outcome}\",reason=\"{reason}\"}} {count}");
        }
        for (stage, histogram) in &state.clob_operation_durations {
            for (index, bound) in DURATION_BUCKETS.iter().enumerate() {
                let _ = writeln!(output, "polymarket_live_clob_operation_duration_seconds_bucket{{{labels},stage=\"{stage}\",le=\"{bound}\"}} {}", histogram.buckets[index]);
            }
            let _ = writeln!(output, "polymarket_live_clob_operation_duration_seconds_bucket{{{labels},stage=\"{stage}\",le=\"+Inf\"}} {}", histogram.count);
            let _ = writeln!(output, "polymarket_live_clob_operation_duration_seconds_sum{{{labels},stage=\"{stage}\"}} {}", histogram.sum);
            let _ = writeln!(output, "polymarket_live_clob_operation_duration_seconds_count{{{labels},stage=\"{stage}\"}} {}", histogram.count);
        }
        for (stage, count) in &state.clob_operation_timeouts {
            let _ = writeln!(output, "polymarket_live_clob_operation_timeouts_total{{{labels},stage=\"{stage}\"}} {count}");
        }
        for (index, bound) in DURATION_BUCKETS.iter().enumerate() {
            let _ = writeln!(output, "polymarket_live_submission_guard_wait_seconds_bucket{{{labels},le=\"{bound}\"}} {}", state.submit_guard_wait.buckets[index]);
        }
        let _ = writeln!(
            output,
            "polymarket_live_submission_guard_wait_seconds_bucket{{{labels},le=\"+Inf\"}} {}",
            state.submit_guard_wait.count
        );
        let _ = writeln!(
            output,
            "polymarket_live_submission_guard_wait_seconds_sum{{{labels}}} {}",
            state.submit_guard_wait.sum
        );
        let _ = writeln!(
            output,
            "polymarket_live_submission_guard_wait_seconds_count{{{labels}}} {}",
            state.submit_guard_wait.count
        );
        for (outcome, count) in &state.transport_runtime_outcomes {
            let _ = writeln!(output, "polymarket_live_transport_runtime_outcomes_total{{{labels},outcome=\"{outcome}\"}} {count}");
        }
        let _ = writeln!(
            output,
            "polymarket_live_submit_unknown_orders{{{labels}}} {}",
            state.unresolved_submit_unknown
        );
        let _ = writeln!(
            output,
            "polymarket_live_submit_unknown_oldest_age_seconds{{{labels}}} {}",
            state.oldest_submit_unknown_age_seconds
        );
        let _ = writeln!(
            output,
            "polymarket_live_order_path_in_flight{{{labels}}} {}",
            state.order_path_in_flight
        );
        let _ = writeln!(
            output,
            "polymarket_live_order_path_last_activity_timestamp_seconds{{{labels}}} {}",
            state.order_path_last_activity_timestamp
        );
        for (eligibility, count) in &state.entry_eligibility_changes {
            let _ = writeln!(output, "polymarket_live_reconciliation_entry_eligibility_changes_total{{{labels},eligibility=\"{eligibility}\"}} {count}");
        }
        for (reason, count) in &state.fallback_activations {
            let _ = writeln!(output, "polymarket_live_reconciliation_fallback_activations_total{{{labels},reason=\"{reason}\"}} {count}");
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
            ("in_progress", i64::from(state.in_progress)),
            (
                "fills_recovered_http_total",
                state.fills_recovered_http as i64,
            ),
        ] {
            let _ = writeln!(
                output,
                "polymarket_live_reconciliation_{metric}{{{labels}}} {value}"
            );
        }
        let _ = writeln!(
            output,
            "polymarket_live_reconciliation_last_recovery_duration_seconds{{{labels}}} {}",
            state.last_recovery_duration_seconds
        );
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
        ("in_progress", "Whether process-scoped reconciliation is currently in progress.", "gauge"),
        ("runs_total", "Reconciliation runs by bounded source and outcome.", "counter"),
        ("duration_seconds", "Reconciliation latency by bounded source.", "histogram"),
        ("fallback_activations_total", "HTTP fallback activations by bounded reason.", "counter"),
        ("fills_recovered_http_total", "Fills recovered by HTTP reconciliation after websocket delivery was uncertain.", "counter"),
        ("last_recovery_duration_seconds", "Duration of the latest reconciliation recovery from degraded to clean.", "gauge"),
        ("entry_eligibility_changes_total", "Entry eligibility changes caused by reconciliation evidence.", "counter"),
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
    output.push_str("# HELP polymarket_live_user_ws_events_total Authenticated user websocket events processed after bounded dequeue.\n# TYPE polymarket_live_user_ws_events_total counter\n");
    output.push_str("# HELP polymarket_live_execution_latency_seconds Live acknowledgement-to-fill latency by bounded stage.\n# TYPE polymarket_live_execution_latency_seconds histogram\n");
    output.push_str("# HELP polymarket_live_execution_outcomes_total Terminal live execution outcomes established by venue evidence.\n# TYPE polymarket_live_execution_outcomes_total counter\n");
    output.push_str("# HELP polymarket_live_order_path_events_total Bounded transitions within the process-scoped live order path.\n# TYPE polymarket_live_order_path_events_total counter\n");
    output.push_str("# HELP polymarket_live_order_path_stage_duration_seconds Internal live order path latency by bounded stage.\n# TYPE polymarket_live_order_path_stage_duration_seconds histogram\n");
    output.push_str("# HELP polymarket_live_order_path_terminal_outcomes_total Exactly one terminal classification for each live order path invocation.\n# TYPE polymarket_live_order_path_terminal_outcomes_total counter\n");
    output.push_str("# HELP polymarket_live_order_path_in_flight Live order path invocations that have not reached a terminal classification.\n# TYPE polymarket_live_order_path_in_flight gauge\n");
    output.push_str("# HELP polymarket_live_order_path_last_activity_timestamp_seconds Unix timestamp of the latest live order path transition.\n# TYPE polymarket_live_order_path_last_activity_timestamp_seconds gauge\n");
    output.push_str("# HELP polymarket_live_clob_operation_duration_seconds Guarded CLOB operation duration by bounded stage.\n# TYPE polymarket_live_clob_operation_duration_seconds histogram\n");
    output.push_str("# HELP polymarket_live_clob_operation_timeouts_total Guarded CLOB operation deadline expirations by bounded stage.\n# TYPE polymarket_live_clob_operation_timeouts_total counter\n");
    output.push_str("# HELP polymarket_live_submission_guard_wait_seconds Time spent waiting for process-safe live submission ownership.\n# TYPE polymarket_live_submission_guard_wait_seconds histogram\n");
    output.push_str("# HELP polymarket_live_transport_runtime_outcomes_total Runtime disposition after a live transport-class submission failure.\n# TYPE polymarket_live_transport_runtime_outcomes_total counter\n");
    output.push_str("# HELP polymarket_live_submit_unknown_orders Durable unresolved submissions whose POST outcome remains unknown.\n# TYPE polymarket_live_submit_unknown_orders gauge\n");
    output.push_str("# HELP polymarket_live_submit_unknown_oldest_age_seconds Age of the oldest durable unresolved submission.\n# TYPE polymarket_live_submit_unknown_oldest_age_seconds gauge\n");
    let _ = writeln!(
        output,
        "polymarket_live_user_ws_events_total {}",
        USER_WS_EVENTS.load(Relaxed)
    );
    let _ = writeln!(
        output,
        "polymarket_live_reconciliation_runs_total{{process_id=\"shared_account\",source=\"user_ws_event\",outcome=\"applied\"}} {}",
        USER_WS_EVENTS.load(Relaxed)
    );
    output.push_str("# HELP polymarket_live_user_ws_fill_events_total Authenticated user websocket events that durably advanced a bot fill.\n# TYPE polymarket_live_user_ws_fill_events_total counter\n");
    let _ = writeln!(
        output,
        "polymarket_live_user_ws_fill_events_total {}",
        USER_WS_FILL_EVENTS.load(Relaxed)
    );
    output.push_str("# HELP polymarket_live_user_ws_reconnects_total Authenticated user websocket reconnect cycles.\n# TYPE polymarket_live_user_ws_reconnects_total counter\n");
    let _ = writeln!(
        output,
        "polymarket_live_user_ws_reconnects_total {}",
        USER_WS_RECONNECTS.load(Relaxed)
    );
    output.push_str("# HELP polymarket_live_user_ws_queue_overflows_total Authenticated user websocket bounded event queue overflows.\n# TYPE polymarket_live_user_ws_queue_overflows_total counter\n");
    let _ = writeln!(
        output,
        "polymarket_live_user_ws_queue_overflows_total {}",
        USER_WS_QUEUE_OVERFLOWS.load(Relaxed)
    );
    output.push_str("# HELP polymarket_live_user_ws_last_event_timestamp_seconds Unix timestamp of the latest processed authenticated user websocket event.\n# TYPE polymarket_live_user_ws_last_event_timestamp_seconds gauge\n");
    let _ = writeln!(
        output,
        "polymarket_live_user_ws_last_event_timestamp_seconds {}",
        USER_WS_LAST_EVENT.load(Relaxed)
    );
    output.push_str("# HELP polymarket_live_user_ws_last_pong_timestamp_seconds Unix timestamp of the latest authenticated user websocket heartbeat acknowledgement.\n# TYPE polymarket_live_user_ws_last_pong_timestamp_seconds gauge\n");
    let _ = writeln!(
        output,
        "polymarket_live_user_ws_last_pong_timestamp_seconds {}",
        USER_WS_LAST_PONG.load(Relaxed)
    );
    output
}

pub(super) struct LiveOrderPathAttempt {
    metrics: Option<Arc<LiveReconciliationMetrics>>,
    process_id: Uuid,
    client_order_id: Uuid,
    market_id: String,
    started_at: Instant,
    terminal: bool,
}

impl LiveOrderPathAttempt {
    pub(super) fn new(
        metrics: Option<Arc<LiveReconciliationMetrics>>,
        process_id: Uuid,
        client_order_id: Uuid,
        market_id: &str,
    ) -> Self {
        if let Some(metrics) = &metrics {
            metrics.record_order_path_started();
        }
        Self {
            metrics,
            process_id,
            client_order_id,
            market_id: market_id.to_string(),
            started_at: Instant::now(),
            terminal: false,
        }
    }

    pub(super) fn stage(
        &self,
        stage: &'static str,
        outcome: &'static str,
        reason: &'static str,
        elapsed: std::time::Duration,
    ) {
        if let Some(metrics) = &self.metrics {
            metrics.record_order_path_event(stage, outcome, reason);
            metrics.record_order_path_duration(stage, elapsed.as_secs_f64());
        }
    }

    pub(super) fn event(&self, stage: &'static str, outcome: &'static str, reason: &'static str) {
        if let Some(metrics) = &self.metrics {
            metrics.record_order_path_event(stage, outcome, reason);
        }
    }

    pub(super) fn finish(mut self, outcome: &'static str, reason: &'static str) {
        if let Some(metrics) = &self.metrics {
            metrics.record_order_path_duration(
                "submission_total",
                self.started_at.elapsed().as_secs_f64(),
            );
            metrics.record_order_path_terminal(outcome, reason);
        }
        tracing::info!(
            event = "live_order_path_terminal",
            process_id = %self.process_id,
            client_order_id = %self.client_order_id,
            market_id = %self.market_id,
            outcome,
            reason,
            elapsed_seconds = self.started_at.elapsed().as_secs_f64(),
            "live order path reached a terminal outcome"
        );
        self.terminal = true;
    }
}

impl Drop for LiveOrderPathAttempt {
    fn drop(&mut self) {
        if self.terminal {
            return;
        }
        if let Some(metrics) = &self.metrics {
            metrics.record_order_path_duration(
                "submission_total",
                self.started_at.elapsed().as_secs_f64(),
            );
            metrics.record_order_path_terminal("error", "unclassified_exit");
        }
        tracing::warn!(
            event = "live_order_path_terminal",
            process_id = %self.process_id,
            client_order_id = %self.client_order_id,
            market_id = %self.market_id,
            outcome = "error",
            reason = "unclassified_exit",
            elapsed_seconds = self.started_at.elapsed().as_secs_f64(),
            "live order path exited without an explicit terminal classification"
        );
    }
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
        metrics.record_started("http_periodic", None);
        metrics.record_report(
            &ReconciliationReport {
                open_orders: 0,
                balances_checked: true,
                mismatches_found: 0,
                unresolved_count: 0,
                checked_at: Utc::now(),
            },
            0,
        );
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
        metrics.record_started("http_periodic", None);
        metrics.record_report(
            &ReconciliationReport {
                open_orders: 1,
                balances_checked: true,
                mismatches_found: 1,
                unresolved_count: 1,
                checked_at: Utc::now(),
            },
            0,
        );
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

    #[test]
    fn live_order_path_exports_bounded_stages_and_one_terminal_outcome() {
        let process_id = Uuid::new_v4();
        let metrics = LiveReconciliationMetrics::new(process_id);
        let attempt = LiveOrderPathAttempt::new(
            Some(metrics.clone()),
            process_id,
            Uuid::new_v4(),
            "market-1",
        );
        attempt.stage(
            "risk_and_metadata",
            "allowed",
            "allowed",
            std::time::Duration::from_millis(12),
        );
        attempt.finish("acknowledged", "venue_acknowledged");

        let rendered = prometheus_metrics();
        assert!(rendered.contains(&format!(
            "polymarket_live_order_path_events_total{{process_id=\"{process_id}\",stage=\"risk_and_metadata\",outcome=\"allowed\",reason=\"allowed\"}} 1"
        )));
        assert!(rendered.contains(&format!(
            "polymarket_live_order_path_terminal_outcomes_total{{process_id=\"{process_id}\",outcome=\"acknowledged\",reason=\"venue_acknowledged\"}} 1"
        )));
        assert!(rendered.contains(&format!(
            "polymarket_live_order_path_in_flight{{process_id=\"{process_id}\"}} 0"
        )));
        assert!(!rendered.contains("client_order_id="));
        assert!(!rendered.contains("market_id="));
    }

    #[test]
    fn dropped_live_order_path_is_visible_as_unclassified() {
        let process_id = Uuid::new_v4();
        let metrics = LiveReconciliationMetrics::new(process_id);
        {
            let _attempt = LiveOrderPathAttempt::new(
                Some(metrics.clone()),
                process_id,
                Uuid::new_v4(),
                "market-2",
            );
        }

        let rendered = prometheus_metrics();
        assert!(rendered.contains(&format!(
            "polymarket_live_order_path_terminal_outcomes_total{{process_id=\"{process_id}\",outcome=\"error\",reason=\"unclassified_exit\"}} 1"
        )));
        assert!(rendered.contains(&format!(
            "polymarket_live_order_path_in_flight{{process_id=\"{process_id}\"}} 0"
        )));
    }
}
