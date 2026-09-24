# Implementation
- The user’s requested outcome and any agreed implementation plan are the authoritative scope. If no separate plan exists, the request itself defines the implementation boundary.
- Execute every in-scope requirement completely and verify the requested outcome. Do not add optional improvements, cleanup, redesigns, or unrelated fixes.
- Before implementing functionality, identify the existing module, contract, registry, persistence path, instrumentation, or execution path that owns the capability. Reuse its supported extension point instead of creating another implementation.
- Do not delete, disable, replace, rewrite, or modify existing systems, behavior, interfaces, or data outside the defined scope, even when doing so would simplify the implementation.
- If completing the work requires a change outside the defined scope, stop before making that change, explain why it is required, and obtain explicit user authorization to amend the scope.
- If an existing system cannot satisfy the request, stop and explain the specific incompatibility. Do not create a parallel or replacement system without explicit user authorization.
- Use `trading_processes.process_id` as the canonical identity for trading-process ownership and scoping.

# North Star
Think of Capitonic as a vision to generate income through systems with automation and algorithms, that require no customers. The polymarket trading bot is an extension of that vision, to systemically generate an  income stream based on a reproducable strategy that can trade with positive net expectancy through a narrow and simplified system. the key is positive net expectancy and a simplified system. Capitonic was not envisioned to live in a labatory forever. Capitonic was envisioned to prove a concept, that an automated trading bot can become a real product, a real business. Captitonic is NOT here to be reduced to a pet project or a labratory, or an experiment platform. Capitonic exists to discover an edge, exploited with a strategy, and repeated to generate net expectancy. Do not make the mistake of seeing Capitonic as a "research platform", because it is not that. Capitonic is not a school, or a university; it is a business. Businesses rely on income to survive. If Capitonic produces income, Capitonic WILL thrive.

## Golden Rules
- All Rust features and systems are implemented and optimized for latency, memory usage efficiency, CPU performance, zero downtime, and database resource usage, ensuring a performant and resiliant system.
- Do not name artifacts, branches, files or code comments based on phases or stages of an implementation. use domain specific naming for the issue or feature being addresses. I dont' want to see 'phase X' in git history, code files, branches, or file names or code comments. 

## Always-On Trading Liveness
- Do not create blunt or pointless kill switches that stop trading processes, revoke durable trading authorization, or require manual re-enablement merely because a container, database connection, reconciliation loop, websocket, or other dependency restarted or experienced a transient failure.
- A normal container deployment or service restart must preserve the configured intent of an enabled trading process and recover back to trading automatically once its required runtime evidence is healthy. Ephemeral in-memory gates must not silently override durable process configuration after restart.
- Transient unavailability, stale evidence, crossed or one-sided books, temporary reconciliation errors, database reconnects, and transport reconnects may block only the specific unsafe action while the condition exists. They must not terminate the process, permanently disable entries, or poison unrelated processes.
- Recovery must be automatic and narrowly scoped. When healthy evidence returns, the affected action must become eligible again without a manual operator ritual unless the operator explicitly disabled it in durable configuration.
- Reserve process termination or persistent trading disablement for explicit operator intent or a proven unrecoverable integrity condition. Do not treat ordinary infrastructure lifecycle events as unrecoverable integrity failures.
- Shared infrastructure faults must not allow one trading process to disable other independent processes. Scope runtime readiness and failure handling by `process_id` wherever process behavior is involved.
- When changing lifecycle or readiness code, verify restart recovery and confirm that configured live processes resume eligibility without bypassing the existing per-order capital, identity, accounting, and market-safety checks.

## Code
- Source changes do not by themselves authorize or require a local container image build. Do not build, rebuild, push, tag, deploy, or promote an image unless the user explicitly requests that image or release action.
- Use Cargo compilation, tests, linting, and focused checks as the default verification for Rust source changes. CI may validate production Dockerfiles using configured path filters, but agents must not trigger a release-image workflow without explicit user authorization.
- When an image build is explicitly requested, build only affected components: bot changes build `polymarket-bot`; ingester changes build the single `ingester` image used by both `ingester-master` and `ingester-worker` through runtime `INGESTER_MODE` configuration; database migration runner changes build `db-migrate`. Never build separate master and worker images.
- Build the bot with `POLYMARKET_GIT_REVISION=<GIT_COMMIT_HASH>`, the ingester with `INGESTER_GIT_REVISION=<GIT_COMMIT_HASH>`, and db-migrate with `DB_MIGRATE_GIT_REVISION=<GIT_COMMIT_HASH>`.
- put sensitive secrets in .env files
- Treat the main worktree's `.env` file as the source of truth for API-key secrets. Worktrees and services must reference or inherit those secrets without creating independent secret values.
- put non-sensitive runtime configuration in docker-compose files
- .env example files are templates, do not put plaintext env vars in the example env files.

## Rust file organization
- Organize Rust code by domain responsibility, with one clear purpose per module.
- Keep files narrowly scoped; split files that mix unrelated responsibilities or become difficult to navigate.
- Place shared types and behavior in the nearest common domain module. Do not create generic dumping-ground modules such as `utils` or `common`.
- Keep public module interfaces minimal and expose implementation details only when required by another module.
- Follow the existing crate and module structure unless the requested change requires a focused reorganization.

# Source Control and Releases

## Branch Roles

- `development` contains accepted releases. Do not implement features or fixes directly on it.
- Use one active integration branch named `integration-<YYYY-MM-DD>`, created from the accepted `development` tip. An optional annotated `integration-cycle/<YYYY-MM-DD>` tag may record its starting boundary.
- New feature, defect, and model-training branches use `feature/<name>`, `defect/<name>`, or `training/<name>`, start from the latest integration tip, and merge only into integration. Keep unrelated domains separate; follow-up work starts from its existing feature, training, or integration lineage.
- Use `docs/<name>` for standalone documentation or repository-policy changes, including `AGENTS.md` and files under `docs/`. Create and work on documentation branches only in the main worktree from the latest integration tip; do not create a separate worktree. Documentation required by a feature or defect remains on its owning branch.
- Keep integration and documentation branches in the main worktree. A narrowly scoped integration-policy or coordination correction may be committed directly on integration only when explicitly requested.
- Tag a committed standalone documentation change with annotated `docs/<name>/git-<full-git-commit-id>` metadata recording its source branch, changed paths, purpose, and integration base. Documentation tags record provenance only and do not confer acceptance, image, or golden status.

## Branch Integration

- Commit changes in coherent domain groups. Feature, defect, training, and documentation branches merge only into integration.
- Outside the golden image workflow, each merge requires explicit authorization naming the branch or commit.
- A branch qualifies for the golden image workflow when it belongs to the active integration cycle, is clean and committed, is not abandoned, and passes its required checks. Report and exclude branches that do not qualify.
- Before merging, report its lineage, abandoned status, worktree and test state, commits, and diff against integration. Explicit authorization remains sufficient despite disclosed findings; stop only for an ambiguous target, potential loss of uncommitted work, or unauthorized destructive history rewriting.
- Use `--ff-only` when integration has not diverged from the branch merge base; otherwise use `--no-ff`. Do not rewrite or discard lineage to obtain a fast-forward.
- Advance `development` only through the golden image workflow or an explicitly authorized configuration-only promotion.

## Golden Image Workflow

`Perform the golden image workflow` authorizes eligible branch merges, candidate builds and local deployment, promotion, golden tagging, pushing to origin, cleanup, and integration-cycle rollover.

1. Inspect and merge every qualifying active-cycle branch into integration under Branch Integration.
2. Compare each image-producing component with its latest golden revision. Run required checks and build immutable candidates with the required embedded Git revisions only for components whose code or shared inputs changed.
3. Record the candidate and rollback tuples, create `image/<image_name>/sha256-<docker-sha256-hash>` provenance tags, and deploy the exact candidates under Safe Image Deployment.
4. On an attributable failure, restore the rollback tuple, create `rejected/release/git-<full-git-commit-id>`, and leave `development` and golden tags unchanged.
5. On success, fast-forward `development` with `--ff-only` to the verified integration commit. For each affected image, create an annotated `golden/<image_name>/sha256-<docker-sha256-hash>` tag recording its candidate tuple and accepted limitations. Golden images may be minted only from the current `development` commit.
6. Push `development`, provenance tags, golden tags, and exact verified image manifests. Verify the expected GitHub Actions results; an independently rebuilt CI image does not inherit golden status.
7. Remove merged worktrees under Worktree Lifecycle. Retain the current-date integration branch when it matches `development`; otherwise create it from `development`.

Migration creation and initial feature or defect validation follow Database Changes and Migrations. The golden image workflow authorizes rebuilding and deploying `db-migrate` when its pending migrations exactly match the committed, approved, and previously validated set. Stop for any new, altered, unvalidated, or unexpected migration.

## Image and Golden Identity

- An `image/...` tag records build provenance. A `golden/...` tag accepts an exact immutable image whose source is the current `development` commit; branch position, tests, deployment, and image tags alone do not confer golden status.
- The candidate tuple records the Git revision, immutable image identities, embedded revisions, migration state, material runtime configuration, applicable model identity, checks, and limitations. The rollback tuple records the predeployment images and revisions, service set, worker capacity, active realtime and backfill allocations, migration state, and material runtime configuration.
- Golden status and change detection are component-specific. Shared code or build-input changes affect every image that consumes them.
- Reuse a selected immutable image when it exists. Do not replace a selected digest with a rebuild without explicit authorization.
- Golden tags are provenance records. Do not delete or move them to hide a later failure; use a rejected or abandoned tag to exclude a failed artifact or lineage from future admission.

## Configuration-Only Promotion

- The user may explicitly authorize a configuration-only fast-forward from integration to `development` without building or minting images.
- Before promotion, inspect and report the complete commit range and confirm that no changed path affects application source, image contents, database migrations, or trading behavior. Otherwise use the golden image workflow.

## Rejected Candidates and Abandoned Lineages

- Tag a failed release snapshot with annotated `rejected/release/git-<full-git-commit-id>` metadata recording its reason, image identities, deployment result, rollback tuple, and replacement when known. This rejects that snapshot without abandoning repaired descendants.
- Before retiring a discarded branch or lineage, create an annotated `abandoned/<domain>/git-<full-git-commit-id>` tag recording its original ref, reason, replacement when known, image identities, and deployment status.
- Rejected snapshots and abandoned lineages are excluded from implicit admission, promotion, and rollback selection. The user may explicitly restore an exact rejected or abandoned commit or image.
- Preserve provenance tags when removing branches or worktrees.

## Safe Image Deployment

- Before deploying any newly built image, inspect active realtime strategies, active backfill jobs, worker allocation units, replica count, and current service health. Preserve enough worker capacity for every enabled realtime strategy plus active backfill allocations; do not assume the existing replica count is sufficient.
- Add any required worker capacity before replacing existing workers. Verify new workers are registered and healthy before proceeding with the remaining deployment.
- Deploy the exact prebuilt image without rebuilding it or changing unrelated services. Preserve configured process intent and worker scale unless the verified allocation demand requires additional capacity.
- Run Post-Deployment Verification before declaring the deployment healthy. A failed check requires investigation, but triggers rollback only when the failure is attributable to the deployed image.
- Record the candidate tuple, worker-capacity calculation, verification results, and unresolved alerts in the deployment result.

### Post-Deployment Verification

- Before replacing containers, record the rollback tuple defined by the golden image workflow and keep it until deployment verification is complete.
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
- On deployment failure, stop further rollout actions and preserve logs and the failed candidate tuple. Do not mint, move, or alter golden tags to disguise the failure.
- If adding capacity or restarting a narrowly affected disposable worker safely restores an attributable failure without changing code, images, durable intent, or data, perform that recovery before rollback and repeat the affected verification checks.
- If the attributable failure remains, restore each affected service to the exact immutable image that was running immediately before deployment, together with its recorded material runtime configuration and required worker capacity. Do not rebuild an old commit, use mutable tags as rollback identity, reset Git branches, or change unrelated services.
- Before rolling back across a database change, verify that the previous image is compatible with the current database state. Never reverse or mutate database state outside a separately authorized migration. If compatibility is not proven, leave the database intact, restore only compatible services, and report the rollback as blocked or partial.
- Recreate only the affected containers, then repeat Post-Deployment Verification against the rollback tuple. A rollback is complete when the affected paths and required functional data flow recover; disclose unrelated checks that remain unhealthy.

### Failure RCA and Repair Instructions

- Whenever a deployment becomes unhealthy, automatically perform a read-only root-cause analysis after stabilizing or rolling back the stack. Correlate the deployment timestamp with image identities, container events and restart counts, recent bounded logs, profile leases, worker capacity and backfill allocations, route resolution, data freshness, migrations, trading-process recovery, and firing alerts.
- Clearly separate the root cause from symptoms. Identify the first failed dependency or violated invariant, explain the resulting failure chain, and cite the concrete evidence used. Do not call a deployment healthy merely because containers or HTTP endpoints are alive.
- Provide a short repair report in direct language using exactly these headings:
  - `Problem`: one sentence describing what is broken.
  - `Impact`: which ingestion products, trading processes, or services are affected.
  - `Cause`: the proven root cause; say `not yet proven` when evidence is incomplete.
  - `Immediate repair`: numbered commands or actions that restore service safely, including exact targets and required verification.
  - `Verification`: the observable evidence that proves recovery, including advancing timestamps and cleared route/profile failures.
  - `Rollback status`: the restored image IDs and revisions, or the precise reason rollback is blocked or partial.
  - `Permanent hardening`: only the smallest code, configuration, alerting, or deployment change needed to prevent recurrence; do not implement it unless authorized.
- Make repair instructions executable and unambiguous. Avoid vague directions such as “check the service,” “monitor it,” or “restart if needed”; name the service, condition, exact safe action, and success signal.

## Model Artifact Provenance

- A `model/<model-name-with-metadata>` tag records provenance for a new immutable model artifact produced by an executed model-build or training workflow. Create it only when that exact artifact exists, its identity can be verified, and the tag can point to the commit that first records or unambiguously references it.
- Deploying, activating, configuring, copying, exporting for runtime use, or packaging an existing model does not constitute a new model build and must not create a new `model/...` tag. Building a container that uses an existing model receives an `image/...` tag only; record the existing model tag, model key, or artifact SHA-256 in the image tag annotation.
- Starting a paper or live trading process with an existing model must not place a `model/...` tag on the process configuration commit, deployment commit, current branch tip, or latest repository commit. Reuse the existing model identity without moving or recreating its tag.
- Model tags must be annotated and record the model artifact SHA-256, artifact path or immutable URI, producing commit, executed model-build or training-run identity, source or input identity, qualification status, and deployment status at tagging time.
- Never infer model-tag eligibility from a model filename, manifest, process deployment, container build, branch name, or the fact that a commit is recent. If the task did not produce a new immutable model artifact, do not create a `model/...` tag.
- These rules govern Git tag creation and provenance only. They do not gate, delay, prohibit, prescribe, or otherwise interfere with model training, retraining, evaluation, export, or experimentation.

## Worktree Creation and Ownership

- Before creating a worktree, run `git worktree list` and reuse any worktree that already owns the requested domain.
- Create one worktree only for a new, independent feature, defect, or model-training domain that requires isolation. Store it at `worktrees/<domain>` with one designated `feature/...`, `defect/...`, or `training/...` branch.
- Use that worktree for the domain’s implementation, tests, fixes, review corrections, follow-up work, and model variations belonging to the same training objective. Related temporary branches use the same worktree.
- Do not create a worktree for documentation, standalone configuration adjustments, read-only investigation, small unrelated changes, or corrections within an existing domain. A small isolated change may use its own branch in the main worktree.
- Keep source changes, tests, and lightweight evidence in the assigned worktree. Store generated data under Worktree Data Artifact Storage.
- Never store a worktree under `target/`, `/tmp`, `/private/tmp`, or another external location.
- Do not create parallel worktrees unless they cover materially different domains and parallel isolation was explicitly requested or is strictly necessary.

## Worktree Environment Initialization

- Immediately after creating any worktree, symlink every top-level runtime `.env` file from the main worktree into the new worktree. The main worktree remains the single source of truth; never copy secret values or create independent worktree environment files.
- Exclude `.env.example` and other `*.example` templates. Do not run Docker Compose, application commands, tests that require runtime credentials, or deployment commands from the new worktree until the links have been created and verified.
- Run the following from the main worktree, replacing `<feature-domain>` with the worktree directory name:

```bash
set -eu
main_worktree="$(git rev-parse --show-toplevel)"
worktree_path="$main_worktree/worktrees/<feature-domain>"

for env_file in "$main_worktree"/.env "$main_worktree"/.env.*; do
  [ -f "$env_file" ] || continue
  case "$env_file" in
    *.example) continue ;;
  esac
  ln -s "$env_file" "$worktree_path/$(basename "$env_file")"
done
```

- Verify every runtime environment file resolves to the main worktree before using the worktree:

```bash
set -eu
main_worktree="$(git rev-parse --show-toplevel)"
worktree_path="$main_worktree/worktrees/<feature-domain>"

for env_file in "$main_worktree"/.env "$main_worktree"/.env.*; do
  [ -f "$env_file" ] || continue
  case "$env_file" in
    *.example) continue ;;
  esac
  link="$worktree_path/$(basename "$env_file")"
  [ -L "$link" ]
  [ "$(readlink "$link")" = "$env_file" ]
  [ -r "$link" ]
done
```

## Model Training and Artifact Storage

- Run model training, backtesting, and data preparation from the assigned `training/...` worktree, with all generated data and artifacts stored under the canonical SSD training root defined in `docs/model-training-artifact-lifecycle.md`.
- Never commit Parquet files, generated datasets, checkpoints, predictions, raw backtest outputs, logs, or large model artifacts. Archive completed and superseded training artifacts on the SSD.
- Keep only maintained source code, configuration, tests, concise results, and provenance pointers in Git. Promote reusable Python modules into `packages/btc-directional-model` before merging the training branch.
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
- Trading process parameters that affect trades, go in the trading playbook config stored in 'trading_processes'. parameters that affect global systems go in environment variables.
- All plaintext env vars that are non sensitive, go in docker-compose files in the 'environment' section. do not put plantext env vars in .env file. 
- Refactoring should not render trading processes as no longer compatible. 
- Refactoring a system, introducing a new system, or removing a system and causing a lack of compatibility is an anti pattern in the trading bot.

## Backfills
- Reuse the existing shared backfill infrastructure, standards, and contracts for every backfill domain; do not create domain-specific backfill workers or Docker Compose files.
- Use the existing shared backfill job tables for every backfill domain; do not create domain-specific job tables or drift from the established contract.
- `ingester.backfill_jobs` is the sole job ledger, history, and scheduling source. `ingester-master` is the sole scheduler and API; `ingester-worker` is the sole worker runtime name.
- Every collection unit implements the documented `RealtimeWorkerStrategy`, `BackfillWorkerStrategy`, or both in `packages/market-data-ingester`. Do not create another strategy contract.
- Add a strategy to the existing registry and schedule it through `POST /backfills`. Never add a provider-specific worker binary, image, Compose service, planner command, queue table, job schema, or scheduling pathway.
- Sharding is deterministic strategy logic invoked by the master. Realtime strategies are not sharded. Worker and deployment selectors are execution controls, not new queues.
- Preserve the API and persistence contracts. Change their versions only for a necessary incompatibility documented in `packages/market-data-ingester/docs/strategy-and-backfill-contract.md`.

## Observability provisioning
- Provision every Grafana, Prometheus, Loki, and Alloy deployment, including all environment configuration changes; do not make manual, unprovisioned changes.

## Websocket consumer latency
- Follow [the websocket consumer latency contract](docs/websocket-consumer-latency.md) for every realtime provider socket.
- Keep the successful data-frame path from `socket.next()` through bounded-channel enqueue free of logging, metrics, parsing, persistence, publication, and other optional work. Perform that work only after dequeueing.
- Do not add high-frequency observability that competes with socket intake. Preserve explicit bounded-channel overflow, continuity-gap recording, fresh-epoch recovery, and automatic restart behavior.

## Observability deployment provenance
- After provisioning Grafana, Prometheus, Loki, or Alloy configuration, create one annotated tag on the exact source commit using `provisioned/observability/<environment>/<YYYYMMDDTHHMMSSZ>`.
- Record the target environment, provisioning timestamp, originating branch, deployment result, configuration hash, and every component and configuration path provisioned.
- Create one tag per provisioning event, including when multiple observability components are deployed together. Do not create separate component tags for the same event.
- A provisioning tag records deployment history only; it does not imply image, integration, candidate, or golden-release status.

## Data artifacts
- Store all backtesting and model-training data only in Parquet format, including weather-model predictions and Kraken futures test data.
- Direct Chainlink Data Streams reference prices use only `market_data.chainlink_btcusd_reference_prices`; PMData reference prices use only `market_data.pmdata_chainlink_btcusd_reference_prices`. Strategies must call the corresponding shared persistence function and must not issue table-specific insert SQL.
- A one-off historical drain is copy-only: it may read source tables and write canonical Parquet, but it must not mutate its database sources. Source removal happens only through a separately guarded database migration after complete manifest validation.

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

## Migration Authorization and Execution

- Before creating or first applying a migration on its feature or defect branch, present a plan identifying its exact database effects, affected objects, data scope, rollback behavior, and expected locking or operational risk.
- Approval of that plan, including `execute the plan`, authorizes creating the migration, deploying it for branch validation, and repeating that validation as needed without renewed approval, provided the migration and stated effects have not changed.
- Before application, confirm that the migration files are committed and that the complete pending migration list exactly matches the approved set. Stop if an approved migration was already applied or any additional migration is pending.
- Apply approved migrations only by recreating the `db-migrate` container from the project Compose configuration:

```bash
docker compose up -d --force-recreate --no-deps db-migrate
```

- Do not run TypeORM commands directly on the host or through another container. After application, verify the migration ledger and intended schema or data state.
- Any database correction, reversal, or rollback requires its own approved TypeORM migration. Never repair or reverse database state with an ad hoc command or script.
- Never create or modify trading processes through migrations; use the established API.
- Keep diagnostic reads bounded and index-conscious. Do not run scans or queries likely to starve database resources.

## Unified Model Runtime contract

- RTDS history and derived candles must use the single [shared RTDS repository](docs/unified-model-runtime/rtds-repository.md). Preserve startup hydration, causal read semantics and existing metrics; do not add consumer-owned RTDS caches, seed tasks or candle builders.
- Follow [the UMR architecture and adapter contracts](docs/unified-model-runtime/README.md) and its instrumentation/integration standards when changing model inference, feature bindings or monitoring.
- Preserve frozen model behavior, existing process identities, version compatibility and stable observability semantics. New models reuse supported packages or add a thin adapter inside the UMR module; do not rewrite shared execution, ingestion or dashboards per model.
- Verify feature/prediction/admission parity and automatic process-scoped recovery before changing a deployed adapter. Never silently substitute data semantics or bypass existing order/accounting controls.

## Ingester Worker Scaling

- Create and manage ingester workers only by scaling the existing `ingester-worker` service in the repository’s Docker Compose project.
- Never create ingester workers with `docker run`, `docker compose run`, manual container creation, or any mechanism outside the Compose project.
- Never create purpose-specific worker services, container names, deployment names, or worker variants. All ingester workers must use the standard `ingester-worker` service name and existing runtime contract.
- Worker scaling is an operational action only. Do not modify source code, Dockerfiles, Compose files, environment files, or configuration to scale workers.
- Do not introduce a parallel worker standard or contract. If the existing `ingester-worker` service cannot perform the requested work through ordinary Compose scaling, stop and report the incompatibility instead of creating or modifying infrastructure.
