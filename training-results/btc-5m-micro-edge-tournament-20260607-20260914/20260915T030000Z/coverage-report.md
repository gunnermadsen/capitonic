# Full-range data coverage

1,705,440 decision rows; 28,424 valid resolved markets. June 7 through September 14 inclusive.

Optional presence counts are not an assertion of continuous or executable coverage. Books require side-specific five-share depth and freshness.

| Source | Rows | Markets | First decision | Last decision |
|---|---:|---:|---|---|
| binance_prints | 368,201 | 12,032 | 2026-07-22 00:00:10+00:00 | 2026-09-14 23:59:00+00:00 |
| candles | 1,695,888 | 28,266 | 2026-06-07 00:00:00+00:00 | 2026-09-14 23:59:55+00:00 |
| core | 1,705,440 | 28,424 | 2026-06-07 00:00:00+00:00 | 2026-09-14 23:59:55+00:00 |
| execution | 974,884 | 19,482 | 2026-06-07 00:00:05+00:00 | 2026-09-14 23:59:55+00:00 |
| kraken | 694,212 | 24,323 | 2026-06-07 00:00:00+00:00 | 2026-08-30 16:53:10+00:00 |
| kraken_l2 | 1,041,435 | 17,363 | 2026-06-07 00:00:05+00:00 | 2026-08-19 10:00:00+00:00 |
| open_interest | 1,256,099 | 20,935 | 2026-07-03 00:05:05+00:00 | 2026-09-14 23:59:55+00:00 |
| oracle | 1,364,352 | 28,424 | 2026-06-07 00:01:00+00:00 | 2026-09-14 23:59:55+00:00 |
| refprice | 1,233,160 | 23,050 | 2026-06-07 05:48:20+00:00 | 2026-08-26 23:59:55+00:00 |
| spot_l2 | 888,313 | 14,834 | 2026-06-07 00:00:00+00:00 | 2026-08-02 00:00:00+00:00 |
| twap | 1,377,010 | 23,100 | 2026-06-07 05:50:00+00:00 | 2026-08-27 00:00:00+00:00 |

## Usable five-share execution

| Side | Rows | Markets | First | Last |
|---|---:|---:|---|---|
| up | 850,458 | 19,377 | 2026-06-07 00:00:05+00:00 | 2026-09-14 23:59:55+00:00 |
| down | 850,277 | 19,373 | 2026-06-07 00:00:05+00:00 | 2026-09-14 23:59:55+00:00 |

Day/source/bucket gaps are retained in coverage-day-source-bucket.parquet. June 7–20 initializes supervised fitting; June 21–July 4 initializes causal calibration and policy selection. All eligible dates return to the final directional fits. Waiting auxiliaries use only causal OOF teacher support; their full-range base models are recorded explicitly.
