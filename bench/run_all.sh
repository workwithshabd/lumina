#!/usr/bin/env bash
set -euo pipefail

# Atihsrah vs imgproxy Master Benchmark Orchestrator
# Executes full reproducible benchmark pipeline and generates artifacts in benchmarks/

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
COMPOSE_FILE="${SCRIPT_DIR}/docker-compose.bench.yml"
BENCHMARKS_DIR="${ROOT_DIR}/benchmarks"

cd "${ROOT_DIR}"

echo "========================================================================"
echo "           Atihsrah vs imgproxy Reproducible Benchmark Pipeline         "
echo "========================================================================"

# 1. Verify Prerequisites
if ! command -v docker &> /dev/null; then
    echo "[-] Error: docker is not installed or not in PATH."
    exit 1
fi

if ! command -v python3 &> /dev/null; then
    echo "[-] Error: python3 is not installed or not in PATH."
    exit 1
fi

# 2. Ensure test corpus exists and verified
echo "[*] Verifying 20 real benchmark images..."
"${SCRIPT_DIR}/download_corpus.sh"

# 3. Spin up benchmark docker compose stack
echo "[*] Launching benchmark stack (origin, atihsrah, imgproxy, imgproxy_cached)..."
docker compose -f "${COMPOSE_FILE}" up -d

# Cleanup hook on exit
cleanup() {
    echo ""
    echo "[*] Cleaning up benchmark stack..."
    docker compose -f "${COMPOSE_FILE}" down
    echo "[+] Stack shutdown complete."
}
trap cleanup EXIT INT TERM

# 4. Wait for services to be ready
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
        echo "[-] Timeout waiting for Atihsrah."
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

echo "[+] All benchmark services healthy and ready."

# 5. Execute Python Benchmark Suite
echo ""
echo "[*] Executing benchmark suite scenarios..."
python3 "${SCRIPT_DIR}/benchmark_suite.py" "$@"

echo ""
echo "========================================================================"
echo "[+] All benchmark scenarios completed successfully."
echo "[+] Benchmark outputs available at:"
echo "    - ${BENCHMARKS_DIR}/raw_results.json"
echo "    - ${BENCHMARKS_DIR}/results.csv"
echo "    - ${BENCHMARKS_DIR}/methodology.md"
echo "========================================================================"
