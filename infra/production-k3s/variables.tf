variable "aws_region" {
  description = "AWS region for every production runtime resource."
  type        = string
  default     = "eu-west-1"

  validation {
    condition     = var.aws_region == "eu-west-1"
    error_message = "The production runtime must be deployed in eu-west-1 (Ireland)."
  }
}

variable "name" {
  description = "Name prefix for the single-node production runtime."
  type        = string
  default     = "capitonic-polymarket-bot"
}

variable "environment" {
  description = "Environment tag value."
  type        = string
  default     = "production"
}

variable "instance_type" {
  description = "ARM64 production trading host (4 vCPU, 8 GiB)."
  type        = string
  default     = "c7g.xlarge"

  validation {
    condition     = var.instance_type == "c7g.xlarge" || var.instance_type == "c6g.xlarge"
    error_message = "Use the approved ARM64 c7g.xlarge or c6g.xlarge production size."
  }
}

variable "ami_id" {
  description = "Optional ARM64 Ubuntu 24.04 AMI override."
  type        = string
  default     = null
}

variable "subnet_id" {
  description = "Optional subnet override; defaults to the first default subnet."
  type        = string
  default     = null
}

variable "root_volume_size_gib" {
  description = "Encrypted gp3 operating-system volume size."
  type        = number
  default     = 20

  validation {
    condition     = var.root_volume_size_gib >= 20
    error_message = "root_volume_size_gib must be at least 20 GiB."
  }
}

variable "data_volume_size_gib" {
  description = "Encrypted gp3 k3s data volume size."
  type        = number
  default     = 60

  validation {
    condition     = var.data_volume_size_gib >= 40
    error_message = "data_volume_size_gib must be at least 40 GiB."
  }
}

variable "repo_url" {
  description = "GitHub HTTPS repository URL checked out by the host."
  type        = string
  default     = "https://github.com/gunnermadsen/capitonic.git"
}

variable "repo_branch" {
  description = "Bootstrap branch; deployments later select an immutable commit."
  type        = string
  default     = "production"
}

variable "app_directory" {
  description = "Host path for the immutable deployment checkout."
  type        = string
  default     = "/opt/polymarket-bot"
}

variable "app_secret_name" {
  description = "Ireland AWS Secrets Manager JSON secret used by the runtime."
  type        = string
  default     = "capitonic/polymarket-bot/production"
}

variable "backup_bucket_name" {
  description = "Ireland S3 bucket that retains validated pre-destroy backups."
  type        = string
  default     = "capitonic-polybot-backups-192200846560-euw1"
}

variable "ecr_registry" {
  description = "Ireland ECR registry used by production images."
  type        = string
  default     = "192200846560.dkr.ecr.eu-west-1.amazonaws.com"
}

variable "k3s_version" {
  description = "Pinned k3s release installed by cloud-init."
  type        = string
  default     = "v1.33.4+k3s1"
}

variable "cloudflare_zone_id" {
  description = "Cloudflare zone ID for capitonic.com."
  type        = string
  sensitive   = true
}

variable "cloudflare_account_id" {
  description = "Cloudflare account containing the production SSH tunnel and Access application."
  type        = string
  sensitive   = true
}

variable "cloudflare_access_email" {
  description = "Operator email allowed through Cloudflare Access."
  type        = string
  sensitive   = true
}

variable "cloudflare_tunnel_id" {
  description = "Existing production Cloudflare Tunnel UUID used only for SSH access."
  type        = string
  sensitive   = true
}

variable "cloudflare_monitor_hostname" {
  description = "Public Cloudflare-proxied hostname routed directly to the k3s Grafana HTTPS ingress."
  type        = string
  default     = "monitor.capitonic.com"
}

variable "cloudflare_system_hostname" {
  description = "Public Cloudflare-proxied hostname for the production Headlamp ingress."
  type        = string
  default     = "system.capitonic.com"
}

variable "cloudflare_apex_hostname" {
  description = "Public Cloudflare-proxied apex hostname redirected to the Grafana hostname."
  type        = string
  default     = "capitonic.com"
}

variable "cloudflare_api_hostname" {
  description = "Cloudflare-proxied hostname for the production microservice APIs."
  type        = string
  default     = "api.capitonic.com"
}

variable "cloudflare_metrics_hostname" {
  description = "Cloudflare-proxied hostname for the production Prometheus API."
  type        = string
  default     = "metrics.capitonic.com"
}

variable "cloudflare_ops_hostname" {
  description = "Cloudflare Access hostname routed through the production tunnel to Argo CD."
  type        = string
  default     = "ops.capitonic.com"
}

variable "cloudflare_ssh_hostname" {
  description = "Cloudflare Access hostname routed through the production tunnel to loopback SSH."
  type        = string
  default     = "ssh.capitonic.com"
}

variable "cloudflare_tunnel_ipv4_cidrs" {
  description = "Cloudflare Tunnel edge IPv4 addresses permitted on TCP/7844."
  type        = list(string)
  default = [
    "198.41.192.7/32", "198.41.192.27/32", "198.41.192.37/32", "198.41.192.47/32",
    "198.41.192.57/32", "198.41.192.67/32", "198.41.192.77/32", "198.41.192.107/32",
    "198.41.192.167/32", "198.41.192.227/32", "198.41.200.13/32", "198.41.200.23/32",
    "198.41.200.33/32", "198.41.200.43/32", "198.41.200.53/32", "198.41.200.63/32",
    "198.41.200.73/32", "198.41.200.113/32", "198.41.200.193/32", "198.41.200.233/32"
  ]
}
