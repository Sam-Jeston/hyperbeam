---
title: Install on a Raspberry Pi
description: Run Hyperbeam on a Raspberry Pi at home or in the office, with sandboxed agents, using k3s and one script.
---

A Raspberry Pi 5 with 8 GB of memory runs Hyperbeam comfortably for one person or a small team: the control plane, Postgres, and two or three agents at a time. It is the same Helm chart as on [any other cluster](/install-kubernetes), on [k3s](https://k3s.io), a small Kubernetes distribution. Agents run in sandboxes, so they can’t reach the Pi’s files or the other machines on your network.

`scripts/pi.sh` does the repetitive part. You run it on your own machine, and it copies the source to the Pi, builds the images there, and installs or updates Hyperbeam.

The same steps work on any small Debian or Ubuntu machine, arm64 or x86. Only [step 1](#1-turn-on-the-memory-cgroup) is specific to the Pi.

## Before you start

On the Pi:

- A Raspberry Pi 5 or 4 with 8 GB of memory, running 64-bit Raspberry Pi OS (Bookworm or later). Check with `uname -m`: it must print `aarch64`.
- An SSD rather than an SD card, if you can. Building the images and running Postgres are both hard on SD cards.
- 15 GB of free disk space.
- SSH access with a key, as a user who can run `sudo` without a password. This is the default on Raspberry Pi OS.

On your own machine:

- This repository, and `ssh` and `rsync`.

And a way for people to reach the Pi. This page uses a [Cloudflare Tunnel](#5-make-it-reachable), which needs no open ports on your router.

## 1. Turn on the memory cgroup

Kubernetes limits each sandbox’s memory with a kernel feature that Raspberry Pi OS turns off. On the Pi, check whether it is on:

```bash
cat /sys/fs/cgroup/cgroup.controllers
```

If the output includes `memory`, skip to step 2. Otherwise add two settings to the end of the kernel command line, which is the single line in `/boot/firmware/cmdline.txt`, and reboot:

```bash
sudo cp /boot/firmware/cmdline.txt /boot/firmware/cmdline.txt.bak
sudo sed -i '1 s/$/ cgroup_enable=memory cgroup_memory=1/' /boot/firmware/cmdline.txt
sudo reboot
```

After the reboot, the first command lists `memory`. Without it, k3s does not start.

## 2. Install k3s

On the Pi:

```bash
curl -sfL https://get.k3s.io | sh -s - server \
  --node-name hyperbeam \
  --write-kubeconfig-mode 600 \
  --disable traefik --disable servicelb
```

- `--disable traefik --disable servicelb` leaves ports 80 and 443 alone. Leave these two flags out if you want k3s to serve Hyperbeam on those ports itself. See [Other ways to reach it](#other-ways-to-reach-it).
- `--node-name hyperbeam` keeps Postgres’s volume bound to the node if you rename the Pi.

k3s starts on boot and enforces NetworkPolicy, which is what isolates the sandboxes. Check that the node is ready:

```bash
sudo k3s kubectl get nodes
```

> [!IMPORTANT]
> If the Pi runs another service on a port between 30000 and 32767, sandboxes can reach it. Plex, on port 32400, is a common one. See [What a sandbox can reach](#what-a-sandbox-can-reach) for the fix, which is easiest to apply now.

## 3. Install podman and Helm

The images are built on the Pi with [podman](https://podman.io), which needs no daemon. On the Pi:

```bash
sudo apt-get update && sudo apt-get install -y podman

mkdir -p ~/.config/containers
echo 'unqualified-search-registries = ["docker.io"]' > ~/.config/containers/registries.conf

curl -fsSL https://raw.githubusercontent.com/helm/helm/main/scripts/get-helm-3 | bash
```

The `registries.conf` line lets podman find the base images the Dockerfiles name, such as `rust` and `node`, on Docker Hub.

## 4. Deploy

On your own machine, from the root of the repository:

```bash
PI=pi@raspberrypi.local PUBLIC_URL=https://hyperbeam.example.com scripts/pi.sh up
```

`PI` is how you reach the Pi over SSH. `PUBLIC_URL` is the address people will use for the UI. It is used in invite links and OAuth sign-ins, so set the real one.

The script:

1. Copies the source to `~/hyperbeam-src` on the Pi.
2. Builds the server, the runner and the Claude Code harness image. The first build compiles the server and takes about 15 minutes on a Pi 5. Later builds reuse what has not changed.
3. Loads the images into k3s. There is no registry.
4. Creates the master key at `~/hyperbeam/master-key` on the Pi, and installs the chart.
5. Points the Claude Code harness profile at the image it built.
6. Prints the address of the Hyperbeam Service inside the cluster, for the next step.

> [!IMPORTANT]
> Back up `~/hyperbeam/master-key` from the Pi, for example with `scp pi@raspberrypi.local:hyperbeam/master-key .`, and keep it somewhere safe. It encrypts every stored credential. If you lose the Pi’s disk, a database backup is unreadable without it. See [Credentials](/credentials#the-master-key).

To build other agents too, name them: `HARNESSES="claude-code codex" scripts/pi.sh up`.

The script sets two limits that suit an 8 GB machine: 3 sandboxes at once (`MAX_SESSIONS`), each with at most 3 GiB of memory (`MEMORY_LIMIT`). Other sessions wait in a queue. Set either variable on the first `up` to choose differently. To change them later, on the Pi:

```bash
sudo KUBECONFIG=/etc/rancher/k3s/k3s.yaml helm upgrade hyperbeam ~/hyperbeam-src/deploy/helm/hyperbeam \
  --namespace hyperbeam --reuse-values --set agents.maxSessions=2
```

## 5. Make it reachable

Hyperbeam now listens at the Service address the script printed, such as `http://10.43.178.72:8080`. That address works only from the Pi itself. A [Cloudflare Tunnel](https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/) on the Pi gives it a public HTTPS address without opening a port on your router.

If you do not have a tunnel yet, [create one](https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/do-more-with-tunnels/local-management/create-local-tunnel/) and install `cloudflared` as a service. Then add Hyperbeam to the `ingress` list in `/etc/cloudflared/config.yml`, above the final catch-all rule:

```yaml
ingress:
  - hostname: hyperbeam.example.com
    service: http://10.43.178.72:8080
  - service: http_status:404
```

Create the DNS record and restart the tunnel:

```bash
cloudflared tunnel route dns TUNNEL_NAME hyperbeam.example.com
sudo systemctl restart cloudflared
```

The Service address stays the same across updates, restarts and reboots. It changes only if you uninstall the chart and install it again. Find it at any time with `sudo k3s kubectl -n hyperbeam get svc hyperbeam`.

> [!WARNING]
> A tunnel puts the sign-in page on the public internet. The first person to open it creates the team and becomes its owner, so do the next step straight away. To restrict who can reach the page at all, put [Cloudflare Access](https://developers.cloudflare.com/cloudflare-one/policies/access/) in front of the hostname.

### Other ways to reach it

- **On your own network only.** Install k3s without `--disable traefik --disable servicelb`. Then turn on the chart’s ingress, on the Pi:

  ```bash
  sudo KUBECONFIG=/etc/rancher/k3s/k3s.yaml helm upgrade hyperbeam ~/hyperbeam-src/deploy/helm/hyperbeam \
    --namespace hyperbeam --reuse-values \
    --set ingress.enabled=true --set ingress.host=hyperbeam.lan
  ```

  Point `hyperbeam.lan` at the Pi’s address in your router’s DNS or your `hosts` file, and use `PUBLIC_URL=http://hyperbeam.lan`.
- **Tailscale or another VPN.** Use the first option, with the Pi’s VPN name as the host.

## 6. Open the UI

Open your public URL. The first person to sign up creates the team and becomes its owner.

Then open **Settings → Harnesses**, select **Claude Code**, and add a credential for it, as in the [quick start](/quickstart#4-connect-an-agent). The profile already names the image the script built. Press <kbd>C</kbd> to start work. The first session takes a little longer while its pod starts.

Continue with [Set up a team](/team-setup).

## Update to a new build

After you pull or change the code, on your own machine:

```bash
PI=pi@raspberrypi.local scripts/pi.sh update
```

This copies the source, rebuilds the server and the runner, loads them and restarts the control plane. Database migrations run when it starts. Running sessions reconnect.

| Command | What it does |
|---|---|
| `scripts/pi.sh update` | Rebuild the server and the runner, and restart the control plane. |
| `scripts/pi.sh update harness [codex …]` | Rebuild harness images, Claude Code by default. New sandboxes use them. |
| `scripts/pi.sh up` | Apply changes to the Helm chart, and build any image that is missing. |
| `scripts/pi.sh status` | Show the node, the pods, the loaded images and free memory. |
| `scripts/pi.sh logs` | Follow the control plane’s logs. |
| `scripts/pi.sh psql` | Open `psql` on the bundled Postgres. |

Set `PI` for each, or export it once.

Images always carry the tag `pi`. A rebuilt image replaces the one before it, so updates need no change to the chart’s settings. After you build a harness for the first time with `update harness`, set its image under **Settings → Harnesses** to `docker.io/hyperbeam/harness-NAME:pi`.

## What a sandbox can reach

The chart’s NetworkPolicy lets a sandbox reach the control plane and cluster DNS, and nothing else. Everything it fetches goes through the [egress allowlist](/security#network-egress). On k3s this covers the Pi itself and your network: a sandbox cannot connect to SSH on the Pi, to a file share, or to your router.

There is one exception. k3s leaves the Kubernetes NodePort range, ports 30000 to 32767 on the Pi, open to pods. If another service on the Pi listens in that range, narrow the range so it leaves that port out. For Plex on port 32400, on the Pi:

```bash
sudo mkdir -p /etc/rancher/k3s
echo 'service-node-port-range: "30000-32399"' | sudo tee /etc/rancher/k3s/config.yaml
sudo systemctl restart k3s
sudo reboot   # only if k3s was already running: clears the rule for the old range
```

To see what listens in the range: `sudo ss -ltn | awk '{print $4}' | grep -E ':3[0-2][0-9]{3}$'`.

## What survives

| Event | What happens |
|---|---|
| The control plane restarts or is updated | No data is lost. Running sandboxes reconnect. |
| The Pi reboots or loses power | k3s, Postgres and Hyperbeam start again on their own. Sandboxes that were running are gone. Send a message to resume a session in a fresh sandbox, from the branch it pushed. |
| An idle sandbox is torn down after 15 minutes | Work that was pushed is kept. Send a message to resume. |
| The Pi’s disk fails | **Everything is lost** unless you have a backup. |

Postgres keeps its data under `/var/lib/rancher/k3s/storage` on the Pi. To back it up, dump the database and copy the file off the Pi, along with the master key:

```bash
sudo k3s kubectl -n hyperbeam exec statefulset/hyperbeam-postgres -- \
  pg_dump -U hyperbeam hyperbeam > hyperbeam-backup.sql
```

## Limits

- **Builds are slow and use the whole machine.** The first build takes about 15 minutes, and agents that compile large projects will feel slow. The build runs at low priority so other services on the Pi stay responsive.
- **Memory decides how many agents run.** Each sandbox asks for 512 MiB and can use up to the limit you set. If the Pi does other work, lower `agents.maxSessions`.
- **One machine.** If the Pi is off, Hyperbeam is down.

## Remove it

On the Pi:

```bash
sudo KUBECONFIG=/etc/rancher/k3s/k3s.yaml helm uninstall hyperbeam --namespace hyperbeam
/usr/local/bin/k3s-uninstall.sh
rm -rf ~/hyperbeam-src
podman rmi --all
```

`k3s-uninstall.sh` deletes the database with the cluster. Keep `~/hyperbeam/master-key` if you kept a database backup. Remove the hostname from the tunnel’s configuration, and restore `/boot/firmware/cmdline.txt.bak` if you want the kernel settings back as they were.

## Next steps

- [Set up a team](/team-setup): invite people and connect GitHub.
- [Configuration](/configuration): every server setting, and the chart values that set them.
- [Security model](/security): what the sandbox can and cannot reach.
