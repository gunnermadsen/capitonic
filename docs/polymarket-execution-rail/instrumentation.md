# Execution instrumentation contract

The execution core extends the existing `ExecutionFreshnessMetrics` owner and structured JSON logs. The v3.4.0-local.2 source includes the Order Execution dashboard and scoped rules in the existing execution-freshness provisioning file. They have not been provisioned to a running environment. No Alloy, deployment tooling, Helm files or chart versions change.

## Existing owners to extend

| Owner | Existing responsibility | Adoption requirement |
| --- | --- | --- |
| btc/unified_model_runtime/telemetry.rs | polymarket_umr_* model/feature/admission/decision/execution/fill/session economics | Preserve semantics; synthetic API orders must not manufacture model inferences or prediction quality. |
| btc/execution_freshness.rs | polymarket_execution_freshness_* with process_id/execution_mode | Reuse readiness and freshness observations; no independent feed monitor. |
| execution/live metrics and Store fill-read metrics | Transport/reconciliation and fill evidence | Reuse event points; do not count a live POST as the equivalent of a paper fill. |
| Existing structured bot logs → Alloy → Loki | Lifecycle transitions and errors | Correlate existing identifiers; bounded logging outside socket intake. |
| common/configs/grafana/dashboards/unified-model-runtime.json | UMR funnel and session outcomes | Preserve existing model measurements; add appropriate execution-origin visibility later. |
| common/configs/grafana/dashboards/live-trading-health.json | Live process/account/reconciliation health | Retain existing config.execution.mode reader; extend shared-path visibility without hiding live-only diagnostics. |
| common/configs/grafana/dashboards/trading-pnl-market-metrics.json | Durable trading economics | Verify provisioned queries before changes; include all real economic effects, allow origin filtering. |

The previous investigation identified global aggregation masking individual reconciliation failure and a possible Loki error-code extraction/query mismatch. Those are evidence to verify during observability implementation, not permission for unrelated repairs in the configuration foundation. Recheck deployed provisioning when that work begins.

## Shared measurements

Define each event once: candidate received, policy applied/rejected/not-applicable, execution intent accepted, submission attempt, acknowledgement, actual fill, reconciliation outcome, settlement credit and recovery. Units, denominator and retry semantics must be documented. Scope shared series by process_id, execution_mode and bounded origin (umr/api). Add bounded rail/contract information only where useful; avoid a label cross-product on every existing family.

Keep request/intent/order/fill IDs and configuration/model hashes in structured logs and existing durable metadata, not unbounded metric or Loki stream labels. Prometheus counters are session evidence subject to restart; PostgreSQL remains economic truth. Reuse existing tables and datasource definitions.

API orders participate in balances, reservations, fees, exposure and realized accounting. They are distinguishable from UMR strategy performance. Paper primary and stress outcomes must be distinguishable. Missing outcomes are not zeros. A policy result must expose the effective owner (artifact, process policy, venue requirement), especially when an artifact imposes a stricter bound than the TPC.

## Acceptance

Exercise successful, rejected, partial-fill, unresolved and recovered operations and trace each end to end. Validate metric labels against dashboard PromQL and actual Loki extraction. Alerts are scoped so another healthy process cannot mask failure; telemetry outages cannot change durable trading authority. Provision all changes through existing sources and deployment provenance. Remove duplicates only after consumers migrate. No high-frequency work is introduced into websocket intake.

## Execution core event boundaries

The core emits `order_intent_admitted`, `order_pre_submit_rejected`, `order_submission_attempt`, `order_submission_result`, `order_execution_observed` and `order_recovery_deferred`. Order events carry process ownership and bounded mode/origin fields where a candidate exists; correlation IDs and hashes remain log fields. Reconciliation operates on orders from potentially different origins, so its process-level retry event does not invent an origin.

Live freshness is observed through the existing adjacent pre-POST hooks. Paper freshness uses the arrival simulator's actual checkpoint. API candidates do not register a fake UMR model, inference, or prediction-quality event. Existing settlement telemetry remains owned by the shared settlement lifecycle. Dashboard/API-versus-strategy interpretation still requires the planned instrumentation integration before process cutover.

## Implemented shared measurements

- `polymarket_order_stage_events_total{process_id,execution_mode,origin,stage,outcome}` counts observations at candidate, preview, preparation, admission, dispatch, observation and replay boundaries. Origin is `api` or `umr`; stages/outcomes are bounded enums. Repeated observations/recovery and retries count again. A submission dispatch does not prove POST acknowledgement; an observation does not increment an economic fill ledger. Historical API shortcuts emit `order_api_replay` logs even without a current metrics owner.
- `polymarket_order_recovery_healthy`, `polymarket_order_last_recovery_timestamp_seconds` and `polymarket_order_unresolved_orders` are scoped by process and mode. They describe process-wide recovery across origins. Initialization reports recovery not yet proven; unresolved counts remain absent until measured. Successful recovery automatically clears failure evidence. Metrics do not change authorization.
- Existing freshness, live transport/reconciliation, Store fill identities and durable economics retain their owners. API settlements remain in balances, exposure, fees and the settlement ledger; incremental UMR strategy-settlement metrics and live strategy telemetry hydration exclude explicitly API-origin orders. Historical unmarked orders preserve their original interpretation.
- `order_stage`, `order_recovery`, `order_api_replay`, existing order lifecycle events and settlement logs carry mode/origin where meaningful. Correlation identifiers remain JSON fields. Loki queries extract `fields.process_id`, `fields.execution_mode`, `fields.origin` and `fields.event` at query time, compatible with existing Compose and Kubernetes collection; no high-cardinality stream labels are introduced.

The new dashboard links from the existing UMR, live-health and trading-PnL dashboards. It shows stage events, recovery, freshness, recent durable order/fill/settlement evidence, and logs. Its durable table is bounded to the latest 200 process-owned orders created in the selected time window; fills are also time-windowed. Existing lifetime PnL remains the economic accounting view and already includes API effects through the same order/fill/settlement tables. Null/missing evidence is not converted into an invented economic zero. UMR inference and model-quality views remain separate.

The new alerts retain process/mode labels, and execution errors retain origin. Recovery uses the worst result per process; another healthy process or replica cannot hide it. No-active-process/no-data is normal for these additive series; existing exporter/service alerts own exporter loss. Rules are appended to `rules-execution-freshness.yml`, already copied by the existing provisioning entrypoint, so build/deploy tooling is unchanged.

`tests/order_observability.rb` validates committed query expressions using an existing Prometheus `promtool` through stdin. Eight fixtures cover healthy-peer masking, stale recovery, inactivity, error origin separation and dashboard PromQL syntax. They pass on the local existing Prometheus binary. JSON/YAML syntax, unique alert IDs, nested Loki extraction and existing provisioning ownership are checked. This executes no provisioning operation.

A read-only comparison of current local provisioned dashboards found live health and UMR content matching this branch apart from the new navigation links. Current PnL provisioning has independent differences; they are not overwritten or reconciled here. Actual dashboard rendering, datasource execution and alert delivery for this candidate await an authorized deployment.

## Shared latency measurements

`polymarket_order_stage_duration_seconds{process_id,execution_mode,origin,stage,outcome}` is a histogram in seconds for both paper and live. `polymarket_order_stage_duration_max_seconds` records the largest observation since that process metrics owner was created. Labels are bounded except the existing canonical process identity; request/order identifiers are never metric labels. Recording uses monotonic `Instant` and preallocated atomic bins, with no database writes, network export, logging, allocation or scrape-lock acquisition on the observation path. Scrapes are approximate during concurrent updates. No measurement changes trading authority.

Outcomes describe the timed operation: `completed` means it returned normally, **not** that an order filled; `rejected` means a policy/gate/venue refusal; `error` means failure; `cancelled` means the future/timer was dropped before completion. Existing lifecycle counters and durable records remain the authority for fills, nonexecution and ambiguity.

| Stage | Boundary |
| --- | --- |
| evidence | API candidate construction, including canonical book capture, existing reference/history/fee reads and snapshot construction. UMR feature/inference timings retain their existing owner. |
| preview / preparation | Shared preview or dry-run service evaluation; includes accounting/admission and, for live preparation, signing diagnostics. Excludes earlier candidate construction and HTTP dispatch. |
| execution | Execution service entry through returned receipt/error; includes reservation, adapter and recovery/observation. Excludes earlier candidate construction and controller-only historical replay shortcuts. |
| reservation_wait | Existing connection/semaphore acquisition, transaction creation and account advisory-lock acquisition. Does not measure lock hold time. |
| accounting | Each existing accounting evaluation, including preview and final validation; multiple samples per request are expected. |
| admission | Initial OMS evaluation including book-lock acquisition and snapshot copies; applies to execute and preview/dry run. |
| final_validation | Shared adjacent paper-arrival/live-pre-POST guard, including renewed authority, accounting and book admission. |
| adapter | Adapter execution through return, including paper arrival delay/commit or live preparation/POST/persistence. |
| paper_arrival | Actual awaited simulation delay, including scheduler delay. Configured delay stays in TPC; this stage is absent for live. |
| persistence | Explicit rail order/rejection/resolution writes, atomic paper order/fill commit, and existing live pending-order claim. Multiple samples per request; not an exhaustive timer of all writes inside live reconciliation. Existing live instrumentation retains additional transport/persistence detail. |
| transport | Existing live POST-result callback duration; absent for paper. |
| reconciliation | Live receipt recovery's awaited reconciliation; background process recovery retains its existing metrics. |
| observation | Fill retrieval, lifecycle resolution and resolution persistence for returned/recovered receipts. |

Stages are nested and **must not be summed** into a total. `execution` includes `adapter`; `adapter` includes `paper_arrival` and final validation; final validation includes accounting. HTTP duration is captured independently by the external qualification driver, including response body transfer but excluding the driver's audit/preflight/report writes. Paper configured delay, observed delay, and overall execution must be reported separately. Shared stages must be compared under equivalent policies and outcomes; paper cannot establish live transport latency.

The existing Order Execution dashboard adds shared mean, p95 and owner-lifetime maximum panels. The p95 query requires at least 20 observations in its rate window. That threshold only suppresses sparse output; it is not a statistical confidence guarantee. Dashboard changes are source-validated, not automatically provisioned by a bot deployment. No speculative latency alert threshold is added before establishing a baseline.

Before qualification, measure both modes' instrumentation correctness, then actual paper API execution with real data. Record sample counts, outcome, book age, intentional delay, query/lock-stage costs and timing distributions. Report sparse or blocked results as such. Performance acceptance and a comparable legacy baseline remain unproven until runtime measurements exist; the prior in-memory OMS benchmark is not an end-to-end budget.
