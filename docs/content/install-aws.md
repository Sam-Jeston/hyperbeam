---
title: Install on AWS
description: Run Hyperbeam on one EC2 instance with the Terraform module, for about $115 a month.
---

The Terraform module in `deploy/terraform/aws` runs Hyperbeam on a single EC2 instance. The instance runs [k3s](https://k3s.io), a small Kubernetes distribution, and you install the same Helm chart as on [any other cluster](/install-kubernetes).

This is a starting point, not a managed service. It keeps the bill low by using one instance and no managed Kubernetes, database, NAT gateway or load balancer. Read it, change it, and see [Limits](#limits) for when to move on.

## What it creates

| Need | How it is met |
|---|---|
| Kubernetes with NetworkPolicy | k3s on one instance. k3s enforces NetworkPolicy, so sandboxes are isolated. |
| Postgres | The chart’s bundled Postgres, on its own encrypted EBS volume with daily snapshots. |
| HTTPS | k3s’s bundled Traefik, with a Let’s Encrypt certificate, on an Elastic IP. |
| Master key | A KMS key. The server generates the master key and stores it wrapped under the KMS key, so there is no key to back up. See [Credentials](/credentials#the-master-key). |
| Images | ECR repositories, pulled with the instance’s IAM role. |
| Admin access | SSM Session Manager. Only ports 80 and 443 are open: no SSH, and the Kubernetes API is not exposed. |
| Network | A small VPC with one public subnet. |

## Cost

On-demand prices in `us-east-1`, in US dollars for a 730-hour month. Check [AWS pricing](https://aws.amazon.com/pricing/) for your region.

| Item | Default | Small |
|---|---|---|
| Instance | `t4g.xlarge`, 4 vCPUs and 16 GiB: $98 | `t4g.large`, 2 vCPUs and 8 GiB: $49 |
| Root disk, gp3 | 100 GB: $8 | 60 GB: $5 |
| Data volume, 20 GB gp3, and its snapshots | $3 | $3 |
| Public IPv4 address | $4 | $4 |
| KMS key and ECR storage | $2 | $2 |
| **Total** | **about $115** | **about $63** |

A one-year Compute Savings Plan takes about 30% off the instance. Model usage is billed by your model provider and is not included.

Each sandbox asks for 0.25 CPU and 512 MiB, and can use up to 2 CPUs and 4 GiB. As a rule of thumb the default runs about 6 sessions at once and the small size about 3. Set `max_sessions` to match; other sessions wait in a queue.

## Before you start

- An AWS account, and the [AWS CLI](https://docs.aws.amazon.com/cli/latest/userguide/getting-started-install.html) signed in to it, with the [Session Manager plugin](https://docs.aws.amazon.com/systems-manager/latest/userguide/session-manager-working-with-install-plugin.html).
- [Terraform](https://developer.hashicorp.com/terraform/install) 1.5 or later, or [OpenTofu](https://opentofu.org).
- [Docker](https://docs.docker.com/get-docker/), [kubectl](https://kubernetes.io/docs/tasks/tools/) and [Helm](https://helm.sh) 3.
- A domain name for Hyperbeam, such as `hyperbeam.example.com`.

Run every command on this page from the root of the repository.

## 1. Create the infrastructure

Write your settings to `deploy/terraform/aws/terraform.tfvars`:

```hcl
domain     = "hyperbeam.example.com"
acme_email = "you@example.com"

# For the small size:
# instance_type  = "t4g.large"
# root_volume_gb = 60
# max_sessions   = 3
```

`variables.tf` lists the other settings. The AWS region comes from your AWS CLI configuration or `AWS_REGION`.

```bash
terraform -chdir=deploy/terraform/aws init
terraform -chdir=deploy/terraform/aws apply
```

The module keeps its state in a local file, `deploy/terraform/aws/terraform.tfstate`. Keep that file, or [configure a remote backend](https://developer.hashicorp.com/terraform/language/backend) before you apply.

Save the outputs the next steps use:

```bash
tf() { terraform -chdir=deploy/terraform/aws output -raw "$1"; }
export AWS_REGION=$(tf region)
```

## 2. Point the domain at the instance

Create an `A` record for your domain with the address from `tf public_ip`. If the domain is in Route 53, set `route53_zone_id` in `terraform.tfvars` instead and the module creates the record.

Traefik requests the certificate when the chart is installed, so the record must resolve by step 5.

## 3. Build and push the images

The project does not publish images yet, so build them from the repository. `t4g` instances are arm64, so images must be built for `linux/arm64`. `tf image_platform` prints the platform for your instance type.

```bash
aws ecr get-login-password | docker login --username AWS --password-stdin "$(tf registry)"

PREFIX=$(tf image_prefix)
PLATFORM=$(tf image_platform)
docker build --platform $PLATFORM -f deploy/docker/Dockerfile --target server -t $PREFIX/server:0.1.0 .
docker build --platform $PLATFORM -f deploy/docker/Dockerfile --target runner -t $PREFIX/runner:0.1.0 .
docker build --platform $PLATFORM -f deploy/docker/Dockerfile.harness --target claude-code -t $PREFIX/harness-claude-code:0.1.0 .

docker push $PREFIX/server:0.1.0
docker push $PREFIX/runner:0.1.0
docker push $PREFIX/harness-claude-code:0.1.0
```

> [!TIP]
> On an x86 machine, Docker builds arm64 images under emulation, and compiling the server that way is slow. Either build on an arm64 machine such as an Apple silicon Mac, or set `instance_type = "t3a.xlarge"` before step 1 to run on x86 for about $12 a month more.

To use other agents, add them to `harnesses` in `terraform.tfvars`, such as `harnesses = ["claude-code", "codex"]`, apply again, and build those targets too.

## 4. Connect to the cluster

The instance installs k3s on first boot, which takes a few minutes, then stores its kubeconfig in an SSM parameter. Fetch it:

```bash
aws ssm get-parameter --name "$(tf kubeconfig_parameter)" --with-decryption \
  --query Parameter.Value --output text > hyperbeam-kubeconfig
chmod 600 hyperbeam-kubeconfig
```

If the file contains only `pending`, k3s is not up yet. Wait a minute and fetch it again.

The Kubernetes API is not open to the internet. In a second terminal, forward it to your machine, and leave this running while you use `kubectl` or `helm`:

```bash
aws ssm start-session --target "$(tf instance_id)" \
  --document-name AWS-StartPortForwardingSession \
  --parameters '{"portNumber":["6443"],"localPortNumber":["6443"]}'
```

Back in the first terminal:

```bash
export KUBECONFIG=$PWD/hyperbeam-kubeconfig
kubectl get nodes
```

> [!IMPORTANT]
> `hyperbeam-kubeconfig` gives full control of the cluster. It is ignored by git. Anyone who may read the SSM parameter and start a session on the instance has the same access.

## 5. Install the chart

The module writes the chart values for you: the image repositories, the KMS key, the public URL and the ingress.

```bash
tf helm_values > hyperbeam-values.yaml

helm install hyperbeam deploy/helm/hyperbeam --namespace hyperbeam --create-namespace \
  -f hyperbeam-values.yaml

kubectl -n hyperbeam rollout status deploy/hyperbeam
```

Do not set `masterKey`. The values name the KMS key instead.

## 6. Open the UI

Open `https://hyperbeam.example.com`. The first request can take up to a minute while Traefik gets the certificate. The first person to sign up creates the team and becomes its owner.

Then open **Settings → Harnesses** and set each harness profile’s image to the one you pushed, such as `PREFIX/harness-claude-code:0.1.0`. Continue with [Set up a team](/team-setup).

## Update to a new build

Build and push the images with a new tag, as in step 3. Then, with the port forward running:

```bash
helm upgrade hyperbeam deploy/helm/hyperbeam --namespace hyperbeam --reuse-values \
  --set image.tag=NEW_TAG --set agents.runnerImage.tag=NEW_TAG
```

Running sessions reconnect when the new control plane starts.

To change the instance size, change `instance_type` and apply. Terraform stops the instance, resizes it and starts it again. Keep the same architecture, or rebuild the images for the new one.

## Backups and recovery

Postgres data and the TLS certificate are on the data volume. AWS takes a snapshot of that volume every day and keeps the last 7 (`snapshot_retention_days`).

| Event | What happens |
|---|---|
| The instance reboots, or AWS moves it to new hardware | Everything comes back. Sandboxes that were running are gone. Send a message to resume a session in a fresh sandbox, from the branch it pushed. |
| The instance is terminated | The data volume and its snapshots are kept, but a new instance starts an empty cluster. You install the chart again and restore the database yourself. |
| The KMS key is deleted | **Stored credentials become unreadable.** |

Snapshots are taken while Postgres is running, and can be up to a day old. For a backup you can restore anywhere, also dump the database:

```bash
kubectl -n hyperbeam exec statefulset/hyperbeam-postgres -- \
  pg_dump -U hyperbeam hyperbeam > hyperbeam-backup.sql
```

A restored database is readable only with the same KMS key.

## Limits

- **One instance, one availability zone.** If the instance fails, Hyperbeam is down until it comes back. For a database with standby and point-in-time recovery, create an RDS instance and set `database.url` and `postgres.enabled=false` in the chart.
- **It grows by instance size.** For more sessions than one instance holds, install the chart on a multi-node cluster such as EKS. See [Install on Kubernetes](/install-kubernetes).
- **The instance role is shared by the node.** The server pod gets its KMS access from the instance’s IAM role. The chart’s NetworkPolicy keeps sandboxes away from the instance metadata service, so do not remove that policy.

## Remove it

```bash
helm uninstall hyperbeam --namespace hyperbeam
terraform -chdir=deploy/terraform/aws destroy
```

Terraform refuses to delete the data volume. To delete it too, remove the `prevent_destroy` block from `aws_ebs_volume.data` in `main.tf` and run `destroy` again. Snapshots already taken are kept until you delete them, and the KMS key is deleted after 30 days.

## Next steps

- [Set up a team](/team-setup): invite people and connect GitHub.
- [Configuration](/configuration): every server setting, and the chart values that set them.
- [Security model](/security): what the sandbox can and cannot reach.
