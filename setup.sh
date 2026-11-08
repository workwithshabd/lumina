#!/usr/bin/env bash
# ==============================================================================
# Atihsrah Setup Script (Automated Local Deployment)
#
# Verifies Docker prerequisites, provisions .env, validates ALLOWED_ORIGINS,
# generates HMAC signing key if missing, boots the container stack via Compose,
# verifies readiness (/ready), and prints a valid ready-to-use signed URL.
# ==============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

echo "=============================================================================="
echo " Starting Atihsrah Local Setup"
echo "=============================================================================="

# ------------------------------------------------------------------------------
# 1. Verify Docker Engine
# ------------------------------------------------------------------------------
if ! command -v docker >/dev/null 2>&1; then
    echo "ERROR: Docker CLI is not installed or not in PATH." >&2
    echo "Please install Docker from https://docs.docker.com/engine/install/ to continue." >&2
    exit 1
fi

if ! docker info >/dev/null 2>&1; then
    echo "ERROR: Docker daemon is not running or accessible." >&2
    echo "Please ensure the Docker service or VM is started." >&2
    exit 1
fi

# Detect Docker Compose implementation
if docker compose version >/dev/null 2>&1; then
    COMPOSE_CMD="docker compose"
elif command -v docker-compose >/dev/null 2>&1; then
    COMPOSE_CMD="docker-compose"
else
    echo "ERROR: Neither 'docker compose' nor 'docker-compose' was found." >&2
    echo "Please install Docker Compose to proceed." >&2
    exit 1
fi
echo "[OK] Docker and Compose detected ($COMPOSE_CMD)."

# ------------------------------------------------------------------------------
# 2. Check and Create .env
# ------------------------------------------------------------------------------
ENV_FILE=".env"
if [ ! -f "$ENV_FILE" ]; then
    if [ ! -f ".env.example" ]; then
        echo "ERROR: .env.example not found in $SCRIPT_DIR." >&2
        exit 1
    fi
    echo "[INFO] Creating $ENV_FILE from .env.example..."
    cp .env.example "$ENV_FILE"
fi

# Helper function to extract a variable value from .env
get_env_val() {
    local key="$1"
    local raw
    raw="$(grep -E "^[[:space:]]*${key}=" "$ENV_FILE" | tail -n 1 | cut -d '=' -f2- || true)"
    # Strip leading/trailing double quotes, single quotes, and whitespace
    raw="${raw#"${raw%%[![:space:]]*}"}"
    raw="${raw%"${raw##*[![:space:]]}"}"
    raw="${raw#\"}"
    raw="${raw%\"}"
    raw="${raw#\'}"
    raw="${raw%\'}"
    echo "$raw"
}

# ------------------------------------------------------------------------------
# 3. Refuse to run with empty ALLOWED_ORIGINS
# ------------------------------------------------------------------------------
ALLOWED_ORIGINS="$(get_env_val "ALLOWED_ORIGINS")"
if [ -z "$ALLOWED_ORIGINS" ]; then
    echo "" >&2
    echo "ERROR: ALLOWED_ORIGINS is empty or unset in $ENV_FILE." >&2
    echo "Atihsrah is a secure image proxy and fetches ONLY from explicitly" >&2
    echo "allowlisted HTTPS origins." >&2
    echo "" >&2
    echo "Please configure ALLOWED_ORIGINS in .env, e.g.:" >&2
    echo "  ALLOWED_ORIGINS=\"https://images.example.com\"" >&2
    echo "Then re-run ./setup.sh." >&2
    exit 1
fi
echo "[OK] ALLOWED_ORIGINS configured: $ALLOWED_ORIGINS"

# ------------------------------------------------------------------------------
# 4. Check and Generate Signing Key (if missing)
# ------------------------------------------------------------------------------
AUTH_MODE="$(get_env_val "ATIHSRAH_AUTH_MODE")"
[ -z "$AUTH_MODE" ] && AUTH_MODE="signed"

SIGNING_KEY="$(get_env_val "ATIHSRAH_SIGNING_KEY")"
if [ "$AUTH_MODE" = "signed" ] && [ -z "$SIGNING_KEY" ]; then
    if command -v openssl >/dev/null 2>&1; then
        NEW_KEY="$(openssl rand -hex 32)"
    else
        # POSIX fallback using kernel CSPRNG
        NEW_KEY="$(od -vN 32 -An -tx1 /dev/urandom | tr -d ' \n')"
    fi

    if grep -q -E "^[[:space:]]*ATIHSRAH_SIGNING_KEY=" "$ENV_FILE"; then
        # Replace existing empty key entry in-place
        if command -v python3 >/dev/null 2>&1; then
            python3 -c "
with open('$ENV_FILE', 'r') as f: lines = f.readlines()
with open('$ENV_FILE', 'w') as f:
    for line in lines:
        if line.strip().startswith('ATIHSRAH_SIGNING_KEY='):
            f.write('ATIHSRAH_SIGNING_KEY=\"$NEW_KEY\"\n')
        else:
            f.write(line)
"
        else
            sed -i.bak "s|^[[:space:]]*ATIHSRAH_SIGNING_KEY=.*|ATIHSRAH_SIGNING_KEY=\"$NEW_KEY\"|" "$ENV_FILE" && rm -f "${ENV_FILE}.bak"
        fi
    else
        echo "ATIHSRAH_SIGNING_KEY=\"$NEW_KEY\"" >> "$ENV_FILE"
    fi
    SIGNING_KEY="$NEW_KEY"
    echo "[INFO] Generated new 32-byte HMAC-SHA256 signing key in $ENV_FILE."
else
    echo "[OK] Signing key configured."
fi

# ------------------------------------------------------------------------------
# 5. Start Container Stack
# ------------------------------------------------------------------------------
echo "[INFO] Starting Atihsrah container stack ($COMPOSE_CMD up -d --build)..."
$COMPOSE_CMD up -d --build

# ------------------------------------------------------------------------------
# 6. Poll /ready Endpoint Until Healthy
# ------------------------------------------------------------------------------
READY_URL="http://127.0.0.1:8080/ready"
MAX_WAIT_SECS=30
echo "[INFO] Polling readiness probe at $READY_URL (up to ${MAX_WAIT_SECS}s)..."

READY=0
for i in $(seq 1 "$MAX_WAIT_SECS"); do
    if command -v curl >/dev/null 2>&1; then
        STATUS_CODE="$(curl -s -o /dev/null -w "%{http_code}" "$READY_URL" 2>/dev/null || true)"
        if [ "$STATUS_CODE" = "200" ]; then
            READY=1
            break
        fi
    else
        HEALTH_STATUS="$($COMPOSE_CMD ps --format json 2>/dev/null | grep -o '"Health":"[^"]*"' | cut -d '"' -f4 || true)"
        if [ "$HEALTH_STATUS" = "healthy" ]; then
            READY=1
            break
        fi
    fi
    sleep 1
done

if [ "$READY" -ne 1 ]; then
    echo "ERROR: Server failed to become ready within ${MAX_WAIT_SECS} seconds." >&2
    echo "Recent container logs:" >&2
    $COMPOSE_CMD logs --tail=50
    exit 1
fi
echo "[OK] Server is ready and serving traffic (HTTP 200 from $READY_URL)."

# ------------------------------------------------------------------------------
# 7. Generate Signed Example URL
# ------------------------------------------------------------------------------
FIRST_ORIGIN="$(echo "$ALLOWED_ORIGINS" | cut -d ',' -f1 | tr -d '[:space:]')"
SAMPLE_PARAMS="src=${FIRST_ORIGIN}/sample.jpg&w=400&h=300&fit=cover&format=webp"

SIGNED_PATH=""
if [ -n "$SIGNING_KEY" ]; then
    if command -v python3 >/dev/null 2>&1; then
        SIGNED_PATH="$(python3 -c "
import hmac, hashlib, time, urllib.parse, sys

key_hex = '$SIGNING_KEY'
if not key_hex: sys.exit(0)
key = bytes.fromhex(key_hex)
exp = int(time.time()) + 3600
origin = '$FIRST_ORIGIN'.rstrip('/')
src = f'{origin}/sample.jpg'

def rfc3986_quote(v): return urllib.parse.quote(str(v), safe='~')
params = {'exp': str(exp), 'fit': 'cover', 'format': 'webp', 'h': '300', 'src': src, 'w': '400'}
sorted_items = sorted(params.items(), key=lambda x: x[0])
encoded_params = [f'{k}={rfc3986_quote(v)}' for k, v in sorted_items]
canonical_str = f'GET\n/transform\n{exp}\n' + '\n'.join(encoded_params)
sig = hmac.new(key, canonical_str.encode('utf-8'), hashlib.sha256).hexdigest()
query = '&'.join(encoded_params) + f'&sig={sig}'
print(f'/transform?{query}')
" 2>/dev/null || true)"
    fi

    # Fallback to built-in CLI inside the container
    if [ -z "$SIGNED_PATH" ]; then
        SIGNED_PATH="$(docker run --rm -e ATIHSRAH_SIGNING_KEY="$SIGNING_KEY" atihsrah:latest /usr/local/bin/atihsrah --sign "$SAMPLE_PARAMS" 2>/dev/null || true)"
    fi
fi

# ------------------------------------------------------------------------------
# 8. Output Ready Summary
# ------------------------------------------------------------------------------
echo ""
echo "=============================================================================="
echo " Atihsrah is ready and serving!"
echo "=============================================================================="
echo "  Public Proxy:       http://localhost:8080"
echo "  Liveness Probe:     http://localhost:8080/health"
echo "  Readiness Probe:    http://localhost:8080/ready"
echo "  Metrics Exporter:   http://127.0.0.1:9090/metrics (internal)"
echo ""
if [ -n "$SIGNED_PATH" ]; then
    echo "  Sample Signed URL (valid for 1 hour):"
    echo "  http://localhost:8080${SIGNED_PATH}"
    echo ""
    echo "  Note: Replace '$FIRST_ORIGIN/sample.jpg' with a real image path"
    echo "  hosted on your origin server."
else
    echo "  Public Mode Sample URL:"
    echo "  http://localhost:8080/transform?${SAMPLE_PARAMS}"
fi
echo "=============================================================================="
