#!/usr/bin/env bash
set -euo pipefail

echo "==> Running Documentation Quality and Ground Truth Guardrail Check..."

ERRORS=0

# 1. Check for forbidden phrases across all documentation files (*.md)
FORBIDDEN_PATTERNS=("FIPS" "100% safe" "0 CVEs" "Auditor:")

for pattern in "${FORBIDDEN_PATTERNS[@]}"; do
    MATCHES=$(git grep -in "$pattern" -- '*.md' || true)
    if [ -n "$MATCHES" ]; then
        echo "ERROR: Found forbidden pattern '$pattern' in documentation:"
        echo "$MATCHES"
        ERRORS=$((ERRORS + 1))
    fi
done

# 2. Check for absolute file:/// links in documentation (relative links required)
FILE_MATCHES=$(git grep -in "file:///" -- '*.md' || true)
if [ -n "$FILE_MATCHES" ]; then
    echo "ERROR: Found absolute 'file:///' link in documentation (use relative links):"
    echo "$FILE_MATCHES"
    ERRORS=$((ERRORS + 1))
fi

# 3. Verify benchmarks/analysis.md does not contain unverified raw benchmark numbers
if [ -f "benchmarks/analysis.md" ]; then
    UNVERIFIED_ROWS=$(grep -E '^\|[ ]*(\*\*|img_)' benchmarks/analysis.md | grep -E '[0-9]+(\.[0-9]+)?[ ]*(ms|req/s|%|MB|KB)' | grep -v 'TODO' || true)
    if [ -n "$UNVERIFIED_ROWS" ]; then
        echo "ERROR: Found unverified raw benchmark numbers in benchmarks/analysis.md without TODO(measured):"
        echo "$UNVERIFIED_ROWS"
        ERRORS=$((ERRORS + 1))
    fi
else
    echo "ERROR: benchmarks/analysis.md not found!"
    ERRORS=$((ERRORS + 1))
fi

if [ "$ERRORS" -gt 0 ]; then
    echo "==> Documentation guardrail check FAILED with $ERRORS error(s)."
    exit 1
fi

echo "==> Documentation guardrail check PASSED: All docs align with empirical ground truth."

