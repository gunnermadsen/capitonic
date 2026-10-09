# Execution instrumentation contract

Specification only in this foundation; no metrics, dashboards, alerts, Alloy or Helm assets are changed or provisioned.

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
