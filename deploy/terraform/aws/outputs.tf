output "public_ip" {
  description = "Address to point the domain's A record at."
  value       = aws_eip.this.public_ip
}

output "instance_id" {
  description = "The instance, for SSM sessions and port forwards."
  value       = aws_instance.this.id
}

output "region" {
  value = local.region
}

output "registry" {
  description = "ECR registry to log Docker in to."
  value       = local.registry
}

output "image_prefix" {
  description = "Push images to IMAGE_PREFIX/server, IMAGE_PREFIX/runner and IMAGE_PREFIX/harness-NAME."
  value       = "${local.registry}/${var.name}"
}

output "image_platform" {
  description = "Platform to build images for."
  value       = "linux/${local.arch}"
}

output "kubeconfig_parameter" {
  description = "SSM parameter the instance writes its kubeconfig to."
  value       = aws_ssm_parameter.kubeconfig.name
}

output "helm_values" {
  description = "Values for the Helm chart: terraform output -raw helm_values > hyperbeam-values.yaml"
  value = yamlencode({
    publicUrl = "https://${var.domain}"
    image     = { repository = aws_ecr_repository.this["server"].repository_url }
    agents = {
      runnerImage = { repository = aws_ecr_repository.this["runner"].repository_url }
      maxSessions = var.max_sessions
    }
    aws = {
      kmsKeyId = aws_kms_key.master.arn
      region   = local.region
    }
    ingress = {
      enabled   = true
      className = "traefik"
      host      = var.domain
      annotations = {
        "traefik.ingress.kubernetes.io/router.entrypoints"      = "websecure"
        "traefik.ingress.kubernetes.io/router.tls"              = "true"
        "traefik.ingress.kubernetes.io/router.tls.certresolver" = "letsencrypt"
      }
    }
  })
}
