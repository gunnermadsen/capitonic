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

## Outside this image version sequence

- Model training retains the existing `model/...` tag for a newly produced, verifiable immutable model artifact. Training, activation, or reuse of an existing model does not mint a bot or ingester image version. Packaging changed model runtime inputs into an image does enter the affected image's normal CI/CD flow.
- Grafana, Prometheus, Loki, and Alloy configuration is provisioned through its owning chart and records the existing `provisioned/observability/...` tag. A configuration-only provision does not mint a bot or ingester image version.
- `db-migrate` has its own candidate image and approved one-shot migration process; ordinary bot or ingester deployment does not run migrations.

## Finishing a version

Production CD selects the committed RC in `infra/production/release.json`, verifies its immutable Ireland ECR digest and `linux/arm64` platform, and creates the final `vMAJOR.MINOR.PATCH` ECR tag against that same manifest. It derives the final version through SemVer validation and shell parameter expansion; text substitution must not guess or strip an arbitrary suffix. CD then writes the exact digest, embedded source revision, and final version into the production Helm overlay and chart `appVersion` before deployment. An existing final tag must already identify the selected RC digest or promotion stops. Choose a new base version per component from the change: patch for a compatible fix, minor for compatible new behavior, or major for a breaking change. On that base, local candidates start at `local.0` and RCs start at `rc.0`; subsequent candidates increment within the same base version.

## Production chart and runtime secret ownership

Argo CD reconciles the existing bot and ingester resources at the immutable Git
revision admitted by production CD. Actions retains image qualification, ECR
promotion, release pins, and full functional verification. Direct Helm deployment
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
before configuring Argo. Before first Argo sync, production CD must select immutable
AWS versions through `prepare-runtime-secrets.py`; empty version references fail closed.
Do not execute EC2 provisioning to update ESO or Argo on an existing host.

AWS stores one JSON object. Explicit property mappings populate the existing
`ingester-auth`, `ingester-provider-auth`, `polymarket-bot-auth`, and
`polymarket-live-auth` Secrets. ESO uses immutable version IDs, OnChange refresh,
Orphan creation, and Retain deletion. Database, monitoring, ECR, and Argo bootstrap
credentials remain outside this ownership transfer. Existing administrative secret
refresh skips ESO-owned Secrets and Argo-owned workload restarts. Manual runtime
refresh dispatches ordinary production CD, using the same admission mechanism.

CD compares projected property values with Kubernetes without printing credentials.
Only changed consumers receive a new opaque credential revision. Grafana also consumes
the bot admin token and is refreshed through its existing Helm owner when that token
changes; its credential revision is provisioned in chart values. Image pins and
credential revisions enter one admitted pod template. Argo waits for each
ExternalSecret's successful refresh after its requested timestamp before applying
later workload waves. Full verification checks actual selected AWS/Kubernetes value
parity. Unchanged projections retain references and rollout tokens, so reconciliation
does not restart pods. Independent AWS changes remain unapplied until admitted by CD.

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
