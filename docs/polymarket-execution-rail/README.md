# Polymarket execution rail standard

Status as of 2026-10-14: shared execution core, UMR routing, and authenticated API override wiring are implemented on the rail-selected process path. Rail contract **1.0.0**, OMS contract **1.0.0**, selector `{ "name": "polymarket", "version": 1 }`. Constants live in `packages/polymarket-bot/src/execution/polymarket/mod.rs`. Execution qualification is recorded separately from contract versioning. Application image versions use existing image provenance, not the unrelated Cargo package version.

This is the authoritative specification for the additive Polymarket order standard. Existing model and agent trading, execution, ingester contracts, database schemas, release workflows and legacy dry-run behavior remain unchanged. New definitions use the existing process API. Selecting the Polymarket rail starts the UMR and shared order service together; authenticated API overrides remain available without another TPC selector. No account access or order submission occurs during configuration compilation; API execution still requires a running, enabled process and matching immutable run.

## Ownership

| Boundary | Owns | Must not do |
| --- | --- | --- |
| Generic order controller | HTTP/auth integration, validated requests and responses | Dispatch by exchange/mode, parse TPC policies, evaluate gates |
| Execution application service | Process lookup, durable authorization, compiler invocation, internal dispatch | Implement another model/agent runtime or duplicate admission |
| Configuration compiler | Strict versioned parsing, existing UMR compatibility validation, resolved settings/hash | Submit, connect to an account, start a runner, change durable intent |
| Polymarket OMS | Shared order evaluation, quantity-aware admission, reservation, intent preparation | Read `trading_processes`, parse JSON, schedule inference |
| Execution adapter | Signing, transport, simulation, domain evidence | Bypass shared admission |
| Shared lifecycle/repositories | Order transitions, fills, reservation recovery, reconciliation, accounting | Add another ledger or silently reinterpret historical orders |

`trading_processes.process_id` is the canonical owner. Execution receives an opaque ownership context plus resolved account/configuration identity, never a process object. UMR and API synthetic candidates use the same application entry point. UMR does not make an HTTP call. The term rail stays internal; it does not occur in external order endpoint paths. No Kraken/futures implementation is included.

## Contracts and invariants

1. UMR owns frozen features, inference, qualified prediction and artifact requirements. A synthetic API candidate is explicitly synthetic, never a fabricated UMR prediction or qualification receipt.
2. Order intent identifies candidate provenance, target instrument/outcome, member and proposed economic action. Process configuration is the sole requested-quantity authority. Dynamic sizing remains unsupported until its decision/fallback contract is specified.
3. The OMS resolves quantity before execution VWAP, fees, edge, risk and capital checks. Any material change requires re-evaluation. Historical `qualified_trade_size` is provenance, not sizing authority.
4. Execution intent seals the approved request, policy decisions, evidence timestamps, configuration identity, ownership and idempotency identity. Reuse existing `models::OrderRequest`, `OrderType`, `OrderState` and repository representations; do not create a replacement persisted order.
5. Submission acknowledgement is not a fill. Adapter observations feed one reducer. Duplicate/out-of-order evidence cannot duplicate economic effects. HTTP and websocket evidence must converge.
6. FOK requires the full authorized execution semantics; FAK permits partial fills and cancellation of the remainder. Share bounds constrain requested quantity, not a minimum FAK fill. New BUY intents explicitly use the existing live cash-budget semantics, including price-improved fill quantities and a conservative fee reservation; see the execution core contract. Legacy paper semantics remain unchanged.
7. Unknown submission outcomes are reconciled before retry. Definitive rejection and zero-fill completion are distinct from ambiguity. An opportunity must not remain incorrectly sealed after a definitive nonexecution.
8. Transient evidence/infrastructure failures affect only the unsafe action and recover automatically. No manual re-enable ritual, durable kill switch, or cross-process disablement is added.
9. Same inputs and configuration produce the same applicable shared-stage decisions. Different market observations need not produce identical fills. Paper fill optimism is measured, not hidden.
10. Historical metadata remains readable. No new tables, migration or parallel account/decision ledger is authorized.

## Deliberate adapter boundaries

Paper/live differ in credentials/signing; exchange transport versus simulated matching; arrival and liquidity evidence; account/balance evidence; actual venue rejection; reconciliation sources; and wallet settlement/redemption versus simulated credit. Their common policy sequence, intent representation, reservation semantics, state transitions, fees and accounting use shared machinery. Paper primary fills and stress previews remain distinct; previews never mutate the primary ledger. Starting collateral initializes the paper account once, not on restart.

## Standard references

- [Execution core and recovery](execution-core.md)
- [Configuration and parameter ownership](configuration.md)
- [Generic order API](api.md)
- [Instrumentation and dashboard contract](instrumentation.md)
- [Implementation acceptance and rollout](implementation.md)
- [Verification and limitations](verification.md)
- [Existing UMR standard](../unified-model-runtime/README.md)

Changes to meanings require an appropriate contract version and regression evidence. An execution feature must extend its owning contract, not introduce a second controller, gate registry, model system or persistence path. The broader TPC reorganization is deferred.
