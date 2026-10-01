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
The joint all-four population and final candidate-specific feature panels remain pending.

Market identity/outcome caches reach September 23. Raw CLOB archives extend through
September 25, without established matching later labels. Applicable core archives,
21 additional source groups, and historical artifact identities were inventoried and
hashed. Optional sources are not thereby approved for causal feature use. Distinct
futures candles, trades, funding, and basis products were not found in the inspected
Parquet locations. Raw PMXT history is limited to a surviving April 13 pilot hour;
the longer execution-snapshot history lacks raw bid/ask levels needed for arbitrary
partial fills and exits.

## Unresolved experimental identity

1. Prior tournaments already evaluated and reported the currently verified newest
   labeled dates. The user must clarify whether the final holdout must contain dates
   never evaluated by any prior tournament or be untouched within this new run.
2. The pinned champion cannot establish causal historical quiet states. Its linked
   executed run fits through August 10 exclusive, calibrates through August 17, uses
   model-selection folds through August 22, and searches a policy through August 24.
   Its stored final policy is `None`. The old champion ledger instead uses a hardcoded
   export policy whose selection cutoff is unproven; the export first appears in a
   September 18 commit. No older immutable fold estimator with a preselected policy
   was established. A causal incumbent ledger/policy provenance is required, or the
   user must explicitly authorize changing the quiet reference to out-of-fold activity
   from the conservative-selective refresh. That substitution has not been made.

Older prepared Binance core caches also omit receipt provenance. Raw candle archives
contain non-null provider availability beginning August 10; earlier backfilled candles
cannot silently be assigned historical live receipt times. Candidate source eligibility
and any compatibility exclusions remain to be validated.

## Implementation and verification

Two offline source-checkpoint modules were added in `btc_directional_model`, with
focused tests for backfill exclusion, conservative receipt timing, native deduplication,
conflict rejection, and separate TWAP windows. No trading-runtime file or interface
was changed. The previous attempt at `aee988e590169a7e853443e00445ff0baeaf50b6` remains
unchanged. All generated data stays under this run's SSD directory.

## Required next decisions

Resolve the two experimental-identity questions above before selecting splits or
constructing quiet-state training data. Then resume the recorded checkpoint dependencies;
training, historical replay, all ablation arms, execution controls, robustness,
qualification, final model artifacts/tags, and candidate charts are still outstanding.
No recommendation here authorizes changing the experiment or starting runtime work.
