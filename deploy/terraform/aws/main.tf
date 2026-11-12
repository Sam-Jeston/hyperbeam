# Hyperbeam on one EC2 instance running k3s.
#
# Terraform creates the instance and what it depends on: a small VPC, image
# repositories, a KMS key for the master key, a volume for Postgres with daily
# snapshots. You then install the Helm chart with the values this module
# outputs. See docs/content/install-aws.md.

terraform {
  required_version = ">= 1.5"

  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = ">= 6.0"
    }
  }
}

data "aws_region" "current" {}
data "aws_caller_identity" "current" {}

data "aws_availability_zones" "available" {
  state = "available"
}

data "aws_ec2_instance_type" "this" {
  instance_type = var.instance_type
}

locals {
  region = data.aws_region.current.region
  az     = data.aws_availability_zones.available.names[0]

  # "arm64" or "amd64", as images and release binaries name it.
  arch = contains(data.aws_ec2_instance_type.this.supported_architectures, "arm64") ? "arm64" : "amd64"

  images     = concat(["server", "runner"], [for h in var.harnesses : "harness-${h}"])
  registry   = "${data.aws_caller_identity.current.account_id}.dkr.ecr.${local.region}.amazonaws.com"
  kubeconfig = "/${var.name}/kubeconfig"
}

# --- Network: one public subnet. No NAT gateway, no load balancer. ---

resource "aws_vpc" "this" {
  cidr_block           = "10.42.0.0/24"
  enable_dns_support   = true
  enable_dns_hostnames = true
  tags                 = { Name = var.name }
}

resource "aws_internet_gateway" "this" {
  vpc_id = aws_vpc.this.id
  tags   = { Name = var.name }
}

resource "aws_subnet" "public" {
  vpc_id            = aws_vpc.this.id
  cidr_block        = aws_vpc.this.cidr_block
  availability_zone = local.az
  tags              = { Name = var.name }
}

resource "aws_route_table" "public" {
  vpc_id = aws_vpc.this.id
  tags   = { Name = var.name }

  route {
    cidr_block = "0.0.0.0/0"
    gateway_id = aws_internet_gateway.this.id
  }
}

resource "aws_route_table_association" "public" {
  subnet_id      = aws_subnet.public.id
  route_table_id = aws_route_table.public.id
}

# Only the UI is reachable. Shell and kubectl access go through SSM Session
# Manager, so neither SSH nor the Kubernetes API is open.
resource "aws_security_group" "this" {
  name_prefix = "${var.name}-"
  description = "Hyperbeam: HTTP and HTTPS in, anything out"
  vpc_id      = aws_vpc.this.id
  tags        = { Name = var.name }

  ingress {
    description = "HTTP, redirected to HTTPS"
    from_port   = 80
    to_port     = 80
    protocol    = "tcp"
    cidr_blocks = var.allowed_cidrs
  }

  ingress {
    description = "HTTPS"
    from_port   = 443
    to_port     = 443
    protocol    = "tcp"
    cidr_blocks = var.allowed_cidrs
  }

  egress {
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
  }

  lifecycle {
    create_before_destroy = true
  }
}

# --- Images ---

resource "aws_ecr_repository" "this" {
  for_each = toset(local.images)

  name         = "${var.name}/${each.key}"
  force_delete = true

  image_scanning_configuration {
    scan_on_push = true
  }
}

# --- Master key ---

# The server generates its master key on first start and stores it in
# Postgres wrapped under this key. Deleting this key makes every stored
# credential unreadable.
resource "aws_kms_key" "master" {
  description             = "${var.name}: wraps the Hyperbeam master key"
  enable_key_rotation     = true
  deletion_window_in_days = 30
}

resource "aws_kms_alias" "master" {
  name          = "alias/${var.name}"
  target_key_id = aws_kms_key.master.key_id
}

# --- Instance role ---

resource "aws_iam_role" "instance" {
  name_prefix = "${var.name}-"

  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect    = "Allow"
      Action    = "sts:AssumeRole"
      Principal = { Service = "ec2.amazonaws.com" }
    }]
  })
}

resource "aws_iam_role_policy_attachment" "ssm" {
  role       = aws_iam_role.instance.name
  policy_arn = "arn:aws:iam::aws:policy/AmazonSSMManagedInstanceCore"
}

resource "aws_iam_role_policy" "instance" {
  name = "hyperbeam"
  role = aws_iam_role.instance.id

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Sid      = "MasterKey"
        Effect   = "Allow"
        Action   = ["kms:GenerateDataKey", "kms:Decrypt"]
        Resource = aws_kms_key.master.arn
      },
      {
        Sid      = "RegistryLogin"
        Effect   = "Allow"
        Action   = "ecr:GetAuthorizationToken"
        Resource = "*"
      },
      {
        Sid      = "PullImages"
        Effect   = "Allow"
        Action   = ["ecr:BatchGetImage", "ecr:GetDownloadUrlForLayer", "ecr:BatchCheckLayerAvailability"]
        Resource = [for r in aws_ecr_repository.this : r.arn]
      },
      {
        Sid      = "PublishKubeconfig"
        Effect   = "Allow"
        Action   = "ssm:PutParameter"
        Resource = aws_ssm_parameter.kubeconfig.arn
      },
    ]
  })
}

resource "aws_iam_instance_profile" "this" {
  name_prefix = "${var.name}-"
  role        = aws_iam_role.instance.name
}

# The instance writes its kubeconfig here once k3s is up.
resource "aws_ssm_parameter" "kubeconfig" {
  name        = local.kubeconfig
  description = "Cluster-admin kubeconfig for the Hyperbeam k3s node"
  type        = "SecureString"
  tier        = "Advanced" # a kubeconfig is close to the 4 KB standard limit
  value       = "pending"

  lifecycle {
    ignore_changes = [value]
  }
}

# --- Instance ---

data "aws_ssm_parameter" "ami" {
  name = "/aws/service/canonical/ubuntu/server/24.04/stable/current/${local.arch}/hvm/ebs-gp3/ami-id"
}

resource "aws_ebs_volume" "data" {
  availability_zone = local.az
  size              = var.data_volume_gb
  type              = "gp3"
  encrypted         = true
  tags              = { Name = "${var.name}-data", Backup = var.name }

  lifecycle {
    prevent_destroy = true
  }
}

resource "aws_instance" "this" {
  ami                         = nonsensitive(data.aws_ssm_parameter.ami.value)
  instance_type               = var.instance_type
  subnet_id                   = aws_subnet.public.id
  vpc_security_group_ids      = [aws_security_group.this.id]
  iam_instance_profile        = aws_iam_instance_profile.this.name
  associate_public_ip_address = true
  tags                        = { Name = var.name }

  user_data = templatefile("${path.module}/user-data.sh.tftpl", {
    region          = local.region
    arch            = local.arch
    k3s_channel     = var.k3s_channel
    acme_email      = var.acme_email
    data_volume     = replace(aws_ebs_volume.data.id, "-", "")
    kubeconfig_name = local.kubeconfig
  })

  root_block_device {
    volume_type = "gp3"
    volume_size = var.root_volume_gb
    encrypted   = true
  }

  metadata_options {
    http_tokens = "required"
    # 2, so the server pod can get the instance role's credentials. Sandboxes
    # cannot: the chart's NetworkPolicy blocks 169.254.0.0/16.
    http_put_response_hop_limit = 2
  }

  lifecycle {
    # A newer AMI or an edited boot script must not replace a running node.
    ignore_changes = [ami, user_data]
  }
}

resource "aws_volume_attachment" "data" {
  device_name = "/dev/sdf"
  volume_id   = aws_ebs_volume.data.id
  instance_id = aws_instance.this.id
}

resource "aws_eip" "this" {
  domain   = "vpc"
  instance = aws_instance.this.id
  tags     = { Name = var.name }

  depends_on = [aws_internet_gateway.this]
}

resource "aws_route53_record" "this" {
  count = var.route53_zone_id == null ? 0 : 1

  zone_id = var.route53_zone_id
  name    = var.domain
  type    = "A"
  ttl     = 300
  records = [aws_eip.this.public_ip]
}

# --- Daily snapshots of the data volume ---

resource "aws_iam_role" "dlm" {
  name_prefix = "${var.name}-dlm-"

  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect    = "Allow"
      Action    = "sts:AssumeRole"
      Principal = { Service = "dlm.amazonaws.com" }
    }]
  })
}

resource "aws_iam_role_policy_attachment" "dlm" {
  role       = aws_iam_role.dlm.name
  policy_arn = "arn:aws:iam::aws:policy/service-role/AWSDataLifecycleManagerServiceRole"
}

resource "aws_dlm_lifecycle_policy" "data" {
  description        = "${var.name}: daily snapshots of the data volume"
  execution_role_arn = aws_iam_role.dlm.arn
  state              = "ENABLED"

  policy_details {
    resource_types = ["VOLUME"]
    target_tags    = { Backup = var.name }

    schedule {
      name      = "daily"
      copy_tags = true

      create_rule {
        interval      = 24
        interval_unit = "HOURS"
        times         = ["03:00"]
      }

      retain_rule {
        count = var.snapshot_retention_days
      }
    }
  }
}
