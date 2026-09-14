# Historical time-bucket aggregate tournament

Four aggregate challengers were trained from reproducible historical donor families in frozen ten-second buckets. All PnL is five-share PnL.

## Net challenger metrics

| Challenger | RTDS | Period | PnL | Stress | Gross +/− | PF | Trades | W/L | Win rate | UP/DOWN | Coverage | Avg entry | Avg cost | Avg confidence | Recovery | Max DD |
|---|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| `aggregate_leader` | without_rtds_candles | policy | 290.68 | 238.48 | 1265.92/975.24 | 1.298 | 1044 | 673/371 | 64.46% | 417/627 | 62.82% | 109.4 | 0.5687 | 0.7351 | 1.397 | 38.57 |
| `aggregate_leader` | without_rtds_candles | sealed | 135.12 | 86.82 | 1127.83/992.72 | 1.136 | 966 | 579/387 | 59.94% | 390/576 | 58.19% | 110.4 | 0.5512 | 0.7310 | 1.317 | 71.85 |
| `aggregate_leader` | without_rtds_candles | confirmation | -20.59 | -84.99 | 1383.34/1403.94 | 0.985 | 1288 | 746/542 | 57.92% | 533/755 | 72.32% | 115.2 | 0.5621 | 0.7157 | 1.397 | 105.20 |
| `aggregate_leader` | with_rtds_candles | policy | 266.81 | 212.41 | 1306.98/1040.17 | 1.257 | 1088 | 697/391 | 64.06% | 427/661 | 65.46% | 101.2 | 0.5712 | 0.7179 | 1.419 | 44.20 |
| `aggregate_leader` | with_rtds_candles | sealed | 227.96 | 176.21 | 1245.61/1017.65 | 1.224 | 1035 | 647/388 | 62.51% | 422/613 | 62.35% | 100.5 | 0.5608 | 0.7138 | 1.362 | 50.43 |
| `aggregate_leader` | with_rtds_candles | confirmation | -138.10 | -207.95 | 1431.46/1569.57 | 0.912 | 1397 | 812/585 | 58.12% | 578/819 | 78.44% | 104.1 | 0.5807 | 0.7103 | 1.522 | 227.28 |
| `aggregate_diverse` | without_rtds_candles | policy | 248.48 | 195.68 | 1279.84/1031.36 | 1.241 | 1056 | 675/381 | 63.92% | 419/637 | 63.54% | 99.3 | 0.5716 | 0.7168 | 1.428 | 62.32 |
| `aggregate_diverse` | without_rtds_candles | sealed | 165.67 | 116.12 | 1184.92/1019.25 | 1.163 | 991 | 603/388 | 60.85% | 411/580 | 59.70% | 99.2 | 0.5546 | 0.7111 | 1.337 | 70.62 |
| `aggregate_diverse` | without_rtds_candles | confirmation | -7.61 | -73.31 | 1433.41/1441.02 | 0.995 | 1314 | 770/544 | 58.60% | 540/774 | 73.78% | 105.5 | 0.5667 | 0.7021 | 1.423 | 104.29 |
| `aggregate_diverse` | with_rtds_candles | policy | 300.94 | 246.44 | 1312.26/1011.31 | 1.298 | 1090 | 716/374 | 65.69% | 414/676 | 65.58% | 101.3 | 0.5814 | 0.7221 | 1.475 | 58.59 |
| `aggregate_diverse` | with_rtds_candles | sealed | 212.72 | 161.12 | 1221.48/1008.76 | 1.211 | 1032 | 654/378 | 63.37% | 413/619 | 62.17% | 100.6 | 0.5723 | 0.7207 | 1.429 | 53.87 |
| `aggregate_diverse` | with_rtds_candles | confirmation | -45.01 | -115.01 | 1445.76/1490.77 | 0.970 | 1400 | 848/552 | 60.57% | 555/845 | 78.61% | 105.5 | 0.5921 | 0.7205 | 1.584 | 145.53 |
| `aggregate_side_split` | without_rtds_candles | policy | 291.15 | 238.05 | 1298.98/1007.82 | 1.289 | 1062 | 682/380 | 64.22% | 388/674 | 63.90% | 106.1 | 0.5671 | 0.7238 | 1.392 | 46.92 |
| `aggregate_side_split` | without_rtds_candles | sealed | 163.09 | 113.89 | 1170.82/1007.73 | 1.162 | 984 | 600/384 | 60.98% | 380/604 | 59.28% | 106.3 | 0.5564 | 0.7249 | 1.345 | 47.60 |
| `aggregate_side_split` | without_rtds_candles | confirmation | -29.31 | -95.56 | 1413.94/1443.25 | 0.980 | 1325 | 772/553 | 58.26% | 510/815 | 74.40% | 110.5 | 0.5668 | 0.7124 | 1.425 | 155.79 |
| `aggregate_side_split` | with_rtds_candles | policy | 279.06 | 223.06 | 1342.14/1063.08 | 1.263 | 1120 | 723/397 | 64.55% | 431/689 | 67.39% | 98.6 | 0.5754 | 0.7172 | 1.442 | 47.86 |
| `aggregate_side_split` | with_rtds_candles | sealed | 171.22 | 117.67 | 1235.12/1063.89 | 1.161 | 1071 | 665/406 | 62.09% | 423/648 | 64.52% | 97.7 | 0.5689 | 0.7188 | 1.411 | 79.94 |
| `aggregate_side_split` | with_rtds_candles | confirmation | -176.03 | -248.83 | 1460.79/1636.82 | 0.892 | 1456 | 841/615 | 57.76% | 590/866 | 81.75% | 100.2 | 0.5817 | 0.7124 | 1.532 | 218.95 |
| `aggregate_consensus` | without_rtds_candles | policy | 265.77 | 213.12 | 1283.74/1017.98 | 1.261 | 1053 | 679/374 | 64.48% | 421/632 | 63.36% | 99.9 | 0.5738 | 0.7192 | 1.440 | 54.86 |
| `aggregate_consensus` | without_rtds_candles | sealed | 187.84 | 138.44 | 1191.73/1003.89 | 1.187 | 988 | 609/379 | 61.64% | 406/582 | 59.52% | 100.0 | 0.5580 | 0.7135 | 1.354 | 71.73 |
| `aggregate_consensus` | without_rtds_candles | confirmation | 4.80 | -60.40 | 1426.62/1421.82 | 1.003 | 1304 | 771/533 | 59.13% | 528/776 | 73.22% | 106.3 | 0.5701 | 0.7052 | 1.442 | 100.22 |
| `aggregate_consensus` | with_rtds_candles | policy | 304.53 | 249.83 | 1317.83/1013.30 | 1.301 | 1094 | 721/373 | 65.90% | 422/672 | 65.82% | 100.7 | 0.5831 | 0.7221 | 1.486 | 58.62 |
| `aggregate_consensus` | with_rtds_candles | sealed | 267.39 | 215.79 | 1245.69/978.30 | 1.273 | 1032 | 669/363 | 64.83% | 416/616 | 62.17% | 99.9 | 0.5762 | 0.7221 | 1.447 | 39.65 |
| `aggregate_consensus` | with_rtds_candles | confirmation | -50.30 | -119.90 | 1440.72/1491.02 | 0.966 | 1392 | 845/547 | 60.70% | 548/844 | 78.16% | 104.7 | 0.5942 | 0.7208 | 1.599 | 153.18 |

## Previous tournament comparison

| Model | Period | PnL | Stress | PF | Trades | W/L |
|---|---|---:|---:|---:|---:|---:|
| Previous bucket tournament | sealed | 181.65 | 137.25 | 1.202 | 888 | 532/356 |
| Previous bucket tournament | confirmation | -49.94 | -109.19 | 0.962 | 1185 | 672/513 |

## aggregate_leader: per-bucket PnL

| Seconds | RTDS | Period | Donors | PnL | Stress | Gross +/− | PF | Trades | W/L | Win rate | UP/DOWN | Coverage | Avg entry | Avg cost | Avg confidence | Recovery | Max DD |
|---:|---|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 60-69 | with_rtds_candles | confirmation | chainlink_fair_value, q5_context, crossvenue | -15.55 | -35.00 | 449.26/464.81 | 0.967 | 389 | 210/179 | 53.98% | 112/277 | 21.84% | 61.2 | 0.5261 | 0.6661 | 1.214 | 69.19 |
| 60-69 | with_rtds_candles | sealed | chainlink_fair_value, q5_context, crossvenue | 92.00 | 76.00 | 419.92/327.92 | 1.281 | 320 | 192/128 | 60.00% | 116/204 | 19.28% | 61.4 | 0.5208 | 0.6769 | 1.171 | 39.63 |
| 60-69 | without_rtds_candles | confirmation | chainlink_fair_value, q5_context, crossvenue | -21.87 | -34.67 | 290.04/311.92 | 0.930 | 256 | 128/128 | 50.00% | 64/192 | 14.37% | 61.4 | 0.4955 | 0.6778 | 1.075 | 60.55 |
| 60-69 | without_rtds_candles | sealed | chainlink_fair_value, q5_context, crossvenue | 27.15 | 15.65 | 284.93/257.78 | 1.105 | 230 | 125/105 | 54.35% | 82/148 | 13.86% | 61.7 | 0.4983 | 0.7028 | 1.077 | 44.77 |
| 70-79 | with_rtds_candles | confirmation | q5_context, price_control, chainlink_fair_value | -56.17 | -68.87 | 276.64/332.81 | 0.831 | 254 | 120/134 | 47.24% | 77/177 | 14.26% | 71.2 | 0.4950 | 0.6854 | 1.077 | 84.50 |
| 70-79 | with_rtds_candles | sealed | q5_context, price_control, chainlink_fair_value | 56.37 | 44.37 | 299.10/242.73 | 1.232 | 240 | 140/100 | 58.33% | 74/166 | 14.46% | 71.4 | 0.5151 | 0.7173 | 1.136 | 37.46 |
| 70-79 | without_rtds_candles | confirmation | q5_context, price_control, chainlink_fair_value | -24.85 | -34.85 | 222.88/247.73 | 0.900 | 200 | 102/98 | 51.00% | 0/200 | 11.23% | 71.1 | 0.5133 | 0.7054 | 1.157 | 61.38 |
| 70-79 | without_rtds_candles | sealed | q5_context, price_control, chainlink_fair_value | 61.47 | 52.82 | 221.52/160.05 | 1.384 | 173 | 104/69 | 60.12% | 0/173 | 10.42% | 71.4 | 0.5088 | 0.7252 | 1.089 | 19.21 |
| 80-89 | with_rtds_candles | confirmation | chainlink_fair_value, external_flow, q5_context | -0.06 | -10.81 | 257.44/257.49 | 1.000 | 215 | 110/105 | 51.16% | 71/144 | 12.07% | 81.5 | 0.4899 | 0.6755 | 1.048 | 38.40 |
| 80-89 | with_rtds_candles | sealed | chainlink_fair_value, external_flow, q5_context | 95.53 | 85.43 | 287.55/192.02 | 1.498 | 202 | 123/79 | 60.89% | 77/125 | 12.17% | 81.3 | 0.4928 | 0.7096 | 1.040 | 33.96 |
| 80-89 | without_rtds_candles | confirmation | chainlink_fair_value, external_flow, q5_context | -13.75 | -25.05 | 266.91/280.66 | 0.951 | 226 | 113/113 | 50.00% | 70/156 | 12.69% | 81.2 | 0.4905 | 0.6807 | 1.052 | 53.19 |
| 80-89 | without_rtds_candles | sealed | chainlink_fair_value, external_flow, q5_context | 66.06 | 54.81 | 298.06/232.00 | 1.285 | 225 | 129/96 | 57.33% | 84/141 | 13.55% | 81.3 | 0.4931 | 0.7137 | 1.046 | 50.53 |
| 90-99 | with_rtds_candles | confirmation | chainlink_fair_value, crossvenue | -4.20 | -15.10 | 251.54/255.74 | 0.984 | 218 | 116/102 | 53.21% | 82/136 | 12.24% | 91.1 | 0.5145 | 0.7047 | 1.156 | 53.55 |
| 90-99 | with_rtds_candles | sealed | chainlink_fair_value, crossvenue | 79.00 | 67.40 | 307.49/228.49 | 1.346 | 232 | 141/91 | 60.78% | 90/142 | 13.98% | 91.3 | 0.5185 | 0.7318 | 1.151 | 40.17 |
| 90-99 | without_rtds_candles | confirmation | chainlink_fair_value, crossvenue | -66.50 | -78.90 | 259.35/325.84 | 0.796 | 248 | 121/127 | 48.79% | 78/170 | 13.92% | 91.4 | 0.5201 | 0.7113 | 1.197 | 77.27 |
| 90-99 | without_rtds_candles | sealed | chainlink_fair_value, crossvenue | 96.48 | 84.33 | 327.13/230.64 | 1.418 | 243 | 152/91 | 62.55% | 87/156 | 14.64% | 91.2 | 0.5249 | 0.7402 | 1.178 | 38.07 |
| 100-109 | with_rtds_candles | confirmation | crossvenue, price_control, external_flow | -37.27 | -61.27 | 516.89/554.17 | 0.933 | 480 | 284/196 | 59.17% | 173/307 | 26.95% | 101.2 | 0.5866 | 0.7299 | 1.553 | 90.37 |
| 100-109 | with_rtds_candles | sealed | crossvenue, price_control, external_flow | 106.35 | 86.25 | 492.75/386.40 | 1.275 | 402 | 261/141 | 64.93% | 146/256 | 24.22% | 101.1 | 0.5759 | 0.7433 | 1.452 | 47.46 |
| 100-109 | without_rtds_candles | confirmation | crossvenue, price_control, external_flow | -7.44 | -19.99 | 287.81/295.26 | 0.975 | 251 | 134/117 | 53.39% | 61/190 | 14.09% | 101.3 | 0.5185 | 0.7146 | 1.175 | 40.22 |
| 100-109 | without_rtds_candles | sealed | crossvenue, price_control, external_flow | 102.11 | 89.61 | 337.82/235.72 | 1.433 | 250 | 156/94 | 62.40% | 78/172 | 15.06% | 101.4 | 0.5213 | 0.7413 | 1.158 | 32.52 |
| 110-119 | with_rtds_candles | confirmation | crossvenue, chainlink_fair_value, external_flow | -82.12 | -107.72 | 565.68/647.80 | 0.873 | 512 | 279/233 | 54.49% | 221/291 | 28.75% | 111.2 | 0.5555 | 0.6777 | 1.371 | 97.21 |
| 110-119 | with_rtds_candles | sealed | crossvenue, chainlink_fair_value, external_flow | 84.18 | 64.98 | 495.03/410.84 | 1.205 | 384 | 232/152 | 60.42% | 169/215 | 23.13% | 111.0 | 0.5388 | 0.6973 | 1.267 | 50.50 |
| 110-119 | without_rtds_candles | confirmation | crossvenue, chainlink_fair_value, external_flow | 13.21 | 1.21 | 291.23/278.01 | 1.048 | 240 | 126/114 | 52.50% | 79/161 | 13.48% | 111.1 | 0.4925 | 0.7016 | 1.055 | 33.94 |
| 110-119 | without_rtds_candles | sealed | crossvenue, chainlink_fair_value, external_flow | 95.23 | 83.58 | 326.86/231.63 | 1.411 | 233 | 140/93 | 60.09% | 86/147 | 14.04% | 110.9 | 0.4976 | 0.7327 | 1.067 | 28.46 |
| 120-129 | with_rtds_candles | confirmation | external_flow, chainlink_fair_value, price_control | -67.44 | -99.34 | 645.62/713.06 | 0.905 | 638 | 374/264 | 58.62% | 264/374 | 35.82% | 120.9 | 0.5873 | 0.7301 | 1.565 | 84.33 |
| 120-129 | with_rtds_candles | sealed | external_flow, chainlink_fair_value, price_control | 79.04 | 54.64 | 547.05/468.01 | 1.169 | 488 | 311/177 | 63.73% | 209/279 | 29.40% | 120.9 | 0.5849 | 0.7562 | 1.503 | 43.44 |
| 120-129 | without_rtds_candles | confirmation | external_flow, chainlink_fair_value, price_control | -67.44 | -99.34 | 645.62/713.06 | 0.905 | 638 | 374/264 | 58.62% | 264/374 | 35.82% | 120.9 | 0.5873 | 0.7301 | 1.565 | 84.33 |
| 120-129 | without_rtds_candles | sealed | external_flow, chainlink_fair_value, price_control | 79.04 | 54.64 | 547.05/468.01 | 1.169 | 488 | 311/177 | 63.73% | 209/279 | 29.40% | 120.9 | 0.5849 | 0.7562 | 1.503 | 43.44 |
| 130-139 | with_rtds_candles | confirmation | crossvenue, external_flow, q5_context | -45.12 | -69.12 | 537.70/582.82 | 0.923 | 480 | 261/219 | 54.37% | 189/291 | 26.95% | 131.1 | 0.5411 | 0.6857 | 1.292 | 76.83 |
| 130-139 | with_rtds_candles | sealed | crossvenue, external_flow, q5_context | 76.81 | 57.51 | 492.58/415.78 | 1.185 | 386 | 230/156 | 59.59% | 171/215 | 23.25% | 131.2 | 0.5346 | 0.7102 | 1.244 | 56.24 |
| 130-139 | without_rtds_candles | confirmation | crossvenue, external_flow, q5_context | 11.69 | -6.61 | 433.77/422.08 | 1.028 | 366 | 201/165 | 54.92% | 168/198 | 20.55% | 131.1 | 0.5213 | 0.6958 | 1.185 | 36.10 |
| 130-139 | without_rtds_candles | sealed | crossvenue, external_flow, q5_context | 62.93 | 47.43 | 403.60/340.66 | 1.185 | 310 | 178/132 | 57.42% | 150/160 | 18.67% | 131.2 | 0.5122 | 0.7214 | 1.138 | 62.62 |
| 140-149 | with_rtds_candles | confirmation | crossvenue, chainlink_fair_value | -24.28 | -43.53 | 376.92/401.20 | 0.939 | 385 | 253/132 | 65.71% | 129/256 | 21.62% | 141.2 | 0.6503 | 0.8009 | 2.040 | 67.05 |
| 140-149 | with_rtds_candles | sealed | crossvenue, chainlink_fair_value | 29.52 | 12.92 | 360.33/330.81 | 1.089 | 332 | 229/103 | 68.98% | 111/221 | 20.00% | 141.2 | 0.6527 | 0.8255 | 2.041 | 30.85 |
| 140-149 | without_rtds_candles | confirmation | crossvenue, chainlink_fair_value | -58.73 | -82.28 | 471.79/530.52 | 0.889 | 471 | 281/190 | 59.66% | 157/314 | 26.45% | 141.0 | 0.6016 | 0.7540 | 1.663 | 83.25 |
| 140-149 | without_rtds_candles | sealed | crossvenue, chainlink_fair_value | 82.62 | 63.32 | 462.04/379.43 | 1.218 | 386 | 252/134 | 65.28% | 139/247 | 23.25% | 141.2 | 0.5901 | 0.7740 | 1.544 | 30.85 |
| 150-159 | with_rtds_candles | confirmation | crossvenue, chainlink_fair_value | -41.91 | -55.31 | 292.27/334.18 | 0.875 | 268 | 138/130 | 51.49% | 87/181 | 15.05% | 151.0 | 0.5255 | 0.7419 | 1.214 | 72.75 |
| 150-159 | with_rtds_candles | sealed | crossvenue, chainlink_fair_value | 118.97 | 106.22 | 350.38/231.41 | 1.514 | 255 | 167/88 | 65.49% | 78/177 | 15.36% | 151.1 | 0.5407 | 0.7948 | 1.253 | 18.39 |
| 150-159 | without_rtds_candles | confirmation | crossvenue, chainlink_fair_value | -46.29 | -60.14 | 300.05/346.35 | 0.866 | 277 | 145/132 | 52.35% | 77/200 | 15.55% | 151.1 | 0.5362 | 0.7500 | 1.268 | 60.34 |
| 150-159 | without_rtds_candles | sealed | crossvenue, chainlink_fair_value | 141.79 | 128.94 | 367.77/225.99 | 1.627 | 257 | 170/87 | 66.15% | 86/171 | 15.48% | 151.1 | 0.5303 | 0.7882 | 1.201 | 12.72 |
| 160-169 | with_rtds_candles | confirmation | chainlink_fair_value | -98.34 | -133.59 | 612.96/711.29 | 0.862 | 705 | 469/236 | 66.52% | 302/403 | 39.58% | 160.8 | 0.6748 | 0.7930 | 2.306 | 118.58 |
| 160-169 | with_rtds_candles | sealed | chainlink_fair_value | 145.64 | 119.54 | 568.28/422.63 | 1.345 | 522 | 379/143 | 72.61% | 198/324 | 31.45% | 160.8 | 0.6517 | 0.8120 | 1.971 | 24.49 |
| 160-169 | without_rtds_candles | confirmation | chainlink_fair_value | -82.29 | -93.69 | 222.22/304.51 | 0.730 | 228 | 122/106 | 53.51% | 83/145 | 12.80% | 161.2 | 0.5868 | 0.8013 | 1.577 | 105.92 |
| 160-169 | without_rtds_candles | sealed | chainlink_fair_value | 130.20 | 118.45 | 318.05/187.86 | 1.693 | 235 | 168/67 | 71.49% | 83/152 | 14.16% | 161.1 | 0.5837 | 0.8416 | 1.481 | 13.66 |
| 170-179 | with_rtds_candles | confirmation | chainlink_fair_value | -152.53 | -174.53 | 416.06/568.59 | 0.732 | 440 | 232/208 | 52.73% | 214/226 | 24.71% | 171.1 | 0.5762 | 0.7510 | 1.524 | 155.46 |
| 170-179 | with_rtds_candles | sealed | chainlink_fair_value | 134.43 | 118.23 | 437.62/303.19 | 1.443 | 324 | 215/109 | 66.36% | 131/193 | 19.52% | 171.1 | 0.5600 | 0.7863 | 1.367 | 22.03 |
| 170-179 | without_rtds_candles | confirmation | chainlink_fair_value | -87.62 | -104.62 | 350.62/438.24 | 0.800 | 340 | 167/173 | 49.12% | 158/182 | 19.09% | 170.9 | 0.5214 | 0.7096 | 1.207 | 92.49 |
| 170-179 | without_rtds_candles | sealed | chainlink_fair_value | 109.40 | 96.10 | 374.92/265.53 | 1.412 | 266 | 165/101 | 62.03% | 113/153 | 16.02% | 171.2 | 0.5169 | 0.7688 | 1.157 | 24.56 |
| 180-189 | with_rtds_candles | confirmation | chainlink_fair_value | -37.05 | -52.95 | 295.13/332.18 | 0.888 | 318 | 214/104 | 67.30% | 140/178 | 17.86% | 181.2 | 0.6770 | 0.8304 | 2.316 | 53.67 |
| 180-189 | with_rtds_candles | sealed | chainlink_fair_value | 98.49 | 85.49 | 327.07/228.57 | 1.431 | 260 | 191/69 | 73.46% | 108/152 | 15.66% | 180.9 | 0.6392 | 0.8544 | 1.935 | 25.74 |
| 180-189 | without_rtds_candles | confirmation | chainlink_fair_value | -69.85 | -89.90 | 389.32/459.17 | 0.848 | 401 | 244/157 | 60.85% | 190/211 | 22.52% | 181.1 | 0.6232 | 0.7908 | 1.833 | 101.95 |
| 180-189 | without_rtds_candles | sealed | chainlink_fair_value | 101.17 | 85.47 | 400.04/298.88 | 1.338 | 314 | 216/98 | 68.79% | 134/180 | 18.92% | 181.0 | 0.6033 | 0.8321 | 1.647 | 42.89 |
| 190-199 | with_rtds_candles | confirmation | chainlink_fair_value, price_control | -24.41 | -32.21 | 155.77/180.18 | 0.865 | 156 | 95/61 | 60.90% | 62/94 | 8.76% | 191.2 | 0.6199 | 0.8417 | 1.801 | 42.28 |
| 190-199 | with_rtds_candles | sealed | chainlink_fair_value, price_control | 142.64 | 133.84 | 273.68/131.04 | 2.088 | 176 | 133/43 | 75.57% | 61/115 | 10.60% | 191.4 | 0.5731 | 0.8793 | 1.481 | 17.02 |
| 190-199 | without_rtds_candles | confirmation | chainlink_fair_value, price_control | -29.05 | -37.45 | 126.18/155.23 | 0.813 | 168 | 125/43 | 74.40% | 67/101 | 9.43% | 191.4 | 0.7619 | 0.9110 | 3.576 | 40.44 |
| 190-199 | without_rtds_candles | sealed | chainlink_fair_value, price_control | 119.56 | 110.31 | 236.47/116.91 | 2.023 | 185 | 151/34 | 81.62% | 64/121 | 11.14% | 190.9 | 0.6688 | 0.9236 | 2.196 | 17.04 |
| 200-209 | with_rtds_candles | confirmation | chainlink_fair_value | 23.72 | 10.42 | 246.44/222.72 | 1.107 | 266 | 194/72 | 72.93% | 99/167 | 14.94% | 201.0 | 0.6932 | 0.8672 | 2.435 | 36.15 |
| 200-209 | with_rtds_candles | sealed | chainlink_fair_value | 196.82 | 183.32 | 355.50/158.68 | 2.240 | 270 | 219/51 | 81.11% | 106/164 | 16.27% | 200.9 | 0.6466 | 0.8875 | 1.917 | 16.26 |
| 200-209 | without_rtds_candles | confirmation | chainlink_fair_value | 2.96 | -15.54 | 299.49/296.53 | 1.010 | 370 | 279/91 | 75.41% | 156/214 | 20.77% | 201.0 | 0.7357 | 0.8763 | 3.036 | 39.59 |
| 200-209 | without_rtds_candles | sealed | chainlink_fair_value | 165.46 | 148.16 | 394.05/228.59 | 1.724 | 346 | 276/70 | 79.77% | 145/201 | 20.84% | 201.0 | 0.6845 | 0.8918 | 2.287 | 29.57 |
| 210-219 | with_rtds_candles | confirmation | chainlink_fair_value | -58.33 | -72.38 | 276.56/334.89 | 0.826 | 281 | 144/137 | 51.25% | 128/153 | 15.78% | 211.3 | 0.5339 | 0.7708 | 1.273 | 77.34 |
| 210-219 | with_rtds_candles | sealed | chainlink_fair_value | 237.15 | 223.15 | 421.46/184.31 | 2.287 | 280 | 196/84 | 70.00% | 115/165 | 16.87% | 211.1 | 0.5106 | 0.8182 | 1.020 | 12.52 |
| 210-219 | without_rtds_candles | confirmation | chainlink_fair_value | -13.08 | -23.78 | 238.52/251.60 | 0.948 | 214 | 118/96 | 55.14% | 104/110 | 12.02% | 211.2 | 0.5430 | 0.8025 | 1.297 | 46.53 |
| 210-219 | without_rtds_candles | sealed | chainlink_fair_value | 225.95 | 214.20 | 376.22/150.27 | 2.504 | 235 | 172/63 | 73.19% | 101/134 | 14.16% | 210.9 | 0.5193 | 0.8500 | 1.090 | 11.06 |
| 220-229 | with_rtds_candles | confirmation | chainlink_fair_value | -20.95 | -38.85 | 348.29/369.23 | 0.943 | 358 | 209/149 | 58.38% | 159/199 | 20.10% | 220.8 | 0.5765 | 0.7790 | 1.487 | 63.19 |
| 220-229 | with_rtds_candles | sealed | chainlink_fair_value | 214.46 | 198.26 | 435.58/221.12 | 1.970 | 324 | 233/91 | 71.91% | 135/189 | 19.52% | 221.2 | 0.5679 | 0.8322 | 1.300 | 17.31 |
| 220-229 | without_rtds_candles | confirmation | chainlink_fair_value | -64.22 | -90.47 | 437.73/501.95 | 0.872 | 525 | 340/185 | 64.76% | 230/295 | 29.48% | 220.7 | 0.6549 | 0.8135 | 2.107 | 95.24 |
| 220-229 | without_rtds_candles | sealed | chainlink_fair_value | 215.95 | 193.85 | 500.77/284.83 | 1.758 | 442 | 330/112 | 74.66% | 189/253 | 26.63% | 221.0 | 0.6315 | 0.8449 | 1.676 | 21.11 |
| 230-239 | with_rtds_candles | confirmation | chainlink_fair_value | -42.39 | -67.14 | 376.21/418.60 | 0.899 | 495 | 338/157 | 68.28% | 221/274 | 27.79% | 230.9 | 0.6834 | 0.8304 | 2.395 | 76.27 |
| 230-239 | with_rtds_candles | sealed | chainlink_fair_value | 255.97 | 235.92 | 477.61/221.63 | 2.155 | 401 | 308/93 | 76.81% | 166/235 | 24.16% | 230.8 | 0.6229 | 0.8468 | 1.537 | 18.39 |
| 230-239 | without_rtds_candles | confirmation | chainlink_fair_value | -9.78 | -33.98 | 388.10/397.88 | 0.975 | 484 | 325/159 | 67.15% | 204/280 | 27.18% | 230.8 | 0.6589 | 0.8213 | 2.096 | 59.16 |
| 230-239 | without_rtds_candles | sealed | chainlink_fair_value | 248.06 | 228.01 | 481.70/233.64 | 2.062 | 401 | 298/103 | 74.31% | 162/239 | 24.16% | 230.9 | 0.6018 | 0.8448 | 1.403 | 15.93 |

## aggregate_diverse: per-bucket PnL

| Seconds | RTDS | Period | Donors | PnL | Stress | Gross +/− | PF | Trades | W/L | Win rate | UP/DOWN | Coverage | Avg entry | Avg cost | Avg confidence | Recovery | Max DD |
|---:|---|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 60-69 | with_rtds_candles | confirmation | chainlink_fair_value, q5_context, crossvenue | 12.52 | -8.83 | 484.09/471.57 | 1.027 | 427 | 249/178 | 58.31% | 97/330 | 23.98% | 61.3 | 0.5561 | 0.6905 | 1.363 | 60.93 |
| 60-69 | with_rtds_candles | sealed | chainlink_fair_value, q5_context, crossvenue | 98.60 | 80.45 | 449.75/351.15 | 1.281 | 363 | 229/134 | 63.09% | 115/248 | 21.87% | 61.4 | 0.5554 | 0.7020 | 1.334 | 22.82 |
| 60-69 | without_rtds_candles | confirmation | chainlink_fair_value, q5_context, crossvenue | -18.81 | -40.66 | 487.78/506.59 | 0.963 | 437 | 245/192 | 56.06% | 104/333 | 24.54% | 61.2 | 0.5480 | 0.6869 | 1.325 | 57.25 |
| 60-69 | without_rtds_candles | sealed | chainlink_fair_value, q5_context, crossvenue | 30.79 | 12.54 | 435.38/404.60 | 1.076 | 365 | 212/153 | 58.08% | 114/251 | 21.99% | 61.3 | 0.5427 | 0.6975 | 1.288 | 44.46 |
| 70-79 | with_rtds_candles | confirmation | q5_context, price_control, chainlink_fair_value | 0.40 | -12.70 | 290.69/290.29 | 1.001 | 262 | 165/97 | 62.98% | 62/200 | 14.71% | 71.0 | 0.6086 | 0.7509 | 1.699 | 38.01 |
| 70-79 | with_rtds_candles | sealed | q5_context, price_control, chainlink_fair_value | 92.86 | 80.21 | 315.26/222.40 | 1.418 | 253 | 175/78 | 69.17% | 83/170 | 15.24% | 71.2 | 0.5975 | 0.7609 | 1.583 | 29.19 |
| 70-79 | without_rtds_candles | confirmation | q5_context, price_control, chainlink_fair_value | 31.68 | 17.83 | 315.18/283.51 | 1.112 | 277 | 181/96 | 65.34% | 59/218 | 15.55% | 71.1 | 0.6097 | 0.7541 | 1.696 | 40.95 |
| 70-79 | without_rtds_candles | sealed | q5_context, price_control, chainlink_fair_value | 65.83 | 52.73 | 316.77/250.94 | 1.262 | 262 | 173/89 | 66.03% | 89/173 | 15.78% | 71.1 | 0.5892 | 0.7604 | 1.540 | 27.27 |
| 80-89 | with_rtds_candles | confirmation | chainlink_fair_value, external_flow, q5_context | -30.78 | -39.83 | 200.44/231.22 | 0.867 | 181 | 86/95 | 47.51% | 51/130 | 10.16% | 81.2 | 0.4876 | 0.6750 | 1.044 | 49.14 |
| 80-89 | with_rtds_candles | sealed | chainlink_fair_value, external_flow, q5_context | 78.16 | 68.21 | 269.29/191.13 | 1.409 | 199 | 119/80 | 59.80% | 70/129 | 11.99% | 81.3 | 0.4981 | 0.7143 | 1.056 | 35.77 |
| 80-89 | without_rtds_candles | confirmation | chainlink_fair_value, external_flow, q5_context | -19.48 | -38.38 | 427.89/447.38 | 0.956 | 378 | 211/167 | 55.82% | 112/266 | 21.22% | 81.0 | 0.5473 | 0.6896 | 1.321 | 65.22 |
| 80-89 | without_rtds_candles | sealed | chainlink_fair_value, external_flow, q5_context | 95.33 | 77.83 | 441.83/346.50 | 1.275 | 350 | 217/133 | 62.00% | 143/207 | 21.08% | 81.3 | 0.5444 | 0.7129 | 1.280 | 38.37 |
| 90-99 | with_rtds_candles | confirmation | chainlink_fair_value, crossvenue | -28.48 | -39.13 | 237.16/265.63 | 0.893 | 213 | 108/105 | 50.70% | 67/146 | 11.96% | 91.3 | 0.5123 | 0.7040 | 1.152 | 66.67 |
| 90-99 | with_rtds_candles | sealed | chainlink_fair_value, crossvenue | 83.55 | 72.45 | 297.28/213.72 | 1.391 | 222 | 138/84 | 62.16% | 74/148 | 13.37% | 91.1 | 0.5251 | 0.7385 | 1.181 | 38.96 |
| 90-99 | without_rtds_candles | confirmation | chainlink_fair_value, crossvenue | -22.26 | -25.06 | 55.77/78.02 | 0.715 | 56 | 23/33 | 41.07% | 56/0 | 3.14% | 91.4 | 0.4684 | 0.6691 | 0.975 | 40.21 |
| 90-99 | without_rtds_candles | sealed | chainlink_fair_value, crossvenue | 42.62 | 39.02 | 109.34/66.72 | 1.639 | 72 | 44/28 | 61.11% | 72/0 | 4.34% | 91.3 | 0.4714 | 0.7027 | 0.959 | 9.57 |
| 100-109 | with_rtds_candles | confirmation | crossvenue, price_control, external_flow | -41.27 | -69.97 | 590.38/631.65 | 0.935 | 574 | 344/230 | 59.93% | 202/372 | 32.23% | 100.9 | 0.5934 | 0.7197 | 1.600 | 50.92 |
| 100-109 | with_rtds_candles | sealed | crossvenue, price_control, external_flow | 86.41 | 63.71 | 533.90/447.49 | 1.193 | 454 | 291/163 | 64.10% | 190/264 | 27.35% | 101.0 | 0.5826 | 0.7373 | 1.496 | 79.38 |
| 100-109 | without_rtds_candles | confirmation | crossvenue, price_control, external_flow | -50.28 | -78.53 | 574.66/624.94 | 0.920 | 565 | 337/228 | 59.65% | 181/384 | 31.72% | 100.9 | 0.5940 | 0.7224 | 1.607 | 55.12 |
| 100-109 | without_rtds_candles | sealed | crossvenue, price_control, external_flow | 96.02 | 73.52 | 537.03/441.01 | 1.218 | 450 | 289/161 | 64.22% | 175/275 | 27.11% | 100.9 | 0.5793 | 0.7380 | 1.474 | 76.21 |
| 110-119 | with_rtds_candles | confirmation | crossvenue, chainlink_fair_value, external_flow | -63.99 | -93.39 | 601.44/665.43 | 0.904 | 588 | 358/230 | 60.88% | 228/360 | 33.02% | 110.8 | 0.6106 | 0.7265 | 1.722 | 78.89 |
| 110-119 | with_rtds_candles | sealed | crossvenue, chainlink_fair_value, external_flow | 104.56 | 81.26 | 543.90/439.34 | 1.238 | 466 | 309/157 | 66.31% | 185/281 | 28.07% | 110.8 | 0.5981 | 0.7458 | 1.590 | 50.57 |
| 110-119 | without_rtds_candles | confirmation | crossvenue, chainlink_fair_value, external_flow | 34.70 | 22.95 | 294.67/259.97 | 1.133 | 235 | 126/109 | 53.62% | 85/150 | 13.19% | 111.0 | 0.4852 | 0.6939 | 1.020 | 33.83 |
| 110-119 | without_rtds_candles | sealed | crossvenue, chainlink_fair_value, external_flow | 100.06 | 89.21 | 311.66/211.60 | 1.473 | 217 | 131/86 | 60.37% | 83/134 | 13.07% | 111.0 | 0.4900 | 0.7272 | 1.034 | 26.16 |
| 120-129 | with_rtds_candles | confirmation | external_flow, chainlink_fair_value, price_control | -45.69 | -58.64 | 271.92/317.62 | 0.856 | 259 | 123/136 | 47.49% | 118/141 | 14.54% | 121.1 | 0.4891 | 0.7023 | 1.056 | 52.64 |
| 120-129 | with_rtds_candles | sealed | external_flow, chainlink_fair_value, price_control | 79.65 | 67.05 | 328.80/249.15 | 1.320 | 252 | 150/102 | 59.52% | 106/146 | 15.18% | 121.1 | 0.5109 | 0.7458 | 1.114 | 24.56 |
| 120-129 | without_rtds_candles | confirmation | external_flow, chainlink_fair_value, price_control | -37.52 | -62.72 | 573.74/611.26 | 0.939 | 504 | 273/231 | 54.17% | 240/264 | 28.30% | 121.0 | 0.5350 | 0.6735 | 1.259 | 58.39 |
| 120-129 | without_rtds_candles | sealed | external_flow, chainlink_fair_value, price_control | 29.24 | 10.34 | 462.07/432.83 | 1.068 | 378 | 212/166 | 56.08% | 179/199 | 22.77% | 121.0 | 0.5239 | 0.6982 | 1.196 | 45.76 |
| 130-139 | with_rtds_candles | confirmation | crossvenue, external_flow, q5_context | 0.71 | -12.69 | 306.10/305.39 | 1.002 | 268 | 144/124 | 53.73% | 97/171 | 15.05% | 131.1 | 0.5158 | 0.7226 | 1.159 | 37.38 |
| 130-139 | with_rtds_candles | sealed | crossvenue, external_flow, q5_context | 94.03 | 81.88 | 325.25/231.22 | 1.407 | 243 | 149/94 | 61.32% | 97/146 | 14.64% | 131.1 | 0.5147 | 0.7561 | 1.127 | 32.14 |
| 130-139 | without_rtds_candles | confirmation | crossvenue, external_flow, q5_context | -7.08 | -20.53 | 306.36/313.44 | 0.977 | 269 | 143/126 | 53.16% | 105/164 | 15.10% | 131.0 | 0.5158 | 0.7242 | 1.161 | 47.82 |
| 130-139 | without_rtds_candles | sealed | crossvenue, external_flow, q5_context | 125.53 | 112.23 | 368.38/242.85 | 1.517 | 266 | 168/98 | 63.16% | 117/149 | 16.02% | 131.3 | 0.5161 | 0.7532 | 1.130 | 25.66 |
| 140-149 | with_rtds_candles | confirmation | crossvenue, chainlink_fair_value | 0.42 | -12.58 | 238.04/237.61 | 1.002 | 260 | 191/69 | 73.46% | 89/171 | 14.60% | 141.2 | 0.7160 | 0.8553 | 2.763 | 41.67 |
| 140-149 | with_rtds_candles | sealed | crossvenue, chainlink_fair_value | 25.76 | 13.21 | 262.59/236.83 | 1.109 | 251 | 183/68 | 72.91% | 84/167 | 15.12% | 141.3 | 0.6899 | 0.8666 | 2.427 | 35.61 |
| 140-149 | without_rtds_candles | confirmation | crossvenue, chainlink_fair_value | -64.94 | -78.44 | 277.21/342.15 | 0.810 | 270 | 136/134 | 50.37% | 99/171 | 15.16% | 141.1 | 0.5309 | 0.7329 | 1.253 | 80.20 |
| 140-149 | without_rtds_candles | sealed | crossvenue, chainlink_fair_value | 98.05 | 85.35 | 342.78/244.73 | 1.401 | 254 | 159/95 | 62.60% | 100/154 | 15.30% | 141.3 | 0.5280 | 0.7640 | 1.195 | 16.79 |
| 150-159 | with_rtds_candles | confirmation | crossvenue, chainlink_fair_value | -69.20 | -90.90 | 453.94/523.14 | 0.868 | 434 | 245/189 | 56.45% | 184/250 | 24.37% | 151.0 | 0.5758 | 0.7366 | 1.494 | 94.25 |
| 150-159 | with_rtds_candles | sealed | crossvenue, chainlink_fair_value | 140.10 | 122.95 | 457.71/317.61 | 1.441 | 343 | 228/115 | 66.47% | 130/213 | 20.66% | 151.0 | 0.5622 | 0.7705 | 1.376 | 22.19 |
| 150-159 | without_rtds_candles | confirmation | crossvenue, chainlink_fair_value | -42.21 | -55.66 | 293.00/335.21 | 0.874 | 269 | 139/130 | 51.67% | 96/173 | 15.10% | 150.9 | 0.5275 | 0.7427 | 1.223 | 65.11 |
| 150-159 | without_rtds_candles | sealed | crossvenue, chainlink_fair_value | 157.08 | 144.08 | 368.73/211.65 | 1.742 | 260 | 177/83 | 68.08% | 94/166 | 15.66% | 151.0 | 0.5393 | 0.7937 | 1.224 | 15.47 |
| 160-169 | with_rtds_candles | confirmation | chainlink_fair_value | -98.34 | -133.59 | 612.96/711.29 | 0.862 | 705 | 469/236 | 66.52% | 302/403 | 39.58% | 160.8 | 0.6748 | 0.7930 | 2.306 | 118.58 |
| 160-169 | with_rtds_candles | sealed | chainlink_fair_value | 145.64 | 119.54 | 568.28/422.63 | 1.345 | 522 | 379/143 | 72.61% | 198/324 | 31.45% | 160.8 | 0.6517 | 0.8120 | 1.971 | 24.49 |
| 160-169 | without_rtds_candles | confirmation | chainlink_fair_value | -82.29 | -93.69 | 222.22/304.51 | 0.730 | 228 | 122/106 | 53.51% | 83/145 | 12.80% | 161.2 | 0.5868 | 0.8013 | 1.577 | 105.92 |
| 160-169 | without_rtds_candles | sealed | chainlink_fair_value | 130.20 | 118.45 | 318.05/187.86 | 1.693 | 235 | 168/67 | 71.49% | 83/152 | 14.16% | 161.1 | 0.5837 | 0.8416 | 1.481 | 13.66 |
| 170-179 | with_rtds_candles | confirmation | chainlink_fair_value | -152.53 | -174.53 | 416.06/568.59 | 0.732 | 440 | 232/208 | 52.73% | 214/226 | 24.71% | 171.1 | 0.5762 | 0.7510 | 1.524 | 155.46 |
| 170-179 | with_rtds_candles | sealed | chainlink_fair_value | 134.43 | 118.23 | 437.62/303.19 | 1.443 | 324 | 215/109 | 66.36% | 131/193 | 19.52% | 171.1 | 0.5600 | 0.7863 | 1.367 | 22.03 |
| 170-179 | without_rtds_candles | confirmation | chainlink_fair_value | -87.62 | -104.62 | 350.62/438.24 | 0.800 | 340 | 167/173 | 49.12% | 158/182 | 19.09% | 170.9 | 0.5214 | 0.7096 | 1.207 | 92.49 |
| 170-179 | without_rtds_candles | sealed | chainlink_fair_value | 109.40 | 96.10 | 374.92/265.53 | 1.412 | 266 | 165/101 | 62.03% | 113/153 | 16.02% | 171.2 | 0.5169 | 0.7688 | 1.157 | 24.56 |
| 180-189 | with_rtds_candles | confirmation | chainlink_fair_value | -37.05 | -52.95 | 295.13/332.18 | 0.888 | 318 | 214/104 | 67.30% | 140/178 | 17.86% | 181.2 | 0.6770 | 0.8304 | 2.316 | 53.67 |
| 180-189 | with_rtds_candles | sealed | chainlink_fair_value | 98.49 | 85.49 | 327.07/228.57 | 1.431 | 260 | 191/69 | 73.46% | 108/152 | 15.66% | 180.9 | 0.6392 | 0.8544 | 1.935 | 25.74 |
| 180-189 | without_rtds_candles | confirmation | chainlink_fair_value | -69.85 | -89.90 | 389.32/459.17 | 0.848 | 401 | 244/157 | 60.85% | 190/211 | 22.52% | 181.1 | 0.6232 | 0.7908 | 1.833 | 101.95 |
| 180-189 | without_rtds_candles | sealed | chainlink_fair_value | 101.17 | 85.47 | 400.04/298.88 | 1.338 | 314 | 216/98 | 68.79% | 134/180 | 18.92% | 181.0 | 0.6033 | 0.8321 | 1.647 | 42.89 |
| 190-199 | with_rtds_candles | confirmation | chainlink_fair_value, price_control | -56.34 | -72.99 | 251.06/307.41 | 0.817 | 333 | 243/90 | 72.97% | 123/210 | 18.70% | 190.9 | 0.7466 | 0.8668 | 3.306 | 85.64 |
| 190-199 | with_rtds_candles | sealed | chainlink_fair_value, price_control | 154.48 | 139.78 | 338.99/184.51 | 1.837 | 294 | 236/58 | 80.27% | 108/186 | 17.71% | 191.1 | 0.6799 | 0.8850 | 2.215 | 23.47 |
| 190-199 | without_rtds_candles | confirmation | chainlink_fair_value, price_control | -73.13 | -88.28 | 239.34/312.48 | 0.766 | 303 | 207/96 | 68.32% | 114/189 | 17.01% | 190.8 | 0.7138 | 0.8602 | 2.815 | 90.01 |
| 190-199 | without_rtds_candles | sealed | chainlink_fair_value, price_control | 147.64 | 133.59 | 342.60/194.96 | 1.757 | 281 | 219/62 | 77.94% | 112/169 | 16.93% | 191.0 | 0.6560 | 0.8791 | 2.010 | 20.60 |
| 200-209 | with_rtds_candles | confirmation | chainlink_fair_value | 23.72 | 10.42 | 246.44/222.72 | 1.107 | 266 | 194/72 | 72.93% | 99/167 | 14.94% | 201.0 | 0.6932 | 0.8672 | 2.435 | 36.15 |
| 200-209 | with_rtds_candles | sealed | chainlink_fair_value | 196.82 | 183.32 | 355.50/158.68 | 2.240 | 270 | 219/51 | 81.11% | 106/164 | 16.27% | 200.9 | 0.6466 | 0.8875 | 1.917 | 16.26 |
| 200-209 | without_rtds_candles | confirmation | chainlink_fair_value | 2.96 | -15.54 | 299.49/296.53 | 1.010 | 370 | 279/91 | 75.41% | 156/214 | 20.77% | 201.0 | 0.7357 | 0.8763 | 3.036 | 39.59 |
| 200-209 | without_rtds_candles | sealed | chainlink_fair_value | 165.46 | 148.16 | 394.05/228.59 | 1.724 | 346 | 276/70 | 79.77% | 145/201 | 20.84% | 201.0 | 0.6845 | 0.8918 | 2.287 | 29.57 |
| 210-219 | with_rtds_candles | confirmation | chainlink_fair_value | -58.33 | -72.38 | 276.56/334.89 | 0.826 | 281 | 144/137 | 51.25% | 128/153 | 15.78% | 211.3 | 0.5339 | 0.7708 | 1.273 | 77.34 |
| 210-219 | with_rtds_candles | sealed | chainlink_fair_value | 237.15 | 223.15 | 421.46/184.31 | 2.287 | 280 | 196/84 | 70.00% | 115/165 | 16.87% | 211.1 | 0.5106 | 0.8182 | 1.020 | 12.52 |
| 210-219 | without_rtds_candles | confirmation | chainlink_fair_value | -13.08 | -23.78 | 238.52/251.60 | 0.948 | 214 | 118/96 | 55.14% | 104/110 | 12.02% | 211.2 | 0.5430 | 0.8025 | 1.297 | 46.53 |
| 210-219 | without_rtds_candles | sealed | chainlink_fair_value | 225.95 | 214.20 | 376.22/150.27 | 2.504 | 235 | 172/63 | 73.19% | 101/134 | 14.16% | 210.9 | 0.5193 | 0.8500 | 1.090 | 11.06 |
| 220-229 | with_rtds_candles | confirmation | chainlink_fair_value | -20.95 | -38.85 | 348.29/369.23 | 0.943 | 358 | 209/149 | 58.38% | 159/199 | 20.10% | 220.8 | 0.5765 | 0.7790 | 1.487 | 63.19 |
| 220-229 | with_rtds_candles | sealed | chainlink_fair_value | 214.46 | 198.26 | 435.58/221.12 | 1.970 | 324 | 233/91 | 71.91% | 135/189 | 19.52% | 221.2 | 0.5679 | 0.8322 | 1.300 | 17.31 |
| 220-229 | without_rtds_candles | confirmation | chainlink_fair_value | -64.22 | -90.47 | 437.73/501.95 | 0.872 | 525 | 340/185 | 64.76% | 230/295 | 29.48% | 220.7 | 0.6549 | 0.8135 | 2.107 | 95.24 |
| 220-229 | without_rtds_candles | sealed | chainlink_fair_value | 215.95 | 193.85 | 500.77/284.83 | 1.758 | 442 | 330/112 | 74.66% | 189/253 | 26.63% | 221.0 | 0.6315 | 0.8449 | 1.676 | 21.11 |
| 230-239 | with_rtds_candles | confirmation | chainlink_fair_value | -42.39 | -67.14 | 376.21/418.60 | 0.899 | 495 | 338/157 | 68.28% | 221/274 | 27.79% | 230.9 | 0.6834 | 0.8304 | 2.395 | 76.27 |
| 230-239 | with_rtds_candles | sealed | chainlink_fair_value | 255.97 | 235.92 | 477.61/221.63 | 2.155 | 401 | 308/93 | 76.81% | 166/235 | 24.16% | 230.8 | 0.6229 | 0.8468 | 1.537 | 18.39 |
| 230-239 | without_rtds_candles | confirmation | chainlink_fair_value | -9.78 | -33.98 | 388.10/397.88 | 0.975 | 484 | 325/159 | 67.15% | 204/280 | 27.18% | 230.8 | 0.6589 | 0.8213 | 2.096 | 59.16 |
| 230-239 | without_rtds_candles | sealed | chainlink_fair_value | 248.06 | 228.01 | 481.70/233.64 | 2.062 | 401 | 298/103 | 74.31% | 162/239 | 24.16% | 230.9 | 0.6018 | 0.8448 | 1.403 | 15.93 |

## aggregate_side_split: per-bucket PnL

| Seconds | RTDS | Period | Donors | PnL | Stress | Gross +/− | PF | Trades | W/L | Win rate | UP/DOWN | Coverage | Avg entry | Avg cost | Avg confidence | Recovery | Max DD |
|---:|---|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 60-69 | with_rtds_candles | confirmation | chainlink_fair_value, q5_context, crossvenue | -0.59 | -22.59 | 492.57/493.16 | 0.999 | 440 | 253/187 | 57.50% | 133/307 | 24.71% | 61.2 | 0.5540 | 0.6884 | 1.355 | 60.88 |
| 60-69 | with_rtds_candles | sealed | chainlink_fair_value, q5_context, crossvenue | 103.79 | 85.04 | 467.47/363.68 | 1.285 | 375 | 238/137 | 63.47% | 132/243 | 22.59% | 61.4 | 0.5582 | 0.7039 | 1.352 | 30.96 |
| 60-69 | without_rtds_candles | confirmation | chainlink_fair_value, q5_context, crossvenue | 10.39 | -6.21 | 379.11/368.72 | 1.028 | 332 | 192/140 | 57.83% | 0/332 | 18.64% | 61.1 | 0.5508 | 0.6951 | 1.334 | 41.93 |
| 60-69 | without_rtds_candles | sealed | chainlink_fair_value, q5_context, crossvenue | 53.72 | 41.02 | 311.09/257.37 | 1.209 | 254 | 155/99 | 61.02% | 0/254 | 15.30% | 61.4 | 0.5468 | 0.7032 | 1.295 | 32.56 |
| 70-79 | with_rtds_candles | confirmation | q5_context, price_control, chainlink_fair_value | -38.30 | -54.45 | 350.36/388.66 | 0.901 | 323 | 175/148 | 54.18% | 0/323 | 18.14% | 70.9 | 0.5443 | 0.7048 | 1.312 | 72.38 |
| 70-79 | with_rtds_candles | sealed | q5_context, price_control, chainlink_fair_value | 59.30 | 47.15 | 293.70/234.40 | 1.253 | 243 | 145/98 | 59.67% | 0/243 | 14.64% | 71.0 | 0.5267 | 0.7098 | 1.181 | 18.48 |
| 70-79 | without_rtds_candles | confirmation | q5_context, price_control, chainlink_fair_value | -6.90 | -10.00 | 48.37/55.27 | 0.875 | 62 | 47/15 | 75.81% | 12/50 | 3.48% | 71.0 | 0.7634 | 0.8868 | 3.580 | 22.41 |
| 70-79 | without_rtds_candles | sealed | q5_context, price_control, chainlink_fair_value | 32.73 | 29.13 | 76.84/44.11 | 1.742 | 72 | 59/13 | 81.94% | 23/49 | 4.34% | 71.4 | 0.7106 | 0.8816 | 2.605 | 13.13 |
| 80-89 | with_rtds_candles | confirmation | chainlink_fair_value, external_flow, q5_context | -61.27 | -74.22 | 271.05/332.32 | 0.816 | 259 | 117/142 | 45.17% | 82/177 | 14.54% | 81.3 | 0.4778 | 0.6790 | 1.010 | 74.03 |
| 80-89 | with_rtds_candles | sealed | chainlink_fair_value, external_flow, q5_context | 61.08 | 48.88 | 312.28/251.20 | 1.243 | 244 | 137/107 | 56.15% | 89/155 | 14.70% | 81.2 | 0.4903 | 0.7142 | 1.030 | 35.35 |
| 80-89 | without_rtds_candles | confirmation | chainlink_fair_value, external_flow, q5_context | -47.56 | -60.66 | 281.07/328.62 | 0.855 | 262 | 120/142 | 45.80% | 80/182 | 14.71% | 81.2 | 0.4730 | 0.6743 | 0.988 | 61.99 |
| 80-89 | without_rtds_candles | sealed | chainlink_fair_value, external_flow, q5_context | 51.12 | 38.67 | 316.37/265.26 | 1.193 | 249 | 138/111 | 55.42% | 95/154 | 15.00% | 81.1 | 0.4920 | 0.7166 | 1.042 | 45.60 |
| 90-99 | with_rtds_candles | confirmation | chainlink_fair_value, crossvenue | -8.74 | -20.74 | 282.94/291.68 | 0.970 | 240 | 123/117 | 51.25% | 75/165 | 13.48% | 91.2 | 0.4981 | 0.6930 | 1.084 | 74.07 |
| 90-99 | with_rtds_candles | sealed | chainlink_fair_value, crossvenue | 61.98 | 51.53 | 280.13/218.15 | 1.284 | 209 | 121/88 | 57.89% | 76/133 | 12.59% | 91.0 | 0.4981 | 0.7247 | 1.071 | 43.42 |
| 90-99 | without_rtds_candles | confirmation | chainlink_fair_value, crossvenue | -9.73 | -18.33 | 196.33/206.06 | 0.953 | 172 | 100/72 | 58.14% | 49/123 | 9.66% | 91.2 | 0.5714 | 0.7652 | 1.458 | 34.92 |
| 90-99 | without_rtds_candles | sealed | chainlink_fair_value, crossvenue | 93.86 | 84.46 | 258.26/164.40 | 1.571 | 188 | 130/58 | 69.15% | 69/119 | 11.33% | 91.1 | 0.5706 | 0.7939 | 1.427 | 20.40 |
| 100-109 | with_rtds_candles | confirmation | crossvenue, price_control, external_flow | -33.50 | -57.50 | 523.12/556.62 | 0.940 | 480 | 268/212 | 55.83% | 181/299 | 26.95% | 101.1 | 0.5514 | 0.7157 | 1.345 | 86.61 |
| 100-109 | with_rtds_candles | sealed | crossvenue, price_control, external_flow | 59.37 | 39.77 | 465.48/406.11 | 1.146 | 392 | 236/156 | 60.20% | 157/235 | 23.61% | 101.1 | 0.5512 | 0.7399 | 1.320 | 72.22 |
| 100-109 | without_rtds_candles | confirmation | crossvenue, price_control, external_flow | -4.64 | -24.89 | 458.92/463.56 | 0.990 | 405 | 223/182 | 55.06% | 112/293 | 22.74% | 101.2 | 0.5317 | 0.7061 | 1.238 | 61.04 |
| 100-109 | without_rtds_candles | sealed | crossvenue, price_control, external_flow | 71.46 | 54.06 | 431.80/360.35 | 1.198 | 348 | 205/143 | 58.91% | 131/217 | 20.96% | 101.0 | 0.5271 | 0.7257 | 1.196 | 60.71 |
| 110-119 | with_rtds_candles | confirmation | crossvenue, chainlink_fair_value, external_flow | -68.14 | -93.59 | 565.58/633.72 | 0.892 | 509 | 279/230 | 54.81% | 221/288 | 28.58% | 111.1 | 0.5534 | 0.6736 | 1.359 | 93.38 |
| 110-119 | with_rtds_candles | sealed | crossvenue, chainlink_fair_value, external_flow | 93.64 | 74.79 | 493.48/399.85 | 1.234 | 377 | 230/147 | 61.01% | 170/207 | 22.71% | 111.0 | 0.5389 | 0.6940 | 1.268 | 55.40 |
| 110-119 | without_rtds_candles | confirmation | crossvenue, chainlink_fair_value, external_flow | 15.53 | 4.08 | 277.13/261.60 | 1.059 | 229 | 120/109 | 52.40% | 79/150 | 12.86% | 111.1 | 0.4890 | 0.7000 | 1.039 | 30.44 |
| 110-119 | without_rtds_candles | sealed | crossvenue, chainlink_fair_value, external_flow | 85.63 | 74.33 | 315.78/230.15 | 1.372 | 226 | 134/92 | 59.29% | 86/140 | 13.61% | 110.9 | 0.4957 | 0.7320 | 1.062 | 24.40 |
| 120-129 | with_rtds_candles | confirmation | external_flow, chainlink_fair_value, price_control | -44.82 | -70.02 | 572.09/616.91 | 0.927 | 504 | 274/230 | 54.37% | 214/290 | 28.30% | 121.0 | 0.5399 | 0.6822 | 1.285 | 72.54 |
| 120-129 | with_rtds_candles | sealed | external_flow, chainlink_fair_value, price_control | 71.77 | 53.62 | 467.95/396.18 | 1.181 | 363 | 214/149 | 58.95% | 164/199 | 21.87% | 121.0 | 0.5285 | 0.7075 | 1.216 | 34.40 |
| 120-129 | without_rtds_candles | confirmation | external_flow, chainlink_fair_value, price_control | -38.39 | -63.44 | 572.57/610.96 | 0.937 | 501 | 274/227 | 54.69% | 219/282 | 28.13% | 120.9 | 0.5407 | 0.6847 | 1.288 | 62.48 |
| 120-129 | without_rtds_candles | sealed | external_flow, chainlink_fair_value, price_control | 64.83 | 46.43 | 469.77/404.94 | 1.160 | 368 | 214/154 | 58.15% | 165/203 | 22.17% | 121.1 | 0.5248 | 0.7088 | 1.198 | 34.66 |
| 130-139 | with_rtds_candles | confirmation | crossvenue, external_flow, q5_context | -115.22 | -154.62 | 718.36/833.58 | 0.862 | 788 | 499/289 | 63.32% | 327/461 | 44.24% | 130.7 | 0.6436 | 0.7636 | 2.004 | 148.87 |
| 130-139 | with_rtds_candles | sealed | crossvenue, external_flow, q5_context | 69.79 | 40.24 | 623.31/553.52 | 1.126 | 591 | 395/196 | 66.84% | 257/334 | 35.60% | 130.9 | 0.6256 | 0.7725 | 1.790 | 77.01 |
| 130-139 | without_rtds_candles | confirmation | crossvenue, external_flow, q5_context | -46.47 | -80.62 | 688.74/735.21 | 0.937 | 683 | 418/265 | 61.20% | 280/403 | 38.35% | 130.9 | 0.6059 | 0.7447 | 1.684 | 86.11 |
| 130-139 | without_rtds_candles | sealed | crossvenue, external_flow, q5_context | 113.99 | 88.14 | 593.71/479.72 | 1.238 | 517 | 341/176 | 65.96% | 213/304 | 31.14% | 131.0 | 0.5958 | 0.7625 | 1.565 | 56.03 |
| 140-149 | with_rtds_candles | confirmation | crossvenue, chainlink_fair_value | -13.17 | -32.12 | 379.97/393.13 | 0.967 | 379 | 251/128 | 66.23% | 129/250 | 21.28% | 141.2 | 0.6497 | 0.7974 | 2.029 | 50.72 |
| 140-149 | with_rtds_candles | sealed | crossvenue, chainlink_fair_value | 22.66 | 7.06 | 346.11/323.45 | 1.070 | 312 | 212/100 | 67.95% | 111/201 | 18.80% | 141.2 | 0.6455 | 0.8215 | 1.981 | 37.06 |
| 140-149 | without_rtds_candles | confirmation | crossvenue, chainlink_fair_value | -36.79 | -49.44 | 268.68/305.48 | 0.880 | 253 | 135/118 | 53.36% | 83/170 | 14.21% | 141.0 | 0.5419 | 0.7435 | 1.301 | 61.41 |
| 140-149 | without_rtds_candles | sealed | crossvenue, chainlink_fair_value | 115.04 | 102.59 | 343.46/228.42 | 1.504 | 249 | 162/87 | 65.06% | 92/157 | 15.00% | 141.4 | 0.5374 | 0.7694 | 1.238 | 16.34 |
| 150-159 | with_rtds_candles | confirmation | crossvenue, chainlink_fair_value | -41.36 | -58.31 | 333.46/374.81 | 0.890 | 339 | 219/120 | 64.60% | 121/218 | 19.03% | 151.0 | 0.6510 | 0.8032 | 2.051 | 77.99 |
| 150-159 | with_rtds_candles | sealed | crossvenue, chainlink_fair_value | 119.34 | 104.14 | 372.20/252.86 | 1.472 | 304 | 222/82 | 73.03% | 106/198 | 18.31% | 151.0 | 0.6321 | 0.8304 | 1.839 | 23.49 |
| 150-159 | without_rtds_candles | confirmation | crossvenue, chainlink_fair_value | -39.07 | -50.97 | 260.54/299.61 | 0.870 | 238 | 124/114 | 52.10% | 78/160 | 13.36% | 151.0 | 0.5331 | 0.7450 | 1.251 | 63.54 |
| 150-159 | without_rtds_candles | sealed | crossvenue, chainlink_fair_value | 126.21 | 113.86 | 345.54/219.34 | 1.575 | 247 | 162/85 | 65.59% | 87/160 | 14.88% | 151.1 | 0.5330 | 0.7878 | 1.210 | 16.27 |
| 160-169 | with_rtds_candles | confirmation | chainlink_fair_value | -98.34 | -133.59 | 612.96/711.29 | 0.862 | 705 | 469/236 | 66.52% | 302/403 | 39.58% | 160.8 | 0.6748 | 0.7930 | 2.306 | 118.58 |
| 160-169 | with_rtds_candles | sealed | chainlink_fair_value | 145.64 | 119.54 | 568.28/422.63 | 1.345 | 522 | 379/143 | 72.61% | 198/324 | 31.45% | 160.8 | 0.6517 | 0.8120 | 1.971 | 24.49 |
| 160-169 | without_rtds_candles | confirmation | chainlink_fair_value | -82.29 | -93.69 | 222.22/304.51 | 0.730 | 228 | 122/106 | 53.51% | 83/145 | 12.80% | 161.2 | 0.5868 | 0.8013 | 1.577 | 105.92 |
| 160-169 | without_rtds_candles | sealed | chainlink_fair_value | 130.20 | 118.45 | 318.05/187.86 | 1.693 | 235 | 168/67 | 71.49% | 83/152 | 14.16% | 161.1 | 0.5837 | 0.8416 | 1.481 | 13.66 |
| 170-179 | with_rtds_candles | confirmation | chainlink_fair_value | -152.53 | -174.53 | 416.06/568.59 | 0.732 | 440 | 232/208 | 52.73% | 214/226 | 24.71% | 171.1 | 0.5762 | 0.7510 | 1.524 | 155.46 |
| 170-179 | with_rtds_candles | sealed | chainlink_fair_value | 134.43 | 118.23 | 437.62/303.19 | 1.443 | 324 | 215/109 | 66.36% | 131/193 | 19.52% | 171.1 | 0.5600 | 0.7863 | 1.367 | 22.03 |
| 170-179 | without_rtds_candles | confirmation | chainlink_fair_value | -87.62 | -104.62 | 350.62/438.24 | 0.800 | 340 | 167/173 | 49.12% | 158/182 | 19.09% | 170.9 | 0.5214 | 0.7096 | 1.207 | 92.49 |
| 170-179 | without_rtds_candles | sealed | chainlink_fair_value | 109.40 | 96.10 | 374.92/265.53 | 1.412 | 266 | 165/101 | 62.03% | 113/153 | 16.02% | 171.2 | 0.5169 | 0.7688 | 1.157 | 24.56 |
| 180-189 | with_rtds_candles | confirmation | chainlink_fair_value | -37.05 | -52.95 | 295.13/332.18 | 0.888 | 318 | 214/104 | 67.30% | 140/178 | 17.86% | 181.2 | 0.6770 | 0.8304 | 2.316 | 53.67 |
| 180-189 | with_rtds_candles | sealed | chainlink_fair_value | 98.49 | 85.49 | 327.07/228.57 | 1.431 | 260 | 191/69 | 73.46% | 108/152 | 15.66% | 180.9 | 0.6392 | 0.8544 | 1.935 | 25.74 |
| 180-189 | without_rtds_candles | confirmation | chainlink_fair_value | -69.85 | -89.90 | 389.32/459.17 | 0.848 | 401 | 244/157 | 60.85% | 190/211 | 22.52% | 181.1 | 0.6232 | 0.7908 | 1.833 | 101.95 |
| 180-189 | without_rtds_candles | sealed | chainlink_fair_value | 101.17 | 85.47 | 400.04/298.88 | 1.338 | 314 | 216/98 | 68.79% | 134/180 | 18.92% | 181.0 | 0.6033 | 0.8321 | 1.647 | 42.89 |
| 190-199 | with_rtds_candles | confirmation | chainlink_fair_value, price_control | -73.33 | -91.98 | 360.18/433.51 | 0.831 | 373 | 216/157 | 57.91% | 160/213 | 20.94% | 191.0 | 0.5983 | 0.7907 | 1.656 | 96.47 |
| 190-199 | with_rtds_candles | sealed | chainlink_fair_value, price_control | 153.80 | 138.10 | 418.93/265.13 | 1.580 | 314 | 211/103 | 67.20% | 125/189 | 18.92% | 190.9 | 0.5539 | 0.8147 | 1.296 | 24.76 |
| 190-199 | without_rtds_candles | confirmation | chainlink_fair_value, price_control | -48.20 | -62.45 | 298.58/346.79 | 0.861 | 285 | 150/135 | 52.63% | 121/164 | 16.00% | 190.9 | 0.5391 | 0.7586 | 1.290 | 66.13 |
| 190-199 | without_rtds_candles | sealed | chainlink_fair_value, price_control | 124.25 | 111.30 | 362.93/238.68 | 1.521 | 259 | 161/98 | 62.16% | 96/163 | 15.60% | 191.1 | 0.5050 | 0.8024 | 1.080 | 20.66 |
| 200-209 | with_rtds_candles | confirmation | chainlink_fair_value | 23.72 | 10.42 | 246.44/222.72 | 1.107 | 266 | 194/72 | 72.93% | 99/167 | 14.94% | 201.0 | 0.6932 | 0.8672 | 2.435 | 36.15 |
| 200-209 | with_rtds_candles | sealed | chainlink_fair_value | 196.82 | 183.32 | 355.50/158.68 | 2.240 | 270 | 219/51 | 81.11% | 106/164 | 16.27% | 200.9 | 0.6466 | 0.8875 | 1.917 | 16.26 |
| 200-209 | without_rtds_candles | confirmation | chainlink_fair_value | 2.96 | -15.54 | 299.49/296.53 | 1.010 | 370 | 279/91 | 75.41% | 156/214 | 20.77% | 201.0 | 0.7357 | 0.8763 | 3.036 | 39.59 |
| 200-209 | without_rtds_candles | sealed | chainlink_fair_value | 165.46 | 148.16 | 394.05/228.59 | 1.724 | 346 | 276/70 | 79.77% | 145/201 | 20.84% | 201.0 | 0.6845 | 0.8918 | 2.287 | 29.57 |
| 210-219 | with_rtds_candles | confirmation | chainlink_fair_value | -58.33 | -72.38 | 276.56/334.89 | 0.826 | 281 | 144/137 | 51.25% | 128/153 | 15.78% | 211.3 | 0.5339 | 0.7708 | 1.273 | 77.34 |
| 210-219 | with_rtds_candles | sealed | chainlink_fair_value | 237.15 | 223.15 | 421.46/184.31 | 2.287 | 280 | 196/84 | 70.00% | 115/165 | 16.87% | 211.1 | 0.5106 | 0.8182 | 1.020 | 12.52 |
| 210-219 | without_rtds_candles | confirmation | chainlink_fair_value | -13.08 | -23.78 | 238.52/251.60 | 0.948 | 214 | 118/96 | 55.14% | 104/110 | 12.02% | 211.2 | 0.5430 | 0.8025 | 1.297 | 46.53 |
| 210-219 | without_rtds_candles | sealed | chainlink_fair_value | 225.95 | 214.20 | 376.22/150.27 | 2.504 | 235 | 172/63 | 73.19% | 101/134 | 14.16% | 210.9 | 0.5193 | 0.8500 | 1.090 | 11.06 |
| 220-229 | with_rtds_candles | confirmation | chainlink_fair_value | -20.95 | -38.85 | 348.29/369.23 | 0.943 | 358 | 209/149 | 58.38% | 159/199 | 20.10% | 220.8 | 0.5765 | 0.7790 | 1.487 | 63.19 |
| 220-229 | with_rtds_candles | sealed | chainlink_fair_value | 214.46 | 198.26 | 435.58/221.12 | 1.970 | 324 | 233/91 | 71.91% | 135/189 | 19.52% | 221.2 | 0.5679 | 0.8322 | 1.300 | 17.31 |
| 220-229 | without_rtds_candles | confirmation | chainlink_fair_value | -64.22 | -90.47 | 437.73/501.95 | 0.872 | 525 | 340/185 | 64.76% | 230/295 | 29.48% | 220.7 | 0.6549 | 0.8135 | 2.107 | 95.24 |
| 220-229 | without_rtds_candles | sealed | chainlink_fair_value | 215.95 | 193.85 | 500.77/284.83 | 1.758 | 442 | 330/112 | 74.66% | 189/253 | 26.63% | 221.0 | 0.6315 | 0.8449 | 1.676 | 21.11 |
| 230-239 | with_rtds_candles | confirmation | chainlink_fair_value | -42.39 | -67.14 | 376.21/418.60 | 0.899 | 495 | 338/157 | 68.28% | 221/274 | 27.79% | 230.9 | 0.6834 | 0.8304 | 2.395 | 76.27 |
| 230-239 | with_rtds_candles | sealed | chainlink_fair_value | 255.97 | 235.92 | 477.61/221.63 | 2.155 | 401 | 308/93 | 76.81% | 166/235 | 24.16% | 230.8 | 0.6229 | 0.8468 | 1.537 | 18.39 |
| 230-239 | without_rtds_candles | confirmation | chainlink_fair_value | -9.78 | -33.98 | 388.10/397.88 | 0.975 | 484 | 325/159 | 67.15% | 204/280 | 27.18% | 230.8 | 0.6589 | 0.8213 | 2.096 | 59.16 |
| 230-239 | without_rtds_candles | sealed | chainlink_fair_value | 248.06 | 228.01 | 481.70/233.64 | 2.062 | 401 | 298/103 | 74.31% | 162/239 | 24.16% | 230.9 | 0.6018 | 0.8448 | 1.403 | 15.93 |

## aggregate_consensus: per-bucket PnL

| Seconds | RTDS | Period | Donors | PnL | Stress | Gross +/− | PF | Trades | W/L | Win rate | UP/DOWN | Coverage | Avg entry | Avg cost | Avg confidence | Recovery | Max DD |
|---:|---|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 60-69 | with_rtds_candles | confirmation | chainlink_fair_value, q5_context, crossvenue | 12.52 | -8.83 | 484.09/471.57 | 1.027 | 427 | 249/178 | 58.31% | 97/330 | 23.98% | 61.3 | 0.5561 | 0.6905 | 1.363 | 60.93 |
| 60-69 | with_rtds_candles | sealed | chainlink_fair_value, q5_context, crossvenue | 98.45 | 80.30 | 449.60/351.15 | 1.280 | 363 | 229/134 | 63.09% | 115/248 | 21.87% | 61.4 | 0.5554 | 0.7021 | 1.335 | 22.82 |
| 60-69 | without_rtds_candles | confirmation | chainlink_fair_value, q5_context, crossvenue | -24.04 | -45.79 | 482.56/506.59 | 0.953 | 435 | 243/192 | 55.86% | 102/333 | 24.42% | 61.2 | 0.5484 | 0.6875 | 1.329 | 59.69 |
| 60-69 | without_rtds_candles | sealed | chainlink_fair_value, q5_context, crossvenue | 30.79 | 12.54 | 435.38/404.60 | 1.076 | 365 | 212/153 | 58.08% | 114/251 | 21.99% | 61.3 | 0.5427 | 0.6975 | 1.288 | 44.46 |
| 70-79 | with_rtds_candles | confirmation | q5_context, price_control, chainlink_fair_value | 0.40 | -12.70 | 290.69/290.29 | 1.001 | 262 | 165/97 | 62.98% | 62/200 | 14.71% | 71.0 | 0.6086 | 0.7509 | 1.699 | 38.01 |
| 70-79 | with_rtds_candles | sealed | q5_context, price_control, chainlink_fair_value | 92.86 | 80.21 | 315.26/222.40 | 1.418 | 253 | 175/78 | 69.17% | 83/170 | 15.24% | 71.2 | 0.5975 | 0.7609 | 1.583 | 29.19 |
| 70-79 | without_rtds_candles | confirmation | q5_context, price_control, chainlink_fair_value | 31.68 | 17.83 | 315.18/283.51 | 1.112 | 277 | 181/96 | 65.34% | 59/218 | 15.55% | 71.1 | 0.6097 | 0.7541 | 1.696 | 40.95 |
| 70-79 | without_rtds_candles | sealed | q5_context, price_control, chainlink_fair_value | 65.83 | 52.73 | 316.77/250.94 | 1.262 | 262 | 173/89 | 66.03% | 89/173 | 15.78% | 71.1 | 0.5892 | 0.7604 | 1.540 | 27.27 |
| 80-89 | with_rtds_candles | confirmation | chainlink_fair_value, external_flow, q5_context | -75.25 | -92.20 | 360.06/435.31 | 0.827 | 339 | 180/159 | 53.10% | 91/248 | 19.03% | 81.0 | 0.5542 | 0.6933 | 1.369 | 97.57 |
| 80-89 | with_rtds_candles | sealed | chainlink_fair_value, external_flow, q5_context | 121.91 | 106.11 | 416.42/294.51 | 1.414 | 316 | 204/112 | 64.56% | 123/193 | 19.04% | 81.2 | 0.5473 | 0.7146 | 1.288 | 24.27 |
| 80-89 | without_rtds_candles | confirmation | chainlink_fair_value, external_flow, q5_context | -33.52 | -51.77 | 404.95/438.47 | 0.924 | 365 | 202/163 | 55.34% | 106/259 | 20.49% | 81.0 | 0.5506 | 0.6937 | 1.342 | 71.74 |
| 80-89 | without_rtds_candles | sealed | chainlink_fair_value, external_flow, q5_context | 105.15 | 87.95 | 439.34/334.19 | 1.315 | 344 | 216/128 | 62.79% | 140/204 | 20.72% | 81.3 | 0.5457 | 0.7154 | 1.284 | 35.80 |
| 90-99 | with_rtds_candles | confirmation | chainlink_fair_value, crossvenue | -31.32 | -41.92 | 234.32/265.63 | 0.882 | 212 | 107/105 | 50.47% | 66/146 | 11.90% | 91.3 | 0.5128 | 0.7047 | 1.155 | 69.51 |
| 90-99 | with_rtds_candles | sealed | chainlink_fair_value, crossvenue | 83.55 | 72.45 | 297.28/213.72 | 1.391 | 222 | 138/84 | 62.16% | 74/148 | 13.37% | 91.1 | 0.5251 | 0.7385 | 1.181 | 38.96 |
| 90-99 | without_rtds_candles | confirmation | chainlink_fair_value, crossvenue | -22.26 | -25.06 | 55.77/78.02 | 0.715 | 56 | 23/33 | 41.07% | 56/0 | 3.14% | 91.4 | 0.4684 | 0.6691 | 0.975 | 40.21 |
| 90-99 | without_rtds_candles | sealed | chainlink_fair_value, crossvenue | 42.62 | 39.02 | 109.34/66.72 | 1.639 | 72 | 44/28 | 61.11% | 72/0 | 4.34% | 91.3 | 0.4714 | 0.7027 | 0.959 | 9.57 |
| 100-109 | with_rtds_candles | confirmation | crossvenue, price_control, external_flow | -24.80 | -52.20 | 571.61/596.41 | 0.958 | 548 | 337/211 | 61.50% | 190/358 | 30.77% | 100.9 | 0.6038 | 0.7270 | 1.666 | 46.44 |
| 100-109 | with_rtds_candles | sealed | crossvenue, price_control, external_flow | 95.18 | 73.58 | 513.94/418.76 | 1.227 | 432 | 284/148 | 65.74% | 178/254 | 26.02% | 101.0 | 0.5931 | 0.7454 | 1.564 | 73.05 |
| 100-109 | without_rtds_candles | confirmation | crossvenue, price_control, external_flow | -33.14 | -60.29 | 560.07/593.21 | 0.944 | 543 | 331/212 | 60.96% | 172/371 | 30.49% | 100.9 | 0.6016 | 0.7285 | 1.654 | 42.32 |
| 100-109 | without_rtds_candles | sealed | crossvenue, price_control, external_flow | 108.87 | 87.22 | 525.41/416.54 | 1.261 | 433 | 285/148 | 65.82% | 162/271 | 26.08% | 100.9 | 0.5877 | 0.7437 | 1.527 | 67.54 |
| 110-119 | with_rtds_candles | confirmation | crossvenue, chainlink_fair_value, external_flow | -71.97 | -100.52 | 576.27/648.23 | 0.889 | 571 | 349/222 | 61.12% | 220/351 | 32.06% | 110.8 | 0.6165 | 0.7311 | 1.768 | 81.61 |
| 110-119 | with_rtds_candles | sealed | crossvenue, chainlink_fair_value, external_flow | 104.57 | 81.77 | 531.31/426.74 | 1.245 | 456 | 305/151 | 66.89% | 180/276 | 27.47% | 110.8 | 0.6030 | 0.7498 | 1.622 | 48.05 |
| 110-119 | without_rtds_candles | confirmation | crossvenue, chainlink_fair_value, external_flow | 24.05 | 12.80 | 277.22/253.17 | 1.095 | 225 | 120/105 | 53.33% | 79/146 | 12.63% | 111.0 | 0.4904 | 0.6991 | 1.044 | 36.66 |
| 110-119 | without_rtds_candles | sealed | crossvenue, chainlink_fair_value, external_flow | 88.90 | 78.40 | 295.98/207.08 | 1.429 | 210 | 126/84 | 60.00% | 80/130 | 12.65% | 110.9 | 0.4938 | 0.7323 | 1.049 | 26.16 |
| 120-129 | with_rtds_candles | confirmation | external_flow, chainlink_fair_value, price_control | -44.60 | -56.30 | 245.02/289.62 | 0.846 | 234 | 114/120 | 48.72% | 103/131 | 13.14% | 121.2 | 0.5041 | 0.7161 | 1.123 | 55.95 |
| 120-129 | with_rtds_candles | sealed | external_flow, chainlink_fair_value, price_control | 81.76 | 70.16 | 308.30/226.54 | 1.361 | 232 | 143/89 | 61.64% | 95/137 | 13.98% | 121.3 | 0.5247 | 0.7598 | 1.181 | 25.56 |
| 120-129 | without_rtds_candles | confirmation | external_flow, chainlink_fair_value, price_control | -42.33 | -65.53 | 523.98/566.31 | 0.925 | 464 | 255/209 | 54.96% | 217/247 | 26.05% | 121.0 | 0.5463 | 0.6810 | 1.319 | 62.87 |
| 120-129 | without_rtds_candles | sealed | external_flow, chainlink_fair_value, price_control | 31.37 | 13.67 | 437.36/405.99 | 1.077 | 354 | 203/151 | 57.34% | 164/190 | 21.33% | 121.1 | 0.5342 | 0.7059 | 1.248 | 48.94 |
| 130-139 | with_rtds_candles | confirmation | crossvenue, external_flow, q5_context | -4.63 | -17.18 | 282.40/287.03 | 0.984 | 251 | 136/115 | 54.18% | 91/160 | 14.09% | 131.1 | 0.5246 | 0.7337 | 1.202 | 38.46 |
| 130-139 | with_rtds_candles | sealed | crossvenue, external_flow, q5_context | 91.37 | 79.67 | 313.07/221.70 | 1.412 | 234 | 145/89 | 61.97% | 96/138 | 14.10% | 131.2 | 0.5205 | 0.7641 | 1.154 | 31.09 |
| 130-139 | without_rtds_candles | confirmation | crossvenue, external_flow, q5_context | -5.75 | -18.75 | 296.66/302.41 | 0.981 | 260 | 140/120 | 53.85% | 101/159 | 14.60% | 131.0 | 0.5218 | 0.7301 | 1.189 | 48.70 |
| 130-139 | without_rtds_candles | sealed | crossvenue, external_flow, q5_context | 129.95 | 116.90 | 365.59/235.64 | 1.551 | 261 | 167/94 | 63.98% | 116/145 | 15.72% | 131.2 | 0.5192 | 0.7565 | 1.145 | 23.86 |
| 140-149 | with_rtds_candles | confirmation | crossvenue, chainlink_fair_value | 0.42 | -12.58 | 238.04/237.61 | 1.002 | 260 | 191/69 | 73.46% | 89/171 | 14.60% | 141.2 | 0.7160 | 0.8553 | 2.763 | 41.67 |
| 140-149 | with_rtds_candles | sealed | crossvenue, chainlink_fair_value | 25.76 | 13.21 | 262.59/236.83 | 1.109 | 251 | 183/68 | 72.91% | 84/167 | 15.12% | 141.3 | 0.6899 | 0.8666 | 2.427 | 35.61 |
| 140-149 | without_rtds_candles | confirmation | crossvenue, chainlink_fair_value | -64.94 | -78.44 | 277.21/342.15 | 0.810 | 270 | 136/134 | 50.37% | 99/171 | 15.16% | 141.1 | 0.5309 | 0.7329 | 1.253 | 80.20 |
| 140-149 | without_rtds_candles | sealed | crossvenue, chainlink_fair_value | 99.34 | 86.69 | 342.78/243.44 | 1.408 | 253 | 159/94 | 62.85% | 99/154 | 15.24% | 141.3 | 0.5291 | 0.7648 | 1.201 | 16.33 |
| 150-159 | with_rtds_candles | confirmation | crossvenue, chainlink_fair_value | -69.20 | -90.90 | 453.94/523.14 | 0.868 | 434 | 245/189 | 56.45% | 184/250 | 24.37% | 151.0 | 0.5758 | 0.7366 | 1.494 | 94.25 |
| 150-159 | with_rtds_candles | sealed | crossvenue, chainlink_fair_value | 140.10 | 122.95 | 457.71/317.61 | 1.441 | 343 | 228/115 | 66.47% | 130/213 | 20.66% | 151.0 | 0.5622 | 0.7705 | 1.376 | 22.19 |
| 150-159 | without_rtds_candles | confirmation | crossvenue, chainlink_fair_value | -40.66 | -54.06 | 293.00/333.66 | 0.878 | 268 | 139/129 | 51.87% | 95/173 | 15.05% | 150.9 | 0.5283 | 0.7432 | 1.227 | 63.57 |
| 150-159 | without_rtds_candles | sealed | crossvenue, chainlink_fair_value | 157.08 | 144.08 | 368.73/211.65 | 1.742 | 260 | 177/83 | 68.08% | 94/166 | 15.66% | 151.0 | 0.5393 | 0.7937 | 1.224 | 15.47 |
| 160-169 | with_rtds_candles | confirmation | chainlink_fair_value | -98.34 | -133.59 | 612.96/711.29 | 0.862 | 705 | 469/236 | 66.52% | 302/403 | 39.58% | 160.8 | 0.6748 | 0.7930 | 2.306 | 118.58 |
| 160-169 | with_rtds_candles | sealed | chainlink_fair_value | 145.64 | 119.54 | 568.28/422.63 | 1.345 | 522 | 379/143 | 72.61% | 198/324 | 31.45% | 160.8 | 0.6517 | 0.8120 | 1.971 | 24.49 |
| 160-169 | without_rtds_candles | confirmation | chainlink_fair_value | -82.29 | -93.69 | 222.22/304.51 | 0.730 | 228 | 122/106 | 53.51% | 83/145 | 12.80% | 161.2 | 0.5868 | 0.8013 | 1.577 | 105.92 |
| 160-169 | without_rtds_candles | sealed | chainlink_fair_value | 130.20 | 118.45 | 318.05/187.86 | 1.693 | 235 | 168/67 | 71.49% | 83/152 | 14.16% | 161.1 | 0.5837 | 0.8416 | 1.481 | 13.66 |
| 170-179 | with_rtds_candles | confirmation | chainlink_fair_value | -152.53 | -174.53 | 416.06/568.59 | 0.732 | 440 | 232/208 | 52.73% | 214/226 | 24.71% | 171.1 | 0.5762 | 0.7510 | 1.524 | 155.46 |
| 170-179 | with_rtds_candles | sealed | chainlink_fair_value | 134.43 | 118.23 | 437.62/303.19 | 1.443 | 324 | 215/109 | 66.36% | 131/193 | 19.52% | 171.1 | 0.5600 | 0.7863 | 1.367 | 22.03 |
| 170-179 | without_rtds_candles | confirmation | chainlink_fair_value | -87.62 | -104.62 | 350.62/438.24 | 0.800 | 340 | 167/173 | 49.12% | 158/182 | 19.09% | 170.9 | 0.5214 | 0.7096 | 1.207 | 92.49 |
| 170-179 | without_rtds_candles | sealed | chainlink_fair_value | 109.40 | 96.10 | 374.92/265.53 | 1.412 | 266 | 165/101 | 62.03% | 113/153 | 16.02% | 171.2 | 0.5169 | 0.7688 | 1.157 | 24.56 |
| 180-189 | with_rtds_candles | confirmation | chainlink_fair_value | -37.05 | -52.95 | 295.13/332.18 | 0.888 | 318 | 214/104 | 67.30% | 140/178 | 17.86% | 181.2 | 0.6770 | 0.8304 | 2.316 | 53.67 |
| 180-189 | with_rtds_candles | sealed | chainlink_fair_value | 98.49 | 85.49 | 327.07/228.57 | 1.431 | 260 | 191/69 | 73.46% | 108/152 | 15.66% | 180.9 | 0.6392 | 0.8544 | 1.935 | 25.74 |
| 180-189 | without_rtds_candles | confirmation | chainlink_fair_value | -69.85 | -89.90 | 389.32/459.17 | 0.848 | 401 | 244/157 | 60.85% | 190/211 | 22.52% | 181.1 | 0.6232 | 0.7908 | 1.833 | 101.95 |
| 180-189 | without_rtds_candles | sealed | chainlink_fair_value | 101.17 | 85.47 | 400.04/298.88 | 1.338 | 314 | 216/98 | 68.79% | 134/180 | 18.92% | 181.0 | 0.6033 | 0.8321 | 1.647 | 42.89 |
| 190-199 | with_rtds_candles | confirmation | chainlink_fair_value, price_control | -56.34 | -72.99 | 251.06/307.41 | 0.817 | 333 | 243/90 | 72.97% | 123/210 | 18.70% | 190.9 | 0.7466 | 0.8668 | 3.306 | 85.64 |
| 190-199 | with_rtds_candles | sealed | chainlink_fair_value, price_control | 154.48 | 139.78 | 338.99/184.51 | 1.837 | 294 | 236/58 | 80.27% | 108/186 | 17.71% | 191.1 | 0.6799 | 0.8850 | 2.215 | 23.47 |
| 190-199 | without_rtds_candles | confirmation | chainlink_fair_value, price_control | -73.13 | -88.28 | 239.34/312.48 | 0.766 | 303 | 207/96 | 68.32% | 114/189 | 17.01% | 190.8 | 0.7138 | 0.8602 | 2.815 | 90.01 |
| 190-199 | without_rtds_candles | sealed | chainlink_fair_value, price_control | 147.64 | 133.59 | 342.60/194.96 | 1.757 | 281 | 219/62 | 77.94% | 112/169 | 16.93% | 191.0 | 0.6560 | 0.8791 | 2.010 | 20.60 |
| 200-209 | with_rtds_candles | confirmation | chainlink_fair_value | 23.72 | 10.42 | 246.44/222.72 | 1.107 | 266 | 194/72 | 72.93% | 99/167 | 14.94% | 201.0 | 0.6932 | 0.8672 | 2.435 | 36.15 |
| 200-209 | with_rtds_candles | sealed | chainlink_fair_value | 196.82 | 183.32 | 355.50/158.68 | 2.240 | 270 | 219/51 | 81.11% | 106/164 | 16.27% | 200.9 | 0.6466 | 0.8875 | 1.917 | 16.26 |
| 200-209 | without_rtds_candles | confirmation | chainlink_fair_value | 2.96 | -15.54 | 299.49/296.53 | 1.010 | 370 | 279/91 | 75.41% | 156/214 | 20.77% | 201.0 | 0.7357 | 0.8763 | 3.036 | 39.59 |
| 200-209 | without_rtds_candles | sealed | chainlink_fair_value | 165.46 | 148.16 | 394.05/228.59 | 1.724 | 346 | 276/70 | 79.77% | 145/201 | 20.84% | 201.0 | 0.6845 | 0.8918 | 2.287 | 29.57 |
| 210-219 | with_rtds_candles | confirmation | chainlink_fair_value | -58.33 | -72.38 | 276.56/334.89 | 0.826 | 281 | 144/137 | 51.25% | 128/153 | 15.78% | 211.3 | 0.5339 | 0.7708 | 1.273 | 77.34 |
| 210-219 | with_rtds_candles | sealed | chainlink_fair_value | 237.15 | 223.15 | 421.46/184.31 | 2.287 | 280 | 196/84 | 70.00% | 115/165 | 16.87% | 211.1 | 0.5106 | 0.8182 | 1.020 | 12.52 |
| 210-219 | without_rtds_candles | confirmation | chainlink_fair_value | -13.08 | -23.78 | 238.52/251.60 | 0.948 | 214 | 118/96 | 55.14% | 104/110 | 12.02% | 211.2 | 0.5430 | 0.8025 | 1.297 | 46.53 |
| 210-219 | without_rtds_candles | sealed | chainlink_fair_value | 225.95 | 214.20 | 376.22/150.27 | 2.504 | 235 | 172/63 | 73.19% | 101/134 | 14.16% | 210.9 | 0.5193 | 0.8500 | 1.090 | 11.06 |
| 220-229 | with_rtds_candles | confirmation | chainlink_fair_value | -20.95 | -38.85 | 348.29/369.23 | 0.943 | 358 | 209/149 | 58.38% | 159/199 | 20.10% | 220.8 | 0.5765 | 0.7790 | 1.487 | 63.19 |
| 220-229 | with_rtds_candles | sealed | chainlink_fair_value | 214.46 | 198.26 | 435.58/221.12 | 1.970 | 324 | 233/91 | 71.91% | 135/189 | 19.52% | 221.2 | 0.5679 | 0.8322 | 1.300 | 17.31 |
| 220-229 | without_rtds_candles | confirmation | chainlink_fair_value | -64.22 | -90.47 | 437.73/501.95 | 0.872 | 525 | 340/185 | 64.76% | 230/295 | 29.48% | 220.7 | 0.6549 | 0.8135 | 2.107 | 95.24 |
| 220-229 | without_rtds_candles | sealed | chainlink_fair_value | 215.95 | 193.85 | 500.77/284.83 | 1.758 | 442 | 330/112 | 74.66% | 189/253 | 26.63% | 221.0 | 0.6315 | 0.8449 | 1.676 | 21.11 |
| 230-239 | with_rtds_candles | confirmation | chainlink_fair_value | -42.39 | -67.14 | 376.21/418.60 | 0.899 | 495 | 338/157 | 68.28% | 221/274 | 27.79% | 230.9 | 0.6834 | 0.8304 | 2.395 | 76.27 |
| 230-239 | with_rtds_candles | sealed | chainlink_fair_value | 255.97 | 235.92 | 477.61/221.63 | 2.155 | 401 | 308/93 | 76.81% | 166/235 | 24.16% | 230.8 | 0.6229 | 0.8468 | 1.537 | 18.39 |
| 230-239 | without_rtds_candles | confirmation | chainlink_fair_value | -9.78 | -33.98 | 388.10/397.88 | 0.975 | 484 | 325/159 | 67.15% | 204/280 | 27.18% | 230.8 | 0.6589 | 0.8213 | 2.096 | 59.16 |
| 230-239 | without_rtds_candles | sealed | chainlink_fair_value | 248.06 | 228.01 | 481.70/233.64 | 2.062 | 401 | 298/103 | 74.31% | 162/239 | 24.16% | 230.9 | 0.6018 | 0.8448 | 1.403 | 15.93 |

## Integrity and interpretation

- Fit, policy, sealed, and confirmation markets have zero overlap.
- Donor selection uses only historical trade evidence before August 26.
- All variants use executable VWAP5, the same fees/reserve, one-cent-per-share stress, and at most one aggregate entry per market.
- The source panel includes all existing core, Chainlink candle/oracle/RefPrice, Binance, Kraken, open-interest, and L2 feature groups; individual donor recipes retain their historical feature scope.
- The panel has no pre-60-second observations, so the aggregate abstains before 60 seconds rather than inventing coverage.
- September has been viewed in earlier research, so these are chronologically isolated but not epistemically fresh results.
