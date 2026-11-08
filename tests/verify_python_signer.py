#!/usr/bin/env python3
"""
Verifies that examples/python/signer.py matches all vectors in docs/signing-vectors.json
"""

import json
import os
import sys

sys.path.insert(0, os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "examples", "python")))
from signer import (
    MAX_SIGNATURE_TTL_SECS,
    build_canonical_string,
    compute_expiry_bucket,
    compute_signature,
    rfc3986_quote,
)

vectors_path = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "docs", "signing-vectors.json"))
with open(vectors_path, "r", encoding="utf-8") as f:
    vector_data = json.load(f)

vectors = vector_data["vectors"]
print(f"Verifying Python signer across {len(vectors)} vectors...")

for vec in vectors:
    if vec["type"] == "positive":
        canonical = build_canonical_string(vec["exp"], vec["params"], kid=vec["kid"])
        if canonical != vec["canonical_string"]:
            print(f"FAIL [{vec['id']}]: canonical mismatch", file=sys.stderr)
            print("Expected:\n" + vec["canonical_string"], file=sys.stderr)
            print("Actual:\n" + canonical, file=sys.stderr)
            sys.exit(1)

        sig = compute_signature(canonical, vec["key"])
        if sig != vec["expected_sig"]:
            print(f"FAIL [{vec['id']}]: signature mismatch", file=sys.stderr)
            print(f"Expected: {vec['expected_sig']}, got: {sig}", file=sys.stderr)
            sys.exit(1)

# Verify expiry bucket logic
now = 1700000000
bucket1 = compute_expiry_bucket(now, 300, 3600)
if bucket1 % 3600 != 0:
    print(f"FAIL: bucket alignment error: {bucket1}", file=sys.stderr)
    sys.exit(1)

threw = False
try:
    compute_expiry_bucket(now, 50000, 40000)  # 90000 > 86400
except ValueError:
    threw = True

if not threw:
    print("FAIL: compute_expiry_bucket failed to raise ValueError on exceeding MAX_SIGNATURE_TTL_SECS", file=sys.stderr)
    sys.exit(1)

print("All Python signer verification checks passed!")

