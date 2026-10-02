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
This configuration is for the local Rancher Desktop route, not public exposure.
