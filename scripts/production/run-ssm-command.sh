#!/usr/bin/env bash
set -euo pipefail
set +x

instance_id="${1:?usage: run-ssm-command.sh <instance-id> <timeout-seconds> <comment> <command>}"
timeout_seconds="${2:?}"
comment="${3:?}"
remote_command="${4:?}"
AWS_REGION="${AWS_REGION:-eu-west-1}"

[[ "$AWS_REGION" == "eu-west-1" ]]
[[ "$instance_id" =~ ^i-[0-9a-f]+$ ]]
[[ "$timeout_seconds" =~ ^[0-9]+$ ]]

encoded="$(printf '%s' "$remote_command" | base64 | tr -d '\n')"
parameters="$(jq -n --arg encoded "$encoded" \
  '{commands:["printf %s " + ($encoded|@sh) + " | base64 -d | /bin/bash"]}')"
command_id="$(aws ssm send-command --region "$AWS_REGION" --instance-ids "$instance_id" \
  --document-name AWS-RunShellScript --parameters "$parameters" --comment "$comment" \
  --timeout-seconds "$timeout_seconds" --query Command.CommandId --output text)"

deadline=$((SECONDS + timeout_seconds + 60))
status=Pending
while (( SECONDS < deadline )); do
  status="$(aws ssm get-command-invocation --region "$AWS_REGION" --command-id "$command_id" \
    --instance-id "$instance_id" --query Status --output text 2>/dev/null || true)"
  case "$status" in
    Success|Failed|TimedOut|Cancelled|Cancelling) break ;;
  esac
  sleep 5
done

result="$(aws ssm get-command-invocation --region "$AWS_REGION" --command-id "$command_id" \
  --instance-id "$instance_id" \
  --query '{Status:Status,ResponseCode:ResponseCode,Output:StandardOutputContent,Error:StandardErrorContent}' \
  --output json)"
jq . <<<"$result"
[[ "$(jq -r .Status <<<"$result")" == "Success" ]]
