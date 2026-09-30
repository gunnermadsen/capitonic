provider "cloudflare" {}

locals {
  cloudflare_tunnel_dns_target = "${var.cloudflare_tunnel_id}.cfargotunnel.com"
}

resource "cloudflare_dns_record" "monitor" {
  zone_id = var.cloudflare_zone_id
  name    = var.cloudflare_monitor_hostname
  content = aws_instance.k3s_host.public_ip
  type    = "A"
  ttl     = 1
  proxied = true
  comment = "Managed by Terraform for the public Capitonic production Grafana ingress"
}

resource "cloudflare_dns_record" "api" {
  zone_id = var.cloudflare_zone_id
  name    = var.cloudflare_api_hostname
  content = aws_instance.k3s_host.public_ip
  type    = "A"
  ttl     = 1
  proxied = true
  comment = "Managed by Terraform for the Capitonic production API ingress"
}

resource "cloudflare_dns_record" "metrics" {
  zone_id = var.cloudflare_zone_id
  name    = var.cloudflare_metrics_hostname
  content = aws_instance.k3s_host.public_ip
  type    = "A"
  ttl     = 1
  proxied = true
  comment = "Managed by Terraform for the Capitonic production Prometheus ingress"
}

resource "cloudflare_dns_record" "ops" {
  zone_id = var.cloudflare_zone_id
  name    = var.cloudflare_ops_hostname
  content = local.cloudflare_tunnel_dns_target
  type    = "CNAME"
  ttl     = 1
  proxied = true
  comment = "Managed by Terraform for the Capitonic production Argo CD tunnel"
}

resource "cloudflare_dns_record" "ssh_ops" {
  zone_id = var.cloudflare_zone_id
  name    = var.cloudflare_ssh_hostname
  content = local.cloudflare_tunnel_dns_target
  type    = "CNAME"
  ttl     = 1
  proxied = true
  comment = "Managed by Terraform for the Capitonic production SSH tunnel"
}

resource "cloudflare_ruleset" "production_https_redirect" {
  zone_id = var.cloudflare_zone_id
  name    = "Capitonic production HTTPS redirects"
  kind    = "zone"
  phase   = "http_request_dynamic_redirect"

  rules = [{
    ref         = "capitonic_production_https_redirect"
    description = "Upgrade the four production HTTP hostnames to HTTPS"
    expression  = "not ssl and http.host in {\"${var.cloudflare_monitor_hostname}\" \"${var.cloudflare_api_hostname}\" \"${var.cloudflare_metrics_hostname}\" \"${var.cloudflare_ops_hostname}\"}"
    action      = "redirect"
    action_parameters = {
      from_value = {
        status_code           = 308
        preserve_query_string = true
        target_url = {
          expression = "concat(\"https://\", http.host, http.request.uri.path)"
        }
      }
    }
  }]
}

resource "cloudflare_ruleset" "production_hsts" {
  zone_id = var.cloudflare_zone_id
  name    = "Capitonic production HSTS"
  kind    = "zone"
  phase   = "http_response_headers_transform"

  rules = [{
    ref         = "capitonic_production_hsts"
    description = "Require HTTPS on subsequent browser visits to production hosts"
    expression  = "ssl and http.host in {\"${var.cloudflare_monitor_hostname}\" \"${var.cloudflare_api_hostname}\" \"${var.cloudflare_metrics_hostname}\" \"${var.cloudflare_ops_hostname}\"}"
    action      = "rewrite"
    action_parameters = {
      headers = {
        Strict-Transport-Security = {
          operation = "set"
          value     = "max-age=300"
        }
      }
    }
  }]
}

resource "cloudflare_zero_trust_access_application" "stack_ssh" {
  zone_id          = var.cloudflare_zone_id
  name             = "Capitonic production stack SSH"
  domain           = var.cloudflare_ssh_hostname
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
