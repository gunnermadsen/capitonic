# OpenAI Agent Paper Trading

Status: frozen implementation scope. Authentication qualification passed on 2026-10-01; trading-runtime implementation has not started.

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

Implementation is isolated on `feature/openai-agent-paper-trading` in the `openai-agent-paper-trading` worktree. Runtime deployment changes are limited to the Polymarket bot Helm chart. Docker Compose is outside scope. Image, application, chart, and Git provenance are created only through the repository's authorized golden image workflow.

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
