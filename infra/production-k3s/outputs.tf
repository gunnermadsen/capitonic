output "instance_id" {
  description = "Single-node k3s production instance ID."
  value       = aws_instance.k3s_host.id
}

output "instance_type" {
  value = aws_instance.k3s_host.instance_type
}

output "architecture" {
  value = "arm64"
}

output "public_ip" {
  value = aws_instance.k3s_host.public_ip
}

output "root_volume_id" {
  value = aws_instance.k3s_host.root_block_device[0].volume_id
}

output "data_volume_id" {
  value = aws_ebs_volume.k3s_data.id
}

output "app_secret_name" {
  value = var.app_secret_name
}

output "backup_bucket_name" {
  value = var.backup_bucket_name
}

output "monitor_hostname" {
  value = var.cloudflare_monitor_hostname
}

output "ssm_start_session_command" {
  value = "aws ssm start-session --region ${var.aws_region} --target ${aws_instance.k3s_host.id}"
}
