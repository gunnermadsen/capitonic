# Model calibration

Calibration fits a small probability correction with the predictor frozen. It does not retrain features or trees, tune admission policy, or grant production qualification. Realtime execution, ingestion, reconciliation, persistence, identity and capital controls remain unchanged.

## Branch and data ownership

Use one `training/model-recalibration` branch from the latest integration tip and the domain worktree `worktrees/model-recalibration`. Link runtime environment files to the main worktree. Keep all datasets, predictions, calibration candidates and detailed results under `/Volumes/docker-data/capitonic-btc-directional-model/model-recalibration-<UTC timestamp>/`. Use Parquet for tabular evidence and maintain `manifests/run-status.md` even for failed or partial runs. Follow [artifact lifecycle](model-training-artifact-lifecycle.md).

Inventory enabled development and production processes by `process_id`, actual loaded model hashes, and policies. Deduplicate shared predictors and repeated process forecasts. Directional, admission and loss-risk probabilities require separate evidence. Out-of-bucket risk scores do not qualify a risk model within its trained bucket.

## Offline fitting

The maintained entry point is `python -m btc_directional_model.model_calibration`, run with the package Python environment from this worktree. Supply `--model`, `--fit`, `--holdout`, `--output`, `--model-key`, `--commit`, and `--method`. Outputs must reside in the canonical SSD root. Evidence columns are `market_id`, `window_start`, `feature_as_of`, `probability_up`, and `outcome_up`. Verify source hashes and prediction semantics before invocation.

Fit and holdout must be chronological and market-disjoint. The fitter selects one earliest forecast per market, requires at least 50 fit markets and both outcomes, and uses one positive correction parameter. Keep at least 50 independent holdout markets. Freeze intervals and data-selection rules before inspecting candidate results. Never split repeated predictions of one market across fitting and validation.

Existing time-bucket classifiers support a logit-temperature correction composed with their existing Platt mapping. Frozen regression outcomes without learned admission or temporal heads support a sigmoid of their raw probability estimate, embedded algebraically through the existing submodel baseline and leaf weights. Tree splits, native-missing paths and source artifacts remain unchanged. Learned admission and temporal components require complete derived-feature replay; the exporter refuses unsupported partial correction.

A probability pass requires a strictly negative upper bound on the paired-market bootstrap 95% Brier difference, improved holdout log loss, and reduced absolute confidence gap. Market bootstrap does not capture all temporal correlation. Assess longer chronological evidence and trading-policy subsets before admission. The tool always records `deployment_qualified=false`: statistical fitting alone cannot approve trading.

Before deployment, replay the unchanged policy over complete causal opportunities, including rejected forecasts. Require supported outcome/admission semantics, Python/Rust golden-vector parity, useful holdout trade support, positive stressed expectancy and no demonstrated economic regression against the frozen baseline. A candidate that removes every trade fails economic qualification. Do not relax thresholds to make a calibration deployable. Missing coverage or insufficient evidence means retain the existing model.

## Model versions and tags

Use a fresh model key `<existing-model-key>-calibration-<YYYYMMDD>-v<N>` for each immutable correction. Never overwrite the old artifact. Record source predictor/model hash, calibration method and coefficients, source data hashes, fit and holdout periods, producing commit, qualification and deployment state. New manifests and golden vectors must pin the new payload hash and retain the feature schema.

Calibration provenance uses an immutable annotated Git tag `calibration/<new-model-key>/git-<full-commit-id>`. Its target must first record or unambiguously reference the actual fitted artifact. Include the SSD run, artifact SHA-256, producing commit, source identity, fitting/holdout evidence, qualification and deployment state. This prefix replaces `model/` for this calibration workflow; it does not confer golden or production status. Do not tag an unexecuted fit or a copied existing model.

## Bot versioning and activation

Only selected, deployment-qualified bundles enter `packages/btc-directional-model/runtime-models/<new-model-key>/`. Datasets and diagnostic candidates stay on the SSD. Because runtime bundles are bot image inputs, prepare a new bot candidate through `scripts/local-image-ci.sh polymarket-bot --build` after a clean committed input change. Let local CI allocate the next eligible `<MAJOR.MINOR.PATCH>-local.N`, checking origin tags, production pins and immutable registry versions. Embed the full `POLYMARKET_GIT_REVISION`. Calibration tags do not replace `image/`, Helm, checkpoint, golden or release provenance. Do not build ingester or migration images for calibration.

A source or documentation change outside bot image inputs does not justify minting an application image version. Never create a local image tag claiming a build that did not occur.

Deploy the exact qualified bot candidate under the existing release rules. Record rollback images and process selections. Patch only model keys/checksums through the established trading-process API; never update process rows through SQL or migrations. Preserve `process_id`, enabled intent, order type, quantity, source selectors and safety parameters. Match the new selections in `infra/processes` while preserving those templates' inactive defaults. Do not run bootstrap templates to disable an already-running process.

Verify selected runtime hashes, fresh inference, automatic restart recovery, unchanged safety checks, reconciliation health and required advancing feed timestamps. Production activation follows accepted development evidence and the established production release workflow. Failed calibration or missing qualification leaves both database and infrastructure selections unchanged.
