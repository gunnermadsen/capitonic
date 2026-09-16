# Parquet archive

These Parquet files are aggregate evaluation tables for the BTC five-minute micro-edge model tournament run `20260915T030000Z`. They were committed with result commit `6cb0ccb623073358d52afa454b3cc87339a9f8ce` and are archived outside Git at:

`/Volumes/docker-data/polymarket-bot/artifacts/training/btc-5m-directional/micro-edge-model-tournament/20260915T030000Z`

The archive is organized by use:

- `metrics/data-coverage/`: source availability and market/day/source coverage tables.
- `metrics/model-evaluation/`: aggregate metrics, calibration, chronological folds, regimes, predictive quality, and five-second evaluations across the 32 new candidates and 3 historical refits.
- `metrics/policy-and-bucket-analysis/`: fixed-policy bucket comparisons and unrestricted retrospective bucket attribution.
- `metrics/matched-baseline-comparison/`: matched-coverage and matched-frequency comparisons against historical baselines.
- `metrics/router-evaluation/`: eligible-first, incremental, opportunity-reservation, and historical-baseline-refit router results plus router bucket metrics.
- `diagnostics/latency/`: latency sensitivity evidence.

Model names, hypotheses, feature groups, and RTDS variants are defined in `candidate-registry.json`. Model artifact identities and hashes remain in `model-provenance.json`; the archived Parquet files are tournament evaluation outputs, not deployable model artifacts.
