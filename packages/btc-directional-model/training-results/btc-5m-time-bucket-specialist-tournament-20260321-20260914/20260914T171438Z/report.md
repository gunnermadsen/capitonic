# BTC Five-Minute Time-Bucket Specialist Tournament

Run: `20260914T171438Z`
Qualification: **trained_evaluated_not_qualified**

## Hypothesis

Historically successful model families are retrained only for the time slices where they showed edge, then the winning bucket descendants are routed together as one challenger.

## Composed sealed result

| PnL | Stress PnL | PF | Trades | Wins/Losses | Coverage | Avg entry | Recovery wins/loss | Max DD |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 181.65 | 137.25 | 1.202 | 888 | 532/356 | 53.49% | 111.9 | 1.244 | 43.50 |

## Composed untouched confirmation result

| PnL | Stress PnL | PF | Trades | Wins/Losses | Coverage | Avg entry | Recovery wins/loss | Max DD |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| -49.94 | -109.19 | 0.962 | 1185 | 672/513 | 66.54% | 120.4 | 1.361 | 154.17 |

## Bucket winners

| Bucket | Winner | RTDS | Side | VWAP | PnL | Stress | PF | Trades | UP/DOWN | W/L | Coverage | Avg entry | Recovery |
|---|---|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| early_twap_up | early_twap_single_regime | without_rtds_candles | up | 5 | 0.00 | 0.00 | 0.000 | 0 | 0/0 | 0/0 | 0.00% | 0.0 | 0.000 |
| bridge_early | time_specialist_early | without_rtds_candles | both | 5 | 47.35 | 31.55 | 1.136 | 316 | 59/257 | 173/143 | 19.04% | 62.8 | 1.065 |
| extended_specialist_down | extended_specialist_official | without_rtds_candles | down | 5 | 86.94 | 77.54 | 1.520 | 188 | 0/188 | 118/70 | 11.33% | 77.2 | 1.109 |
| middle_q5_down | crossvenue_middle_110 | without_rtds_candles | down | 5 | 78.49 | 69.89 | 1.502 | 172 | 0/172 | 111/61 | 10.36% | 110.9 | 1.212 |
| multivenue_middle | kraken_crossvenue_middle | without_rtds_candles | both | 5 | 91.98 | 69.78 | 1.201 | 444 | 208/236 | 253/191 | 26.75% | 125.5 | 1.103 |
| settlement_middle | middle_agreement | with_rtds_candles | both | 5 | 128.97 | 103.12 | 1.269 | 517 | 213/304 | 348/169 | 31.14% | 153.0 | 1.623 |
| late_discovery | multivenue_late | with_rtds_candles | both | 5 | 214.61 | 201.66 | 2.250 | 259 | 79/180 | 201/58 | 15.60% | 189.2 | 1.540 |
| terminal_discovery | settlement_aligned_terminal | with_rtds_candles | both | 5 | 293.33 | 269.38 | 1.944 | 479 | 208/271 | 372/107 | 28.86% | 214.1 | 1.789 |

## All candidate variants — sealed test

| Bucket | Candidate | RTDS | Side | VWAP | Confidence | PnL | Stress | PF | Trades | UP/DOWN | W/L | Coverage | Avg entry | Recovery |
|---|---|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| early_twap_up | early_twap_open_interest | without_rtds_candles | up | 5 | 0.65 | 0.00 | 0.00 | 0.000 | 0 | 0/0 | 0/0 | 0.00% | 0.0 | 0.000 |
| early_twap_up | early_twap_open_interest | with_rtds_candles | up | 5 | 0.75 | 0.00 | 0.00 | 0.000 | 0 | 0/0 | 0/0 | 0.00% | 0.0 | 0.000 |
| early_twap_up | early_refprice_single_regime | without_rtds_candles | up | 5 | 0.85 | 0.00 | 0.00 | 0.000 | 0 | 0/0 | 0/0 | 0.00% | 0.0 | 0.000 |
| early_twap_up | early_refprice_single_regime | with_rtds_candles | up | 5 | 0.75 | 0.00 | 0.00 | 0.000 | 0 | 0/0 | 0/0 | 0.00% | 0.0 | 0.000 |
| early_twap_up | early_twap_single_regime | without_rtds_candles | up | 5 | 0.85 | 0.00 | 0.00 | 0.000 | 0 | 0/0 | 0/0 | 0.00% | 0.0 | 0.000 |
| early_twap_up | early_twap_single_regime | with_rtds_candles | up | 5 | 0.75 | 0.00 | 0.00 | 0.000 | 0 | 0/0 | 0/0 | 0.00% | 0.0 | 0.000 |
| bridge_early | bridge_aware_specialist | without_rtds_candles | both | 5 | 0.85 | 12.51 | 7.16 | 1.155 | 107 | 39/68 | 85/22 | 6.45% | 62.2 | 3.345 |
| bridge_early | bridge_aware_specialist | with_rtds_candles | up | 5 | 0.65 | 2.18 | -4.77 | 1.014 | 139 | 139/0 | 89/50 | 8.37% | 62.5 | 1.755 |
| bridge_early | time_specialist_early | without_rtds_candles | both | 5 | 0.55 | 47.35 | 31.55 | 1.136 | 316 | 59/257 | 173/143 | 19.04% | 62.8 | 1.065 |
| bridge_early | time_specialist_early | with_rtds_candles | both | 5 | 0.55 | 69.11 | 54.21 | 1.221 | 298 | 65/233 | 169/129 | 17.95% | 62.9 | 1.073 |
| extended_specialist_down | extended_specialist_official | without_rtds_candles | down | 5 | 0.55 | 86.94 | 77.54 | 1.520 | 188 | 0/188 | 118/70 | 11.33% | 77.2 | 1.109 |
| extended_specialist_down | extended_specialist_official | with_rtds_candles | down | 5 | 0.75 | 67.27 | 60.92 | 1.781 | 127 | 0/127 | 98/29 | 7.65% | 77.4 | 1.897 |
| extended_specialist_down | specialist_dual_head | without_rtds_candles | down | 5 | 0.55 | 86.94 | 77.54 | 1.520 | 188 | 0/188 | 118/70 | 11.33% | 77.2 | 1.109 |
| extended_specialist_down | specialist_dual_head | with_rtds_candles | down | 5 | 0.75 | 67.27 | 60.92 | 1.781 | 127 | 0/127 | 98/29 | 7.65% | 77.4 | 1.897 |
| middle_q5_down | middle_q5_admission | without_rtds_candles | down | 5 | 0.65 | 48.65 | 43.25 | 1.472 | 108 | 0/108 | 70/38 | 6.51% | 110.7 | 1.251 |
| middle_q5_down | middle_q5_admission | with_rtds_candles | down | 5 | 0.55 | 74.21 | 68.16 | 1.678 | 121 | 0/121 | 78/43 | 7.29% | 110.7 | 1.081 |
| middle_q5_down | crossvenue_middle_110 | without_rtds_candles | down | 5 | 0.55 | 78.49 | 69.89 | 1.502 | 172 | 0/172 | 111/61 | 10.36% | 110.9 | 1.212 |
| middle_q5_down | crossvenue_middle_110 | with_rtds_candles | down | 5 | 0.55 | 76.21 | 65.01 | 1.339 | 224 | 0/224 | 140/84 | 13.49% | 110.9 | 1.244 |
| multivenue_middle | multivenue_consensus | without_rtds_candles | both | 5 | 0.55 | 86.19 | 56.04 | 1.142 | 603 | 222/381 | 383/220 | 36.33% | 125.0 | 1.525 |
| multivenue_middle | multivenue_consensus | with_rtds_candles | both | 5 | 0.55 | 33.39 | 8.49 | 1.058 | 498 | 192/306 | 280/218 | 30.00% | 125.6 | 1.214 |
| multivenue_middle | kraken_crossvenue_middle | without_rtds_candles | both | 5 | 0.55 | 91.98 | 69.78 | 1.201 | 444 | 208/236 | 253/191 | 26.75% | 125.5 | 1.103 |
| multivenue_middle | kraken_crossvenue_middle | with_rtds_candles | both | 5 | 0.55 | 109.04 | 81.94 | 1.189 | 542 | 244/298 | 317/225 | 32.65% | 125.4 | 1.185 |
| settlement_middle | crossvenue_settlement_middle | without_rtds_candles | both | 5 | 0.55 | 182.40 | 158.25 | 1.437 | 483 | 171/312 | 333/150 | 29.10% | 153.0 | 1.544 |
| settlement_middle | crossvenue_settlement_middle | with_rtds_candles | both | 5 | 0.65 | 156.99 | 143.59 | 1.718 | 268 | 101/167 | 192/76 | 16.14% | 153.5 | 1.470 |
| settlement_middle | middle_agreement | without_rtds_candles | both | 5 | 0.75 | 112.74 | 90.29 | 1.330 | 449 | 173/276 | 350/99 | 27.05% | 152.7 | 2.658 |
| settlement_middle | middle_agreement | with_rtds_candles | both | 5 | 0.55 | 128.97 | 103.12 | 1.269 | 517 | 213/304 | 348/169 | 31.14% | 153.0 | 1.623 |
| settlement_middle | price_time_calibrated_middle | without_rtds_candles | both | 5 | 0.55 | 169.77 | 144.72 | 1.387 | 501 | 165/336 | 340/161 | 30.18% | 153.2 | 1.523 |
| settlement_middle | price_time_calibrated_middle | with_rtds_candles | both | 5 | 0.55 | 165.50 | 148.15 | 1.538 | 347 | 125/222 | 227/120 | 20.90% | 153.4 | 1.230 |
| late_discovery | multivenue_late | without_rtds_candles | both | 5 | 0.75 | 216.20 | 198.05 | 1.898 | 363 | 99/264 | 285/78 | 21.87% | 189.1 | 1.925 |
| late_discovery | multivenue_late | with_rtds_candles | both | 5 | 0.75 | 214.61 | 201.66 | 2.250 | 259 | 79/180 | 201/58 | 15.60% | 189.2 | 1.540 |
| late_discovery | time_specialist_late | without_rtds_candles | both | 5 | 0.75 | 216.20 | 198.05 | 1.898 | 363 | 99/264 | 285/78 | 21.87% | 189.1 | 1.925 |
| late_discovery | time_specialist_late | with_rtds_candles | both | 5 | 0.75 | 214.61 | 201.66 | 2.250 | 259 | 79/180 | 201/58 | 15.60% | 189.2 | 1.540 |
| terminal_discovery | settlement_aligned_terminal | without_rtds_candles | both | 5 | 0.65 | 282.03 | 255.98 | 1.794 | 521 | 231/290 | 395/126 | 31.39% | 214.2 | 1.747 |
| terminal_discovery | settlement_aligned_terminal | with_rtds_candles | both | 5 | 0.65 | 293.33 | 269.38 | 1.944 | 479 | 208/271 | 372/107 | 28.86% | 214.1 | 1.789 |
| terminal_discovery | kraken_crossvenue_terminal | without_rtds_candles | both | 5 | 0.75 | 254.39 | 234.14 | 1.992 | 405 | 174/231 | 306/99 | 24.40% | 214.0 | 1.551 |
| terminal_discovery | kraken_crossvenue_terminal | with_rtds_candles | both | 5 | 0.55 | 269.57 | 251.02 | 2.006 | 371 | 161/210 | 244/127 | 22.35% | 214.0 | 0.958 |
| terminal_discovery | price_control_terminal | without_rtds_candles | both | 5 | 0.75 | 256.66 | 236.46 | 2.014 | 404 | 171/233 | 306/98 | 24.34% | 214.3 | 1.551 |
| terminal_discovery | price_control_terminal | with_rtds_candles | both | 5 | 0.65 | 284.80 | 267.70 | 2.269 | 342 | 146/196 | 242/100 | 20.60% | 214.8 | 1.067 |

## All candidate variants — untouched confirmation

| Bucket | Candidate | RTDS | Side | VWAP | PnL | Stress | PF | Trades | UP/DOWN | W/L | Coverage | Avg entry | Recovery |
|---|---|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| early_twap_up | early_twap_open_interest | without_rtds_candles | up | 5 | 0.00 | 0.00 | 0.000 | 0 | 0/0 | 0/0 | 0.00% | 0.0 | 0.000 |
| early_twap_up | early_twap_open_interest | with_rtds_candles | up | 5 | 0.00 | 0.00 | 0.000 | 0 | 0/0 | 0/0 | 0.00% | 0.0 | 0.000 |
| early_twap_up | early_refprice_single_regime | without_rtds_candles | up | 5 | 0.00 | 0.00 | 0.000 | 0 | 0/0 | 0/0 | 0.00% | 0.0 | 0.000 |
| early_twap_up | early_refprice_single_regime | with_rtds_candles | up | 5 | 0.00 | 0.00 | 0.000 | 0 | 0/0 | 0/0 | 0.00% | 0.0 | 0.000 |
| early_twap_up | early_twap_single_regime | without_rtds_candles | up | 5 | 0.00 | 0.00 | 0.000 | 0 | 0/0 | 0/0 | 0.00% | 0.0 | 0.000 |
| early_twap_up | early_twap_single_regime | with_rtds_candles | up | 5 | 0.00 | 0.00 | 0.000 | 0 | 0/0 | 0/0 | 0.00% | 0.0 | 0.000 |
| bridge_early | bridge_aware_specialist | without_rtds_candles | both | 5 | 36.47 | 31.02 | 1.630 | 109 | 34/75 | 94/15 | 6.12% | 63.2 | 3.845 |
| bridge_early | bridge_aware_specialist | with_rtds_candles | up | 5 | 7.61 | 0.71 | 1.053 | 138 | 138/0 | 93/45 | 7.75% | 62.6 | 1.963 |
| bridge_early | time_specialist_early | without_rtds_candles | both | 5 | 6.87 | -11.93 | 1.016 | 376 | 30/346 | 197/179 | 21.11% | 62.6 | 1.083 |
| bridge_early | time_specialist_early | with_rtds_candles | both | 5 | -43.41 | -59.66 | 0.892 | 325 | 41/284 | 160/165 | 18.25% | 63.0 | 1.087 |
| extended_specialist_down | extended_specialist_official | without_rtds_candles | down | 5 | 5.64 | -5.41 | 1.022 | 221 | 0/221 | 121/100 | 12.41% | 77.4 | 1.183 |
| extended_specialist_down | extended_specialist_official | with_rtds_candles | down | 5 | -18.44 | -24.99 | 0.870 | 131 | 0/131 | 90/41 | 7.36% | 78.1 | 2.523 |
| extended_specialist_down | specialist_dual_head | without_rtds_candles | down | 5 | 5.64 | -5.41 | 1.022 | 221 | 0/221 | 121/100 | 12.41% | 77.4 | 1.183 |
| extended_specialist_down | specialist_dual_head | with_rtds_candles | down | 5 | -18.44 | -24.99 | 0.870 | 131 | 0/131 | 90/41 | 7.36% | 78.1 | 2.523 |
| middle_q5_down | middle_q5_admission | without_rtds_candles | down | 5 | 32.49 | 26.79 | 1.272 | 114 | 0/114 | 70/44 | 6.40% | 111.4 | 1.251 |
| middle_q5_down | middle_q5_admission | with_rtds_candles | down | 5 | 12.03 | 4.83 | 1.072 | 144 | 0/144 | 77/67 | 8.09% | 111.1 | 1.072 |
| middle_q5_down | crossvenue_middle_110 | without_rtds_candles | down | 5 | 3.46 | -6.04 | 1.016 | 190 | 0/190 | 106/84 | 10.67% | 110.9 | 1.243 |
| middle_q5_down | crossvenue_middle_110 | with_rtds_candles | down | 5 | -29.51 | -45.11 | 0.923 | 312 | 0/312 | 174/138 | 17.52% | 111.1 | 1.366 |
| multivenue_middle | multivenue_consensus | without_rtds_candles | both | 5 | -28.13 | -66.48 | 0.966 | 767 | 271/496 | 467/300 | 43.07% | 125.3 | 1.611 |
| multivenue_middle | multivenue_consensus | with_rtds_candles | both | 5 | -98.13 | -132.48 | 0.886 | 687 | 269/418 | 369/318 | 38.57% | 125.5 | 1.309 |
| multivenue_middle | kraken_crossvenue_middle | without_rtds_candles | both | 5 | -55.41 | -80.51 | 0.907 | 502 | 232/270 | 249/253 | 28.19% | 125.5 | 1.085 |
| multivenue_middle | kraken_crossvenue_middle | with_rtds_candles | both | 5 | -99.80 | -137.20 | 0.892 | 748 | 383/365 | 398/350 | 42.00% | 124.5 | 1.274 |
| settlement_middle | crossvenue_settlement_middle | without_rtds_candles | both | 5 | -101.99 | -133.44 | 0.858 | 629 | 240/389 | 380/249 | 35.32% | 152.9 | 1.779 |
| settlement_middle | crossvenue_settlement_middle | with_rtds_candles | both | 5 | -43.44 | -57.34 | 0.872 | 278 | 108/170 | 159/119 | 15.61% | 153.6 | 1.532 |
| settlement_middle | middle_agreement | without_rtds_candles | both | 5 | -68.65 | -97.35 | 0.871 | 574 | 223/351 | 422/152 | 32.23% | 152.7 | 3.188 |
| settlement_middle | middle_agreement | with_rtds_candles | both | 5 | -32.34 | -64.99 | 0.953 | 653 | 293/360 | 413/240 | 36.66% | 152.9 | 1.805 |
| settlement_middle | price_time_calibrated_middle | without_rtds_candles | both | 5 | -51.24 | -82.14 | 0.924 | 618 | 182/436 | 381/237 | 34.70% | 152.7 | 1.739 |
| settlement_middle | price_time_calibrated_middle | with_rtds_candles | both | 5 | -85.58 | -103.53 | 0.812 | 359 | 131/228 | 184/175 | 20.16% | 153.5 | 1.295 |
| late_discovery | multivenue_late | without_rtds_candles | both | 5 | -42.74 | -62.84 | 0.893 | 402 | 91/311 | 278/124 | 22.57% | 189.5 | 2.512 |
| late_discovery | multivenue_late | with_rtds_candles | both | 5 | 3.34 | -9.01 | 1.013 | 247 | 71/176 | 160/87 | 13.87% | 189.8 | 1.815 |
| late_discovery | time_specialist_late | without_rtds_candles | both | 5 | -42.74 | -62.84 | 0.893 | 402 | 91/311 | 278/124 | 22.57% | 189.5 | 2.512 |
| late_discovery | time_specialist_late | with_rtds_candles | both | 5 | 3.34 | -9.01 | 1.013 | 247 | 71/176 | 160/87 | 13.87% | 189.8 | 1.815 |
| terminal_discovery | settlement_aligned_terminal | without_rtds_candles | both | 5 | -34.01 | -64.91 | 0.942 | 618 | 283/335 | 421/197 | 34.70% | 214.0 | 2.269 |
| terminal_discovery | settlement_aligned_terminal | with_rtds_candles | both | 5 | -40.32 | -69.67 | 0.930 | 587 | 271/316 | 397/190 | 32.96% | 214.4 | 2.247 |
| terminal_discovery | kraken_crossvenue_terminal | without_rtds_candles | both | 5 | -63.39 | -85.29 | 0.859 | 438 | 199/239 | 282/156 | 24.59% | 214.2 | 2.105 |
| terminal_discovery | kraken_crossvenue_terminal | with_rtds_candles | both | 5 | -75.60 | -97.60 | 0.850 | 440 | 205/235 | 218/222 | 24.71% | 214.9 | 1.156 |
| terminal_discovery | price_control_terminal | without_rtds_candles | both | 5 | -67.44 | -89.29 | 0.851 | 437 | 192/245 | 284/153 | 24.54% | 214.1 | 2.182 |
| terminal_discovery | price_control_terminal | with_rtds_candles | both | 5 | -46.18 | -64.33 | 0.887 | 363 | 178/185 | 199/164 | 20.38% | 214.6 | 1.368 |

## Per-challenger period and bucket PnL

### early_twap_open_interest__without_rtds_candles

Bucket: `early_twap_up`; side: `up`; executable VWAP: `5` shares.

| Period | PnL | Stress | Gross profit | Gross loss | PF | Trades | UP/DOWN | W/L | Win rate | Coverage | Avg entry | Avg cost | Avg confidence | Recovery | Max DD |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| development | 64.51 | 43.36 | 453.99 | 389.48 | 1.166 | 423 | 423/0 | 305/118 | 72.10% | 24.65% | 40.8 | 0.6707 | 0.7753 | 2.217 | 23.51 |
| sealed | 0.00 | 0.00 | 0.00 | 0.00 | 0.000 | 0 | 0/0 | 0/0 | 0.00% | 0.00% | 0.0 | 0.0000 | 0.0000 | 0.000 | 0.00 |
| confirmation | 0.00 | 0.00 | 0.00 | 0.00 | 0.000 | 0 | 0/0 | 0/0 | 0.00% | 0.00% | 0.0 | 0.0000 | 0.0000 | 0.000 | 0.00 |

### early_twap_open_interest__with_rtds_candles

Bucket: `early_twap_up`; side: `up`; executable VWAP: `5` shares.

| Period | PnL | Stress | Gross profit | Gross loss | PF | Trades | UP/DOWN | W/L | Win rate | Coverage | Avg entry | Avg cost | Avg confidence | Recovery | Max DD |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| development | 6.55 | 4.90 | 38.41 | 31.86 | 1.206 | 33 | 33/0 | 23/10 | 69.70% | 1.92% | 41.7 | 0.6364 | 0.7991 | 1.908 | 14.69 |
| sealed | 0.00 | 0.00 | 0.00 | 0.00 | 0.000 | 0 | 0/0 | 0/0 | 0.00% | 0.00% | 0.0 | 0.0000 | 0.0000 | 0.000 | 0.00 |
| confirmation | 0.00 | 0.00 | 0.00 | 0.00 | 0.000 | 0 | 0/0 | 0/0 | 0.00% | 0.00% | 0.0 | 0.0000 | 0.0000 | 0.000 | 0.00 |

### early_refprice_single_regime__without_rtds_candles

Bucket: `early_twap_up`; side: `up`; executable VWAP: `5` shares.

| Period | PnL | Stress | Gross profit | Gross loss | PF | Trades | UP/DOWN | W/L | Win rate | Coverage | Avg entry | Avg cost | Avg confidence | Recovery | Max DD |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| development | 29.36 | 24.21 | 95.18 | 65.82 | 1.446 | 103 | 103/0 | 86/17 | 83.50% | 6.00% | 41.4 | 0.7605 | 0.8769 | 3.498 | 14.61 |
| sealed | 0.00 | 0.00 | 0.00 | 0.00 | 0.000 | 0 | 0/0 | 0/0 | 0.00% | 0.00% | 0.0 | 0.0000 | 0.0000 | 0.000 | 0.00 |
| confirmation | 0.00 | 0.00 | 0.00 | 0.00 | 0.000 | 0 | 0/0 | 0/0 | 0.00% | 0.00% | 0.0 | 0.0000 | 0.0000 | 0.000 | 0.00 |

### early_refprice_single_regime__with_rtds_candles

Bucket: `early_twap_up`; side: `up`; executable VWAP: `5` shares.

| Period | PnL | Stress | Gross profit | Gross loss | PF | Trades | UP/DOWN | W/L | Win rate | Coverage | Avg entry | Avg cost | Avg confidence | Recovery | Max DD |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| development | 23.34 | 18.74 | 92.17 | 68.83 | 1.339 | 92 | 92/0 | 73/19 | 79.35% | 5.36% | 41.3 | 0.7241 | 0.8192 | 2.869 | 15.33 |
| sealed | 0.00 | 0.00 | 0.00 | 0.00 | 0.000 | 0 | 0/0 | 0/0 | 0.00% | 0.00% | 0.0 | 0.0000 | 0.0000 | 0.000 | 0.00 |
| confirmation | 0.00 | 0.00 | 0.00 | 0.00 | 0.000 | 0 | 0/0 | 0/0 | 0.00% | 0.00% | 0.0 | 0.0000 | 0.0000 | 0.000 | 0.00 |

### early_twap_single_regime__without_rtds_candles

Bucket: `early_twap_up`; side: `up`; executable VWAP: `5` shares.

| Period | PnL | Stress | Gross profit | Gross loss | PF | Trades | UP/DOWN | W/L | Win rate | Coverage | Avg entry | Avg cost | Avg confidence | Recovery | Max DD |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| development | 48.80 | 42.85 | 106.39 | 57.59 | 1.847 | 119 | 119/0 | 104/15 | 87.39% | 6.93% | 42.0 | 0.7750 | 0.8777 | 3.753 | 7.68 |
| sealed | 0.00 | 0.00 | 0.00 | 0.00 | 0.000 | 0 | 0/0 | 0/0 | 0.00% | 0.00% | 0.0 | 0.0000 | 0.0000 | 0.000 | 0.00 |
| confirmation | 0.00 | 0.00 | 0.00 | 0.00 | 0.000 | 0 | 0/0 | 0/0 | 0.00% | 0.00% | 0.0 | 0.0000 | 0.0000 | 0.000 | 0.00 |

### early_twap_single_regime__with_rtds_candles

Bucket: `early_twap_up`; side: `up`; executable VWAP: `5` shares.

| Period | PnL | Stress | Gross profit | Gross loss | PF | Trades | UP/DOWN | W/L | Win rate | Coverage | Avg entry | Avg cost | Avg confidence | Recovery | Max DD |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| development | 19.63 | 15.18 | 88.90 | 69.27 | 1.283 | 89 | 89/0 | 70/19 | 78.65% | 5.19% | 41.1 | 0.7238 | 0.8233 | 2.871 | 13.66 |
| sealed | 0.00 | 0.00 | 0.00 | 0.00 | 0.000 | 0 | 0/0 | 0/0 | 0.00% | 0.00% | 0.0 | 0.0000 | 0.0000 | 0.000 | 0.00 |
| confirmation | 0.00 | 0.00 | 0.00 | 0.00 | 0.000 | 0 | 0/0 | 0/0 | 0.00% | 0.00% | 0.0 | 0.0000 | 0.0000 | 0.000 | 0.00 |

### bridge_aware_specialist__without_rtds_candles

Bucket: `bridge_early`; side: `both`; executable VWAP: `5` shares.

| Period | PnL | Stress | Gross profit | Gross loss | PF | Trades | UP/DOWN | W/L | Win rate | Coverage | Avg entry | Avg cost | Avg confidence | Recovery | Max DD |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| development | 33.42 | 28.17 | 93.56 | 60.14 | 1.556 | 105 | 37/68 | 89/16 | 84.76% | 6.32% | 62.8 | 0.7672 | 0.8879 | 3.576 | 19.03 |
| sealed | 12.51 | 7.16 | 93.21 | 80.70 | 1.155 | 107 | 39/68 | 85/22 | 79.44% | 6.45% | 62.2 | 0.7538 | 0.8865 | 3.345 | 19.54 |
| confirmation | 36.47 | 31.02 | 94.37 | 57.89 | 1.630 | 109 | 34/75 | 94/15 | 86.24% | 6.12% | 63.2 | 0.7789 | 0.8819 | 3.845 | 13.81 |

### bridge_aware_specialist__with_rtds_candles

Bucket: `bridge_early`; side: `up`; executable VWAP: `5` shares.

| Period | PnL | Stress | Gross profit | Gross loss | PF | Trades | UP/DOWN | W/L | Win rate | Coverage | Avg entry | Avg cost | Avg confidence | Recovery | Max DD |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| development | 22.33 | 17.13 | 126.05 | 103.71 | 1.215 | 104 | 104/0 | 71/33 | 68.27% | 6.26% | 62.2 | 0.6191 | 0.7697 | 1.770 | 26.41 |
| sealed | 2.18 | -4.77 | 158.45 | 156.26 | 1.014 | 139 | 139/0 | 89/50 | 64.03% | 8.37% | 62.5 | 0.6163 | 0.7578 | 1.755 | 41.83 |
| confirmation | 7.61 | 0.71 | 151.77 | 144.16 | 1.053 | 138 | 138/0 | 93/45 | 67.39% | 7.75% | 62.6 | 0.6425 | 0.7605 | 1.963 | 28.69 |

### time_specialist_early__without_rtds_candles

Bucket: `bridge_early`; side: `both`; executable VWAP: `5` shares.

| Period | PnL | Stress | Gross profit | Gross loss | PF | Trades | UP/DOWN | W/L | Win rate | Coverage | Avg entry | Avg cost | Avg confidence | Recovery | Max DD |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| development | 113.93 | 96.23 | 472.83 | 358.90 | 1.317 | 354 | 42/312 | 210/144 | 59.32% | 21.30% | 62.8 | 0.5074 | 0.7053 | 1.107 | 36.22 |
| sealed | 47.35 | 31.55 | 396.30 | 348.95 | 1.136 | 316 | 59/257 | 173/143 | 54.75% | 19.04% | 62.8 | 0.4960 | 0.6971 | 1.065 | 28.70 |
| confirmation | 6.87 | -11.93 | 439.36 | 432.49 | 1.016 | 376 | 30/346 | 197/179 | 52.39% | 21.11% | 62.6 | 0.4987 | 0.6887 | 1.083 | 61.87 |

### time_specialist_early__with_rtds_candles

Bucket: `bridge_early`; side: `both`; executable VWAP: `5` shares.

| Period | PnL | Stress | Gross profit | Gross loss | PF | Trades | UP/DOWN | W/L | Win rate | Coverage | Avg entry | Avg cost | Avg confidence | Recovery | Max DD |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| development | 68.09 | 52.39 | 407.23 | 339.15 | 1.201 | 314 | 50/264 | 179/135 | 57.01% | 18.89% | 62.9 | 0.5052 | 0.7028 | 1.104 | 40.73 |
| sealed | 69.11 | 54.21 | 382.05 | 312.94 | 1.221 | 298 | 65/233 | 169/129 | 56.71% | 17.95% | 62.9 | 0.4992 | 0.6974 | 1.073 | 25.18 |
| confirmation | -43.41 | -59.66 | 359.36 | 402.77 | 0.892 | 325 | 41/284 | 160/165 | 49.23% | 18.25% | 63.0 | 0.4973 | 0.6823 | 1.087 | 86.20 |

### extended_specialist_official__without_rtds_candles

Bucket: `extended_specialist_down`; side: `down`; executable VWAP: `5` shares.

| Period | PnL | Stress | Gross profit | Gross loss | PF | Trades | UP/DOWN | W/L | Win rate | Coverage | Avg entry | Avg cost | Avg confidence | Recovery | Max DD |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| development | 54.78 | 44.43 | 265.54 | 210.76 | 1.260 | 207 | 0/207 | 125/82 | 60.39% | 12.45% | 76.8 | 0.5296 | 0.7374 | 1.210 | 30.20 |
| sealed | 86.94 | 77.54 | 254.13 | 167.19 | 1.520 | 188 | 0/188 | 118/70 | 62.77% | 11.33% | 77.2 | 0.5140 | 0.7314 | 1.109 | 27.95 |
| confirmation | 5.64 | -5.41 | 257.53 | 251.89 | 1.022 | 221 | 0/221 | 121/100 | 54.75% | 12.41% | 77.4 | 0.5210 | 0.7097 | 1.183 | 46.40 |

### extended_specialist_official__with_rtds_candles

Bucket: `extended_specialist_down`; side: `down`; executable VWAP: `5` shares.

| Period | PnL | Stress | Gross profit | Gross loss | PF | Trades | UP/DOWN | W/L | Win rate | Coverage | Avg entry | Avg cost | Avg confidence | Recovery | Max DD |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| development | 27.84 | 21.19 | 148.19 | 120.34 | 1.231 | 133 | 0/133 | 94/39 | 70.68% | 8.00% | 77.8 | 0.6450 | 0.8244 | 1.957 | 16.25 |
| sealed | 67.27 | 60.92 | 153.39 | 86.13 | 1.781 | 127 | 0/127 | 98/29 | 77.17% | 7.65% | 77.4 | 0.6460 | 0.8276 | 1.897 | 15.16 |
| confirmation | -18.44 | -24.99 | 123.64 | 142.09 | 0.870 | 131 | 0/131 | 90/41 | 68.70% | 7.36% | 78.1 | 0.6961 | 0.8335 | 2.523 | 54.14 |

### specialist_dual_head__without_rtds_candles

Bucket: `extended_specialist_down`; side: `down`; executable VWAP: `5` shares.

| Period | PnL | Stress | Gross profit | Gross loss | PF | Trades | UP/DOWN | W/L | Win rate | Coverage | Avg entry | Avg cost | Avg confidence | Recovery | Max DD |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| development | 54.78 | 44.43 | 265.54 | 210.76 | 1.260 | 207 | 0/207 | 125/82 | 60.39% | 12.45% | 76.8 | 0.5296 | 0.7374 | 1.210 | 30.20 |
| sealed | 86.94 | 77.54 | 254.13 | 167.19 | 1.520 | 188 | 0/188 | 118/70 | 62.77% | 11.33% | 77.2 | 0.5140 | 0.7314 | 1.109 | 27.95 |
| confirmation | 5.64 | -5.41 | 257.53 | 251.89 | 1.022 | 221 | 0/221 | 121/100 | 54.75% | 12.41% | 77.4 | 0.5210 | 0.7097 | 1.183 | 46.40 |

### specialist_dual_head__with_rtds_candles

Bucket: `extended_specialist_down`; side: `down`; executable VWAP: `5` shares.

| Period | PnL | Stress | Gross profit | Gross loss | PF | Trades | UP/DOWN | W/L | Win rate | Coverage | Avg entry | Avg cost | Avg confidence | Recovery | Max DD |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| development | 27.84 | 21.19 | 148.19 | 120.34 | 1.231 | 133 | 0/133 | 94/39 | 70.68% | 8.00% | 77.8 | 0.6450 | 0.8244 | 1.957 | 16.25 |
| sealed | 67.27 | 60.92 | 153.39 | 86.13 | 1.781 | 127 | 0/127 | 98/29 | 77.17% | 7.65% | 77.4 | 0.6460 | 0.8276 | 1.897 | 15.16 |
| confirmation | -18.44 | -24.99 | 123.64 | 142.09 | 0.870 | 131 | 0/131 | 90/41 | 68.70% | 7.36% | 78.1 | 0.6961 | 0.8335 | 2.523 | 54.14 |

### middle_q5_admission__without_rtds_candles

Bucket: `middle_q5_down`; side: `down`; executable VWAP: `5` shares.

| Period | PnL | Stress | Gross profit | Gross loss | PF | Trades | UP/DOWN | W/L | Win rate | Coverage | Avg entry | Avg cost | Avg confidence | Recovery | Max DD |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| development | 45.53 | 39.73 | 157.05 | 111.52 | 1.408 | 116 | 0/116 | 75/41 | 64.66% | 6.98% | 111.2 | 0.5465 | 0.7873 | 1.299 | 12.69 |
| sealed | 48.65 | 43.25 | 151.66 | 103.01 | 1.472 | 108 | 0/108 | 70/38 | 64.81% | 6.51% | 110.7 | 0.5365 | 0.7840 | 1.251 | 18.80 |
| confirmation | 32.49 | 26.79 | 152.00 | 119.51 | 1.272 | 114 | 0/114 | 70/44 | 61.40% | 6.40% | 111.4 | 0.5353 | 0.7550 | 1.251 | 16.45 |

### middle_q5_admission__with_rtds_candles

Bucket: `middle_q5_down`; side: `down`; executable VWAP: `5` shares.

| Period | PnL | Stress | Gross profit | Gross loss | PF | Trades | UP/DOWN | W/L | Win rate | Coverage | Avg entry | Avg cost | Avg confidence | Recovery | Max DD |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| development | 44.75 | 37.85 | 182.89 | 138.15 | 1.324 | 138 | 0/138 | 83/55 | 60.14% | 8.30% | 111.3 | 0.5150 | 0.7412 | 1.140 | 15.05 |
| sealed | 74.21 | 68.16 | 183.59 | 109.38 | 1.678 | 121 | 0/121 | 78/43 | 64.46% | 7.29% | 110.7 | 0.5005 | 0.7381 | 1.081 | 18.11 |
| confirmation | 12.03 | 4.83 | 178.00 | 165.97 | 1.072 | 144 | 0/144 | 77/67 | 53.47% | 8.09% | 111.1 | 0.4964 | 0.7011 | 1.072 | 18.21 |

### crossvenue_middle_110__without_rtds_candles

Bucket: `middle_q5_down`; side: `down`; executable VWAP: `5` shares.

| Period | PnL | Stress | Gross profit | Gross loss | PF | Trades | UP/DOWN | W/L | Win rate | Coverage | Avg entry | Avg cost | Avg confidence | Recovery | Max DD |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| development | 63.49 | 54.44 | 240.19 | 176.70 | 1.359 | 181 | 0/181 | 115/66 | 63.54% | 10.89% | 111.3 | 0.5440 | 0.7644 | 1.282 | 17.04 |
| sealed | 78.49 | 69.89 | 235.01 | 156.51 | 1.502 | 172 | 0/172 | 111/61 | 64.53% | 10.36% | 110.9 | 0.5330 | 0.7643 | 1.212 | 27.27 |
| confirmation | 3.46 | -6.04 | 226.14 | 222.69 | 1.016 | 190 | 0/190 | 106/84 | 55.79% | 10.67% | 110.9 | 0.5330 | 0.7328 | 1.243 | 31.40 |

### crossvenue_middle_110__with_rtds_candles

Bucket: `middle_q5_down`; side: `down`; executable VWAP: `5` shares.

| Period | PnL | Stress | Gross profit | Gross loss | PF | Trades | UP/DOWN | W/L | Win rate | Coverage | Avg entry | Avg cost | Avg confidence | Recovery | Max DD |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| development | 51.93 | 38.83 | 332.02 | 280.09 | 1.185 | 262 | 0/262 | 162/100 | 61.83% | 15.76% | 111.3 | 0.5572 | 0.7130 | 1.367 | 19.00 |
| sealed | 76.21 | 65.01 | 300.73 | 224.52 | 1.339 | 224 | 0/224 | 140/84 | 62.50% | 13.49% | 110.9 | 0.5355 | 0.7001 | 1.244 | 33.22 |
| confirmation | -29.51 | -45.11 | 353.35 | 382.86 | 0.923 | 312 | 0/312 | 174/138 | 55.77% | 17.52% | 111.1 | 0.5551 | 0.6859 | 1.366 | 53.73 |

### multivenue_consensus__without_rtds_candles

Bucket: `multivenue_middle`; side: `both`; executable VWAP: `5` shares.

| Period | PnL | Stress | Gross profit | Gross loss | PF | Trades | UP/DOWN | W/L | Win rate | Coverage | Avg entry | Avg cost | Avg confidence | Recovery | Max DD |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| development | 203.44 | 170.29 | 795.87 | 592.43 | 1.343 | 663 | 233/430 | 450/213 | 67.87% | 39.89% | 125.3 | 0.5974 | 0.7698 | 1.573 | 27.95 |
| sealed | 86.19 | 56.04 | 694.34 | 608.15 | 1.142 | 603 | 222/381 | 383/220 | 63.52% | 36.33% | 125.0 | 0.5865 | 0.7588 | 1.525 | 66.57 |
| confirmation | -28.13 | -66.48 | 805.46 | 833.58 | 0.966 | 767 | 271/496 | 467/300 | 60.89% | 43.07% | 125.3 | 0.5961 | 0.7412 | 1.611 | 102.84 |

### multivenue_consensus__with_rtds_candles

Bucket: `multivenue_middle`; side: `both`; executable VWAP: `5` shares.

| Period | PnL | Stress | Gross profit | Gross loss | PF | Trades | UP/DOWN | W/L | Win rate | Coverage | Avg entry | Avg cost | Avg confidence | Recovery | Max DD |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| development | 155.87 | 128.07 | 723.10 | 567.23 | 1.275 | 556 | 216/340 | 346/210 | 62.23% | 33.45% | 125.9 | 0.5449 | 0.7133 | 1.292 | 36.18 |
| sealed | 33.39 | 8.49 | 608.92 | 575.53 | 1.058 | 498 | 192/306 | 280/218 | 56.22% | 30.00% | 125.6 | 0.5274 | 0.7048 | 1.214 | 78.66 |
| confirmation | -98.13 | -132.48 | 764.37 | 862.50 | 0.886 | 687 | 269/418 | 369/318 | 53.71% | 38.57% | 125.5 | 0.5442 | 0.6865 | 1.309 | 129.60 |

### kraken_crossvenue_middle__without_rtds_candles

Bucket: `multivenue_middle`; side: `both`; executable VWAP: `5` shares.

| Period | PnL | Stress | Gross profit | Gross loss | PF | Trades | UP/DOWN | W/L | Win rate | Coverage | Avg entry | Avg cost | Avg confidence | Recovery | Max DD |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| development | 213.44 | 189.24 | 646.78 | 433.34 | 1.493 | 484 | 223/261 | 308/176 | 63.64% | 29.12% | 125.2 | 0.5273 | 0.7551 | 1.172 | 39.65 |
| sealed | 91.98 | 69.78 | 548.86 | 456.88 | 1.201 | 444 | 208/236 | 253/191 | 56.98% | 26.75% | 125.5 | 0.5075 | 0.7475 | 1.103 | 44.92 |
| confirmation | -55.41 | -80.51 | 542.19 | 597.60 | 0.907 | 502 | 232/270 | 249/253 | 49.60% | 28.19% | 125.5 | 0.4971 | 0.7152 | 1.085 | 90.08 |

### kraken_crossvenue_middle__with_rtds_candles

Bucket: `multivenue_middle`; side: `both`; executable VWAP: `5` shares.

| Period | PnL | Stress | Gross profit | Gross loss | PF | Trades | UP/DOWN | W/L | Win rate | Coverage | Avg entry | Avg cost | Avg confidence | Recovery | Max DD |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| development | 188.19 | 158.14 | 794.12 | 605.93 | 1.311 | 601 | 298/303 | 373/228 | 62.06% | 36.16% | 124.8 | 0.5367 | 0.7092 | 1.248 | 40.60 |
| sealed | 109.04 | 81.94 | 684.94 | 575.90 | 1.189 | 542 | 244/298 | 317/225 | 58.49% | 32.65% | 125.4 | 0.5233 | 0.7066 | 1.185 | 36.88 |
| confirmation | -99.80 | -137.20 | 828.57 | 928.37 | 0.892 | 748 | 383/365 | 398/350 | 53.21% | 42.00% | 124.5 | 0.5374 | 0.6857 | 1.274 | 135.19 |

### crossvenue_settlement_middle__without_rtds_candles

Bucket: `settlement_middle`; side: `both`; executable VWAP: `5` shares.

| Period | PnL | Stress | Gross profit | Gross loss | PF | Trades | UP/DOWN | W/L | Win rate | Coverage | Avg entry | Avg cost | Avg confidence | Recovery | Max DD |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| development | 178.57 | 152.22 | 628.91 | 450.34 | 1.397 | 527 | 186/341 | 367/160 | 69.64% | 31.71% | 153.0 | 0.6089 | 0.7868 | 1.642 | 36.03 |
| sealed | 182.40 | 158.25 | 599.40 | 417.00 | 1.437 | 483 | 171/312 | 333/150 | 68.94% | 29.10% | 153.0 | 0.5941 | 0.7926 | 1.544 | 27.02 |
| confirmation | -101.99 | -133.44 | 614.89 | 716.88 | 0.858 | 629 | 240/389 | 380/249 | 60.41% | 35.32% | 152.9 | 0.6170 | 0.7704 | 1.779 | 134.82 |

### crossvenue_settlement_middle__with_rtds_candles

Bucket: `settlement_middle`; side: `both`; executable VWAP: `5` shares.

| Period | PnL | Stress | Gross profit | Gross loss | PF | Trades | UP/DOWN | W/L | Win rate | Coverage | Avg entry | Avg cost | Avg confidence | Recovery | Max DD |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| development | 167.80 | 154.10 | 376.51 | 208.71 | 1.804 | 274 | 104/170 | 202/72 | 73.72% | 16.49% | 153.5 | 0.5942 | 0.8232 | 1.555 | 14.18 |
| sealed | 156.99 | 143.59 | 375.57 | 218.58 | 1.718 | 268 | 101/167 | 192/76 | 71.64% | 16.14% | 153.5 | 0.5787 | 0.8318 | 1.470 | 19.63 |
| confirmation | -43.44 | -57.34 | 296.01 | 339.45 | 0.872 | 278 | 108/170 | 159/119 | 57.19% | 15.61% | 153.6 | 0.5826 | 0.7926 | 1.532 | 79.08 |

### middle_agreement__without_rtds_candles

Bucket: `settlement_middle`; side: `both`; executable VWAP: `5` shares.

| Period | PnL | Stress | Gross profit | Gross loss | PF | Trades | UP/DOWN | W/L | Win rate | Coverage | Avg entry | Avg cost | Avg confidence | Recovery | Max DD |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| development | 158.72 | 134.42 | 485.39 | 326.68 | 1.486 | 486 | 192/294 | 391/95 | 80.45% | 29.24% | 152.9 | 0.7216 | 0.8689 | 2.770 | 21.47 |
| sealed | 112.74 | 90.29 | 454.37 | 341.63 | 1.330 | 449 | 173/276 | 350/99 | 77.95% | 27.05% | 152.7 | 0.7118 | 0.8765 | 2.658 | 30.83 |
| confirmation | -68.65 | -97.35 | 462.87 | 531.52 | 0.871 | 574 | 223/351 | 422/152 | 73.52% | 32.23% | 152.7 | 0.7420 | 0.8589 | 3.188 | 106.63 |

### middle_agreement__with_rtds_candles

Bucket: `settlement_middle`; side: `both`; executable VWAP: `5` shares.

| Period | PnL | Stress | Gross profit | Gross loss | PF | Trades | UP/DOWN | W/L | Win rate | Coverage | Avg entry | Avg cost | Avg confidence | Recovery | Max DD |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| development | 184.18 | 158.93 | 607.07 | 422.89 | 1.436 | 505 | 199/306 | 355/150 | 70.30% | 30.39% | 153.1 | 0.6103 | 0.7869 | 1.649 | 28.97 |
| sealed | 128.97 | 103.12 | 608.82 | 479.84 | 1.269 | 517 | 213/304 | 348/169 | 67.31% | 31.14% | 153.0 | 0.6036 | 0.7895 | 1.623 | 36.37 |
| confirmation | -32.34 | -64.99 | 657.70 | 690.04 | 0.953 | 653 | 293/360 | 413/240 | 63.25% | 36.66% | 152.9 | 0.6228 | 0.7713 | 1.805 | 102.34 |

### price_time_calibrated_middle__without_rtds_candles

Bucket: `settlement_middle`; side: `both`; executable VWAP: `5` shares.

| Period | PnL | Stress | Gross profit | Gross loss | PF | Trades | UP/DOWN | W/L | Win rate | Coverage | Avg entry | Avg cost | Avg confidence | Recovery | Max DD |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| development | 185.07 | 158.92 | 629.99 | 444.92 | 1.416 | 523 | 159/364 | 364/159 | 69.60% | 31.47% | 153.0 | 0.6056 | 0.7874 | 1.617 | 30.87 |
| sealed | 169.77 | 144.72 | 608.54 | 438.76 | 1.387 | 501 | 165/336 | 340/161 | 67.86% | 30.18% | 153.2 | 0.5912 | 0.7887 | 1.523 | 18.65 |
| confirmation | -51.24 | -82.14 | 626.19 | 677.42 | 0.924 | 618 | 182/436 | 381/237 | 61.65% | 34.70% | 152.7 | 0.6135 | 0.7703 | 1.739 | 97.92 |

### price_time_calibrated_middle__with_rtds_candles

Bucket: `settlement_middle`; side: `both`; executable VWAP: `5` shares.

| Period | PnL | Stress | Gross profit | Gross loss | PF | Trades | UP/DOWN | W/L | Win rate | Coverage | Avg entry | Avg cost | Avg confidence | Recovery | Max DD |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| development | 130.14 | 112.89 | 449.08 | 318.93 | 1.408 | 345 | 125/220 | 223/122 | 64.64% | 20.76% | 153.8 | 0.5503 | 0.7767 | 1.298 | 32.66 |
| sealed | 165.50 | 148.15 | 473.08 | 307.58 | 1.538 | 347 | 125/222 | 227/120 | 65.42% | 20.90% | 153.4 | 0.5382 | 0.7819 | 1.230 | 24.05 |
| confirmation | -85.58 | -103.53 | 368.71 | 454.29 | 0.812 | 359 | 131/228 | 184/175 | 51.25% | 20.16% | 153.5 | 0.5395 | 0.7469 | 1.295 | 110.65 |

### multivenue_late__without_rtds_candles

Bucket: `late_discovery`; side: `both`; executable VWAP: `5` shares.

| Period | PnL | Stress | Gross profit | Gross loss | PF | Trades | UP/DOWN | W/L | Win rate | Coverage | Avg entry | Avg cost | Avg confidence | Recovery | Max DD |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| development | 213.38 | 194.53 | 444.94 | 231.56 | 1.922 | 377 | 110/267 | 300/77 | 79.58% | 22.68% | 189.2 | 0.6640 | 0.8771 | 2.028 | 22.70 |
| sealed | 216.20 | 198.05 | 457.04 | 240.84 | 1.898 | 363 | 99/264 | 285/78 | 78.51% | 21.87% | 189.1 | 0.6472 | 0.8778 | 1.925 | 13.94 |
| confirmation | -42.74 | -62.84 | 355.43 | 398.17 | 0.893 | 402 | 91/311 | 278/124 | 69.15% | 22.57% | 189.5 | 0.6946 | 0.8613 | 2.512 | 71.74 |

### multivenue_late__with_rtds_candles

Bucket: `late_discovery`; side: `both`; executable VWAP: `5` shares.

| Period | PnL | Stress | Gross profit | Gross loss | PF | Trades | UP/DOWN | W/L | Win rate | Coverage | Avg entry | Avg cost | Avg confidence | Recovery | Max DD |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| development | 207.95 | 195.70 | 352.52 | 144.57 | 2.438 | 245 | 80/165 | 193/52 | 78.78% | 14.74% | 189.7 | 0.5978 | 0.8725 | 1.522 | 12.18 |
| sealed | 214.61 | 201.66 | 386.35 | 171.73 | 2.250 | 259 | 79/180 | 201/58 | 77.61% | 15.60% | 189.2 | 0.5901 | 0.8698 | 1.540 | 20.57 |
| confirmation | 3.34 | -9.01 | 260.16 | 256.82 | 1.013 | 247 | 71/176 | 160/87 | 64.78% | 13.87% | 189.8 | 0.6251 | 0.8463 | 1.815 | 39.67 |

### time_specialist_late__without_rtds_candles

Bucket: `late_discovery`; side: `both`; executable VWAP: `5` shares.

| Period | PnL | Stress | Gross profit | Gross loss | PF | Trades | UP/DOWN | W/L | Win rate | Coverage | Avg entry | Avg cost | Avg confidence | Recovery | Max DD |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| development | 213.38 | 194.53 | 444.94 | 231.56 | 1.922 | 377 | 110/267 | 300/77 | 79.58% | 22.68% | 189.2 | 0.6640 | 0.8771 | 2.028 | 22.70 |
| sealed | 216.20 | 198.05 | 457.04 | 240.84 | 1.898 | 363 | 99/264 | 285/78 | 78.51% | 21.87% | 189.1 | 0.6472 | 0.8778 | 1.925 | 13.94 |
| confirmation | -42.74 | -62.84 | 355.43 | 398.17 | 0.893 | 402 | 91/311 | 278/124 | 69.15% | 22.57% | 189.5 | 0.6946 | 0.8613 | 2.512 | 71.74 |

### time_specialist_late__with_rtds_candles

Bucket: `late_discovery`; side: `both`; executable VWAP: `5` shares.

| Period | PnL | Stress | Gross profit | Gross loss | PF | Trades | UP/DOWN | W/L | Win rate | Coverage | Avg entry | Avg cost | Avg confidence | Recovery | Max DD |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| development | 207.95 | 195.70 | 352.52 | 144.57 | 2.438 | 245 | 80/165 | 193/52 | 78.78% | 14.74% | 189.7 | 0.5978 | 0.8725 | 1.522 | 12.18 |
| sealed | 214.61 | 201.66 | 386.35 | 171.73 | 2.250 | 259 | 79/180 | 201/58 | 77.61% | 15.60% | 189.2 | 0.5901 | 0.8698 | 1.540 | 20.57 |
| confirmation | 3.34 | -9.01 | 260.16 | 256.82 | 1.013 | 247 | 71/176 | 160/87 | 64.78% | 13.87% | 189.8 | 0.6251 | 0.8463 | 1.815 | 39.67 |

### settlement_aligned_terminal__without_rtds_candles

Bucket: `terminal_discovery`; side: `both`; executable VWAP: `5` shares.

| Period | PnL | Stress | Gross profit | Gross loss | PF | Trades | UP/DOWN | W/L | Win rate | Coverage | Avg entry | Avg cost | Avg confidence | Recovery | Max DD |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| development | 252.74 | 226.39 | 598.13 | 345.39 | 1.732 | 527 | 241/286 | 408/119 | 77.42% | 31.71% | 214.6 | 0.6601 | 0.8546 | 1.980 | 31.26 |
| sealed | 282.03 | 255.98 | 637.02 | 354.98 | 1.794 | 521 | 231/290 | 395/126 | 75.82% | 31.39% | 214.2 | 0.6316 | 0.8540 | 1.747 | 21.52 |
| confirmation | -34.01 | -64.91 | 550.30 | 584.31 | 0.942 | 618 | 283/335 | 421/197 | 68.12% | 34.70% | 214.0 | 0.6743 | 0.8275 | 2.269 | 90.00 |

### settlement_aligned_terminal__with_rtds_candles

Bucket: `terminal_discovery`; side: `both`; executable VWAP: `5` shares.

| Period | PnL | Stress | Gross profit | Gross loss | PF | Trades | UP/DOWN | W/L | Win rate | Coverage | Avg entry | Avg cost | Avg confidence | Recovery | Max DD |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| development | 263.32 | 238.27 | 589.62 | 326.29 | 1.807 | 501 | 230/271 | 393/108 | 78.44% | 30.14% | 215.1 | 0.6609 | 0.8550 | 2.014 | 20.50 |
| sealed | 293.33 | 269.38 | 604.11 | 310.78 | 1.944 | 479 | 208/271 | 372/107 | 77.66% | 28.86% | 214.1 | 0.6358 | 0.8615 | 1.789 | 18.48 |
| confirmation | -40.32 | -69.67 | 533.94 | 574.25 | 0.930 | 587 | 271/316 | 397/190 | 67.63% | 32.96% | 214.4 | 0.6718 | 0.8304 | 2.247 | 95.45 |

### kraken_crossvenue_terminal__without_rtds_candles

Bucket: `terminal_discovery`; side: `both`; executable VWAP: `5` shares.

| Period | PnL | Stress | Gross profit | Gross loss | PF | Trades | UP/DOWN | W/L | Win rate | Coverage | Avg entry | Avg cost | Avg confidence | Recovery | Max DD |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| development | 257.70 | 237.65 | 494.22 | 236.52 | 2.090 | 401 | 188/213 | 315/86 | 78.55% | 24.13% | 215.1 | 0.6387 | 0.8832 | 1.753 | 25.45 |
| sealed | 254.39 | 234.14 | 510.74 | 256.35 | 1.992 | 405 | 174/231 | 306/99 | 75.56% | 24.40% | 214.0 | 0.6117 | 0.8868 | 1.551 | 18.36 |
| confirmation | -63.39 | -85.29 | 385.77 | 449.16 | 0.859 | 438 | 199/239 | 282/156 | 64.38% | 24.59% | 214.2 | 0.6547 | 0.8624 | 2.105 | 91.32 |

### kraken_crossvenue_terminal__with_rtds_candles

Bucket: `terminal_discovery`; side: `both`; executable VWAP: `5` shares.

| Period | PnL | Stress | Gross profit | Gross loss | PF | Trades | UP/DOWN | W/L | Win rate | Coverage | Avg entry | Avg cost | Avg confidence | Recovery | Max DD |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| development | 240.33 | 221.73 | 517.88 | 277.55 | 1.866 | 372 | 178/194 | 252/120 | 67.74% | 22.38% | 214.9 | 0.5285 | 0.8173 | 1.125 | 23.26 |
| sealed | 269.57 | 251.02 | 537.43 | 267.86 | 2.006 | 371 | 161/210 | 244/127 | 65.77% | 22.35% | 214.0 | 0.4926 | 0.8078 | 0.958 | 18.49 |
| confirmation | -75.60 | -97.60 | 427.81 | 503.41 | 0.850 | 440 | 205/235 | 218/222 | 49.55% | 24.71% | 214.9 | 0.5102 | 0.7714 | 1.156 | 119.18 |

### price_control_terminal__without_rtds_candles

Bucket: `terminal_discovery`; side: `both`; executable VWAP: `5` shares.

| Period | PnL | Stress | Gross profit | Gross loss | PF | Trades | UP/DOWN | W/L | Win rate | Coverage | Avg entry | Avg cost | Avg confidence | Recovery | Max DD |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| development | 256.67 | 236.77 | 487.36 | 230.69 | 2.113 | 398 | 181/217 | 313/85 | 78.64% | 23.95% | 215.1 | 0.6392 | 0.8843 | 1.743 | 25.36 |
| sealed | 256.66 | 236.46 | 509.89 | 253.23 | 2.014 | 404 | 171/233 | 306/98 | 75.74% | 24.34% | 214.3 | 0.6121 | 0.8855 | 1.551 | 18.36 |
| confirmation | -67.44 | -89.29 | 383.83 | 451.27 | 0.851 | 437 | 192/245 | 284/153 | 64.99% | 24.54% | 214.1 | 0.6627 | 0.8642 | 2.182 | 100.59 |

### price_control_terminal__with_rtds_candles

Bucket: `terminal_discovery`; side: `both`; executable VWAP: `5` shares.

| Period | PnL | Stress | Gross profit | Gross loss | PF | Trades | UP/DOWN | W/L | Win rate | Coverage | Avg entry | Avg cost | Avg confidence | Recovery | Max DD |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| development | 252.52 | 235.32 | 491.13 | 238.61 | 2.058 | 344 | 160/184 | 249/95 | 72.38% | 20.70% | 215.1 | 0.5572 | 0.8481 | 1.273 | 23.98 |
| sealed | 284.80 | 267.70 | 509.31 | 224.51 | 2.269 | 342 | 146/196 | 242/100 | 70.76% | 20.60% | 214.8 | 0.5212 | 0.8448 | 1.067 | 16.16 |
| confirmation | -46.18 | -64.33 | 361.66 | 407.84 | 0.887 | 363 | 178/185 | 199/164 | 54.82% | 20.38% | 214.6 | 0.5540 | 0.8200 | 1.368 | 75.03 |


## Winner VWAP capacity — sealed test

| Bucket | Shares | PnL | Stress | PF | Trades | Coverage |
|---|---:|---:|---:|---:|---:|---:|
| early_twap_up | 5 | 0.00 | 0.00 | 0.000 | 0 | 0.00% |
| early_twap_up | 10 | 0.00 | 0.00 | 0.000 | 0 | 0.00% |
| early_twap_up | 20 | 0.00 | 0.00 | 0.000 | 0 | 0.00% |
| early_twap_up | 30 | 0.00 | 0.00 | 0.000 | 0 | 0.00% |
| early_twap_up | 50 | 0.00 | 0.00 | 0.000 | 0 | 0.00% |
| early_twap_up | 75 | 0.00 | 0.00 | 0.000 | 0 | 0.00% |
| early_twap_up | 100 | 0.00 | 0.00 | 0.000 | 0 | 0.00% |
| early_twap_up | 125 | 0.00 | 0.00 | 0.000 | 0 | 0.00% |
| early_twap_up | 150 | 0.00 | 0.00 | 0.000 | 0 | 0.00% |
| early_twap_up | 175 | 0.00 | 0.00 | 0.000 | 0 | 0.00% |
| early_twap_up | 200 | 0.00 | 0.00 | 0.000 | 0 | 0.00% |
| bridge_early | 5 | 47.35 | 31.55 | 1.136 | 316 | 19.04% |
| bridge_early | 10 | 94.44 | 62.84 | 1.135 | 316 | 19.04% |
| bridge_early | 20 | 210.02 | 147.22 | 1.153 | 314 | 18.92% |
| bridge_early | 30 | 313.23 | 219.03 | 1.152 | 314 | 18.92% |
| bridge_early | 50 | 515.84 | 358.84 | 1.150 | 314 | 18.92% |
| bridge_early | 75 | 817.37 | 582.62 | 1.160 | 313 | 18.86% |
| bridge_early | 100 | 1115.67 | 803.67 | 1.165 | 312 | 18.80% |
| bridge_early | 125 | 1503.04 | 1115.54 | 1.180 | 310 | 18.67% |
| bridge_early | 150 | 1893.25 | 1432.75 | 1.192 | 307 | 18.49% |
| bridge_early | 175 | 2043.04 | 1509.29 | 1.177 | 305 | 18.37% |
| bridge_early | 200 | 2436.27 | 1832.27 | 1.187 | 302 | 18.19% |
| extended_specialist_down | 5 | 86.94 | 77.54 | 1.520 | 188 | 11.33% |
| extended_specialist_down | 10 | 173.65 | 154.85 | 1.519 | 188 | 11.33% |
| extended_specialist_down | 20 | 342.43 | 304.83 | 1.511 | 188 | 11.33% |
| extended_specialist_down | 30 | 503.90 | 447.80 | 1.501 | 187 | 11.27% |
| extended_specialist_down | 50 | 809.26 | 716.76 | 1.482 | 185 | 11.14% |
| extended_specialist_down | 75 | 1187.85 | 1049.85 | 1.471 | 184 | 11.08% |
| extended_specialist_down | 100 | 1503.11 | 1321.11 | 1.447 | 182 | 10.96% |
| extended_specialist_down | 125 | 1865.49 | 1637.99 | 1.443 | 182 | 10.96% |
| extended_specialist_down | 150 | 2220.79 | 1947.79 | 1.439 | 182 | 10.96% |
| extended_specialist_down | 175 | 2784.04 | 2469.04 | 1.489 | 180 | 10.84% |
| extended_specialist_down | 200 | 3059.39 | 2701.39 | 1.470 | 179 | 10.78% |
| middle_q5_down | 5 | 78.49 | 69.89 | 1.502 | 172 | 10.36% |
| middle_q5_down | 10 | 156.87 | 139.67 | 1.501 | 172 | 10.36% |
| middle_q5_down | 20 | 312.96 | 278.56 | 1.500 | 172 | 10.36% |
| middle_q5_down | 30 | 464.27 | 412.67 | 1.494 | 172 | 10.36% |
| middle_q5_down | 50 | 777.99 | 692.99 | 1.505 | 170 | 10.24% |
| middle_q5_down | 75 | 1132.21 | 1005.46 | 1.489 | 169 | 10.18% |
| middle_q5_down | 100 | 1453.12 | 1285.12 | 1.471 | 168 | 10.12% |
| middle_q5_down | 125 | 1766.98 | 1558.23 | 1.458 | 167 | 10.06% |
| middle_q5_down | 150 | 2024.10 | 1776.60 | 1.437 | 165 | 9.94% |
| middle_q5_down | 175 | 2347.89 | 2059.14 | 1.434 | 165 | 9.94% |
| middle_q5_down | 200 | 2665.26 | 2335.26 | 1.430 | 165 | 9.94% |
| multivenue_middle | 5 | 91.98 | 69.78 | 1.201 | 444 | 26.75% |
| multivenue_middle | 10 | 187.63 | 143.33 | 1.206 | 443 | 26.69% |
| multivenue_middle | 20 | 377.84 | 289.64 | 1.209 | 441 | 26.57% |
| multivenue_middle | 30 | 572.50 | 440.80 | 1.212 | 439 | 26.45% |
| multivenue_middle | 50 | 942.28 | 722.78 | 1.209 | 439 | 26.45% |
| multivenue_middle | 75 | 1340.24 | 1011.74 | 1.198 | 438 | 26.39% |
| multivenue_middle | 100 | 1760.37 | 1322.37 | 1.194 | 438 | 26.39% |
| multivenue_middle | 125 | 2166.23 | 1618.73 | 1.191 | 438 | 26.39% |
| multivenue_middle | 150 | 2777.09 | 2126.09 | 1.208 | 434 | 26.14% |
| multivenue_middle | 175 | 3152.50 | 2394.75 | 1.202 | 433 | 26.08% |
| multivenue_middle | 200 | 3503.57 | 2643.57 | 1.198 | 430 | 25.90% |
| settlement_middle | 5 | 128.97 | 103.12 | 1.269 | 517 | 31.14% |
| settlement_middle | 10 | 256.16 | 204.56 | 1.267 | 516 | 31.08% |
| settlement_middle | 20 | 499.78 | 397.18 | 1.261 | 513 | 30.90% |
| settlement_middle | 30 | 744.87 | 591.27 | 1.260 | 512 | 30.84% |
| settlement_middle | 50 | 1253.43 | 997.93 | 1.263 | 511 | 30.78% |
| settlement_middle | 75 | 1942.57 | 1561.57 | 1.275 | 508 | 30.60% |
| settlement_middle | 100 | 2523.87 | 2018.87 | 1.268 | 505 | 30.42% |
| settlement_middle | 125 | 3212.28 | 2583.53 | 1.274 | 503 | 30.30% |
| settlement_middle | 150 | 3787.05 | 3035.55 | 1.269 | 501 | 30.18% |
| settlement_middle | 175 | 4268.28 | 3396.78 | 1.260 | 498 | 30.00% |
| settlement_middle | 200 | 4802.07 | 3806.07 | 1.255 | 498 | 30.00% |
| late_discovery | 5 | 214.61 | 201.66 | 2.250 | 259 | 15.60% |
| late_discovery | 10 | 428.69 | 402.79 | 2.248 | 259 | 15.60% |
| late_discovery | 20 | 850.27 | 798.67 | 2.237 | 258 | 15.54% |
| late_discovery | 30 | 1273.03 | 1195.63 | 2.234 | 258 | 15.54% |
| late_discovery | 50 | 2113.69 | 1984.69 | 2.228 | 258 | 15.54% |
| late_discovery | 75 | 3155.70 | 2962.20 | 2.221 | 258 | 15.54% |
| late_discovery | 100 | 4164.59 | 3907.59 | 2.206 | 257 | 15.48% |
| late_discovery | 125 | 5120.81 | 4802.06 | 2.184 | 255 | 15.36% |
| late_discovery | 150 | 6112.28 | 5729.78 | 2.176 | 255 | 15.36% |
| late_discovery | 175 | 7099.89 | 6653.64 | 2.169 | 255 | 15.36% |
| late_discovery | 200 | 8077.92 | 7567.92 | 2.162 | 255 | 15.36% |
| terminal_discovery | 5 | 293.33 | 269.38 | 1.944 | 479 | 28.86% |
| terminal_discovery | 10 | 584.42 | 536.62 | 1.940 | 478 | 28.80% |
| terminal_discovery | 20 | 1164.71 | 1069.51 | 1.940 | 476 | 28.67% |
| terminal_discovery | 30 | 1752.09 | 1610.19 | 1.956 | 473 | 28.49% |
| terminal_discovery | 50 | 2894.41 | 2658.91 | 1.947 | 471 | 28.37% |
| terminal_discovery | 75 | 4364.84 | 4013.84 | 1.964 | 468 | 28.19% |
| terminal_discovery | 100 | 5773.83 | 5306.83 | 1.956 | 467 | 28.13% |
| terminal_discovery | 125 | 7121.32 | 6543.82 | 1.948 | 462 | 27.83% |
| terminal_discovery | 150 | 8470.72 | 7780.72 | 1.938 | 460 | 27.71% |
| terminal_discovery | 175 | 9779.21 | 8975.96 | 1.926 | 459 | 27.65% |
| terminal_discovery | 200 | 11086.50 | 10168.50 | 1.917 | 459 | 27.65% |

## Integrity

- Training, policy fitting, sealed testing, and confirmation windows are chronological and disjoint.
- Settlement labels, outcomes, and execution prices are supervision/evaluation only; the early specialist uses only causal TWAP/refprice state available at its decision timestamp.
- RTDS candle and RTDS-free variants are paired on identical rows, splits, policies, and seeds.
- Economic replay is chronological and admits at most one entry per market.
- Inputs are immutable existing Parquet artifacts; no database, ingester, source, table, schema, runtime, or deployment was changed.
- The sealed interval has been observed by prior research and is chronological but not epistemically fresh.
- Current executable VWAP evidence is retained through September 13 with observed interruptions reported as missing coverage.
