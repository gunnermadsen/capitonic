#!/usr/bin/env bash
set -euo pipefail
set +x

AWS_REGION="${AWS_REGION:-eu-west-1}"
ACCOUNT_ID="${AWS_ACCOUNT_ID:-$(aws sts get-caller-identity --query Account --output text)}"
STATE_BUCKET="${TF_STATE_BUCKET:-capitonic-polybot-tfstate-$ACCOUNT_ID-euw1}"
BACKUP_BUCKET="${BACKUP_BUCKET:-capitonic-polybot-backups-$ACCOUNT_ID-euw1}"
APP_SECRET_NAME="${APP_SECRET_NAME:-capitonic/polymarket-bot/production}"

[[ "$AWS_REGION" == "eu-west-1" ]]

ensure_bucket() {
  local bucket="$1"
  if ! aws s3api head-bucket --bucket "$bucket" 2>/dev/null; then
    aws s3api create-bucket --bucket "$bucket" --region "$AWS_REGION" \
      --create-bucket-configuration LocationConstraint="$AWS_REGION" >/dev/null
  fi
  aws s3api put-public-access-block --bucket "$bucket" --public-access-block-configuration \
    BlockPublicAcls=true,IgnorePublicAcls=true,BlockPublicPolicy=true,RestrictPublicBuckets=true
  aws s3api put-bucket-encryption --bucket "$bucket" --server-side-encryption-configuration \
    '{"Rules":[{"ApplyServerSideEncryptionByDefault":{"SSEAlgorithm":"AES256"},"BucketKeyEnabled":true}]}'
  aws s3api put-bucket-versioning --bucket "$bucket" --versioning-configuration Status=Enabled
  location="$(aws s3api get-bucket-location --bucket "$bucket" --query LocationConstraint --output text)"
  [[ "$location" == "eu-west-1" ]]
}

ensure_bucket "$STATE_BUCKET"
ensure_bucket "$BACKUP_BUCKET"

for repository in capitonic/polymarket-bot capitonic/ingester capitonic/db-migrate; do
  aws ecr describe-repositories --region "$AWS_REGION" --repository-names "$repository" >/dev/null 2>&1 || \
    aws ecr create-repository --region "$AWS_REGION" --repository-name "$repository" \
      --image-scanning-configuration scanOnPush=true \
      --encryption-configuration encryptionType=AES256 >/dev/null
  aws ecr put-image-tag-mutability --region "$AWS_REGION" --repository-name "$repository" \
    --image-tag-mutability IMMUTABLE >/dev/null
done

if ! aws secretsmanager describe-secret --region "$AWS_REGION" --secret-id "$APP_SECRET_NAME" >/dev/null 2>&1; then
  echo "AWS Secrets Manager secret $APP_SECRET_NAME must be created by scripts/sync-aws-secret.mjs before provisioning." >&2
  exit 1
fi
secret_region="$(aws secretsmanager describe-secret --region "$AWS_REGION" --secret-id "$APP_SECRET_NAME" \
  --query ARN --output text | cut -d: -f4)"
[[ "$secret_region" == "eu-west-1" ]]

printf 'Foundation ready: state=%s backups=%s region=%s\n' "$STATE_BUCKET" "$BACKUP_BUCKET" "$AWS_REGION"
