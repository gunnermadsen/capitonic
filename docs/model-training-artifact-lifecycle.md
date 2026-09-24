# Model training artifact lifecycle

This document defines where BTC directional model training work lives, what may enter Git, how completed runs are archived, and how a selected model is admitted into the Unified Model Runtime (UMR).

## Authority and scope

`AGENTS.md` contains the mandatory repository controls. This document contains the operating procedure for `training/...` worktrees and `packages/btc-directional-model`.

The canonical SSD root is:

```text
/Volumes/docker-data/capitonic-btc-directional-model
```

Do not start a training, backtest, or data-preparation run when this root is unavailable. Do not substitute a repository, worktree, `/tmp`, or another disk without explicit user authorization.

Existing paths under `polymarket-bot/artifacts`, `polymarket-bot/model-training-archive`, and `archives/polymarket-bot-worktrees` are legacy locations. Existing references remain valid until their artifacts are deliberately migrated; new runs use the canonical root.

## Run directories

Create one directory per model run. Use a lowercase model name followed by a UTC timestamp:

```text
<model-name>-<YYYYMMDDTHHMMSSZ>/
```

Use only the directories required by the run:

```text
<model-name>-<YYYYMMDDTHHMMSSZ>/
├── code/
├── inputs/
├── datasets/
├── models/
├── predictions/
├── backtests/
├── metrics/
├── diagnostics/
├── logs/
└── manifests/
```

- `code/`: exploratory or run-local Python that has not yet been promoted into the package.
- `inputs/`: source manifests, immutable input references, and checksums.
- `datasets/`: Parquet training, calibration, validation, and holdout data.
- `models/`: checkpoints, fitted candidates, calibration objects, and exported artifacts.
- `predictions/`: fold, holdout, and qualification predictions.
- `backtests/`: raw ledgers, trades, and detailed backtest outputs.
- `metrics/`: machine-readable metrics and concise result tables.
- `diagnostics/`: plots, audits, and investigation outputs.
- `logs/`: execution logs.
- `manifests/`: run identity, provenance, checksums, and lifecycle state.

Generated artifacts remain on the SSD. Do not place them in the repository and then archive them later.

## Run manifest

Every run must have a manifest or `README.md` under `manifests/` recording:

- model name and UTC run ID;
- training branch, worktree, and Git commit;
- purpose, command, configuration, and environment identity;
- training, calibration, validation, and holdout intervals;
- input datasets, immutable source identities, and SHA-256 checksums;
- output artifacts and SHA-256 checksums;
- metrics, qualification decision, and known limitations;
- promoted source destinations and final Git commit when applicable;
- selected deployment artifact when applicable; and
- active, completed, archived, rejected, or deployed status.

A concise report committed to Git must point to this manifest and identify the run without embedding machine-specific copies of generated data.

## Training workflow

1. Create or reuse the model objective's `training/<name>` worktree.
2. Create the timestamped SSD run directory before generating data.
3. Direct extractors, trainers, evaluators, and backtests to the run directory. Database extraction remains bounded and read-only unless a separately approved migration applies.
4. Write tabular training and backtest data as Parquet. Keep checkpoints, predictions, plots, logs, and raw ledgers on the SSD.
5. Update the manifest as artifacts are produced and verify checksums before qualification.
6. Keep related candidate variations under the same training objective and worktree. Use separate timestamped run directories rather than additional worktrees.

## Preparing a training branch for integration

Before requesting integration:

1. Stop writers and verify the completed run manifest and artifact checksums.
2. Move maintained Python modules from the run's `code/` directory into `packages/btc-directional-model`. Do not retain a second maintained copy on the SSD; record the destination paths, source hashes, and final Git commit in the manifest.
3. Keep exploratory scripts on the SSD or delete them. Do not promote one-off probes or scratch code.
4. Keep raw training results, model candidates, predictions, and backtest ledgers on the SSD. Commit only concise reports, configurations, tests, and provenance pointers.
5. Verify that no Parquet, dataset, checkpoint, raw prediction, log, or large model artifact is staged for Git.
6. Run the package's required tests and reproduce the reported summary from the archived manifest and artifacts.

Moving maintained modules into Git freezes the implementation used to build the model. The run manifest binds that implementation to the archived inputs and outputs.

## Archiving older artifacts

Archive completed and superseded runs under the same canonical root without changing their checksums or manifest identity.

- Move generated data and inactive artifacts out of repository and worktree directories into their corresponding SSD run directory.
- Preserve manifests, hashes, qualification decisions, and deployment history.
- Do not rewrite Git history merely to remove files from the current tree.
- Do not remove a deployed runtime bundle until its replacement and rollback compatibility are verified.
- Treat legacy paths as read-only provenance until a deliberate migration validates and updates every reference.

## Deployment admission

Ordinary training integration does not move model artifacts into the repository. When the user explicitly authorizes a model for paper or live deployment:

1. Select one immutable SSD artifact and verify its SHA-256 against the run manifest.
2. Produce the runtime manifest, feature identity, golden vectors, and qualification evidence.
3. Copy only the selected deployable bundle into `packages/btc-directional-model/runtime-models/<model-key>/` when it is suitable for Git. Large artifacts remain outside Git and must be staged from an immutable SSD path or artifact registry by the authorized image build.
4. Record the source run, training commit, artifact checksum, feature checksum, adapter identity, qualification status, and deployment status.
5. Run source-to-runtime prediction parity and all applicable UMR verification.
6. Create a `model/...` provenance tag only when the workflow produced and verified a new immutable model artifact.

Exporting, copying, activating, or packaging an existing artifact does not constitute a new training run and does not mint a new model tag.

## UMR adapter boundary

A model adapter may:

- validate and load the immutable runtime bundle;
- bind canonical UMR features to model inputs; and
- translate model outputs into the existing prediction and admission contracts.

An adapter must not recreate RTDS history, candles, feature repositories, caches, persistence, admission policy, risk controls, process identity, execution, accounting, settlement, recovery, or observability. Extend the existing adapter interface only when a genuinely new mathematical capability cannot be represented by a supported adapter.

Before deployment, verify feature, prediction, admission, process-scoping, restart-recovery, and frozen-vector parity. Follow `docs/unified-model-runtime/integration.md` for the runtime integration procedure.
