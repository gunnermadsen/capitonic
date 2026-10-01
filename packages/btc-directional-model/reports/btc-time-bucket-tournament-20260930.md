# BTC time-bucket tournament: source checkpoint

Run: `btc-time-bucket-tournament-20261001T001539Z` (September 30 local date).
Branch: `training/btc-time-bucket-tournament`.
Integration base: `3580b6f780f2842683ab44346d763adc39498bf9`.

**Status: partial; no model trained, evaluated, qualified, or deployed.** The complete
eight-challenger plan remains required. No downstream checkpoint has been accepted.

The canonical evidence is
`/Volumes/docker-data/capitonic-btc-directional-model/btc-time-bucket-tournament-20261001T001539Z/manifests/run.json`.
The full supplied plan is retained under `inputs/authoritative-plan.md`; execution
boundaries and the explicitly amended worktree selection are recorded in the manifest.

## Completed source checks

The four mandatory products were hashed and checked for native-identity conflicts,
duplicates, basic value/timestamp validity, and demonstrated historical availability.
Hourly coverage, daily audit facts, and separate causal source selections are on SSD.

| Product | Raw rows | Causally eligible unique rows | Duplicate rows removed |
|---|---:|---:|---:|
| Direct Chainlink RefPrice | 6,011,719 | 1,806,148 | 0 |
| PMData Chainlink RefPrice | 11,228,942 | 8,748,102 | 2,480,840 |
| PMData Chainlink TWAP | 8,098,911 | 8,098,909 | 2 |
| Polymarket RTDS Chainlink TWAP | 4,620,172 | 4,620,172 | 0 |

No conflicting native identities or invalid values under these initial checks were
found. Direct live provenance begins August 10 at 20:32:27 UTC and ends September 1
at 19:02:06 UTC. Older backfilled Direct rows remain excluded from historical feature
availability. PMData original provider receipt and Polymarket publication/receipt
timestamps remain distinct. `valid_from_timestamp` is never treated as publication.

A diagnostic source join covered 53,465 market identities and all thirteen buckets
(695,045 decisions). All four products have nonzero fresh causal joins. The output
`metrics/mandatory-product-market-bucket-coverage.parquet` preserves source and receipt
ages; the 2/5/10-second diagnostic counts do not select model staleness thresholds.
The all-four coverage intersection is nonzero across all thirteen buckets on August 20
through September 1. Requiring both 30/60-second windows for each TWAP gives 17,284,
34,541, and 35,636 matched decisions at diagnostic ages of 2, 5, and 10 seconds.
These are coverage diagnostics, not selected model cutoffs or accepted feature panels.
Daily/bucket counts are in `metrics/mandatory-product-day-bucket-coverage.parquet`.

Market identity/outcome caches reach September 23. Raw CLOB archives extend through
September 25, without established matching later labels. Applicable core archives,
21 additional source groups, and historical artifact identities were inventoried and
hashed. Optional sources are not thereby approved for causal feature use. Distinct
futures candles, trades, funding, and basis products were not found in the inspected
Parquet locations. Raw PMXT history is limited to a surviving April 13 pilot hour;
the longer execution-snapshot history lacks raw bid/ask levels needed for arbitrary
partial fills and exits.

## Accepted amendment and missing incumbent evidence

The user explicitly retained all valid historical dates, including dates used in prior
tournaments. The final holdout needs to be untouched within this run. Exact chronological
roles must be frozen before exploration; every evaluation and holdout observation must
remain excluded from fitting, calibration, feature selection and policy tuning, including
later walk-forward fits. The verbatim amendment is retained in
`inputs/historical-coverage-and-incumbent-amendment.md`. No exploration has started;
exact date allocation remains pending completion of the source checkpoint.

The bounded SSD search covered canonical training runs, legacy model-training archives,
raw artifacts, runtime model packages, verified drains, feature snapshots and execution
snapshot archives. The documented legacy `archives/polymarket-bot-worktrees` location
does not exist. Database/observability backup contents were not restored or used as a
training-source fallback.

Real feature snapshots establish these separate identities:

| Mode | Process ID | Observed model |
|---|---|---|
| Paper | `469d761a-0918-43a9-b708-37ce23ab20dc` | `btc-5m-conservative-selective-paper-20260917` |
| Live | `a5ae37ff-e946-4979-91e1-44346e4c32f3` | `btc-5m-conservative-selective-development-live-pilot-20260921-v1` |

The September 23 snapshot file SHA-256 is
`fbff83098d1cfc40eaf7b161a293f9b5158d187747db5f9904f68a9799a03f23`.
Its model identities and receipt/creation timestamps were checked directly. The inspected
snapshots contain inference observations, without order/fill identities, actual-trade
timestamps, admission history, or enabled/disabled coverage. The familiar 193-row
`champion.parquet` and original selective trade ledgers are simulated backtests, not
actual paper/live activity. Earlier exports also reused a model key with a different
artifact hash, so key equality alone is insufficient identity evidence.

**Mandatory input not established:** actual incumbent order/fill activity with event and
original recording timestamps, time-versioned process/model identity, and observation
coverage distinguishing quiet from disabled or missing periods. The exact evidence,
hashes, search scope and exclusions are recorded in
`manifests/incumbent-activity-evidence.json`. Missing rows cannot establish quiet states.
No simulated refresh activity has been substituted, and the source checkpoint remains
unaccepted. This is an evidence blocker, not a remaining holdout-policy question.

Older prepared Binance core caches also omit receipt provenance. Raw candle archives
contain non-null provider availability beginning August 10; earlier backfilled candles
cannot silently be assigned historical live receipt times. Candidate source eligibility
and any compatibility exclusions remain to be validated.

## Implementation and verification

Two offline source-checkpoint modules were added in `btc_directional_model`, with
focused tests for backfill exclusion, conservative receipt timing, native deduplication,
conflict rejection, separate TWAP windows, and matched coverage requiring every product.
All eight focused tests and Ruff checks passed. No trading-runtime file or interface
was changed. The previous attempt at `aee988e590169a7e853443e00445ff0baeaf50b6` remains
unchanged. All generated data stays under this run's SSD directory.

## Missing outcomes and required evidence

Completion requires the missing actual-activity evidence before accepting the source
checkpoint or constructing quiet-state training data. No operational data export or
system change was attempted. Resume the same run when the required input is established;
training, historical replay, all ablation arms, execution controls, robustness,
qualification, final model artifacts/tags, and candidate charts are still outstanding.
The full eight-challenger tournament remains incomplete; no model is qualified.
