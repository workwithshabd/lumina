#!/usr/bin/env bash
set -euo pipefail

# Downloads the 20 real benchmark images and verifies their SHA256 checksums.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
MANIFEST="${SCRIPT_DIR}/corpus_manifest.csv"
CORPUS_DIR="${ROOT_DIR}/benchmarks/corpus"
BENCH_CORPUS_DIR="${SCRIPT_DIR}/corpus"

mkdir -p "${CORPUS_DIR}"
mkdir -p "${BENCH_CORPUS_DIR}"

if [ ! -f "${MANIFEST}" ]; then
    echo "ERROR: Manifest not found at ${MANIFEST}" >&2
    exit 1
fi

echo "========================================================================"
echo "          Atihsrah Benchmark: Downloading 20 Real Images                "
echo "========================================================================"

# Skip header row and parse CSV
tail -n +2 "${MANIFEST}" | while IFS=',' read -r id filename cat dims url lic notes sha; do
    # Remove quotes around notes and sha
    notes=$(echo "${notes}" | tr -d '"')
    sha=$(echo "${sha}" | tr -d '"')
    dest="${CORPUS_DIR}/${filename}"

    need_download=1
    if [ -f "${dest}" ]; then
        if command -v sha256sum >/dev/null 2>&1; then
            current_sha=$(sha256sum "${dest}" | awk '{print $1}')
        else
            current_sha=$(shasum -a 256 "${dest}" | awk '{print $1}')
        fi
        if [ "${current_sha}" = "${sha}" ]; then
            echo "[OK] ${filename} exists and SHA256 matches (${sha:0:12}...)"
            need_download=0
        fi
    fi

    if [ "${need_download}" -eq 1 ]; then
        echo "[*] Downloading ${filename} from ${url}..."
        curl -sSL -H "User-Agent: Mozilla/5.0" -o "${dest}" "${url}"

        if command -v sha256sum >/dev/null 2>&1; then
            downloaded_sha=$(sha256sum "${dest}" | awk '{print $1}')
        else
            downloaded_sha=$(shasum -a 256 "${dest}" | awk '{print $1}')
        fi

        if [ "${downloaded_sha}" != "${sha}" ]; then
            echo "[-] ERROR: Checksum mismatch for ${filename}!" >&2
            echo "    Expected: ${sha}" >&2
            echo "    Actual:   ${downloaded_sha}" >&2
            exit 1
        fi
        echo "[+] Verified ${filename} (${sha:0:12}...)"
    fi

    # Mirror into bench/corpus
    cp "${dest}" "${BENCH_CORPUS_DIR}/${filename}"
done

echo "========================================================================"
echo "[+] All 20 real benchmark images verified successfully."
echo "========================================================================"

