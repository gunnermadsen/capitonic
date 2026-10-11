# Funded paired order qualification

The maintained external `paired.py` runner extends `Qualification`'s existing HTTP,
read-only accounting reader and Prometheus reporting. It installs no rail code,
TPC schema, ledger, runtime switch or alternative execution path. `qualify.py`
continues to prohibit live submission. Offline tests never opt into real transport.
No CI/CD, Helm, image, UMR or ingester changes are required.

## Scope and acceptance

| Test | Required evidence | Failure or incomplete result |
| --- | --- | --- |
| Setup | Existing retained process IDs, API trigger, Polymarket selector v1, canonical policies, five requested shares, expected live account | Wrong identity, active process, unsupported contract or unexpected policy difference stops setup before PATCH |
| Shared preparation | Same synthetic candidate and token, five-share sizing, order type, matching shared policy decisions; current real evidence; no economic writes | Either mode rejects or dry-run fails: no execute POST |
| Live boundary | Process-bound status has clean idempotency, accounting proven, no unresolved orders, fresh reconciliation, connected user stream and enabled entries; signing identities match | Startup HTTP success alone never qualifies readiness |
| Paired execution | Concurrent calls to existing `/orders/{process_id}/execute`; different process-scoped keys, same candidate | Separate observed books/times are retained; no assertion of identical fills |
| Durable lifecycle | Order retrieval, operation identity, foreign ownership 404, canonical fill identities and source, debit delta equals fills plus fees, cash and price bounds | Acknowledgement, rejection, timeout, unknown state, or zero fills never proves successful execution |
| FAK/FOK | FAK allows partial fills; a terminal filled FOK consumes its full cash authorization within existing rounding tolerance | Real markets may not yield desired partial/failure scenarios; existing deterministic tests remain authoritative for those cases |
| Retry | Explicit replay uses identical saved bodies/keys and retains the original order ID; observation rechecks canonical fills/debit and one order per key | Changed identity or duplicate economics fails; unresolved transport outcome never generates a replacement key |
| Reconciliation/reservation | Durable resolution and absence from the existing reservation predicate; process-bound live status recorded after execution | Unhealthy reconciliation remains visible and prevents certification even if fills exist |
| Settlement | Official winning token, actual filled quantity, fees, payout, net PnL, one credited settlement and capital-credit delta | Missing settlement stays pending; a zero payout cannot prove positive credit |
| Latency | Existing process/mode-labelled Prometheus histograms and external HTTP timings, unchanged metrics owner | One pair is diagnostic; p95 remains null below 20 observations; no extra funded orders for sample collection |

Expected policy exception: the previously approved paper daily loss limit is $25,
versus $10 live. It is recorded, never copied to live or silently removed from the
report. Every other shared policy must match. Paper simulation parameters and live
account/signing/reconciliation remain intentional adapter differences.

The funded runner enforces a conservative per-order total cash ceiling outside the
rail using existing TPC price/fee limits, and checks actual debits afterward. The
example ceiling is **$5.500001**: five requested shares × $0.55 maximum price ×
(1 + existing maximum fee rate of 1), plus the existing micro-dollar rounding
allowance. This is a conservative bound, not a predicted charge. Current maximum
principal is $2.75. Do not describe five requested shares as a hard filled-share
ceiling: the existing BUY cash-budget contract permits price-improved extra shares.
Both modes retain all normal capital, market-minimum, risk and evidence checks.

## User-operated commands

Run from the assigned worktree. The runner reads only the admin token from its
`.env` symlink to the main worktree; it never sources shell content or prints the
secret. It is restricted to localhost and read-only SQL in Rancher Desktop k3s.
Use a durable evidence directory outside Git; retain it across retries/restarts.

```sh
cd /Users/gunnermadsen/development/polymarket-bot/worktrees/polymarket-execution-rail
export PYTHONDONTWRITEBYTECODE=1

# Changes only this retained pair through existing process APIs:
# fixed requested sizing/bounds = 5, minimum seconds after open = 0, selected FAK;
# fresh immutable run identity;
# starts the API-only processes. No order submission.
python3 packages/polymarket-bot/tests/order_api/paired.py setup \
  --evidence-dir "$HOME/rail-order-evidence/fak-pair" \
  --account polymarket-primary --max-cash-usd 5.500001 --order-type fak

# Optional non-submitting check. Startup/reconciliation may need time to recover.
python3 packages/polymarket-bot/tests/order_api/paired.py preflight \
  --evidence-dir "$HOME/rail-order-evidence/fak-pair" \
  --outcome up --probability-up 0.95

# FUNDED: waits up to 600 seconds for an eligible opening window, repeats
# fresh preflight, then one live and one paper execute request.
# Probability is explicit synthetic test input, not a model prediction.
python3 packages/polymarket-bot/tests/order_api/paired.py execute \
  --evidence-dir "$HOME/rail-order-evidence/fak-pair" \
  --outcome up --probability-up 0.95 --wait-seconds 600 --submit-live

# Read-only: repeat after reconciliation and after market resolution.
python3 packages/polymarket-bot/tests/order_api/paired.py observe \
  --evidence-dir "$HOME/rail-order-evidence/fak-pair"

# User-operated same-key replay test, never a new order identity.
# An originally undelivered request can execute for the first time here.
python3 packages/polymarket-bot/tests/order_api/paired.py replay \
  --evidence-dir "$HOME/rail-order-evidence/fak-pair" --submit-live

# Capture before stopping, while both metric owners remain available.
python3 packages/polymarket-bot/tests/order_api/paired.py latency \
  --evidence-dir "$HOME/rail-order-evidence/fak-pair"

# After settlement evidence is captured, stop only the retained pair.
python3 packages/polymarket-bot/tests/order_api/paired.py stop \
  --evidence-dir "$HOME/rail-order-evidence/fak-pair"
```

`execute` cannot be repeated once keys exist. After any timeout/error, run `observe`
first and preserve `run.json`; then explicitly use `replay` if necessary. Replay
requires the same active process configuration and cash bounds, because a request
that never arrived may be submitted for the first time. No automatic new-key retry,
outcome switching, funded retry loop or process creation exists. Before either
submission, the runner may wait for another opening window within its bounded timeout.
A local exclusive lock prevents simultaneous runner invocations across directories.
Do not operate the same test processes manually while a paired run is in progress.

A partially failed setup/start is not rolled back automatically. Its recorded
before/after configurations identify exactly what succeeded. Resolve it through the
normal process API; do not erase evidence or modify the database to force progress.

`run.json` holds request timestamps/durations, full receipts, saved keys, definitions
and read-only economic evidence. `parity.json` reports lifecycle, fill VWAP/fees,
policy differences and incomplete boundaries. A successful `observe` command means
its available assertions passed, not that pending settlement or wallet evidence
was certified. `latency.json` and `latency.prom` reuse existing metric semantics.

FOK requires a separately initiated pair after the FAK pair is resolved. Reuse the
same two processes, stop them, and select `--order-type fok` with a new evidence
directory. This is another funded order; it is never submitted by the FAK command.

## Opening-window execution

Setup changes only the retained pair's `entry_timing_policy.params.min_seconds_after_open`
to zero, alongside the existing five-share setup. The 90-second before-close rule
and all other policies remain intact. Already prepared runs need normal stopped
process setup with a new evidence directory; active configurations are not hot-patched.
Do not replace an evidence directory that contains attempted orders.

`execute` reads the existing ingested market's authoritative start/end and database
time through bounded read-only SQL. It accepts only five-minute markets and pins
one market to both synthetic candidates. It can use the current opening window or
wait for the next one. No UMR prediction, invocation, or clock-modulo market guess
is involved. `preflight` alone remains a non-submitting diagnostic.

New POSTs must begin before second **25**, leaving five seconds of margin within
the requested 0–30 second window. Preparation that overruns the cutoff is discarded
before keys are allocated; readiness/admission may be retried while waiting. The
wait defaults to 600 seconds and accepts `--wait-seconds` from 1 through 900. Once
keys are saved, no automatic next-market attempt is permitted, even if only one
side was sent. Each transport authorization rechecks the cutoff after configuration
reads. Network and execution latency mean fills are not guaranteed before second 30.

Outside the cutoff, explicit same-key replay is allowed only when a read-only lookup
proves that the original process/key already has a durable order. A timeout or saved
attempt flag alone does not prove this. Undelivered requests cannot first execute
late; preserve evidence and use read-only observation to resolve uncertainty.

Opening prices are not guaranteed to be cheaper. Synthetic probability is test
input, not evidence of positive expectancy; the existing TPC price/cash limits apply.

## Remaining certification boundaries

- Full live/paper certification requires actual fills in both modes and clean live
  reconciliation. Source and offline tests cannot supply that evidence.
- Winning live settlement must be tied to actual wallet redemption/account evidence.
  The existing lifecycle keeps positive payouts pending redemption; this runner does
  not introduce a redemption mechanism or infer wallet credit from ledger state.
- Positive paper credit and subsequent execution availability must be demonstrated.
  A losing pair cannot prove this. The existing paper settlement/continued-order
  tooling and bounded, explicitly chosen subsequent tests remain available.
- Process/container restart recovery is covered by existing contracts but is not
  forced during a funded pair. No deployment or restart is performed by this runner.
- Synthetic API qualification does not qualify UMR translation or selector cutover.
  New `trigger=umr` routing remains unavailable until its separate integration work.

## Verified implementation evidence

On 2026-10-11, source inspection followed controller → application process lookup →
compiled configuration → shared OMS/service → existing paper/live adapters → shared
repositories and maintenance. The live service binds process/account identity,
reuses live signing and pending claims, requests reconciliation after submission,
and resolves reservations only through the existing lifecycle. No checkpoint-table
write or alternate order ledger was introduced by these tests.

Read-only local inspection found both retained processes stopped, paper configured
for five requested shares and live for one. Setup is therefore required. No funded
order, configuration change, process start, build or deployment was performed while
implementing this suite. Runtime live readiness and funded parity remain unverified
until the commands above produce their evidence.

Verification: 23 focused Rust rail tests and 18 configuration-contract tests passed;
two explicit timing benchmarks were ignored. The external Python suite has 27
checks: 24 offline checks passed, and the three opt-in accounting SQL checks also
passed against the local read-only database. Funded submission remains user-run.
The suite is runnable with standard unittest discovery; no CI workflow was edited.

Opening-window extension: 28 offline Python tests passed; three existing opt-in SQL checks were skipped in this focused run. No runtime state or rail implementation changed.
