# Model calibration result — October 3, 2026

**No candidate qualified for deployment.** Database model selections and `infra/processes` remain unchanged. No bot image build or deployment occurred.

Branch: `training/model-recalibration`, based on `integration-2026-10-02` at `1c2fb1a06491d8440bc8fc79c3288b0faf7f77ea`. Fitting implementation: `f531e034` (full revision in artifact provenance).

SSD run: `/Volumes/docker-data/capitonic-btc-directional-model/model-recalibration-20261003T154512Z`. Detailed results: `metrics/calibration-results.json`, `metrics/policy-replay.json`, `metrics/production-validation.json`; checksums: `manifests/canonical-inventory.sha256`.

## Inventory and boundaries

Eight enabled development processes and two enabled production processes were inspected. They reference seven directional artifacts (including the live-pilot version) and two loss-risk artifacts. Shared process forecasts were not treated as independent evidence. No trading, ingester, reconciliation, schema, or runtime Rust changes were made.

## Frozen fitting and independent evaluation

Conservative-family corrections used the matching frozen model’s August 24–September 13 archived high-confidence, cheap-contract opportunities (232 markets). Extended specialist used its archived August 20–25 programmatic 60–89-second trades (95 markets). Model source hashes and shared conservative tree payloads were checked. VWAP used its frozen final model after original fitting/calibration, September 15–21; only 34 markets met the fixed high-confidence, cheap-contract fitting condition.

Recent runtime forecast evidence available in the bounded export was October 2–3, ending October 3 at 15:00 UTC. No fitting used those outcomes. It is chronological holdout evidence, but previous investigation already inspected recent performance; it is not an epistemically pristine prospective test. One earliest causal forecast per market evaluates broader probability quality; separate policy projection evaluates all recorded opportunities.

| Candidate family | Holdout markets | Brier before | Brier after | Result |
|---|---:|---:|---:|---|
| btc-5m-conservative-selective-paper-20260917-calibration-20261003-v2 | 468 | 0.2238 | 0.2328 | Failed probability qualification |
| btc-5m-conservative-selective-confidence-075-paper-20261001-calibration-20261003-v2 | 468 | 0.2239 | 0.2329 | Failed probability qualification |
| btc-5m-conservative-selective-confidence-080-paper-20261001-calibration-20261003-v2 | 468 | 0.2238 | 0.2328 | Failed probability qualification |
| btc-5m-conservative-selective-vwap-capacity-q5-paper-20260924 | — | — | — | Insufficient fitting support: 34 markets, minimum 50 |
| btc-5m-extended-specialist-official-umr-20260902-confidence-070-calibration-20261003-v2 | 464 | 0.2152 | 0.2205 | Failed probability qualification |
| btc-5m-conservative-selective-development-live-pilot-20260921-v1-calibration-20261003-v2 | 468 | 0.2241 | 0.2331 | Failed probability qualification |

Production paper independently worsened from Brier 0.2241 to 0.2331. The production live-pilot result is included in the table.

The conservative-family correction had temperature slope approximately 0.374928. The frozen regression specialist’s raw-probability sigmoid slope was approximately 3.077092. These corrections reduced confidence in the historical trading subset but worsened Brier and log loss on broader recent forecasts. This rejects these specific scalar candidates; it does not establish that every possible conditional calibration would fail.

## Unchanged-policy projection

The conservative baseline and both lower-confidence variants projected zero candidate trades, versus baseline counts of 13, 63 and 28, respectively. Extended specialist projected zero trades in its base process and only one or two in its risk-labelled processes; those small candidate projections had negative stressed PnL. These are diagnostics with causal snapshot prices, a five-share quantity and a 0.07 fee assumption, not actual fills or queue simulation. They cannot authorize deployment.

The high-precision loss-veto artifact has coupled learned admission. Complete derived-feature replay is required before changing its directional probability; unsupported partial export is refused. Both loss-risk artifacts are qualified for later entry windows, but the enabled processes evaluate entries at seconds 60–85. Recent exported risk evidence is outside those qualified buckets and cannot qualify an in-bucket correction. These three artifacts remain uncalibrated and unchanged.

## Artifact and release state

Five diagnostic `-calibration-20261003-v2` model payloads were actually generated on the SSD. They are explicitly unqualified and are not copied into the bot runtime catalog. Earlier `v1` diagnostic outputs were superseded after a stable-loss objective correction and are excluded from deployment; five focused tests verify the final fitter.

Annotated `calibration/<new-model-key>/git-<full-result-commit>` tags record only the verified v2 artifacts and their failed qualification. Original model artifacts and tags are preserved. No accepted model version exists to select through the API, so database and infrastructure selections must not advance.

Bot local version allocation was inspected with the existing CI `--next-version` command. No image inputs changed: Python tooling and documentation are outside the bot image inputs, and unqualified artifacts remain on the SSD. A bot local/image tag must not be minted until a selected qualified runtime bundle changes those inputs and an actual build is performed. See [calibration procedure](model-calibration.md).

## Verification and limitations

Focused calibration tests: five passed. Ruff checks and formatting passed. Read-only exports checked forecast snapshot identity and causal timestamps; source and output hashes are retained. Probability and projected-policy qualification failed. Rust runtime/golden-vector admission and deployment checks were not claimed or run for rejected artifacts.

The requested production/development activation, matching `infra/processes` update and new bot image are blocked by failed qualification, not by missing deployment authorization. No release, integration promotion, database mutation or process restart was performed.
