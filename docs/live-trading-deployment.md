# Live trading deployment

## Runtime contract

Paper and live BTC processes are first-class modes of the same managed
`btc_5m/realtime_paper` process runtime. They use the same strategy parser,
market-data inputs, model inference, entry admission, order identity, persistence,
settlement, and process lifecycle. `config.execution.mode` selects the venue;
it does not select a separate strategy implementation.

Live trading requires all of the following:

- an immutable runtime model whose manifest and embedded deployment metadata agree;
- `live_capital_allowed=true` and either a `development_live_pilot` scope with
  `production_qualified=false`, or a non-paper production scope with
  `production_qualified=true`;
- matching model, artifact SHA-256, and feature-schema SHA-256 identities in the
  process selection;
- `execution.mode=live`, a bounded `account_ref`, and equal
  `execute_signals`/`live_capital` flags;
- configured signer, API credentials, signature type, funder identity, CLOB and
  user-stream endpoints;
- successful authenticated balance/allowance and open-order reads, positive
  collateral, a clean process-scoped reconciliation, and healthy market evidence;
- the existing per-order marketability, depth, freshness, fee, capital, identity,
  accounting, and order-safety checks.

The runtime rejects a paper-only model for live capital. Editing only the process
metadata cannot grant eligibility because the model loader validates the immutable
manifest, embedded model metadata, and hashes. Runtime secrets remain in the main
worktree environment files. Non-secret trade parameters belong to the durable
process playbook.

### Execution controls

Execution controls are flat, optional process properties. Omission means the
corresponding additional limit is not configured.

- `max_order_notional_usd` limits one requested order in paper and live venues.
- `require_exit_book` requires a current, internally consistent opposing outcome
  book in paper and live venues.
- `max_open_notional_usd`, `max_open_positions`, and `max_daily_loss_usd` are live
  account-capital protections backed by authenticated reconciliation evidence.
- `account_ref` binds live ownership, reconciliation, orders, fills, positions,
  and accounting to one configured account. It is required for live and forbidden
  for paper.

Normal restarts preserve durable enabled intent. Transient dependency or evidence
failures block only unsafe actions and recover automatically. A live process runs
until explicitly stopped or completed; deployment must not add a timer or automatic
completion mechanism.

## Paper-to-live model and process cutover

Promote an established paper process without changing model behavior or creating a
parallel execution system:

1. Record the source `process_id`, process key, model key, artifact hash, feature
   schema hash, strategy parameters, and recent paper evidence.
2. Use `core-promote-runtime-live-pilot` on the immutable paper runtime directory.
   The command creates a new model identity and hashes with scope
   `development_live_pilot`, `production_qualified=false`, and
   `live_capital_allowed=true`. It must preserve all model fields other than model
   identity and deployment authorization, and preserve every golden vector.
3. Add the exported model and one inactive live-process request template on a
   feature branch from the current integration branch. Do not modify the source
   paper process or reuse its `process_id`.
4. Retain the source strategy, prediction policy, feature contract, target size,
   timing, price, confidence, edge, reserve, depth, freshness, and fee parameters.
   Change only the model identity, venue, account binding, and explicit live-capital
   controls.
5. Commit and verify the artifact, then build only `polymarket-bot` with
   `POLYMARKET_GIT_REVISION` set to that commit. Record the immutable image ID and
   model provenance.
6. Deploy the exact image without rebuilding or changing unrelated services. Keep
   the new live definition inactive with both trading flags false.
7. Run the process-scoped `live-preflight`. It must prove credential connectivity,
   positive collateral, balance/allowance and open-order access, account identity,
   zero reconciliation mismatches/unresolved items, process accounting, and that
   order submission remains disabled. Run the signing-only order dry-run against a
   current outcome token; it must not submit an order.
8. After preflight succeeds, update the same stable process key with both trading
   flags true, preview the start, and start that `process_id`.
9. Verify the intended image/revision, process status, process-bound live identity,
   clean reconciliation, connected user stream, current market routes and evidence,
   and advancing decision timestamps. Absence of a trade is not a failure for a
   selective model. A trade is required to satisfy every frozen model, economic,
   market, capital, identity, and accounting gate.
10. Continue bounded monitoring for orders, fills, reconciliation, settlement,
    accounting, process recovery, and attributable alerts. Complete the process
    only after explicit operator instruction.

## BTC conservative selective live pilot

The process `BTC 5m conservative selective live pilot` is derived from source
process `469d761a-0918-43a9-b708-37ce23ab20dc` (`BTC 5m conservative selective
paper`). The source paper process remains unchanged.

The live artifact is
`btc-5m-conservative-selective-development-live-pilot-20260921-v1` with model
SHA-256 `a26e1a7fbc014c362ec20251b6242c1953e8a59a0c34dfab2912a90c459405c7`
and feature-schema SHA-256
`5dceedc374621f50e844ddf43cea5f58068c4dc70c36169c037dbb2691c7f39a`.
Promotion preserves the five-share strategy, 30-210 second decision schedule,
0.88 confidence floor, 0.10 stressed-edge floor, and 0.55 maximum share cost.

The pilot binds `polymarket-primary` and configures a $3 maximum order, $3 maximum
open notional, one open position, $3 daily realized-loss limit, and a required exit
book. These controls limit exposure without changing the model or completing the
process. Before deployment, the source paper process had 40 credited settlements,
35 wins, five losses, $97.05 entry notional, and $74.479785 net PnL. This evidence
supports a bounded real-capital pilot but does not claim production qualification.

Credential diagnostics confirmed readable API keys, balance/allowance and open
orders, zero open orders, POLY_1271 wallet configuration, and $13.202018 collateral.
The deployment is successful when the new process is running, authenticated,
reconciled, receiving current evidence, and eligible to submit an order whenever
all strategy and safety gates pass. Immediate trade frequency is not a deployment
success criterion.
