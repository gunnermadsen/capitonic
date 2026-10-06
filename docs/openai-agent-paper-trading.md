# OpenAI Agent Paper Trading

Status: development paper integration verified on 2026-10-06 in `feature/umr-openai-agent`: subscription authentication, actual order/fill/credited settlement, and restart recovery passed. This is not model qualification, golden-image acceptance, or production authorization.

## Objective

Add an `openai_agent` decision provider to the existing BTC Unified Model Runtime boundary so a paper trading process can request an early directional prediction from an eligible OpenAI model through Sign in with ChatGPT plan usage. The provider must reuse the existing process-owned data, decision, admission, order, paper venue, persistence, settlement, accounting, and observability paths.

## Ownership and compatibility

- `trading_processes.process_id` remains the canonical owner of configuration, decisions, execution, and outcomes.
- Existing process-level `sources` remain the only market-data selectors. The agent reads the bot's shared causal runtime state and creates no feed or gRPC connection.
- Existing model selectors and process schema versions retain their meanings and resume behavior.
- A new backward-compatible process schema version adds an `openai_agent` router selection identified by an immutable `profile_key` and `profile_sha256`.
- The immutable agent profile owns the OpenAI model ID, instructions, request and response contracts, attempts at seconds 45, 75, and 105, the request timeout, and a hard decision ceiling of second 120.
- OAuth credentials are installation infrastructure and never appear in trading-process configuration.
- The first implementation is paper-only. Process activation rejects `openai_agent` when effective execution mode is live.

## Runtime boundary

The asynchronous OpenAI provider lives beside the synchronous `ModelAdapter` capability inside the bot's UMR boundary. It returns a typed directional evaluation into the existing `BtcDecision` and `ApprovedIntent` pipeline. It cannot submit orders, create streams, write a separate ledger, or choose paper versus live execution.

A valid agent direction follows the existing entry-admission, risk, decision authorization, order planning, `ExecutionVenue`, `PaperVenue`, fill, settlement, and accounting paths. Provider errors defer only the affected opportunity and do not disable the durable process.

## Prediction contract

- Every eligible market receives an attempt.
- The provider must return `up` or `down`, probability, confidence, and bounded reason codes.
- The first valid response wins. Later configured attempts are retries only after a transport, authentication, capacity, timeout, or response-validation failure.
- No request starts after second 120.
- One provider request may be in flight for a process and market, and existing process/market entry uniqueness remains authoritative.
- The cloud request contains a bounded, versioned, causal snapshot rather than a continuous raw stream.

## Authentication

One installation-scoped Sign in with ChatGPT credential profile serves all `openai_agent` processes. Initial authorization uses an interactive loopback PKCE flow. The runtime validates the ID token and granted `chatgpt.tokens.use.direct` scope, discovers eligible models, serializes refreshes, and atomically persists the complete rotating credential set in encrypted durable storage. Authentication outages produce visible provider-deferred decisions and never trigger an API-key or model fallback.

## Persistence and observability

No database migration or administrative database mutation is expected. Agent evidence is stored as versioned `agent_evaluation` metadata in the existing strategy-decision record. Existing process-scoped UMR, decision, execution, fill, settlement, and P&L instrumentation remains authoritative; no agent-specific ledger or monitoring service is introduced.

## Deployment and release

Implementation is isolated on `feature/umr-openai-agent` in the `openai-agent-paper-trading` worktree. Bot image SemVer base is `3.3.0`, selected through the existing release manifest; Cargo package version remains `0.1.0`. Local CI and CD retain their existing versioning rules and pin the exact built candidate and embedded revision. Runtime changes use the bot and Grafana owning charts. Docker Compose and database migrations are outside scope. Feature verification deployment does not confer golden status or authorize integration merging or production promotion.

The v5 router selects `openai_agent` with `profile_key: btc-5m-openai-agent-v1`; definition writes resolve its immutable `profile_sha256`. Required sources are market contracts, orderbooks, resolutions, RTDS Chainlink and Binance one-second OHLCV. Polygon Chainlink oracle and Binance futures open interest are optional causal context. TWAP is not an inference input: its existing runtime window is display-only. Context is bounded to available RTDS observations and closed Binance candles over the preceding 60 seconds, plus the market/execution snapshot and optional last available oracle/open-interest observations. No SSD query or extra stream subscription is created.

Installation bootstrap uses the maintained `openai-agent-auth` executable with `POLYMARKET_OPENAI_CREDENTIAL_PATH` pointing to the credential volume and `POLYMARKET_OPENAI_CREDENTIAL_KEY` inherited from the main runtime `.env`. It prints a loopback authorization URL for manual browser approval, validates identity and scopes, checks the pinned model and atomically saves encrypted credentials. `--check` verifies credentials and model access. Helm `openaiAgent.enabled` mounts the retained credential PVC and its encryption-key Secret; credentials never enter a process definition. Runtime refresh is serialized and survives cancellation of a market request.

Agent requests, provider failures, pending work, inference latency and decision timing use process-scoped UMR metrics. The existing UMR dashboard and provisioned Grafana alert rules cover authentication, capacity, sustained absence of predictions, latency and deadline violations. Paper orders, fills, settlement and P&L remain on existing instrumentation.

## Authentication qualification gate

Before trading-runtime implementation, a disposable local OAuth bootstrap and one-shot Kubernetes Job must prove:

1. Dynamic client registration and loopback PKCE authorization complete for the selected ChatGPT account.
2. The granted scopes include `chatgpt.tokens.use.direct`.
3. Kubernetes can reach `https://api.openai.com/v1` over TLS.
4. `GET /v1/models` returns at least one eligible model.
5. A `store: false`, `stream: true` Responses request reaches `response.completed`.
6. Credentials are neither committed nor printed in Job logs.

Failure of this qualification gate stops the feature before UMR, process schema, execution, database, or permanent Helm changes.

### Qualification result

The disposable probe completed from Rancher Desktop k3s in the `capitonic` namespace:

- Sign in with ChatGPT dynamic-client authorization completed with a validated ID token.
- The granted scopes included `chatgpt.tokens.use.direct` and `offline_access`.
- `GET https://api.openai.com/v1/models` returned five account-visible models.
- The probe selected the first server-ordered visible model, `gpt-6-astra`.
- `POST https://api.openai.com/v1/responses` used `store: false` and `stream: true` and reached `response.completed`.
- The successful Job had no service-account token, database credentials, ingress, market-data access, or trading access.
- The temporary Kubernetes Job and Secret were deleted and the test OAuth credential was revoked after evidence collection.

An initial disposable attempt rejected a newline-terminated authorization header and included part of that header in its error log. The affected Job and Secret were deleted immediately, both tokens were revoked, error output was hardened, and the successful qualification used a newly authorized credential.

## Verification boundary

Implementation verification must cover prior-schema compatibility, paper-only activation, source reuse, causal input construction, scheduling and idempotency, provider error recovery, actual `PaperVenue` order/fill/settlement persistence, restart recovery, and unchanged existing-model behavior. No new execution or paper simulation pathway is permitted.

## Development verification — 2026-10-06

Local CI built `v3.3.0-local.5` from `6f1b05c2f209a5c41003dade6f61d3337f585aa4`; local CD deployed image `sha256:5858b656748d6e746a7e5b9c10e5164d4cb5992c7a3ad00cf2bbbe88301b8d75` through bot chart `0.4.7` (Helm revision 34). Formatting, Clippy, component tests, model packaging tests, and Helm lint/render passed. Cargo remains `0.1.0`; CI/CD scripts, workflows, database migrations, and Compose files are unchanged.

Process `057d0d78-d355-4e64-8894-b74a8ef36eb3` owns paper run `c94d8741-7da1-577c-83bc-8c31680efe50`, frozen hash `5dd3bdcd4961f2e9ade200c0875483f426b1324d0fa137c872f5ca0ab0375e5e`. Its required and optional sources are those listed above. All nine enabled processes were running with fresh heartbeats; worker capacity was 14 healthy workers for nine desired realtime profiles and no active backfill shards. Migration ledger remained 134 entries, latest timestamp `1791043201000`.

The agent forecast UP with probability `0.63`, inference latency `6.164497753` seconds, and execution at market second `56.175`. Existing order `paper-f99ff353-c30e-5783-b6ed-6d30045d1f88` filled five shares at `0.52`; fill `2f406256-5192-5752-adef-a78ed46e3301` recorded source `paper`, notional `2.60`, and fees `0.08736`. Official UP resolution for market `5330853` arrived at `16:17:16.199933 UTC`. The existing settlement ledger credited payout `5.00` and net paper P&L `2.31264`, matching process/member UMR metrics. One result proves plumbing, not positive expectancy.

A normal restart after the fill resumed the same run/hash and preserved exactly one entry for that market. Advancing RTDS and Binance timestamps were verified. The next market produced a natural paper order rejection for `stale_reference_execution_evidence`; the agent remained enabled. Existing per-order freshness checks were not weakened. Prometheus scrapes agent metrics, the UMR dashboard includes agent panels, and all five agent alert rules are provisioned. The latency warning was still firing over its historical 15-minute window at the final check; no other firing alerts were observed.

### Problem

Initial development candidates exposed incomplete agent compatibility with subscription streaming, reference evidence, and directional decision persistence.

### Impact

Only the new agent paper process failed functional admission; the eight pre-existing processes remained running. Failed run identities and rejected candidate provenance were retained.

### Cause

Subscription output arrived in text deltas with an empty terminal output array; async completion could coincide with missing fresh Chainlink lineage; the existing signed-edge persistence allowlist omitted the agent strategy.

### Immediate repair

1. Applied bounded completed-stream parsing, agent-only reference admission, deadline-bounded completed-forecast retention, and explicit agent support in the existing directional persistence validator.
2. Ran `scripts/local-image-ci.sh polymarket-bot --build`, pinned its exact image with `scripts/local-image-cd.sh pin polymarket-bot v3.3.0-local.5`, and deployed with `scripts/local-image-cd.sh deploy polymarket-bot`.
3. Restarted only the failed agent through `/admin/trading-processes/057d0d78-d355-4e64-8894-b74a8ef36eb3/start` after updating its next run key through the established API.

### Verification

Actual paper fill and credited official settlement, persisted forecast attribution, unchanged migration state, fresh feeds/heartbeats, automatic restart recovery, no duplicate market entry, and process/member P&L parity passed as recorded above.

### Rollback status

No rollback was needed for the repaired final candidate. The predeployment tuple for `.5` is retained under `local-image-cd.0oAveb` in the host temporary directory; its prior image was `.4` (`sha256:e7a06ec12395b28a2d4aab6c026ae28362b86a444f5962003d46ead52f1556c2`, revision `0c9c73927b15b4a3bac88763b2a991ae3ebdaedc`), which is rejected for agent persistence compatibility and is not an acceptance target. The repaired `.5` image remains deployed; no integration merge, development promotion, or production mutation occurred.

### Permanent hardening

Focused tests now cover streamed terminal responses, missing/stale reference rejection, existing guard construction, bounded forecast retention/rollover, and signed directional edge persistence for the agent. No parallel execution, data, or accounting system was added.
