# Implementation
- The user’s requested outcome and any agreed implementation plan define the scope. Complete and verify that outcome without adding optional cleanup, redesigns, or unrelated changes.
- A plan transferred to another task or thread is context only, not authorization to implement it; the receiving task must assimilate it, define the exact narrow file and behavior scope, and wait for explicit implementation authorization in that task.
- Before implementing functionality, identify and reuse the existing module, contract, registry, persistence path, instrumentation, or execution path that owns the capability.
- Make the smallest coherent change that satisfies the request. Necessary adjacent changes to compilation, tests, or directly affected contracts are permitted when they do not materially broaden the requested outcome; report them after implementation.
- Do not extract, reorganize, rename, or rewrite existing logic solely for structure or testability; if the necessary diff materially exceeds the named behavior or owning files, stop and obtain explicit authorization for the expanded scope.
- Obtain explicit authorization before making destructive changes, changing unrelated behavior or data, materially expanding the requested outcome, or introducing a parallel or replacement system.
- If the existing system cannot satisfy the request without one of those changes, stop and explain the specific incompatibility.
- Do not create, modify, rename, or delete files under `.github/workflows/` without explicit user authorization naming the workflow file and requested behavior; approval of a broader plan or adjacent release change does not authorize workflow changes.
- Use `trading_processes.process_id` as the canonical identity for trading-process ownership and scoping.

## Golden Rules
- Rust changes must avoid material regressions in latency, memory use, CPU use, database load, availability, and recovery behavior. Measure or benchmark when a change plausibly affects a performance-sensitive path.
- Do not name artifacts, branches, files or code comments based on phases or stages of an implementation. use domain specific naming for the issue or feature being addresses. I dont' want to see 'phase X' in git history, code files, branches, or file names or code comments. 

## Always-On Trading Liveness
- Preserve the durable enabled or disabled intent of every trading process across deployments, service restarts, reconnects, and other ordinary infrastructure lifecycle events. Ephemeral runtime state must not override durable process configuration.
- Transient failures may block only the unsafe action while the condition exists. They must not terminate the process, persistently disable trading, require manual re-enablement, or affect independent processes; scope readiness and failure handling by `process_id`.
- Restore eligibility automatically when the required evidence becomes healthy. Reserve process termination or persistent disablement for explicit operator intent or a proven unrecoverable integrity condition.
- When changing lifecycle or readiness behavior, verify automatic restart recovery without bypassing existing capital, identity, accounting, order, or market-safety controls.

## Code
- Source changes do not by themselves authorize or require a local container image build. Do not build, rebuild, push, tag, deploy, or promote an image unless the user explicitly requests that image or release action.
- Use Cargo compilation, tests, linting, and focused checks as the default verification for Rust source changes. CI may validate production Dockerfiles using configured path filters, but agents must not trigger a release-image workflow without explicit user authorization.
- After ingester Rust changes, run `scripts/local-image-ci.sh ingester --checks-only` on the working tree before committing or pushing. It runs the CI formatting, Clippy, tests, and documentation checks with Rust 1.92.0 and does not build an image.
- Run `scripts/local-image-ci.sh <component> --build` only when a local candidate image build is explicitly requested; it selects the next local version for changed inputs, requires a clean commit, and does not deploy the image. Use an explicit `<MAJOR.MINOR.PATCH-local.N>` only when that exact version is requested.
- Do not copy or vendor third-party Rust crates into the repository. Declare registry dependencies in the owning `Cargo.toml`; inspect downloaded crate sources only in Cargo's external registry cache.
- When an image build is explicitly requested, build only affected components: bot changes build `polymarket-bot`; ingester changes build the single `ingester` image used by both `ingester-master` and `ingester-worker` through runtime `INGESTER_MODE` configuration; database migration runner changes build `db-migrate`. Never build separate master and worker images.
- Build the bot with `POLYMARKET_GIT_REVISION=<GIT_COMMIT_HASH>`, the ingester with `INGESTER_GIT_REVISION=<GIT_COMMIT_HASH>`, and db-migrate with `DB_MIGRATE_GIT_REVISION=<GIT_COMMIT_HASH>`.

## Rust file organization
- Organize Rust code by domain responsibility, with one clear purpose per module.
- Keep files narrowly scoped; split files that mix unrelated responsibilities or become difficult to navigate.
- Place shared types and behavior in the nearest common domain module. Do not create generic dumping-ground modules such as `utils` or `common`.
- Keep public module interfaces minimal and expose implementation details only when required by another module.
- Follow the existing crate and module structure unless the requested change requires a focused reorganization.

# Source Control and Releases

## Production Deployment Coordination

- These rules govern agent actions for production releases; local development retains its existing workflow. The executing production workflow may continue its approved steps and documented rollback.
- Live production releases must originate from a `development` release PR targeting `production` that `gunnermadsen` reviews and manually merges in GitHub. That manual merge is the release approval; a separate approving PR review is not required. Agents may prepare the PR but must not approve or merge it on the user's behalf. Required CI checks must pass. The merged revision is the deployment input; do not deploy an unmerged PR.
- Include the selected components, exact image versions and digests, source revisions, material configuration and secret-version pins, migration requirements, and rollback targets in the release PR. Changed image inputs do not authorize adding another component. Preserve unselected component pins.
- Do not push directly to `production`, including generated deployment-pin commits. Prepare deployment pins in the release PR before approval. Do not bypass branch protections or any configured deployment-approval gate.
- Manual production deployment workflow executions are dry-run only. Do not enable live deployment through manual inputs, manual reruns, or an alternative workflow entry point. Internal workflow calls may execute only as part of the approved PR-triggered release or its documented rollback.
- Before merging or starting a release, initiating an authorized dry-run, or changing production state, check active production workflows and remote deployment operations. Never assume CD is idle; if activity is uncertain, establish its status first. Wait for conflicting operations to finish.
- While production CD is active, the agent must remain read-only: wait and monitor status, logs, and errors. Do not push production, launch another workflow, cancel a run, or change code, configuration, versions, or other state unless the user explicitly requests that action during the active deployment. Such authorization does not turn a manual deployment run into a live release.
- On production CD failure, stop further rollout actions, preserve evidence, and report the cause as a pipeline defect, application defect, version discrepancy, infrastructure failure, or not yet proven, together with next steps. Do not bypass checks, improvise deployment workarounds, or modify pipeline logic to finish the failed release. Repairs require user direction; a corrected live release follows the same PR approval path.
- Production mutations must run through documented GitHub Actions workflows. Direct mutations through SSM, SSH, cloudflared, cloud APIs, or another access path require explicit authorization for the specific action and a documented, repeatable, reviewable procedure. General release authorization does not authorize direct production commands. Read-only diagnostics remain permitted.
- Production rollback must use the documented workflow and the affected component's existing owner: Argo CD for Argo-managed applications, or the established owner for other components. Restore the recorded compatible release without competing with active reconciliation, rebuilding rollback images, or reversing database state. Workflow-defined rollback remains permitted; additional agent-initiated recovery requires explicit authorization and must not become a manual live-release path.
- Dry-run must use the existing CD execution path without deploying, applying migrations, changing secrets or registry state, or pushing Git refs. Report unavailable live checks as unverified; never report a dry-run as a successful deployment.

## Branch Roles

- Once a branch is merged into integration, do not add commits to it; commit follow-up changes on integration or a new branch from the latest integration tip.
- `development` contains accepted releases. Do not implement features or fixes directly on it.
- `production` receives accepted `development` releases and reviewed deployment metadata only through user-approved release PRs merged in GitHub. Never implement features or fixes directly on it, push directly to it, or append CI/CD-generated admission commits after merge.
- Before preparing another release, fetch and inspect `origin/production` for file changes absent from `integration`. Reconcile any production-only changes into `integration` with explicit merge authorization, preserving deployed image and secret pins. Do not merge tree-equivalent production PR merge commits solely to align ancestry, and never reset or force-push production.
- Keep local and remote `development` aligned with accepted integration; after advancing `development`, continue integration only from that same accepted tip and never retain a competing local `development` lineage.
- Maintain one permanent branch named `integration`; use it to aggregate accepted feature, defect, training, documentation, and repository-policy changes.
- New feature, defect, and model-training branches use `feature/<name>`, `defect/<name>`, or `training/<name>`, start from the `integration` tip, and merge only into `integration`. Keep unrelated domains separate; follow-up work starts from its existing feature, training, or integration lineage.
- Make standalone documentation and repository-policy changes directly on `integration` in the main worktree. Documentation required by a feature, defect, or training change remains on its owning branch.

## Branch Integration

- Commit changes in coherent domain groups. Feature, defect, and training branches merge only into integration.
- Each merge requires explicit authorization naming the branch or commit.
- Assess only branches selected for merging. If none are named, consider active-cycle branches with an existing worktree and commits outside integration. A branch qualifies when it is clean, committed, not abandoned, and passes its required checks. Report exclusions.
- Before merging, give one compact summary of each selected branch's lineage, worktree state, checks, commits, and diff against integration. Stop only for an ambiguous target, potential loss of uncommitted work, or unauthorized destructive history rewriting.
- Use `--ff-only` when integration has not diverged from the branch merge base; otherwise use `--no-ff`. Do not rewrite or discard lineage to obtain a fast-forward.
- Advance `development` from integration only with explicit authorization naming the integration branch or commit.

## Local Image Boundaries

- Local CI owns `local.N` image builds and provenance. Local CD owns exact local image pins and deployment, but must not change the base-only chart `appVersion`.
- Local application builds and deployments use only `vX.Y.Z-local.N`; never build, alias, or run RC images locally.
- Model training and observability provisioning retain their `model/...` and `provisioned/observability/...` provenance. They do not enter the application image version sequence unless an image input changes.

## Helm Chart Versioning

- Version each chart independently through `Chart.yaml` `version`, using SemVer `X.Y.Z`. The chart version identifies deployable chart structure; `appVersion` identifies the component's base application version and uses base-only `vX.Y.Z`.
- Bump the chart version only when the chart changes the rendered Kubernetes platform contract, such as resource kinds, pod configuration, environment variables, secrets, mounts, services, storage, scheduling, or RBAC; do not bump it for `appVersion`, image pins, digests, source revisions, or baked application and observability content.
- An update limited to `appVersion` and its corresponding image tag, digest, or source-revision pins does not require a chart-version bump.
- Keep exact environment image versions in values files: local values may pin `vX.Y.Z-local.N`, and production values may pin `vX.Y.Z-rc.N` or final `vX.Y.Z`. A newly selected image pin must share the `appVersion` base.
- Run Helm lint and render checks when chart structure changes. Run the applicable pin-consistency check when only application image identity changes.
- Create an immutable annotated Git tag `helm/<chart-name>/vX.Y.Z` only when `Chart.yaml` `version` changes. Record the chart path and change summary. Never move or overwrite the tag. Do not create a new Helm tag for an `appVersion`-only update.

## Image Identity

- Component change detection is independent. Shared code or build-input changes affect every image that consumes them.
- Before allocating a component version, check production release pins and registry tags. Treat promoted version bases as closed, select the next SemVer base for changed inputs, and keep RC, Helm, and release metadata consistent.
- Reuse an existing immutable image when its component inputs are unchanged. Do not replace a selected digest with a rebuild without explicit authorization.
- Local image CI and CD may build, tag, and deploy `local.N` images for Rancher Desktop k3s. Local image evidence does not gate advancing or pushing `development`.
- Remote GitHub CI reads the base-only chart `appVersion`, reuses an existing matching `vX.Y.Z-rc.N` image on retry, or allocates `N + 1` after the highest immutable matching RC tag in ECR; it owns production ARM64 image builds and RC publication from `development`.
- Production registry repositories are immutable. CI may publish an absent SemVer RC tag or verify an existing publication on retry, but it must never move or overwrite an RC tag or publish a mutable production tag.
- Successful remote CI completion, including verification of the immutable RC digest and embedded source revision, is sufficient development image evidence. Do not require golden, candidate-admission, image-hash, or development-checkpoint Git tags.

## Rejected Candidates and Abandoned Lineages

- Tag a failed release snapshot with annotated `rejected/release/git-<full-git-commit-id>` metadata recording its reason, image identities, deployment result, rollback tuple, and replacement when known. This rejects that snapshot without abandoning repaired descendants.
- Before retiring a discarded branch or lineage, create an annotated `abandoned/<domain>/git-<full-git-commit-id>` tag recording its original ref, reason, replacement when known, image identities, and deployment status.
- Rejected snapshots and abandoned lineages are excluded from implicit admission, promotion, and rollback selection. The user may explicitly restore an exact rejected or abandoned commit or image.
- Preserve provenance tags when removing branches or worktrees.

## Safe Image Deployment

- Before deploying any selected image, inspect active realtime strategies, active backfill jobs, worker allocation units, replica count, and current service health. Preserve enough worker capacity for every enabled realtime strategy plus active backfill allocations; do not assume the existing replica count is sufficient.
- Before replacing containers, record the rollback tuple and keep it until deployment verification is complete.
- Add any required worker capacity before replacing existing workers. Verify new workers are registered and healthy before proceeding with the remaining deployment.
- Deploy the exact prebuilt image without rebuilding it or changing unrelated services. Preserve configured process intent and worker scale unless the verified allocation demand requires additional capacity.
- Run Local Deployment Verification after a local image deployment. Run Full Post-Deployment Verification for production or when explicitly requested. A failed check requires investigation, but triggers rollback only when the failure is attributable to the deployed image.
- Record the selected image identity, rollback tuple, worker-capacity calculation, verification results, and unresolved alerts in the deployment result.

### Local Deployment Verification

- Use one bounded verification pass to confirm:
  1. Affected services run the exact selected image IDs and embedded revisions without restart loops; migration state matches the approved set and no unexpected migration is pending.
  2. Worker capacity covers desired realtime profiles and active backfills; every desired profile has a current healthy owner and required bot routes resolve.
  3. Every process enabled before deployment remains enabled, resumes automatically, and has a fresh heartbeat. Existing capital, identity, accounting, order, and market-safety controls remain active. Do not force a trade to prove readiness.
  4. Timestamps advance for feeds required by those processes, and there is no new sustained critical alert or affected-path error attributable to the deployment.
- Recheck only a failed or transient condition. Record results and limitations in the deployment result. A container health endpoint alone does not prove that trading can resume.

### Full Post-Deployment Verification

- After deployment, verify in this order:
  1. Every expected container is running without a restart loop and uses the intended immutable image ID and embedded Git revision.
  2. Database, migration runner, ingester master, every ingester worker, bot, Prometheus, Grafana, Loki, and Alloy report their expected health or successful completion state; migrations complete with none pending.
  3. Worker capacity still covers all desired realtime profiles and active backfills, and every desired realtime profile has a current lease owner with `observed_state='running'` and `health_status='healthy'`.
  4. The master resolves every required product to a live worker, and the bot establishes the expected routes without sustained `no_current_owner`, `owner_unavailable`, route-resolution, stale-product, or missing-publication errors.
  5. Fresh RTDS, orderbook, resolution, contract, Binance, Polygon, and required reference-price timestamps advance after the deployment rather than merely containing old rows; no required product has an ingestion gap.
  6. Every enabled trading process preserves durable intent, resumes required subscriptions, and becomes eligible automatically when its evidence is healthy without bypassing capital, identity, accounting, order, or market-safety controls.
  7. Order, fill, reconciliation, settlement, and accounting paths affected by the deployment remain healthy, and Grafana shows no new sustained critical alerts attributable to the deployment.
- Use bounded, resource-conscious queries and recent log windows for verification. Do not run large diagnostic scans against production tables.
- Report the status of the intended fix and affected paths. Disclose unrelated failures and checks that could not be completed without treating them as rollback triggers.

### Automatic Rollback and Recovery

- Roll back only when the intended fix remains defective or evidence reasonably attributes a regression in an affected path to the deployed image. A coincident, pre-existing, or unrelated failure must be reported and handled separately; it must not trigger rollback of the image.
- On deployment failure, stop further rollout actions and preserve logs and the failed image identity.
- If adding capacity or restarting a narrowly affected disposable worker safely restores an attributable failure without changing code, images, durable intent, or data, perform that recovery before rollback and repeat the affected verification checks. For production, perform this recovery only through the documented workflow or a specifically authorized procedure under Production Deployment Coordination.
- If the attributable failure remains, restore each affected service to the exact immutable image that was running immediately before deployment, together with its recorded material runtime configuration and required worker capacity. Do not rebuild an old commit, use mutable tags as rollback identity, reset Git branches, or change unrelated services.
- Before rolling back across a database change, verify that the previous image is compatible with the current database state. Never reverse or mutate database state outside a separately authorized migration. If compatibility is not proven, leave the database intact, restore only compatible services, and report the rollback as blocked or partial.
- Recreate only the affected containers, then repeat the applicable deployment verification against the rollback tuple. A rollback is complete when the affected paths and required functional data flow recover; disclose unrelated checks that remain unhealthy.

### Failure RCA and Repair Instructions

- Whenever a deployment becomes unhealthy, automatically perform a read-only root-cause analysis after stabilizing or rolling back the stack. Correlate the deployment timestamp with image identities, container events and restart counts, recent bounded logs, profile leases, worker capacity and backfill allocations, route resolution, data freshness, migrations, trading-process recovery, and firing alerts.
- Clearly separate the root cause from symptoms. Identify the first failed dependency or violated invariant, explain the resulting failure chain, and cite the concrete evidence used. Do not call a deployment healthy merely because containers or HTTP endpoints are alive.
- Provide a short repair report stating the problem, impact, cause, immediate repair, recovery evidence, and rollback status. Say `not yet proven` when the cause is inconclusive. Include only the smallest permanent hardening needed to prevent recurrence, and do not implement it unless authorized.
- Make repair instructions executable and unambiguous. Avoid vague directions such as “check the service,” “monitor it,” or “restart if needed”; name the service, condition, exact safe action, and success signal.

## Model Artifact Provenance

- Create an annotated `model/<model-name-with-metadata>` tag only when an executed build or training workflow produces a new immutable model artifact whose identity can be verified. Point the tag to the commit that first records or unambiguously references that artifact.
- Record the artifact SHA-256, immutable path or URI, producing commit, build or training-run identity, source or input identity, qualification status, and deployment status in the tag annotation.
- Reusing, deploying, activating, configuring, copying, exporting, or packaging an existing model does not create a new model artifact and must not create or move a `model/...` tag. Reuse the existing model identity instead.
- A container that uses an existing model receives only the applicable `image/...` provenance. Record the existing model tag, model key, or artifact SHA-256 in the image annotation.
- Model provenance rules govern tagging only; they do not restrict training, evaluation, export, or experimentation.

## Worktree Creation and Ownership

- Before creating a worktree, run `git worktree list` and reuse any worktree that already owns the requested domain.
- Create one worktree only for a new, independent feature, defect, or model-training domain that requires isolation. Store it at `worktrees/<domain>` with one designated `feature/...`, `defect/...`, or `training/...` branch.
- Use that worktree for the domain’s implementation, tests, fixes, review corrections, follow-up work, and model variations belonging to the same training objective. Related temporary branches use the same worktree.
- Do not create a worktree for documentation, standalone configuration adjustments, read-only investigation, small unrelated changes, or corrections within an existing domain. A small isolated change may use its own branch in the main worktree.
- Keep source changes, tests, and lightweight evidence in the assigned worktree. Store generated data under Worktree Data Artifact Storage.
- Never store a worktree under `target/`, `/tmp`, `/private/tmp`, or another external location.
- Do not create parallel worktrees unless they cover materially different domains and parallel isolation was explicitly requested or is strictly necessary.

## Worktree Environment Initialization

- Immediately after creating a worktree, run `scripts/link-worktree-env.sh <worktree-path>` from the main worktree. The script must symlink every top-level runtime `.env` file into the worktree, excluding `.env.example` and other `*.example` templates, and verify that each link resolves to the main worktree.
- The main worktree remains the single source of truth for runtime secrets. Never copy secret values or create independent worktree environment files.
- Do not run commands requiring runtime credentials from the worktree until the script succeeds.

## Model Training and Artifact Storage

- Run model training, backtesting, and data preparation from the assigned `training/...` worktree, with all generated data and artifacts stored under the canonical SSD training root defined in `docs/model-training-artifact-lifecycle.md`.
- Never commit Parquet files, generated datasets, checkpoints, predictions, raw backtest outputs, logs, or large model artifacts. Archive completed and superseded training artifacts on the SSD.
- Never create or commit a repository-root `training-results/` directory. Store all new training-result outcomes in their SSD run directory; Git may contain only concise reports and provenance pointers under maintained documentation or package paths.
- Keep only maintained source code, configuration, tests, concise results, and provenance pointers in Git. Promote reusable Python modules into `packages/btc-directional-model` before merging the training branch.
- Give every completed, partial, or exploratory SSD run a `manifests/run-status.md` that records its lifecycle status, completion evidence, missing outputs, qualification state, and deployment authorization without inferring unknown facts.
- Admit a selected immutable model artifact into `packages/btc-directional-model` only for an explicitly authorized UMR deployment. Record its source run, SHA-256, manifest, and qualification evidence.
- UMR adapters may translate canonical inputs and model outputs only. They must reuse existing feature, inference, persistence, admission, observability, identity, and recovery contracts without duplicating their logic.
- Follow `docs/model-training-artifact-lifecycle.md` for directory layout, manifests, archival, source promotion, and deployment admission. Stop before training if the canonical SSD root is unavailable.

## Worktree Lifecycle

- Keep a feature worktree until its changes are:
  - committed;
  - proportionately verified;
  - merged into its designated base or integration branch when integration is authorized; and
  - no longer needed for generated artifacts or cached evidence.
- Before removing a worktree, verify:
  - `git status --porcelain` is empty;
  - its commits are preserved on a branch or contained in the designated base;
  - it contains no unique untracked or ignored artifacts that must be retained.
- Remove completed worktrees promptly after those checks pass.
- Removing a worktree must not automatically delete its branch.
- Never force-remove a dirty worktree unless the user explicitly authorizes discarding its remaining contents.

# System

## Runtime Configuration

- The trading process configuration (TPC) stored in the `trading_processes` playbook is the sole authority for non-secret controls affecting trading decisions. Any new or modified trade gate, admission or risk control, order-execution policy, or execution-rail selection must be represented in the TPC and consumed through the existing compiled-settings contract; do not introduce parallel configuration stores, flags, or control paths.
- Creating, updating—including any TPC or configuration field—enabling, disabling, or deleting a trading process requires prior written user authorization naming its `process_id` and exact intended change. Written authorization in the active task is sufficient and does not require a second confirmation; use only the established API and verify the persisted result.
- Store sensitive runtime values only in the main worktree's applicable `.env` files. Materialize them into Kubernetes Secrets through the owning provisioned workflow; never place secret values in chart values, manifests, or committed configuration.
- Store global non-sensitive runtime configuration in the owning Helm chart values or authoritative provisioned configuration. Do not store non-sensitive configuration in `.env` files.
- Treat `.env.example` and other `*.example` files as documentation templates only. Never place real secret values in them.
- Changes to the TPC schema, defaults, templates, validation, compilation, or interpretation are reviewed source changes under the normal branch-integration lifecycle. TPC is not CI/CD state or release metadata; CI/CD must not create, select, mutate, promote, or version TPC values. Preserve compatibility for existing processes unless an incompatibility and migration path are explicitly authorized.

## Backfills
- Reuse the existing shared backfill infrastructure, standards, and contracts for every backfill domain; do not create domain-specific workers or deployment definitions.
- Use the existing shared backfill job tables for every backfill domain; do not create domain-specific job tables or drift from the established contract.
- `ingester.backfill_jobs` is the sole job ledger, history, and scheduling source. `ingester-master` is the sole scheduler and API; `ingester-worker` is the sole worker runtime name.
- Every collection unit implements the documented `RealtimeWorkerStrategy`, `BackfillWorkerStrategy`, or both in `packages/market-data-ingester`. Do not create another strategy contract.
- Add a strategy to the existing registry and schedule it through `POST /backfills`. Never add a provider-specific worker binary, image, Kubernetes workload, planner command, queue table, job schema, or scheduling pathway.
- Sharding is deterministic strategy logic invoked by the master. Realtime strategies are not sharded. Worker and deployment selectors are execution controls, not new queues.
- Preserve the API and persistence contracts. Change their versions only for a necessary incompatibility documented in `packages/market-data-ingester/docs/strategy-and-backfill-contract.md`.

## Observability provisioning
- Provision every Grafana, Prometheus, Loki, and Alloy deployment, including all environment configuration changes; do not make manual, unprovisioned changes.
- Every Grafana alert, Prometheus metric, and Loki log must close a named visibility gap with a specific purpose and actionable condition; vague, general, duplicate, and catch-all instrumentation is prohibited.

## Websocket consumer latency
- Follow [the websocket consumer latency contract](docs/websocket-consumer-latency.md) for every realtime provider socket.
- Keep the successful data-frame path from `socket.next()` through bounded-channel enqueue free of logging, metrics, parsing, persistence, publication, and other optional work. Perform that work only after dequeueing.
- Do not add high-frequency observability that competes with socket intake. Preserve explicit bounded-channel overflow, continuity-gap recording, fresh-epoch recovery, and automatic restart behavior.

## Observability deployment provenance
- Production CD records observability deployment provenance in its GitHub Actions summary and retained workflow logs/artifacts; it must not create or push Git tags or commits. The tagging rules below apply to provisioning outside production CD.
- After provisioning Grafana, Prometheus, Loki, or Alloy configuration, create one annotated tag on the exact source commit. For the local development environment, use `provisioned/observability/<YYYYMMDDTHHMMSSZ>`; development is implicit in that name. For other environments, use `provisioned/observability/<environment>/<YYYYMMDDTHHMMSSZ>`.
- Record the target environment, provisioning timestamp, originating branch, deployment result, configuration hash, and every component and configuration path provisioned.
- Create one tag per provisioning event, including when multiple observability components are deployed together. Do not create separate component tags for the same event.
- A provisioning tag records deployment history only; it does not imply image, integration, candidate, or release status.

## Data artifacts
- Store all backtesting and model-training data only in Parquet format, including weather-model predictions and Kraken futures test data.
- Direct Chainlink Data Streams reference prices use only `market_data.chainlink_btcusd_reference_prices`; PMData reference prices use only `market_data.pmdata_chainlink_btcusd_reference_prices`. Strategies must call the corresponding shared persistence function and must not issue table-specific insert SQL.
- A one-off historical drain is copy-only: it may read source tables and write canonical Parquet, but it must not mutate its database sources. Source removal happens only through a separately guarded database migration after complete manifest validation.
- A registered ingester drain job is distinct from a one-off historical drain. It may remove allowlisted, verified, closed Timescale chunks through a committed `db-migrate` function after the drain contract's source-parity and durable-publication checks succeed. The drain worker must not issue ad hoc DELETE, TRUNCATE, or DROP commands.

## Temporary code and scripts
- Do not commit one-off scripts, diagnostic probes, data insertion helpers, scratch files, or other temporary artifacts. Keep them outside the repository, such as under `/tmp`, and delete them when finished. The `scripts/` directory is only for maintained, reusable project tooling.
- Do not add temporary shim logic inline with permanent implementation code.
- A temporary shim may be committed only with explicit user authorization. Isolate it in a dedicated, domain-named module, import it at the narrowest integration point, and clearly record why it exists and the exact condition for removing it.
- Before committing, remove temporary artifacts and obsolete shims from the diff.

# Database Changes and Migrations

- `db-migrate` is the exclusive authority for schema changes and administrative data mutations. This includes schemas, tables, columns, constraints, indexes, extensions, database functions, triggers, roles, grants, reference data, corrections, cleanup, and administrative backfills.
- Every such change must be a committed TypeScript TypeORM migration under `packages/db-migrate/src/migrations`. Required SQL must be contained in that migration and executed through its TypeORM `QueryRunner`.
- Never alter database state through a standalone SQL file, one-off shell, Python, Rust, JavaScript, or TypeScript script, interactive `psql`, ORM synchronization, application startup, another service, host-side migration command, or temporary migration file. There is no exceptional one-off mutation path.
- Established runtime persistence and shared backfill contracts may perform their normal application writes. Bounded read-only diagnostics are also permitted. Neither exception may be used for schema changes, administrative backfills, cleanup, corrections, or reference-data mutation.
- The registered ingester drain worker may invoke only the committed, allowlisted `db-migrate` drain-removal function for verified closed chunks. This is the sole runtime exception for source removal; one-off drains and other administrative mutations remain migration-only.

## Migration Authorization and Execution

- Before creating or first applying a migration on its feature or defect branch, present a plan identifying its exact database effects, affected objects, data scope, rollback behavior, and expected locking or operational risk.
- Before application, confirm that the migration files are committed and that the complete pending migration list exactly matches the approved set. Stop if an approved migration was already applied or any additional migration is pending.
- For the isolated Kubernetes development database in namespace `capitonic`, apply approved migrations only through a separately invoked, one-shot `db-migrate` Job from `capitonic-helm-chart/charts/db-migrate`. The Job must use the committed migration image and connect only to that Kubernetes database. Do not run it as a Helm hook or database startup action.
- Do not run TypeORM commands directly on the host or through any other container. After application, verify the migration ledger and intended schema or data state.
- Any database correction, reversal, or rollback requires its own approved TypeORM migration. Never repair or reverse database state with an ad hoc command or script.
- Never create or modify trading processes through migrations; use the established API.
- Keep diagnostic reads bounded and index-conscious. Do not run scans or queries likely to starve database resources.

## Unified Model Runtime contract

- RTDS history and derived candles must use the single [shared RTDS repository](docs/unified-model-runtime/rtds-repository.md). Preserve startup hydration, causal read semantics and existing metrics; do not add consumer-owned RTDS caches, seed tasks or candle builders.
- Follow [the UMR architecture and adapter contracts](docs/unified-model-runtime/README.md) and its instrumentation/integration standards when changing model inference, feature bindings or monitoring.
- Preserve frozen model behavior, existing process identities, version compatibility and stable observability semantics. New models reuse supported packages or add a thin adapter inside the UMR module; do not rewrite shared execution, ingestion or dashboards per model.
- Verify feature/prediction/admission parity and automatic process-scoped recovery before changing a deployed adapter. Never silently substitute data semantics or bypass existing order/accounting controls.

## Ingester Worker Scaling

- In the local Rancher Desktop k3s development cluster, create and manage ingester workers in the `capitonic` namespace only through the `ingester-worker` Deployment owned by the `ingester` Helm chart.
- With worker scaling disabled, set the replica count through Helm values. With worker scaling enabled, Helm omits the replica field and the ingester master alone updates the Deployment's `/scale` subresource through its namespace-scoped service account.
- Never create ingester workers through manual pod creation or a mechanism outside the owning Helm chart and Deployment.
- Never create purpose-specific worker services, container names, deployment names, or worker variants. All ingester workers must use the standard `ingester-worker` service name and existing runtime contract.
- Changes to the Kubernetes worker scaler are feature work and must preserve the existing worker contract.
- Do not introduce a parallel worker standard or contract. If the owning `ingester-worker` service or Deployment cannot perform the requested work through ordinary scaling, stop and report the incompatibility instead of creating replacement infrastructure.

## Local development deployment ownership

- Rancher Desktop k3s in namespace `capitonic` is the local development runtime.
- Maintain Kubernetes resources through their owning charts under `capitonic-helm-chart/charts/`. Keep `common/configs/` and `common/scripts/` authoritative; run `python3 scripts/helm/bake-assets.py` before Helm operations that need those assets and `--clean` afterward. Runtime `.env` files in the main worktree remain the secret source of truth.
- Schedule realtime and backfill work through the ingester master's authenticated API. With Kubernetes worker scaling enabled, never use Helm replica values, `kubectl scale`, or direct Kubernetes API calls to satisfy operational worker demand; the master alone owns the worker Deployment `/scale` resource.
- Use `docs/capitonic-k3s-cutover.md` for local routes, data backups, cutover acceptance, and routine Helm and Kubernetes commands.

## Polymarket execution rail contract

- Follow [the Polymarket execution rail standard](docs/polymarket-execution-rail/README.md) for the additive order contract and its implementation. Keep legacy model/agent execution unchanged; use the existing process API and persistence. New execution consumes compiled settings, never raw TPC JSON. Generic order controllers must not dispatch by exchange or paper/live mode.
