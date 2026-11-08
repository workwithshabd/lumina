#!/usr/bin/env python3
"""
Atihsrah Reference URL Signer (Python 3)

Implements strict RFC 3986 canonical query parameter encoding and HMAC-SHA256
signature generation for the Atihsrah image proxy.
Standard library only: requires hmac, hashlib, urllib.parse, math.
"""

import hashlib
import hmac
import math
import time
import urllib.parse
from typing import Any, Dict, Optional

MAX_SIGNATURE_TTL_SECS: int = 86400  # 24 hours


def rfc3986_quote(value: Any) -> str:
    """
    Strict RFC 3986 percent-encoding.
    Only unreserved characters [A-Za-z0-9-_.~] are left unescaped.
    Characters such as '!', '(', ')', '*', '\'', and '+' are explicitly percent-encoded.
    """
    return urllib.parse.quote(str(value), safe="~")


def compute_expiry_bucket(now_secs: int, min_ttl_secs: int, bucket_secs: int) -> int:
    """
    Computes an expiration timestamp snapped to a discrete time bucket.
    This guarantees that repeated requests for the same image within a time window
    yield identical signed URLs, enabling upstream CDN and browser cache hits.

    Raises ValueError if bucket_secs + min_ttl_secs > MAX_SIGNATURE_TTL_SECS.
    """
    if bucket_secs <= 0 or min_ttl_secs <= 0:
        raise ValueError("min_ttl_secs and bucket_secs must be positive integers")
    if bucket_secs + min_ttl_secs > MAX_SIGNATURE_TTL_SECS:
        raise ValueError(
            f"bucket_secs ({bucket_secs}) + min_ttl_secs ({min_ttl_secs}) exceeds "
            f"MAX_SIGNATURE_TTL_SECS ({MAX_SIGNATURE_TTL_SECS})"
        )
    return math.ceil((now_secs + min_ttl_secs) / bucket_secs) * bucket_secs


def build_canonical_string(
    exp: int,
    params: Dict[str, Any],
    kid: Optional[str] = None,
) -> str:
    """
    Constructs the canonical string for HMAC-SHA256 signature generation.

    Canonical string format:
    GET\\n
    /transform\\n
    {exp}\\n
    {param1_key}={param1_val}\\n
    ...
    {paramN_key}={paramN_val}

    Rules:
    - Sorted alphabetically by key (ASCII byte ordering).
    - 'sig' is excluded.
    - 'exp' is included in the sorted parameter list.
    - Each value is strictly RFC 3986 percent-encoded.
    """
    q = {k: str(v) for k, v in params.items() if k != "sig"}
    q["exp"] = str(exp)
    if kid is not None:
        q["kid"] = str(kid)

    sorted_items = sorted(q.items(), key=lambda kv: kv[0])

    lines = ["GET", "/transform", str(exp)]
    for k, v in sorted_items:
        lines.append(f"{k}={rfc3986_quote(v)}")

    return "\n".join(lines)


def compute_signature(canonical: str, key_hex: str) -> str:
    """
    Computes the HMAC-SHA256 hex digest for a canonical string using a hex key.
    """
    key_bytes = bytes.fromhex(key_hex)
    if len(key_bytes) < 32:
        raise ValueError("Signing key must be at least 32 bytes (64 hex characters)")

    return hmac.new(
        key_bytes,
        canonical.encode("utf-8"),
        hashlib.sha256,
    ).hexdigest()


def sign_atihsrah_url(
    base_url: str,
    params: Dict[str, Any],
    key_hex: str,
    kid: Optional[str] = None,
    exp: Optional[int] = None,
    min_ttl_secs: int = 300,
    bucket_secs: int = 3600,
    now_secs: Optional[int] = None,
) -> str:
    """
    Signs an Atihsrah transformation URL.

    NOTE: Every parameter sent to Atihsrah MUST be included in the signature.
    Appending query parameters (e.g. &w=400) after signing will invalidate
    the signature and return HTTP 401 Unauthorized.
    """
    if exp is None:
        now = int(time.time()) if now_secs is None else now_secs
        exp = compute_expiry_bucket(now, min_ttl_secs, bucket_secs)

    canonical = build_canonical_string(exp, params, kid=kid)
    sig = compute_signature(canonical, key_hex)

    q = {k: str(v) for k, v in params.items() if k != "sig"}
    q["exp"] = str(exp)
    if kid is not None:
        q["kid"] = str(kid)
    q["sig"] = sig

    query_parts = [f"{rfc3986_quote(k)}={rfc3986_quote(v)}" for k, v in q.items()]
    base = base_url.rstrip("/")
    return f"{base}/transform?{'&'.join(query_parts)}"


if __name__ == "__main__":
    # TEST VECTOR, NEVER USE: documented test vector from docs/signing-vectors.json
    test_key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
    url = sign_atihsrah_url(
        "https://proxy.example.com",
        {"src": "https://images.example.com/flower.jpg", "w": 300},
        key_hex=test_key,
        exp=1772540000,
    )
    print("Example signed URL:", url)

