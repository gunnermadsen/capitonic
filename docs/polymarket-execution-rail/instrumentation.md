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
