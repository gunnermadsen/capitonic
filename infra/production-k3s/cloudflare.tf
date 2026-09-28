provider "cloudflare" {}

locals {
  cloudflare_tunnel_dns_target = "${var.cloudflare_tunnel_id}.cfargotunnel.com"
}

resource "cloudflare_dns_record" "monitor" {
  zone_id = var.cloudflare_zone_id
  name    = var.cloudflare_monitor_hostname
  content = local.cloudflare_tunnel_dns_target
  type    = "CNAME"
  ttl     = 1
  proxied = true
  comment = "Managed by Terraform for the Capitonic production Grafana tunnel"
}

resource "cloudflare_zero_trust_access_application" "monitor" {
  zone_id          = var.cloudflare_zone_id
  name             = "Capitonic production monitoring"
  domain           = var.cloudflare_monitor_hostname
  type             = "self_hosted"
  session_duration = "8h"

  policies = [{
    name       = "Allow production operator"
    decision   = "allow"
    precedence = 1
    include = [{
      email = { email = var.cloudflare_access_email }
    }]
  }]
}
