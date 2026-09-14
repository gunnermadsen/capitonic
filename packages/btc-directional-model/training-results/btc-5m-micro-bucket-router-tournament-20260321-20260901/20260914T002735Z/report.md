# BTC Five-Minute Micro-Bucket Router Tournament

Run: `20260914T002735Z`
Qualification: **trained_evaluated_outperformed_champion**
Selected router: **full_rtds_free**

## Frozen hypothesis

Historically successful model families are retrained inside the time slices where they showed edge; individual, prior-weighted, and agreement-gated descendants are then folded into a no-trade-by-default router.

## Router comparison — chronological confirmation (not globally blind)

| Router | PnL | Stress | PF | Trades | UP/DOWN | W/L | Coverage | Avg entry | Recovery | Max DD |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| champion_replay | 70.46 | 4.46 | 1.048 | 132 | 132/0 | 79/53 | 7.94% | 63.5 | 1.422 | 320.15 |
| early_micro_individual | 86.28 | 21.28 | 1.059 | 130 | 0/130 | 78/52 | 7.82% | 65.0 | 1.417 | 340.04 |
| early_micro_prior_weighted | 78.17 | 44.37 | 1.099 | 154 | 154/0 | 101/53 | 9.27% | 61.9 | 1.734 | 151.53 |
| early_micro_agreement | 0.00 | 0.00 | 0.000 | 0 | 0/0 | 0/0 | 0.00% | 0.0 | 0.000 | 0.00 |
| later_rtds | 275.91 | 80.91 | 1.073 | 390 | 0/390 | 272/118 | 23.47% | 157.2 | 2.149 | 441.90 |
| full_hybrid | 339.01 | 114.71 | 1.075 | 535 | 96/439 | 364/171 | 32.19% | 117.6 | 1.979 | 563.54 |
| full_rtds_free | 676.38 | 499.58 | 1.190 | 440 | 96/344 | 298/142 | 26.47% | 112.5 | 1.764 | 407.81 |
| prior_composed_replay | 2003.04 | 1480.04 | 1.212 | 1046 | 287/759 | 736/310 | 62.94% | 114.8 | 1.958 | 723.83 |

## Common-window comparison — 2026-08-26 to 2026-08-27 (end exclusive)

| Router | PnL | Stress | PF | Trades | W/L | Coverage | Max DD |
|---|---:|---:|---:|---:|---:|---:|---:|
| champion_replay | -62.32 | -67.82 | 0.629 | 11 | 5/6 | 4.60% | 106.64 |
| early_micro_individual | 104.59 | 90.59 | 1.391 | 28 | 18/10 | 11.72% | 62.12 |
| early_micro_prior_weighted | -81.00 | -84.05 | 0.341 | 16 | 7/9 | 6.69% | 115.40 |
| early_micro_agreement | 0.00 | 0.00 | 0.000 | 0 | 0/0 | 0.00% | 0.00 |
| later_rtds | 154.18 | 143.18 | 2.035 | 22 | 16/6 | 9.21% | 54.31 |
| full_hybrid | 145.57 | 124.52 | 1.362 | 52 | 32/20 | 21.76% | 101.99 |
| full_rtds_free | 137.72 | 119.17 | 1.401 | 47 | 30/17 | 19.67% | 78.76 |
| prior_composed_replay | 41.59 | -10.91 | 1.040 | 105 | 69/36 | 43.93% | 177.32 |

## Qualified bucket layers

| Bucket | Layer | Form | RTDS | Side | VWAP | Dev stress | Design stress | Confirm stress | Trades | PF |
|---|---|---|---|---|---:|---:|---:|---:|---:|---:|
| early_40_44 | early_twap_open_interest_40_44__without_rtds_candles | individual | without_rtds_candles | up | 50 | 105.93 | 40.20 | 0.00 | 0 | 0.000 |
| early_40_44 | early_40_44__prior_weighted__without_rtds_candles | prior_weighted | without_rtds_candles | up | 50 | 108.31 | 269.36 | 0.00 | 0 | 0.000 |
| early_40_44 | early_40_44__agreement__without_rtds_candles | agreement | without_rtds_candles | up | 50 | 88.53 | 277.73 | 0.00 | 0 | 0.000 |
| early_45_49_control | early_refprice_45_49__without_rtds_candles | individual | without_rtds_candles | up | 50 | 87.01 | 26.22 | 0.00 | 0 | 0.000 |
| early_60_64 | time_specialist_60_64__with_rtds_candles | individual | with_rtds_candles | up | 50 | 164.45 | 0.53 | 245.06 | 63 | 1.412 |
| early_60_64 | early_60_64__prior_weighted__without_rtds_candles | prior_weighted | without_rtds_candles | up | 5 | 1.18 | 23.52 | 22.48 | 96 | 1.307 |
| early_65_69 | bridge_aware_65_69__without_rtds_candles | individual | without_rtds_candles | down | 50 | 59.53 | 162.58 | 21.28 | 130 | 1.059 |
| early_65_69 | time_specialist_65_69__without_rtds_candles | individual | without_rtds_candles | up | 50 | 368.33 | 143.92 | 185.57 | 76 | 1.268 |
| early_65_69 | time_specialist_65_69__with_rtds_candles | individual | with_rtds_candles | up | 50 | 234.17 | 330.66 | 316.27 | 72 | 1.504 |
| early_65_69 | early_65_69__prior_weighted__without_rtds_candles | prior_weighted | without_rtds_candles | up | 50 | 333.14 | 98.35 | 201.08 | 75 | 1.294 |
| extended_75_89_control | extended_official_75_89__with_rtds_candles | individual | with_rtds_candles | down | 50 | 144.15 | 119.00 | -153.35 | 99 | 0.885 |
| extended_75_89_control | dual_head_75_89__with_rtds_candles | individual | with_rtds_candles | down | 50 | 144.15 | 119.00 | -153.35 | 99 | 0.885 |
| extended_75_89_control | extended_75_89_control__prior_weighted__with_rtds_candles | prior_weighted | with_rtds_candles | down | 50 | 144.15 | 119.00 | -153.35 | 99 | 0.885 |
| extended_75_89_control | extended_75_89_control__agreement__with_rtds_candles | agreement | with_rtds_candles | down | 50 | 144.15 | 119.00 | -153.35 | 99 | 0.885 |
| middle_150_169 | settlement_150_169__with_rtds_candles | individual | with_rtds_candles | down | 50 | 363.95 | 8.30 | -146.87 | 357 | 1.009 |
| middle_150_169 | agreement_150_169__with_rtds_candles | individual | with_rtds_candles | down | 50 | 390.90 | 79.78 | 2.18 | 406 | 1.054 |
| middle_150_169 | price_time_150_169__without_rtds_candles | individual | without_rtds_candles | down | 50 | 173.38 | 22.42 | 147.09 | 207 | 1.124 |
| middle_150_169 | price_time_150_169__with_rtds_candles | individual | with_rtds_candles | down | 50 | 299.87 | 283.19 | -254.65 | 344 | 0.976 |
| middle_150_169 | middle_150_169__prior_weighted__without_rtds_candles | prior_weighted | without_rtds_candles | down | 50 | 283.70 | 126.75 | 249.32 | 199 | 1.186 |
| middle_150_169 | middle_150_169__agreement__without_rtds_candles | agreement | without_rtds_candles | down | 50 | 283.70 | 126.75 | 249.32 | 199 | 1.186 |
| middle_150_169 | middle_150_169__prior_weighted__with_rtds_candles | prior_weighted | with_rtds_candles | down | 50 | 354.90 | 158.21 | 77.25 | 362 | 1.077 |
| middle_150_169 | middle_150_169__agreement__with_rtds_candles | agreement | with_rtds_candles | down | 50 | 354.90 | 158.21 | 77.25 | 362 | 1.077 |
| late_185_209 | multivenue_185_209__without_rtds_candles | individual | without_rtds_candles | down | 50 | 157.60 | 79.99 | 804.47 | 127 | 1.852 |
| late_185_209 | multivenue_185_209__with_rtds_candles | individual | with_rtds_candles | down | 50 | 170.49 | 137.32 | 561.91 | 124 | 1.558 |
| late_185_209 | time_specialist_185_209__without_rtds_candles | individual | without_rtds_candles | down | 50 | 157.60 | 79.99 | 804.47 | 127 | 1.852 |
| late_185_209 | time_specialist_185_209__with_rtds_candles | individual | with_rtds_candles | down | 50 | 170.49 | 137.32 | 561.91 | 124 | 1.558 |
| late_185_209 | late_185_209__prior_weighted__without_rtds_candles | prior_weighted | without_rtds_candles | down | 50 | 157.60 | 79.99 | 804.47 | 127 | 1.852 |
| late_185_209 | late_185_209__agreement__without_rtds_candles | agreement | without_rtds_candles | down | 50 | 157.60 | 79.99 | 804.47 | 127 | 1.852 |
| late_185_209 | late_185_209__prior_weighted__with_rtds_candles | prior_weighted | with_rtds_candles | down | 50 | 170.49 | 137.32 | 561.91 | 124 | 1.558 |
| late_185_209 | late_185_209__agreement__with_rtds_candles | agreement | with_rtds_candles | down | 50 | 170.49 | 137.32 | 561.91 | 124 | 1.558 |

## Integrity and limitations

- Model fitting ends before the August 14 settlement cutover; August 14 is transition-audit-only.
- Policy fitting, observed design validation, and untouched confirmation are chronological and disjoint.
- August 20–26 is design evidence, not relabeled as an unseen test.
- Confirmation execution is a read-only Parquet snapshot of the established current orderbook table.
- Early causal feature coverage ends at 2026-08-27T00:00:00+00:00 (exclusive); all-router comparisons therefore include the exact common window above.
- No database row, table, schema, source, ingester, runtime model, trading process, or image was changed.
- VWAP replay assumes the recorded ask ladder was fillable and does not model queue position.
