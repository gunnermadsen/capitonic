# Generic order API contract

Specified, not implemented by the configuration foundation. Keep current authenticated admin access controls when these routes are added; naming does not authorize public access.

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

An API-driven process must not schedule UMR, including after restart. Trigger selection cannot bypass durable process authority. UMR calls the same application service directly, never through HTTP. Account exposure includes other sleeves sharing that account; process identity alone is not capital isolation.

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

Authentication failures use the existing auth middleware. Malformed/unsupported configurations are 400, missing process/order is 404 (do not disclose a foreign order), conflicting idempotency is 409, unavailable unimplemented capability is 501, transient required-evidence/dependency unavailability is 503. A evaluated market/policy rejection is an explicit rejected receipt, not an internal-server error. Preparation diagnostics never expose credentials, signatures or a replayable signed payload. Controller responses must distinguish validation acceptance, submission and economic completion.
