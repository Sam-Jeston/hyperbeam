#!/usr/bin/env bash
# Run the test suite locally, the same way CI does (.github/workflows/test.yml),
# against a throwaway Postgres in Docker so the end-to-end tests really run
# instead of skipping themselves.
#
#   scripts/test.sh             The full gate: web, docs, clippy, every Rust test (default)
#   scripts/test.sh rust [ARGS] Only the Rust tests; ARGS go to `cargo test`,
#                               e.g. `scripts/test.sh rust -p hyperbeam-server --test e2e`
#   scripts/test.sh db          Start the test Postgres and print its URL
#   scripts/test.sh env         Print an `export` line, for `eval "$(scripts/test.sh env)"`
#   scripts/test.sh db-stop     Remove the test Postgres container
#
# The container keeps its data in memory (tmpfs). Every test creates a database
# of its own and never drops it, so removing the container is the clean-up.
# It is left running between runs; `db-stop` removes it.
#
# Environment:
#   PG_CONTAINER=hyperbeam-test-pg   container name
#   PG_PORT=5441                     host port (5440 is the dev Postgres in the README)
#   PG_IMAGE=postgres:16             same image as CI
#   HYPERBEAM_TEST_DATABASE_URL      if already set, used as-is and no container is started
set -euo pipefail

PG_CONTAINER=${PG_CONTAINER:-hyperbeam-test-pg}
PG_PORT=${PG_PORT:-5441}
PG_IMAGE=${PG_IMAGE:-postgres:16}
ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
DEFAULT_URL="postgres://hyperbeam:hyperbeam@127.0.0.1:$PG_PORT/postgres"

log() { printf '\033[1;34m==>\033[0m %s\n' "$*" >&2; }
die() { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }

need_docker() {
  command -v docker >/dev/null \
    || die "docker not found. On WSL, start Docker Desktop and enable WSL integration for this distro."
  docker info >/dev/null 2>&1 \
    || die "the Docker daemon is not reachable. Start Docker Desktop (or dockerd) and try again."
}

db_up() {
  if [ -n "${HYPERBEAM_TEST_DATABASE_URL:-}" ]; then
    log "using HYPERBEAM_TEST_DATABASE_URL from the environment"
    return
  fi
  need_docker
  local state
  state=$(docker inspect -f '{{.State.Running}}' "$PG_CONTAINER" 2>/dev/null || true)
  if [ "$state" = "false" ]; then
    docker start "$PG_CONTAINER" >/dev/null
  elif [ -z "$state" ]; then
    log "starting $PG_IMAGE as $PG_CONTAINER on 127.0.0.1:$PG_PORT"
    docker run -d --name "$PG_CONTAINER" \
      -e POSTGRES_USER=hyperbeam -e POSTGRES_PASSWORD=hyperbeam \
      -p "127.0.0.1:$PG_PORT:5432" --tmpfs /var/lib/postgresql/data \
      "$PG_IMAGE" -c fsync=off -c max_connections=500 >/dev/null
  fi
  log "waiting for Postgres"
  for _ in $(seq 60); do
    # pg_isready inside the container can succeed during initdb's temporary
    # server, so also check a real query over TCP.
    if docker exec "$PG_CONTAINER" psql -h 127.0.0.1 -U hyperbeam -d postgres -tAc 'select 1' >/dev/null 2>&1; then
      export HYPERBEAM_TEST_DATABASE_URL="$DEFAULT_URL"
      return
    fi
    sleep 1
  done
  docker logs --tail 30 "$PG_CONTAINER" >&2 || true
  die "Postgres did not become ready"
}

rust_tests() {
  cd "$ROOT"
  # The end-to-end tests start these as real processes.
  log "building the runner and the test agent"
  cargo build --locked -p hyperbeam-runner -p hyperbeam-test-agent
  log "cargo test (HYPERBEAM_TEST_DATABASE_URL=$HYPERBEAM_TEST_DATABASE_URL)"
  if [ $# -eq 0 ]; then set -- --workspace; fi
  cargo test --locked "$@"
}

gate() {
  cd "$ROOT"
  # The server embeds web/dist, so the web app is built before any Rust.
  log "web: install, typecheck, build"
  (cd web && bun install --frozen-lockfile && bun run typecheck && bun run build)
  log "docs: install, build (fails on broken links)"
  (cd docs && bun install --frozen-lockfile && bun run build)
  log "clippy"
  cargo clippy --workspace --all-targets --locked -- -D warnings
  rust_tests
  log "all green"
}

cmd=${1:-all}
[ $# -gt 0 ] && shift
case "$cmd" in
  all)     db_up; gate ;;
  rust)    db_up; rust_tests "$@" ;;
  db)      db_up; echo "$HYPERBEAM_TEST_DATABASE_URL" ;;
  env)     db_up; echo "export HYPERBEAM_TEST_DATABASE_URL=$HYPERBEAM_TEST_DATABASE_URL" ;;
  db-stop) need_docker; docker rm -f "$PG_CONTAINER" >/dev/null 2>&1 || true; log "removed $PG_CONTAINER" ;;
  -h|--help|help) sed -n '2,/^set -euo/p' "$0" | sed '$d; s/^# \{0,1\}//' ;;
  *) die "unknown command: $cmd (try --help)" ;;
esac
