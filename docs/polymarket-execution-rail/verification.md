# Configuration foundation verification

## Acceptance scope

This implementation covers frozen-plan phases 1–2 only: standards, mappings, baseline regression and additive TPC admission/compiler. It does not claim a working OMS, new order HTTP routes, paper/live execution parity, funded qualification or deployment. Compiled definitions remain inactive and cannot reach the legacy runner. No database was mutated for verification; API admission is tested through the application validator with a lazy, unconnected Store. Actual database round-trip execution and controller E2E belong to subsequent qualification.

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
