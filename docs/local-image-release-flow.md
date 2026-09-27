# Local image release flow

This is the operator runbook for the bot and ingester image path in local k3s. Each boundary is started deliberately. CI and CD are separate commands; neither command starts the next boundary. `AGENTS.md` owns branch, provenance, deployment-safety, and migration rules, while the scripts own their repeatable build, pin, and rollout actions.

| Responsibility | Owner |
| --- | --- |
| Select work, commit code, choose branch merges, and initiate each command | Operator or agent |
| Check image inputs, select `local.N`, build the image, and create `image/...` provenance tags | `scripts/local-image-ci.sh` |
| Validate the candidate, pin the chart image and `appVersion`, and deploy the committed pin | `scripts/local-image-cd.sh` |
| Record accepted checkpoint and first-use `golden/...` provenance | Operator or agent under `AGENTS.md` |
| Validate the accepted RC and atomically push its Git refs with `development` | `scripts/local-image-ci.sh <component> --tag-rc` |

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

Record the predeployment rollback tuple and perform Development Checkpoint Verification from `AGENTS.md`. If accepted, fast-forward `development`, record its annotated checkpoint tag, and create a `golden/...` tag only when each image is first accepted. Then run the accepted RC tagging command for each component independently:

```sh
scripts/local-image-ci.sh <component> --tag-rc
```

That command checks the accepted chart, running image ID, and provenance before atomically pushing `development` and that component's RC and provenance refs to origin. Align the current integration branch with `development`. A golden image remains identified by its immutable image ID and embedded source revision; an RC alias does not change its bytes.

## Separate production boundary

GitHub Actions builds affected production images from `development` and publishes them to ECR under Git-revision tags. Production deployment is a separate command. Those CI images are independently built; a shared source commit does not prove their registry digests equal the locally verified k3s image IDs. Record and verify the registry identity before making a final-release claim. Local golden acceptance neither starts production CD nor publishes the local image to ECR.

## Outside this image version sequence

- Model training retains the existing `model/...` tag for a newly produced, verifiable immutable model artifact. Training, activation, or reuse of an existing model does not mint a bot or ingester image version. Packaging changed model runtime inputs into an image does enter the affected image's normal CI/CD flow.
- Grafana, Prometheus, Loki, and Alloy configuration is provisioned through its owning chart and records the existing `provisioned/observability/...` tag. A configuration-only provision does not mint a bot or ingester image version.
- `db-migrate` has its own candidate image and approved one-shot migration process; ordinary bot or ingester deployment does not run migrations.

## Finishing a version: proposed convention

Final release tagging and opening a new base version are not automated yet. A future finalization should explicitly select one accepted RC per component, confirm production readiness and the immutable registry digest, and record the final tag against that exact accepted source and image identity. Do not infer finality from an RC count or elapsed time. Choose the next base version per component from the change: patch for a compatible fix, minor for compatible new behavior, or major for a breaking change. On that new base, local candidates start at `local.1` and RCs at `rc.0`. Define the final Git/image tag command and version rollover before the first final release; do not silently advance either from a checkpoint.
