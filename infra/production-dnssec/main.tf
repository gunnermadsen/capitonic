terraform {
  required_version = ">= 1.10.0"
  backend "s3" {}

  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = "~> 5.0"
    }
    cloudflare = {
      source  = "cloudflare/cloudflare"
      version = "~> 5.0"
    }
  }
}

provider "aws" {
  region = "us-east-1"
}

provider "cloudflare" {}

variable "cloudflare_zone_id" {
  type = string
}

import {
  to = cloudflare_zone_dnssec.capitonic
  id = var.cloudflare_zone_id
}

resource "cloudflare_zone_dnssec" "capitonic" {
  zone_id = var.cloudflare_zone_id
  status  = "active"

  lifecycle {
    prevent_destroy = true
  }
}

resource "aws_route53domains_delegation_signer_record" "capitonic" {
  domain_name = "capitonic.com"

  signing_attributes {
    algorithm  = tonumber(cloudflare_zone_dnssec.capitonic.algorithm)
    flags      = cloudflare_zone_dnssec.capitonic.flags
    public_key = cloudflare_zone_dnssec.capitonic.public_key
  }

  lifecycle {
    prevent_destroy = true
    # AWS omits flags/public_key on reads; the workflow verifies the live DS digest.
    ignore_changes = [signing_attributes]
  }
}

output "expected_ds" {
  value = join(" ", [
    tostring(cloudflare_zone_dnssec.capitonic.key_tag),
    cloudflare_zone_dnssec.capitonic.algorithm,
    cloudflare_zone_dnssec.capitonic.digest_type,
    upper(cloudflare_zone_dnssec.capitonic.digest),
  ])
}
