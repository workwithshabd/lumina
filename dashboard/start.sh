#!/usr/bin/env bash
# ==============================================================================
# Atihsrah MERN Studio Dashboard Launcher
#
# Starts both the high-performance Atihsrah image proxy (port 8080)
# and the MERN Studio Dashboard (port 5001).
# ==============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

echo "========================================================================"
echo "          🚀 Starting Atihsrah Studio & MERN Dashboard"
echo "========================================================================"

PROXY_PORT="8080"
METRICS_PORT="9090"
DASHBOARD_PORT="5001"

# 1. Clean up any stale/orphaned processes holding the ports
for p in "${PROXY_PORT}" "${METRICS_PORT}" "${DASHBOARD_PORT}"; do
    if lsof -ti ":${p}" > /dev/null 2>&1; then
        echo "[*] Cleaning up orphaned process holding port :${p}..."
        lsof -ti ":${p}" | xargs kill -9 2>/dev/null || true
        sleep 0.5
    fi
done

# 2. Check or build release binary
if [ ! -f "${ROOT_DIR}/target/release/atihsrah" ]; then
    echo "[*] Building release binary with Thin LTO..."
    cargo build --release --manifest-path "${ROOT_DIR}/Cargo.toml"
fi

# 3. Signing Key & Environment
SIGNING_KEY="${ATIHSRAH_SIGNING_KEY:-b87ee72d9bdae9a943c0ee2b51d296df78ed27534bace77c2d93e30f13344f4d}"

echo "[*] Launching Atihsrah Proxy on 127.0.0.1:${PROXY_PORT} (Metrics on :${METRICS_PORT})..."
ATIHSRAH_BIND="127.0.0.1:${PROXY_PORT}" \
ATIHSRAH_METRICS_BIND="127.0.0.1:${METRICS_PORT}" \
ATIHSRAH_AUTH_MODE="signed" \
ATIHSRAH_SIGNING_KEY="${SIGNING_KEY}" \
ALLOWED_ORIGINS="https://images.unsplash.com,https://upload.wikimedia.org" \
ATIHSRAH_PRIVATE_ORIGINS="http://127.0.0.1:${DASHBOARD_PORT}" \
ATIHSRAH_AVIF_SPEED="9" \
"${ROOT_DIR}/target/release/atihsrah" > /tmp/atihsrah_proxy.log 2>&1 &
PROXY_PID=$!

echo "[*] Launching MERN Dashboard Server on 127.0.0.1:${DASHBOARD_PORT}..."
ATIHSRAH_PROXY_URL="http://127.0.0.1:${PROXY_PORT}" \
ATIHSRAH_METRICS_URL="http://127.0.0.1:${METRICS_PORT}" \
ATIHSRAH_SIGNING_KEY="${SIGNING_KEY}" \
PORT="${DASHBOARD_PORT}" \
node "${SCRIPT_DIR}/server/index.js" &
SERVER_PID=$!

cleanup() {
    echo ""
    echo "[*] Shutting down Atihsrah services..."
    kill "$PROXY_PID" 2>/dev/null || true
    kill "$SERVER_PID" 2>/dev/null || true
    echo "[+] Done. Goodbye!"
}
trap cleanup EXIT INT TERM

# 4. Wait for Atihsrah Proxy ready
retries=0
until curl -s -f "http://127.0.0.1:${PROXY_PORT}/ready" > /dev/null 2>&1; do
    if ! kill -0 "$PROXY_PID" 2>/dev/null; then
        echo "[-] Atihsrah proxy process failed to start! Logs from /tmp/atihsrah_proxy.log:"
        cat /tmp/atihsrah_proxy.log
        exit 1
    fi
    retries=$((retries + 1))
    if [ $retries -ge 50 ]; then
        echo "[-] Timeout waiting for Atihsrah proxy to be ready."
        exit 1
    fi
    sleep 0.2
done

# 5. Wait for Dashboard Server ready
retries=0
until curl -s -f "http://127.0.0.1:${DASHBOARD_PORT}/api/status" > /dev/null 2>&1; do
    if ! kill -0 "$SERVER_PID" 2>/dev/null; then
        echo "[-] MERN Dashboard server failed to start!"
        exit 1
    fi
    retries=$((retries + 1))
    if [ $retries -ge 50 ]; then
        echo "[-] Timeout waiting for Dashboard server."
        exit 1
    fi
    sleep 0.2
done

echo ""
echo "========================================================================"
echo "  ✨ Studio Dashboard Ready!"
echo "  👉 Open your browser at: http://127.0.0.1:${DASHBOARD_PORT}"
echo ""
echo "  Services:"
echo "  - Web UI:            http://127.0.0.1:${DASHBOARD_PORT}/"
echo "  - Proxy API:         http://127.0.0.1:${PROXY_PORT}/transform"
echo "  - Metrics:           http://127.0.0.1:${METRICS_PORT}/metrics"
echo "  - Dashboard Backend: http://127.0.0.1:${DASHBOARD_PORT}/api/status"
echo "========================================================================"
echo "Press Ctrl+C in this terminal to stop all services."

# Monitor processes
while kill -0 "$PROXY_PID" 2>/dev/null && kill -0 "$SERVER_PID" 2>/dev/null; do
    sleep 1
done
