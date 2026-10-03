# VWAP Q5 confidence correction — October 3, 2026

Prepared one new immutable version of the VWAP Q5 conservative selective model. No model predictor retraining, trading-policy change, runtime Rust change, ingester change, reconciliation change, database mutation, image build or deployment occurred.

Process: `8e8f7724-0dbd-46d9-b913-4bf3418d293f` (`BTC 5m conservative selective VWAP capacity Q5 paper`). Worktree: `worktrees/model-recalibration`; existing branch: `training/model-recalibration`. Producing calibration implementation commit: `fc1e56b8` (full revision retained in model provenance).

## Model and evidence

Source model: `btc-5m-conservative-selective-vwap-capacity-q5-paper-20260924`, SHA-256 `61c895293a6532e63d9dd6b6b6e1e58869b825a218523c2d181b6f6da1a4a54e`.

New model: `btc-5m-conservative-selective-vwap-capacity-q5-paper-20260924-calibration-20261003-v1`, SHA-256 `0003781497b587713b310cfb0a33b4af8227a8c7b0f2c25374e375afad4b25ad`.

The positive temperature multiplier is `0.31894062182560295`, composed with the original Platt calibration slope and intercept. Trees, features, cadence, reliability penalty, model contract, execution quantity and admission policy are identical. Directional predictions remain identical. Deployment scope remains `paper_only`, `production_qualified=false`, `live_capital_allowed=false`. The original model is retained.

Canonical SSD run: `/Volumes/docker-data/capitonic-btc-directional-model/model-recalibration-vwap-q5-20261003T165938Z`. Detailed evidence: `metrics/evaluation.json`, `models/<new-model-key>/calibration-result.json`, `manifests/run-status.md` and `manifests/inventory.sha256`. Generated datasets remain Parquet on the SSD.

Fit: 34 independent September 15–21 markets, after original predictor fitting and calibration, using the exact locked VWAP classifier's high-confidence, cheap-contract opportunities. Original joblib SHA-256: `65d41630a2211cf2979e5fa952d34fb494bbc85c7027bc100d35b96bdbe86453`. Every fitting probability was reproduced from that classifier/calibrator. The explicit `--minimum-fit-markets 30` override allows this single-parameter fit; the default remains 50. This changes offline evidence handling, not trading admission.

Evaluation: 33 later settled original-model trades, September 25–October 2, exported with an October 3 15:00 UTC cutoff. Probabilities and causal feature timestamps come from stored order metadata; official outcomes come from interval markets and credited paper settlements. One trade per market, exact source model hashes, no fit/holdout overlap and feature timestamps preceding fills were verified. Prior diagnostics examined recent results, so this is chronological holdout rather than a pristine prospective test.

| Settled-trade metric | Original | Corrected |
|---|---:|---:|
| Average confidence | 92.36% | 69.00% |
| Actual directional accuracy | 66.67% | 66.67% |
| Overconfidence gap | 25.70 percentage points | 2.34 percentage points |
| Brier error | 0.28752 | 0.22225 |
| Log loss | 0.91186 | 0.63667 |

This materially improves confidence alignment on the observed traded cohort. It does not increase win rate or establish better profitability. The 95% paired-market bootstrap Brier-change interval is `[-0.13680, +0.00538]`: it includes zero, and 33 markets provide limited evidence. The tool's conservative probability-pass diagnostic remains false; this is reported without requiring the existing model to repeat production qualification.

## Broader probability and trading effects

On 468 earliest runtime forecasts per market from October 2–3, broader Brier error worsens from `0.22349` to `0.23610`. Confidence falls from `61.35%` to `53.85%` against `64.53%` actual accuracy. This global scalar correction therefore creates additional underconfidence outside the selected trading cohort; it is not an improvement across all forecasts.

All 33 historical traded forecasts would fall below the original admission confidence threshold after correction. A projection across all retained runtime opportunities finds 14 original model-admitted rows in 9 markets and zero corrected model-admitted rows. These are model-admission diagnostics, not simulated fills or prospective trade counts. Trading safety thresholds are unchanged. The correction could make this process quiet even though inference remains healthy.

## Prepared selection and pending database action

The runtime bundle contains `model.json`, `manifest.json` and 256 authentic reference `golden-vectors.json` rows. The existing `infra/processes/btc-5m-conservative-selective-vwap-capacity-q5-paper-20260924.json` now pins the new model key and SHA in its selection and model metadata. Process identity, feature SHA, policy, quantity, and inactive template defaults remain unchanged. This file describes the prepared target, not the currently running database state.

The database still selects the original model. The established `PATCH /admin/trading-processes/<process_id>` API rejects model reconfiguration while this BTC process is enabled; its active selector-only exception allows source-selector/playbook-version changes, not model changes. See `packages/polymarket-bot/src/application/control_api.rs::update_trading_process` and `process_definition.rs::selector_only_config_change`. Stopping the process or installing a new image would contradict this execution's explicit no-deployment instruction, so neither occurred. No alternate database mutation path was used.

At an authorized deployment, obtain the current definition through `GET /admin/trading-processes/8e8f7724-0dbd-46d9-b913-4bf3418d293f`. Preserve its full current configuration and metadata, changing only:

- `config.raw.btc_realtime_paper.strategy.decision_strategy.models[primary].selection.model_key` to the new key;
- that selection's `artifact_sha256` to the new SHA;
- `metadata.model_key` and `metadata.model_artifact_sha256` to the same identity.

Here `[primary]` means the member with `member_id="primary"`, not a numeric array index. Preserve feature schema SHA `5dceedc374621f50e844ddf43cea5f58068c4dc70c36169c037dbb2691c7f39a` and all other fields. Submit the resulting `{config, metadata}` through the existing process PATCH only in the release-controlled inactive interval, after the exact bundle is available to the bot. Preserve and restore the process's prior enabled intent using its established lifecycle API. Do not run the inactive bootstrap template against the running process, and do not issue SQL updates. None of these activation actions were executed here.

## Tags and local image version

The annotated provenance tag is `calibration/<new-model-key>/git-<full-result-commit>`, targeting the commit that first records this newly executed artifact. It records the SSD run, producing commit, source/model/evidence hashes, measured improvement and limitations, and `deployment=not-deployed`. No duplicate `model/` tag is needed.

Adding this runtime bundle changes bot image inputs. A future authorized local build therefore needs a fresh bot local version with the full `POLYMARKET_GIT_REVISION`, plus actual `image/polymarket-bot/v<version>` and image-ID tags from local CI. Production's `v3.2.5` closes that base; the expected next open candidate base is `3.2.6`, subject to origin/registry checks at build time. This branch still carries the old release pin, so the existing CI build guard requires a newer eligible base before building changed inputs. Release allocator/pin changes were not added to this model-only task. No local image tag, Helm pin, release tag, golden tag or checkpoint was fabricated, and no image was built.

## Verification

Seven focused calibration tests and two existing conservative export checks passed; Ruff checks and formatting passed. All 256 corrected reference outputs match the frozen sklearn predictor with its composed calibration to maximum probability error below `1e-12`. Model/manifest/golden checksums and equality of every unchanged model field were verified. The existing Rust test `every_umr_package_uses_process_quantity_and_preserves_reference_predictions` checks actual runtime loading, predictions, admission and quantity bindings for every bundled UMR artifact, including this version; it passed (one test, 428 unrelated library tests filtered).
