provider "aws" {
  region = var.aws_region

  default_tags {
    tags = local.common_tags
  }
}

data "aws_caller_identity" "current" {}

data "aws_vpc" "default" {
  default = true
}

data "aws_subnets" "default" {
  filter {
    name   = "vpc-id"
    values = [data.aws_vpc.default.id]
  }

  filter {
    name   = "default-for-az"
    values = ["true"]
  }
}

data "aws_subnet" "selected" {
  id = local.subnet_id
}

data "aws_ssm_parameter" "ubuntu_ami" {
  name = local.ami_ssm_parameter_name
}

data "aws_secretsmanager_secret" "app" {
  name = var.app_secret_name
}

data "aws_s3_bucket" "backups" {
  bucket = var.backup_bucket_name
}

resource "aws_iam_role" "k3s_host" {
  name = "${local.name_prefix}-host"

  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect    = "Allow"
      Principal = { Service = "ec2.amazonaws.com" }
      Action    = "sts:AssumeRole"
    }]
  })
}

resource "aws_iam_role_policy" "k3s_host" {
  name = "${local.name_prefix}-runtime"
  role = aws_iam_role.k3s_host.id

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Sid      = "ReadOnlyProductionSecret"
        Effect   = "Allow"
        Action   = ["secretsmanager:DescribeSecret", "secretsmanager:GetSecretValue", "secretsmanager:ListSecretVersionIds"]
        Resource = data.aws_secretsmanager_secret.app.arn
      },
      {
        Sid      = "EcrLogin"
        Effect   = "Allow"
        Action   = "ecr:GetAuthorizationToken"
        Resource = "*"
      },
      {
        Sid    = "PullPinnedProductionImages"
        Effect = "Allow"
        Action = [
          "ecr:BatchCheckLayerAvailability", "ecr:BatchGetImage", "ecr:GetDownloadUrlForLayer"
        ]
        Resource = [
          for repository in ["polymarket-bot", "ingester", "db-migrate"] :
          "arn:aws:ecr:${var.aws_region}:${data.aws_caller_identity.current.account_id}:repository/capitonic/${repository}"
        ]
      },
      {
        Sid      = "WriteValidatedBackups"
        Effect   = "Allow"
        Action   = ["s3:AbortMultipartUpload", "s3:GetObject", "s3:ListBucket", "s3:PutObject"]
        Resource = [data.aws_s3_bucket.backups.arn, "${data.aws_s3_bucket.backups.arn}/production/*"]
      }
    ]
  })
}

resource "aws_iam_role_policy_attachment" "ssm_core" {
  role       = aws_iam_role.k3s_host.name
  policy_arn = "arn:aws:iam::aws:policy/AmazonSSMManagedInstanceCore"
}

resource "aws_iam_instance_profile" "k3s_host" {
  name = "${local.name_prefix}-host"
  role = aws_iam_role.k3s_host.name
}

resource "aws_security_group" "k3s_host" {
  name        = "${local.name_prefix}-sg"
  description = "Outbound-only access for the single-node Capitonic k3s runtime"
  vpc_id      = data.aws_vpc.default.id

  tags = { Name = "${local.name_prefix}-sg" }
}

resource "aws_vpc_security_group_ingress_rule" "https" {
  security_group_id = aws_security_group.k3s_host.id
  description       = "Public HTTPS ingress to Traefik; SSH remains Cloudflare Tunnel only"
  cidr_ipv4         = "0.0.0.0/0"
  from_port         = 443
  ip_protocol       = "tcp"
  to_port           = 443
}

resource "aws_vpc_security_group_egress_rule" "https" {
  security_group_id = aws_security_group.k3s_host.id
  description       = "TLS egress for AWS, GitHub, registries, providers, and package repositories"
  cidr_ipv4         = "0.0.0.0/0"
  from_port         = 443
  ip_protocol       = "tcp"
  to_port           = 443
}

resource "aws_vpc_security_group_egress_rule" "cloudflare_tunnel_http2" {
  for_each = toset(var.cloudflare_tunnel_ipv4_cidrs)

  security_group_id = aws_security_group.k3s_host.id
  description       = "Cloudflare Tunnel HTTP/2 egress to ${each.value}"
  cidr_ipv4         = each.value
  from_port         = 7844
  ip_protocol       = "tcp"
  to_port           = 7844
}

resource "aws_vpc_security_group_egress_rule" "dns_udp" {
  security_group_id = aws_security_group.k3s_host.id
  description       = "DNS through the VPC resolver"
  cidr_ipv4         = data.aws_vpc.default.cidr_block
  from_port         = 53
  ip_protocol       = "udp"
  to_port           = 53
}

resource "aws_vpc_security_group_egress_rule" "dns_tcp" {
  security_group_id = aws_security_group.k3s_host.id
  description       = "DNS TCP through the VPC resolver"
  cidr_ipv4         = data.aws_vpc.default.cidr_block
  from_port         = 53
  ip_protocol       = "tcp"
  to_port           = 53
}

resource "aws_vpc_security_group_egress_rule" "ntp" {
  security_group_id = aws_security_group.k3s_host.id
  description       = "NTP for signed requests and market timestamps"
  cidr_ipv4         = "0.0.0.0/0"
  from_port         = 123
  ip_protocol       = "udp"
  to_port           = 123
}

resource "aws_ebs_volume" "k3s_data" {
  availability_zone = data.aws_subnet.selected.availability_zone
  size              = var.data_volume_size_gib
  type              = "gp3"
  encrypted         = true

  tags = { Name = "${local.name_prefix}-k3s-data" }
}

resource "aws_instance" "k3s_host" {
  ami                         = local.ami_id
  instance_type               = var.instance_type
  subnet_id                   = local.subnet_id
  vpc_security_group_ids      = [aws_security_group.k3s_host.id]
  associate_public_ip_address = true
  iam_instance_profile        = aws_iam_instance_profile.k3s_host.name
  user_data_replace_on_change = true

  user_data_base64 = base64gzip(templatefile("${path.module}/templates/user-data.sh.tftpl", {
    app_directory   = var.app_directory
    app_secret_name = var.app_secret_name
    aws_region      = var.aws_region
    ecr_registry    = var.ecr_registry
    k3s_version     = var.k3s_version
    repo_branch     = var.repo_branch
    repo_url        = var.repo_url
  }))

  metadata_options {
    http_endpoint               = "enabled"
    http_tokens                 = "required"
    http_put_response_hop_limit = 1
  }

  root_block_device {
    volume_type           = "gp3"
    volume_size           = var.root_volume_size_gib
    encrypted             = true
    delete_on_termination = true
  }

  tags = {
    Name      = local.name_prefix
    Component = "trading-runtime"
  }

  lifecycle {
    precondition {
      condition     = local.subnet_id != null
      error_message = "No default subnet was found; set subnet_id explicitly."
    }
  }

  depends_on = [aws_iam_role_policy.k3s_host, aws_iam_role_policy_attachment.ssm_core]
}

resource "aws_volume_attachment" "k3s_data" {
  device_name = "/dev/sdf"
  volume_id   = aws_ebs_volume.k3s_data.id
  instance_id = aws_instance.k3s_host.id
}
