# Polymarket execution core

Candidate: **v3.4.0-local.2**. Rail and OMS contracts remain **1.0.0**. This implements the shared execution library and recovery boundary. The generic controller and API process registration now call this library; UMR routing cutover, deployment and funded qualification remain later work. New-contract definitions never enter a legacy strategy runner. See the API contract for read-only evaluation and durable request identity.

## Entry and ownership

`ExecutionService::execute(Candidate)` is the one application entry for qualified UMR and explicit synthetic API candidates. Construction uses the existing Store, BtcRepository, BookRegistry and, for live execution, the concrete process-bound LiveVenue. No alternate signer, HTTP client, market stream, model runtime or ledger is introduced. Concrete live binding prevents a wrapper from silently losing venue capability hooks.

The application validates durable enabled intent, current compiled configuration identity and the existing run manifest before a new submission. An exact persisted replay is observed without resubmission, including after the process stops. A changed candidate or configuration cannot reuse the same client order ID. Historical orders retain their original process, run, account and configuration. Order status observation does not reassign ownership.

The OMS receives compiled settings and opaque ownership; it has no Store, process lookup or policy JSON parsing. Compilation identity is verified when constructing it. Quantities come exclusively from the compiled sizing policy. UMR candidates retain their qualified decision and existing reference execution seal. An increased execution limit requires fresh qualification rather than relaxing a model-owned decision. Synthetic candidates are explicitly `origin=api`, carry no fabricated model qualification, and use the same applicable execution checks. Configured risk inference reuses the existing risk package evaluator.

## Shared order sequence

1. Claim the account transaction lock and check idempotency and durable process/run authority.
2. Resolve the member order type and requested shares from policies.
3. Walk current books for that quantity using the existing depth/VWAP helper. Establish the observation clock after obtaining the book snapshot.
4. Reuse existing book, fee, edge and marketable-depth validators. Exit-book requirements apply identically in both modes. FAK cannot shrink the requested quantity merely to pass shared admission.
5. Evaluate collateral, aggregate exposure, open-market count, daily loss and configured high-water/loss-regime policies. Live domain evidence reuses canonical exposure and recognized-PnL readers: resolution releases market risk before redemption, while redemption remains a collateral constraint. Optional controls remain absent when unconfigured.
6. Seal ownership, configuration/candidate identities, policy evidence, fee rate, quantity semantics and provenance into the existing OrderRequest metadata.
7. Revalidate current market evidence and capital adjacent to the actual POST or simulated arrival. Revalidation also checks the original sealed cash reservation; an improved quote cannot reduce the reservation for the still-authorized original limit.
8. Pass the admitted request to the domain adapter and observe canonical order/fill evidence through the shared lifecycle validator.

Reference selection and frozen feature qualification remain UMR responsibilities. Their policy values are carried in the existing UMR compatibility configuration; this library does not create another feature or reference-data pipeline. HTTP request authentication and process/run setup belong to the generic controller/application wiring.

## BUY quantities and price improvement

The integration baseline already supports BUY fills exceeding requested shares while remaining within authorized cash (see `rest_fill_backfill_accepts_price_improved_buy_shares_within_authorized_notional`). The new standard therefore explicitly seals `quantity_semantics=buy_cash_budget`:

- Requested shares remain the process sizing authority. The signed request remains a share-sized limit order through the existing SDK builder.
- Authorized BUY cash is requested shares multiplied by the sealed limit price. Actual fills can contain additional shares at improved prices. They must fit the existing cash authorization and one-micro-dollar rounding tolerance.
- Paper walks the same arrival book/depth haircut against that cash budget. FOK requires the complete cash amount; FAK permits a partial amount and cancelled remainder. Fill fees and settlement quantities use actual fills.
- Admission reserves the cash amount plus `cash amount × observed fee rate`. This bounds the existing dynamic fee formula under price improvement, including additional shares. It adds no configurable fee override.
- Participation limits apply to the requested order against displayed depth at its executable limit. The common gate runs again at arrival. Paper depth haircuts can still cause legitimate partial execution or nonexecution after shared admission.

Only new-contract metadata selects cash-budget paper matching and fee reservations. Unmarked historical and legacy paper orders retain their existing share-sized simulation and fee reservation behavior. No flat TPC parameter is removed or reinterpreted.

## Persistence, reservations and recovery

Account-scoped PostgreSQL transaction locks serialize admission across new services. The existing live venue retains its submission lock and durable pending-order claim before POST. Unknown live outcomes keep their pending reservation and reconcile before a retry; no automatic second POST occurs. Account evidence includes stopped owners and account identity sealed on new outstanding orders.

The Store shares an admission semaphore across its clones, reserving at least one pool connection for existing readers while execution leases wait. New execution needs a pool capacity of at least two. Reuse the application-owned Store when constructing services.

Paper uses the existing arrival simulator with an atomic Store transaction for the order, canonical fill identities and fills. No in-memory balance debit happens before or after that write: new paper capital is reconstructed from the existing durable ledger. A failed transaction changes no economic state; an uncertain commit is looked up by client order ID. The first persisted paper order records the initial collateral seed. Subsequent runs and changed configuration cannot seed another balance.

Both modes validate fill ownership, immutable identities and authorized economics. Duplicate evidence is not counted twice. Acknowledgement is not a fill. Cancelled FAK orders remain reserved until reconciliation establishes the result; a zero-fill result releases the opportunity without deleting its history. A persisted execution-resolution annotation lives in the existing order payload, separate from immutable request metadata.

`reconcile` and cancellation-aware `run_recovery` reuse existing settlement/reconciliation owners. The application owns the recovery task and its cancellation token. Transient failures retry on the next interval without modifying enabled/status. Configured live authorization is restored once per service instance through the existing restart method, conditional on current durable intent. Paper settlement discovery and idempotent recognition share the legacy implementation; new balances read credited ledger rows, while legacy callers retain their existing in-memory credit behavior.

## Qualification boundary

Deterministic tests exercise the OMS, actual paper arrival simulator and injected commit failures. Existing live transport/reconciliation tests remain the evidence for ambiguous POST, REST/websocket fill recovery and settlement invariants. This candidate has not executed the new service against a database or a funded venue. The PostgreSQL transaction SQL, cross-service lock scheduling and full service restart wiring require isolated development integration qualification; unit tests are not evidence of a real database rollback or funded execution. No new process or run is created by this implementation.
