#!/usr/bin/env bash
# Ship a release binary to dalang and activate it.
#
#   ./deploy/deploy.sh [binary] [sha] [user@host]
#   ./deploy/deploy.sh --rollback <sha> [user@host]
#
# Defaults: target/release/bunamateur, current git HEAD, ardfard@dalang.
# Uploads to /opt/bunamateur/releases/<sha>, flips the `current` symlink,
# restarts the service and health-gates on 127.0.0.1:8040. If the new release
# fails the health check the previous symlink is restored automatically.
# Keeps the 5 most recent releases.
set -euo pipefail

here="$(cd "$(dirname "$0")/.." && pwd)"
root=/opt/bunamateur

if [[ ${1:-} == --rollback ]]; then
  sha="${2:?usage: deploy.sh --rollback <sha> [host]}"
  host="${3:-ardfard@dalang}"
  bin=""
else
  bin="${1:-$here/target/release/bunamateur}"
  sha="${2:-$(git -C "$here" rev-parse HEAD)}"
  host="${3:-ardfard@dalang}"
  [[ -x $bin ]] || { echo "deploy: missing $bin — run: make release" >&2; exit 1; }
fi

release="$root/releases/$sha"
ssh_opts=(-o BatchMode=yes -o StrictHostKeyChecking=accept-new)

if [[ -n $bin ]]; then
  echo "deploying $sha → $host:$release"
  ssh "${ssh_opts[@]}" "$host" "mkdir -p '$release'"
  scp "${ssh_opts[@]}" -q "$bin" "$host:$release/bunamateur.tmp"
  ssh "${ssh_opts[@]}" "$host" "chmod +x '$release/bunamateur.tmp' && mv '$release/bunamateur.tmp' '$release/bunamateur'"
else
  echo "rolling back to $sha on $host"
fi

ssh "${ssh_opts[@]}" "$host" bash -s -- "$root" "$release" <<'EOF'
set -euo pipefail
root=$1 release=$2
[[ -x $release/bunamateur ]] || { echo "no binary at $release" >&2; exit 1; }
port=$(grep -E '^PORT=' "$root/.env" | cut -d= -f2-); port=${port:-8040}
prev=$(readlink -f "$root/current" 2>/dev/null || true)

activate() { ln -sfn "$1" "$root/current"; sudo systemctl restart bunamateur; }
healthy() {
  for _ in $(seq 1 30); do
    # /readyz pings the DB. Releases older than /readyz return 404; for those,
    # 200 on / is enough (main.rs binds only after DB connect + migrations).
    code=$(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:$port/readyz" || true)
    [[ $code == 200 ]] && return 0
    [[ $code == 404 ]] && curl -fs -o /dev/null "http://127.0.0.1:$port/" && return 0
    sleep 1
  done
  return 1
}

activate "$release"
if ! healthy; then
  echo "health check failed for $release" >&2
  sudo journalctl -u bunamateur -n 40 --no-pager >&2 || true
  if [[ -n $prev && $prev != "$release" ]]; then
    echo "restoring $prev" >&2
    activate "$prev"
  fi
  exit 1
fi

# Prune: keep the 5 newest releases plus whatever is current.
cur=$(readlink -f "$root/current")
ls -1dt "$root"/releases/*/ 2>/dev/null | sed 's:/$::' | tail -n +6 | while read -r d; do
  [[ $d == "$cur" ]] || rm -rf -- "$d"
done
echo "ok: $cur on :$port"
EOF
