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

The unchanged local CI script completed on repaired source `2f4805b5c23b0d99a04e885e778568bd8d36b332`: formatting and Clippy passed; **561 Rust tests passed, 0 failed, 5 ignored**; **17 existing Python tests passed**. The two external-driver safety tests passed separately, including under Python optimization. Read-only SQL confirmed zero debit for the empty process and exact old/new debit agreement for an existing populated paper process.

Built candidate: `capitonic/polymarket-bot:v3.4.0-local.4`, image ID `sha256:2722557e7cf1be789d87ebb3f3a787429d3dc0013bc81054c6901b0ad964ea43`. Local annotated image/version provenance points to the repaired source. This candidate was **not deployed or pushed**. Helm files/versions, CI/CD workflows/scripts, image build/deploy code, ingesters and database schema remain unchanged. Positive API preparation and paper execution must be repeated after separately authorized deployment; they are not claimed by this build.

## Shared latency qualification — 2026-10-09

Implemented shared monotonic timing under the existing execution metrics owner, with `process_id`, `execution_mode`, `origin`, bounded stage and operation-outcome labels. The timers preserve execution results and classify rejection/error/cancellation separately. Recording uses preallocated atomic histograms without checkpoint persistence, network export or logging. Existing live pending-claim and POST callbacks expose their respective timing; paper reports actual arrival wait and atomic persistence separately. See the instrumentation contract for nesting and coverage limits.

Validation and deployment:

- Unchanged local CI passed formatting, Clippy, **563 Rust tests** (0 failed, 5 existing ignored) and **17 Python tooling tests**. Two new Rust tests cover histogram boundaries/sums/maxima, label isolation, both modes, result preservation, rejection/error and cancelled futures. Existing durable paper fault tests also exercise failed-commit timing. All five durable paper tests passed separately.
- External driver safety/duration/report tests: **5 passed**, including timeout/HTTP error timing, sparse percentile suppression, replay separation and creation-bounded audit queries. Four preceded the image build; the fifth validates the subsequent external-driver-only audit fix. The four initial tests also passed under optimized Python; the final five were rerun under both modes.
- **11** existing-owner Prometheus/dashboard query fixtures passed. Three latency panels were added to the maintained Order Execution dashboard source. They were not provisioned: bot CD does not deploy Grafana, and unrelated provisioned dashboard differences were not overwritten.
- Local CI selected **v3.4.0-local.5**, source `b4e196733cfc5da28672fc4f3d7e9ad3a6c11ae6`, image ID `sha256:faeb8e44fad9622e2ac5c14b94f5086d4727028c65ea300acc12903280f44ddd`.
- The unchanged local CD script generated image/appVersion pins, committed as `4c5df6b3`. Per the user's explicit instruction, no chart version was manually changed; it remains **0.4.15**. No template, CI/CD, build/deploy script, ingester or schema change was made.
- Local CD deployed Helm revision **49**, passed exact-image, bot process recovery, unchanged migration/enabled-process set, worker/profile and feed advancement checks. Capacity was 14 ready workers, 9 desired realtime profiles, 0 active backfill allocations. Rollback remains the recorded v3.4.0-local.3 image `sha256:247c4641a5391635191f8cff897124654873848dcc729b565e0cbed9f563a53b` at Helm revision 48. Its CD snapshot is `/var/folders/6w/fnv3n0_569q0458hy9dvzyyh0000gn/T/local-image-cd.jnup7f`.
- Final bot check: exact selected image, ready, **0 restarts**, no ERROR-level entries in the bounded final five-minute/1,200-line log window. No production mutation, remote push or funded order was performed.

Actual qualification used the same external driver and real local ingester data:

- Paper process: `0a4b7c63-f76e-44e4-95c5-09fcfba6101f`.
- Live preparation process: `88122111-db4b-455b-a588-f4f4b7c48be1`.
- Matching new-contract policies, API trigger, fixed five-share FAK setup. No policy was loosened.
- Initial read-only driver audit timed out at five seconds while considering historical Timescale chunks. The driver now prunes fills and decisions at its owned process's creation time (minus one second), preserving its complete valid economic scope. The application query and five-second timeout are unchanged. Startup then passed using the same created process identities.
- **57 real API/storage assertions passed**. Both processes were stopped after two bounded preparation attempts. Final audits confirmed zero orders/fills/settlements/strategy decisions and preservation of all nine pre-existing enabled process configurations/running states. There were **zero execution requests**.
- Preparation initially rejected outside the configured entry window. Within the window, repeated `future_input_timestamp` rejections prevented successful paper preview and complete live preparation. Existing readiness evidence for market `5432581` showed source `15:42:06.060Z`, receipt `15:42:06.001459Z`, check `15:42:06.009991944Z`: the source was approximately 50 ms ahead of the check. The OMS's strict future-source validation rejects such evidence. The readiness response otherwise reported healthy; resolving this semantic difference is not achieved by weakening a gate during qualification.
- Live had one successful preview, followed by a rejected preparation; its existing reconciliation also reported three mismatches and zero unresolved orders. No funded test or accounting repair was attempted.

Measured paper values from 14 observations per stage (seconds converted to milliseconds below):

| Stage / outcome | Samples | Mean ms | Observed maximum ms |
| --- | ---: | ---: | ---: |
| Evidence / completed | 14 | 4.362 | 14.763 |
| Accounting / completed | 14 | 304.851 | 403.822 |
| Admission / rejected | 14 | 0.027 | 0.042 |
| Preview / rejected | 14 | 304.898 | 403.892 |

Preview includes accounting and admission, but excludes prior evidence construction; these nested stages must not be added together. Accounting consumes almost all measured preview time. Its timer encloses the existing collection of accounting reads; these results do **not** identify an individual SQL query as the cause. Live accounting averaged 706.771 ms across 19 samples (maximum 1,171.895 ms); different account checks prevent interpreting that as a paper/live execution parity result. The server's paper histograms were independently verified in stored Prometheus samples using `last_over_time` after process shutdown.

All paper p95/p99 fields are **null** because counts are below the report's minimum thresholds. There is no executed-order, reservation-wait, paper-arrival, persistence or final-validation runtime latency result. Configured simulation delay is reported separately in external evidence, but was never exercised. No legacy-vs-new end-to-end latency comparison or optimization claim is made.

Raw local evidence is retained in `/tmp/order-latency-fak/` (`run.json`, `latency.json`, `latency.prom`), with local CI/CD logs and the scoped Prometheus response under `/tmp/order-latency-*`. Only this concise result is committed. Test-driver/documentation follow-ups do not change image inputs or require another image allocation.

Acceptance: shared instrumentation, automated checks, local build/deployment and blocked-preparation measurement are verified. Positive preview/dry-run, paper FAK/FOK execution/persistence and executed-order latency qualification remain **blocked**, so development order qualification is **not complete**. No gate, clock tolerance, accounting query, reservation behavior or live reconciliation rule was changed to obtain a passing test.

## Accounting query optimization qualification — 2026-10-09

Applied three result-equivalent query optimizations on the existing repository paths:
explicit process filtering in the shared daily high-water-mark fill scan; unresolved
order filtering before account JSON extraction; and one process-owned fill-order set
for open-market evaluation. The latter two are new execution repository queries; the
shared high-water query remains one implementation for all callers and both modes.
No mode-only filter, process-creation cutoff, new cache, checkpoint, table, schema,
accounting rule, gate, or reconciliation change was introduced. Rail/OMS contracts
remain 1.0.0. Historical paper/live fills, ambiguity and conflicting credits remain visible.

Source `b2de90de38861b0045cfe18da8e497c2ef51b568` passed the unchanged local CI:
formatting, Clippy, **563 Rust tests** (0 failed, 5 existing ignored), and **17 Python
tooling tests**. **8 external qualification tests passed**, including three new opt-in,
read-only PostgreSQL tests executing the actual SQL against inline fixtures. They cover
both fill modes, causal cutoffs, empty processes, canonical/duplicate/conflicting
credits, process ownership, stopped account sleeves and cancelled/expired reservations.

Local CI built **v3.4.0-local.6**, image
`sha256:62dfbcd78560caa29609726f674c68530afa96c2a0c63020835f89762918ac55`.
Local CD generated and committed the image/appVersion pin (`ac71ad2f`) and deployed
Helm revision **50**. Chart version remains **0.4.15** under the user's explicit
instruction; no manual chart edits, templates, CI/CD/build/deploy tooling, ingesters,
production changes or remote pushes occurred. CD verified exact image identity,
process recovery, unchanged enabled-process/migration sets, worker/profile capacity
and advancing feeds. Capacity remained 14 ready workers, 9 desired realtime profiles,
0 active backfill allocations. Rollback is revision 49, v3.4.0-local.5,
`sha256:faeb8e44fad9622e2ac5c14b94f5086d4727028c65ea300acc12903280f44ddd`;
the CD rollback snapshot is `/var/folders/6w/fnv3n0_569q0458hy9dvzyyh0000gn/T/local-image-cd.ZP0ieD`.

Dedicated API qualification used paper process `b4171f11-9fde-45e0-ad8c-37f6954769ec`
and non-submitting live process `bd63f9ce-e58b-4395-aee2-aab3ce197621`. Their execution
configurations exactly match the prior latency run. Shared policies match between
paper/live, with only the established paper simulation policy differing. Fourteen
previews per mode used real local ingester evidence and the normal API/OMS path.
The existing shared metrics measured:

| Accounting stage | Prior samples | New samples | Prior mean ms | New mean ms | Mean reduction | Prior maximum ms | New maximum ms |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Paper | 14 | 14 | 304.851 | 47.353 | 84.47% | 403.822 | 81.984 |
| Live | 19 | 14 | 706.771 | 371.732 | 47.40% | 1171.895 | 642.104 |

The first sample batch included one successful paper preview and two successful live
previews. Paper rejected-preview mean was 44.730 ms versus 304.898 ms previously;
evidence mean was 4.458 ms versus 4.362 ms. Accounting contains multiple SQL reads;
live also includes venue balance access. These are separate local runs with identical
policies, not a controlled load benchmark or a guarantee of tail latency. Fourteen
samples do not support p95/p99 reporting, and nested stages must not be added together.

**59 API/storage assertions passed**, including no economic writes from preview,
configuration/authentication/ownership checks and preservation of all nine prior enabled
processes. Bounded subsequent preparation reached the next valid entry window but
stopped on repeated `future_input_timestamp`, the existing readiness/OMS issue.
The dedicated live process also reported one existing reconciliation mismatch and
zero unresolved orders. No policy, clock tolerance or gate was weakened. Neither mode
completed dry-run preparation; there were **zero execution requests, orders, fills,
settlements or strategy decisions** for these test processes. Both were stopped and
audited. Therefore paper fill/persistence and complete executed-order latency remain
unqualified; the measured accounting reduction does not claim their completion.

Local Prometheus reported no active alerts; the bounded post-deployment bot log check
had no ERROR entries. Grafana was not provisioned or modified. Evidence is retained at
`/tmp/order-accounting-qualified-fak/` (`preview-latency.json`, `preview-latency.prom`,
`latency-difference.json`, final `latency.json`, and `run.json`), with local CI/CD logs
at `/tmp/order-accounting-ci.log` and `/tmp/order-accounting-cd.log`. The prior comparison
is `/tmp/order-latency-fak/latency.json`; raw SQL exploration remains under
`/tmp/rail-hot-path-analysis/`. Only this concise verification result is committed.
