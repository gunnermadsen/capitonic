# BTC five-minute micro-edge training tournament

## Objective and scope

Train a diverse new generation of five-share models across each model’s full supported decision-time range, then measure where each model has edge. Each model may retain multiple profitable time buckets under one model identity. Compare the model-by-bucket results first, then combine complementary models using the existing router pattern. This supersedes the original one-bucket-per-model requirement. Seek higher net expectancy and profit factor, fewer losses at comparable frequency, better calibrated confidence, and lower loss-recovery burden. Do not require one model to dominate every metric.

This document is a plan only. It does not authorize training, deployment, process changes, database mutations, images, or merges. The user explicitly requested a new worktree for this tournament; the older tournament worktree remains intact as read-only evidence.

- Branch: `feature/btc-micro-edge-tournament`.
- Worktree: `worktress/btc-micro-edge-tournament`.
- Base: `integration-2026-09-14`, commit `e2b6f05532fba4c46b49b6daec971d06725640d6`.
- Source scope: June 7 through the latest available September 14 watermark, across all existing relevant datasets.
- Existing frozen snapshot ends at **2026-09-14 00:00 UTC**, so its last market starts September 13 at 23:55. This is not coverage of the whole September 14 calendar day. At execution time, include available September 14 markets only after they have resolved; record the actual watermark. Never label a partial day complete.
- Every subsequent tournament reuses this full source range. No permanent sealed block, progressively shortened training range, or automatic exclusion of previously evaluated dates.

## Verified data evidence and preparation

The existing combined Parquet panel, restricted to market starts on/after June 7, contains **1,041,143 rows / 28,139 distinct markets** through September 13 23:55 UTC. It has five-second checkpoints from 60 through 240. These are observed panel counts, not proof of complete raw-source coverage.

| Data product | Present rows | Markets with any presence | First / last observed market in this panel |
|---|---:|---:|---|
| Core Binance/path features | 1,041,143 | 28,139 | Jun 7 00:00 / Sep 13 23:55 |
| Oracle features | 1,041,143 | 28,139 | Jun 7 00:00 / Sep 13 23:55 |
| RTDS candles | 1,035,259 | 27,981 | Jun 7 00:00 / Sep 13 23:55 |
| RefPrice features | 844,888 | 22,997 | Jun 7 05:45 / Aug 26 23:55 |
| Open interest | 764,050 | 20,650 | Jul 3 00:05 / Sep 13 23:55 |
| Binance prints | 204,263 | 11,171 | Jul 22 00:00 / Sep 13 23:55 |
| Kraken spot/flow | 425,871 | 24,319 | Jun 7 00:00 / Aug 30 16:50 |
| Binance spot L2 | 547,770 | 14,829 | Jun 7 00:00 / Aug 1 23:55 |
| Kraken L2 | 650,605 | 17,585 | Jun 7 00:00 / Aug 19 23:55 |
| Both sides have non-null VWAP5 | 598,099 | 18,664 | Jun 7 00:00 / Sep 13 23:55 |

Presence uses the actual `has_*` flags. Non-null VWAP is not yet a freshness, depth, or executable-order check. Endpoint dates do not imply uninterrupted coverage. The larger unfiltered manifest counts must not be presented as June–September counts. In particular its Kraken L2 group summary overstates actual presence compared with `has_kraken_l2`.

Additional findings:

- The early causal TWAP panel still ends August 26 23:55, despite its parent run carrying a September 14 name. Its manifest records 541,339 rows / 21,735 markets starting June 7 05:50. Reusing it unchanged would repeat the missing early-entry confirmation problem.
- Tail manifests report no Binance spot L2 rows for August 27–September 13 and no Binance print rows for August 27–September 9; print rows resume September 10.
- SSD Kraken spot candle/print partition directories reach August 30. Kraken L2 raw dated directories also reach August 30, beyond the combined panel's August 19 feature endpoint. Directory presence is a recovery lead, not verified complete data.
- Models/results are in the previous worktree; source partitions also live on the SSD. Reference these immutable sources rather than duplicating the archive.

### Preparation contract for the future run

1. Inventory existing Parquet manifests and raw partition metadata for every group above, plus official resolved outcomes and causal TWAP state. Produce day-by-source-by-bucket counts, gaps, source/receive timestamps, label provenance and usable VWAP5 counts. Check overlaps for duplicates or conflicting values.
2. Recover omitted existing data into the shared preparation path: especially early TWAP beyond August 26 and Kraken L2 August 20–30. Inspect existing database sources only if archives lack the relevant coverage; use bounded date partitions and existing SQL contracts. No full-table diagnostic scans or new backfill infrastructure.
3. Maximize usable evidence rather than demanding uniform coverage. Do not globally drop dates, markets or models because an optional source is absent, a bucket has a small sample, or a coverage repair is incomplete. Build one canonical market/decision-time panel over the union of available histories. Keep long-history core rows when optional data are missing. Use actual availability masks and bounded causal age; never interpret unavailable L2 as zero imbalance or carry stale values indefinitely.
4. Train models with optional inputs on the full usable history using training-supported missing-value handling and availability indicators. Distinguish genuinely required inputs from optional enrichment; abstention is required only when the missing input makes that particular decision invalid. Train source-dependent challengers on their available support and evaluate their incremental value on matched rows against a source-free baseline. A model declaring Kraken/RefPrice/L2 optional can continue under its tested missing-data behavior; a model genuinely requiring absent evidence abstains only for that model; another eligible specialist may act. Do not shorten every model to the intersection of all feeds.
5. Separate prediction-only coverage from economic coverage. Official outcomes can supervise directional models where books are absent. Payoff fitting, policy selection and PnL require authentic five-share execution evidence. Do not invent prices or count missing evidence as profitable abstention.
6. Record per-source latest usable timestamps. If a raw source genuinely ends early, report that limit and use its full available subset; never claim every dataset covers the entire requested range. Missing core/labels/books exclude only affected examples/actions.
7. Preserve canonical direct Chainlink and PMData source identities; do not silently interchange them. Use source observations available by the decision time and the established shared RTDS semantics. Completed settlement averages are labels only, never early-entry inputs.

## Settlement cutover: preserve meaning, test its impact

Treat August 14 as the supplied cutover date; resolve the exact boundary from per-market settlement metadata rather than assuming every August 14 market switched at midnight. Keep official market outcomes as the economic truth on both sides. Ambiguous transition markets remain in the inventory and diagnostics, but cannot receive guessed labels.

Recommended default: pool the full range with causal, dimensionless return/distance/volatility features and an explicit settlement-regime indicator. Fit transformations and calibration only within each training fold. Feed-independent price/flow models may transfer well, but a changed settlement rule can still change their target near the boundary.

Run a bounded comparison on representative finalists:

- Pooled official-label control: original causal feature semantics plus regime indicator.
- Regime-aware normalization: comparable causal boundary distances scaled by contemporaneous volatility, with separate regime calibration when supported. Normalize feature meaning, not realized PnL or official outcomes.
- Optional auxiliary TWAP target: only where authentic/reconstructible inputs support it. Distinguish authentic from reconstructed targets and use this as auxiliary supervision, not replacement pre-cutover settlement labels.

Use a post-cutover-only diagnostic refit to measure transfer harm, but it does not replace full-range training or consume dates for future runs. If pooled transfer is harmful, compare nonzero pre-cutover weights rather than silently discarding history. Count weighting/normalization variants within the search budget. Report pre-cutover, transition, post-cutover and September results separately.

## Candidate population: diversity with a bounded budget

Reuse the existing training package, causal feature builders, calibration and execution replay. A candidate requires a specific economic hypothesis; different names or random seeds do not constitute different models.

| Recipe | Distinct source of possible micro-edge |
|---|---|
| Path persistence | Volatility-scaled continuation and boundary-cross stability |
| Reversal exhaustion | Pullback, acceleration reversal and failed boundary crossings |
| RefPrice dislocation | Causal reference/venue basis and convergence speed |
| Partial TWAP bridge | Observed partial average versus remaining settlement uncertainty |
| Cross-venue lead/lag | Binance–Kraken return and flow disagreement |
| Order-flow persistence | Signed flow, replenishment and near-term pressure |
| Positioning interaction | Open-interest changes conditional on price/flow state |
| L2 liquidity response | Imbalance, cancellations and replenishment, conditional on real presence |
| Q5 payoff-aware direction | Net value of a five-share entry rather than accuracy alone |
| Market-price residual | Calibrated outcome probability relative to current executable price |
| Loss-aware admission | OOF candidate loss probability/severity to avoid expensive failures |
| Within-bucket waiting | Enter now versus a later checkpoint inside the same allowed bucket |

Use at most two suitable estimator/objective variants per recipe: existing regularized histogram trees and a genuinely different supported compact baseline/objective. Do not force all recipes into both variants or add a dependency solely to increase the count. Admission and waiting models train on out-of-fold base predictions; no in-sample teacher leakage. Any within-bucket waiting label may use future outcomes for supervision, never as decision features.

Budget:

- Up to 24 distinct recipe/estimator screens, sharing cached features and fold definitions.
- Fit each recipe/estimator across all of its usable times, with elapsed time as an input; generate causal predictions at every supported checkpoint. Do not preassign a best bucket or make separate bucket fits the default. Weight rows so denser sampling does not let one market dominate training.
- Extend the initial field with meaningful RTDS, settlement-normalization, source, side or objective variants. Target 30–60 distinct trained models, with an operational ceiling of 96 configurations if measured resources and genuine hypotheses support it. Reuse predictions for bucket analysis; model-by-bucket cells are not extra models or fits.
- Evaluate every model in every supported bucket. There is no top-three-bucket cap, one-bucket limit, or forced one-winner-per-family pruning. Multiple disjoint buckets can be retained for the same fitted artifact.
- Separate model fitting from bucket admission/calibration. Bucket-specific policies may differ, but must share the same underlying model identity unless a separately declared retraining actually occurred.
- Exact configuration/data/fold hashes reuse existing results only when identical. Compare prediction hashes and trade-ledger overlap to collapse duplicates. Near-identical contenders (e.g. >98% same decisions) need measurable incremental value to remain separate.

Execution budget is calibrated using the first representative fits when training is authorized, not a benchmark now. Start with one training job, bound estimator threads, and permit at most two concurrent jobs after measuring memory. Reserve resources for trading services; stream partitioned Parquet, cache fold features once, cap native thread pools and resume completed fits by hash. Prioritize recipe diversity over extra seeds if the measured budget is tight. Report omitted configurations and actual CPU/wall time; do not promise an unmeasured runtime.

## Train models first, then discover their useful buckets

Use ten-second reporting/admission buckets `[0,10), [10,20), …, [290,300)` as the primary partition, matching the user's examples. Keep five-second diagnostics within them using actual supported cadence; wider 15/30-second summaries are descriptive and do not count overlapping observations as independent evidence. Seconds are elapsed since market open.

The present combined panel covers 60–240 only; the early builder has 30–150 checkpoints but limited dates. Recover other checkpoints from existing causal sources where available, while allowing supported models and periods to proceed. Mark unsupported cells unavailable rather than zero-performance. Do not demand every model cover every bucket. Within-bucket waiting requires real finer-cadence evidence; otherwise defer that recipe alone.

For every fitted model, generate out-of-fold predictions across all supported times and publish a model × bucket matrix containing net/stress PnL, PF, trades, W/L, compensation ratio, confidence calibration, source coverage and temporal support. Preserve positive, negative, low-sample and unavailable cells in the report. Missing features must not be used to retroactively remove losing trades.

Measure bucket opportunities independently: replay each bucket with at most one entry per market inside that bucket, even if an earlier bucket could have traded. A ledger that already took only the first trade in the whole market cannot reveal the later opportunities it suppressed. Independently replayed buckets may overlap in markets and are not additive portfolio PnL. Also report actual unrestricted-model trade attribution, then a fresh chronological replay restricted to the selected bucket set.

Retain every supported beneficial bucket, including disjoint buckets. Illustrative outputs (not measured results):

| Model identity | Eligible buckets |
|---|---|
| Model A | 60–69, 80–89, 110–119 |
| Model E | 40–49, 80–89, 120–129, 150–159 |

Model A remains one artifact referenced by three policies; Model E remains one artifact referenced by four. Both remain candidates in 80–89. The router resolves contention at execution time. No automatic retraining per bucket and no restriction to a single overall best bucket.

Select the bucket allowlist and admission rules on training-side rolling evidence, then freeze that set for the next outer evaluation block. The final artifact retains its selected multi-bucket allowlist. Report discoveries on the entire observed range as retrospective discoveries; do not imply that choosing buckets after seeing their results made those results prospective.

## Full-range reuse and honest evaluation

No permanently sealed results. Every tournament uses the same full June 7–September 14 source range and the same reproducible chronological folds. Previously viewed results remain reusable development evidence; repeated iteration must not be described as newly blind validation.

- Initial supervised warm-up: June 7–21. Generate rolling out-of-fold predictions from June 21 onward using only earlier resolved markets.
- Begin outer evaluation July 5 after two weeks of causal policy/calibration evidence; evaluate weekly to the actual September 14 watermark, with a final partial block when necessary. Earlier dates remain in training and rolling diagnostics. Report the warm-up coverage explicitly.
- Within each outer training prefix, choose recipes, bucket allowlists, hyperparameters, normalization, calibration and admission using only inner chronological predictions. Refit the selected model using that prefix, then replay the next outer block with the frozen policy. Purge any training label whose settlement was not available before the test boundary, and keep all rows of a market in one fold.
- June 7–July 5 is not discarded: it initializes training and selection, and is used in every later refit. July–September outer predictions are stitched once per market for a causal evaluation of the selection procedure.
- Final models refit on **all eligible June 7–September 14 data**, including dates used for evaluation earlier in the run. Final-fit training performance is labeled in-sample and is not substituted for the stitched causal results.
- Report both the causal selection-procedure score (bucket allowlists may differ between independently frozen folds) and the final bucket allowlist’s retrospective OOF score. The latter is selection-conditioned, not an unbiased estimate of a newly selected final artifact.
- A second tournament resets neither the source start nor the fit end to an earlier date. Reuse all dates, log newly attempted hypotheses, preserve prior ledgers, and disclose repeated-selection bias. New later data can eventually supply prospective evidence without withholding the requested historical range now.

## VWAP5 execution and ranking

Five shares for every candidate, baseline, policy selection, stress replay and router. No VWAP50 selection or capacity-derived ranking. Use the contemporaneous five-share ask ladder, actual applicable fees, the existing 0.005/share reserve and an additional 0.01/share stress debit. Check source contract consistency before adopting those inherited values. Apply at least the existing freshness/depth rules; model future-entry latency using actual later book observations when available. Never assume queue priority or fill missing depth.

Report actual net PnL, stressed net PnL, PF, gross wins/losses, trades, wins/losses, win rate, losses per 100 trades, trades per eligible day, calendar-market coverage, executable-market coverage, average entry second/price, drawdown, losing streaks, and calibrated Brier/log loss and reliability bins.

Define compensation ratio as **mean absolute losing trade / mean winning trade** (wins needed to recover an average loss; lower is better). Break-even win rate = ratio / (1 + ratio). When either side is absent, report undefined/infinite as appropriate; do not rank a handful of no-loss trades as certain superiority. Confidence improvement means better calibration and precision at matched coverage, not simply bigger predicted probabilities.

Support is an evidence label, not a universal exclusion threshold: report every usable model/bucket cell, including fewer than 100 trades, fewer than four weekly blocks, or fewer than 30 post-cutover trades. Use descriptive tiers such as promising/limited support and better-supported; do not discard training rows or hide candidates because a tier is unmet. Retain positive net/stress buckets as candidate edge locations and distinguish uncertain findings from robust evidence. Prefer repeated positive evidence when choosing the default router, but preserve low-sample contenders and report their marginal effect rather than automatically banning them. Inspect whether one day or regime supplies the whole gain; no quality quota. Use day-block bootstrap uncertainty and paired comparisons, with resampling restricted to already generated ledgers. Intervals do not erase repeated model selection.

Keep a Pareto shortlist: candidates that improve one objective without being clearly worse in all the others. Publish profit, loss-control and frequency leaders separately, plus matched-frequency comparisons. Primary ordering among supported candidates: post-cutover stress PnL per eligible calendar day, then full-range stress PnL/day, PF and compensation ratio. Report uncertainty and retain ties rather than arbitrary decimal precision. High PF alone cannot compensate for negligible support.

## Router evaluation

First report standalone specialists; then replay a router using only their individually selected buckets. Default to at most one entry per market across the router. Earlier fills consume later opportunities, so do not sum standalone ledgers. A skip in one bucket leaves later buckets eligible.

Allow several models to qualify for the same bucket and one model to qualify for several buckets. Keep all supported model–bucket associations in the catalog. At a decision timestamp, choose at most one order using calibrated stressed expected value and training-side incremental router evidence, then a deterministic tie break. Do not duplicate model loading or count the same order twice. Permit no-trade and unfilled buckets. Compare a chronological eligible-first router with one bounded opportunity-cost variant that may reserve a market for a later bucket based solely on information available now. Train any reservation rule inside folds. Report incremental trades, displaced trades, lost later opportunities, source-related abstentions and PnL attribution by bucket.

Benchmark against prior four-tournament specialists at five shares on identical covered markets, including terminal PF>2 models. Frozen historical artifacts are only genuinely out-of-sample after their training/selection cutoff; otherwise label replay as contaminated descriptive evidence. For fair full-history comparisons, refit the baseline recipe inside the same chronological folds. Do not compare the new VWAP5 totals directly with historical VWAP50 PnL.

## Deliverables and stopping point for the eventual run

Deliver one coverage manifest; a unique candidate registry with hypotheses and exclusions; Parquet OOF predictions/trade ledgers; candidate and router metrics by fold, regime, bucket and source availability; model bundles with exact hashes, feature contracts and multi-bucket allowlists and policies; and a concise numerical report with the Pareto shortlist and matched baseline comparisons. Record unsupported inputs and export readiness separately from model performance.

Keep new outputs inside this worktree; reference existing immutable SSD sources. No migrations, new services, source deletion, training-process creation or production deployment. Any later runtime export follows UMR parity, the shared RTDS repository and process_id scoping, preserving existing capital, accounting and market-safety checks. No persistent stop-trading behavior for transient missing inputs.

Stop after the bounded candidate field, model-by-bucket comparison, causal router replay and final full-range refits are reported. Do not launch another tournament automatically.

## Evidence paths used for this plan

All relative project paths below resolve from the repository root. Existing results remain read-only references.

- `worktress/time-bucket-specialist-tournament/packages/btc-directional-model/data/btc-full-coverage-vwap-admission-20260321-20260914/vwap-admission-panel.parquet` and its `vwap-admission-panel-manifest.json` (snapshot SHA-256: `f11e3bc597dfd546d667c5f610791fd48feab0015217c64278fc06e46612bffa`).
- Same data root: `btc-full-coverage-vwap-admission-tail-20260827-20260914/{source,complete-source,auxiliary-source,spot-l2,kraken-feature}-manifest.json`.
- Same data root: `btc-time-bucket-early-source-20260321-20260914/{source,preparation}-manifest.json`.
- Previous worktree training-results: `btc-5m-time-bucket-specialist-tournament-20260321-20260914/20260914T171438Z/early-causal-panel-manifest.json` and `source-contract.json`.
- Prior four run IDs: `20260913T215700Z`, `20260914T002735Z`, `20260914T145421Z`, `20260914T171438Z`; their model hashes and metrics are the baseline identities. Fifth tournament excluded from this baseline set.
- SSD roots: `/Volumes/docker-data/kraken-spot-data`, `/Volumes/docker-data/kraken-data/spot-l2/cryptohftdata/kraken_spot`, `/Volumes/docker-data/polymarket-bot/model-training-archive`, `/Volumes/docker-data/archives/polymarket-bot-worktrees`.
- Reuse `packages/btc-directional-model/src/btc_directional_model/{time_bucket_source_preparation,time_bucket_specialist_tournament,micro_bucket_router_tournament}.py` and existing replay/feature modules. Any future extension stays in this package.
- Runtime compatibility: `docs/unified-model-runtime/README.md` and `docs/unified-model-runtime/rtds-repository.md`.

Planning verification used local manifests, a narrow Parquet column read and SSD directory metadata; no database queries or training were executed. Full raw-source completeness and September 14 intraday coverage remain preparation checks, not asserted facts.

## Previous training code and SSD archive: inspected behavior

- `packages/btc-directional-model/src/btc_directional_model/time_bucket_specialist_tournament.py`, `_slice`, `_fit_model` and `run_tournament`: the prior specialist runner slices fitting and evaluation to a candidate's assigned bucket before fitting. Its estimator is a histogram gradient-boosting regressor with a chronological logistic calibration tail. This is reusable fitting machinery, but its preassigned-bucket orchestration does not implement the clarified train-first, discover-many-buckets design.
- `packages/btc-directional-model/src/btc_directional_model/historical_signature_replay.py`, `_bucket_results`: groups archived ledgers by tournament/run/candidate and reports each identity across 5/10/15/30-second buckets. This is the closest existing analysis pattern. Its `_screened` filter requires 30 trades, positive net/stress PnL, PF above 1 and repeated evidence or 100 trades. Reuse the summaries, but show unscreened cells too; these descriptive cutoffs are not mandatory data/training exclusions.
- That replay is explicitly read-only and cannot infer decisions absent from archived trade ledgers. New model evaluation must retain prediction/opportunity ledgers so suppressed later-bucket opportunities can be evaluated directly.
- `packages/btc-directional-model/src/btc_directional_model/historical_aggregate_tournament.py`: chooses historical donors, then retrains them separately inside ten-second buckets. It is not evidence that a shared fitted model was evaluated everywhere. Do not copy this bucket-first donor restriction into the new tournament.
- Verified SSD archive root: `/Volumes/docker-data/archives/polymarket-bot-worktrees/time-bucket-specialist-tournament`. Its `archive-manifest.json` records original paths, byte sizes and SHA-256 values. Archived working outputs live under `packages/btc-directional-model/runs/`, and source caches under `packages/btc-directional-model/data/`; the archive does not use a `training-results/` directory. For example, the original run is `runs/btc-time-bucket-specialist-tournament-20260321-20260901/20260913T215700Z/` with `metrics.json`, `report.md`, `tournament.joblib` and checkpoints. Committed summaries also exist in repository `training-results/`. Resolve sources through the archive manifest instead of assuming the old working cache path still exists.

Revision scope: plan only; training code and archived artifacts remain unchanged.

## Authorized execution record

The user subsequently authorized end-to-end training in this existing worktree, with no merges, branches, worktrees, images, deployments, database mutations, or infrastructure changes. The execution modules are tournament-local additions in the existing Python package; shared trainers, extractors, SQL files and trading code remain unchanged.

- Run identity: `20260915T030000Z` under `packages/btc-directional-model/runs/btc-micro-edge-20260607-20260914/`.
- Field: 32 new hypothesis/RTDS/objective configurations plus three distinct matched historical refits. Two selected historical identities have identical feature/estimator/bucket recipes and share one refit; all four origins are retained in `config/btc-micro-edge-baselines.json` with verified artifact hashes.
- Recovered the full August 27 raw partition from the existing tail cache. September 14 contributes 285 resolved valid markets through the requested day’s end.
- Existing stored orderbooks supplied 95,613 additional rows at previously unrequested checkpoints before 30 seconds and after 240 seconds. Queries reused the established capacity extractor with bounded daily timestamps, read-only connections, no parallel database workers, and a 60-second statement timeout. Larger ladders are optional input context; every fill, fee calculation, reserve and selection uses five shares.
- New histogram estimators use 60 iterations, 15 leaves, minimum leaf size 100, learning rate 0.07, L2 regularization 3 and 63 bins. Historical controls retain their archived estimator parameters. Compact logistic controls use training-only imputation/scaling. After measuring resource use, two disjoint fitting jobs use two native threads each and nice priority 10. The machine has 12 CPUs and 36 GiB RAM; observed combined worker memory remained below 7 GiB during the wider fits. Initial priority-setting restrictions were corrected before sustained concurrent fitting.
- Optional-source gaps remain missing. Waiting admission requires causal OOF teacher predictions, so its auxiliary fit begins after the initial warm-up; its underlying directional model still refits the full June 7–September 14 range. This necessary supervision boundary is disclosed in per-artifact fit counts.
- The supplied August 14 cutover remains a date-level indicator, not a verified exact transition timestamp. Official resolved outcomes are retained; transition-day diagnostics are separate. A bounded post-cutover-only refit is a transfer diagnostic and does not replace any full-range final model.
- Baseline comparisons refit archived features and estimators using the new common chronological OOF calibration/admission, official labels and VWAP5. They are matched controls, not a reproduction of earlier sealed selection or mixed-quantity totals.
- Resume validates code/input identities and artifact hashes. Each fold saves its fitted estimator, OOF predictions and completion metadata atomically. Final artifacts are reloaded for prediction parity. Only completed actual training artifacts receive model provenance tags after their references are committed.

Execution entry points (run from this worktree with the existing package virtual environment and `PYTHONPATH=packages/btc-directional-model/src`):

1. `python -m btc_directional_model.micro_edge_data --snapshot` — last-day read-only extraction, already completed.
2. `python -m btc_directional_model.micro_edge_execution` — checkpointed extraction of omitted book checkpoints, already completed.
3. `python -m btc_directional_model.micro_edge_data` — hash-validated daily source preparation.
4. `python -m btc_directional_model.micro_edge_tournament` — resume/run the registered field, full-range final refits and reports.

Detailed numerical evidence belongs to the completed run reports, not the small verification fits. Verification artifacts are clearly separated under `verification/` and excluded from tournament rankings and model provenance tags.
