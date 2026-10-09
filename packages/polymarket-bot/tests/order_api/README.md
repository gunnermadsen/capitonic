# Order API qualification

`qualify.py` is an opt-in external HTTP driver for the deployed application. It installs no code, switches, mocks or alternate contracts in the rail. It creates isolated process definitions through the existing API, uses the versioned configuration examples, and audits existing tables through read-only PostgreSQL sessions. Its only order submission path checks both the owned paper process ID and its current configured mode. Live mode supports preview and signing preparation only.

Prerequisites: local Rancher Desktop `capitonic`, the qualified bot image, healthy required ingester evidence, and the existing bot admin token inherited from the main worktree environment. Never put that token in arguments or evidence. The driver always targets `http://localhost/api/bot`; SQL always uses `kubectl --context rancher-desktop`, `default_transaction_read_only=on`, and a five-second statement timeout. No production target is supported.

Use a fresh external evidence directory for each qualification. Requests, receipts, definitions, image identity and assertions are retained in `run.json`. Never commit raw evidence. Invoke one action at a time and inspect failures before continuing:

```sh
python3 packages/polymarket-bot/tests/order_api/qualify.py prepare --evidence-dir /tmp/order-api-run --order-type fak
python3 packages/polymarket-bot/tests/order_api/qualify.py evaluate --evidence-dir /tmp/order-api-run
python3 packages/polymarket-bot/tests/order_api/qualify.py prepare-orders --evidence-dir /tmp/order-api-run --timeout 360
python3 packages/polymarket-bot/tests/order_api/qualify.py paper --evidence-dir /tmp/order-api-run --timeout 360
python3 packages/polymarket-bot/tests/order_api/qualify.py stop --evidence-dir /tmp/order-api-run
python3 packages/polymarket-bot/tests/order_api/qualify.py audit --evidence-dir /tmp/order-api-run
```

Repeat in another evidence directory with `--order-type fok` to qualify that policy. `start` is only for finishing setup when creation succeeded but startup did not begin; an explicit restart requires a fresh immutable run key through the normal process API. Do not rerun `prepare` over existing evidence.

Missing/stale evidence must not be worked around by loosening policies. Repeated unavailability stops preparation for diagnosis. Paper execution permits at most three attempts and one filled order per run. The request identity is saved before submission; after any uncertain response, recover that exact request/order before initiating another execution. The driver does not automatically retry ambiguous submissions. Never delete evidence or use a fresh directory to bypass uncertainty.

Stop the dedicated processes after a blocked run. Following a fill, retain maintenance until required settlement evidence is captured before stopping; settlement qualification itself is a later segment. Do not stop or change unrelated processes.

The driver covers actual HTTP and database boundaries; it does not replace deterministic Rust contract/fault tests. Successful preparation, paper fill, partial-fill scenarios and funded execution are distinct outcomes. A blocked preparation is not a paper execution pass. The `paper` action remains unqualified until run against a deployed candidate that passes preparation.

Offline driver boundary tests:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s packages/polymarket-bot/tests/order_api -p 'test_*.py'
```
