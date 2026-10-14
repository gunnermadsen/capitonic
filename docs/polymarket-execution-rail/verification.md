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

Candidate v3.4.0-local.2 was the initial generic controller and instrumentation source. The current rail-selected runtime adds UMR execution through the same service while keeping authenticated API overrides available. Existing legacy process behavior and dry-run routes are retained.

Focused evidence before full CI:

- Three authenticated HTTP tests cover all new routes, strict envelopes and override rejection, response/error mapping, unavailable preparation, and ownership-safe lookup using a registered deterministic application.
- OMS tests cover actual policy reporting, original-owner/request replay despite a changed configuration, and distinct process/mode/origin metrics. API-specific boundary tests cover signing diagnostic redaction and a rejected record that cannot be mistaken for an approved execution intent.
- Application configuration testing verifies paper/live policy projection, immutable run identity across resume, and one rail selector for concurrent UMR and API availability.
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
- Both use `btc_realtime_paper_process_v6`, fixed five-share sizing, FAK, and matching shared `order_policies` from the maintained examples. Rail selection alone activates UMR execution while the authenticated API remains available. Run identifiers are unique. Live execution was never requested.
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

## Reusable process pair and inactive lifecycle transitions

Local bot `v3.4.0-local.7`, source `cf0134f8872daedef450715fafc20690421ca72f`,
image `sha256:1f2b4d63292cee273463860b0c4439426779eca3d4ad9ed95e6310fc5405bee6`,
was deployed through the unchanged local CI/CD scripts. CI passed 564 Rust tests
(five explicitly ignored) and 17 Python tooling tests. All ten external driver/SQL
regression tests passed, including repeated reuse of the same process identities.

The existing lifecycle API now permits inactive created, stopped, failed, completed,
and expired definitions to move to stopped or completed; these states are also
eligible for the existing start preflight. Active-runtime shutdown, run identity,
accounting and execution safety checks remain in their existing paths. Stale runtime
callbacks cannot rewrite inactive operator decisions. API checks verified stopped →
completed, completed → stopped → completed, and repeat completion without an update.

The four duplicate qualification processes were completed through the API:
`88122111-db4b-455b-a588-f4f4b7c48be1`, `0a4b7c63-f76e-44e4-95c5-09fcfba6101f`,
`99a8c962-33d1-43be-8dee-78b600145bd8`, and `fff03502-3e34-416e-a8fc-7765b837e05e`.
The driver retains only paper `b4171f11-9fde-45e0-ad8c-37f6954769ec` and live
`bd63f9ce-e58b-4395-aee2-aab3ce197621`; both remain stopped. Future runs reuse their
configuration and economic history with fresh run identities, never new processes.

All 68 process records remain present. All unrelated definitions and enabled states
were preserved; the nine enabled processes recovered after deployment. The selected
image is ready with zero restarts, bounded logs contain no ERROR/panic entries, and
CD verified unchanged migration state, advancing feeds and capacity of 14 ready
workers for nine desired realtime profiles with no active backfill shards. No orders
were requested during this lifecycle verification. Evidence: `/tmp/rail-lifecycle-ci.log`,
`/tmp/rail-lifecycle-cd.log`, and `/tmp/rail-test-process-completion.json`. The CD rollback
snapshot is `/var/folders/6w/fnv3n0_569q0458hy9dvzyyh0000gn/T/local-image-cd.FpoZ9v`.

## Shared book clocks and paper API execution — 2026-10-09

The preparation defect was a source-clock interpretation mismatch: readiness and
existing execution guards permit a bounded remote-source lead, while the new OMS
called the frozen strategy validator's strict zero-lead check. The shared book
validator now exposes the existing execution interpretation to both new paper/live
OMS paths: source lead is bounded by `max_book_age_ms`, receipt time remains causal,
and stale/incomplete/unhealthy evidence still rejects. No timestamp is rewritten,
policy loosened, new tolerance configured, or legacy strategy behavior changed.
Regression evidence covers selected/opposite books, the observed 50 ms lead, exact
source boundaries, future receipts, stale evidence and automatic recovery in both
modes, and preservation of the legacy strict validator.

Unchanged local CI passed formatting, Clippy, **565 Rust tests** (five explicitly
ignored) and **17 Python tooling tests**. The external driver/SQL suite passed all
**12 tests**, including paper-only scope, process reuse and correct/incorrect official
settlement payouts. The existing deterministic suite also passed FAK partial fills,
FOK atomic rejection, failed paper persistence, lost commit response, capital recovery
and idempotent positive/zero settlement-credit cases.

Local CI built **v3.4.0-local.8**, source
`f768a07a0af88319245038d23057f228539e3c2a`, image
`sha256:521d99b26fcb17ed01bf6e8743e3d685a42321e40593123c85e3fd5343ff10a7`.
The local CD script alone generated image/appVersion pins and deployed Helm revision
52. Chart version remains 0.4.15. It verified exact image, recovery of all nine enabled
processes, unchanged migrations, advancing feeds, 14 ready workers and nine desired
realtime profiles with no active backfill allocations. Rollback is v3.4.0-local.7,
image `sha256:1f2b4d63292cee273463860b0c4439426779eca3d4ad9ed95e6310fc5405bee6`;
CD snapshot: `/var/folders/6w/fnv3n0_569q0458hy9dvzyyh0000gn/T/local-image-cd.z9GcQJ`.
No CI/CD, ingester, schema, production or remote Git changes occurred.

Only retained paper process `b4171f11-9fde-45e0-ad8c-37f6954769ec` was configured
and started for testing, using its new `order_policies` contract. Fixed five-share
sizing, 150 ms arrival delay, 80% visible depth, entry window, exposure limits and
all other trading policies were preserved. Only FAK/FOK selection and the required
fresh run identity changed. The live test process was neither changed nor started.

| Order type | Market | Order | Execution ms | HTTP execute ms | Arrival ms | Final validation ms | Reservation wait ms | Persistence mean/sample ms |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| FAK | 5434508 | paper-0c777ea0-7e2e-4f89-9a99-ee9e2b007e25 | 273.979 | 299.185 | 151.578 | 34.721 | 0.225 | 2.100 |
| FOK | 5435275 | paper-bfa61b2a-6a01-4dc6-9c4e-50a2bbf57a6f | 256.315 | 275.550 | 151.332 | 35.260 | 0.742 | 2.383 |
| FAK | 5435432 | paper-da5196e1-00d1-49cd-923f-6b050737cccf | 287.522 | 302.913 | 150.701 | 38.214 | 0.747 | 3.285 |

Each row has one successful execution and two persistence observations. Server times
are from shared Prometheus histograms, independently confirmed in stored Prometheus
samples; HTTP times are external monotonic measurements. Stages are nested, not
additive. Each run has a separate process metrics owner, so restarted-owner counters
are not combined. **Executed-order p95 remains unqualified:** only three fills were
requested across these market-period samples, below the existing 20-observation
minimum. The qualified Prometheus p95 query returns no series. No rejection/replay
samples are substituted for successful executions. Twenty-one successful replays per
run had external HTTP p95 of 22.194, 22.967 and 12.421 ms respectively; these are replay
latencies, not new execution latency or live transport latency.

All four order routes passed real-service qualification: preview and dry run created
no economic records; execute returned durable fills; retrieval matched canonical
ownership, configuration identity and fill evidence. Same-key retries returned the
same order with no duplicate effects; changed payloads returned 409. Retrieval and
replay remained valid after stop and after configuration/run changes. Authentication,
forbidden overrides, malformed input, unknown resources and foreign ownership checks
passed. No `future_input_timestamp` rejection recurred. An intervening one-sided
market correctly prevented preparation; later eligible evidence recovered without
changing gates. Entry-window and pending-exposure rejections also recovered normally.

All three orders filled five shares and settled through official resolution, including
two prior-run orders recovered by later runs of the same process. Each has one credited
settlement with matching official winner, fill quantity, notional, fees and net PnL;
repeat recovery/retrieval preserved the original settlement identity and credit time.
All three were losing outcomes with zero payout. Total durable debit is **3.990750**,
total payout **0**, and persisted-history-derived available paper collateral **996.009250**
from the original 1000 seed. Positive payout and actual partial FAK fills were not
observed in these market samples; their deterministic regression coverage is separate.
The final process has three orders, three fills, three credited settlements and zero
strategy decisions. It is stopped. All 68 process identities remain; all unrelated
configurations/statuses/enabled states, including the live test process, are unchanged.
The bot remains ready with zero restarts, bounded logs contain no ERROR/panic entries,
and Prometheus reports no firing alerts. No extra whole-pod restart was injected;
recovery here exercises process stop/start with unresolved settlement ownership.

The external driver now supports `--mode paper` and a `settlement` action using normal
GET endpoints and bounded read-only SQL. It retains the same fixed process identities
and adds no runtime test implementation or trading pathway. Evidence is under
`/tmp/rail-paper-fak-a`, `/tmp/rail-paper-fok-b`, `/tmp/rail-paper-fak-c`, with consolidated
`/tmp/rail-paper-execution-report.json`, `/tmp/rail-clock-ci.log`,
`/tmp/rail-clock-cd.log` and `/tmp/rail-paper-driver-tests.log`.

### Read-only follow-up query analysis

The separately requested subagent applied no source or database changes. Warmed
prepared-query server planning/execution medians were:

| Candidate | Existing ms | Combined ms | Round trips |
| --- | ---: | ---: | --- |
| Retained paper process capital | 16.296 | 13.692 | 3 → 1 |
| Historical paper process capital | 14.313 | 13.621 | 3 → 1 |
| Live account discovery + seven reservation reads | 7.073 | 6.359 | 8 → 1 |

These exclude network/pool waits and do not prove end-to-end savings. Current-data
result equivalence passed; live pending results were empty, so nonempty/overflow
regressions would still be required before implementation. An alternative ownership
query regressed from 31.077 to 47.320 ms in fresh-session total time and was rejected.
Historical Timescale planning remains significant; no history/source/ownership checks
were weakened. Full report: `/tmp/rail-query-followup-report.md`; plans and result
comparisons: `/tmp/rail-query-followup/`. No proposed query optimization was applied.


## Inactive live preflight and retained-process qualification (2026-10-09)

Candidate `v3.4.0-local.9` uses source
`ff681d2731f3282871d3fe4934392d75cfda36f4` and image
`sha256:764a8998371f1936dac844c7e720b261d6a1680acfdaa9d92544b3171aa91b85`.
The unchanged local CI/CD scripts built and deployed it at Helm bot revision 53.
CI passed 565 Rust tests (five ignored) and 17 tooling tests; all 13 external
qualification tests passed, including the three opt-in read-only SQL cases.
The bot chart version remains 0.4.15; only script-generated image/appVersion pins
changed. No pipeline, image-build/deploy script, ingester or database schema changed.

The inactive `/admin/trading-processes/{process_id}/live-preflight` route previously
returned 501 for the new TPC because it called the intentionally legacy-only
validator. It now compiles selected rail definitions and reuses the same execution
settings projection as startup. Legacy validation is unchanged. Regression
assertions cover compiled mode, account, exposure and authorization fields and
invalid-policy rejection. The external qualification driver also supports advancing
a verified settled paper order without replacing its process or metrics owner;
it refuses an unsettled order, changed economics, changed configuration or a
stopped process while preserving the pending request identity.

The deployment preserved the nine pre-existing enabled processes and migration
state. Capacity remained 14 ready workers, nine desired profiles and zero active
backfill shards. Rollback evidence is in
`/var/folders/6w/fnv3n0_569q0458hy9dvzyyh0000gn/T/local-image-cd.eDyY2Y`;
CI/CD logs are `/tmp/rail-live-preflight-ci.log` and
`/tmp/rail-live-preflight-cd.log`.

The user's approved Grafana commit `fd32e5d3121306341de9dbbec347bfa9d42393ab`
was merged as `fbe39cecdbd1814a009405ed569c30061c0deaf9`. Its complete PnL
content matches the prior deployed dashboard and remains unchanged apart from the
rail navigation link. The combined Grafana chart was rendered, diffed, linted and
provisioned at revision 47 with existing runtime values and image. Changes were
limited to the Order Execution dashboard, three navigation links, and shared
execution recovery/error alerts. Provenance:
`provisioned/observability/20261009T202316Z`.
The dashboard GET and bounded economics SELECT through the Grafana Postgres
datasource passed; Prometheus scraped process/mode/origin latency; the exact Loki
queries returned nested paper/API stage and recovery events. Both new alerts
loaded and evaluated without error. Eleven committed observability query fixtures
passed. Evidence: `/tmp/rail-observability-qualification/`.

The retained live process `bd63f9ce-e58b-4395-aee2-aab3ce197621` remains stopped.
Its sizing policy now has lower/default/upper shares all equal to one. Current
market metadata reports minimum size five; the shared OMS checks target quantity
against that venue minimum and must not silently raise or clip it. The repaired
non-submitting preflight returned HTTP 200 and verified credentials, account
identity, readable collateral and zero open venue orders. It did **not** establish
live readiness: one unmatched wallet trade made accounting unproven, zero owned
orders were unresolved, and the inactive venue reported `global_enable_required`.
That initial gate has an existing configured-startup restoration path; it is not
evidence of a persistent manual re-enable requirement. The preflight
also reports that its trading-disabled configuration prerequisite is not satisfied
by the retained execution flags; the process itself stayed inactive.

The historical mismatch was an unlinked five-share wallet BUY at 15:53:07 UTC
(transaction `0xd2c73c64ae45c68da0708c7c4327e3286a35066f5f5da95eee861fcd3932297c`).
The refreshed 20:18 UTC reconciliation found a different unlinked five-share BUY
at 19:31:43 UTC (transaction
`0xdfc1b0330983461d94613959f2f3780fd9b203c3c253bf9016cbf7b5e23bb28b`).
At 20:33 UTC, after the 19:31 trade aged out of the existing one-hour activity
window, a bounded recheck automatically returned zero mismatches and proven
accounting. A further preflight with the existing submission flags temporarily
disabled passed every preflight condition (`ready=true`); the original flags were
restored while the process remained stopped with its sizing target set to one. Orders/fills
were unchanged throughout. This proves non-submitting credential/reconciliation
preflight, not one-share order admission or signing.

These are account activity records, not retained-process fills. Account matching
requires canonical order ownership; matching a paper fill by token is insufficient.
The configured production SSH hostname failed DNS resolution, so a possible
production origin remains unverified. No trade was reassigned, no accounting gate
was bypassed, and no funded order was submitted. Live FAK/FOK signing/submission,
fill and redemption qualification remain incomplete. Evidence:
`/tmp/rail-live-one-share/` and `/tmp/rail-qualification-continued/`.

The separate read-only query analysis found warm server planning/execution medians
of 13.515 → 11.611 ms for retained paper capital, 14.974 → 14.120 ms for a historical
paper process, and 5.891 → 5.607 ms for live account/reservation reads. Same-snapshot
results matched, but live reservations were empty; nonempty/overflow parity and
network/pool savings are not proven. No query optimization was applied. Report:
`/tmp/rail-query-qualification-report.md`.


### Paper qualification outcome and remaining blockers

The first local.9 cohort ended at 20:46:44 UTC without meeting full paper
qualification. Only the retained process was used; no replacement process, reset
capital, relaxed policy or ingester change was introduced. Both retained processes
were stopped at that boundary. Its API inventory audit preserved all nine unrelated
enabled processes and their configurations. The paper process was subsequently
restarted to recover the late resolution described below and collect a separate
metrics cohort; live remained stopped.

One additional pre-deployment FAK order on local.8 filled and settled with payout
zero (`paper-e723094d-dd67-4288-aa33-0685ea8f0f90`). It is excluded from local.9
latency statistics. Under one uninterrupted local.9 paper metrics owner:

| Order | Result |
| --- | --- |
| `paper-b14bab96-4949-489f-824d-7a05126d5fb5` | Five shares at 0.34, fee 0.07854; official settlement credited payout zero. |
| `paper-3639b1e0-0da0-4091-bbbb-2cf4557a1e78` | Five shares at 0.24, fee 0.06384; official settlement credited payout zero. |
| `paper-ab040587-5543-416a-9921-0cfe7b0d0583` | Price improvement filled 5.4166666667 shares at 0.12, fee 0.04004; market 5442351 lacked an official winner at the bounded deadline, then recovered and credited payout zero after the late official resolution. |
| `paper-c801efb5-577f-40df-befd-a445416b6d6f` | Arrival-time `orderbook_marketability` rejection; no fill or debit. |

Preview, non-submitting paper dry run, execute and retrieval passed. Canonical
order/fill ownership, no synthetic UMR strategy decisions, actual notional-plus-fee
debits, three same-key successful replays without duplicate effects, and the real
arrival-time rejection were verified. Existing deterministic CI cases passed for
FAK partial fill, FOK atomic failure, failed persistence, duplicate evidence,
settlement idempotency and recovery. Earlier actual FOK evidence remains recorded
above; it is not relabelled as a local.9 funded or paper FOK execution.

The fractional fill follows the existing `buy_cash_budget` contract: a requested
share target times limit price seals a cash amount, and improved prices can produce
more shares. Therefore a one-share TPC sizing target is **not** an exact one-share
fill ceiling. No funded order was attempted under that ambiguous quantity bound.
The external settlement checker now rounds its recomputed amounts to the existing
Postgres `NUMERIC(30,10)` ledger scale instead of comparing extra multiplication
digits to rounded storage. A focused regression covers the observed fractional
fill, winning and losing economics, tie rounding, and a one-unit discrepancy.
All 15 external tests passed including the three read-only SQL cases and the
collection driver stopping without submission when the existing daily loss policy
rejects preparation. The external
driver alone was resumed with that checker; the process, pod and metrics owner
were not restarted. No application arithmetic or storage contract changed.

| Shared Prometheus stage | Samples | Mean ms | Maximum ms |
| --- | ---: | ---: | ---: |
| Execution, completed / actual filled receipts | 3 | 264.623 | 268.038 |
| Execution, rejected | 1 | 258.756 | 258.756 |
| Final validation, completed | 3 | 37.735 | 41.598 |
| Reservation wait, including rejected attempt | 4 | 0.480 | 0.547 |
| Persistence observations, including rejected attempt | 8 | 2.976 | 5.583 |
| Paper arrival delay, including rejected attempt | 4 | 151.369 | 151.824 |
| Accounting across preview/preparation/execution | 62 | 42.128 | 106.131 |

Stages overlap and must not be added. Configured paper delay is 150 ms. **Execution
p95 is unavailable: three successful observations do not meet the 20-sample floor.**
The separate accounting histogram has 62 samples and an approximate p95 of 85 ms;
it is not execution p95. Histogram interpolation is coarse; exact maxima above
come from the shared owner-lifetime maximum gauge. Rejections, preparation, HTTP
retrieval polling and historical replay were not counted as successful executions.

At the first cohort deadline, official resolution was missing for market 5442351
(closed 20:35 UTC). The resolution profile had a healthy owner and fresh heartbeat, but
its latest source fact remained 20:32 UTC; bounded worker logs showed websocket
resets around 20:30:49 and 20:37:05 UTC. Bot logs showed no settlement write failure.
A direct public CLOB market read returned 403, so venue delay versus ingestion loss
is not proven. The existing process-scoped Grafana settlement-incomplete/stale
alerts fired; freshness and shared order recovery remained healthy. No resolution
was invented or ingester code/state changed to finish the test. This input blocker
was not attributed to the new preflight image, so the image was not rolled back.

Official resolution arrived at 20:46:48.806495 UTC, four seconds after the first
cohort stopped. Restarting the same retained paper process recovered the existing
order and credited its zero payout once at 20:49:43.084716 UTC. Fill ownership,
ledger-scale debit/PnL and one canonical settlement passed again. Subsequent orders
use a separate metrics cohort; the earlier three latency samples are not combined
with the restarted owner. The longer settlement observation budget changes only
the external driver, not any rail policy or resolution semantics.

The restarted metrics owner completed two more FAK fills, each with successful
same-key replay and exactly one official zero-payout settlement:

| Order | Fill and credited result |
| --- | --- |
| `paper-92569ec1-d077-4a2c-b2dc-dfeb3f16b3ce` | Five shares at 0.14, fee 0.04214; credited at 20:57:02 UTC. |
| `paper-a87c6895-b6ff-4f72-8e99-cdefa628a298` | Five shares at 0.41, fee 0.084665; credited at 21:03:44 UTC. |

One additional attempt (`paper-3b84bbf7-0ae7-49a4-b6eb-6ca9e461492f`)
was definitively rejected for `orderbook_readiness`, with no fill/debit. The
successful retry used a new identity only after definitive nonexecution.

| Restarted-owner stage | Samples | Mean ms | Maximum ms |
| --- | ---: | ---: | ---: |
| Execution, completed / actual filled receipts | 2 | 301.551 | 322.442 |
| Execution, rejected | 1 | 402.929 | 402.929 |
| Final validation, completed | 2 | 43.931 | 45.133 |
| Reservation wait, including rejected attempt | 3 | 0.616 | 1.024 |
| Persistence observations, including rejected attempt | 6 | 3.334 | 7.318 |
| Paper arrival delay, including rejected attempt | 3 | 152.485 | 155.490 |

Prometheus stored both cohorts' counts, sums and maxima. Neither cohort meets the
20-success execution p95 floor; cumulative scrapes across the restart are not
combined. The second owner's accounting p95 estimate is 90.893 ms across 71
observations, not order execution p95.

At 21:05:31 UTC the next preview rejected `daily_loss_limit`; the external driver
stopped without another execute. The retained process now has nine lifetime fills
and nine credited settlements, all payout zero, with total actual debit
11.289560000004 and calculated remaining paper collateral 988.710439999996
from seed 1000.
Realized ledger PnL is -11.28956. The unchanged 10 USD daily limit checks already
realized PnL before admission; it is not a hard worst-case loss cap, so the last
eligible order crossed that threshold. No capital reset or limit increase was used
to obtain more samples. Both retained processes finished stopped/disabled; the
nine unrelated enabled processes and their configurations were preserved. Live
still has zero orders/fills, and its lower/default/upper sizing values remain one.

Positive payout and its reuse without duplicate credit remain **unproven on the
retained process**, and enough successful samples for execution p95 were not
collected. The active qualification blocker is now the existing daily loss policy;
all tested paper orders have official credited settlements. The new-contract live credential/reconciliation preflight passed, but
one-share admission/signing and funded FAK/FOK execution, fills and redemption
remain **unqualified**. The live sizing policy is one; current market metadata is
five and the shared OMS enforces that minimum. All live order/fill snapshots stayed
empty. Paper/live shared policies remain equal except for this explicitly requested
sizing difference and paper simulation settings; this is not an equal-size live
execution comparison.

Evidence: `/tmp/rail-paper-qualified-sample/`, `/tmp/rail-paper-owner-latency/`,
`/tmp/rail-paper-collect.log`, `/tmp/rail-paper-settlement-diagnostic/`,
`/tmp/rail-paper-recovered-sample/`, `/tmp/rail-paper-continue.log`,
`/tmp/rail-observability-qualification/paper-recovered-prometheus.json`. The current image remains local.9;
subsequent dashboard, documentation and external-test changes do not alter bot
image inputs. No Git refs were pushed.


## Configured loss limits and lifecycle transitions (2026-10-09)

Implemented authenticated PATCH `/admin/trading-processes/{process_id}/status`
through the existing lifecycle manager, shared with start/stop/complete. Recognized
inactive targets persist after cleanup regardless of previous status; starting and
stopping resolve to their actual runtime outcomes. Repeated requests preserve
consistent state and timestamps. Pending terminal persistence completes before a
new operator transition. Generation-scoped callbacks retain their isolation.
The terminal writer now actually updates allowed inactive rows rather than
acknowledging a differing status without writing it.

Removed the fixed daily-loss maximum from both legacy execution validation and
new rail policy validation. Both accept any positive representable Decimal;
null behavior and the existing parameter shape remain unchanged. Regression
coverage includes values above ten through Decimal::MAX, invalid zero/negative/
overflow values, and shared paper/live OMS rejection at the configured threshold.
Other execution limits and stop-before-reconfiguration behavior are preserved.

Local CI passed 569 Rust tests (five existing opt-in tests ignored), formatting,
Clippy and 17 tooling tests. The unchanged CI/CD scripts built and deployed
v3.4.0-local.10 from source 19f14ac0c1c60215e095611d3bcec8b15b4593d6.
Image ID: `sha256:9dd3fff000fb434ad667b31b18fb6a042b60c4e8892fb2c39ab6c745574368d7`.
Bot Helm revision 54; chart version remains 0.4.15. Only image/appVersion pins
were generated by CD. Rollback is local.9, image ID
`sha256:764a8998371f1936dac844c7e720b261d6a1680acfdaa9d92544b3171aa91b85`.
Snapshot: `/var/folders/6w/fnv3n0_569q0458hy9dvzyyh0000gn/T/local-image-cd.GEGgrU`.

The retained API-only paper process was started before deployment without orders
and automatically resumed afterward with unchanged configuration/economics.
CD verified 14 ready workers, nine desired realtime profiles, zero active
backfill allocations, exact image identity and enabled-process recovery.
Post-deployment API qualification passed 162 assertions: the inactive transition
matrix, idempotent timestamps, authentication/shape/missing-process rejection,
active completion cleanup, starting/running and stopping/stopped consistency,
legacy endpoint compatibility, rejected immutable-run startup preserving state,
and process isolation. The deployed API accepted a 25 USD loss limit on both
retained definitions while inactive, rejected zero/negative/overflow, and rejected
running configuration changes. Tests restored both original configurations and
left both retained processes stopped/disabled. All nine unrelated enabled
processes were preserved. No order execution endpoint was invoked; paper
orders/fills/settlements remained unchanged and live still had zero orders/fills.
The retained paper TPC still specifies ten; removing the validator ceiling does
not raise the configured threshold. Injected runtime shutdown/persistence failures
were not performed against the shared development deployment.

Commit cfac5f824f7c932ce594508491cea75434ad3eb9 was merged with --no-ff in
1beffbcd432da46e42cc46e307bb2a949781fe11. Its sole PnL dashboard change was
provisioned through the existing Grafana chart using preserved release values.
The rail navigation link was retained. Grafana revision 48 passed health and
PnL/rail dashboard API checks; committed panel/selection/navigation content
matched. The chart comparison contained only the PnL ConfigMap data change and
provisioning checksum. Provenance:
`provisioned/observability/20261009T224216Z` on source 19f14ac0.
No chart version was manually changed, no migration or ingester/model/agent/CI/CD
implementation changed, and no Git refs were pushed.

Evidence: `/tmp/rail-status-loss-ci.log`, `/tmp/rail-status-loss-cd.log`,
`/tmp/rail-status-qualification/`, `/tmp/rail-status-qualification-restart.log`,
`/tmp/rail-status-qualification-transitions.log`, `/tmp/rail-status-grafana/`.

## Retained process paired qualification — 2026-10-10 UTC

The external order API driver now supports read-only `compare-policies` and
`compare-preview` actions. The latter sends the identical synthetic candidate
through both existing preview endpoints and preserves the canonical receipts.
Neither action certifies execution parity, submits live orders, or changes a rail
contract. No Rust, schema, ingester, model/agent, CI/CD, or Helm source changed.

The retained paper process received the explicitly approved simulated daily-loss
limit of $25 through the existing process API. Live retained its $10 limit and
one-share sizing. Shared policy comparison records these two differences; all
other shared policy objects match. The current market minimum remains five.
Live preview and dry-run return `below_minimum_order_size` before signing. Its
runtime automatically progressed from initial unproven accounting to proven
accounting, clean idempotency, enabled entries, a connected user websocket, and
zero unresolved orders. No live execution request was made. This does not prove
the provenance of previously unmatched wallet activity or funded reconciliation.
The live process was stopped and its original configuration restored after checks.

The existing paired preview returned `open_position_limit` for paper and
`below_minimum_order_size` for live. Different configured sizes and account state
prevent an admission-parity conclusion. Both canonical rejections are retained.

All 17 external driver/accounting tests passed, including the opt-in read-only
SQL fixtures. Full local CI initially failed three existing UMR telemetry tests
under concurrency; the complete serial run passed 569 Rust tests (five ignored)
and 17 tooling tests, plus formatting and Clippy. No unrelated telemetry repair
was made; parallel-suite reliability remains a disclosed limitation.

Local CI reused immutable image `v3.4.0-local.10`,
`sha256:9dd3fff000fb434ad667b31b18fb6a042b60c4e8892fb2c39ab6c745574368d7`,
source `19f14ac0c1c60215e095611d3bcec8b15b4593d6`, because application image
inputs did not change. Local CD confirmed this exact image was already deployed
and made no Helm release change. Its capacity check found 14 ready workers,
nine desired realtime profiles, and zero active backfill shards. No version was
artificially allocated, no merge or push occurred, and no chart version was edited.

Grafana health passed, but the previously provisioned `order-execution` dashboard
returned 404 and was absent from dashboard search during this run. Its source
remains in this branch. The full Grafana chart was not reapplied over independent
runtime changes. Rail metrics remain exposed by the bot; dashboard availability
is an outstanding certification limitation. Both existing shared alert UIDs
`order_execution_recovery_unhealthy` and `order_execution_errors` remain provisioned.

Evidence is external: `/tmp/rail-paired-paper`, `/tmp/rail-paired-live`,
`/tmp/rail-paired-live-second`, `/tmp/rail-paired-preview-active`,
`/tmp/rail-paired-ci.log`, `/tmp/rail-paired-ci-serial.log`,
`/tmp/rail-paired-cd.log`, and `/tmp/rail-paired-sql-tests.log`.

The bounded paper run completed five FAK fills across five markets. Each passed
preview, dry-run, execution, canonical retrieval, ownership isolation, fee-inclusive
debit, authorized cash budget, and same-key replay checks. All five settled with
exactly one credited ledger entry and zero payout. Replaying all five keys after
stopping returned the original orders without changing fills or credits. The new
simulated debit was $6.630595; lifetime retained-process debit is $17.920155000004
and credited payout remains zero. No capital or history was reset.

| Order | Fill price | Shares | Fee | Settlement payout |
| --- | ---: | ---: | ---: | ---: |
| `paper-01ea6b98-f1ea-4e18-a9cc-7082b26643d2` | 0.34 | 5 | 0.0785400000 | 0.0 |
| `paper-3c480db0-70b4-4711-86e0-d57e36038128` | 0.11 | 5 | 0.0342650000 | 0.0 |
| `paper-0b4804f6-4aa2-4047-a7e1-5919a2863769` | 0.07 | 5 | 0.0227850000 | 0.0 |
| `paper-b0d76d58-9f1a-4005-ab5e-f7fe84b043a9` | 0.21 | 5 | 0.0580650000 | 0.0 |
| `paper-628da61a-814e-4412-affc-835fec7a24e4` | 0.54 | 5 | 0.0869400000 | 0.0 |

Final Prometheus evidence uses one unchanged process/pod metrics owner:

| Stage | Observations | Mean ms | Maximum ms |
| --- | ---: | ---: | ---: |
| Execution completed | 5 | 290.033 | 328.792 |
| Final validation | 5 | 46.386 | 52.318 |
| Reservation wait | 5 | 0.343 | 0.524 |
| Persistence | 10 | 5.110 | 28.084 |
| Intentional paper arrival | 5 | 150.950 | 151.853 |

Stages overlap and must not be added. Execution p95 remains unavailable under
the established 20-sample reporting minimum. Live-labelled accounting, evidence,
admission, preview and preparation series were present in Prometheus; submission
was unexercised. Evidence: `/tmp/rail-live-latency-series.json`.

Both retained processes ended stopped/disabled. Paper retains the approved $25
limit; live configuration is restored. All nine unrelated enabled processes
retained their configurations and remained running. An initial 45-second settlement
probe expired before market close; continued observation subsequently verified all
five official settlements without intervention.

**Certification remains incomplete:** no paired funded live fill exists, the
one-share live configuration is below the five-share venue minimum, positive
paper credit/reuse was not observed, execution p95 is under-sampled, and the rail
dashboard is unavailable. Existing deterministic FAK/FOK, partial-fill, failure,
and shared admission tests passed; these do not replace funded FAK/FOK evidence.
The bounded run did not expand capital, loosen policies, manufacture a winner, or
claim an unobserved outcome. Funded submission must be performed by the user.

## Integration refresh and local chart provisioning — 2026-10-13

Approved integration commit `5c925b47f1573ac1e453ac3c1cbff3dc909ac147` was
merged into the rail branch at `6f28243298418c9b0cc99fbf427ebacc376ca5a5`.
The only merge conflicts were bot chart pins: retain local.18 and inherit the
accepted chart version 0.4.18. No new image inputs or release-tooling edits were
introduced by the conflict resolution.

Local CI checks passed: 582 Rust tests, five ignored, formatting, Clippy, and
17 tooling tests. Rust tests ran serially because of the previously recorded
shared UMR telemetry test interference. External order API unittest discovery
passed 32 tests with four opt-in tests skipped. Bot and db-migrate chart lint and
named rendering passed; no migration Job was applied.

The first local CD attempt stopped before mutation because its image-only manifest
guard rejected an incoming PVC annotation. After explicit approval, the existing
bot Helm chart provisioned only `argocd.argoproj.io/compare-options: IgnoreExtraneous`
on `polymarket-openai-credentials`. Helm revision 66 is deployed with chart 0.4.18.
The unchanged local CD script then passed and reported the exact image already
deployed, with no additional Helm upgrade.

Reused image: `v3.4.0-local.18`, digest
`sha256:da47ddb40ca3b544e15e4b21bedd809bac20ca14cae720387280f8a48023483d`,
source revision `b1c8685c45172804bf4b5d6bbc6ef6095d42dbb8`. All CI-defined bot
image inputs match that source, so no rebuild or version allocation was needed.
The original pod `polymarket-bot-5cb959b7dc-bxcls` remained ready with zero
restarts. All 11 enabled process identities were preserved and running with
heartbeats newer than two minutes. Migration ledger and backfill allocation
snapshots were unchanged. Capacity remained 14 ready workers for nine desired
realtime profiles and zero active backfill shards. The PVC remains Bound.

This verifies the integration and chart-only deployment, not new order execution,
model-policy ownership migration, UMR cutover or paired live certification. No
process configuration, capital, production state or remote Git refs were changed.
Grafana dashboard availability and the earlier qualification gaps were not
requalified in this deployment.

Local evidence: `/tmp/rail-integration-ci.log`,
`/tmp/rail-refresh-offline-tests.log`, `/tmp/rail-merge-helm.log`,
`/tmp/rail-integration-chart-provision.log`, `/tmp/rail-integration-cd.log`,
and `/tmp/rail-integration-cd-recheck.log`.
