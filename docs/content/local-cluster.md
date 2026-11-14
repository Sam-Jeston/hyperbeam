---
title: Run in a local cluster
description: Run Hyperbeam on your own machine with the Helm chart, in a kind cluster, with every session in its own sandbox pod.
---

The [quick start](/quickstart) runs agents as local processes, which is the fastest way to try Hyperbeam and the easiest way to work on it. For anything you want to keep running, use the Helm chart in a local Kubernetes cluster instead. This is the same setup as a [production install](/install-kubernetes), on your own machine:

- **Sandboxed agents.** Each session runs in its own pod as a non-root user, and can reach only the control plane and the hosts on the egress allowlist. Agents can’t touch your files or your network.
- **Restarts are safe.** Kubernetes restarts the control plane if it crashes. Postgres stores its data on a persistent volume. Running sandboxes reconnect after a control plane restart.
- **The same as production.** What works here works on a shared cluster.

This page uses [kind](https://kind.sigs.k8s.io). See [Other local clusters](#other-local-clusters) for k3d and minikube.

## Before you start

You need:

- [Docker](https://docs.docker.com/get-docker/)
- [kind](https://kind.sigs.k8s.io/docs/user/quick-start/#installation) 0.24 or later. Its default network plugin enforces NetworkPolicy, which keeps sandboxes isolated.
- [kubectl](https://kubernetes.io/docs/tasks/tools/)
- [Helm](https://helm.sh) 3

Give Docker at least 4 CPUs and 8 GiB of memory. Each sandbox asks for 0.25 CPU and 512 MiB, and can use up to 2 CPUs and 4 GiB.

## 1. Create the cluster

```bash
kind create cluster --name hyperbeam
```

## 2. Build the images and load them into the cluster

Build the images from your checkout, so that the cluster runs the code in front of you. Then load them into the kind node, so the cluster doesn’t need a registry.

```bash
docker build -f deploy/docker/Dockerfile --target server -t hyperbeam/server:dev .
docker build -f deploy/docker/Dockerfile --target runner -t hyperbeam/runner:dev .
docker build -f deploy/docker/Dockerfile.harness --target claude-code -t hyperbeam/harness-claude-code:dev .

kind load docker-image --name hyperbeam \
  hyperbeam/server:dev hyperbeam/runner:dev hyperbeam/harness-claude-code:dev
```

The first build compiles the server and takes several minutes. `Dockerfile.harness` also has `codex`, `gemini` and `opencode` targets. Build and load one for each agent you plan to use.

> [!TIP]
> Don’t tag local images `latest`. The chart pulls images only when they are missing, so loaded images are used as they are, but a `latest` tag makes it unclear which build is running.

## 3. Create the master key

```bash
openssl rand -base64 32 > hyperbeam-master-key
```

> [!IMPORTANT]
> Back this file up. It is ignored by git, so it won’t be committed. The master key encrypts every stored credential. The chart keeps it on upgrades, but if you delete the cluster and install again, you need the same key to read credentials stored in a database you kept. See [Credentials](/credentials#the-master-key).

## 4. Install the chart

```bash
helm install hyperbeam deploy/helm/hyperbeam --namespace hyperbeam --create-namespace \
  --set masterKey="$(cat hyperbeam-master-key)" \
  --set image.repository=hyperbeam/server --set image.tag=dev \
  --set agents.runnerImage.repository=hyperbeam/runner --set agents.runnerImage.tag=dev
```

On a laptop, you can also add `--set agents.maxSessions=4` to limit how many sandboxes run at once. Other sessions wait in a queue.

Wait for the control plane and Postgres to start:

```bash
kubectl -n hyperbeam rollout status deploy/hyperbeam
```

## 5. Open the UI

```bash
kubectl -n hyperbeam port-forward svc/hyperbeam 8080:8080
```

Open <http://localhost:8080>. The first person to sign up creates the team and becomes its owner.

## 6. Point the harness at your image

The harness profiles name published images by default. Change them to the images you loaded:

1. Open **Settings → Harnesses** and select **Claude Code**.
2. Select **Edit** and set **Container image** to `hyperbeam/harness-claude-code:dev`.
3. Add a credential for it, as in the [quick start](/quickstart#4-connect-an-agent).

You don’t install an ACP adapter on your machine. The adapter is in the harness image.

Press <kbd>C</kbd> to start work. The first session takes a little longer while the pod starts. To watch the sandboxes:

```bash
kubectl -n hyperbeam-agents get pods --watch
```

Continue with [Set up a team](/team-setup).

## Update to a new build

Rebuild the images with a new tag, load them, and upgrade the release:

```bash
TAG=dev-$(git rev-parse --short HEAD)
docker build -f deploy/docker/Dockerfile --target server -t hyperbeam/server:$TAG .
docker build -f deploy/docker/Dockerfile --target runner -t hyperbeam/runner:$TAG .
kind load docker-image --name hyperbeam hyperbeam/server:$TAG hyperbeam/runner:$TAG

helm upgrade hyperbeam deploy/helm/hyperbeam --namespace hyperbeam --reuse-values \
  --set image.tag=$TAG --set agents.runnerImage.tag=$TAG
```

Running sessions reconnect when the new control plane starts. If you rebuild a harness image, load it the same way and change the image in **Settings → Harnesses**. New sandboxes use the new image.

## What survives

| Event | What happens |
|---|---|
| The control plane restarts or is upgraded | No data is lost. Running sandboxes reconnect. |
| Docker or your machine restarts | The kind cluster starts again with Docker, and Postgres keeps its data. Sandboxes that were running are gone. Send a message to resume a session in a fresh sandbox, from the branch it pushed. |
| An idle sandbox is torn down after 15 minutes | Work that was pushed is kept. Send a message to resume. |
| You run `kind delete cluster` | **Everything is deleted**, including the database. |

To keep your data when you delete the cluster, run Postgres outside the cluster and set `database.url` (see below), or back up the database first:

```bash
kubectl -n hyperbeam exec statefulset/hyperbeam-postgres -- \
  pg_dump -U hyperbeam hyperbeam > hyperbeam-backup.sql
```

### Use your own Postgres

To run Postgres outside the cluster, set `database.url` and turn off the bundled one. From a kind cluster, the Docker host is usually reachable at `host.docker.internal` on Docker Desktop, or at the `kind` network’s gateway (often `172.18.0.1`) on Linux. Make sure Postgres listens on that address, not only on `127.0.0.1`.

```bash
helm upgrade hyperbeam deploy/helm/hyperbeam --namespace hyperbeam --reuse-values \
  --set postgres.enabled=false \
  --set database.url=postgres://hyperbeam:PASSWORD@host.docker.internal:5432/hyperbeam
```

## Other local clusters

The chart works on any cluster. What changes is how you load the images and whether NetworkPolicy is enforced.

| Cluster | Load images | NetworkPolicy |
|---|---|---|
| [kind](https://kind.sigs.k8s.io) 0.24+ | `kind load docker-image --name NAME IMAGE…` | Enforced by the default network plugin. |
| [k3d](https://k3d.io) | `k3d image import -c NAME IMAGE…` | Enforced by k3s by default. |
| [minikube](https://minikube.sigs.k8s.io) | `minikube image load IMAGE` | Start it with `minikube start --cni=calico`. |
| Docker Desktop | Depends on the cluster type: kubeadm clusters use Docker’s images directly. | Not enforced. |

> [!WARNING]
> Where NetworkPolicy is not enforced, sandboxes have open network access. Agents still can’t reach your files, but they can reach any host.

On kind, the network plugin does not apply policy to traffic from a pod to its own node, so a sandbox may be able to connect to the Kubernetes API server. It has no service account token, so it is an anonymous client there. See [Network isolation](/install-kubernetes#network-isolation).

## Remove it

```bash
helm uninstall hyperbeam --namespace hyperbeam
kind delete cluster --name hyperbeam
```

## Next steps

- [Install on Kubernetes](/install-kubernetes): the same chart on a shared cluster, with a registry and an ingress.
- [Install on AWS](/install-aws): a Terraform module that runs it on one EC2 instance.
- [Install on a Raspberry Pi](/install-raspberry-pi): k3s on a Pi, deployed with one script.
- [Configuration](/configuration): every server setting, and the chart values that set them.
