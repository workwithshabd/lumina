#!/usr/bin/env bash
set -euo pipefail

# Atihsrah vs imgproxy vs Thumbor Master Benchmark Orchestrator (macOS Native & Containerized)
# Executes full reproducible benchmark pipeline and generates artifacts in a unique timestamped run directory.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
COMPOSE_FILE="${SCRIPT_DIR}/docker-compose.bench.yml"
VENV_DIR="${ROOT_DIR}/.venv_bench"
REQ_FILE="${SCRIPT_DIR}/requirements.txt"

cd "${ROOT_DIR}"

echo "========================================================================"
echo "    Atihsrah vs imgproxy vs Thumbor Local macOS Benchmark Pipeline     "
echo "========================================================================"

# 1. Verify Prerequisites
for tool in docker cargo uv curl openssl; do
    if ! command -v "$tool" &> /dev/null; then
        echo "[-] Error: required command '$tool' is not installed or not in PATH."
        exit 1
    fi
done

# 2. Setup or verify isolated pinned Python environment
if [ ! -d "${VENV_DIR}" ]; then
    echo "[*] Initializing isolated benchmark Python environment in .venv_bench..."
    uv venv "${VENV_DIR}" --python 3.12
    uv pip install --python "${VENV_DIR}" -r "${REQ_FILE}"
else
    # Quick check if thumbor and skimage are installed
    if [ ! -f "${VENV_DIR}/bin/thumbor" ]; then
        echo "[*] Installing pinned requirements into .venv_bench..."
        uv pip install --python "${VENV_DIR}" -r "${REQ_FILE}"
    fi
fi

PYTHON_BIN="${VENV_DIR}/bin/python3"
THUMBOR_BIN="${VENV_DIR}/bin/thumbor"

# 3. Parse profile and pass-through arguments
PROFILE="default"
PASSTHROUGH_ARGS=()

while [[ $# -gt 0 ]]; do
    case "$1" in
        --profile=*)
            PROFILE="${1#*=}"
            shift
            ;;
        --profile)
            PROFILE="$2"
            shift 2
            ;;
        *)
            PASSTHROUGH_ARGS+=("$1")
            shift
            ;;
    esac
done

if [ "$PROFILE" = "throughput" ]; then
    export ATIHSRAH_AVIF_SPEED=9
    export CACHE_SINGLE_ENTRY_MAX_MB=25
    PROFILE_LABEL="Benchmark Throughput (speed=9, max_entry=25MB)"
else
    export ATIHSRAH_AVIF_SPEED=6
    export CACHE_SINGLE_ENTRY_MAX_MB=10
    PROFILE_LABEL="Production Default (speed=6, max_entry=10MB)"
fi

echo "[*] Active Profile: ${PROFILE_LABEL}"

# 4. Handle --validate-only early without starting servers or containers
for arg in "${PASSTHROUGH_ARGS[@]+"${PASSTHROUGH_ARGS[@]}"}"; do
    if [ "$arg" = "--validate-only" ]; then
        echo "[*] Validation-only mode requested. Running offline verification suite..."
        "${PYTHON_BIN}" "${SCRIPT_DIR}/benchmark_suite.py" --profile-label "${PROFILE_LABEL}" "${PASSTHROUGH_ARGS[@]}"
        exit $?
    fi
done

# 5. Verify test corpus exists and matches manifest checksums
echo "[*] Verifying 20 real benchmark images against manifest..."
"${SCRIPT_DIR}/download_corpus.sh"

# 6. Build native release binary from current checked-out source
echo "[*] Compiling fresh native macOS release binary (cargo build --release)..."
cargo build --release

# 7. Build container image from current checked-out source (strictly offline)
echo "[*] Building fresh containerized Atihsrah image (docker build --network=none)..."
docker build --network=none -t atihsrah:latest -f Dockerfile .

# 8. Spin up containerized benchmark stack (strictly 127.0.0.1 bindings)
echo "[*] Launching containerized benchmark stack (origin, atihsrah, imgproxy, imgproxy_cached)..."
docker compose -f "${COMPOSE_FILE}" up -d --force-recreate origin atihsrah imgproxy imgproxy_cached

# Process PIDs for cleanup
AT_NATIVE_PID=0
THUMBOR_PID=0

# Guaranteed cleanup trap
cleanup() {
    echo ""
    echo "[*] Stopping benchmark services and performing guaranteed cleanup..."
    if [ "$AT_NATIVE_PID" -gt 0 ]; then
        kill "$AT_NATIVE_PID" 2>/dev/null || true
    fi
    if [ "$THUMBOR_PID" -gt 0 ]; then
        kill "$THUMBOR_PID" 2>/dev/null || true
    fi
    docker compose -f "${COMPOSE_FILE}" down
    echo "[+] Cleanup complete. No benchmark services left running."
}
trap cleanup EXIT INT TERM

# 8. Launch Native Atihsrah on 127.0.0.1:8080 (metrics on 127.0.0.1:9090)
echo "[*] Launching Native Atihsrah on 127.0.0.1:8080 (metrics 127.0.0.1:9090)..."
ATIHSRAH_BIND=127.0.0.1:8080 \
ATIHSRAH_METRICS_BIND=127.0.0.1:9090 \
ALLOWED_ORIGINS=https://localhost:8000,https://127.0.0.1:8000 \
ATIHSRAH_PRIVATE_ORIGINS=https://localhost:8000,https://127.0.0.1:8000 \
ATIHSRAH_PRIVATE_CA_FILE="${ROOT_DIR}/bench/certs/ca.crt" \
ATIHSRAH_AUTH_MODE=signed \
ATIHSRAH_SIGNING_KEY=9f8e7d6c5b4a3928172635445566778899aabbccddeeff001122334455667788 \
ATIHSRAH_AVIF_SPEED="${ATIHSRAH_AVIF_SPEED}" \
MAX_CONCURRENT_TRANSFORMS=8 \
CACHE_MAX_SIZE_MB=500 \
CACHE_SINGLE_ENTRY_MAX_MB="${CACHE_SINGLE_ENTRY_MAX_MB}" \
MAX_SOURCE_FETCH_MB=25 \
RATE_LIMIT_PER_MINUTE=100000 \
RATE_LIMIT_MISS_PER_MINUTE=10000 \
LOG_LEVEL=warn \
ATIHSRAH_DEBUG_HEADERS=true \
"${ROOT_DIR}/target/release/atihsrah" > /tmp/atihsrah_bench_native.log 2>&1 &
AT_NATIVE_PID=$!

# 9. Launch Native Thumbor on 127.0.0.1:8888
echo "[*] Launching Native Thumbor on 127.0.0.1:8888..."
SSL_CERT_FILE="${ROOT_DIR}/bench/certs/ca.crt" \
"${THUMBOR_BIN}" -p 8888 -i 127.0.0.1 --use-environment True > /tmp/thumbor_bench.log 2>&1 &
THUMBOR_PID=$!

# 10. Startup Health Checks with Bounded Timeouts
echo "[*] Polling service health endpoints..."
max_retries=30

retries=0
until curl -k -s -f "https://127.0.0.1:8000/stats" > /dev/null 2>&1; do
    retries=$((retries + 1))
    if [ $retries -ge $max_retries ]; then
        echo "[-] Timeout waiting for mock origin server."
        docker compose -f "${COMPOSE_FILE}" logs origin
        exit 1
    fi
    sleep 1
done

retries=0
until curl -s -f "http://127.0.0.1:8080/ready" > /dev/null 2>&1; do
    retries=$((retries + 1))
    if [ $retries -ge $max_retries ]; then
        echo "[-] Timeout waiting for Native Atihsrah."
        cat /tmp/atihsrah_bench_native.log
        exit 1
    fi
    sleep 1
done

retries=0
until curl -s -f "http://127.0.0.1:8084/ready" > /dev/null 2>&1; do
    retries=$((retries + 1))
    if [ $retries -ge $max_retries ]; then
        echo "[-] Timeout waiting for Containerized Atihsrah."
        docker compose -f "${COMPOSE_FILE}" logs atihsrah
        exit 1
    fi
    sleep 1
done

retries=0
until curl -s -f "http://127.0.0.1:8081/health" > /dev/null 2>&1; do
    retries=$((retries + 1))
    if [ $retries -ge $max_retries ]; then
        echo "[-] Timeout waiting for imgproxy."
        docker compose -f "${COMPOSE_FILE}" logs imgproxy
        exit 1
    fi
    sleep 1
done

retries=0
until curl -s -f "http://127.0.0.1:8082/health" > /dev/null 2>&1; do
    retries=$((retries + 1))
    if [ $retries -ge $max_retries ]; then
        echo "[-] Timeout waiting for imgproxy_cached (nginx)."
        docker compose -f "${COMPOSE_FILE}" logs imgproxy_cached
        exit 1
    fi
    sleep 1
done

retries=0
until curl -s -f "http://127.0.0.1:8888/healthcheck" > /dev/null 2>&1; do
    retries=$((retries + 1))
    if [ $retries -ge $max_retries ]; then
        echo "[-] Timeout waiting for Thumbor."
        cat /tmp/thumbor_bench.log
        exit 1
    fi
    sleep 1
done

echo "[+] All benchmark services healthy and ready on 127.0.0.1."

# 11. Execute Python Benchmark Suite
echo ""
echo "[*] Executing benchmark suite scenarios..."
"${PYTHON_BIN}" "${SCRIPT_DIR}/benchmark_suite.py" \
    --host-atihsrah "http://127.0.0.1:8080" \
    --host-atihsrah-docker "http://127.0.0.1:8084" \
    --host-imgproxy "http://127.0.0.1:8081" \
    --host-imgproxy-cached "http://127.0.0.1:8082" \
    --host-thumbor "http://127.0.0.1:8888" \
    --origin-host "https://127.0.0.1:8000" \
    --origin-container "https://origin:8000" \
    --atihsrah-native-pid "$AT_NATIVE_PID" \
    --thumbor-pid "$THUMBOR_PID" \
    --profile-label "${PROFILE_LABEL}" \
    ${PASSTHROUGH_ARGS[@]+"${PASSTHROUGH_ARGS[@]}"}

echo ""
echo "========================================================================"
echo "[+] All benchmark scenarios completed successfully."
echo "========================================================================"
