# Generic order API contract

Implemented by candidate v3.4.0-local.2. Application-relative routes are registered at the root; deployment ingress prefixes remain external to this contract. Keep current authenticated admin access controls when these routes are added; naming does not authorize public access.

| Route | Contract |
| --- | --- |
| POST /orders/{process_id}/preview | Resolve process configuration and current evidence; evaluate candidate. No executable order, reservation, signing or submission. |
| POST /orders/{process_id}/dry-run | Preview plus supported preparation/serialization/signing diagnostics. No submission or economic mutation. |
| POST /orders/{process_id}/execute | Revalidate current evidence, reserve and execute through the shared application service. |
| GET /orders/{process_id}/order/{order_id} | Verify process ownership and return durable status/evidence without reinterpreting through the current TPC. |

The existing `/admin/live/order-dry-run` remains unchanged during coexistence. Reuse its appropriate signing/preparation primitives behind the new adapter; do not route the new API through the legacy controller or copy policy logic. Preserve its useful signing identity/redaction diagnostics. Retire it only after caller migration and legacy retirement acceptance.

## Controller separation

The controller handles HTTP/auth/request shape and delegates. It contains no exchange names or paper/live dispatch branches. The application service reads the process/TPC, checks durable authorization, invokes the versioned compiler and selects an internal execution implementation. The OMS receives immutable typed settings and an opaque ownership context; it cannot read process records or raw TPC JSON. A registered test implementation must work without modifying controller code.

Request inputs identify the candidate instrument/action/member and idempotency identity. Exchange, mode, credentials, account and policy overrides are forbidden. Quantity follows the sizing policy, not an unconstrained caller field. API candidates are synthetic, never model approvals. Real causal evidence is required for applicable risk/admission; inapplicable model checks are explicitly reported, not silently passed. The request envelope below is the version-1 controller contract. Its implementation must not add provider-specific fields or bypass the configured member policy.

A rail-selected process schedules UMR independently of authenticated API availability, including after restart. API requests neither select nor suppress UMR; they call the same application service without passing through the model runtime. Account exposure includes other sleeves sharing that account; process identity alone is not capital isolation.

## Results, idempotency and errors

Receipts distinguish preview evaluation, preparation performed, submission accepted, acknowledgement, partial/complete fill, definitive rejection and unknown outcome. Include ownership, operation/order identity, config hash, origin and policy/evidence results. Dry run reports adapter preparation capability (performed, unavailable, inapplicable); paper never fabricates a signature. Redact private keys, credentials, owner tokens and signatures. Preview/dry run do not create executable durable orders, consume execution idempotency keys or reserve capital. Logging/metrics are permitted.

Execute revalidates and prepares a new submission; a successful preview or signed dry-run payload is not durable authorization. Same process + idempotency key + payload returns the original operation; conflicting reuse returns conflict. Lookup the original operation before applying a newer TPC on replay. HTTP acknowledgement is not final execution success. Unsupported configuration/capability is explicit, never a fallback rail. No API can bypass a disabled process.

## Required verification

Authentication, malformed requests, ownership mismatch, unknown/unsupported configuration, missing evidence, unavailable preparation, redaction, retry/conflict, historical retrieval after configuration change, and no economic effects for preview/dry run. HTTP tests use normal application wiring with deterministic adapters; funded tests are separate and explicitly authorized. Existing legacy HTTP tests continue to pass.

## Version-1 request and receipt envelopes

Preview and dry run accept this strict candidate envelope:

```json
{
  "version": 1,
  "candidate": {
    "member_id": "conservative",
    "market_id": "<existing market identity>",
    "outcome": "up",
    "synthetic_prediction": { "probability_up": "0.80" }
  }
}
```

Execute uses the same envelope plus a required UUID `idempotency_key`. These are synthetic entry candidates: the Polymarket contract resolves the token from market/outcome, BUY side, quantity and order type from configuration, and executable prices/fees from real evidence. No arbitrary caller price, account, quantity, policy, authorization or implementation override. Probability is an explicit synthetic input in [0,1], not a claim of model qualification; confidence is derived consistently, not independently supplied. Existing legacy signing diagnostics retain their broader raw-order inputs unchanged. General exit/liquidation commands are not introduced by this entry-order contract.

Application response fields are `version`, `process_id`, `operation_id`, nullable `order_id`, `origin` (server-assigned `api`), `config_sha256`, `status`, `policy_results`, and `evidence`. Policy results identify name/version, applied/rejected/not_applicable, bounded reason and effective owner. Preview returns evaluation and no order ID; dry run adds `preparation` with performed/unavailable/inapplicable steps and redacted diagnostics. No signer-specific object is interpreted by the controller. Execute returns an existing operation on replay; asynchronous acceptance is explicitly not a fill. Retrieval adds current lifecycle/fill/accounting evidence from the existing durable representation. API schema version is independent of rail/OMS versions.

Authentication failures use the existing auth middleware. Malformed/unsupported configurations are 400, missing process/order is 404 (do not disclose a foreign order), conflicting idempotency is 409, unavailable unimplemented capability is 501, transient required-evidence/dependency unavailability is 503. An evaluated market/policy rejection is an explicit rejected receipt, not an internal-server error. Preparation diagnostics never expose credentials, signatures or a replayable signed payload. Controller responses must distinguish validation acceptance, submission and economic completion.

## Application wiring and verification boundary

The controller delegates to `OrderApplication` through the existing `ControlApi`; it cannot inspect a TPC or select a venue. `RuntimeControl` supplies the existing process manager implementation. A deterministic alternate implementation is exercised through the same authenticated router in HTTP tests. API mode, quantity, member order type, fees and gates come from compiled process policies. An unknown member is a 400 response.

Use the existing process start endpoint for a rail-selected definition. Start/resume preserves the current lifecycle and immutable run ledger, registers the order service on the existing active playbook, and uses the existing shared market-data and UMR runtime. Authenticated API overrides remain available through the same order service while UMR inference is active. Startup dependency failures preserve durable enabled intent and retry through the existing manager maintenance pass. Legacy controllers and runners retain their previous paths.

Preview/dry run require an enabled, running API process and current canonical market evidence. They read accounting and evaluate the OMS without taking a reservation or writing an order. The current binary-market adapter accepts the market present in its shared runtime snapshot; a different/missing market returns 503. Prediction probability is synthetic; the existing causal repository builds real market/fee/reference evidence. The selected outcome determines probability (`p_up` or `1-p_up`). Optional configured risk evaluation uses that real evidence.

Dry run uses the existing live signing/preparation primitive and an explicit diagnostic allowlist. No signed payload, owner token, signature, credential, or private key reaches the response. Paper reports signing as inapplicable and serialization as performed. Execution does not reuse a preview or dry-run authorization: it rechecks process configuration and enabled intent, including at arrival/pre-POST.

A terminal API policy rejection with observable price evidence is stored as a rejected order using the existing ledger. Its metadata says `intent_prepared=false` and `price_kind=observed_unapproved_ask`: the recorded ask is evidence, never a submitted or executed price. No fill or reservation is created. This preserves retry/conflict semantics without a second operation ledger. Missing required evidence returns 503 without fabricating a priced order. Invalid request/configuration or unavailable dependencies do not claim an operation identity.

Replay checks the original process and canonical request hash before loading current configuration or requiring an active runtime. It can return the original order after a process stops or its TPC changes, without resubmission. Decimal-equivalent probabilities share an identity; changed payload or owner conflicts. Read-only lookup returns canonical fills, persisted lifecycle resolution, settlement/accounting evidence and applicable paper arrival evidence. Acknowledged/unknown states are not represented as filled.

Automated evidence covers authenticated HTTP dispatch, forbidden overrides, status/error mapping, ownership-safe lookup, canonical replay identity across configuration changes, configuration-to-runtime policy preservation, policy reports, preparation redaction and non-executable rejection records. These tests do not claim a database-backed HTTP execution or funded trade. Those remain development qualification, and this candidate is not activated or deployed by its local CI build.

## Process lifecycle control

Process-scoped `btc_runtime.live_status` includes optional `reconciliation` diagnostics
from the last durably recorded live reconciliation pass: `run_id` (the existing
`live_reconciliation_runs` identity), `process_id`, `account_ref`, `checked_at`,
`lookback_hours`, the existing `process_accounting_proof`, and separate counts for
unmatched account trades, position mismatches, foreign open orders, unresolved orders
and aggregate mismatches. Counts describe the bound account's reconciliation evidence;
they do not attribute unmatched wallet activity to the requesting process. Detailed
position identities remain in that referenced report. No additional database read or
write is performed to serve status.

The `live_process_accounting_not_proven:` reason prefix now carries the proof's
specific reason, such as `unmatched_account_trades`, or
`awaiting_initial_reconciliation` before a report exists. Optional
`reconciliation_error` preserves the current reconciliation error even when another
readiness reason has precedence. The snapshot's timestamp is essential: a retained
completed report does not imply the latest attempt succeeded or its evidence is fresh.
The existing `idempotency_clean` field remains an aggregate reconciliation-safety
compatibility field; false does not itself establish duplicate submission.

These fields explain existing decisions and do not authorize orders or change any
gate. Paper omits these live-only diagnostics and retains `paper_mode` /
`not_applicable`. Shared OMS policies, API order contracts, paper/live accounting,
metrics and process authorization remain unchanged.

Authenticated `PATCH /admin/trading-processes/{process_id}/status` accepts a body such as `{"status":"completed"}`. Supported targets are created,
starting, running, stopping, stopped, failed, completed and expired. It returns
the existing process response envelope.
Unknown fields/statuses are rejected. Existing GET status and POST start/stop/complete
remain compatible and share the lifecycle manager's serialized transitions.

Starting/running requests use existing startup or durable restart recovery and
resolve to running on success. Repeated running requests reuse the owned runtime.
Stopping resolves to stopped. Inactive targets quiesce any owned runtime before
persisting the requested state; shutdown failure resolves to failed. Previous
status alone does not prohibit an operator transition. Identity, immutable run
ownership, startup checks and generation-scoped callback protections remain active.
A pending terminal write must finish before a subsequent requested transition;
failed persistence returns an error, never a successful but unwritten status.
Repeated requests for an already consistent inactive state preserve timestamps.

Loss-limit changes still use the existing process configuration PATCH while
inactive. Legacy `execution.max_daily_loss_usd` and rail
`loss_limit_policy.params.max_daily_loss_usd` accept null or positive representable
Decimal values without a hardcoded ceiling. Legacy omission remains supported;
rail policy objects retain their existing required, nullable parameter keys. Order admission blocks
when the configured UTC daily realized PnL is at or below the negative limit.
This checks already realized loss; it is not a hard cap on an admitted order's
possible subsequent loss. Runtime configuration remains bound until restart.


Dry-run identity diagnostics include `signer_matches_funder`, copied from the existing
live diagnostic alongside `signer_matches_client`. Poly1271 order signers identify
the deposit wallet/funder, so signer-to-client inequality alone is not an identity
failure. This additive redacted boolean changes no signing or execution behavior.
