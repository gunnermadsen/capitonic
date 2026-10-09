# Order API qualification

`qualify.py` is an opt-in external HTTP driver for the deployed application. It installs no code, switches, mocks or alternate contracts in the rail. It reuses the fixed local paper/live process pair through the existing API, preserves their new-contract policies, and audits existing tables through read-only PostgreSQL sessions. Its only order submission path checks both the owned paper process ID and its current configured mode. Live mode supports preview and signing preparation only.

Prerequisites: local Rancher Desktop `capitonic`, the qualified bot image, healthy required ingester evidence, and the existing bot admin token inherited from the main worktree environment. Never put that token in arguments or evidence. The driver always targets `http://localhost/api/bot`; SQL always uses `kubectl --context rancher-desktop`, `default_transaction_read_only=on`, and a five-second statement timeout. No production target is supported.

Reuse these process identities for every qualification:

- Paper: `b4171f11-9fde-45e0-ad8c-37f6954769ec`.
- Live: `bd63f9ce-e58b-4395-aee2-aab3ce197621`.

`prepare` requires both processes to be stopped, reads their current definitions,
and patches only the requested order type and fresh immutable run/preregistration
identity. It never creates a process or resets capital/history. Missing or mismatched
processes stop qualification. FAK/FOK runs use this same pair sequentially.
Use a fresh external evidence directory for each qualification. Requests, receipts, definitions, image identity and assertions are retained in `run.json`. Never commit raw evidence. Invoke one action at a time and inspect failures before continuing:

```sh
python3 packages/polymarket-bot/tests/order_api/qualify.py prepare --evidence-dir /tmp/order-api-run --order-type fak
python3 packages/polymarket-bot/tests/order_api/qualify.py evaluate --evidence-dir /tmp/order-api-run
python3 packages/polymarket-bot/tests/order_api/qualify.py prepare-orders --evidence-dir /tmp/order-api-run --timeout 360
python3 packages/polymarket-bot/tests/order_api/qualify.py paper --evidence-dir /tmp/order-api-run --timeout 360
python3 packages/polymarket-bot/tests/order_api/qualify.py stop --evidence-dir /tmp/order-api-run
python3 packages/polymarket-bot/tests/order_api/qualify.py audit --evidence-dir /tmp/order-api-run
```

Use `--mode paper` on `prepare` to restrict setup, startup, preparation and execution
to the retained paper process. The selected modes persist in the evidence directory;
no live definition or lifecycle is changed in this mode.

Repeat in another evidence directory with `--order-type fok` to qualify that policy. `start` is only for finishing setup when configuration succeeded but startup did not begin; an explicit restart requires a fresh immutable run key through the normal process API. Do not rerun `prepare` over existing evidence.

Missing/stale evidence must not be worked around by loosening policies. Repeated unavailability stops preparation for diagnosis. Paper execution permits at most three attempts and one filled order per run. The request identity is saved before submission; after any uncertain response, recover that exact request/order before initiating another execution. The driver does not automatically retry ambiguous submissions. Never delete evidence or use a fresh directory to bypass uncertainty.

Stop the dedicated processes after a blocked run. Following a fill, retain maintenance until required settlement evidence is captured before stopping; settlement qualification itself is a later segment. Do not stop or change unrelated processes.

The driver covers actual HTTP and database boundaries; it does not replace deterministic Rust contract/fault tests. Successful preparation, paper fill, partial-fill scenarios and funded execution are distinct outcomes. A blocked preparation is not a paper execution pass. The `paper` action remains unqualified until run against a deployed candidate that passes preparation.

Offline driver boundary tests:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s packages/polymarket-bot/tests/order_api -p 'test_*.py'
```

Latency evidence: run `qualify.py latency --evidence-dir /tmp/order-api-run` after preparation/execution and **before stopping** the dedicated processes. It scrapes the existing bot Prometheus endpoint, retains only their series in `latency.prom`, and writes `latency.json` with stage counts/means/maxima, estimated percentiles, current book-age gauges, configured paper arrival delay and external HTTP timings. Capture again after a further bounded run if needed; each report represents the cumulative lifetime of these dedicated metrics owners, not an incremental delta. A changed pod identity/restart count/image invalidates cumulative qualification and is rejected. Do not combine restarted-owner samples.

Request timing uses a monotonic clock around HTTP request/response transfer, including HTTP errors and transport exceptions; it excludes driver preflight, SQL audit and evidence serialization. Replayed execute requests are reported separately. Older evidence without durations remains untimed. p50 requires 2 observations, p95 20, p99 100; these minimum counts do not certify statistical confidence. Sparse or histogram-overflow percentiles remain null. Server histogram percentiles are interpolated estimates; nested stages are not additive. A single fill provides diagnostic timings, not tail-latency qualification. Never increase order budgets or bypass unresolved-order recovery to obtain more samples.

Accounting SQL regressions use the same read-only local database reader and inline
CTE fixtures. They execute the actual repository queries without creating tables
or writing data. Enable them explicitly alongside the offline driver checks:

```sh
ORDER_API_SQL_TESTS=1 PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s packages/polymarket-bot/tests/order_api -p 'test_*.py'
```

These checks cover process ownership, cancelled/expired reservations, stopped
account sleeves, duplicate fills/credits, both fill modes, causal cutoffs, empty
processes and conflicting settlement economics. They do not submit orders.
