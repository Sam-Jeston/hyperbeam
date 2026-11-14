---
title: Install on Kubernetes
description: Build the images, install the Helm chart, and point the harness profiles at your images.
---

The Helm chart in `deploy/helm/hyperbeam` installs the control plane, an optional Postgres, and a locked-down namespace for agent sandboxes.

To try the chart on your own machine first, without a registry, see [Run in a local cluster](/local-cluster). For a ready-made cluster on one EC2 instance, see [Install on AWS](/install-aws). For a machine at home or in the office, see [Install on a Raspberry Pi](/install-raspberry-pi).

## Before you start

- A Kubernetes cluster whose CNI **enforces NetworkPolicy**, such as Calico or Cilium. See [Network isolation](#network-isolation).
- [Helm](https://helm.sh) 3.

## Images

The images are published to GitHub Container Registry for `amd64` and `arm64`, and built from `main` on every push:

| Image | What it is |
|---|---|
| `ghcr.io/sam-jeston/hyperbeam/server` | The control plane. |
| `ghcr.io/sam-jeston/hyperbeam/runner` | The runner that is copied into each sandbox. |
| `ghcr.io/sam-jeston/hyperbeam/harness-claude-code` | One per coding agent: also `harness-codex`, `harness-gemini` and `harness-opencode`. |

Each has three tags: `latest`, one per commit such as `sha-1a2b3c4`, and the chart’s version. The chart and the built-in harness profiles use these images unless you say otherwise, so there is nothing to build. Harness images are also rebuilt weekly, to pick up new releases of the agents.

Your language toolchains go in an [environment](/environments), an image of your own that agents run in.

> [!NOTE]
> A version tag and `latest` move as `main` moves, and a node keeps the copy it already pulled. To stay on one build, set `image.tag` and `agents.runnerImage.tag` to a `sha-` tag. To move to a newer one, change those tags.

## Install the chart

```bash
helm install hyperbeam deploy/helm/hyperbeam --namespace hyperbeam --create-namespace \
  --set masterKey="$(openssl rand -base64 32)" \
  --set publicUrl=https://hyperbeam.example.com
```

The chart stores the master key in a Kubernetes Secret and keeps the existing value on upgrade. To manage the Secret yourself, set `existingSecret` instead of `masterKey`.

> [!IMPORTANT]
> Back up the master key. It encrypts every stored credential. To change it, see [Rotating the master key](/credentials#rotating-the-master-key): setting a different `masterKey` on its own makes stored credentials unreadable.

### Keep the master key in AWS

On AWS, the master key can stay out of the cluster. Set one of these instead of `masterKey`:

```bash
# The key is generated on first start and stored in Postgres, encrypted by a KMS key.
  --set aws.kmsKeyId=arn:aws:kms:eu-west-1:111122223333:key/1234abcd-12ab-34cd-56ef-1234567890ab \

# Or: the key is read from a Secrets Manager secret you created.
  --set aws.masterKeySecret=hyperbeam/master-key \
```

The server needs an IAM role that allows `kms:GenerateDataKey` and `kms:Decrypt` on the KMS key, or `secretsmanager:GetSecretValue` on the secret. On EKS, give it one with IAM roles for service accounts:

```bash
  --set 'serviceAccount.annotations.eks\.amazonaws\.com/role-arn=arn:aws:iam::111122223333:role/hyperbeam'
```

or with a Pod Identity association for the `hyperbeam` service account. Set `aws.region` if you name the key or secret by anything other than its ARN and the pod has no `AWS_REGION`. [Credentials](/credentials#the-master-key) explains both options.

Then open the UI and create the team. Continue with [Set up a team](/team-setup).

### Build your own images

To run a build of your own, or to keep the images in your own registry:

```bash
docker build -f deploy/docker/Dockerfile --target server -t REGISTRY/hyperbeam/server:0.1.0 .
docker build -f deploy/docker/Dockerfile --target runner -t REGISTRY/hyperbeam/runner:0.1.0 .
docker build -f deploy/docker/Dockerfile.harness --target claude-code -t REGISTRY/hyperbeam/harness-claude-code:0.1.0 .
```

Push them, and add these to the `helm install` command:

```bash
  --set image.repository=REGISTRY/hyperbeam/server \
  --set agents.runnerImage.repository=REGISTRY/hyperbeam/runner \
```

Then, under **Settings → Harnesses**, set each harness profile’s image to the one you pushed.

## What the chart creates

- The control plane, as one replica.
- A single-instance Postgres, unless you set `database.url` to use your own.
- A namespace for sandboxes, `hyperbeam-agents`, under the `restricted` Pod Security Standard. The control plane may create and delete pods and their token secrets there, and nothing else.
- A NetworkPolicy that lets sandboxes reach only the control plane and cluster DNS. With `egress.enforce: true` (the default) everything else goes through the egress allowlist proxy.

Each session gets a pod that runs as a non-root user with no service account token, no added capabilities, and resource limits (2 CPUs, 4 GiB of memory and a 10 GiB workspace by default). Idle sandboxes are torn down after 15 minutes.

## Network isolation

The NetworkPolicy is what keeps sandboxes from reaching anything but Hyperbeam.

> [!WARNING]
> On a cluster whose CNI does not enforce NetworkPolicy, the policy is silently ignored and sandboxes have open network access.

Some CNIs, including the one `kind` ships, do not apply policy to traffic from a pod to its own node, so a sandbox may still be able to open a connection to the Kubernetes API server. It has no service account token, so it is an anonymous client there. If your cluster grants anonymous users anything, tighten that or add a CNI-specific host policy.

## Next steps

- [Configuration](/configuration) lists every server setting.
- [Security model](/security) explains what the sandbox can and cannot reach.
