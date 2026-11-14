#!/usr/bin/env bash
# Run Hyperbeam in a local kind cluster for development.
#
#   scripts/dev-cluster.sh up        Start everything (after a reboot, or from scratch) and port-forward the UI
#   scripts/dev-cluster.sh update    Rebuild server + runner, load them, restart the control plane
#   scripts/dev-cluster.sh update harness [codex ...]   Rebuild harness images (claude-code by default)
#   scripts/dev-cluster.sh forward   Port-forward the UI to localhost:8080, reconnecting on restarts
#   scripts/dev-cluster.sh status    Nodes, pods and loaded images
#   scripts/dev-cluster.sh logs      Follow control plane logs (extra args go to kubectl logs)
#   scripts/dev-cluster.sh psql      Open psql on the bundled Postgres
#   scripts/dev-cluster.sh stop      Stop the kind node container (data is kept)
#
# Images always use the `dev` tag. Loading a rebuilt `dev` image replaces the
# old one in the node, and the chart pulls only when an image is missing, so
# new pods pick up the new build without changing any settings.
#
# Environment:
#   CLUSTER=hyperbeam           kind cluster name
#   HARNESSES="claude-code"     harness images to build on first `up`
#   PORT=8080                   local port for the UI
#   MAX_SESSIONS=4              concurrent sandboxes (chart default is 20)
set -euo pipefail

CLUSTER=${CLUSTER:-hyperbeam}
HARNESSES=${HARNESSES:-claude-code}
PORT=${PORT:-8080}
MAX_SESSIONS=${MAX_SESSIONS:-4}

NODE="$CLUSTER-control-plane"
CTX="kind-$CLUSTER"
NS=hyperbeam
TAG=dev
ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
KEY_FILE="$ROOT/hyperbeam-master-key"

k() { kubectl --context "$CTX" "$@"; }
log() { printf '\033[1;34m==>\033[0m %s\n' "$*"; }
die() { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }

require() {
  for bin in docker kind kubectl helm; do
    command -v "$bin" >/dev/null || die "$bin is not installed"
  done
  docker info >/dev/null 2>&1 || die "Docker is not running"
}

# --- cluster ---------------------------------------------------------------

ensure_cluster() {
  if ! kind get clusters 2>/dev/null | grep -qx "$CLUSTER"; then
    log "Creating kind cluster '$CLUSTER'"
    kind create cluster --name "$CLUSTER"
  elif [[ $(docker inspect -f '{{.State.Running}}' "$NODE") != true ]]; then
    # kind nodes use restart=on-failure, so they often stay down after a reboot.
    log "Starting node container $NODE"
    docker start "$NODE" >/dev/null
  fi
  log "Waiting for the API server"
  local i
  for i in $(seq 60); do
    k get --raw /readyz >/dev/null 2>&1 && break
    sleep 2
  done
  k wait --for=condition=Ready node --all --timeout=120s >/dev/null
}

# --- images ----------------------------------------------------------------

on_node() { docker exec "$NODE" crictl inspecti "docker.io/$1" >/dev/null 2>&1; }

build() { # build <dockerfile> <target> <image>
  log "Building $3"
  docker build -f "$ROOT/deploy/docker/$1" --target "$2" -t "$3" "$ROOT"
}

load() {
  log "Loading into $CLUSTER: $*"
  kind load docker-image --name "$CLUSTER" "$@"
  # Reloading a tag leaves the previous build untagged in the node; drop those.
  docker exec "$NODE" sh -c \
    "crictl images | awk '\$2==\"<none>\" {print \$3}' | xargs -r crictl rmi >/dev/null 2>&1" || true
}

harness_image() { echo "hyperbeam/harness-$1:$TAG"; }

build_core() {
  build Dockerfile server "hyperbeam/server:$TAG"
  build Dockerfile runner "hyperbeam/runner:$TAG"
  load "hyperbeam/server:$TAG" "hyperbeam/runner:$TAG"
}

build_harnesses() {
  local h images=()
  for h in "$@"; do
    build Dockerfile.harness "$h" "$(harness_image "$h")"
    images+=("$(harness_image "$h")")
  done
  load "${images[@]}"
}

# Build only what the node doesn't have yet (fresh cluster), loading from the
# local Docker cache when the image was built before.
ensure_images() {
  local img missing_core=0 missing_harness=()
  for img in "hyperbeam/server:$TAG" "hyperbeam/runner:$TAG"; do
    on_node "$img" && continue
    if docker image inspect "$img" >/dev/null 2>&1; then load "$img"; else missing_core=1; fi
  done
  (( missing_core )) && build_core
  local h
  for h in $HARNESSES; do
    img=$(harness_image "$h")
    on_node "$img" && continue
    if docker image inspect "$img" >/dev/null 2>&1; then load "$img"; else missing_harness+=("$h"); fi
  done
  (( ${#missing_harness[@]} )) && build_harnesses "${missing_harness[@]}"
  return 0
}

# --- release ---------------------------------------------------------------

ensure_release() {
  local args=(
    upgrade --install hyperbeam "$ROOT/deploy/helm/hyperbeam"
    --kube-context "$CTX" --namespace "$NS" --create-namespace
    --set image.repository=hyperbeam/server --set image.tag=$TAG
    --set agents.runnerImage.repository=hyperbeam/runner --set agents.runnerImage.tag=$TAG
    --set agents.maxSessions="$MAX_SESSIONS"
  )
  if helm status hyperbeam --kube-context "$CTX" -n "$NS" >/dev/null 2>&1; then
    # The chart keeps the master key and Postgres password from the existing Secret.
    log "Upgrading the Helm release (applies chart changes)"
    args+=(--reuse-values)
  else
    if [[ ! -s $KEY_FILE ]]; then
      log "Creating master key at $KEY_FILE (back it up)"
      openssl rand -base64 32 > "$KEY_FILE"
    fi
    log "Installing the Helm release"
    args+=(--set masterKey="$(cat "$KEY_FILE")")
  fi
  helm "${args[@]}" >/dev/null
  wait_ready
}

# After a node restart the API still reports the old pods as Running/Ready
# until the kubelet catches up, so `rollout status` can return too early.
# Instead, ask Postgres directly and require every control plane pod to be Ready.
wait_ready() {
  log "Waiting for Postgres and the control plane"
  local i not_ready
  for i in $(seq 90); do
    if k -n "$NS" exec statefulset/hyperbeam-postgres -- pg_isready -U hyperbeam -q >/dev/null 2>&1; then
      not_ready=$(k -n "$NS" get pods -o \
        jsonpath='{range .items[*]}{.status.phase}/{.status.conditions[?(@.type=="Ready")].status}{"\n"}{end}' \
        2>/dev/null | grep -vc '^Running/True$' || true)
      k -n "$NS" rollout status deploy/hyperbeam --timeout=5s >/dev/null 2>&1 && [[ $not_ready == 0 ]] && return 0
    fi
    sleep 2
  done
  k -n "$NS" get pods
  die "control plane did not become ready"
}

# Point builtin harness profiles at the local images, but only while they still
# name the published default, so edits made in Settings are left alone.
point_harnesses() {
  local h sql=""
  for h in $HARNESSES; do
    sql+="UPDATE harnesses SET image = '$(harness_image "$h")' WHERE name = '$h' AND image LIKE 'ghcr.io/%hyperbeam/harness-%';"
  done
  local out
  out=$(k -n "$NS" exec statefulset/hyperbeam-postgres -- psql -U hyperbeam -d hyperbeam -qtAc "$sql; SELECT name || ' -> ' || image FROM harnesses ORDER BY name" 2>&1) \
    || { echo "$out" >&2; die "could not update harness images"; }
  log "Harness images:"
  sed 's/^/    /' <<<"$out"
}

clear_stale_sandboxes() {
  # Sandboxes don't survive a node restart; remove the leftovers so they don't
  # sit in Error/Unknown. The control plane makes fresh ones on the next message.
  k -n hyperbeam-agents delete pods --field-selector=status.phase!=Running,status.phase!=Pending \
    --ignore-not-found --wait=false >/dev/null 2>&1 || true
}

# --- commands --------------------------------------------------------------

forward() {
  log "UI at http://localhost:$PORT (Ctrl-C to stop)"
  while true; do
    k -n "$NS" port-forward svc/hyperbeam "$PORT:8080" || true
    echo "port-forward dropped; reconnecting..." >&2
    sleep 2
  done
}

cmd_up() {
  ensure_cluster
  ensure_images
  ensure_release
  point_harnesses
  clear_stale_sandboxes
  [[ ${1:-} == --no-forward ]] || forward
}

cmd_update() {
  ensure_cluster
  if [[ ${1:-} == harness ]]; then
    shift
    build_harnesses "${@:-claude-code}"
    log "New sandboxes will use the rebuilt harness image."
  else
    build_core
    log "Restarting the control plane"
    k -n "$NS" rollout restart deploy/hyperbeam >/dev/null
    k -n "$NS" rollout status deploy/hyperbeam --timeout=180s
  fi
}

cmd_status() {
  k get nodes
  echo
  k get pods -n "$NS" -o wide
  echo
  k get pods -n hyperbeam-agents 2>/dev/null || true
  echo
  docker exec "$NODE" crictl images | grep -E 'IMAGE|hyperbeam'
}

main() {
  local cmd=${1:-up}
  shift || true
  require
  case $cmd in
    up) cmd_up "$@" ;;
    update) cmd_update "$@" ;;
    forward) forward ;;
    status) cmd_status ;;
    logs) k -n "$NS" logs deploy/hyperbeam -f "$@" ;;
    psql) k -n "$NS" exec -it statefulset/hyperbeam-postgres -- psql -U hyperbeam -d hyperbeam ;;
    stop) log "Stopping $NODE"; docker stop "$NODE" >/dev/null ;;
    -h|--help|help) sed -n '2,21p' "$0" | sed 's/^# \{0,1\}//' ;;
    *) die "unknown command '$cmd' (try --help)" ;;
  esac
}

main "$@"
