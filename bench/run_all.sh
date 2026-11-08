#!/usr/bin/env bash
set -euo pipefail

# Atihsrah vs imgproxy vs Thumbor Master Benchmark Orchestrator (macOS Native & Containerized)
# Executes full reproducible benchmark pipeline and generates artifacts in benchmarks/

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
COMPOSE_FILE="${SCRIPT_DIR}/docker-compose.bench.yml"
BENCHMARKS_DIR="${ROOT_DIR}/benchmarks"

cd "${ROOT_DIR}"

echo "========================================================================"
echo "    Atihsrah vs imgproxy vs Thumbor Local macOS Benchmark Pipeline     "
echo "========================================================================"

# 1. Verify Prerequisites
for tool in docker python3 cargo uv curl openssl; do
    if ! command -v "$tool" &> /dev/null; then
        echo "[-] Error: required command '$tool' is not installed or not in PATH."
        exit 1
    fi
done

# 2. Build Native Release Binary if missing
if [ ! -f "target/release/atihsrah" ]; then
    echo "[*] Compiling optimized native macOS release binary..."
    cargo build --release
fi

# 3. Ensure test corpus exists and is verified
echo "[*] Verifying 20 real benchmark images against manifest..."
"${SCRIPT_DIR}/download_corpus.sh"

# 4. Spin up containerized benchmark stack
echo "[*] Launching containerized benchmark stack (origin, atihsrah_docker, imgproxy, imgproxy_cached)..."
docker compose -f "${COMPOSE_FILE}" up -d --force-recreate origin atihsrah imgproxy imgproxy_cached

# 5. Launch Native Atihsrah on port 8080 (metrics 9090)
echo "[*] Launching Native Atihsrah on port 8080 (metrics 9090)..."
ATIHSRAH_BIND=127.0.0.1:8080 \
ATIHSRAH_METRICS_BIND=127.0.0.1:9090 \
ALLOWED_ORIGINS=https://localhost:8000,https://127.0.0.1:8000 \
ATIHSRAH_PRIVATE_ORIGINS=https://localhost:8000,https://127.0.0.1:8000 \
ATIHSRAH_PRIVATE_CA_FILE="${ROOT_DIR}/bench/certs/ca.crt" \
ATIHSRAH_AUTH_MODE=signed \
ATIHSRAH_SIGNING_KEY=9f8e7d6c5b4a3928172635445566778899aabbccddeeff001122334455667788 \
MAX_CONCURRENT_TRANSFORMS=4 \
CACHE_MAX_SIZE_MB=500 \
CACHE_SINGLE_ENTRY_MAX_MB=25 \
MAX_SOURCE_FETCH_MB=25 \
RATE_LIMIT_PER_MINUTE=100000 \
RATE_LIMIT_MISS_PER_MINUTE=10000 \
LOG_LEVEL=warn \
ATIHSRAH_DEBUG_HEADERS=true \
"${ROOT_DIR}/target/release/atihsrah" > /tmp/atihsrah_bench_native.log 2>&1 &
AT_NATIVE_PID=$!

# 6. Ensure Thumbor venv exists and launch Native Thumbor on port 8888
if [ ! -f "/tmp/thumbor_venv/bin/thumbor" ]; then
    echo "[*] Setting up Thumbor in /tmp/thumbor_venv..."
    uv venv /tmp/thumbor_venv
    uv pip install thumbor --python /tmp/thumbor_venv
fi

echo "[*] Launching Native Thumbor on port 8888..."
SSL_CERT_FILE="${ROOT_DIR}/bench/certs/ca.crt" \
/tmp/thumbor_venv/bin/thumbor -p 8888 --use-environment True > /tmp/thumbor_bench.log 2>&1 &
THUMBOR_PID=$!

# Cleanup hook on exit
cleanup() {
    echo ""
    echo "[*] Cleaning up benchmark services..."
    kill "$AT_NATIVE_PID" 2>/dev/null || true
    kill "$THUMBOR_PID" 2>/dev/null || true
    docker compose -f "${COMPOSE_FILE}" down
    echo "[+] Cleanup complete."
}
trap cleanup EXIT INT TERM

# 7. Wait for services to be ready
echo "[*] Polling service health endpoints..."
max_retries=30

retries=0
until curl -k -s -f "https://localhost:8000/stats" > /dev/null 2>&1; do
    retries=$((retries + 1))
    if [ $retries -ge $max_retries ]; then
        echo "[-] Timeout waiting for mock origin server."
        docker compose -f "${COMPOSE_FILE}" logs origin
        exit 1
    fi
    sleep 1
done

retries=0
until curl -s -f "http://localhost:8080/ready" > /dev/null 2>&1; do
    retries=$((retries + 1))
    if [ $retries -ge $max_retries ]; then
        echo "[-] Timeout waiting for Native Atihsrah."
        cat /tmp/atihsrah_bench_native.log
        exit 1
    fi
    sleep 1
done

retries=0
until curl -s -f "http://localhost:8084/ready" > /dev/null 2>&1; do
    retries=$((retries + 1))
    if [ $retries -ge $max_retries ]; then
        echo "[-] Timeout waiting for Containerized Atihsrah."
        docker compose -f "${COMPOSE_FILE}" logs atihsrah
        exit 1
    fi
    sleep 1
done

retries=0
until curl -s -f "http://localhost:8081/health" > /dev/null 2>&1; do
    retries=$((retries + 1))
    if [ $retries -ge $max_retries ]; then
        echo "[-] Timeout waiting for imgproxy."
        docker compose -f "${COMPOSE_FILE}" logs imgproxy
        exit 1
    fi
    sleep 1
done

retries=0
until curl -s -f "http://localhost:8082/health" > /dev/null 2>&1; do
    retries=$((retries + 1))
    if [ $retries -ge $max_retries ]; then
        echo "[-] Timeout waiting for imgproxy_cached (nginx)."
        docker compose -f "${COMPOSE_FILE}" logs imgproxy_cached
        exit 1
    fi
    sleep 1
done

retries=0
until curl -s -f "http://localhost:8888/healthcheck" > /dev/null 2>&1; do
    retries=$((retries + 1))
    if [ $retries -ge $max_retries ]; then
        echo "[-] Timeout waiting for Thumbor."
        cat /tmp/thumbor_bench.log
        exit 1
    fi
    sleep 1
done

echo "[+] All benchmark services healthy and ready."

# 8. Execute Python Benchmark Suite
echo ""
echo "[*] Executing benchmark suite scenarios..."
uv run --with pillow --with numpy python3 "${SCRIPT_DIR}/benchmark_suite.py" \
    --host-atihsrah "http://localhost:8080" \
    --host-atihsrah-docker "http://localhost:8084" \
    --host-imgproxy "http://localhost:8081" \
    --host-imgproxy-cached "http://localhost:8082" \
    --host-thumbor "http://localhost:8888" \
    --origin-host "https://localhost:8000" \
    --origin-container "https://origin:8000" \
    --atihsrah-native-pid "$AT_NATIVE_PID" \
    --thumbor-pid "$THUMBOR_PID" \
    "$@"

echo ""
echo "========================================================================"
echo "[+] All benchmark scenarios completed successfully."
echo "[+] Benchmark outputs available at:"
echo "    - ${BENCHMARKS_DIR}/raw_results.json"
echo "    - ${BENCHMARKS_DIR}/results.csv"
echo "    - ${BENCHMARKS_DIR}/system_info.json"
echo "    - ${BENCHMARKS_DIR}/methodology.md"
echo "    - ${BENCHMARKS_DIR}/analysis.md"
echo "========================================================================"
