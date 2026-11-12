variable "name" {
  description = "Prefix for every resource this module creates."
  type        = string
  default     = "hyperbeam"
}

variable "domain" {
  description = "DNS name people use to reach Hyperbeam, e.g. hyperbeam.example.com. Must resolve to the instance's address before a TLS certificate can be issued."
  type        = string
}

variable "acme_email" {
  description = "Email address Let's Encrypt sends certificate expiry notices to."
  type        = string
}

variable "route53_zone_id" {
  description = "ID of a Route 53 hosted zone to create the DNS record in. Leave null to create the record yourself."
  type        = string
  default     = null
}

variable "instance_type" {
  description = "EC2 instance type. Its architecture (arm64 or x86_64) decides which images you build. Use a Nitro instance type."
  type        = string
  default     = "t4g.xlarge"
}

variable "root_volume_gb" {
  description = "Size of the root disk, which holds container images and sandbox workspaces (up to 10 GiB per running session)."
  type        = number
  default     = 100
}

variable "data_volume_gb" {
  description = "Size of the volume that holds Postgres data."
  type        = number
  default     = 20
}

variable "snapshot_retention_days" {
  description = "How many daily snapshots of the data volume to keep."
  type        = number
  default     = 7
}

variable "max_sessions" {
  description = "Most sandboxes running at once. Others queue. Roughly 1.5 per vCPU is a reasonable start."
  type        = number
  default     = 6
}

variable "harnesses" {
  description = "Harness images to create a repository for. Each is a target in deploy/docker/Dockerfile.harness."
  type        = list(string)
  default     = ["claude-code"]
}

variable "allowed_cidrs" {
  description = "Networks that may reach the UI on ports 80 and 443. Let's Encrypt must be able to reach port 443 to issue and renew the certificate."
  type        = list(string)
  default     = ["0.0.0.0/0"]
}

variable "k3s_channel" {
  description = "k3s release channel to install: stable, latest, or a minor version such as v1.33."
  type        = string
  default     = "stable"
}
