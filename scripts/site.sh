#!/usr/bin/env bash
# Build the landing page and docs, and deploy them as one static site to a
# machine that serves them with Bun behind a Cloudflare tunnel (the same Pi
# scripts/pi.sh deploys Hyperbeam to).
#
#   scripts/site.sh build                  Build into site/public-build/ (landing at /, docs at /docs)
#   PI=user@host scripts/site.sh deploy    Build, copy to the Pi, (re)start its service
#   PI=user@host scripts/site.sh status    Service state and a local request on the Pi
#
# Environment:
#   PI=user@host          SSH destination (required for deploy/status)
#   SITE_HOST=hyperbeam.fun   Canonical host; other hosts (www.) are redirected to it
#   SITE_PORT=4330        Loopback port the service listens on, for cloudflared
#   HYPERBEAM_REPO_URL    Source links ("Edit this page", GitHub icon); defaults to the public repo
#
# On the Pi it installs ~/hyperbeam-site/{serve.ts,public/} and a systemd unit,
# hyperbeam-site.service. The tunnel's ingress for SITE_HOST is set up once by
# hand in /etc/cloudflared/config.yml (see docs: Development → Landing page and docs).
set -euo pipefail

PI=${PI:-}
SITE_HOST=${SITE_HOST:-hyperbeam.fun}
SITE_PORT=${SITE_PORT:-4330}
export HYPERBEAM_REPO_URL=${HYPERBEAM_REPO_URL:-https://github.com/Sam-Jeston/hyperbeam}

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
OUT=$ROOT/site/public-build
REMOTE=hyperbeam-site

log() { printf '\033[1;34m==>\033[0m %s\n' "$*"; }
die() { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }

cmd_build() {
  log "Building the docs and landing page (source links: $HYPERBEAM_REPO_URL)"
  (cd "$ROOT/docs" && bun install --frozen-lockfile >/dev/null && bun run build)
  (cd "$ROOT/site" && bun install --frozen-lockfile >/dev/null && bun run build)
  rm -rf "$OUT"
  mkdir -p "$OUT"
  cp -R "$ROOT/site/dist/." "$OUT/"
  cp -R "$ROOT/docs/dist" "$OUT/docs"
  log "Built $(find "$OUT" -type f | wc -l | tr -d ' ') files → ${OUT#"$ROOT"/}"
}

require_pi() {
  [[ -n $PI ]] || die "set PI to the SSH destination, e.g. PI=sam@192.168.1.40"
  ssh -o BatchMode=yes -o ConnectTimeout=10 "$PI" true 2>/dev/null || die "cannot reach $PI over SSH with a key"
}

unit() {
  cat <<EOF
# Installed by scripts/site.sh in the Hyperbeam repository; edits here are overwritten on deploy.
# Serves the landing page and docs on loopback; cloudflared puts them on the internet as $SITE_HOST.
[Unit]
Description=Hyperbeam landing page and docs ($SITE_HOST), static files served by Bun
After=network-online.target
Wants=network-online.target
StartLimitIntervalSec=0

[Service]
User=__USER__
WorkingDirectory=__HOME__/$REMOTE
ExecStart=__HOME__/.bun/bin/bun serve.ts
Environment=PORT=$SITE_PORT
Environment=HOST=127.0.0.1
Environment=CANONICAL_HOST=$SITE_HOST
Restart=always
RestartSec=2

NoNewPrivileges=yes
ProtectSystem=strict
ProtectHome=read-only
PrivateTmp=yes
PrivateDevices=yes

[Install]
WantedBy=multi-user.target
EOF
}

cmd_deploy() {
  require_pi
  cmd_build
  log "Copying to $PI:~/$REMOTE"
  ssh -o BatchMode=yes "$PI" "mkdir -p $REMOTE"
  rsync -az --delete "$OUT/" "$PI:$REMOTE/public/"
  rsync -az "$ROOT/deploy/site/serve.ts" "$PI:$REMOTE/serve.ts"
  log "Installing and restarting hyperbeam-site.service"
  unit | ssh -o BatchMode=yes "$PI" "bash -euo pipefail -c '
    [ -x \"\$HOME/.bun/bin/bun\" ] || { echo \"bun is not installed at ~/.bun/bin/bun\" >&2; exit 1; }
    sed -e \"s#__USER__#\$(id -un)#\" -e \"s#__HOME__#\$HOME#g\" | sudo tee /etc/systemd/system/hyperbeam-site.service >/dev/null
    sudo systemctl daemon-reload
    sudo systemctl enable hyperbeam-site.service >/dev/null 2>&1
    sudo systemctl restart hyperbeam-site.service
    for i in \$(seq 1 20); do
      curl -fsS -o /dev/null -H \"Host: $SITE_HOST\" http://127.0.0.1:$SITE_PORT/ && exit 0
      sleep 0.5
    done
    sudo journalctl -u hyperbeam-site -n 20 --no-pager >&2
    exit 1
  '" || die "the site did not come up on the Pi"
  log "Serving on the Pi at 127.0.0.1:$SITE_PORT, public as https://$SITE_HOST"
}

cmd_status() {
  require_pi
  ssh -o BatchMode=yes "$PI" "systemctl status hyperbeam-site --no-pager | sed -n 1,5p; curl -sS -o /dev/null -w 'GET / -> %{http_code}\n' -H 'Host: $SITE_HOST' http://127.0.0.1:$SITE_PORT/"
}

case ${1:-} in
  build) cmd_build ;;
  deploy) cmd_deploy ;;
  status) cmd_status ;;
  *) sed -n '2,18p' "$0" | sed 's/^# \{0,1\}//'; exit 1 ;;
esac
