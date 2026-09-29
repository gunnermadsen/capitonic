#!/usr/bin/env bash
set -euo pipefail
set +x

AWS_REGION="${AWS_REGION:-eu-west-1}"
APP_SECRET_NAME="${APP_SECRET_NAME:-capitonic/polymarket-bot/production}"
SSH_USER="${CAPITONIC_SSH_USER:-ubuntu}"

[[ "$AWS_REGION" == "eu-west-1" ]]
[[ "$(id -u)" == "0" ]]
id "$SSH_USER" >/dev/null 2>&1

temporary_directory="$(mktemp -d /run/capitonic-ssh.XXXXXX)"
trap 'rm -rf "$temporary_directory"' EXIT
chmod 0700 "$temporary_directory"
secret_json="$temporary_directory/secret.json"
authorized_keys_source="$temporary_directory/authorized_keys"

aws secretsmanager get-secret-value \
  --region "$AWS_REGION" \
  --secret-id "$APP_SECRET_NAME" \
  --query SecretString \
  --output text >"$secret_json"
chmod 0600 "$secret_json"
ssh_password="$(jq -er '.RDP_PASSWORD | strings | select(length > 0)' "$secret_json")" || {
  echo "AWS Secrets Manager secret $APP_SECRET_NAME must contain RDP_PASSWORD for the protected SSH account." >&2
  exit 1
}
jq -r '.SSH_AUTHORIZED_KEYS // ""' "$secret_json" >"$authorized_keys_source"

key_count=0
while IFS= read -r public_key; do
  [[ -z "${public_key//[[:space:]]/}" ]] && continue
  [[ "$public_key" =~ ^(ssh-ed25519|ssh-rsa|ecdsa-sha2-nistp(256|384|521))[[:space:]]+[A-Za-z0-9+/=]+([[:space:]].*)?$ ]] || {
    echo "SSH_AUTHORIZED_KEYS contains an unsupported or malformed public key." >&2
    exit 1
  }
  key_count=$((key_count + 1))
done <"$authorized_keys_source"

if ! dpkg-query -W -f='${Status}' openssh-server 2>/dev/null | grep -qx 'install ok installed'; then
  apt-get update
  DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends openssh-server
  apt-get clean
  rm -rf /var/lib/apt/lists/*
fi

ssh_home="$(getent passwd "$SSH_USER" | cut -d: -f6)"
ssh_group="$(id -gn "$SSH_USER")"
printf '%s:%s\n' "$SSH_USER" "$ssh_password" | chpasswd
unset ssh_password
if (( key_count > 0 )); then
  install -d -m 0700 -o "$SSH_USER" -g "$ssh_group" "$ssh_home/.ssh"
  install -m 0600 -o "$SSH_USER" -g "$ssh_group" "$authorized_keys_source" "$ssh_home/.ssh/authorized_keys"
fi

cat >/etc/ssh/sshd_config.d/90-capitonic-tunnel.conf <<EOF
AddressFamily inet
ListenAddress 127.0.0.1
PasswordAuthentication yes
KbdInteractiveAuthentication no
PermitRootLogin no
AllowUsers $SSH_USER
EOF

install -d -m 0755 /run/sshd
/usr/sbin/sshd -t
systemctl unmask ssh.service >/dev/null
systemctl disable --now ssh.socket >/dev/null 2>&1 || true
systemctl enable --now ssh.service

ss -ltnH '( sport = :22 )' | awk '{print $4}' | grep -qx '127.0.0.1:22'
[[ "$(ss -ltnH '( sport = :22 )' | wc -l | tr -d ' ')" == "1" ]]

echo "Configured protected SSH on 127.0.0.1:22 for Cloudflare Tunnel access only."
