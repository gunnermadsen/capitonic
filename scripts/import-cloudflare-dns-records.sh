#!/usr/bin/env bash
set -euo pipefail

TF_DIR="${TF_DIR:-infra/production-k3s}"

: "${CLOUDFLARE_API_TOKEN:?CLOUDFLARE_API_TOKEN is required}"
: "${TF_VAR_cloudflare_account_id:?TF_VAR_cloudflare_account_id is required}"
: "${TF_VAR_cloudflare_zone_id:?TF_VAR_cloudflare_zone_id is required}"
: "${TF_VAR_cloudflare_monitor_hostname:?TF_VAR_cloudflare_monitor_hostname is required}"
: "${TF_VAR_cloudflare_ssh_hostname:?TF_VAR_cloudflare_ssh_hostname is required}"
: "${TF_VAR_cloudflare_api_hostname:?TF_VAR_cloudflare_api_hostname is required}"
: "${TF_VAR_cloudflare_metrics_hostname:?TF_VAR_cloudflare_metrics_hostname is required}"
: "${TF_VAR_cloudflare_ops_hostname:?TF_VAR_cloudflare_ops_hostname is required}"

import_record_if_present() {
  local address hostname response count record_id
  address="$1"
  hostname="$2"

  if terraform -chdir="$TF_DIR" state list | grep -qx "$address"; then
    echo "$address is already managed in Terraform state."
    return
  fi

  response="$(
    curl -fsS \
      -H "Authorization: Bearer $CLOUDFLARE_API_TOKEN" \
      -H "Content-Type: application/json" \
      "https://api.cloudflare.com/client/v4/zones/$TF_VAR_cloudflare_zone_id/dns_records?name=$hostname"
  )"

  count="$(jq -r '.result | length' <<<"$response")"
  case "$count" in
    0)
      echo "No existing Cloudflare DNS record found for $hostname; Terraform will create it."
      ;;
    1)
      record_id="$(jq -r '.result[0].id' <<<"$response")"
      echo "Importing existing Cloudflare DNS record for $hostname into $address."
      terraform -chdir="$TF_DIR" import -input=false "$address" "$TF_VAR_cloudflare_zone_id/$record_id"
      ;;
    *)
      echo "Multiple Cloudflare DNS records found for $hostname; refusing to choose one." >&2
      exit 1
      ;;
  esac
}

import_record_if_present cloudflare_dns_record.monitor "$TF_VAR_cloudflare_monitor_hostname"
import_record_if_present cloudflare_dns_record.ssh_ops "$TF_VAR_cloudflare_ssh_hostname"
import_record_if_present cloudflare_dns_record.api "$TF_VAR_cloudflare_api_hostname"
import_record_if_present cloudflare_dns_record.metrics "$TF_VAR_cloudflare_metrics_hostname"
import_record_if_present cloudflare_dns_record.ops "$TF_VAR_cloudflare_ops_hostname"

import_access_if_present() {
  local access_address="$1" hostname="$2" access_response access_count access_id
  if terraform -chdir="$TF_DIR" state list | grep -qx "$access_address"; then
    echo "$access_address is already managed in Terraform state."
    return
  fi
  access_response="$(curl -fsS -H "Authorization: Bearer $CLOUDFLARE_API_TOKEN" \
    "https://api.cloudflare.com/client/v4/accounts/$TF_VAR_cloudflare_account_id/access/apps")"
  access_count="$(jq -r --arg hostname "$hostname" \
    '[.result[] | select(.domain == $hostname)] | length' <<<"$access_response")"
  case "$access_count" in
    0)
      echo "No existing Cloudflare Access application found for $hostname; Terraform will create it."
      ;;
    1)
      access_id="$(jq -r --arg hostname "$hostname" \
        '.result[] | select(.domain == $hostname) | .id' <<<"$access_response")"
      echo "Importing the existing Cloudflare Access application for $hostname."
      terraform -chdir="$TF_DIR" import -input=false "$access_address" "$TF_VAR_cloudflare_zone_id/$access_id"
      ;;
    *)
      echo "Multiple Cloudflare Access applications found for $hostname; refusing to choose one." >&2
      exit 1
      ;;
  esac
}

import_access_if_present cloudflare_zero_trust_access_application.stack_ssh "$TF_VAR_cloudflare_ssh_hostname"
import_access_if_present cloudflare_zero_trust_access_application.stack_ops "$TF_VAR_cloudflare_ops_hostname"
