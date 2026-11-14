#!/usr/bin/env bash
# Deploy Hyperbeam to a Raspberry Pi (or any small arm64/x86 machine) that
# runs k3s, from this checkout. Run it on your own machine: it copies the
# source to the Pi over SSH, builds the images there with podman, loads them
# into k3s and installs or restarts the release. See docs: /install-raspberry-pi.
#
#   PI=user@host scripts/pi.sh up        First install, or bring an install up to date with the chart
#   PI=user@host scripts/pi.sh update    Rebuild server + runner from this checkout, load them, restart the control plane
#   PI=user@host scripts/pi.sh update harness [codex ...]   Rebuild harness images (claude-code by default)
#   PI=user@host scripts/pi.sh status    Node, pods, loaded images and memory
#   PI=user@host scripts/pi.sh logs      Follow control plane logs (extra args go to kubectl logs)
#   PI=user@host scripts/pi.sh psql      Open psql on the bundled Postgres
#
# Images always use the `pi` tag. Loading a rebuilt `pi` image replaces the
# old one in k3s, and the chart pulls only when an image is missing, so new
# pods pick up the new build without changing any settings.
#
# The Pi needs k3s, podman and helm, and passwordless sudo for the SSH user.
# The master key is created on the Pi at ~/hyperbeam/master-key on first
# install. Back it up.
#
# Environment:
#   PI=user@host                SSH destination (required)
#   PUBLIC_URL=https://...      Where people will reach the UI (required for the first install)
#   HARNESSES="claude-code"     harness images to build on first `up`
#   MAX_SESSIONS=3              concurrent sandboxes, set on first install (chart default is 20)
#   MEMORY_LIMIT=3Gi            memory limit per sandbox, set on first install (chart default is 4Gi)
set -euo pipefail

PI=${PI:-}
PUBLIC_URL=${PUBLIC_URL:-}
HARNESSES=${HARNESSES:-claude-code}
MAX_SESSIONS=${MAX_SESSIONS:-3}
MEMORY_LIMIT=${MEMORY_LIMIT:-3Gi}

NS=hyperbeam
TAG=pi
ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
# On the Pi, relative to the SSH user's home.
SRC=hyperbeam-src
KEY_FILE=hyperbeam/master-key

log() { printf '\033[1;34m==>\033[0m %s\n' "$*"; }
die() { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }

# Run a script on the Pi. It is read from stdin, so it needs no quoting here;
# arguments are passed to it as $1, $2, ...
pi() {
  local args=""
  local arg
  for arg in "$@"; do args+=" $(printf '%q' "$arg")"; done
  ssh -o BatchMode=yes -o ServerAliveInterval=30 "$PI" "bash -euo pipefail -s --$args"
}

# The same, for commands that need a terminal (logs, psql).
pi_tty() { ssh -t -o ServerAliveInterval=30 "$PI" "$@"; }

K="sudo k3s kubectl"
HELM="sudo KUBECONFIG=/etc/rancher/k3s/k3s.yaml helm"

require() {
  [[ -n $PI ]] || die "set PI to the SSH destination of the Pi, e.g. PI=pi@raspberrypi.local"
  command -v rsync >/dev/null || die "rsync is not installed"
  ssh -o BatchMode=yes -o ConnectTimeout=10 "$PI" true 2>/dev/null || die "cannot reach $PI over SSH with a key"
  pi <<'EOF' || die "the Pi is not ready (see the docs: Install on a Raspberry Pi)"
missing=""
for bin in k3s podman helm rsync; do command -v "$bin" >/dev/null || missing+=" $bin"; done
[ -z "$missing" ] || { echo "not installed on the Pi:$missing" >&2; exit 1; }
sudo -n true 2>/dev/null || { echo "the SSH user needs passwordless sudo" >&2; exit 1; }
sudo k3s kubectl get --raw /readyz >/dev/null 2>&1 || { echo "k3s is not running (sudo systemctl status k3s)" >&2; exit 1; }
EOF
}

# --- source ----------------------------------------------------------------

sync_source() {
  log "Copying the source to $PI:~/$SRC"
  rsync -az --delete \
    --exclude /target --exclude node_modules --exclude dist --exclude .git \
    --exclude .hyperbeam-dev --exclude .env --exclude '*.pem' --exclude hyperbeam-master-key \
    --exclude .terraform --exclude '*.tfstate*' --exclude '*.tfvars' \
    --exclude hyperbeam-kubeconfig --exclude hyperbeam-values.yaml \
    "$ROOT/" "$PI:$SRC/"
}

# --- images ----------------------------------------------------------------

image() { echo "docker.io/hyperbeam/$1:$TAG"; }

on_node() { pi "$(image "$1")" <<<'sudo k3s crictl inspecti "$1" >/dev/null 2>&1'; }

build() { # build <dockerfile> <target> <name>
  log "Building $(image "$3") on the Pi"
  pi "$SRC" "$1" "$2" "$(image "$3")" <<'EOF'
cd "$1"
log=~/hyperbeam-build.log
# Compiling competes with whatever else the machine does; let it lose.
# The full output goes to the log; only the steps are shown.
set +e
nice -n 10 podman build -f "deploy/docker/$2" --target "$3" -t "$4" . 2>&1 | tee "$log" | grep --line-buffered -E '^(\[[0-9]+/[0-9]+\] )?STEP|Compiling hyperbeam'
status=${PIPESTATUS[0]}
set -e
if [ "$status" -ne 0 ]; then
  tail -40 "$log" >&2
  echo "the build failed; the full output is in $log on the Pi" >&2
  exit 1
fi
EOF
}

load() {
  log "Loading into k3s: $*"
  pi "$@" <<'EOF'
for img in "$@"; do
  podman save "$img" | sudo k3s ctr images import - >/dev/null
done
# Reloading a tag leaves the previous build untagged in k3s; drop those.
sudo k3s crictl images 2>/dev/null | awk '$2=="<none>" {print $3}' | xargs -r sudo k3s crictl rmi >/dev/null 2>&1 || true
# In podman, untagged images are also the build cache, so only old ones go.
podman image prune -f --filter until=168h >/dev/null 2>&1 || true
EOF
}

build_core() {
  build Dockerfile server server
  build Dockerfile runner runner
  load "$(image server)" "$(image runner)"
}

build_harnesses() {
  local h images=()
  for h in "$@"; do
    build Dockerfile.harness "$h" "harness-$h"
    images+=("$(image "harness-$h")")
  done
  load "${images[@]}"
}

# Build only what k3s doesn't have yet.
ensure_images() {
  if ! on_node server || ! on_node runner; then build_core; fi
  local h missing=()
  for h in $HARNESSES; do
    on_node "harness-$h" || missing+=("$h")
  done
  (( ${#missing[@]} )) && build_harnesses "${missing[@]}"
  return 0
}

# --- release ---------------------------------------------------------------

ensure_release() {
  local images=(
    --set image.repository=docker.io/hyperbeam/server --set "image.tag=$TAG"
    --set agents.runnerImage.repository=docker.io/hyperbeam/runner --set "agents.runnerImage.tag=$TAG"
  )
  if pi <<<"$HELM status hyperbeam -n $NS >/dev/null 2>&1"; then
    # The chart keeps the master key and Postgres password from the existing Secret.
    log "Upgrading the Helm release (applies chart changes)"
    pi "$SRC" "${images[@]}" <<EOF
chart=\$1; shift
$HELM upgrade hyperbeam "\$chart/deploy/helm/hyperbeam" -n $NS --reuse-values "\$@" >/dev/null
EOF
  else
    [[ -n $PUBLIC_URL ]] || die "set PUBLIC_URL for the first install, e.g. PUBLIC_URL=https://hyperbeam.example.com"
    log "Installing the Helm release (master key: ~/$KEY_FILE on the Pi; back it up)"
    pi "$SRC" "$KEY_FILE" "${images[@]}" \
      --set "publicUrl=$PUBLIC_URL" --set "agents.maxSessions=$MAX_SESSIONS" --set "agents.memoryLimit=$MEMORY_LIMIT" <<EOF
chart=\$1; key=\$2; shift 2
umask 077
mkdir -p "\$(dirname "\$key")"
[ -s "\$key" ] || openssl rand -base64 32 > "\$key"
$HELM install hyperbeam "\$chart/deploy/helm/hyperbeam" -n $NS --create-namespace \
  --set-string "masterKey=\$(cat "\$key")" "\$@" >/dev/null
EOF
  fi
  wait_ready
}

wait_ready() {
  log "Waiting for Postgres and the control plane"
  pi <<EOF || die "the control plane did not become ready (scripts/pi.sh logs)"
$K -n $NS rollout status statefulset/hyperbeam-postgres --timeout=300s >/dev/null
$K -n $NS rollout status deploy/hyperbeam --timeout=300s >/dev/null || { $K -n $NS get pods; $K -n $NS logs deploy/hyperbeam --tail=20; exit 1; }
EOF
}

# Point builtin harness profiles at the local images, but only while they still
# name the published default, so edits made in Settings are left alone.
point_harnesses() {
  local h sql=""
  for h in $HARNESSES; do
    sql+="UPDATE harnesses SET image = '$(image "harness-$h")' WHERE name = '$h' AND image LIKE 'ghcr.io/hyperbeam/%';"
  done
  sql+="SELECT name || ' -> ' || image FROM harnesses ORDER BY name"
  log "Harness images:"
  pi "$sql" <<EOF | sed 's/^/    /'
$K -n $NS exec statefulset/hyperbeam-postgres -- psql -U hyperbeam -d hyperbeam -qtAc "\$1"
EOF
}

# --- commands --------------------------------------------------------------

cmd_up() {
  sync_source
  ensure_images
  ensure_release
  point_harnesses
  log "Hyperbeam is running on the Pi. Its Service address, for a tunnel or proxy:"
  pi <<EOF
echo "    http://\$($K -n $NS get svc hyperbeam -o jsonpath='{.spec.clusterIP}'):8080"
EOF
}

cmd_update() {
  sync_source
  if [[ ${1:-} == harness ]]; then
    shift
    build_harnesses "${@:-claude-code}"
    log "New sandboxes will use the rebuilt harness image."
  else
    build_core
    log "Restarting the control plane"
    pi <<<"$K -n $NS rollout restart deploy/hyperbeam >/dev/null"
    wait_ready
    pi <<<"$K -n $NS get pods"
  fi
}

cmd_status() {
  pi <<EOF
$K get nodes
echo
$K get pods -n $NS -o wide
echo
$K get pods -n hyperbeam-agents 2>/dev/null || true
echo
sudo k3s crictl images 2>/dev/null | grep -E 'IMAGE|hyperbeam'
echo
free -h | sed -n 1,2p
EOF
}

main() {
  local cmd=${1:-}
  shift || true
  case $cmd in
    -h|--help|help|"") sed -n '2,28p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
  esac
  require
  case $cmd in
    up) cmd_up "$@" ;;
    update) cmd_update "$@" ;;
    status) cmd_status ;;
    logs) pi_tty "$K -n $NS logs deploy/hyperbeam -f $(printf '%q ' "$@")" ;;
    psql) pi_tty "$K -n $NS exec -it statefulset/hyperbeam-postgres -- psql -U hyperbeam -d hyperbeam" ;;
    *) die "unknown command '$cmd' (try --help)" ;;
  esac
}

main "$@"
