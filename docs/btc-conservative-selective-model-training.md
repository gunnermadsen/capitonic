# BTC conservative selective model training

## Objective

Train one time-conditioned BTC five-minute directional model that abstains by default and
trades only when its calibrated probability clears a conservative confidence and economic
edge policy. The target is not maximum trade count. The target is a small, repeatable set of
high-precision entries whose average loss can be recovered by no more than two average wins.

This lineage deliberately does not use a time-bucket router or train separate bucket models.
Decision time is an input to one model, and one frozen policy applies across its supported
60–240 second decision range.

- Branch: `feature/btc-conservative-selective-model`
- Worktree: `worktress/btc-conservative-selective-model`
- Base: `integration-2026-09-17`, commit `64c888dee2adb559886d3d0e593d58528efc1099`
- Primary quantity: five shares
- Status: preliminary training authorized; no runtime export or deployment authorized

## Permitted information

Model inputs are restricted to causal Binance BTCUSDT one-second OHLC-derived path, return,
range, volatility, momentum and time features. The static opening boundary may be represented
only through causal Binance-to-boundary distance features. Official resolved outcome is the
supervision target. Polymarket VWAP5, fees and fixed execution reserves are used only for
policy selection and economic evaluation, never as estimator inputs.

Explicitly excluded from estimator inputs:

- RTDS and realtime Chainlink RefPrice;
- Kraken data;
- Binance or Polymarket trade-print features;
- Polymarket or Binance L2/orderbook features;
- open interest;
- completed settlement or future-window values;
- prior model predictions.

## Two-run contract

### Preliminary run

Use the immutable panel ending at 2026-09-14. Verify its SHA-256 before fitting. Use strictly
chronological, market-disjoint windows:

| Purpose | Window |
|---|---|
| Estimator fit | 2026-03-21 through 2026-07-31 |
| Probability calibration | 2026-08-01 through 2026-08-10 |
| Policy selection | 2026-08-10 through 2026-08-20 |
| Sealed evaluation | 2026-08-21 through 2026-08-31 |
| Later confirmation | 2026-09-01 through 2026-09-14 |

Fit rows receive inverse-per-market weights so the 37 checkpoints do not count as 37
independent markets. At replay, the frozen policy may take at most one trade per market and
takes the first chronological checkpoint that qualifies. This is causal and prevents choosing
the best checkpoint with hindsight.

The preliminary run answers only whether this narrowly specified approach is promising. It
does not authorize capital and must not be called final evidence.

### Post-backfill run

Reuse this branch, feature contract, estimator recipe, replay semantics and reporting code.
Create a new immutable run directory and point the config at the completed backfill manifest.
Do not tune against the preliminary sealed or confirmation results. Extend the fit/calibration
prefix with newly available older history where feature semantics are identical. Reserve the
newest contiguous resolved period that was unavailable to the preliminary run as the final
prospective confirmation block.

The post-backfill run must publish a row-level and market-level coverage comparison against the
preliminary source. It must not silently treat synthetic Binance direction as an official
Polymarket outcome. Older Binance-only history may support a separately declared pretraining
experiment, but final calibration, policy selection and qualification require official
Polymarket outcomes and authentic execution evidence.

## Selection and reporting

Policy search is bounded to configured confidence, edge and maximum-cost values. A candidate
must have the configured minimum selection trades, positive stressed PnL, precision at least
the configured floor and compensation ratio no greater than two. The selected policy is ranked
by Wilson lower confidence bound for precision, then stressed PnL per trade, then fewer trades.
If no policy clears the constraints, the result is `no qualifying policy`; thresholds are not
relaxed automatically.

Report separately for selection, sealed and confirmation windows:

- eligible markets, trades and trades/day;
- wins, losses, precision and Wilson lower bound;
- gross win, gross loss, net and stressed PnL;
- profit factor;
- mean win, mean absolute loss and compensation ratio;
- maximum drawdown and longest losing streak;
- entry second, side and price distributions;
- Brier score and log loss over every scored market/checkpoint, not only trades.

No model qualifies merely because it trades rarely. The minimum acceptable result is a frozen
policy with positive stressed results in both sealed and confirmation windows, no compensation
ratio above two, and no evidence that one day supplies the entire gain. Small samples remain a
limitation even if these checks pass.

## Artifacts and stopping point

Generated data, models, predictions, ledgers, metrics and logs belong under
`/Volumes/docker-data/polymarket-bot/artifacts/training/btc-directional-model/conservative-selective/<run-id>/`.
Each run records source hashes, Git branch/commit, configuration hash, runtime versions and
the exact command. Lightweight configuration and this plan remain in Git.

Stop after the preliminary report and model artifact are produced. Do not export to the unified
runtime, create a trading process, deploy, merge, or start the post-backfill run automatically.

