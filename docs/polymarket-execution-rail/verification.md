# Execution rail verification

## Acceptance scope

The v3.4.0-local.0 foundation covered frozen-plan phases 1–2 only: standards, mappings, baseline regression and additive TPC admission/compiler. It does not claim a working OMS, new order HTTP routes, paper/live execution parity, funded qualification or deployment. Compiled definitions remain inactive and cannot reach the legacy runner. No database was mutated for verification; API admission is tested through the application validator with a lazy, unconnected Store. Actual database round-trip execution and controller E2E belong to subsequent qualification.

## Regression baseline

Base: integration-2026-10-08, ca910a01daf6b43de6d6a53daa610b1a1d7babb3. Preserve and run the existing component suite, including:

- application tests: agent_v5_preserves_sources_and_rejects_live_and_older_schema; durable_resume_uses_only_explicit_running_live_capital_authorization; paper_and_live_definitions_resolve_only_the_saved_process_quantity; btc_resume_preserves_frozen_parameters_across_service_rebuilds; live_start_cannot_relax_the_two_second_execution_freshness_contract.
- btc/strategy tests: agent_prediction_reuses_directional_entry_and_execution_guards; payoff_models_apply_native_admission_before_directional_execution; dynamic_crypto_fee_matches_golden_formula.
- btc/paper tests: fok_rejects_atomically_when_collateral_cannot_cover_notional_and_fee; delayed_arrival_preview_never_mutates_primary_paper_state; resume_rehydrates_capital_counts_and_settlement_idempotency; process_sized_paper_orders_keep_requested_quantity_and_account_actual_fills.
- btc/live_execution tests: process_sized_live_orders_reach_delegate_unchanged_and_keep_quantity_sealed.
- existing HTTP tests, including legacy live order dry run, unchanged except additive None fields in a Rust fixture.

These existing tests are the baseline gate/decision fixtures; do not create a second model or market simulator solely to reproduce them.

## New acceptance evidence

`tests/polymarket_order_contract.rs` covers shared policy equality across paper/live definitions; deterministic policy/source ordering and decimal normalization; round trips and configuration identity; duplicate/unknown/missing policies; unsupported versions; parameter/ownership strictness; legacy serialization/effective controls; mixed-contract rejection; share bounds; explicitly unavailable dynamic mode; exact member FOK/FAK mappings; mode/account/model authorization; optional existing loss controls; source/artifact compatibility; catalog pinning without partial candidate mutation; and reuse of the existing agent contract.

Application test `new_order_contract_is_admitted_only_for_storage_and_never_legacy_execution` uses existing manager/Store construction without a connection. Both examples pass storage validation; explicit start and durable resume reject before a runner is created. Invalid candidate policies reject before API persistence. PUT/PATCH keep their existing lifecycle locks and write functions. No order routes or runtime machinery are added.

## Performance acceptance

Before qualification: no new work on the legacy inference/execution/socket hot paths; no new database reads or network calls inside configuration compilation; policy parsing occurs at definition admission, not per tick. New compilation is bounded by configured policies/sources/members and reuses cached immutable UMR models.

Host acceptance for a warmed single-member example: median <25ms, p95 <100ms across 100 compilations. Cold model loading is separately excluded, never represented as warmed performance. The explicit ignored timing test is run locally; CI correctness does not depend on host load. Actual order-path latency/memory budgets must be measured against unchanged execution before the core implementation is qualified, not claimed by this compiler.

## Commands

```sh
cargo +1.92.0 test --locked --package polymarket-bot --test polymarket_order_contract
cargo +1.92.0 test --locked --package polymarket-bot --test polymarket_order_contract warmed_configuration -- --ignored --nocapture
scripts/local-image-ci.sh polymarket-bot 3.4.0-local.0
```

The final command is authorized local check/build only, requires a clean committed source and the repository's version allocation provenance. It does not deploy or push. Do not modify its implementation, CI workflows, Dockerfiles, Helm pins, or ingester code to make this feature pass.

## Result record

Before local image CI: 17 focused contract tests passed; the explicit application storage/start/resume isolation test passed; the explicit timing test passed with warmed median 282.708µs and p95 311.042µs (100 runs, local debug build). One timing test is intentionally ignored by normal CI and was invoked separately.

The unchanged `scripts/local-image-ci.sh polymarket-bot 3.4.0-local.0` completed successfully on 2026-10-08:

- Script tests: 8 passed; model-packaging tests: 9 passed.
- Rust formatting and Clippy (`--all-targets`, warnings denied): passed.
- Full component tests: 516 passed, 0 failed, 4 ignored. Three ignored tests were pre-existing manual/SSD-dependent checks; the fourth is the new timing check, executed separately above. Existing model/agent/paper/live and legacy HTTP dry-run tests passed.
- Candidate image: `capitonic/polymarket-bot:v3.4.0-local.0`.
- Image ID: `sha256:9c30a979842e4fa37e77bcade7c64bceb185cafbefcf5c81d5a4a07a5303494e`.
- Image source: `f0850faa9ddbb122839f6afab3ba3baf83978650`.
- CI created local annotated image/version and image/hash provenance tags. The local `ci/polymarket-bot/v3.4.0-test.0` tag selects the requested SemVer base; its annotation explicitly disclaims a remote CI result or image.
- Origin version refs and ECR final tags were read before allocation; no v3.4.0 allocation existed. Main integration base stayed unchanged.
- Scope check: no modifications under `.github`, `scripts`, `common`, `capitonic-helm-chart`, `packages/market-data-ingester`, or root Cargo manifests/lockfile.

This result record is a documentation-only follow-up to the image source; it does not change image inputs or mint another image. The worktree is retained for subsequent rail work. No order-execution qualification, production mutation, deployment, branch integration or remote push is claimed.

## Execution core acceptance evidence

The v3.4.0-local.1 scope implements the shared OMS, application entry, domain adapters and durable lifecycle library. It preserves the foundation's inactive process-routing boundary. The user explicitly authorized narrow extensions in existing gate, paper and Store owners; no legacy runtime is routed through the new service.

- New OMS/lifecycle tests: 15 passed. Coverage includes shared paper/live admission, quantity-dependent pricing, limit-specific participation, funds/exposure/position/daily-loss checks, stale/future/identity evidence, automatic next-action recovery, configuration identity, duplicate/conflicting/reordered fill evidence, nonfinal acknowledgements, explicit rejection, partial FAK cancellation, cash-budget price improvement and fee reservation parity with durable exposure replay.
- New durable paper tests: 5 passed. They call the actual arrival simulator and inject a precommit failure, lost commit response and transient arrival rejection; they verify durable-capital authority and cash-budget versus unchanged legacy share sizing.
- Manual warmed admission timing: median 250.166µs, p95 270.541µs across 100 local debug calls. The check excludes database/transport IO and model loading; it is not an end-to-end latency claim. It is intentionally ignored in ordinary CI.
- Existing live fixtures retained: ambiguous POST is not retried; uncertain ownership remains unresolved; REST recovers missed and cancelled-FAK fills; price-improved BUY accounting uses authorized cash; sealed dynamic fees are reused. Existing paper settlement/restart and legacy HTTP regression tests remain in full local CI.

The narrow shared extensions expose existing validation helpers, factor fill insertion into a caller-owned transaction, and factor paper settlement processing with an optional legacy memory-credit owner. No second order/fill/settlement ledger, database schema, ingester, agent/model execution system or release pathway is added.

Database caveat: fault injection verifies the paper commit boundary and lifecycle logic, not a running PostgreSQL transaction. The new SQL and complete service recovery wiring have not been exercised against a database here. Real database concurrency, API E2E and funded execution remain development qualification requirements. No live trade or database mutation was performed during this work.

The unchanged `scripts/local-image-ci.sh polymarket-bot 3.4.0-local.1` completed successfully on 2026-10-08:

- Python tooling tests: 17 passed (8 local-CI tests and 9 model-packaging tests).
- Rust formatting and Clippy (`--all-targets`, warnings denied): passed.
- Full component tests: 536 passed, 0 failed, 5 ignored. The additional ignored test is the explicit OMS timing check, run separately above. Existing model/agent/paper/live and legacy HTTP regressions passed.
- Candidate image: `capitonic/polymarket-bot:v3.4.0-local.1`.
- Image ID: `sha256:ce10da77359ca08b8991dd18448f827ecb57c3a1f970cde6b273841251ecf7f0`.
- Image source: `0eca147734ab33fef10469b7201e01a1bfb08622`.
- Image input hash: `d9f11491a813ae06b7a608110b654130fdb3551c07102059a8aee1211cd59a9f`.
- Local annotated provenance: `image/polymarket-bot/v3.4.0-local.1` and `image/polymarket-bot/sha256-ce10da77359ca08b8991dd18448f827ecb57c3a1f970cde6b273841251ecf7f0`.
- Version preflight checked origin tags, refreshed the production reference (bot v3.2.9-rc.0) and read ECR final tags; no conflicting v3.4.0 allocation existed.
- Scope check: no changes to charts/Helm versions, CI/CD, GitHub workflows, build/deploy tooling, ingesters, model/agent runtimes, database schema or root Cargo manifests/lockfile. Existing application routing remains inactive for the new definition.

This verification record is a documentation-only follow-up to the image source. No deployment, process activation, database mutation, branch integration, registry publication or remote push occurred. Database-backed and funded qualification remain unverified as described above.


## Order API and instrumentation acceptance evidence

Candidate v3.4.0-local.2 implements the frozen plan's generic controller and instrumentation source. API-triggered definitions now use the existing process manager and shared feeds; UMR cutover remains unavailable. Existing legacy process behavior and dry-run routes are retained.

Focused evidence before full CI:

- Three authenticated HTTP tests cover all new routes, strict envelopes and override rejection, response/error mapping, unavailable preparation, and ownership-safe lookup using a registered deterministic application.
- OMS tests cover actual policy reporting, original-owner/request replay despite a changed configuration, and distinct process/mode/origin metrics. API-specific boundary tests cover signing diagnostic redaction and a rejected record that cannot be mistaken for an approved execution intent.
- Application configuration testing verifies paper/live policy projection, immutable run identity across resume and rejection of unactivated UMR routing.
- Eight Prometheus query fixtures pass against the existing local Prometheus binary without provisioning. Provisioning YAML and dashboard JSON parse; alert IDs are unique; the existing provisioning entrypoint already includes the extended rule file.
- Clippy passes with warnings denied. No production state, local process configuration, database, or running observability configuration was mutated.

Qualification limits: these tests do not execute the new controller against a real PostgreSQL database, place a paper or funded live order through a running candidate, or prove restart recovery end to end. The new dashboard/alert sources are not deployed. Those runtime checks remain the dedicated development qualification boundary. No database-backed or funded parity claim is made by a successful image build.

The unchanged `scripts/local-image-ci.sh polymarket-bot 3.4.0-local.2` completed successfully on 2026-10-08:

- Full Rust component suite: **545 passed, 0 failed, 5 ignored**. The unchanged manual/SSD-dependent exclusions remain; no new ignored correctness test was added.
- Python tooling suite: **17 passed** (8 local-CI tests, 9 model-packaging tests).
- Formatting and Clippy (`--all-targets`, warnings denied): passed.
- Separate committed observability regression: **8 query fixtures passed**, using `ruby packages/polymarket-bot/tests/order_observability.rb kubectl --context rancher-desktop -n capitonic exec -i prometheus-0 -- promtool`. Input was provided through stdin; no live configuration changed.
- Candidate image: `capitonic/polymarket-bot:v3.4.0-local.2`.
- Image ID: `sha256:65b3df405de1d509e1cc5059836d4c385d2f0ca9586ff4b73413b2a60e2cfef2`.
- Image source: `5f6b9b13ffe800d79b11525078d9dfe69ff8cd8d`.
- Image input hash: `e2cd7d258d20d18e5e8e5fc044abb967419253c2d5885f5c5c59fc97977d2710`.
- Local annotated provenance: `image/polymarket-bot/v3.4.0-local.2` and `image/polymarket-bot/sha256-65b3df405de1d509e1cc5059836d4c385d2f0ca9586ff4b73413b2a60e2cfef2`.
- Origin tag, refreshed production-pin and ECR tag reads found no conflicting v3.4.0 allocation. Production remained pinned to bot v3.2.9-rc.0; no production action was performed.
- Scope verification found no edits to `.github`, `scripts`, `common/scripts`, `capitonic-helm-chart`, `packages/market-data-ingester`, or root Cargo manifests/lockfile. No chart version changed. New observability source uses the existing provisioning owners.

This result record is a documentation-only follow-up to the image source. No deployment, process activation, database mutation, observability provisioning, integration merge, registry publication or remote push occurred. Source/fixture acceptance does not replace the runtime qualification limits above.

## Development API qualification — 2026-10-09

Tested the deployed combined source `43f896c7dd6f5300b501e66cfbc081c2ec93ebe4`, image `v3.4.0-local.3`, using the real local ingress, process manager and database. The external driver lives in `packages/polymarket-bot/tests/order_api/`; no test mode, substitute venue, ledger, schema or configuration contract was added to runtime machinery.

- Dedicated paper process: `fff03502-3e34-416e-a8fc-7765b837e05e`.
- Dedicated live preparation process: `99a8c962-33d1-43be-8dee-78b600145bd8`.
- Both use `btc_realtime_paper_process_v6`, `runtime.trigger=api`, fixed five-share sizing, FAK, and matching shared `order_policies` from the maintained examples. Run identifiers are unique. Live execution was never requested.
- 57 assertions passed over actual API/storage checks: definition admission, duplicate/unknown/mixed/unsupported policy rejection, invalid-update immutability, start-preview/start, inactive-process rejection, preview/dry-run/detail authentication, forbidden overrides, malformed requests, unavailable members/markets, unknown order lookup, and unchanged economic records.
- Both processes were stopped through the existing API after the blocker was identified. Existing nine enabled processes retained their configurations and running state. Both test processes have zero orders, fills, settlements and strategy decisions. The test driver made zero execution requests.

Acceptance status: configuration/isolation passed; non-executing negative HTTP cases passed; successful preview and signing preparation blocked; paper execution, real FAK/FOK fills, foreign existing-order retrieval, replay and fill/accounting agreement remain **unverified**. No positive economic outcome is inferred from the negative tests. No new image is authorized for deployment by this test request.

### Defects established by real-service testing

1. API candidate construction called `load_point_in_time_inputs` with a CLOB epoch, invoking the obsolete `polymarket.orderbook_checkpoints` reader. Both modes returned 503. Local database logs proved `relation "polymarket.orderbook_checkpoints" does not exist`. The fix snapshots both books from the existing canonical gRPC-fed `BookRegistry` under one read lock and uses the repository's supported `None` book-epoch path for unchanged reference/history/fee reads. No missing table is recreated, no new feed is introduced, and shared OMS/arrival checks remain authoritative. A regression checks book identity, epoch, timestamps, depth, missing and quarantined evidence.
2. The new paper-capital query's `COALESCE(SUM(...),0)` returned **zero rows** for the empty test process under the deployed Timescale partial/final hash-aggregate plan. This would make `fetch_one` fail after the book dependency was repaired. A scalar outer `COALESCE((SELECT SUM(...)),0)` returned one numeric zero on the same read-only query. Only the new rail's paper-capital reader and external audit query were adjusted; errors still propagate and populated-account semantics remain unchanged.

### Persistence boundary sweep

Read-only catalog inspection and source call tracing found no absent write target in the new rail or the canonical persistence methods it calls. The runtime role has SELECT/INSERT/UPDATE permissions on the checked targets:

| Responsibility | Existing targets / ownership |
| --- | --- |
| Order identity, status, reservations and resolution | `polymarket.orders`; reservations are existing order states plus transaction-scoped advisory locks, not a new table |
| Atomic paper fill commit and live fill deduplication | `polymarket.fills`, `polymarket.fill_identities`; `ExecutionLease::commit_paper` uses the existing order SQL and `Store::insert_fill_in_transaction` |
| Settlement recognition and recovery | `polymarket.btc_paper_settlement_ledger`, `polymarket.btc_official_resolution_watches`, `polymarket.btc_interval_markets`, `polymarket.btc_market_labels` |
| Live venue and account reconciliation | `polymarket.live_venue_events`, `polymarket.live_reconciliation_runs`, `polymarket.account_trades`, `polymarket.account_position_snapshots`, `polymarket.account_reconciliation_runs`, `polymarket.risk_events` |
| Process lifecycle and immutable run evidence | `polymarket.trading_processes`, `polymarket.trading_process_events`; run manifests are existing events |

Dormant `BtcRepository::insert_orderbook_checkpoint` and `insert_orderbook_checkpoint_pair` still reference the retired checkpoint table, but have no runtime call sites in bot source and are not rail write paths. They were not removed or changed. This catalog sweep proves target existence/privileges, not successful fill/settlement transactions; those still require the blocked execution qualification.
