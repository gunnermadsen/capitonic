# BTC ignored training archive inventory

This inventory covers `/Volumes/docker-data/archives/polymarket-bot/ml-training/btc-directional-model-ignored-training-20260825-fa05ed3.tar.zst`. The archive remains the source artifact. Inspection used a temporary extracted directory, which was removed after the inventory was verified. Its four model artifact groups were subsequently moved to the canonical SSD root as described below.

## Identity and verification

- Archive date: 2026-08-25, from the filename and file metadata.
- Filename commit suffix: `fa05ed3`, resolving to repository commit `fa05ed342aff7ae8f4b18fbe1a31575f15038f93` (`docs(git): clarify model provenance tagging`). The suffix alone does not prove this commit produced the archive.
- Compressed size: 23,935,967,108 bytes (22.30 GiB).
- Archive SHA-256: `bb927adea127ac93394b187970e36c9394e669a0aa16d0f2f874cfeca398e5a9`, matching the adjacent `.sha256` file.
- Tar listing: 6,002 entries. Extraction: 5,616 regular files, three symlinks, and 383 directories. Regular-file bytes total 24,480,519,907 (22.80 GiB), matching the tar listing.
- All tar entry paths were relative and contained no `..` traversal. The three symlinks were inspected as links and not followed.

## Contents by area

| Area under `packages/btc-directional-model/` | Regular files | Bytes | GiB | Content |
| --- | ---: | ---: | ---: | --- |
| `data/` | 4,247 | 21,947,953,295 | 20.44 | Historical Parquet source caches, sidecar JSON, and input panels. |
| `runs/` | 1,320 | 2,463,156,371 | 2.29 | Timestamped training/evaluation outputs, checkpoints, predictions, metrics, diagnostics, and reports. |
| `artifacts/` | 49 | 69,410,241 | 0.06 | Frozen or exported model bundles and associated manifests or feature vectors. |

The archive contains 3,716 Parquet files, 1,501 JSON files, 148 Joblib files, 55 pickle files, 84 CSV files, 60 HTML files, 25 Markdown files, 14 SHA-256 files, one `.partial` file, and 12 files without an extension. These counts exclude the three symlinks.

## Historical data caches

These are directory names and physical file sizes. A cache directory is not evidence that its model completed training or qualified for deployment.

| Data directory | Files | GiB |
| --- | ---: | ---: |
| `btc-asymmetric-early-no-calibration-20260414-20260820` | 1 | 0.00 |
| `btc-asymmetric-value-calibrated-20260414-20260802` | 569 | 6.02 |
| `btc-asymmetric-value-hunter-20260414-20260802` | 240 | 2.50 |
| `btc-asymmetric-value-one-second-20260414-20260802` | 244 | 4.61 |
| `btc-chainlink-oi-champion-20260321-20260729` | 4 | 0.06 |
| `btc-chainlink-oi-champion-benchmark` | 27 | 0.01 |
| `btc-continuous-edge-directional-20260525-20260723` | 6 | 0.08 |
| `btc-continuous-edge-payoff-robust-20260525-20260802` | 6 | 0.11 |
| `btc-core-chainlink-oi-20260321-20260729` | 264 | 1.19 |
| `btc-core-oracle-early-entry-20260321-20260729` | 393 | 0.40 |
| `btc-early-price-value-20260414-20260802` | 367 | 2.20 |
| `btc-middle-market-tournament-20260802-20260810` | 30 | 0.06 |
| `btc-spot-l2-chainlink-candles-20260414-20260802` | 2,095 | 3.19 |

## Run outputs

There are 48 named run lineages and 110 timestamped run directories, including two directly under `runs/` and three nested under `exploratory/` or `precommit-results/`. Some runs contain only a progress or split record. Lifecycle, qualification, and deployment states need source-specific review; this inventory does not infer them from file presence.

| Run lineage | Timestamped runs | Files | GiB |
| --- | ---: | ---: | ---: |
| `btc-accuracy-timing-20260421-20260720` | 2 | 49 | 0.11 |
| `btc-asymmetric-book-admission-readiness` | 0 | 1 | 0.00 |
| `btc-asymmetric-core-oracle-book-admission-20260414-20260802` | 2 | 83 | 0.00 |
| `btc-asymmetric-core-oracle-d4-side-calibration-20260414-20260802` | 1 | 27 | 0.00 |
| `btc-asymmetric-core-oracle-gen2-calibration-20260716-20260802` | 3 | 80 | 0.12 |
| `btc-asymmetric-core-oracle-gen2-readiness` | 0 | 1 | 0.00 |
| `btc-asymmetric-decision-quality-20260414-20260802` | 1 | 6 | 0.00 |
| `btc-asymmetric-early-no-calibration-20260414-20260802` | 1 | 6 | 0.00 |
| `btc-asymmetric-early-no-calibration-20260414-20260820` | 3 | 12 | 0.00 |
| `btc-asymmetric-value-calibrated-20260414-20260802` | 5 | 170 | 0.58 |
| `btc-asymmetric-value-hunter-20260414-20260802` | 3 | 79 | 0.06 |
| `btc-asymmetric-value-one-second-20260414-20260802` | 1 | 29 | 0.24 |
| `btc-book-residual-20260527-20260720` | 6 | 54 | 0.04 |
| `btc-boundary-alignment-20260421-20260720` | 2 | 27 | 0.06 |
| `btc-boundary-reversal-accuracy-20260321-20260729` | 2 | 27 | 0.08 |
| `btc-chainlink-oi-champion-20260321-20260729` | 4 | 52 | 0.01 |
| `btc-chainlink-oi-forward-20260729-20260802` | 2 | 4 | 0.00 |
| `btc-continuous-edge-directional-20260525-20260723` | 2 | 10 | 0.00 |
| `btc-continuous-edge-payoff-aware-20260525-20260723` | 1 | 5 | 0.00 |
| `btc-core-20260421-20260620` | 4 | 18 | 0.02 |
| `btc-core-20260421-20260720` | 2 | 8 | 0.01 |
| `btc-core-coverage-088-20260421-20260720` | 1 | 5 | 0.01 |
| `btc-core-coverage-089-20260421-20260720` | 7 | 27 | 0.02 |
| `btc-core-coverage-20260421-20260720` | 2 | 6 | 0.01 |
| `btc-core-early-entry-benchmark-20260421-20260720` | 1 | 7 | 0.01 |
| `btc-core-early-entry-calibration-20260421-20260720` | 1 | 5 | 0.03 |
| `btc-core-oracle-early-entry-20260321-20260729` | 1 | 7 | 0.01 |
| `btc-correctness-admission-20260421-20260720` | 1 | 5 | 0.00 |
| `btc-early-price-value-20260414-20260802` | 2 | 34 | 0.15 |
| `btc-entry-benchmark-20260421-20260720` | 5 | 26 | 0.02 |
| `btc-fold-robust-frequency-20260421-20260720` | 3 | 53 | 0.12 |
| `btc-fold-robust-frequency-policy-20260421-20260720` | 2 | 8 | 0.00 |
| `btc-frequency-policy-20260421-20260720` | 5 | 20 | 0.01 |
| `btc-mature-reversal-accuracy-20260321-20260729` | 1 | 26 | 0.08 |
| `btc-mature-reversal-fixed-120-20260321-20260729` | 2 | 7 | 0.00 |
| `btc-mature-reversal-fixed-120-reversal-decision-20260321-20260729` | 1 | 5 | 0.01 |
| `btc-mature-reversal-fixed-120-selective-accuracy-20260321-20260729` | 2 | 7 | 0.01 |
| `btc-mature-reversal-oracle-accuracy-20260321-20260729` | 3 | 28 | 0.02 |
| `btc-oracle-book-matched-20260526-20260721` | 2 | 16 | 0.00 |
| `btc-path-persistence-20260421-20260720` | 2 | 8 | 0.04 |
| `btc-price-aware-economic-20260321-20260729` | 4 | 27 | 0.07 |
| `btc-regime-robust-accuracy-20260321-20260729` | 2 | 80 | 0.19 |
| `btc-residual-admission-20260321-20260721` | 3 | 18 | 0.01 |
| `btc-residual-admission-source-20260321-20260721` | 2 | 35 | 0.10 |
| `btc-saved-policy-20260421-20260720` | 1 | 6 | 0.00 |
| `btc-saved-policy-expanded-thresholds-20260421-20260720` | 1 | 6 | 0.00 |
| `btc-spot-l2-chainlink-candles-20260414-20260802` | 1 | 69 | 0.04 |
| `btc-strict-book-chronology-20260527-20260720` | 3 | 17 | 0.01 |
| Direct timestamp directories (unassigned lineage) | 2 | 14 | 0.01 |

## Frozen and exported artifacts

| Artifact directory | Files | GiB | Notable contents |
| --- | ---: | ---: | --- |
| `asymmetric-value-20260805` | 4 | 0.06 | `core_l2_price_hgb.joblib`, `core_oracle_price_hgb.joblib`, `core_price_hgb.joblib`, `evaluation-predictions.parquet` |
| `btc-chainlink-oi-paper-candidates` | 18 | 0.00 | `freeze-manifest.json`, `freeze-manifest.sha256`, `golden-features.metadata.json`, `golden-features.parquet` |
| `btc-core-20260421-20260620` | 5 | 0.00 | `freeze-manifest.json`, `freeze-manifest.sha256`, `holdout-access-2026-06-14-2026-06-21.json`, `model-summary.json` |
| `btc-core-20260421-20260720` | 21 | 0.00 | `freeze-manifest.json`, `freeze-manifest.sha256`, `holdout-access-2026-07-14-2026-07-21.json`, `model-summary.json` |

## Migration-relevant findings

- The 110 timestamped run directories are absent from the current canonical SSD root by name and timestamp. This tarball was outside the three legacy roots inventoried in the previous migration.
- The 84 CSV files are legacy tabular outputs. The repository standard requires training and backtest data stored in the canonical lake to use Parquet; the archive has not been transformed or admitted.
- `data/btc-early-price-value-20260414-20260802/core-source/2026-07-31.parquet.partial` is an incomplete source artifact. Preserve its partial status if this lineage is migrated.
- Three absolute symlinks under `data/btc-early-price-value-20260414-20260802/external/` point to old repository paths for Chainlink reference prices, L2, and candles. The links do not contain their targets; resolve source identities before migration.
- The archive includes model binaries and manifests, but their presence does not establish qualification, deployment authorization, or eligibility for a new `model/...` tag.
- The compressed archive and its checksum sidecar remain unchanged.

## Model artifact bundle transfer

On 2026-09-24, the four directories under the archive's `packages/btc-directional-model/artifacts/` were extracted and placed directly under `/Volumes/docker-data/capitonic-btc-directional-model/`:

- `asymmetric-value-20260805/`
- `btc-chainlink-oi-paper-candidates/`
- `btc-core-20260421-20260620/`
- `btc-core-20260421-20260720/`

The 48 model artifact files total 69,402,045 bytes. Every destination file was verified against its source SHA-256. The archive's `.DS_Store` file was excluded. `artifact-bundle-migration-20260825-fa05ed3.json` and `artifact-bundle-inventory-20260825-fa05ed3.sha256` in the canonical root record the source paths, archive identity, destination paths, sizes, and per-file checksums. The temporary extraction was removed. This transfer did not qualify or authorize any model for deployment.
