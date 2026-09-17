# BTC conservative selective model: training and qualification plan

## Status and authorization boundary

This is the complete proposed plan for review. It authorizes no data generation, model fitting,
threshold search, artifact creation, Git commit, runtime export, trading process, deployment or
merge. Execution begins only after the user approves this plan and then explicitly approves the
exact preliminary-run manifest and configuration produced by the readiness audit.

The unrequested run `20260917T161000Z` and its fitted estimator are excluded from selection,
comparison and qualification evidence. Its results must not influence the thresholds or model
choice below. Existing implementation on `feature/btc-conservative-selective-model` is not
accepted merely because it exists; implementation must be reviewed against this plan before use.

## Business objective

Produce one deployable BTC five-minute directional model whose default action is `no_trade`.
The model should make a small number of economically favorable entries that can be trusted more
than a high-frequency classifier or time-bucket router.

The final output is one model bundle with one causal feature contract, one probability
calibration contract and one admission policy. Internal baselines and bounded parameter
comparisons are development controls; they do not become separate deployed models.

Primary requirements:

1. High precision at deliberately low coverage.
2. An average losing trade recoverable by no more than two average winning trades.
3. Positive net expectancy after fees, execution reserve and stress slippage.
4. Stable evidence across chronological periods rather than profit concentrated in one day,
   side, time bucket or short regime.
5. No dependency on RTDS, Kraken, realtime Chainlink RefPrice, trade prints, orderbook-derived
   predictors or open interest.

Expected frequency is an outcome, not a quota. One or two trades per day is acceptable; three
or four per week is acceptable. The policy must never manufacture trades to meet a quota.

## Model scope: one general model, not a router

Train one time-conditioned directional probability model over every supported causal decision
checkpoint. Elapsed and remaining time are estimator inputs. The same fitted estimator scores
all checkpoints and both sides.

Time buckets are reporting dimensions only. They may reveal where performance comes from, but
they are not independently optimized, assigned different models or used to delete losing
buckets after evaluation. If the model cannot qualify without retrospective bucket selection,
the result is failure.

At most one trade may be taken per market. Replay inspects checkpoints chronologically and takes
the first checkpoint satisfying every frozen probability, economic, price, freshness and risk
requirement. Selecting the best checkpoint after observing later rows is prohibited.

## Current verified data baseline

Current immutable broad panel:

`/Volumes/docker-data/archives/polymarket-bot-worktrees/time-bucket-specialist-tournament/packages/btc-directional-model/data/btc-full-coverage-vwap-admission-20260321-20260914/vwap-admission-panel.parquet`

- SHA-256: `f11e3bc597dfd546d667c5f610791fd48feab0015217c64278fc06e46612bffa`
- Date range: 2026-03-21 through the 2026-09-14 watermark
- Markets: 50,602
- Rows: 1,872,274
- Checkpoints: 37 per market, 60 through 240 seconds
- Core Binance/path coverage: 50,602 markets / 100%
- Official outcome coverage: 50,602 markets / 100%
- Static opening-boundary context: 50,602 markets / 100%
- Authentic VWAP execution coverage: 23,824 markets / 47.1%, 714,142 rows

This distinction is mandatory:

- Directional scoring uses all 50,602 markets.
- Policy fitting, trade replay, PnL and recovery-ratio evidence use only markets with authentic
  contemporaneous VWAP5 evidence.
- Missing execution prices are never imputed from outcomes, midpoint, terminal price or another
  market. They are economically unevaluable, not abstentions or wins.

Before either run, a readiness audit rebuilds these counts through the exact source watermark.
“Full coverage” means every valid row from every permitted source in the declared range, not an
intersection with optional datasets and not an arbitrary recent slice.

## Permitted data and feature contract

### Supervision

- Official resolved Polymarket `Up`/`Down` outcome is the only final target.
- Market identity and exact UTC window start/end establish grouping and chronology.
- Settlement values, completed averages, future candles and future rows are labels or audit
  evidence only; none may enter the estimator.
- Synthetic Binance five-minute direction is not an official Polymarket label. Older
  Binance-only history may later support a separately authorized pretraining experiment, but it
  is outside the two runs in this plan.

### Estimator inputs

Use causal fields derived from Binance BTCUSDT one-second aggregate candles and the static
opening boundary:

- elapsed and remaining time;
- price path from the market open;
- Binance distance from the static opening boundary;
- returns over 1, 5, 15, 30, 60, 90, 120 and 180 seconds;
- realized volatility over 5, 15, 30, 60, 90, 120 and 180 seconds;
- causal high/low range, range position and path efficiency;
- boundary/path crossings, time since crossing and crossing direction/density;
- momentum alignment, acceleration, reversal, excursion, pullback and recovery;
- volatility-shock and terminal-distance normalization;
- UTC hour and weekday cyclical encodings.

The primary feature contract is price-only. Binance kline volume, taker-buy volume and trade
count are retained only for a prespecified ablation. They are aggregate candle fields rather
than raw prints, but they enter the final model only if nested walk-forward evidence shows a
repeatable improvement without degrading calibration, recovery ratio or prospective results.

### Explicitly forbidden estimator inputs

- RTDS;
- realtime or historical Chainlink RefPrice features;
- Chainlink candle features;
- Polygon oracle observations beyond the static market opening boundary;
- Kraken prices, candles, trades or books;
- Binance or Polymarket individual trade prints;
- Binance or Polymarket L2/orderbook fields;
- Polymarket price, spread, depth, VWAP or implied probability;
- open interest;
- prior model predictions or router decisions;
- settlement bridge, TWAP target or completed-market information.

Polymarket VWAP5, fees and configured reserves are allowed only in the admission/evaluation
layer after the probability is produced. They are never model features.

## Data integrity and leakage controls

Every run fails before fitting unless all checks pass:

1. Immutable sources and manifests have verified SHA-256 identities.
2. Market IDs are unique within their five-minute window and all expected checkpoints are
   accounted for or explicitly reported missing.
3. Outcomes are resolved and consistent across duplicated source records.
4. Every feature timestamp is no later than its decision timestamp.
5. No feature references data after the checkpoint or market close.
6. All rows of one market remain in one chronological fold.
7. Labels unavailable at a fold boundary are purged from that fold's fit.
8. Fit, calibration, policy-selection and evaluation market IDs are disjoint.
9. Feature null, infinity and range diagnostics are reported by day and checkpoint.
10. The feature allowlist is enforced by name and lineage; forbidden tokens cause failure.
11. Each market receives total fit weight one so 37 checkpoints are not 37 independent votes.
12. Settlement-regime and calendar coverage are reported without removing inconvenient rows.

## Estimator design and bounded development search

### Controls

1. Persistence control: choose the side implied by causal Binance distance from the boundary.
2. Regularized logistic model using the identical price-only feature contract.

### Primary estimator

Use `HistGradientBoostingClassifier`: nonlinear interactions, native missing-value support and no
new dependency. Serious starting configuration:

| Parameter | Value |
|---|---:|
| learning rate | 0.03 |
| boosting iterations | 300 |
| maximum leaves | 15 |
| minimum samples per leaf | 200 market-weighted rows |
| L2 regularization | 8.0 |
| maximum bins | 127 |
| random seed | one frozen seed |
| class weighting | none unless a fold is materially imbalanced |
| sample weighting | inverse checkpoint count per market |

Early stopping uses a chronological inner-validation tail, never a random split.

A bounded nested search may compare only these prespecified alternatives:

- maximum leaves: 7, 15, 31;
- minimum samples per leaf: 100, 200, 400;
- L2 regularization: 4, 8, 16;
- price-only versus price-plus-aggregate-kline-volume ablation.

Use a fractional factorial set of no more than 12 configurations, not the 54-cell cross-product.
One frozen seed selects the configuration; two additional seeds test only the winner. Reject a
configuration whose decisions materially change across seeds. This selects one output; it is
not a tournament of deployable models.

## Probability calibration

Each outer fold uses a later, chronologically separate calibration block. Primary calibration is
logistic calibration on raw log-odds with market-equal weights. Isotonic calibration is a
diagnostic and may replace it only if it improves Brier score, log loss and expected calibration
error in a later inner block without unstable tails.

Required reports:

- row- and market-level Brier score and log loss;
- expected and maximum calibration error;
- reliability bins with counts;
- calibration slope and intercept;
- accuracy/calibration by side, month, decision band and volatility regime.

Admission uses conservative probability: calibrated probability minus the larger of measured
local calibration error and its configured safety floor. Unsupported or overconfident tail bins
cannot authorize trades.

## Economic admission design

For side probability `p`, VWAP5 cost `c`, execution reserve `r` and stress debit `s`:

- expected net value/share: `p - c - r`;
- stressed expected value/share: `p_conservative - c - r - s`;
- winning PnL/share: `1 - c - r - s`;
- losing PnL/share: `c + r + s`.

The recovery requirement is structural:

`(c + r + s) / (1 - c - r - s) <= 2`

Therefore `c + r + s <= 2/3`. With `r = 0.005` and `s = 0.010`, the raw VWAP5 ceiling is
`0.651666...`; configure **0.65**. A higher-cost entry cannot qualify because confidence is high.

Frozen admission-search dimensions:

- conservative probability floors: 0.72, 0.76, 0.80, 0.84, 0.88;
- stressed edge floors: 0.03, 0.05, 0.08, 0.10;
- raw VWAP5 ceilings: 0.55, 0.60, 0.65;
- checkpoints: all available 60–240 second checkpoints;
- quantity: exactly five shares.

These 60 simple policies use the same out-of-fold predictions. Select one using training-side
nested evidence only. Never select from the final sealed or prospective block. Replay takes the
first qualifying checkpoint per market.

## Chronological training and evaluation

Exact calendar dates are resolved from the run watermark during readiness. Evidence roles are
fixed.

### Rolling development evidence

Use expanding-window outer folds, preferably monthly after a minimum 28-day warm-up:

1. fit on the historical prefix;
2. calibrate on the next chronological block;
3. select admission on the next block using fold-local out-of-fold predictions;
4. freeze estimator, calibrator and policy;
5. replay the following block once.

Stitch each outer test prediction once per market. These results estimate the complete training
procedure. Final-refit in-sample statistics never replace them.

### Preliminary run

Use every resolved market in the current pre-backfill manifest. This run validates the full
pipeline and tests whether the frozen family can plausibly meet the business objective.

- All eligible history participates in expanding folds.
- The newest 21 complete calendar days are opened once as a sealed retrospective block.
- Because related history was previously inspected, this is sealed retrospective evidence, not
  pristine out-of-sample proof.
- Directional evaluation covers every market with core inputs and an official outcome.
- Economic evaluation covers every market with authentic VWAP5, with missingness compared to
  the full directional cohort.
- A bundle is provisional only if all preliminary gates pass; provisional is not deployable.

### Post-backfill run

The second run reuses the frozen source allowlist, features, estimator search budget, fold
generator, calibration rules, admission grid, execution assumptions and gates. It is not a
threshold-retuning exercise.

Before fitting, publish old-versus-new date range, additions by day, label/VWAP additions,
conflicts, common-cohort equality and unresolved gaps. Backfilled older data extend fitting and
rolling evidence; they do not become prospective merely because downloaded later.

The prospective confirmation cohort begins after this plan is frozen and contains only markets
not used or inspected during preliminary development. A serious paper candidate must pass both
the expanded nested historical procedure and untouched prospective confirmation. Too few
prospective trades means `insufficient prospective support`, not relaxed gates.

## Qualification gates

All gates are mandatory.

### Data and reproducibility

- 100% manifest/hash traceability for included rows.
- Zero market overlap across evidence roles.
- Deterministic predictions/decisions on a clean rerun from the same commit.
- Artifact prediction parity against stored reference vectors.

### Directional quality

- Selected-trade precision at least 0.78 on stitched historical evidence.
- 95% Wilson lower bound at least 0.68 historically and 0.60 prospectively.
- Brier and log loss better than persistence and logistic controls on matched rows.
- Expected calibration error <= 0.04 overall and <= 0.07 in the selected-confidence region.
- No unexplained dependence on one side for all gains.

### Economic quality

- Positive net and stressed PnL historically and prospectively.
- Stressed profit factor >= 1.50 historically and >= 1.20 prospectively.
- Mean absolute loss / mean win <= 2.0 in every primary evidence block.
- No executed raw VWAP5 above 0.65.
- Maximum drawdown <= six average winning trades.
- Longest losing streak <= three in qualification evidence.
- Stressed PnL stays positive after removing the most profitable day.
- No day contributes more than 25% of total positive stressed PnL.

### Support and stability

- At least 60 stitched historical trades across at least eight calendar weeks.
- At least 20 prospective trades across at least four calendar weeks before serious paper
  qualification. At three to four trades/week, collection continues until support exists.
- Positive stressed PnL in at least 70% of calendar months containing trades.
- No material collapse across the August settlement-regime boundary.
- Policy remains acceptable under another `+0.01/share` adverse slippage and one-checkpoint delay
  where authentic later execution evidence exists.
- Across three winner seeds: >=90% direction agreement and >=80% market-admission agreement.

## Required diagnostics and reports

Keep every configuration and both controls visible. Report:

- source/feature coverage by day and checkpoint;
- directional and executable cohort counts;
- all fold ranges;
- calibration metrics and reliability plots;
- confusion matrix, precision, recall and abstention;
- trades, wins/losses, accuracy and Wilson intervals;
- entry side, second, price and stressed-edge distributions;
- gross/net/stressed PnL, profit factor and expectancy/trade;
- compensation ratio, break-even accuracy, drawdown and losing streaks;
- results by month, weekday, UTC hour, side, time band, volatility and settlement regime;
- day-block bootstrap and leave-one-day/week-out sensitivity;
- matched controls, feature ablation and seed stability;
- every failed gate with its exact value.

Accuracy without entry economics is not success. Profit without support or calibration is not
success. Rare trades without repeated chronological evidence are not success.

## Execution workflow after approval

### Gate 1 — approve this plan

No implementation or training begins before explicit approval.

### Gate 2 — readiness audit and exact run proposal

A read-only inventory produces the immutable source manifest, watermark, resolved fold dates,
counts, feature allowlist, exact configuration set, admission grid, compute budget and artifact
directory. The user explicitly approves that exact preliminary run.

### Gate 3 — implementation verification

Implement only this plan. Unit-test chronology, weighting, first-qualified replay, PnL,
forbidden features, manifest failure, recovery ceiling, fold disjointness and label purging. Run
a tiny synthetic smoke fit excluded from evidence. Present the final config and command without
running the full fit.

### Gate 4 — preliminary training

After explicit authorization, execute once from a clean committed feature branch. Bound threads,
record resource use, checkpoint folds atomically and store bulky artifacts only on the SSD. Do
not change parameters mid-run. Failure remains a recorded failure.

### Gate 5 — preliminary review

Present the complete report and gate table. Do not start post-backfill training, export, create a
process, deploy or merge automatically.

### Gate 6 — post-backfill readiness and authorization

Verify new coverage/common-cohort consistency and present the exact second-run manifest/config
for explicit approval. Method changes require an approved revised plan and new lineage.

### Gate 7 — second training and decision

Run once and classify as `qualified for paper observation`, `insufficient support`, or `failed`.
Paper qualification does not authorize live capital.

## Artifact and provenance contract

Authorized runs live under:

`/Volumes/docker-data/polymarket-bot/artifacts/training/btc-directional-model/conservative-selective/<run-id>/`

Required contents: source/coverage/feature manifests; frozen config and hash; fold assignments;
all out-of-fold predictions; opportunity/trade ledgers; calibrators; full search results;
qualified bundle if any; parity vectors; metrics/report; logs/resource usage; exact clean Git
commit and artifact hashes.

Generated data and models are not committed. A model provenance tag is created only for a real
immutable artifact after its producing commit and qualification are recorded. An unqualified fit
receives no deployable-model identity.

## Stopping point

Planning stops when the user approves or revises this document. Execution stops after each
authorized run and report. No subsequent training, adjustment, export, process, merge, image or
deployment occurs without separate explicit instruction.
