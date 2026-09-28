#!/usr/bin/env bash
set -euo pipefail
set +x

version="v4.47.2"
expected_sha256="05df1f6aed334f223bb3e6a967db259f7185e33650c3b6447625e16fea0ed31f"
target="/usr/local/bin/yq"

case "$(uname -m)" in
  aarch64|arm64) ;;
  *) echo "Production yq bootstrap requires an ARM64 host." >&2; exit 1 ;;
esac

if [[ -x "$target" ]] && [[ "$(sha256sum "$target" | cut -d' ' -f1)" == "$expected_sha256" ]]; then
  exit 0
fi

temporary_file="$(mktemp /tmp/capitonic-yq.XXXXXX)"
trap 'rm -f "$temporary_file"' EXIT
curl -fsSL "https://github.com/mikefarah/yq/releases/download/$version/yq_linux_arm64" -o "$temporary_file"
echo "$expected_sha256  $temporary_file" | sha256sum -c -
install -m 0755 "$temporary_file" "$target"
