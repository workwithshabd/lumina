# Atihsrah URL Signing Specification & Verification Guide

This document defines the canonical string specification, security model, replay considerations, key rotation protocols, and multi-language client reference implementations for generating signed URLs for Atihsrah image proxy endpoints (`GET /transform`).

---

## 1. Security Architecture & Threat Model

When running with `ATIHSRAH_AUTH_MODE=signed`, Atihsrah requires every transformation request to contain a cryptographic HMAC-SHA256 signature (`sig`) and an expiration timestamp (`exp`).

### Cryptographic Foundation
- **Algorithm**: `HMAC-SHA256` (RFC 2104).
- **Key Material**: Raw symmetric signing key (minimum 32 bytes / 256 bits, encoded as a 64-character hexadecimal string).
  > **Note on Key Handling**: The server holds the raw cryptographic secret directly in memory. **Do NOT hash the signing key with password-hashing algorithms** such as Argon2, bcrypt, scrypt, or PBKDF2. Password hashing algorithms are designed for human-chosen password derivation, not for HMAC authentication keys.
- **Verification**: Signature equality comparison uses constant-time byte comparison (`subtle::ConstantTimeEq`) to prevent timing side-channel attacks.

### Replay Attack Limitations
> **IMPORTANT SECURITY NOTICE**:
> The `exp` (expiration) parameter **LIMITS** the replay window; it does **NOT** prevent replay within that window.
>
> Atihsrah is strictly stateless and has no external database dependency (no Redis, Memcached, or distributed locks). Consequently, **no nonce or JTI replay-cache is implemented**. Any valid signed URL can be retrieved and served multiple times prior to timestamp expiration (which is normally desired for CDN and browser caching).
>
> **Best Practice**: Keep `exp` short-lived (e.g. 5 to 60 minutes) for dynamically generated client links, or set longer expiration (up to `MAX_SIGNATURE_TTL_SECS`, default 86400s) only for immutable assets cached upstream.

### CDN Edge Caching vs. Signature Expiration
> [!IMPORTANT]
> **Once an upstream edge CDN caches a response, signature expiration no longer restricts access to that cached object until the edge cache TTL elapses.**
>
> In signed mode, Atihsrah clamps the `Cache-Control` header `max-age` to the remaining signature validity:
> `Cache-Control: public, max-age={remaining_ttl}` (capped at 31,536,000s if `v` is present, or 86,400s without `v`).
> Note: The `immutable` directive is NOT emitted when `exp` is present, because the response's authorization is inherently time-bounded. (Only in public mode with `v` and no `exp` does Atihsrah emit `immutable`).
> Edge CDNs (e.g. Cloudflare, CloudFront, Fastly) store this response and serve subsequent identical requests directly from edge cache points of presence without reaching Atihsrah. Therefore, Atihsrah's server-side timestamp validation (`exp`) is bypassed once an edge cache hit occurs.
> If access revocation or strict time-gating is required, keep expiration short, or trigger explicit edge cache invalidations.

### CDN Configuration: Mandatory `Vary: Accept` Support
When `format=auto` is requested, Atihsrah dynamically negotiates output formats (`AVIF`, `WebP`, `JPEG`, `PNG`) matching the client's `Accept` HTTP header and emits `Vary: Accept`.

> [!WARNING]
> **Major CDNs DO NOT vary cache keys on the `Accept` header by default.**
> If your CDN does not include `Accept` in its cache key, an AVIF image cached for a modern browser will be returned to incompatible legacy clients, causing rendering errors.
>
> You must explicitly configure your CDN cache key policy (unverified: check the vendor's current docs):
> - **Cloudflare** *(unverified: check the vendor's current docs)*: Cloudflare ignores the `Accept` header in standard cache keys. You must use [Cloudflare Custom Cache Keys](https://developers.cloudflare.com/cache/how-to/cache-keys/) (Enterprise) with `http.request.headers["accept"]` or deploy a Cloudflare Worker to normalize the `Accept` header into distinct cache key segments.
> - **AWS CloudFront** *(unverified: check the vendor's current docs)*: The default `Managed-CachingOptimized` policy ignores `Accept`. You must create a custom [CloudFront Cache Key Policy](https://docs.aws.amazon.com/AmazonCloudFront/latest/DeveloperGuide/controlling-the-cache-key.html) under "Cache Key and Origin Requests" and add `Accept` to the "Headers" cache key list.
> - **Fastly** *(unverified: check the vendor's current docs)*: Fastly supports `Vary` via Varnish, but wild variation in browser `Accept` strings causes excessive cache fragmentation. Fastly strongly recommends normalizing the header in `vcl_recv` before caching. See [Fastly Vary Header Documentation](https://docs.fastly.com/en/guides/understanding-the-vary-header).

### Recommended Client Pattern: Explicit Formats with `<picture>` Tag
Because many CDNs ignore the `Accept` header by default unless customized, the most reliable and portable way to serve modern formats without relying on `Vary: Accept` edge negotiation is the HTML `<picture>` element with explicit `format=` URLs:

```html
<picture>
  <!-- Modern clients supporting AVIF -->
  <source type="image/avif" srcset="https://proxy.example.com/transform?src=https%3A%2F%2Fimages.example.com%2Fphoto.jpg&w=800&format=avif&exp=1772543600&sig=...">
  <!-- Modern clients supporting WebP -->
  <source type="image/webp" srcset="https://proxy.example.com/transform?src=https%3A%2F%2Fimages.example.com%2Fphoto.jpg&w=800&format=webp&exp=1772543600&sig=...">
  <!-- Universal fallback JPEG -->
  <img src="https://proxy.example.com/transform?src=https%3A%2F%2Fimages.example.com%2Fphoto.jpg&w=800&format=jpeg&exp=1772543600&sig=..." alt="Photo" width="800" loading="lazy">
</picture>
```
This guarantees deterministic cache keys on every CDN, allows browsers to negotiate formats locally, and eliminates `Vary: Accept` cache partitioning.

---

## 2. Canonical String Specification

To guarantee consistent signature verification across heterogeneous client environments and programming languages, the query string must be canonicalized into a deterministic plaintext string before computing the HMAC digest.

### Canonical String Structure

```text
GET\n
/transform\n
{exp}\n
{param1_key}={param1_encoded_val}\n
{param2_key}={param2_encoded_val}\n
...
{paramN_key}={paramN_encoded_val}
```

### Formatting Rules

1. **HTTP Method**: Must strictly be uppercase `GET`, followed by a newline (`\n`).
2. **Endpoint Path**: Must strictly be `/transform`, followed by a newline (`\n`).
3. **Expiration Line**: The decimal string representation of the expiration timestamp `{exp}` in integer seconds since Unix Epoch, followed by a newline (`\n`).
4. **Parameter Exclusion**: The `sig` parameter itself is **strictly excluded** from the canonical parameter list.
5. **Parameter Inclusion**: All other query parameters passed in the request (including `exp`, `src` / `url`, `w`, `h`, `fit`, `format`, `quality_target`, `v`, `kid`, `allow_upscale`) **MUST** be included.
6. **Alphabetical Sorting**: Parameters are sorted alphabetically in ascending ASCII / lexicographical order by parameter key name (`a.cmp(b)`).
7. **RFC 3986 Percent-Encoding**: Each parameter value must be percent-encoded according to strict **RFC 3986** rules:
   - **Unreserved Characters** (`A-Z`, `a-z`, `0-9`, `-`, `_`, `.`, `~`) must **NEVER** be encoded.
   - All other characters (including `/`, `:`, `?`, `=`, `&`, `+`, spaces, etc.) must be encoded as `%XX` using uppercase hexadecimal digits.
8. **Delimiter**: Each canonical parameter pair is serialized as `key=percent_encoded(value)`, and pairs are joined with a single newline (`\n`).

---

## 3. Worked Example

### Inputs
- **Secret Key (Hex)** *(TEST VECTOR, NEVER USE)*:
  `0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef`
- **Request Endpoint**:
  `GET /transform`
- **Parameters**:
  - `src`: `https://images.example.com/flower.jpg`
  - `w`: `300`
  - `exp`: `1772540000`

### Step 1: Collect and Sort Query Parameters (Excluding `sig`)
Sorted alphabetically by key:
1. `exp`: `1772540000`
2. `src`: `https://images.example.com/flower.jpg` (RFC 3986 encoded: `https%3A%2F%2Fimages.example.com%2Fflower.jpg`)
3. `w`: `300`

### Step 2: Assemble Canonical String
```text
GET
/transform
1772540000
exp=1772540000
src=https%3A%2F%2Fimages.example.com%2Fflower.jpg
w=300
```

Raw bytes representation (newlines shown as `\n`):
```text
GET\n/transform\n1772540000\nexp=1772540000\nsrc=https%3A%2F%2Fimages.example.com%2Fflower.jpg\nw=300
```

### Step 3: Compute HMAC-SHA256
Feed the UTF-8 bytes of the canonical string to HMAC-SHA256 using the raw binary key:
- **Resulting Signature (`sig`)**:
  `d1ded29494fb8104468060d937eff6071508795ab923db667fe49b0cede896fd`

### Step 4: Final Request URL
```text
https://proxy.example.com/transform?src=https%3A%2F%2Fimages.example.com%2Fflower.jpg&w=300&exp=1772540000&sig=d1ded29494fb8104468060d937eff6071508795ab923db667fe49b0cede896fd
```

---

## 4. Reference Implementations in 3 Languages

### Python 3

Reference implementation located at [`examples/python/signer.py`](../examples/python/signer.py).
Requires standard library only (`hmac`, `hashlib`, `urllib.parse`, `math`).

```python
import hashlib
import hmac
import math
import urllib.parse
from typing import Any, Dict, Optional

MAX_SIGNATURE_TTL_SECS = 86400  # 24 hours


def rfc3986_quote(value: Any) -> str:
    """Strict RFC 3986 URL percent-encoding (preserves only [A-Za-z0-9-_.~])."""
    return urllib.parse.quote(str(value), safe="~")


def compute_expiry_bucket(now_secs: int, min_ttl_secs: int, bucket_secs: int) -> int:
    """Snaps expiration to a discrete bucket window for CDN cache reuse."""
    if bucket_secs <= 0 or min_ttl_secs <= 0:
        raise ValueError("min_ttl_secs and bucket_secs must be positive integers")
    if bucket_secs + min_ttl_secs > MAX_SIGNATURE_TTL_SECS:
        raise ValueError("bucket_secs + min_ttl_secs exceeds MAX_SIGNATURE_TTL_SECS")
    return math.ceil((now_secs + min_ttl_secs) / bucket_secs) * bucket_secs


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
    """Signs an Atihsrah transform URL using HMAC-SHA256."""
    key_bytes = bytes.fromhex(key_hex)
    if len(key_bytes) < 32:
        raise ValueError("Signing key must be at least 32 bytes (64 hex characters)")

    if exp is None:
        import time
        now = int(time.time()) if now_secs is None else now_secs
        exp = compute_expiry_bucket(now, min_ttl_secs, bucket_secs)

    q = {k: str(v) for k, v in params.items() if k != "sig"}
    q["exp"] = str(exp)
    if kid is not None:
        q["kid"] = str(kid)

    sorted_items = sorted(q.items(), key=lambda kv: kv[0])
    lines = ["GET", "/transform", str(exp)]
    for k, v in sorted_items:
        lines.append(f"{k}={rfc3986_quote(v)}")
    canonical = "\n".join(lines)

    sig = hmac.new(key_bytes, canonical.encode("utf-8"), hashlib.sha256).hexdigest()
    q["sig"] = sig

    query_parts = [f"{rfc3986_quote(k)}={rfc3986_quote(v)}" for k, v in q.items()]
    base = base_url.rstrip("/")
    return f"{base}/transform?{'&'.join(query_parts)}"
```

---

### JavaScript / Node.js

Reference implementation located at [`examples/node/signer.js`](../examples/node/signer.js).
Requires Node.js built-in `crypto` module.

```javascript
const crypto = require('crypto');

const MAX_SIGNATURE_TTL_SECS = 86400; // 24 hours

/**
 * Strict RFC 3986 percent-encoding matching standard URL specifications.
 */
function rfc3986Encode(val) {
  return encodeURIComponent(String(val)).replace(
    /[!'()*]/g,
    c => '%' + c.charCodeAt(0).toString(16).toUpperCase()
  );
}

/**
 * Snaps expiration to a discrete bucket window for CDN cache reuse.
 */
function computeExpiryBucket(nowSecs, minTtlSecs, bucketSecs) {
  if (bucketSecs <= 0 || minTtlSecs <= 0) {
    throw new Error('minTtlSecs and bucketSecs must be positive integers');
  }
  if (bucketSecs + minTtlSecs > MAX_SIGNATURE_TTL_SECS) {
    throw new Error('bucketSecs + minTtlSecs exceeds MAX_SIGNATURE_TTL_SECS');
  }
  return Math.ceil((nowSecs + minTtlSecs) / bucketSecs) * bucketSecs;
}

/**
 * Generates a signed Atihsrah transform URL.
 */
function signAtihsrahUrl(baseUrl, params, options) {
  const { keyHex, kid = null, minTtlSecs = 300, bucketSecs = 3600 } = options;
  const keyBytes = Buffer.from(keyHex, 'hex');
  if (keyBytes.length < 32) {
    throw new Error('Signing key must be at least 32 bytes (64 hex characters)');
  }

  let exp = options.exp;
  if (exp === undefined || exp === null) {
    const now = options.nowSecs !== undefined ? options.nowSecs : Math.floor(Date.now() / 1000);
    exp = computeExpiryBucket(now, minTtlSecs, bucketSecs);
  }

  const qParams = { ...params, exp: String(exp) };
  if (kid) {
    qParams.kid = String(kid);
  }
  delete qParams.sig;

  // 1. Sort parameters alphabetically by key
  const sortedKeys = Object.keys(qParams).sort();

  // 2. Build canonical string
  const canonicalParts = [
    'GET',
    '/transform',
    String(exp),
  ];
  for (const k of sortedKeys) {
    canonicalParts.push(`${k}=${rfc3986Encode(qParams[k])}`);
  }
  const canonicalString = canonicalParts.join('\n');

  // 3. Compute HMAC-SHA256
  const sig = crypto
    .createHmac('sha256', keyBytes)
    .update(canonicalString, 'utf8')
    .digest('hex');

  // 4. Construct URL
  qParams.sig = sig;
  const queryString = Object.entries(qParams)
    .map(([k, v]) => `${rfc3986Encode(k)}=${rfc3986Encode(v)}`)
    .join('&');

  const base = baseUrl.replace(/\/+$/, '');
  return `${base}/transform?${queryString}`;
}
```

---

### Rust

Uses `hmac`, `sha2`, `hex`, and `percent-encoding`.

```rust
use hmac::{Hmac, Mac};
use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use sha2::Sha256;

pub const RFC3986_UNRESERVED: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');

pub fn rfc3986_encode(val: &str) -> String {
    utf8_percent_encode(val, RFC3986_UNRESERVED).to_string()
}

pub fn sign_atihsrah_url(
    base_url: &str,
    mut params: Vec<(String, String)>,
    exp: u64,
    key: &[u8],
    kid: Option<&str>,
) -> String {
    // 1. Prepare parameters
    params.retain(|(k, _)| k != "sig");
    if !params.iter().any(|(k, _)| k == "exp") {
        params.push(("exp".to_string(), exp.to_string()));
    }
    if let Some(id) = kid {
        if !params.iter().any(|(k, _)| k == "kid") {
            params.push(("kid".to_string(), id.to_string()));
        }
    }

    // 2. Sort parameters alphabetically by key
    params.sort_by(|a, b| a.0.cmp(&b.0));

    // 3. Assemble canonical string
    let mut canonical = format!("GET\n/transform\n{exp}\n");
    let joined: Vec<String> = params
        .iter()
        .map(|(k, v)| format!("{}={}", k, rfc3986_encode(v)))
        .collect();
    canonical.push_str(&joined.join("\n"));

    // 4. Compute HMAC-SHA256
    let mut mac = Hmac::<Sha256>::new_from_slice(key)
        .expect("HMAC key length >= 32 bytes");
    mac.update(canonical.as_bytes());
    let sig = hex::encode(mac.finalize().into_bytes());

    // 5. Build final URL
    let mut query_pairs: Vec<String> = params
        .iter()
        .map(|(k, v)| format!("{}={}", k, rfc3986_encode(v)))
        .collect();
    query_pairs.push(format!("sig={sig}"));

    let base = base_url.trim_end_matches('/');
    format!("{base}/transform?{}", query_pairs.join("&"))
}
```

---

## 5. Key Rotation & Multi-Key Configuration

Atihsrah supports zero-downtime key rotation via the `ATIHSRAH_SIGNING_KEYS` environment variable:

```bash
ATIHSRAH_SIGNING_KEYS="key_2026_primary:<generate with: openssl rand -hex 32>,key_2025_retiring:<generate with: openssl rand -hex 32>"
```

### Key Selection Rules:
1. When generating or verifying signed requests with multi-key deployments, clients include the `kid` query parameter (e.g. `kid=key_2026_primary`).
2. If `kid` is provided, Atihsrah looks up that specific key. If unknown, the request is rejected with `401 Unauthorized` (`UNKNOWN_KID`).
3. If only a single key is configured (or `ATIHSRAH_SIGNING_KEY` is used), `kid` is optional and defaults to the single active key.
4. If multiple keys exist and no `kid` is supplied, the request is rejected with `401 Unauthorized`.

---

## 6. Public Auth Mode Risks

Running with `ATIHSRAH_AUTH_MODE=public` disables cryptographic signature verification. When enabled:
- Anyone with network access to the server can request arbitrary transformations of allowlisted origins.
- While origin domain restrictions (`ALLOWED_ORIGINS`), private IP filtering (SSRF), and IP rate limiters remain active, public mode leaves the service exposed to distributed CPU exhaustion and cache churn.
- Atihsrah emits a loud **SECURITY WARNING** in its JSON startup logs whenever `public` mode is initialized.
- Always use `ATIHSRAH_AUTH_MODE=signed` in production deployments.
