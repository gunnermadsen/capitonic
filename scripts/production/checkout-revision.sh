#!/usr/bin/env bash
set -euo pipefail
set +x

revision="${1:?usage: checkout-revision.sh <40-character-git-revision>}"
[[ "$revision" =~ ^[0-9a-f]{40}$ ]]

AWS_REGION="${AWS_REGION:-eu-west-1}"
APP_DIRECTORY="${APP_DIRECTORY:-/opt/polymarket-bot}"
APP_SECRET_NAME="${APP_SECRET_NAME:-capitonic/polymarket-bot/production}"
REPO_URL="${REPO_URL:-https://github.com/gunnermadsen/capitonic.git}"
[[ "$AWS_REGION" == "eu-west-1" ]]

temporary_directory="$(mktemp -d /run/capitonic-checkout.XXXXXX)"
trap 'rm -rf "$temporary_directory"' EXIT
chmod 0700 "$temporary_directory"
aws secretsmanager get-secret-value --region "$AWS_REGION" --secret-id "$APP_SECRET_NAME" \
  --query SecretString --output text >"$temporary_directory/secret.json"
chmod 0600 "$temporary_directory/secret.json"
username="$(jq -er '.GITHUB_USERNAME' "$temporary_directory/secret.json")"
pat="$(jq -er '.GITHUB_PAT' "$temporary_directory/secret.json")"
printf 'https://%s:%s@github.com\n' "$username" "$pat" >"$temporary_directory/credentials"
chmod 0600 "$temporary_directory/credentials"
unset username pat

if [[ ! -d "$APP_DIRECTORY/.git" ]]; then
  rm -rf "$APP_DIRECTORY"
  git -c credential.helper="store --file=$temporary_directory/credentials" clone "$REPO_URL" "$APP_DIRECTORY"
fi
git -C "$APP_DIRECTORY" -c credential.helper="store --file=$temporary_directory/credentials" \
  fetch --no-tags origin "$revision"
git -C "$APP_DIRECTORY" checkout --detach "$revision"
[[ "$(git -C "$APP_DIRECTORY" rev-parse HEAD)" == "$revision" ]]
git -C "$APP_DIRECTORY" status --porcelain --untracked-files=no | grep -q . && {
  echo "Deployment checkout contains tracked changes." >&2
  exit 1
}

echo "Checked out immutable deployment revision $revision."
