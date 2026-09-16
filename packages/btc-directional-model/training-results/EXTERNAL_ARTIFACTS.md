# External generated artifacts

Generated artifacts formerly stored below this directory were migrated from integration snapshot `2dc90520c90b` to the external SSD. Code, configuration, lightweight reports, and provenance remain in Git.

The canonical run path is:

`/Volumes/docker-data/polymarket-bot/artifacts/<activity>/btc-5m-directional/<workflow>/<run-id>/`

Legacy directories containing `replay` use the `backtests` activity; the remaining directories use `training`. Existing UTC run-directory names are preserved. A legacy result without a run-directory timestamp uses the UTC creation time of its first Git commit.

Within each run, generated files are organized under `models/`, `datasets/`, `predictions/`, `trades/`, `metrics/`, or `diagnostics/` according to their purpose. Every migrated run contains a `README.md` and `manifests/artifact-sha256.txt` on the SSD.

The repository retains `packages/btc-directional-model/tests/fixtures/umr-core-feature-parity.parquet` because it is a small deterministic test fixture, not a generated training or backtest artifact.
