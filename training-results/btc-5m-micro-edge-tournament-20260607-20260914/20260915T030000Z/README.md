# BTC five-minute micro-edge tournament

Run: `20260915T030000Z`. Training only; no runtime deployment.

- [Complete model comparison](report.md)
- [All 1,050 model/bucket comparisons](bucket-report.md)
- [Settlement regime comparison](regime-report.md)
- [Data coverage](coverage-report.md)
- [Metrics and uncertainty](metrics.json)
- [Verified model artifact provenance](model-provenance.json)
- [Final verification](verification.json)
- [External Parquet archive and purpose mapping](parquet-archive.md)

All 35 trained artifacts, chronological fold checkpoints, predictions and actual VWAP5 trade ledgers remain in `/Users/gunnermadsen/development/polymarket-bot/worktress/btc-micro-edge-tournament/packages/btc-directional-model/runs/btc-micro-edge-20260607-20260914/20260915T030000Z`. The provenance files identify their absolute paths and SHA-256 hashes. The numerical Parquet tables are stored in the external archive mapped above. Model artifacts are referenced rather than copied into Git.

Full-range final directional fitting spans June 7–September 14 inclusive. Chronological evaluation spans July 5–September 14 after supervised and calibration warm-up. Waiting auxiliaries require causal teacher support; their underlying directional models use the full range. Final full-range artifacts are not independently tested after fitting; reported performance comes from the earlier chronological fold models.

Three refitted controls represent four historical tournament origins. Prior sealed tournament totals are not substituted for these matched comparisons. Candidate selection reused historical research; this is not a new blind holdout. All performance is simulated five-share economics.

## Observed outcomes

All 32 planned new candidates and 3 historical refits completed. The full-range panel retains 1,705,440 five-second decision rows across 28,424 resolved markets. Economic evaluation covers 20,360 resolved markets from July 5 through September 14, after chronological warm-up. Optional source gaps remain missing; the coverage report identifies each source's actual span.

The largest full-period new-model PnL is `within_bucket_wait__no_rtds`: **$706.38**, PF **1.113**, 6,624 trades, 4,643 wins / 1,981 losses, 70.09% win rate, 32.53% resolved-market coverage, average entry 108.9 seconds, 2.105 average winning trades to compensate for an average loss, and $141.60 maximum drawdown. Stress PnL is $375.18. Its day-bootstrap 95% stress interval is **-$132.14 to $870.55**, so this is not strong evidence of a reliably positive stressed return. RTDS candle features are absent from this model and its base.

Twenty of the 32 new standalone candidates have positive stressed PnL. No new model dominates all objectives. The strongest baseline by full-period PF remains `baseline_20260914T145421Z`: PF 1.177, $153.60 PnL and 1,181 trades. On the common executable market set, the highest-PnL new model's stress difference versus this baseline is **-$116.18**, with paired-day 95% interval -$524.04 to $206.20. Its larger headline PnL therefore does not establish superiority on matched coverage.

Both new-model routers lost money: eligible-first **-$949.12**, PF 0.946; opportunity-reservation **-$636.79**, PF 0.959. Their stressed losses were $1,708.37 and $1,309.24 respectively. The historical-refit router earned $227.42, PF 1.108. Higher trading frequency did not establish a better combined strategy.

Individual fixed-policy bucket diagnostics contain 54 cells with PF above 2, including 50 with at least 100 trades. For example, `q5_payoff__no_rtds` at 250–259 seconds has PF **4.413**, $185.43 PnL, 173 trades and 155 wins / 18 losses. `market_residual__rtds` at 270–279 seconds has PF **3.075**, $443.83 PnL, 520 trades and 410 wins / 110 losses. These cells are selected retrospectively from 1,050 comparisons, have overlapping markets, and cannot be summed into a portfolio or presented as untouched confirmation. All positive and negative cells are retained.

The bounded cutover diagnostic favored pooled history: $580.47 / PF 1.130 versus $191.72 / PF 1.038 for post-cutover-only fitting on the same August 23–September 14 evaluation span. This supports retaining the older history for this comparison; it does not prove the settlement change is irrelevant. The original August 14 timing remains a date-level assumption.

The main report is ordered by post-cutover stressed PnL per day, then full-period PF, rather than full-period PnL alone. All PnL uses five shares per trade, recorded VWAP5, fees and the documented slippage reserves.

## Verification and execution

All 35 final artifact hashes, 455 fold model/prediction hashes, market separation with five-minute label embargo, one-trade-per-market ledgers, source labels, VWAP5 prices/fees/PnL and composed final-model reload parity passed verification. The focused and reused test suite passed 38 tests; scoped lint and whitespace checks passed. No candidates were omitted. The saved main-artifact span was 1 hour 52 minutes, including resumptions; full aggregate CPU time was not retained and is explicitly unavailable in execution-summary.json.
