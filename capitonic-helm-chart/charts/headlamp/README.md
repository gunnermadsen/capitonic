# Local Headlamp access

Open `http://localhost/system/` and use `HEADLAMP_USERNAME` and `HEADLAMP_PASSWORD`
from the main worktree's ignored, mode-0600 `.env.headlamp`. Feature worktrees
symlink that file; do not copy or commit credentials.

Run `helm dependency build capitonic-helm-chart/charts/headlamp` first on a fresh
checkout, then `python3 scripts/helm/provision-headlamp.py --check` to lint and render,
or `python3 scripts/helm/provision-headlamp.py` to deploy only the local Headlamp release.
After editing the canonical credentials, repeat provisioning to update the login.

The provisioning helper derives `HEADLAMP_PASSWORD_HASH` from `HEADLAMP_PASSWORD`
using bcrypt. The chart combines `HEADLAMP_USERNAME:HEADLAMP_PASSWORD_HASH` into
Traefik's required Secret `users` format. Plaintext passwords never enter Helm
values or manifests. No credential aliases are used.

Traefik Basic Auth protects all `/system` requests and removes its Authorization
header before forwarding. Headlamp uses the automatically rotated pod service
account token with existing read-only viewer permissions. A NetworkPolicy allows
backend connections only from Traefik; keep it and the password middleware in place.
The default configuration is for the local Rancher Desktop route.

## Production provisioning

`deploy-production-headlamp.yml` accepts `workflow_dispatch` and `workflow_call`
with a committed `revision`. It deploys only Headlamp through the existing AWS
OIDC role and production SSM runner. The EC2 provisioning workflow calls it after
DNS and ESO installation; Terraform `plan` skips it.
For a fresh full-stack bootstrap it passes `deploy_headlamp: false` and invokes
Headlamp after stack deployment has provisioned the certificate issuer.

Production values expose `https://system.capitonic.com/` through existing
Traefik TLS and a chart-ingress certificate issued by `letsencrypt-production`.
The existing production DNS workflow provisions its Cloudflare-proxied A record
against the production server IP and includes it in HTTPS and HSTS rules.
ESO reads `HEADLAMP_USERNAME` and
`HEADLAMP_PASSWORD` from the admitted immutable AWS secret version and creates
only a bcrypt `users` entry in `headlamp-basic-auth`. Helm does not own that Secret.
The chart owns the SecretStore, ExternalSecret, middleware and viewer resources.

To rotate production credentials, upload `.env.headlamp` with the existing
`scripts/sync-aws-secret.mjs`, then run
`python3 scripts/production/prepare-runtime-secrets.py --component headlamp` on
the production host. Commit its selected `externalSecrets` metadata into
`environments/production/headlamp.yaml`, update chart provenance as required,
and deploy that revision. Unrelated AWS property changes retain the existing pin.
This follows the existing OnChange admission policy; AWS uploads alone do not rotate
production access. Application admission defaults remain unchanged.

The deployment helper waits for ESO reconciliation and checks valid/invalid
passwords, fresh node/pod metrics and denied secret access. An attributable
Headlamp failure restores its previous Helm revision or removes only a failed
initial release. Existing application workload specifications are compared before
and after deployment. No EC2 provisioning run is required to deploy Headlamp.
