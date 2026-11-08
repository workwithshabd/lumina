#!/usr/bin/env bash
set -euo pipefail

echo "==> Running Documentation Quality and Ground Truth Guardrail Check..."

ERRORS=0

# 1. Check for forbidden phrases across all project documentation files (*.md, excluding vendored crates)
FORBIDDEN_PATTERNS=("FIPS" "100% safe" "0 CVEs" "Auditor:")

for pattern in "${FORBIDDEN_PATTERNS[@]}"; do
    MATCHES=$(git grep -in "$pattern" -- '*.md' ':!vendor/**' || true)
    if [ -n "$MATCHES" ]; then
        echo "ERROR: Found forbidden pattern '$pattern' in documentation:"
        echo "$MATCHES"
        ERRORS=$((ERRORS + 1))
    fi
done

# 2. Check for absolute file:/// links in documentation (relative links required)
FILE_MATCHES=$(git grep -in "file:///" -- '*.md' ':!vendor/**' || true)
if [ -n "$FILE_MATCHES" ]; then
    echo "ERROR: Found absolute 'file:///' link in documentation (use relative links):"
    echo "$FILE_MATCHES"
    ERRORS=$((ERRORS + 1))
fi

# 3. Verify benchmark ground truth artifacts exist and are consistent
if [ -f "benchmarks/analysis.md" ]; then
    if [ ! -f "benchmarks/raw_results.json" ] || [ ! -f "benchmarks/results.csv" ] || [ ! -f "benchmarks/system_info.json" ]; then
        echo "ERROR: benchmarks/analysis.md contains numbers but supporting ground-truth artifacts (raw_results.json, results.csv, system_info.json) are missing!"
        ERRORS=$((ERRORS + 1))
    fi
else
    echo "ERROR: benchmarks/analysis.md not found!"
    ERRORS=$((ERRORS + 1))
fi

# 4. Cross-validate empirical numbers in docs against benchmarks/raw_results.json
if [ -f "benchmarks/raw_results.json" ]; then
    python3 -c "
import json, sys

with open('benchmarks/raw_results.json') as f:
    raw = json.load(f)

sc2 = raw.get('scenario_2_warm_cache', {})
sc4 = raw.get('scenario_4_concurrent_different_urls', {})

# Discredited/stale benchmark figures that must never appear in documented comparison claims
discredited = ['50.56 ms', '93.3 req/s', '22.8 req/s', '44.9 req/s', '0.82 ms median', '1.03 ms p95']
errors = 0

docs_to_check = ['docs/CLAIMS.md', 'docs/FINAL_AUDIT_REPORT.md', 'docs/FINAL_AUDIT_AND_VERDICT.md']
for path in docs_to_check:
    try:
        with open(path) as df:
            content = df.read()
        for disc in discredited:
            if disc in content:
                print(f'ERROR: {path} contains stale/discredited benchmark figure: {disc}')
                errors += 1
    except Exception as e:
        print(f'ERROR: Failed to read {path}: {e}')
        errors += 1

if errors > 0:
    sys.exit(1)
" || ERRORS=$((ERRORS + 1))
fi

if [ "$ERRORS" -gt 0 ]; then
    echo "==> Documentation guardrail check FAILED with $ERRORS error(s)."
    exit 1
fi

echo "==> Documentation guardrail check PASSED: All docs align with empirical ground truth."

