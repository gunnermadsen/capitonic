# Implementation acceptance and rollout

Frozen scope: additive order contract and Polymarket execution standard, no broader TPC reorganization. Existing rail behavior and its dry-run endpoint remain unchanged until controlled retirement. The approved shared-helper extensions preserve legacy callers and behavior. No ingester changes, new tables/ledger, model/agent trading subsystem, CI/CD/workflow/build-script changes or automatic process conversion. No Helm changes during this implementation; later deployment requires separate scope and repository versioning policy.

## Work grouping

| Package | Plan phases | Deliverable / acceptance boundary |
| --- | --- | --- |
| Contract foundation | 1–2 | Standards, baseline, typed compiler, strict additive API admission; no executable new rail |
| Polymarket execution core | 3–4 | Shared OMS, adapters, lifecycle/accounting/recovery complete as a unit |
| Order API and visibility | 5–6 | Generic preview/dry-run/execute/retrieve endpoints and provisioned visibility |
| Development qualification | 7 | Deterministic, paper E2E and separately authorized live evidence |
| UMR integration and cutover | 8 | Same application entry point, controlled process migration and rollback |
| Legacy retirement | 9 | Remove old paths only after no resumable process or unresolved order depends on them |

Use domain-scoped modules, commits and the owning feature worktree. Do not name artifacts/branches/comments after phase numbers. Tests belong with their owner. Documentation changes accompany contract changes. No speculative future exchange implementation.

## Phase 1 — Standards and baseline

Accept only when parameter and gate ownership/default/applicability mappings are complete; existing readers and account/telemetry dependencies are recorded; candidate→order intent→execution intent→evidence contracts and adapter boundaries are documented; legacy model/agent/paper/live regression fixtures are identified; and performance limits are recorded before implementation qualification. Link this standard from repository instructions and UMR documentation. No duplicated documentation authority.

## Phase 2 — Minimal TPCP support

Accept only when both generations coexist; new definitions require an explicit supported selector/schema; strict compilation rejects duplicate/unknown/missing policies, invalid parameters and competing order fields; existing effective-execution serialization remains stable; exact member order-type mappings and decimal/configuration identity are tested; model/source compatibility reuses UMR; invalid definition updates cannot reach the existing write path; and new definitions cannot fall through to legacy start/resume. Existing PUT/PATCH and Store remain owners. API trigger may be declared but cannot run before implementation. No configuration/database mutation is part of validating the compiler.

## Phase 3 — Shared execution core

Accept only when one application entry serves UMR/API candidates, typed settings enter one OMS, existing checks are reused, quantity precedes dependent pricing/fees/admission, reservations prevent concurrent overspend, rejections submit nothing, adapters cannot bypass admission and legacy fixtures remain unchanged. Instrument event boundaries while building the core. Identical shared inputs require identical results; paper/live adapters supply domain evidence.

## Phase 4 — Lifecycle and recovery

Accept only with deterministic fault evidence for lost submission response, duplicate/reordered events, partial fill then cancellation, persistence failure, stale evidence/reconnect, restart with unresolved orders and settlement replay. No duplicate economic effects, premature finality, re-seeded paper balance or reassigned order ownership. Ambiguity reconciles before retry. Recovery is automatic and process/action-scoped. Shared account ownership includes stopped sleeves with unresolved economic state.

Known drift regressions to cover: FAK depth clipping/rejection difference; missing delegated venue capability hooks; opportunity retained after zero fill; explicit exchange rejection misclassified as ambiguous; cash-budget fills versus share authorization under price improvement; unequal opposite-book checks; paper aggregate/loss checks; and stress-preview clock skew. Verify each against current integration before repair; a refactor is not permission for unrelated behavior changes.

## Phase 5 — Generic controller and dry-run standard

Accept only when controller has no concrete execution dispatch, process config is authoritative, synthetic provenance is explicit, previews/dry runs have no economic effects, preparation diagnostics are redacted, execution revalidates, idempotency/ownership/error semantics pass HTTP tests, a registered test implementation needs no controller change, and legacy dry run remains unchanged. No new public route is implemented by the foundation.

## Phase 6 — Instrumentation

Accept only when current metric/log owners are reused; event meanings and mode/origin labels align; Grafana traces shared and domain-specific stages; all economic effects include API orders while strategy-performance views distinguish them; alerts cannot mask individual failures; queries match actual labels/extraction; changes are provisioned; and telemetry has no effect on trading authority or socket intake. Preserve duplicates until consumers migrate.

## Phase 7 — Development qualification

Dedicated paper/live process identities are created through the existing API only when authorized. Use normal application wiring and equivalent shared policies, fixed sizing first. Prove preview/dry run, paper E2E, at least one explicitly authorized live fill matched to venue evidence, applicable settlement/redemption completion, accounting and restart recovery. Exhaustive external failures/partial fills use deterministic fixtures because a real exchange cannot guarantee those events on demand. No unresolved discrepancy in passed scenarios. Funding authorization names account, budget and run; ordinary CI cannot trade. Shared accounts are not capital-isolated by process IDs.

Dynamic sizing remains unavailable until proposal, precision, invalid/missing-output behavior and fallback are specified and qualified. Do not silently clamp upward or assume changing mode implements it. Qualification reports distinguish deterministic proof, observed live evidence and unverified cases; profitability is not an execution-refactor acceptance criterion.

## Phase 8 — UMR integration and cutover

Accept only with frozen inference/feature/qualification parity, same shared application entry, exact one-owner intent routing, original owner for outstanding orders, unchanged unselected processes and a compatible code+configuration rollback tuple. No startup/bulk conversion. Preserve process_id, history and enabled intent. Each production conversion requires repository release authorization and documented operations. Source acceptance does not authorize build/deploy/merge/push.

## Phase 9 — Retirement

No enabled/resumable process or unresolved order may require the old path. Preserve historical reading and rollback compatibility. Convert through process API; then remove obsolete flat fields, legacy routing, duplicate telemetry and old dry-run endpoint after caller migration. Version necessary compatibility breaks separately. Broader TPC reorganization remains out of scope.

## Version and integration boundaries

Feature starts from integration-2026-10-08 at ca910a01daf6b43de6d6a53daa610b1a1d7babb3, on feature/polymarket-execution-rail. The foundation candidate was v3.4.0-local.0; the execution core candidate is v3.4.0-local.1; the API and instrumentation candidate is v3.4.0-local.2. Neither is a promotion or deployment. Existing 3.3.x model/agent work continues independently. Integrate selected older-version changes only through authorized merges, keeping both contract suites and production image/secret provenance. Rail/OMS contracts start at 1.0.0; selector major 1 resolves a compatible internal implementation. No separate selectable OMS version. Future refactors must preserve ownership and acceptance boundaries.
