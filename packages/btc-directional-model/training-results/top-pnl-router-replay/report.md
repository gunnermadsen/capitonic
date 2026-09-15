# Top-ten PnL frozen router replay

Exact top ten from the established four-tournament Q5 PnL ranking. No retraining, extra filters, blending or membership optimization.

Selection/sealed: September 1–7, 2026. Subsequent confirmation: September 7–14, 2026. UTC, end exclusive. Selection-period performance is retrospective; confirmation was already observed in prior analysis and is not globally blind.

First qualifying entry per market. Same-second conflicts use frozen PnL rank. All original side, bucket, confidence, cost and edge policies retained. Q50 uses actual ladder replay, not linear scaling.

## Router results

| Period | Q | PnL | Stress | PF | Trades | W/L | Win rate | Recovery wins/loss | Coverage | Avg confidence | Max DD |
|---|---:|---:|---:|---:|---:|---|---:|---:|---:|---:|---:|
| sealed | 5 | 285.89 | 252.39 | 1.578 | 670 | 488/182 | 72.84% | 1.699 | 40.36% | 84.35% | 17.66 |
| sealed | 50 | 2752.67 | 2422.67 | 1.557 | 660 | 478/182 | 72.42% | 1.687 | 39.76% | 84.32% | 213.39 |
| confirmation | 5 | -75.43 | -119.43 | 0.912 | 880 | 567/313 | 64.43% | 1.987 | 49.41% | 82.58% | 99.32 |
| confirmation | 50 | -806.99 | -1244.49 | 0.905 | 875 | 562/313 | 64.23% | 1.983 | 49.13% | 82.49% | 1013.49 |

## Frozen members and attribution at Q5

| Rank | Exact member | Original sealed PnL | Router sealed trades / PnL | Router confirmation trades / PnL |
|---:|---|---:|---|---|
| 1 | settlement_aligned_terminal__with_rtds_candles | 293.33 | 155 / 64.59 | 230 / -0.03 |
| 2 | price_control_terminal__with_rtds_candles | 284.80 | 40 / -17.17 | 56 / 14.17 |
| 3 | settlement_aligned_terminal__without_rtds_candles | 282.03 | 49 / 0.14 | 72 / -6.87 |
| 4 | kraken_crossvenue_terminal__with_rtds_candles | 269.57 | 18 / 16.89 | 50 / -41.01 |
| 5 | price_control_terminal__without_rtds_candles | 256.66 | 12 / -5.50 | 23 / -0.50 |
| 6 | kraken_crossvenue_terminal__without_rtds_candles | 254.39 | 4 / 4.32 | 5 / -2.31 |
| 7 | multivenue_late__without_rtds_candles | 216.20 | 349 / 214.71 | 379 / -44.15 |
| 8 | time_specialist_late__without_rtds_candles | 216.20 | 0 / 0.00 | 0 / 0.00 |
| 9 | multivenue_late__with_rtds_candles | 214.61 | 43 / 7.91 | 65 / 5.27 |
| 10 | time_specialist_late__with_rtds_candles | 214.61 | 0 / 0.00 | 0 / 0.00 |

## Verification and limitations

- All four model bundles verified by SHA-256; archived panel and source configuration matched original provenance. 30 individual period/quantity metric checks reproduced the original results.
- Ten frozen models re-inferred from causal features; no fitting or policy selection executed. Model duplicates retained and resolved by the same entry arbitration.
- One entry per market verified. Signals that disagree at different times are included in overlap diagnostics, not necessarily simultaneous conflicts.
- Recorded VWAP ladder fillability is assumed. Queue position, live latency, capital constraints and competing portfolio positions are not newly simulated. Net PnL includes original fees and reserve; stress adds original per-share slippage.
- No model tag, runtime export, database mutation, image build, deployment or merge performed.

Detailed inputs and identities: [router-manifest.json](router-manifest.json). Complete metrics and parity checks: [metrics.json](metrics.json). Trade-level evidence is stored in the accompanying Parquet files.
