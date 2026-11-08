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

python3 - <<EOF
import csv
import hashlib
import os
import sys
import urllib.request

manifest_path = "${MANIFEST}"
corpus_dir = "${CORPUS_DIR}"
bench_dir = "${BENCH_CORPUS_DIR}"

os.makedirs(corpus_dir, exist_ok=True)
os.makedirs(bench_dir, exist_ok=True)

with open(manifest_path, 'r', encoding='utf-8') as f:
    reader = csv.DictReader(f)
    for row in reader:
        filename = row['filename'].strip()
        url = row['source_url'].strip()
        expected_sha = row['sha256'].strip()
        dest = os.path.join(corpus_dir, filename)
        bench_dest = os.path.join(bench_dir, filename)

        need_download = True
        if os.path.exists(dest):
            with open(dest, 'rb') as img_f:
                current_sha = hashlib.sha256(img_f.read()).hexdigest()
            if current_sha.lower() == expected_sha.lower():
                print(f"[OK] {filename} exists and SHA256 matches ({expected_sha[:12]}...)")
                need_download = False

        if need_download:
            print(f"[*] Downloading {filename} from {url}...")
            req = urllib.request.Request(
                url,
                headers={"User-Agent": "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AtihsrahBench/1.0"}
            )
            with urllib.request.urlopen(req) as resp, open(dest, 'wb') as out_f:
                out_f.write(resp.read())

            with open(dest, 'rb') as img_f:
                downloaded_sha = hashlib.sha256(img_f.read()).hexdigest()

            if downloaded_sha.lower() != expected_sha.lower():
                print(f"[-] ERROR: Checksum mismatch for {filename}!", file=sys.stderr)
                print(f"    Expected: {expected_sha}", file=sys.stderr)
                print(f"    Actual:   {downloaded_sha}", file=sys.stderr)
                sys.exit(1)
            print(f"[+] Verified {filename} ({expected_sha[:12]}...)")

        # Mirror to bench/corpus
        with open(dest, 'rb') as src_f, open(bench_dest, 'wb') as dst_f:
            dst_f.write(src_f.read())
EOF

echo "========================================================================"
echo "[+] All 20 real benchmark images verified successfully."
echo "========================================================================"

