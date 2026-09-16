# Parquet archive

These Parquet files are evaluation evidence for the frozen top-ten Q5-PnL router replay described in this directory. They were produced by source commit `8b08e7ff57625450d0c0e24a954d0e64cc3441fa` and are archived outside Git at:

`/Volumes/docker-data/polymarket-bot/model-training-archive/top-pnl-router-replay/8b08e7ff57625450d0c0e24a954d0e64cc3441fa`

The archive is organized by use:

- `member-predictions/`: causal predictions for the ten frozen BTC five-minute directional members. Files `01` through `10` correspond to the ranked `members` array in `router-manifest.json`.
- `member-attribution/`: per-member metrics and contribution accounting used to explain router results.
- `member-trade-ledgers/`: member-level sealed and confirmation trades at quantities 5 and 50.
- `router-trade-ledgers/`: final first-qualifying-entry router trades for the sealed and confirmation periods at quantities 5 and 50.

The exact member names, model inputs, policies, RTDS variants, source bundles, and hashes remain in `router-manifest.json`. The files support replay and evaluation only; no model was trained or deployed by this run.
