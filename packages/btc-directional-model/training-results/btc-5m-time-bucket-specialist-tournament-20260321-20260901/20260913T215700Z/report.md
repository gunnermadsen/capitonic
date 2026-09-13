# BTC Five-Minute Time-Bucket Specialist Tournament

Run: `20260913T215700Z`
Qualification: **trained_evaluated_not_qualified**

## Hypothesis

Historically successful model families are retrained only for the time slices where they showed edge, then the winning bucket descendants are routed together as one challenger.

## Composed sealed result

| PnL | Stress PnL | PF | Trades | Wins/Losses | Coverage | Avg entry | Recovery wins/loss | Max DD |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| -1321.51 | -1915.01 | 0.895 | 1187 | 792/395 | 68.69% | 100.2 | 2.240 | 1754.97 |

## Bucket winners

| Bucket | Winner | RTDS | Side | VWAP | PnL | Stress | PF | Trades | UP/DOWN | W/L | Coverage | Avg entry | Recovery |
|---|---|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| early_twap_up | early_refprice_single_regime | with_rtds_candles | up | 50 | -256.54 | -363.54 | 0.903 | 214 | 214/0 | 123/91 | 12.47% | 41.4 | 1.497 |
| bridge_early | bridge_aware_specialist | with_rtds_candles | down | 50 | 142.75 | 88.25 | 1.126 | 109 | 0/109 | 74/35 | 6.31% | 62.7 | 1.879 |
| extended_specialist_down | extended_specialist_official | with_rtds_candles | down | 50 | -12.83 | -253.83 | 0.997 | 482 | 0/482 | 325/157 | 27.89% | 77.0 | 2.076 |
| middle_q5_down | crossvenue_middle_110 | with_rtds_candles | down | 50 | 111.14 | 62.14 | 1.104 | 98 | 0/98 | 60/38 | 5.67% | 111.6 | 1.430 |
| multivenue_middle | kraken_crossvenue_middle | without_rtds_candles | both | 50 | -1279.67 | -1549.17 | 0.784 | 539 | 259/280 | 366/173 | 31.19% | 124.6 | 2.698 |
| settlement_middle | middle_agreement | without_rtds_candles | both | 50 | -899.31 | -1234.31 | 0.855 | 670 | 302/368 | 501/169 | 38.77% | 152.8 | 3.468 |
| late_discovery | multivenue_late | with_rtds_candles | down | 50 | 189.32 | 137.32 | 1.183 | 104 | 0/104 | 71/33 | 6.02% | 191.5 | 1.819 |
| terminal_discovery | price_control_terminal | with_rtds_candles | down | 50 | -611.61 | -746.61 | 0.785 | 270 | 0/270 | 176/94 | 15.62% | 214.2 | 2.387 |

## All candidate variants — sealed test

| Bucket | Candidate | RTDS | Side | VWAP | Confidence | PnL | Stress | PF | Trades | UP/DOWN | W/L | Coverage | Avg entry | Recovery |
|---|---|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| early_twap_up | early_twap_open_interest | without_rtds_candles | up | 50 | 0.65 | 497.68 | 324.68 | 1.153 | 346 | 346/0 | 246/100 | 20.16% | 41.0 | 2.134 |
| early_twap_up | early_twap_open_interest | with_rtds_candles | up | 50 | 0.55 | -138.22 | -195.22 | 0.900 | 114 | 114/0 | 60/54 | 6.64% | 41.5 | 1.234 |
| early_twap_up | early_refprice_single_regime | without_rtds_candles | up | 30 | 0.55 | -177.77 | -328.07 | 0.948 | 501 | 501/0 | 318/183 | 29.20% | 41.2 | 1.832 |
| early_twap_up | early_refprice_single_regime | with_rtds_candles | up | 50 | 0.55 | -256.54 | -363.54 | 0.903 | 214 | 214/0 | 123/91 | 12.47% | 41.4 | 1.497 |
| early_twap_up | early_twap_single_regime | without_rtds_candles | up | 50 | 0.55 | -873.21 | -1098.21 | 0.848 | 450 | 450/0 | 260/190 | 26.22% | 41.3 | 1.615 |
| early_twap_up | early_twap_single_regime | with_rtds_candles | up | 50 | 0.55 | -16.25 | -55.75 | 0.982 | 79 | 79/0 | 44/35 | 4.60% | 41.7 | 1.280 |
| bridge_early | bridge_aware_specialist | without_rtds_candles | down | 50 | 0.75 | 77.67 | 7.67 | 1.051 | 140 | 0/140 | 93/47 | 8.10% | 62.7 | 1.883 |
| bridge_early | bridge_aware_specialist | with_rtds_candles | down | 50 | 0.75 | 142.75 | 88.25 | 1.126 | 109 | 0/109 | 74/35 | 6.31% | 62.7 | 1.879 |
| bridge_early | time_specialist_early | without_rtds_candles | up | 50 | 0.55 | 559.84 | 484.84 | 1.494 | 150 | 150/0 | 113/37 | 8.68% | 63.1 | 2.044 |
| bridge_early | time_specialist_early | with_rtds_candles | up | 50 | 0.85 | 322.84 | 274.84 | 1.888 | 96 | 96/0 | 87/9 | 5.56% | 63.3 | 5.121 |
| extended_specialist_down | extended_specialist_official | without_rtds_candles | down | 5 | 0.75 | -59.42 | -80.97 | 0.854 | 431 | 0/431 | 319/112 | 24.94% | 76.9 | 3.335 |
| extended_specialist_down | extended_specialist_official | with_rtds_candles | down | 50 | 0.55 | -12.83 | -253.83 | 0.997 | 482 | 0/482 | 325/157 | 27.89% | 77.0 | 2.076 |
| extended_specialist_down | specialist_dual_head | without_rtds_candles | down | 5 | 0.75 | -59.42 | -80.97 | 0.854 | 431 | 0/431 | 319/112 | 24.94% | 76.9 | 3.335 |
| extended_specialist_down | specialist_dual_head | with_rtds_candles | down | 50 | 0.55 | -12.83 | -253.83 | 0.997 | 482 | 0/482 | 325/157 | 27.89% | 77.0 | 2.076 |
| middle_q5_down | middle_q5_admission | without_rtds_candles | down | 50 | 0.55 | -297.38 | -399.38 | 0.879 | 204 | 0/204 | 118/86 | 11.81% | 111.3 | 1.561 |
| middle_q5_down | middle_q5_admission | with_rtds_candles | down | 10 | 0.75 | 12.65 | 3.55 | 1.068 | 91 | 0/91 | 63/28 | 5.27% | 111.7 | 2.106 |
| middle_q5_down | crossvenue_middle_110 | without_rtds_candles | down | 5 | 0.75 | -12.48 | -21.38 | 0.934 | 178 | 0/178 | 123/55 | 10.30% | 111.1 | 2.394 |
| middle_q5_down | crossvenue_middle_110 | with_rtds_candles | down | 50 | 0.65 | 111.14 | 62.14 | 1.104 | 98 | 0/98 | 60/38 | 5.67% | 111.6 | 1.430 |
| multivenue_middle | multivenue_consensus | without_rtds_candles | both | 50 | 0.75 | -1371.31 | -1866.31 | 0.843 | 990 | 438/552 | 751/239 | 57.29% | 124.1 | 3.728 |
| multivenue_middle | multivenue_consensus | with_rtds_candles | down | 50 | 0.75 | -58.97 | -142.97 | 0.968 | 168 | 0/168 | 110/58 | 9.72% | 127.9 | 1.959 |
| multivenue_middle | kraken_crossvenue_middle | without_rtds_candles | both | 50 | 0.75 | -1279.67 | -1549.17 | 0.784 | 539 | 259/280 | 366/173 | 31.19% | 124.6 | 2.698 |
| multivenue_middle | kraken_crossvenue_middle | with_rtds_candles | down | 50 | 0.65 | -337.83 | -452.83 | 0.880 | 230 | 0/230 | 136/94 | 13.31% | 125.8 | 1.645 |
| settlement_middle | crossvenue_settlement_middle | without_rtds_candles | both | 50 | 0.75 | -801.57 | -1124.07 | 0.864 | 645 | 314/331 | 482/163 | 37.33% | 153.0 | 3.423 |
| settlement_middle | crossvenue_settlement_middle | with_rtds_candles | down | 50 | 0.65 | 198.30 | 8.30 | 1.058 | 380 | 0/380 | 281/99 | 21.99% | 152.8 | 2.682 |
| settlement_middle | middle_agreement | without_rtds_candles | both | 50 | 0.75 | -899.31 | -1234.31 | 0.855 | 670 | 302/368 | 501/169 | 38.77% | 152.8 | 3.468 |
| settlement_middle | middle_agreement | with_rtds_candles | both | 50 | 0.75 | -581.75 | -879.75 | 0.891 | 596 | 261/335 | 451/145 | 34.49% | 153.1 | 3.490 |
| settlement_middle | price_time_calibrated_middle | without_rtds_candles | both | 50 | 0.85 | -805.81 | -1028.81 | 0.783 | 446 | 209/237 | 350/96 | 25.81% | 153.3 | 4.655 |
| settlement_middle | price_time_calibrated_middle | with_rtds_candles | down | 50 | 0.65 | 470.19 | 283.19 | 1.148 | 374 | 0/374 | 280/94 | 21.64% | 152.7 | 2.594 |
| late_discovery | multivenue_late | without_rtds_candles | both | 50 | 0.85 | -102.95 | -142.95 | 0.899 | 80 | 29/51 | 49/31 | 4.63% | 191.6 | 1.759 |
| late_discovery | multivenue_late | with_rtds_candles | down | 50 | 0.75 | 189.32 | 137.32 | 1.183 | 104 | 0/104 | 71/33 | 6.02% | 191.5 | 1.819 |
| late_discovery | time_specialist_late | without_rtds_candles | both | 50 | 0.85 | -102.95 | -142.95 | 0.899 | 80 | 29/51 | 49/31 | 4.63% | 191.6 | 1.759 |
| late_discovery | time_specialist_late | with_rtds_candles | down | 50 | 0.75 | 189.32 | 137.32 | 1.183 | 104 | 0/104 | 71/33 | 6.02% | 191.5 | 1.819 |
| terminal_discovery | settlement_aligned_terminal | without_rtds_candles | down | 50 | 0.85 | -158.13 | -274.13 | 0.919 | 232 | 0/232 | 180/52 | 13.43% | 214.6 | 3.767 |
| terminal_discovery | settlement_aligned_terminal | with_rtds_candles | down | 50 | 0.65 | -190.94 | -327.94 | 0.926 | 274 | 0/274 | 191/83 | 15.86% | 214.2 | 2.485 |
| terminal_discovery | kraken_crossvenue_terminal | without_rtds_candles | down | 50 | 0.75 | -264.28 | -339.78 | 0.842 | 151 | 0/151 | 93/58 | 8.74% | 214.0 | 1.904 |
| terminal_discovery | kraken_crossvenue_terminal | with_rtds_candles | down | 50 | 0.65 | -666.26 | -814.76 | 0.773 | 297 | 0/297 | 203/94 | 17.19% | 213.8 | 2.792 |
| terminal_discovery | price_control_terminal | without_rtds_candles | down | 50 | 0.85 | -75.26 | -125.76 | 0.925 | 101 | 0/101 | 68/33 | 5.84% | 215.5 | 2.227 |
| terminal_discovery | price_control_terminal | with_rtds_candles | down | 50 | 0.65 | -611.61 | -746.61 | 0.785 | 270 | 0/270 | 176/94 | 15.62% | 214.2 | 2.387 |

## Winner VWAP capacity — sealed test

| Bucket | Shares | PnL | Stress | PF | Trades | Coverage |
|---|---:|---:|---:|---:|---:|---:|
| early_twap_up | 5 | -24.31 | -35.16 | 0.909 | 217 | 12.65% |
| early_twap_up | 10 | -48.71 | -70.41 | 0.909 | 217 | 12.65% |
| early_twap_up | 20 | -97.78 | -141.18 | 0.909 | 217 | 12.65% |
| early_twap_up | 30 | -147.56 | -212.66 | 0.908 | 217 | 12.65% |
| early_twap_up | 50 | -256.54 | -363.54 | 0.903 | 214 | 12.47% |
| early_twap_up | 75 | -391.48 | -551.98 | 0.902 | 214 | 12.47% |
| early_twap_up | 100 | -505.92 | -717.92 | 0.903 | 212 | 12.35% |
| early_twap_up | 125 | -642.89 | -907.89 | 0.902 | 212 | 12.35% |
| early_twap_up | 150 | -783.53 | -1101.53 | 0.900 | 212 | 12.35% |
| early_twap_up | 175 | -928.25 | -1299.25 | 0.899 | 212 | 12.35% |
| early_twap_up | 200 | -679.41 | -1097.41 | 0.933 | 209 | 12.18% |
| bridge_early | 5 | 17.80 | 12.25 | 1.157 | 111 | 6.42% |
| bridge_early | 10 | 35.42 | 24.32 | 1.156 | 111 | 6.42% |
| bridge_early | 20 | 70.09 | 47.89 | 1.154 | 111 | 6.42% |
| bridge_early | 30 | 95.53 | 62.53 | 1.140 | 110 | 6.37% |
| bridge_early | 50 | 142.75 | 88.25 | 1.126 | 109 | 6.31% |
| bridge_early | 75 | 209.74 | 127.99 | 1.123 | 109 | 6.31% |
| bridge_early | 100 | 273.83 | 164.83 | 1.120 | 109 | 6.31% |
| bridge_early | 125 | 388.92 | 255.17 | 1.141 | 107 | 6.19% |
| bridge_early | 150 | 458.87 | 298.37 | 1.139 | 107 | 6.19% |
| bridge_early | 175 | 526.10 | 338.85 | 1.136 | 107 | 6.19% |
| bridge_early | 200 | 478.55 | 268.55 | 1.108 | 105 | 6.08% |
| extended_specialist_down | 5 | 4.00 | -20.25 | 1.008 | 485 | 28.07% |
| extended_specialist_down | 10 | 7.40 | -41.10 | 1.008 | 485 | 28.07% |
| extended_specialist_down | 20 | 9.21 | -87.59 | 1.005 | 484 | 28.01% |
| extended_specialist_down | 30 | 10.38 | -134.82 | 1.004 | 484 | 28.01% |
| extended_specialist_down | 50 | -12.83 | -253.83 | 0.997 | 482 | 27.89% |
| extended_specialist_down | 75 | 92.11 | -264.89 | 1.013 | 476 | 27.55% |
| extended_specialist_down | 100 | 98.36 | -372.64 | 1.010 | 471 | 27.26% |
| extended_specialist_down | 125 | 25.64 | -560.61 | 1.002 | 469 | 27.14% |
| extended_specialist_down | 150 | -5.70 | -709.20 | 1.000 | 469 | 27.14% |
| extended_specialist_down | 175 | 14.55 | -802.70 | 1.001 | 467 | 27.03% |
| extended_specialist_down | 200 | -146.79 | -1074.79 | 0.992 | 464 | 26.85% |
| middle_q5_down | 5 | 13.09 | 8.14 | 1.123 | 99 | 5.73% |
| middle_q5_down | 10 | 26.18 | 16.28 | 1.123 | 99 | 5.73% |
| middle_q5_down | 20 | 52.14 | 32.34 | 1.123 | 99 | 5.73% |
| middle_q5_down | 30 | 77.74 | 48.04 | 1.122 | 99 | 5.73% |
| middle_q5_down | 50 | 111.14 | 62.14 | 1.104 | 98 | 5.67% |
| middle_q5_down | 75 | 120.51 | 48.51 | 1.075 | 96 | 5.56% |
| middle_q5_down | 100 | 70.57 | -22.43 | 1.033 | 93 | 5.38% |
| middle_q5_down | 125 | 81.79 | -34.46 | 1.031 | 93 | 5.38% |
| middle_q5_down | 150 | 91.69 | -47.81 | 1.029 | 93 | 5.38% |
| middle_q5_down | 175 | 100.65 | -62.10 | 1.027 | 93 | 5.38% |
| middle_q5_down | 200 | 108.15 | -77.85 | 1.025 | 93 | 5.38% |
| multivenue_middle | 5 | -126.90 | -154.25 | 0.789 | 547 | 31.66% |
| multivenue_middle | 10 | -254.36 | -309.06 | 0.788 | 547 | 31.66% |
| multivenue_middle | 20 | -516.15 | -625.35 | 0.785 | 546 | 31.60% |
| multivenue_middle | 30 | -766.89 | -930.09 | 0.786 | 544 | 31.48% |
| multivenue_middle | 50 | -1279.67 | -1549.17 | 0.784 | 539 | 31.19% |
| multivenue_middle | 75 | -1895.83 | -2297.83 | 0.786 | 536 | 31.02% |
| multivenue_middle | 100 | -2606.24 | -3139.24 | 0.779 | 533 | 30.84% |
| multivenue_middle | 125 | -3305.59 | -3965.59 | 0.774 | 528 | 30.56% |
| multivenue_middle | 150 | -3936.44 | -4722.44 | 0.775 | 524 | 30.32% |
| multivenue_middle | 175 | -4684.65 | -5598.15 | 0.770 | 522 | 30.21% |
| multivenue_middle | 200 | -5321.75 | -6355.75 | 0.770 | 517 | 29.92% |
| settlement_middle | 5 | -88.76 | -122.86 | 0.858 | 682 | 39.47% |
| settlement_middle | 10 | -180.41 | -248.51 | 0.856 | 681 | 39.41% |
| settlement_middle | 20 | -370.73 | -506.53 | 0.852 | 679 | 39.29% |
| settlement_middle | 30 | -572.31 | -775.11 | 0.848 | 676 | 39.12% |
| settlement_middle | 50 | -899.31 | -1234.31 | 0.855 | 670 | 38.77% |
| settlement_middle | 75 | -1407.42 | -1906.92 | 0.848 | 666 | 38.54% |
| settlement_middle | 100 | -1853.89 | -2515.89 | 0.849 | 662 | 38.31% |
| settlement_middle | 125 | -2389.00 | -3214.00 | 0.845 | 660 | 38.19% |
| settlement_middle | 150 | -2785.89 | -3772.89 | 0.848 | 658 | 38.08% |
| settlement_middle | 175 | -3371.07 | -4515.57 | 0.842 | 654 | 37.85% |
| settlement_middle | 200 | -3694.08 | -5000.08 | 0.848 | 653 | 37.79% |
| late_discovery | 5 | 21.78 | 16.28 | 1.204 | 110 | 6.37% |
| late_discovery | 10 | 40.58 | 29.68 | 1.190 | 109 | 6.31% |
| late_discovery | 20 | 74.79 | 53.19 | 1.175 | 108 | 6.25% |
| late_discovery | 30 | 132.47 | 100.67 | 1.214 | 106 | 6.13% |
| late_discovery | 50 | 189.32 | 137.32 | 1.183 | 104 | 6.02% |
| late_discovery | 75 | 261.38 | 184.13 | 1.168 | 103 | 5.96% |
| late_discovery | 100 | 357.92 | 257.92 | 1.179 | 100 | 5.79% |
| late_discovery | 125 | 420.80 | 297.05 | 1.168 | 99 | 5.73% |
| late_discovery | 150 | 425.14 | 279.64 | 1.142 | 97 | 5.61% |
| late_discovery | 175 | 484.80 | 315.05 | 1.138 | 97 | 5.61% |
| late_discovery | 200 | 485.37 | 293.37 | 1.121 | 96 | 5.56% |
| terminal_discovery | 5 | -58.45 | -72.00 | 0.793 | 271 | 15.68% |
| terminal_discovery | 10 | -117.35 | -144.45 | 0.793 | 271 | 15.68% |
| terminal_discovery | 20 | -236.62 | -290.82 | 0.791 | 271 | 15.68% |
| terminal_discovery | 30 | -357.78 | -439.08 | 0.790 | 271 | 15.68% |
| terminal_discovery | 50 | -611.61 | -746.61 | 0.785 | 270 | 15.62% |
| terminal_discovery | 75 | -970.61 | -1171.61 | 0.772 | 268 | 15.51% |
| terminal_discovery | 100 | -1244.87 | -1510.87 | 0.779 | 266 | 15.39% |
| terminal_discovery | 125 | -1584.20 | -1916.70 | 0.775 | 266 | 15.39% |
| terminal_discovery | 150 | -1993.23 | -2389.23 | 0.765 | 264 | 15.28% |
| terminal_discovery | 175 | -2376.55 | -2836.80 | 0.760 | 263 | 15.22% |
| terminal_discovery | 200 | -2757.45 | -3283.45 | 0.757 | 263 | 15.22% |

## Integrity

- Training, policy fitting, sealed testing, and confirmation windows are chronological and disjoint.
- Settlement labels, outcomes, and execution prices are supervision/evaluation only; the early specialist uses only causal TWAP/refprice state available at its decision timestamp.
- RTDS candle and RTDS-free variants are paired on identical rows, splits, policies, and seeds.
- Economic replay is chronological and admits at most one entry per market.
- Inputs are immutable existing Parquet artifacts; no database, ingester, source, table, schema, runtime, or deployment was changed.
- The sealed interval has been observed by prior research and is chronological but not epistemically fresh.
- Order-book VWAP evidence ends on August 26; the August 26-September 1 confirmation block is prediction-only and cannot produce economic trades.
