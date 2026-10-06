# Local image release flow

This is the operator runbook for the bot and ingester image path in local k3s. Each boundary is started deliberately. CI and CD are separate commands; neither command starts the next boundary. `AGENTS.md` owns branch, provenance, deployment-safety, and migration rules, while the scripts own their repeatable build, pin, and rollout actions.

| Responsibility | Owner |
| --- | --- |
| Select work, commit code, choose branch merges, and initiate each command | Operator or agent |
| Check image inputs, select `local.N`, build the image, and create `image/...` provenance tags | `scripts/local-image-ci.sh` |
| Validate the candidate, pin the chart image and `appVersion`, and deploy the committed pin | `scripts/local-image-cd.sh` |
| Record accepted checkpoint and first-use `golden/...` provenance | Operator or agent under `AGENTS.md` |
| Validate one accepted RC and atomically push its Git refs with `development` | `scripts/local-image-ci.sh <component> --tag-rc` |
| Validate and atomically push every RC selected by the production release manifest | `scripts/tag-accepted-rc-set.sh` |

## Feature or defect branch: local candidate

Start from the current integration tip. Make and commit the component change, then invoke CI for that component. CI runs its checks and builds only when committed image inputs differ from a reusable candidate:

```sh
scripts/local-image-ci.sh polymarket-bot --build
# or, on an ingester branch:
scripts/local-image-ci.sh ingester --build
```

Use the `Candidate image` and image ID reported by CI. Invoke CD separately to generate the Helm pin, review and commit its changes, then invoke deployment as another command:

```sh
scripts/local-image-cd.sh pin <component> <vMAJOR.MINOR.PATCH-local.N>
git add capitonic-helm-chart/charts/<component>/Chart.yaml capitonic-helm-chart/charts/<component>/values.yaml
git commit -m "Pin local <component> candidate in Helm chart"
scripts/local-image-cd.sh deploy <component>
```

CD updates the chart image and `appVersion`; for ingester it also pins the immutable image ID and embedded Git revision. It does not increment the Helm chart's own `version`. Confirm the running pods use the selected image ID and that CD's recovery checks pass. When validating another branch in the same cycle, preserve earlier deployed chart pins and their source lineage so the next chart does not regress an image. Branch integration between validations remains a deliberate source-control action.

## Integration branch: golden checkpoint

Before preparing a release, reconcile outstanding production admission commits into integration, preserving deployed image digests and secret-version pins. Production accepts qualified development promotions and scoped CD admission commits; reconcile generated production commits back into integration after deployment.

Select qualifying branches and merge them into integration under `AGENTS.md`. Compare component image inputs with the tested candidates. Reuse an immutable candidate when its inputs match; do not rebuild it merely to assign an RC alias. Run required component checks once for changed inputs.

For each selected component, invoke CD to alias the tested candidate as the next RC and generate its chart pin. The first RC for a new base version is `rc.0`; subsequent RCs increment on that same base. Review and commit the chart pins, then invoke deployment separately for each component:

```sh
scripts/local-image-cd.sh rc <component> <vMAJOR.MINOR.PATCH-local.N>
# Commit the generated Chart.yaml and values.yaml changes.
scripts/local-image-cd.sh deploy <component>
```

Record the predeployment rollback tuple and perform Development Checkpoint Verification from `AGENTS.md`. If accepted, fast-forward `development`, record its annotated checkpoint tag, and create a `golden/...` tag only when each image is first accepted. When `infra/production/release.json` selects more than one component, validate and publish the complete release set together:

```sh
scripts/tag-accepted-rc-set.sh
```

The command checks every accepted chart, running image ID, and provenance before publishing `development`, the aligned integration branch, checkpoint, candidate, image-hash, golden, and RC refs in one `git push --atomic` operation. This prevents CI from observing a partially tagged release manifest. Use `scripts/local-image-ci.sh <component> --tag-rc` only when the release manifest selects a single component. Never push those related refs separately. A golden image remains identified by its immutable image ID and embedded source revision; an RC alias does not change its bytes.

## Separate production boundary

GitHub Actions retains the local CI component boundaries: it runs the bot and ingester formatting, Clippy, unit/integration tests, the existing Docker-backed outage-recovery integration workflow, ingester documentation checks, and the db-migrate TypeScript build/tests. Every production image publication depends on all of those checks. It builds only components whose image inputs changed or whose accepted RC selection changed. Production images are built by GitHub CI from the accepted component source as `linux/arm64`, then published under immutable component-source-revision and SemVer RC tags from `infra/production/release.json`; no mutable `production` image tag is permitted. CI verifies the matching accepted `rc/<component>/<version>` Git provenance before publishing an absent ECR RC, and verifies an existing ECR RC's architecture and embedded revision without rebuilding or overwriting it. Local CI qualifies the local k3s image and publishes Git provenance only; it must not stage or push the production ECR image. An independently rebuilt CI image does not inherit local golden status merely because it has the same source.

### Production CD release contract

The existing `CD.yml` is the single production release workflow. Manual executions
and internal workflow calls validate and report only. They cannot execute production
SSM commands, change Argo/Kubernetes state, apply migrations, refresh secrets, or
publish registry images or Git refs. This is deployment planning, not a successful
production deployment or a simulation of live checks.

| Order | Workflow job | Behavior |
| --- | --- | --- |
| 1 | Detect production components | Establish the exact revision and baseline; select committed chart/value/configuration changes, preserving an empty no-op. Application source and `release.json` do not select components. |
| 2 | Validate release scope | Validate the selected scope without implicitly adding components. |
| 3 | Verify release authorization | In parallel with validation, admit only the first direct CD execution of a production push associated with a merged same-repository `development → production` PR. `gunnermadsen` must approve the final PR head before merging and must merge it. Direct pushes without this evidence fail closed. Manual runs, internal calls and reruns remain planning-only. |
| 3 | Validate release pins / images / charts | Parallel matrix jobs reuse `validate-release-pins.sh`: committed version/secret declarations and RC/golden provenance; read-only ECR digest/ARM64/source verification; runner-local Helm lint/render. Each publishes a separate report. |
| 4 | Report deployment plan | Collect all checks and report the revision, baseline, selected components, pins and planned actions. Any failed check blocks deployment. Live readiness, secret availability, migration ledger/compatibility, worker capacity and rollback remain unverified. |
| 5 | Deploy and verify | Execute only after all preceding jobs succeed and live PR admission is true with a nonempty selection. Use the exact merged revision and existing selected-component helper. The historical job name remains for deployed-baseline lookup. Empty selections skip host checkout and all production execution. |

Production values already contain immutable final version tags, expected digests,
source revisions, chart metadata, secret-version pins and credential revisions.
CD validates these inputs; it never prepares new pins, publishes final image tags,
writes `release.json`, commits metadata, or creates/pushes Git tags. Unselected
component pins are preserved. Existing helpers and unrelated workflows keep their
owners. Live selected execution still uses ECR credential refresh and existing
Argo/Helm reconciliation; planning does not promise those commands are no-ops.
Selected-release scope, migration-ledger, worker-capacity, chart-rendering, tunnel
and Argo/shared-credential preflight checks complete before credential refresh or
cluster reconciliation. Argo advances and rolls back only explicitly selected
applications. Changed shared-secret references require every declared consumer's
owner to be selected; missing bot or Grafana dependencies fail before deployment
instead of silently expanding scope. The same Argo helper supports a read-only
`--preflight-only` check, without a separate deployment implementation.

Production provisioning installs `yq` through its existing installer. Deployment
checks for version v4.47.2 and fails if missing or incompatible; it does not install
host tooling. Runner-side validation retains its pinned tool installation.
Observability deployment evidence stays in Actions summaries and retained logs,
without CD Git publication.

Run planning after pushing the scoped change to `development`:

```bash
gh workflow run CD.yml --ref development
```

The manual run compares that revision against `origin/production`, reports declared
configuration/secret differences and renders selected charts without deploying.
Cloudflared rendering uses a diagnostic tunnel-ID placeholder. `release.json`
remains for other consumers; CD does not migrate local tooling or provisioning.

GitHub production branch protection requires PR review, dismisses stale approvals
and applies to administrators. CD additionally verifies the named approver and
final reviewed head, since a generic required review is not identity-specific.
GitHub does not allow a PR author to approve their own PR: a release PR authored
by `gunnermadsen` cannot satisfy this contract. Prepare it through an authorized
separate author; do not bypass approval or substitute a merge click for review.
A failed release stops rollout; live reruns require a new approved release PR.

## Outside this image version sequence

- Model training retains the existing `model/...` tag for a newly produced, verifiable immutable model artifact. Training, activation, or reuse of an existing model does not mint a bot or ingester image version. Packaging changed model runtime inputs into an image does enter the affected image's normal CI/CD flow.
- Grafana, Prometheus, Loki, and Alloy configuration is provisioned through its owning chart and records the existing `provisioned/observability/...` tag. A configuration-only provision does not mint a bot or ingester image version.
- `db-migrate` has its own candidate image and approved one-shot migration process; ordinary bot or ingester deployment does not run migrations.

## Finishing a version

Once a final version is published, its base is closed: subsequent changed images use the next SemVer base with local.0 and rc.0, after checking remote Git and registry state. Local CI reads the selected base from the release manifest, rejects changed inputs under a promoted base, and only reuses candidates from the selected base.

Final ECR version tags and matching production chart pins must already exist in the reviewed release revision. CD validates those immutable identities rather than promoting RCs or rewriting release metadata. Choose a new base version per component from the change: patch for a compatible fix, minor for compatible new behavior, or major for a breaking change. On that base, local candidates start at `local.0` and RCs start at `rc.0`; subsequent candidates increment within the same base version.

## Production chart and runtime secret ownership

Argo CD reconciles the existing bot and ingester resources at the immutable Git
revision admitted by production CD. CD validates committed release pins and retains
full functional verification in its disabled deployment path. Direct Helm deployment
of those applications stops after adoption; other releases and separately approved
migrations retain their existing owners. Pruning and replacement syncs are disabled.
The ingester master remains the owner of worker scaling.

External Secrets Operator is a separate official Helm release pinned by
`infra/production/external-secrets-release.json`, with production values in
`capitonic-helm-chart/environments/production/external-secrets.yaml`. The dedicated
`Deploy Production External Secrets` workflow bootstraps it and subsequently asks
Argo to reconcile it. The Argo provisioning workflow retains native authentication,
read-only default access, and existing ingress configuration while enabling chart
reconciliation. Neither workflow provisions or replaces EC2.

On clean EC2 provisioning, the node workflow installs ESO after k3s readiness.
The full-stack workflow then bootstraps existing Kubernetes secrets and infrastructure
before configuring Argo. Before first Argo sync, immutable AWS version references
must already be committed in the reviewed production values; empty version
references fail closed.
Do not execute EC2 provisioning to update ESO or Argo on an existing host.

AWS stores one JSON object. Explicit property mappings populate the existing
`ingester-auth`, `ingester-provider-auth`, `polymarket-bot-auth`, and
`polymarket-live-auth` Secrets. ESO uses immutable version IDs, OnChange refresh,
Orphan creation, and Retain deletion. Database, monitoring, ECR, and Argo bootstrap
credentials remain outside this ownership transfer. Existing administrative secret
refresh skips ESO-owned Secrets and Argo-owned workload restarts. Runtime
secret pin updates require reviewed production values; CD does not discover newer versions.

Credential revisions and immutable secret-version references are committed release
inputs. CD does not compute new revisions or edit these values. Argo waits for each
ExternalSecret's successful refresh after its requested timestamp before applying
later workload waves. The disabled deployment path retains verification of selected
AWS/Kubernetes value parity. Independent AWS changes remain unapplied until a reviewed
release changes the committed pins.

### Cutover acceptance contract

First adopt the existing resources using their current images, preserving workload
UIDs, selectors, volumes, worker capacity, and enabled process identities. Confirm
secret parity, healthy ESO/Argo reconciliation, advancing profile watermarks, process
readiness, migration state, and no attributable critical alerts. Preserve the
pre-cutover snapshot and prior admitted application revisions.

The planned parallel microservice change is the image/version acceptance input;
do not manufacture an unrelated application source change to test deployment.
Combine its qualified immutable image pin with an AWS-property mapping change,
keeping the destination Kubernetes key/environment name stable. Retain the previous
AWS property/version through verification. Record Deployment generations, ReplicaSet
identities, immutable images, enabled process IDs, and feed watermarks before/after.
Require one pod-template rollout per affected Deployment and no unrelated rollout.
Reconcile the same admitted release again and require unchanged ReplicaSets/pod UIDs.
Then validate a secret-only update affects only declared consumers. Completion of
ownership handover does not count as completion of this image/version test.

For an attributable failed release, restore previous immutable Argo target revisions,
including AWS-version mappings, restore the Grafana Helm revision when affected,
and wait for reconciliation and verify functional recovery. Do not run Helm
rollback against an actively reconciling different Argo target. On a full-verifier
failure with uncertain attribution, preserve evidence and perform read-only RCA
before deciding rollback. To reverse ownership, suspend Argo and ESO reconciliation
before restoring the original writers; retain Secrets, storage, database state,
and the recorded rollback tuple.
