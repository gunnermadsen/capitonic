# BTC loss-aware selective training result

## Identity

- Run: `20260917T183248Z`
- Git commit: `6b72f0b0a11afebdc02ea6210787c57ae4b15542`
- Source SHA-256: `f11e3bc597dfd546d667c5f610791fd48feab0015217c64278fc06e46612bffa`
- Config SHA-256: `93d9cdea28f0368dc4ba93fa6d12769fe6bd3f9424fe395877f70449f5f54ce7`
- Diagnostic model SHA-256: `178a3c731170beafd42e9f802b6999ebc3f40d04df4b3122146b7f66d997be3b`
- Artifact root: `/Volumes/docker-data/polymarket-bot/artifacts/training/btc-directional-model/loss-aware-selective/20260917T183248Z`
- Status: `historical_failed`

## Coverage audit

The immutable panel passed its readiness checks: 50,602 markets, 1,872,274 rows, exactly 37
checkpoints per market, 23,824 markets with authentic VWAP5 execution evidence, and no outcome
conflicts. The feature contract excludes RTDS, Chainlink RefPrice/candles, Kraken, trade prints,
L2, open interest, future state, and Polymarket prices as model features.

## Stitched out-of-fold result

| Metric | Loss-aware | Matched directional baseline |
| --- | ---: | ---: |
| Trades | 270 | 176 |
| Weeks traded | 8 | 6 |
| Accuracy | 56.67% | 55.11% |
| Wilson lower bound | 50.70% | 47.73% |
| Stressed PnL | 16.8289 | -7.4469 |
| Stressed profit factor | 1.0530 | 0.9661 |
| Wins needed per loss | 1.2419 | 1.2709 |
| Longest loss streak | 7 | 4 |
| Maximum drawdown | 28.0304 | 33.3416 |

Removing May 10 does not change either ledger because the nested risk evaluation begins later.
The result therefore fails independently of the original five-loss streak.

The admission policy rejected 65 baseline trades. It avoided 80.7000 stressed loss dollars but
also rejected 81.5250 stressed win dollars, for a net rejection value of -0.8250.

## Qualification

The run passed minimum trade count, minimum weeks, recovery ratio, stressed-PnL retention, and
positive stressed PnL after removing its best day. It failed accuracy, Wilson bound, stressed
profit factor, maximum loss streak, improvement over the baseline streak, rejection economics,
and the May 10-excluded conclusion gate.

This model is not qualified for export, paper deployment, or live trading. Its fitted bundle is
stored as `diagnostic.joblib`, not `qualified.joblib`.

## Diagnosis

The causal risk head did not generalize. On 5,804 out-of-fold broad candidates it achieved ROC
AUC 0.5182 and Brier score 0.2438; individual fold AUCs ranged from 0.5084 to 0.5651. Its mean
predicted loss probability was 38.11% against a 40.30% observed loss rate. This is effectively
near-random ranking for the intended admission decision.

Policy selection was also unstable across short chronological selection blocks. Selected
confidence floors ranged from 0.72 to 0.88, risk ceilings from 0.30 to 0.40, and one fold could
not select any qualifying policy. In the worst executed test block, the combined policy placed
172 trades at 55.81% accuracy and produced the seven-loss streak. The final partial block placed
16 trades at 43.75% accuracy and produced a six-loss streak.

The diagnostic ledger contains 13 two-loss streaks, eight three-loss streaks, two four-loss
streaks, one six-loss streak, and one seven-loss streak. Long-streak losses were associated with
higher 60/120-second volatility and wider 30/60-second ranges, but those associations did not
produce a useful out-of-fold classifier under this feature and label design.
