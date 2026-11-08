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

# 4. Cross-validate empirical numbers in docs dynamically against benchmarks/raw_results.json
if [ -f "benchmarks/raw_results.json" ]; then
    python3 -c "
import json, sys

with open('benchmarks/raw_results.json') as f:
    raw = json.load(f)

sc2 = raw.get('scenario_2_warm_cache', {})
sc4 = raw.get('scenario_4_concurrent_different_urls', {})

# Dynamically extract ground-truth metrics from raw results
dynamic_metrics = {
    'Native Warm Cache p50': f\"{sc2['atihsrah_native']['p50_ms']} ms\",
    'Native Warm Cache p95': f\"{sc2['atihsrah_native']['p95_ms']} ms\",
    'imgproxy Direct Warm Cache p50': f\"{sc2['imgproxy_direct']['p50_ms']} ms\",
    'imgproxy Cached Warm Cache p50': f\"{sc2['imgproxy_cached']['p50_ms']} ms\",
    'imgproxy Cold Concurrency Throughput': f\"{sc4['imgproxy']['throughput_rps']} req/s\",
    'Atihsrah Native Throughput': f\"{sc4['atihsrah_native']['throughput_rps']} req/s\",
    'Atihsrah Docker Throughput': f\"{sc4['atihsrah_docker']['throughput_rps']} req/s\",
}

# Stale/discredited figures that must never appear in documentation
discredited = ['50.56 ms', '93.3 req/s', '22.8 req/s', '44.9 req/s', '0.82 ms median', '1.03 ms p95', '0.83 ms p50', '1.85 ms vs 0.82 ms']
errors = 0

docs_to_check = ['docs/CLAIMS.md', 'docs/FINAL_AUDIT_REPORT.md', 'docs/FINAL_AUDIT_AND_VERDICT.md']
for path in docs_to_check:
    try:
        with open(path) as df:
            content = df.read()
        
        # 1. Assert no stale/discredited figures
        for disc in discredited:
            if disc in content:
                print(f'ERROR: {path} contains stale/discredited benchmark figure: {disc}')
                errors += 1
                
        # 2. Dynamically verify presence of exact raw data metrics
        for label, metric_val in dynamic_metrics.items():
            if metric_val not in content:
                print(f'ERROR: {path} is missing expected raw benchmark metric {label}: {metric_val}')
                errors += 1
    except Exception as e:
        print(f'ERROR: Failed to validate {path}: {e}')
        errors += 1

if errors > 0:
    print(f'==> Dynamic documentation check FAILED with {errors} mismatch(es).')
    sys.exit(1)
print('==> Dynamic documentation check PASSED: all documented metrics match raw_results.json.')
" || ERRORS=$((ERRORS + 1))
fi

if [ "$ERRORS" -gt 0 ]; then
    echo "==> Documentation guardrail check FAILED with $ERRORS error(s)."
    exit 1
fi

echo "==> Documentation guardrail check PASSED: All docs align with empirical ground truth."

