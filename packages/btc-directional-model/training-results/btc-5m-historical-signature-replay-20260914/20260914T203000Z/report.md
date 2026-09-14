# BTC five-minute historical model signature replay

This is a read-only replay of archived model outputs. No model was trained, and no missing prediction was reconstructed.

## Inventory

- Canonical archived files examined: **697**
- Unique training-result runs found: **49**
- Candidate Parquet ledgers inspected: **174**
- Replayable ledgers: **135**
- Unique replayable model signatures: **232**
- Normalized archived trades: **191793**
- Unavailable ledgers retained in inventory: **39**

## Feasibility

A signature below is descriptive evidence, not a promotion gate or a new standard. The screening view retains at least 30 trades, positive raw and stressed PnL, PF above one, and either two positive blocks/months or 100 trades.
Within each bucket, the feasibility sketch prefers broader positive evidence support, then stressed PnL per trade and sample size; it does not simply choose the largest PnL.

| Bucket width | Replay cells | Positive stressed cells | Screened cells | Covered bucket starts |
|---:|---:|---:|---:|---:|
| 5s | 4629 | 2330 | 217 | 41 |
| 10s | 2464 | 1207 | 227 | 22 |
| 15s | 1697 | 817 | 190 | 13 |
| 30s | 934 | 417 | 148 | 7 |

## Retrospective aggregate feasibility sketches

These sketches select signatures and replay them on the same historical evidence. They are intentionally optimistic and are not tournament results.

| Granularity | Bucket signatures | Trades | PnL | Stress PnL | PF | W/L | UP/DOWN | Avg entry | Date range |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|
| 5s | 41 | 3571 | 1595.11 | 1416.56 | 1.621 | 2721/850 | 1645/1926 | 101.1 | 2026-06-29 16:30:00+00:00 – 2026-09-13 23:35:00+00:00 |
| 10s | 22 | 2747 | 649.92 | 512.57 | 1.282 | 1998/749 | 1095/1652 | 109.4 | 2026-06-29 16:30:00+00:00 – 2026-09-13 21:55:00+00:00 |
| 15s | 13 | 1776 | 498.28 | 409.48 | 1.330 | 1289/487 | 605/1171 | 89.7 | 2026-06-29 17:15:00+00:00 – 2026-09-13 11:30:00+00:00 |
| 30s | 7 | 1845 | 434.36 | 342.11 | 1.245 | 1254/591 | 739/1106 | 89.6 | 2026-06-29 17:00:00+00:00 – 2026-09-13 21:55:00+00:00 |

## Selected screened historical signature per 10-second bucket

| Seconds | Model signature | Trades | PnL | Stress | PF | W/L | UP/DOWN | Positive blocks | Positive months | Range |
|---:|---|---:|---:|---:|---:|---:|---:|---:|---:|---|
| 0-9 | — | 0 | — | — | — | — | — | — | — | No signature passed the descriptive screen |
| 10-19 | `btc-5m-full-window-payoff-challenger-20260525-20260802/20260821T204106Z/oof_expert_distilled_admission` | 175 | 31.31 | 22.56 | 1.163 | 108/67 | 160/15 | 1/1 | 2/2 | 2026-07-23 18:00:00+00:00 – 2026-08-01 23:55:00+00:00 |
| 20-29 | — | 0 | — | — | — | — | — | — | — | No signature passed the descriptive screen |
| 30-39 | `btc-5m-fair-value-challenger-tournament-20260525-20260802/20260822T152633Z/stratified_fair_value` | 228 | 46.77 | 35.37 | 1.222 | 163/65 | 75/153 | 1/1 | 3/3 | 2026-06-30 19:10:00+00:00 – 2026-08-01 22:35:00+00:00 |
| 40-49 | `btc-5m-fair-value-challenger-tournament-20260525-20260802/20260822T152633Z/specialist_distilled_fair_value` | 230 | 61.33 | 49.83 | 1.286 | 163/67 | 123/107 | 1/1 | 2/2 | 2026-06-29 18:10:00+00:00 – 2026-07-30 11:50:00+00:00 |
| 50-59 | `btc-5m-fair-value-challenger-tournament-20260525-20260802/20260822T152633Z/exogenous_fair_value` | 228 | 60.53 | 49.13 | 1.298 | 166/62 | 80/148 | 1/1 | 2/2 | 2026-06-29 17:15:00+00:00 – 2026-07-31 23:50:00+00:00 |
| 60-69 | `btc-5m-fair-value-challenger-tournament-20260525-20260802/20260822T152633Z/exogenous_fair_value` | 141 | 18.87 | 11.82 | 1.136 | 98/43 | 73/68 | 1/1 | 2/3 | 2026-06-29 16:55:00+00:00 – 2026-08-01 20:40:00+00:00 |
| 70-79 | `btc-5m-time-bucket-specialist-tournament-20260321-20260914/20260914T171438Z/extended_specialist_official__without_rtds_candles` | 59 | 18.53 | 15.58 | 1.318 | 36/23 | 0/59 | 2/2 | 1/1 | 2026-09-02 08:25:00+00:00 – 2026-09-13 03:10:00+00:00 |
| 80-89 | `btc-5m-time-bucket-specialist-tournament-20260321-20260914/20260914T171438Z/extended_specialist_official__without_rtds_candles` | 75 | 35.66 | 31.91 | 1.514 | 49/26 | 0/75 | 2/2 | 1/1 | 2026-09-01 02:05:00+00:00 – 2026-09-13 11:30:00+00:00 |
| 90-99 | `btc-5m-fair-value-challenger-tournament-20260525-20260802/20260822T152633Z/stratified_fair_value` | 386 | 31.47 | 12.17 | 1.091 | 289/97 | 143/243 | 1/1 | 3/3 | 2026-06-30 12:00:00+00:00 – 2026-08-01 23:35:00+00:00 |
| 100-109 | `btc-5m-early-entry-economic-tournament-20260321-20260826/20260901T194936Z/crossvenue_middle_specialist` | 97 | 17.38 | 12.53 | 1.223 | 70/27 | 46/51 | 2/2 | 1/1 | 2026-08-15 04:35:00+00:00 – 2026-08-25 23:05:00+00:00 |
| 110-119 | `btc-5m-time-bucket-specialist-tournament-20260321-20260914/20260914T171438Z/crossvenue_middle_110__without_rtds_candles` | 122 | 29.08 | 22.98 | 1.221 | 71/51 | 0/122 | 2/2 | 1/1 | 2026-09-01 06:55:00+00:00 – 2026-09-13 21:55:00+00:00 |
| 120-129 | `btc-5m-early-entry-economic-tournament-20260321-20260826/20260901T194936Z/external_flow_only_control` | 86 | 20.80 | 16.50 | 1.315 | 63/23 | 45/41 | 2/2 | 1/1 | 2026-08-14 21:00:00+00:00 – 2026-08-25 22:55:00+00:00 |
| 130-139 | `btc-5m-amalgamated-bucket-tournament-20260321-20260901/20260914T145421Z/middle_120_149__prior_weighted__without_rtds_candles` | 118 | 60.70 | 54.80 | 1.637 | 86/32 | 47/71 | 2/2 | 1/1 | 2026-08-26 01:50:00+00:00 – 2026-08-31 22:30:00+00:00 |
| 140-149 | `btc-5m-early-entry-economic-tournament-20260321-20260826/20260901T194936Z/crossvenue_middle_specialist` | 36 | 15.44 | 13.64 | 1.550 | 26/10 | 14/22 | 2/2 | 1/1 | 2026-08-14 19:00:00+00:00 – 2026-08-25 22:15:00+00:00 |
| 150-159 | `btc-5m-early-entry-economic-tournament-20260321-20260826/20260901T194936Z/middle_agreement_ensemble` | 37 | 3.63 | 1.78 | 1.115 | 26/11 | 26/11 | 2/2 | 1/1 | 2026-08-14 21:25:00+00:00 – 2026-08-25 07:40:00+00:00 |
| 160-169 | `btc-5m-early-entry-economic-tournament-20260321-20260826/20260901T194936Z/latent_boundary_margin_specialist` | 32 | 11.94 | 10.34 | 1.654 | 24/8 | 13/19 | 2/2 | 1/1 | 2026-08-15 00:40:00+00:00 – 2026-08-25 18:15:00+00:00 |
| 170-179 | `btc-5m-fair-value-challenger-tournament-20260525-20260802/20260822T152633Z/chainlink_stratified_payoff` | 58 | 23.35 | 20.45 | 1.617 | 46/12 | 19/39 | 1/1 | 3/3 | 2026-06-29 17:15:00+00:00 – 2026-08-01 05:40:00+00:00 |
| 180-189 | `btc-5m-fair-value-challenger-tournament-20260525-20260802/20260822T152633Z/terminal_margin_fair_value` | 369 | 53.41 | 34.96 | 1.244 | 305/64 | 163/206 | 1/1 | 2/3 | 2026-06-29 16:55:00+00:00 – 2026-08-01 21:55:00+00:00 |
| 190-199 | `btc-5m-fair-value-challenger-tournament-20260525-20260802/20260822T152633Z/stratified_fair_value` | 98 | 40.29 | 35.39 | 1.604 | 77/21 | 35/63 | 1/1 | 2/2 | 2026-06-30 10:25:00+00:00 – 2026-07-31 21:10:00+00:00 |
| 200-209 | `btc-5m-fair-value-challenger-tournament-20260525-20260802/20260822T152633Z/terminal_margin_fair_value` | 294 | 110.07 | 95.37 | 1.717 | 246/48 | 141/153 | 1/1 | 1/3 | 2026-06-29 16:30:00+00:00 – 2026-08-01 23:55:00+00:00 |
| 210-219 | `btc-5m-amalgamated-bucket-tournament-20260321-20260901/20260914T145421Z/price_control_terminal_210_239__without_rtds_candles` | 84 | 76.08 | 71.88 | 2.816 | 68/16 | 0/84 | 3/3 | 1/1 | 2026-08-26 00:55:00+00:00 – 2026-08-31 21:45:00+00:00 |
| 220-229 | `btc-5m-fair-value-challenger-tournament-20260525-20260802/20260822T152633Z/exogenous_fair_value` | 45 | 12.96 | 10.71 | 1.379 | 32/13 | 17/28 | 1/1 | 2/2 | 2026-06-30 18:00:00+00:00 – 2026-07-30 02:40:00+00:00 |
| 230-239 | `btc-5m-fair-value-challenger-tournament-20260525-20260802/20260822T152633Z/tail_weighted_fair_value` | 34 | 12.87 | 11.17 | 1.544 | 27/7 | 14/20 | 1/1 | 3/3 | 2026-06-30 00:55:00+00:00 – 2026-08-01 10:40:00+00:00 |

## Included latest bucket-screening results

The September fixed-VWAP5 screening artifact is included as supporting evidence, not mislabeled as an aggregate tournament.

| Variant | Assigned bucket | RTDS | Period | Trades | PnL | Stress | PF | W/L |
|---|---|---|---|---:|---:|---:|---:|---:|
| `bridge_aware_specialist__without_rtds_candles` | bridge_early | without_rtds_candles | confirmation | 109 | 36.47 | 31.02 | 1.630 | 94/15 |
| `middle_q5_admission__without_rtds_candles` | middle_q5_down | without_rtds_candles | confirmation | 114 | 32.49 | 26.79 | 1.272 | 70/44 |
| `middle_q5_admission__with_rtds_candles` | middle_q5_down | with_rtds_candles | confirmation | 144 | 12.03 | 4.83 | 1.072 | 77/67 |
| `bridge_aware_specialist__with_rtds_candles` | bridge_early | with_rtds_candles | confirmation | 138 | 7.61 | 0.71 | 1.053 | 93/45 |
| `extended_specialist_official__without_rtds_candles` | extended_specialist_down | without_rtds_candles | confirmation | 221 | 5.64 | -5.41 | 1.022 | 121/100 |
| `specialist_dual_head__without_rtds_candles` | extended_specialist_down | without_rtds_candles | confirmation | 221 | 5.64 | -5.41 | 1.022 | 121/100 |
| `crossvenue_middle_110__without_rtds_candles` | middle_q5_down | without_rtds_candles | confirmation | 190 | 3.46 | -6.04 | 1.016 | 106/84 |
| `time_specialist_late__with_rtds_candles` | late_discovery | with_rtds_candles | confirmation | 247 | 3.34 | -9.01 | 1.013 | 160/87 |
| `multivenue_late__with_rtds_candles` | late_discovery | with_rtds_candles | confirmation | 247 | 3.34 | -9.01 | 1.013 | 160/87 |
| `time_specialist_early__without_rtds_candles` | bridge_early | without_rtds_candles | confirmation | 376 | 6.87 | -11.93 | 1.016 | 197/179 |
| `specialist_dual_head__with_rtds_candles` | extended_specialist_down | with_rtds_candles | confirmation | 131 | -18.44 | -24.99 | 0.870 | 90/41 |
| `extended_specialist_official__with_rtds_candles` | extended_specialist_down | with_rtds_candles | confirmation | 131 | -18.44 | -24.99 | 0.870 | 90/41 |
| `crossvenue_middle_110__with_rtds_candles` | middle_q5_down | with_rtds_candles | confirmation | 312 | -29.51 | -45.11 | 0.923 | 174/138 |
| `crossvenue_settlement_middle__with_rtds_candles` | settlement_middle | with_rtds_candles | confirmation | 278 | -43.44 | -57.34 | 0.872 | 159/119 |
| `time_specialist_early__with_rtds_candles` | bridge_early | with_rtds_candles | confirmation | 325 | -43.41 | -59.66 | 0.892 | 160/165 |
| `time_specialist_late__without_rtds_candles` | late_discovery | without_rtds_candles | confirmation | 402 | -42.74 | -62.84 | 0.893 | 278/124 |
| `multivenue_late__without_rtds_candles` | late_discovery | without_rtds_candles | confirmation | 402 | -42.74 | -62.84 | 0.893 | 278/124 |
| `price_control_terminal__with_rtds_candles` | terminal_discovery | with_rtds_candles | confirmation | 363 | -46.18 | -64.33 | 0.887 | 199/164 |
| `settlement_aligned_terminal__without_rtds_candles` | terminal_discovery | without_rtds_candles | confirmation | 618 | -34.01 | -64.91 | 0.942 | 421/197 |
| `middle_agreement__with_rtds_candles` | settlement_middle | with_rtds_candles | confirmation | 653 | -32.34 | -64.99 | 0.953 | 413/240 |
| `multivenue_consensus__without_rtds_candles` | multivenue_middle | without_rtds_candles | confirmation | 767 | -28.13 | -66.48 | 0.966 | 467/300 |
| `settlement_aligned_terminal__with_rtds_candles` | terminal_discovery | with_rtds_candles | confirmation | 587 | -40.32 | -69.67 | 0.930 | 397/190 |
| `kraken_crossvenue_middle__without_rtds_candles` | multivenue_middle | without_rtds_candles | confirmation | 502 | -55.41 | -80.51 | 0.907 | 249/253 |
| `price_time_calibrated_middle__without_rtds_candles` | settlement_middle | without_rtds_candles | confirmation | 618 | -51.24 | -82.14 | 0.924 | 381/237 |
| `kraken_crossvenue_terminal__without_rtds_candles` | terminal_discovery | without_rtds_candles | confirmation | 438 | -63.39 | -85.29 | 0.859 | 282/156 |
| `price_control_terminal__without_rtds_candles` | terminal_discovery | without_rtds_candles | confirmation | 437 | -67.44 | -89.29 | 0.851 | 284/153 |
| `middle_agreement__without_rtds_candles` | settlement_middle | without_rtds_candles | confirmation | 574 | -68.65 | -97.35 | 0.871 | 422/152 |
| `kraken_crossvenue_terminal__with_rtds_candles` | terminal_discovery | with_rtds_candles | confirmation | 440 | -75.60 | -97.60 | 0.850 | 218/222 |
| `price_time_calibrated_middle__with_rtds_candles` | settlement_middle | with_rtds_candles | confirmation | 359 | -85.58 | -103.53 | 0.812 | 184/175 |
| `multivenue_consensus__with_rtds_candles` | multivenue_middle | with_rtds_candles | confirmation | 687 | -98.13 | -132.48 | 0.886 | 369/318 |
| `crossvenue_settlement_middle__without_rtds_candles` | settlement_middle | without_rtds_candles | confirmation | 629 | -101.99 | -133.44 | 0.858 | 380/249 |
| `kraken_crossvenue_middle__with_rtds_candles` | multivenue_middle | with_rtds_candles | confirmation | 748 | -99.80 | -137.20 | 0.892 | 398/350 |
| `settlement_aligned_terminal__with_rtds_candles` | terminal_discovery | with_rtds_candles | sealed | 479 | 293.33 | 269.38 | 1.944 | 372/107 |
| `price_control_terminal__with_rtds_candles` | terminal_discovery | with_rtds_candles | sealed | 342 | 284.80 | 267.70 | 2.269 | 242/100 |
| `settlement_aligned_terminal__without_rtds_candles` | terminal_discovery | without_rtds_candles | sealed | 521 | 282.03 | 255.98 | 1.794 | 395/126 |
| `kraken_crossvenue_terminal__with_rtds_candles` | terminal_discovery | with_rtds_candles | sealed | 371 | 269.57 | 251.02 | 2.006 | 244/127 |
| `price_control_terminal__without_rtds_candles` | terminal_discovery | without_rtds_candles | sealed | 404 | 256.66 | 236.46 | 2.014 | 306/98 |
| `kraken_crossvenue_terminal__without_rtds_candles` | terminal_discovery | without_rtds_candles | sealed | 405 | 254.39 | 234.14 | 1.992 | 306/99 |
| `time_specialist_late__with_rtds_candles` | late_discovery | with_rtds_candles | sealed | 259 | 214.61 | 201.66 | 2.250 | 201/58 |
| `multivenue_late__with_rtds_candles` | late_discovery | with_rtds_candles | sealed | 259 | 214.61 | 201.66 | 2.250 | 201/58 |
| `time_specialist_late__without_rtds_candles` | late_discovery | without_rtds_candles | sealed | 363 | 216.20 | 198.05 | 1.898 | 285/78 |
| `multivenue_late__without_rtds_candles` | late_discovery | without_rtds_candles | sealed | 363 | 216.20 | 198.05 | 1.898 | 285/78 |
| `crossvenue_settlement_middle__without_rtds_candles` | settlement_middle | without_rtds_candles | sealed | 483 | 182.40 | 158.25 | 1.437 | 333/150 |
| `price_time_calibrated_middle__with_rtds_candles` | settlement_middle | with_rtds_candles | sealed | 347 | 165.50 | 148.15 | 1.538 | 227/120 |
| `price_time_calibrated_middle__without_rtds_candles` | settlement_middle | without_rtds_candles | sealed | 501 | 169.77 | 144.72 | 1.387 | 340/161 |
| `crossvenue_settlement_middle__with_rtds_candles` | settlement_middle | with_rtds_candles | sealed | 268 | 156.99 | 143.59 | 1.718 | 192/76 |
| `middle_agreement__with_rtds_candles` | settlement_middle | with_rtds_candles | sealed | 517 | 128.97 | 103.12 | 1.269 | 348/169 |
| `middle_agreement__without_rtds_candles` | settlement_middle | without_rtds_candles | sealed | 449 | 112.74 | 90.29 | 1.330 | 350/99 |
| `kraken_crossvenue_middle__with_rtds_candles` | multivenue_middle | with_rtds_candles | sealed | 542 | 109.04 | 81.94 | 1.189 | 317/225 |
| `extended_specialist_official__without_rtds_candles` | extended_specialist_down | without_rtds_candles | sealed | 188 | 86.94 | 77.54 | 1.520 | 118/70 |
| `specialist_dual_head__without_rtds_candles` | extended_specialist_down | without_rtds_candles | sealed | 188 | 86.94 | 77.54 | 1.520 | 118/70 |
| `crossvenue_middle_110__without_rtds_candles` | middle_q5_down | without_rtds_candles | sealed | 172 | 78.49 | 69.89 | 1.502 | 111/61 |
| `kraken_crossvenue_middle__without_rtds_candles` | multivenue_middle | without_rtds_candles | sealed | 444 | 91.98 | 69.78 | 1.201 | 253/191 |
| `middle_q5_admission__with_rtds_candles` | middle_q5_down | with_rtds_candles | sealed | 121 | 74.21 | 68.16 | 1.678 | 78/43 |
| `crossvenue_middle_110__with_rtds_candles` | middle_q5_down | with_rtds_candles | sealed | 224 | 76.21 | 65.01 | 1.339 | 140/84 |
| `specialist_dual_head__with_rtds_candles` | extended_specialist_down | with_rtds_candles | sealed | 127 | 67.27 | 60.92 | 1.781 | 98/29 |
| `extended_specialist_official__with_rtds_candles` | extended_specialist_down | with_rtds_candles | sealed | 127 | 67.27 | 60.92 | 1.781 | 98/29 |
| `multivenue_consensus__without_rtds_candles` | multivenue_middle | without_rtds_candles | sealed | 603 | 86.19 | 56.04 | 1.142 | 383/220 |
| `time_specialist_early__with_rtds_candles` | bridge_early | with_rtds_candles | sealed | 298 | 69.11 | 54.21 | 1.221 | 169/129 |
| `middle_q5_admission__without_rtds_candles` | middle_q5_down | without_rtds_candles | sealed | 108 | 48.65 | 43.25 | 1.472 | 70/38 |
| `time_specialist_early__without_rtds_candles` | bridge_early | without_rtds_candles | sealed | 316 | 47.35 | 31.55 | 1.136 | 173/143 |
| `multivenue_consensus__with_rtds_candles` | multivenue_middle | with_rtds_candles | sealed | 498 | 33.39 | 8.49 | 1.058 | 280/218 |
| `bridge_aware_specialist__without_rtds_candles` | bridge_early | without_rtds_candles | sealed | 107 | 12.51 | 7.16 | 1.155 | 85/22 |
| `bridge_aware_specialist__with_rtds_candles` | bridge_early | with_rtds_candles | sealed | 139 | 2.18 | -4.77 | 1.014 | 89/50 |

## Interpretation limits

- Signatures preserve archived outputs, but some archives contain only selected trades rather than every prediction timestamp.
- Evidence windows differ across historical runs. Cross-signature PnL is descriptive and not a common-window tournament comparison.
- Every replayed trade is standardized to five shares. When only an archived entry price is present, Q5 PnL is reconstructed at that archived price; this removes archived quantity scaling but cannot recreate a missing VWAP5 book snapshot.
- The aggregate sketches select and evaluate on the same historical evidence; they demonstrate mechanical feasibility only.
- Multiple archived copies of the same relative artifact are deduplicated, preferring the current worktree copy.
- Missing direction, timestamp, or auditable five-share economics causes a ledger to remain inventoried but excluded from PnL replay.
