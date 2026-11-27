#!/usr/bin/env bash
# One-time (idempotent) provisioning of the dalang VM for BunAmateur.
#
# Creates /opt/bunamateur/{releases,.env}, a PostgreSQL role + database,
# the systemd unit, and cloudflared ingress rules for 文.ngublag.com (primary)
# and bun.ngublag.com (alias).
# Safe to re-run: existing secrets in /opt/bunamateur/.env are never rotated.
#
# Usage: ./deploy/bootstrap-server.sh [user@host]
set -euo pipefail

host="${1:-ardfard@dalang}"
here="$(cd "$(dirname "$0")" && pwd)"

scp -q "$here/bunamateur.service" "$host:/tmp/bunamateur.service"

ssh -o BatchMode=yes "$host" bash -s <<'EOF'
set -euo pipefail
root=/opt/bunamateur
port=8040
hostname=xn--7dv.ngublag.com   # 文.ngublag.com (IDNA / punycode): primary, used in email links
aliases=(bun.ngublag.com)      # extra hostnames that serve the same site
hostnames=("$hostname" "${aliases[@]}")
origins=$(printf 'https://%s,' "${hostnames[@]}"); origins=${origins%,}
ingress=/etc/cloudflared/config.yml

sudo install -d -o ardfard -g ardfard -m 755 "$root" "$root/releases"

# --- PostgreSQL -------------------------------------------------------------
if [[ ! -f $root/.env ]]; then
  db_pass="$(openssl rand -hex 24)"
  enc_key="$(openssl rand -hex 32)"
  if ! sudo -u postgres psql -tAc "SELECT 1 FROM pg_roles WHERE rolname='bunamateur'" | grep -q 1; then
    sudo -u postgres psql -v ON_ERROR_STOP=1 -c "CREATE ROLE bunamateur LOGIN PASSWORD '$db_pass'"
  else
    sudo -u postgres psql -v ON_ERROR_STOP=1 -c "ALTER ROLE bunamateur PASSWORD '$db_pass'"
  fi
  if ! sudo -u postgres psql -tAc "SELECT 1 FROM pg_database WHERE datname='bunamateur'" | grep -q 1; then
    sudo -u postgres createdb -O bunamateur bunamateur
  fi

  umask 077
  cat > "$root/.env" <<ENV
APP_ENV=production
HOST=127.0.0.1
PORT=$port
DATABASE_URL=postgres://bunamateur:$db_pass@127.0.0.1:5432/bunamateur
ENCRYPTION_KEY=$enc_key
APP_PUBLIC_URL=https://$hostname
ALLOWED_ORIGINS=$origins
COOKIE_SECURE=true
# cloudflared on loopback is the only client; it sets X-Forwarded-For.
TRUST_PROXY=true
SESSION_TTL_DAYS=30
# Switch to resend + RESEND_API_KEY to send real mail.
EMAIL_PROVIDER=log
EMAIL_FROM="BunAmateur <onboarding@resend.dev>"
RUST_LOG=bunamateur=info,tower_http=info
ENV
  echo "wrote $root/.env (new DB password + ENCRYPTION_KEY)"
else
  echo "$root/.env exists — leaving secrets untouched"
fi

# Keys added after the first bootstrap: append only if missing.
ensure() { grep -q "^$1=" "$root/.env" || { echo "$1=$2" >> "$root/.env"; echo "added $1 to .env"; }; }
ensure LOG_FORMAT json
# /metrics is public behind the tunnel unless this is set.
ensure METRICS_BEARER_TOKEN "$(openssl rand -hex 24)"
# Every served hostname must be an allowed origin, or CORS/CSRF rejects it.
for h in "${hostnames[@]}"; do
  if ! grep -qE "^ALLOWED_ORIGINS=(.*,)?https://$h(,|$)" "$root/.env"; then
    sed -i "s|^ALLOWED_ORIGINS=\\(.*\\)|ALLOWED_ORIGINS=\\1,https://$h|" "$root/.env"
    echo "added https://$h to ALLOWED_ORIGINS"
  fi
done

# --- systemd ----------------------------------------------------------------
sudo install -m 644 /tmp/bunamateur.service /etc/systemd/system/bunamateur.service
rm -f /tmp/bunamateur.service
sudo systemctl daemon-reload
sudo systemctl enable bunamateur >/dev/null

# --- cloudflared ingress ----------------------------------------------------
changed=false
for h in "${hostnames[@]}"; do
  if ! sudo grep -q "hostname: $h$" "$ingress"; then
    $changed || sudo cp "$ingress" "$ingress.bak.$(date +%Y%m%d%H%M%S)"
    # Insert before the catch-all 404 rule (must stay last).
    sudo sed -i "s|^  - service: http_status:404|  - hostname: $h\n    service: http://127.0.0.1:$port\n  - service: http_status:404|" "$ingress"
    echo "added $h -> 127.0.0.1:$port to cloudflared ingress"
    changed=true
  else
    echo "cloudflared ingress for $h already present"
  fi
done
if $changed; then
  sudo cloudflared tunnel --config "$ingress" ingress validate
  sudo systemctl restart cloudflared
fi
EOF
