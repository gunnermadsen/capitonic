# btc-5m-amalgamated-bucket-router

Run: `20260914T145421Z`
Qualification: **trained_evaluated_did_not_outperform_champion**
Selected router: **late_only_rtds_free**
Selection evidence: **design**

## Frozen hypothesis

Historically successful model families are retrained inside the time slices where they showed edge; individual, prior-weighted, and agreement-gated descendants are then folded into a no-trade-by-default router.

## Router comparison — chronological confirmation (not globally blind)

| Router | VWAP | PnL | Stress | PF | Trades | UP/DOWN | W/L | Coverage | Avg entry | Recovery | Max DD |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| champion_replay | 5 | 8.29 | 1.64 | 1.057 | 133 | 133/0 | 80/53 | 8.00% | 63.5 | 1.428 | 31.06 |
| amalgamated_prior_weighted_rtds_free | 5 | 131.68 | 86.88 | 1.144 | 896 | 430/466 | 584/312 | 53.91% | 122.0 | 1.637 | 54.76 |
| amalgamated_agreement_rtds_free | 5 | 171.48 | 127.18 | 1.195 | 886 | 411/475 | 589/297 | 53.31% | 123.8 | 1.660 | 46.04 |
| best_layer_rtds_free | 5 | 119.49 | 94.29 | 1.247 | 504 | 0/504 | 350/154 | 30.32% | 128.9 | 1.823 | 52.21 |
| reservation_amalgamated_rtds_free | 5 | 184.39 | 142.09 | 1.227 | 846 | 333/513 | 578/268 | 50.90% | 133.4 | 1.758 | 64.46 |
| late_only_rtds_free | 5 | 66.49 | 53.69 | 1.274 | 256 | 0/256 | 179/77 | 15.40% | 159.6 | 1.824 | 34.04 |
| terminal_only | 5 | 103.61 | 97.36 | 2.503 | 125 | 0/125 | 100/25 | 7.52% | 215.7 | 1.598 | 8.90 |
| selective_rtds_best_layer | 5 | 110.92 | 83.67 | 1.223 | 545 | 80/465 | 391/154 | 32.79% | 132.0 | 2.076 | 56.63 |
| prior_composed_replay | 5 | 222.54 | 169.89 | 1.237 | 1053 | 289/764 | 744/309 | 63.36% | 114.7 | 1.946 | 69.76 |

## Common-window comparison — 2026-08-26 to 2026-08-27 (end exclusive)

| Router | PnL | Stress | PF | Trades | W/L | Coverage | Max DD |
|---|---:|---:|---:|---:|---:|---:|---:|
| champion_replay | -6.16 | -6.71 | 0.632 | 11 | 5/6 | 4.60% | 10.60 |
| amalgamated_prior_weighted_rtds_free | -13.19 | -17.74 | 0.881 | 91 | 50/41 | 38.08% | 29.12 |
| amalgamated_agreement_rtds_free | -16.61 | -21.11 | 0.850 | 90 | 49/41 | 37.66% | 32.54 |
| best_layer_rtds_free | 18.46 | 16.26 | 1.533 | 44 | 31/13 | 18.41% | 11.86 |
| reservation_amalgamated_rtds_free | 1.13 | -2.87 | 1.013 | 80 | 48/32 | 33.47% | 19.68 |
| late_only_rtds_free | 9.02 | 8.42 | 2.420 | 12 | 9/3 | 5.02% | 2.76 |
| terminal_only | 7.51 | 6.81 | 1.778 | 14 | 11/3 | 5.86% | 7.01 |
| selective_rtds_best_layer | 18.94 | 16.89 | 1.653 | 41 | 29/12 | 17.15% | 10.88 |
| prior_composed_replay | 7.10 | 1.80 | 1.069 | 106 | 70/36 | 44.35% | 17.68 |

## champion_replay — confirmation PnL by bucket

| Bucket | Seconds | PnL | Stress | PnL/trade | PF | Trades | UP/DOWN | W/L | Loss rate | Coverage | Avg entry | Confidence | Recovery | Max DD |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| early_40_49 | 40-49 | 0.00 | 0.00 | 0.000 | 0.000 | 0 | 0/0 | 0/0 | 0.00% | 0.00% | 0.0 | 0.000 | 0.000 | 0.00 |
| early_60_74 | 60-74 | 8.29 | 1.64 | 0.062 | 1.057 | 133 | 133/0 | 80/53 | 39.85% | 8.00% | 63.5 | 0.752 | 1.428 | 31.06 |
| boundary_110_119 | 110-119 | 0.00 | 0.00 | 0.000 | 0.000 | 0 | 0/0 | 0/0 | 0.00% | 0.00% | 0.0 | 0.000 | 0.000 | 0.00 |
| middle_120_149 | 120-149 | 0.00 | 0.00 | 0.000 | 0.000 | 0 | 0/0 | 0/0 | 0.00% | 0.00% | 0.0 | 0.000 | 0.000 | 0.00 |
| middle_150_169 | 150-169 | 0.00 | 0.00 | 0.000 | 0.000 | 0 | 0/0 | 0/0 | 0.00% | 0.00% | 0.0 | 0.000 | 0.000 | 0.00 |
| late_185_209 | 185-209 | 0.00 | 0.00 | 0.000 | 0.000 | 0 | 0/0 | 0/0 | 0.00% | 0.00% | 0.0 | 0.000 | 0.000 | 0.00 |
| terminal_210_239 | 210-239 | 0.00 | 0.00 | 0.000 | 0.000 | 0 | 0/0 | 0/0 | 0.00% | 0.00% | 0.0 | 0.000 | 0.000 | 0.00 |

## amalgamated_prior_weighted_rtds_free — confirmation PnL by bucket

| Bucket | Seconds | PnL | Stress | PnL/trade | PF | Trades | UP/DOWN | W/L | Loss rate | Coverage | Avg entry | Confidence | Recovery | Max DD |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| early_40_49 | 40-49 | 0.00 | 0.00 | 0.000 | 0.000 | 0 | 0/0 | 0/0 | 0.00% | 0.00% | 0.0 | 0.000 | 0.000 | 0.00 |
| early_60_74 | 60-74 | -20.50 | -30.10 | -0.107 | 0.917 | 192 | 192/0 | 99/93 | 48.44% | 11.55% | 63.4 | 0.627 | 1.161 | 49.44 |
| boundary_110_119 | 110-119 | 0.00 | 0.00 | 0.000 | 0.000 | 0 | 0/0 | 0/0 | 0.00% | 0.00% | 0.0 | 0.000 | 0.000 | 0.00 |
| middle_120_149 | 120-149 | 94.79 | 66.04 | 0.165 | 1.167 | 575 | 238/337 | 392/183 | 31.83% | 34.60% | 125.2 | 0.778 | 1.836 | 50.57 |
| middle_150_169 | 150-169 | 17.86 | 15.96 | 0.470 | 1.666 | 38 | 0/38 | 29/9 | 23.68% | 2.29% | 157.2 | 0.823 | 1.934 | 15.25 |
| late_185_209 | 185-209 | 12.72 | 11.52 | 0.530 | 1.497 | 24 | 0/24 | 14/10 | 41.67% | 1.44% | 190.6 | 0.840 | 0.935 | 17.19 |
| terminal_210_239 | 210-239 | 26.81 | 23.46 | 0.400 | 1.566 | 67 | 0/67 | 50/17 | 25.37% | 4.03% | 217.5 | 0.809 | 1.878 | 9.51 |

## amalgamated_agreement_rtds_free — confirmation PnL by bucket

| Bucket | Seconds | PnL | Stress | PnL/trade | PF | Trades | UP/DOWN | W/L | Loss rate | Coverage | Avg entry | Confidence | Recovery | Max DD |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| early_40_49 | 40-49 | 0.00 | 0.00 | 0.000 | 0.000 | 0 | 0/0 | 0/0 | 0.00% | 0.00% | 0.0 | 0.000 | 0.000 | 0.00 |
| early_60_74 | 60-74 | -0.21 | -8.71 | -0.001 | 0.999 | 170 | 170/0 | 92/78 | 45.88% | 10.23% | 63.6 | 0.634 | 1.181 | 27.95 |
| boundary_110_119 | 110-119 | 0.00 | 0.00 | 0.000 | 0.000 | 0 | 0/0 | 0/0 | 0.00% | 0.00% | 0.0 | 0.000 | 0.000 | 0.00 |
| middle_120_149 | 120-149 | 110.94 | 81.84 | 0.191 | 1.195 | 582 | 241/341 | 400/182 | 31.27% | 35.02% | 125.3 | 0.778 | 1.839 | 43.72 |
| middle_150_169 | 150-169 | 18.18 | 16.08 | 0.433 | 1.592 | 42 | 0/42 | 32/10 | 23.81% | 2.53% | 157.3 | 0.820 | 2.010 | 19.14 |
| late_185_209 | 185-209 | 15.76 | 14.51 | 0.630 | 1.616 | 25 | 0/25 | 15/10 | 40.00% | 1.50% | 190.8 | 0.840 | 0.928 | 17.19 |
| terminal_210_239 | 210-239 | 26.81 | 23.46 | 0.400 | 1.566 | 67 | 0/67 | 50/17 | 25.37% | 4.03% | 217.5 | 0.809 | 1.878 | 9.51 |

## best_layer_rtds_free — confirmation PnL by bucket

| Bucket | Seconds | PnL | Stress | PnL/trade | PF | Trades | UP/DOWN | W/L | Loss rate | Coverage | Avg entry | Confidence | Recovery | Max DD |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| early_40_49 | 40-49 | 0.00 | 0.00 | 0.000 | 0.000 | 0 | 0/0 | 0/0 | 0.00% | 0.00% | 0.0 | 0.000 | 0.000 | 0.00 |
| early_60_74 | 60-74 | 4.12 | -1.13 | 0.039 | 1.034 | 105 | 0/105 | 66/39 | 37.14% | 6.32% | 62.5 | 0.796 | 1.637 | 37.42 |
| boundary_110_119 | 110-119 | 0.00 | 0.00 | 0.000 | 0.000 | 0 | 0/0 | 0/0 | 0.00% | 0.00% | 0.0 | 0.000 | 0.000 | 0.00 |
| middle_120_149 | 120-149 | -0.78 | -14.03 | -0.003 | 0.997 | 265 | 0/265 | 180/85 | 32.08% | 15.94% | 126.8 | 0.825 | 2.123 | 44.19 |
| middle_150_169 | 150-169 | 36.61 | 33.71 | 0.631 | 1.946 | 58 | 0/58 | 45/13 | 22.41% | 3.49% | 156.8 | 0.802 | 1.779 | 9.10 |
| late_185_209 | 185-209 | 36.46 | 34.96 | 1.215 | 2.680 | 30 | 0/30 | 21/9 | 30.00% | 1.81% | 190.3 | 0.832 | 0.871 | 8.81 |
| terminal_210_239 | 210-239 | 43.08 | 40.78 | 0.937 | 3.154 | 46 | 0/46 | 38/8 | 17.39% | 2.77% | 217.6 | 0.908 | 1.506 | 6.15 |

## reservation_amalgamated_rtds_free — confirmation PnL by bucket

| Bucket | Seconds | PnL | Stress | PnL/trade | PF | Trades | UP/DOWN | W/L | Loss rate | Coverage | Avg entry | Confidence | Recovery | Max DD |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| early_40_49 | 40-49 | 0.00 | 0.00 | 0.000 | 0.000 | 0 | 0/0 | 0/0 | 0.00% | 0.00% | 0.0 | 0.000 | 0.000 | 0.00 |
| early_60_74 | 60-74 | -10.34 | -12.74 | -0.215 | 0.842 | 48 | 48/0 | 25/23 | 47.92% | 2.89% | 62.0 | 0.725 | 1.290 | 16.00 |
| boundary_110_119 | 110-119 | 0.00 | 0.00 | 0.000 | 0.000 | 0 | 0/0 | 0/0 | 0.00% | 0.00% | 0.0 | 0.000 | 0.000 | 0.00 |
| middle_120_149 | 120-149 | 122.17 | 89.47 | 0.187 | 1.190 | 654 | 285/369 | 447/207 | 31.65% | 39.35% | 125.2 | 0.774 | 1.814 | 66.38 |
| middle_150_169 | 150-169 | 21.54 | 19.34 | 0.490 | 1.701 | 44 | 0/44 | 34/10 | 22.73% | 2.65% | 157.3 | 0.820 | 1.999 | 19.14 |
| late_185_209 | 185-209 | 17.90 | 16.60 | 0.688 | 1.700 | 26 | 0/26 | 16/10 | 38.46% | 1.56% | 190.8 | 0.839 | 0.941 | 17.19 |
| terminal_210_239 | 210-239 | 33.13 | 29.43 | 0.448 | 1.660 | 74 | 0/74 | 56/18 | 24.32% | 4.45% | 217.6 | 0.812 | 1.874 | 9.51 |

## late_only_rtds_free — confirmation PnL by bucket

| Bucket | Seconds | PnL | Stress | PnL/trade | PF | Trades | UP/DOWN | W/L | Loss rate | Coverage | Avg entry | Confidence | Recovery | Max DD |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| early_40_49 | 40-49 | 0.00 | 0.00 | 0.000 | 0.000 | 0 | 0/0 | 0/0 | 0.00% | 0.00% | 0.0 | 0.000 | 0.000 | 0.00 |
| early_60_74 | 60-74 | 0.00 | 0.00 | 0.000 | 0.000 | 0 | 0/0 | 0/0 | 0.00% | 0.00% | 0.0 | 0.000 | 0.000 | 0.00 |
| boundary_110_119 | 110-119 | 0.00 | 0.00 | 0.000 | 0.000 | 0 | 0/0 | 0/0 | 0.00% | 0.00% | 0.0 | 0.000 | 0.000 | 0.00 |
| middle_120_149 | 120-149 | 0.00 | 0.00 | 0.000 | 0.000 | 0 | 0/0 | 0/0 | 0.00% | 0.00% | 0.0 | 0.000 | 0.000 | 0.00 |
| middle_150_169 | 150-169 | 33.66 | 23.06 | 0.159 | 1.166 | 212 | 0/212 | 152/60 | 28.30% | 12.76% | 153.2 | 0.847 | 2.172 | 26.76 |
| late_185_209 | 185-209 | 32.83 | 30.63 | 0.746 | 1.822 | 44 | 0/44 | 27/17 | 38.64% | 2.65% | 190.3 | 0.841 | 0.872 | 16.91 |
| terminal_210_239 | 210-239 | 0.00 | 0.00 | 0.000 | 0.000 | 0 | 0/0 | 0/0 | 0.00% | 0.00% | 0.0 | 0.000 | 0.000 | 0.00 |

## terminal_only — confirmation PnL by bucket

| Bucket | Seconds | PnL | Stress | PnL/trade | PF | Trades | UP/DOWN | W/L | Loss rate | Coverage | Avg entry | Confidence | Recovery | Max DD |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| early_40_49 | 40-49 | 0.00 | 0.00 | 0.000 | 0.000 | 0 | 0/0 | 0/0 | 0.00% | 0.00% | 0.0 | 0.000 | 0.000 | 0.00 |
| early_60_74 | 60-74 | 0.00 | 0.00 | 0.000 | 0.000 | 0 | 0/0 | 0/0 | 0.00% | 0.00% | 0.0 | 0.000 | 0.000 | 0.00 |
| boundary_110_119 | 110-119 | 0.00 | 0.00 | 0.000 | 0.000 | 0 | 0/0 | 0/0 | 0.00% | 0.00% | 0.0 | 0.000 | 0.000 | 0.00 |
| middle_120_149 | 120-149 | 0.00 | 0.00 | 0.000 | 0.000 | 0 | 0/0 | 0/0 | 0.00% | 0.00% | 0.0 | 0.000 | 0.000 | 0.00 |
| middle_150_169 | 150-169 | 0.00 | 0.00 | 0.000 | 0.000 | 0 | 0/0 | 0/0 | 0.00% | 0.00% | 0.0 | 0.000 | 0.000 | 0.00 |
| late_185_209 | 185-209 | 0.00 | 0.00 | 0.000 | 0.000 | 0 | 0/0 | 0/0 | 0.00% | 0.00% | 0.0 | 0.000 | 0.000 | 0.00 |
| terminal_210_239 | 210-239 | 103.61 | 97.36 | 0.829 | 2.503 | 125 | 0/125 | 100/25 | 20.00% | 7.52% | 215.7 | 0.917 | 1.598 | 8.90 |

## selective_rtds_best_layer — confirmation PnL by bucket

| Bucket | Seconds | PnL | Stress | PnL/trade | PF | Trades | UP/DOWN | W/L | Loss rate | Coverage | Avg entry | Confidence | Recovery | Max DD |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| early_40_49 | 40-49 | 0.00 | 0.00 | 0.000 | 0.000 | 0 | 0/0 | 0/0 | 0.00% | 0.00% | 0.0 | 0.000 | 0.000 | 0.00 |
| early_60_74 | 60-74 | 1.97 | -2.03 | 0.025 | 1.031 | 80 | 80/0 | 64/16 | 20.00% | 4.81% | 62.5 | 0.898 | 3.878 | 12.89 |
| boundary_110_119 | 110-119 | 0.00 | 0.00 | 0.000 | 0.000 | 0 | 0/0 | 0/0 | 0.00% | 0.00% | 0.0 | 0.000 | 0.000 | 0.00 |
| middle_120_149 | 120-149 | 4.02 | -11.93 | 0.013 | 1.012 | 319 | 0/319 | 217/102 | 31.97% | 19.19% | 126.3 | 0.830 | 2.102 | 57.24 |
| middle_150_169 | 150-169 | 41.61 | 38.41 | 0.650 | 1.993 | 64 | 0/64 | 50/14 | 21.88% | 3.85% | 156.9 | 0.806 | 1.792 | 10.94 |
| late_185_209 | 185-209 | 18.92 | 16.67 | 0.420 | 1.410 | 45 | 0/45 | 28/17 | 37.78% | 2.71% | 189.7 | 0.832 | 1.168 | 12.69 |
| terminal_210_239 | 210-239 | 44.40 | 42.55 | 1.200 | 5.849 | 37 | 0/37 | 32/5 | 13.51% | 2.23% | 218.4 | 0.901 | 1.094 | 3.50 |

## prior_composed_replay — confirmation PnL by bucket

| Bucket | Seconds | PnL | Stress | PnL/trade | PF | Trades | UP/DOWN | W/L | Loss rate | Coverage | Avg entry | Confidence | Recovery | Max DD |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| early_40_49 | 40-49 | 0.00 | 0.00 | 0.000 | 0.000 | 0 | 0/0 | 0/0 | 0.00% | 0.00% | 0.0 | 0.000 | 0.000 | 0.00 |
| early_60_74 | 60-74 | 9.60 | 5.90 | 0.130 | 1.117 | 74 | 0/74 | 48/26 | 35.14% | 4.45% | 63.0 | 0.806 | 1.653 | 25.06 |
| boundary_110_119 | 110-119 | 13.69 | 12.09 | 0.428 | 1.432 | 32 | 0/32 | 20/12 | 37.50% | 1.93% | 111.2 | 0.748 | 1.164 | 7.32 |
| middle_120_149 | 120-149 | 84.06 | 69.31 | 0.285 | 1.350 | 295 | 196/99 | 221/74 | 25.08% | 17.75% | 125.5 | 0.847 | 2.212 | 36.59 |
| middle_150_169 | 150-169 | 25.95 | 17.10 | 0.147 | 1.207 | 177 | 93/84 | 141/36 | 20.34% | 10.65% | 153.8 | 0.851 | 3.245 | 16.57 |
| late_185_209 | 185-209 | 24.59 | 23.24 | 0.911 | 2.217 | 27 | 0/27 | 20/7 | 25.93% | 1.62% | 189.8 | 0.824 | 1.288 | 4.72 |
| terminal_210_239 | 210-239 | 48.59 | 45.54 | 0.797 | 2.863 | 61 | 0/61 | 50/11 | 18.03% | 3.67% | 215.9 | 0.837 | 1.588 | 8.26 |

## August 14 settlement-source transition audit

Diagnostic only; this window never selects a model or policy.

| Router | PnL | Stress | PF | Trades | W/L | Coverage | Max DD |
|---|---:|---:|---:|---:|---:|---:|---:|
| amalgamated_prior_weighted_rtds_free | 20.04 | 17.89 | 1.583 | 43 | 31/12 | 14.93% | 8.33 |
| amalgamated_agreement_rtds_free | 23.51 | 21.36 | 1.734 | 43 | 32/11 | 14.93% | 7.13 |
| best_layer_rtds_free | 3.98 | 3.18 | 1.306 | 16 | 12/4 | 5.56% | 3.74 |
| reservation_amalgamated_rtds_free | 19.73 | 18.08 | 1.952 | 33 | 26/7 | 11.46% | 4.91 |
| late_only_rtds_free | -0.52 | -0.97 | 0.943 | 9 | 6/3 | 3.12% | 5.17 |
| terminal_only | 0.39 | 0.24 | 1.109 | 3 | 2/1 | 1.04% | 3.55 |
| selective_rtds_best_layer | 3.47 | 2.62 | 1.228 | 17 | 13/4 | 5.90% | 8.06 |

## Qualified bucket layers

| Bucket | Layer | Form | RTDS | Side | VWAP | Dev stress | Design stress | Confirm stress | Trades | PF |
|---|---|---|---|---|---:|---:|---:|---:|---:|---:|

## Integrity and limitations

- Model fitting ends before the August 14 settlement cutover; August 14 is transition-audit-only.
- Policy fitting, observed design validation, and untouched confirmation are chronological and disjoint; the transition audit is diagnostic only.
- August 20–26 is design evidence, not relabeled as an unseen test.
- Confirmation execution is a read-only Parquet snapshot of the established current orderbook table.
- Early causal feature coverage ends at 2026-08-27T00:00:00+00:00 (exclusive); all-router comparisons therefore include the exact common window above.
- No database row, table, schema, source, ingester, runtime model, trading process, or image was changed.
- VWAP replay assumes the recorded ask ladder was fillable and does not model queue position.
- Tournament qualification uses VWAP5 only; larger quantities are post-selection capacity evidence.
