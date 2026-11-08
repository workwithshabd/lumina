# Atihsrah

[![License: AGPL-3.0](https://img.shields.io/badge/License-AGPL--3.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.85%2B%20Stable-orange.svg)](https://www.rust-lang.org)
[![Security: Threat Modeled](https://img.shields.io/badge/Security-27%20Threat%20Vectors%20Tested-brightgreen.svg)](THREAT_MODEL.md)

**Atihsrah** is an open-source ([AGPL-3.0](LICENSE)), self-hosted, secure image optimization reverse proxy written in Rust.

It fetches images **only** from explicitly allowlisted HTTPS origins, resizes and transforms them on-the-fly (resize, crop, format conversion, quality target), caches results in a byte-bounded in-memory LRU cache, and serves them with minimal overhead.

Because Atihsrah uses pure-Rust codecs by default via `image-rs` and `ravif`, it compiles to a single binary with no external C image libraries (`libvips`, `libwebp`, `libpng`). It packages into a minimal container (`FROM scratch` copying rootfs derived from `gcr.io/distroless/cc-debian12`) with **~13.9 MB** compressed transfer footprint (~44.8 MB unpacked file contents) and **0 HIGH/CRITICAL findings in Trivy container scan** (v0.76.1 on 2026-11-08; scope: 10 Debian base packages, 0 language files; cargo audit: 0 vulnerabilities, 1 unmaintained crate warning for transitive paste).

There is **no `unsafe` in our own code**; third-party codec dependencies and cryptographic primitives contain internal unsafe.

---

## What It Is and Is Not

### What It Is
- **A Single-Binary Reverse Proxy**: Pulls images on-demand from your existing storage or CDN, transforms them, and serves them.
- **SSRF-Hardened by Design**: Strict allowlisted HTTPS origins, multi-stage DNS resolution with rebinding prevention, IPv4-mapped IPv6 rejection, redirect prohibition, and lookalike domain detection.
- **Memory-Safe & Bounded**: Built-in byte-weighted in-memory LRU cache (`moka`), strict pre-decode image dimension/pixel bounds, estimated memory budgeting, single-flight transform deduplication, and CPU-bound work isolated in `tokio::task::spawn_blocking`.
- **Stateless & Self-Contained**: Requires **no databases, no Redis, no background queues, and no cloud SDKs**.
- **Observability-First**: Structured JSON logs (`tracing`) with automatic redaction of URLs, secrets, and auth headers; dedicated Prometheus metrics server on an isolated internal port (`127.0.0.1:9090`); and robust `/health` and `/ready` probes.

### What It Is NOT
- **No Upload Endpoint**: Atihsrah never accepts file uploads and never stores original source images on disk.
- **No Databases**: No MongoDB, no Redis, no PostgreSQL, no SQLite.
- **No Dashboard or Admin UI**: Pure headless proxy configured entirely via environment variables.
- **No Cloud Vendor SDKs or Billing**: No proprietary AWS/GCP/Azure SDKs, no AI image generators, and no billing meters.
- **No In-Band Profiling Headers**: Never leaks raw internal execution latency numbers or timing side-channels in HTTP response headers.

---

## Objective Comparison with imgproxy

Atihsrah and imgproxy represent different architectural design choices in self-hosted image proxy infrastructure:

| Dimension | Atihsrah | imgproxy | Notes / Tradeoffs |
|---|---|---|---|
| **Implementation Language** | Rust (Axum, Tokio) | Go + C (`cgo`) | Atihsrah contains **no `unsafe` in our own code**; imgproxy links C libraries (`libvips`). |
| **Image Processing Engine** | Pure-Rust `image-rs` & `ravif` | `libvips` (C library) | Atihsrah uses pure-Rust codecs without C image library linking. imgproxy achieves higher cold transformation throughput via C assembly SIMD vectorization. |
| **Container Footprint** | **~13.9 MB compressed transfer** (~44.8 MB unpacked file contents) | ~40–85 MB uncompressed (requires glibc, libvips, and shared C libraries) | Atihsrah has 0 HIGH/CRITICAL findings in Trivy container scan (v0.76.1 on 2026-11-08; scope: 10 Debian base packages, 0 language files). |
| **In-Memory Caching** | **Built-in byte-bounded LRU cache** (`moka`) with single-flight coalescing | None built-in (relies on external CDN or proxy) | Atihsrah delivers sub-millisecond warm cache hits standalone (see `bench/run_all.sh` and `benchmarks/analysis.md` for reproducible measurements). When imgproxy is paired with Nginx `proxy_cache`, warm cache latency is comparable. |
| **Cold Concurrency & Throughput** | Lower cold throughput | **Higher cold throughput (imgproxy wins)** | `libvips` multi-threaded C pipeline decodes and resizes cold images faster than pure-Rust `image-rs`. |
| **AVIF Encoding Speed** | Slower (CPU-intensive pure Rust) | **Faster (imgproxy wins)** | imgproxy links C `libheif`/`libaom` with assembly vectorization. Atihsrah requires tuning `ATIHSRAH_AVIF_SPEED=6..8` for high volume. |
| **WebP Encoding Mode** | **Lossless-only WebP** (in this Pure Rust Option A build) | **Lossy & Lossless WebP (imgproxy wins on lossy WebP)** | Atihsrah restricts WebP to graphics and alpha to prevent photo size inflation; imgproxy supports lossy WebP for photos. |
| **SSRF Defense** | **Built-in, non-bypassable**: DNS rebinding prevention, IPv4-mapped IPv6 filters, zero redirects | Built-in private IP filters (`IMGPROXY_ALLOW_LOCAL_ADDRESSES=false`) | Both enforce socket-level local IP blocking. |
| **Request Coalescing** | **Built-in single-flight coalescing** (1 origin fetch per 100 concurrent requests) | Configurable via queuing | Atihsrah deduplicates concurrent identical requests at the memory layer. |

---

## When to Choose imgproxy Instead

Choose **imgproxy** over Atihsrah if:
1. **You require lossy WebP compression for photos**: In this pure-Rust build (`FORMAT_OPTION: A`), Atihsrah only encodes lossless WebP. Photos are automatically routed to AVIF or JPEG under `format=auto`.
2. **You process high volumes of cold AVIF images**: `libvips` / `libheif` / `libaom` C optimizations transcode cold AVIF images significantly faster with lower CPU consumption.
3. **You need advanced graphical filters**: Facial recognition cropping, watermarking overlays, blur/sharpen convolution filters, or animated GIF/video transcoding.
4. **You already operate an external caching layer**: If all traffic is already fronted by an edge CDN or Nginx caching proxy, Atihsrah's built-in in-memory cache is redundant.

---

## When to Choose Atihsrah

Choose **Atihsrah** over imgproxy if:
1. **Self-contained minimal container without C image libraries**: You want a minimal scratch container with pure-Rust image codecs and zero external C image library dependencies (no `libvips`, no `libwebp`).
2. **Built-in in-memory caching**: You want sub-millisecond warm cache hits and thundering-herd coalescing out of the box without maintaining a separate Redis or reverse proxy caching tier.
3. **Memory safety & minimal attack surface**: You require a pure-Rust pipeline that eliminates C memory safety vulnerabilities in image parsing.
4. **Strict SSRF hardening**: You operate in multi-tenant environments where private network isolation and DNS rebinding prevention must be non-bypassable by design.

---

## Quickstart

You can clone the repository, run the automated setup script, and start serving signed images in **under 10 minutes**:

### Prerequisites
- Docker Engine & Docker Compose (or local Rust 1.85+ toolchain if running bare binary)
- OpenSSL (for key generation)
- Curl

### 1. Run Setup Script
```bash
git clone https://github.com/shabd/atihsrah.git
cd atihsrah

# Run the automated setup script:
./setup.sh
```

The script will:
1. Verify Docker and Docker Compose prerequisites.
2. Create `.env` from `.env.example` if absent.
3. Verify that `ALLOWED_ORIGINS` is configured (refusing to start if empty).
4. Automatically generate a 32-byte HMAC-SHA256 signing key via `openssl rand -hex 32`.
5. Build and launch the container stack via `docker compose up -d --build`.
6. Poll `http://127.0.0.1:8080/ready` until healthy.
7. Compute and print a **ready-to-use signed URL**.

### 2. Manual Run via Docker Compose
```bash
# 1. Provision configuration
cp .env.example .env

# 2. Edit .env to set your origin and generate a signing key:
# openssl rand -hex 32
vim .env

# 3. Start the stack
docker compose up -d

# 4. Check status
docker compose ps
curl -i http://localhost:8080/ready
```

### 3. Run Directly on Bare Metal (Single Binary)
```bash
cp .env.example .env
cargo build --release
./target/release/atihsrah
```

---

## Configuration Reference

All configuration is provided via environment variables (or `.env`).

| Variable | Required | Default | Description |
|---|---|---|---|
| `ALLOWED_ORIGINS` | **Yes** | — | Comma-separated HTTPS origins (e.g. `https://images.example.com`). Strict HTTPS only, port 443 only. |
| `ATIHSRAH_BIND` | No | `0.0.0.0:8080` | Public HTTP image proxy listening address. |
| `ATIHSRAH_METRICS_BIND` | No | `0.0.0.0:9090` | Dedicated Prometheus metrics listening address (isolated from public traffic). |
| `ATIHSRAH_AUTH_MODE` | No | `signed` | `signed` (HMAC required) or `public` (unauthenticated, emits startup warning). |
| `ATIHSRAH_SIGNING_KEY` | If signed | — | 32-byte HMAC secret in lowercase hex (64 hex characters). |
| `ATIHSRAH_SIGNING_KEYS` | No | — | Key rotation store: `k1:hexkey1,k2:hexkey2`. Clients specify `kid=k1`. |
| `MAX_SIGNATURE_TTL_SECS` | No | `86400` | Maximum allowed future validity window for signatures (in seconds). |
| `ATIHSRAH_CLOCK_SKEW_SECS` | No | `30` | Clock skew tolerance for timestamp validation (in seconds). |
| `CACHE_MAX_SIZE_MB` | No | `500` | Maximum byte-bounded memory capacity for cached images (in MB). |
| `CACHE_SINGLE_ENTRY_MAX_MB`| No | `10` | Maximum size of an individual cached image (in MB). |
| `CACHE_TTL_SECS` | No | `86400` | In-memory cache time-to-live (in seconds). |
| `SOURCE_FETCH_TIMEOUT_SECS` | No | `15` | Timeout for fetching upstream source images (in seconds). |
| `SOURCE_CONNECT_TIMEOUT_SECS` | No | `5` | Upstream TCP connection timeout (in seconds). |
| `MAX_SOURCE_FETCH_MB` | No | `20` | Maximum download size for upstream images (in MB). |
| `MAX_SOURCE_DIMENSION` | No | `8000` | Maximum allowed source image width or height in pixels (header probe). |
| `MAX_OUTPUT_DIMENSION` | No | `4096` | Maximum allowed output width or height in pixels. |
| `MAX_PROCESSING_MEMORY_MB` | No | `512` | Estimated memory budget limit per image decode/transform (in MB). |
| `MAX_CONCURRENT_TRANSFORMS`| No | CPU cores | Max concurrent transformations running in `spawn_blocking`. |
| `RATE_LIMIT_PER_MINUTE` | No | `1000` | Token bucket rate limit per client IP per minute. |
| `RATE_LIMIT_MISS_PER_MINUTE`| No | `100` | Stricter rate limit per client IP for requests that miss cache. |
| `TRUSTED_PROXY_CIDRS` | No | — | Comma-separated trusted reverse proxy subnets (e.g. `10.0.0.0/8,172.16.0.0/12`). |
| `ATIHSRAH_REQUEST_TIMEOUT_SECS` | No | `30` | Request timeout before returning 504 Gateway Timeout. |
| `ATIHSRAH_GLOBAL_CONCURRENCY_LIMIT` | No | `1024` | Global concurrent connections before shedding load with 503. |
| `ATIHSRAH_SHUTDOWN_TIMEOUT_SECS` | No | `30` | Graceful drain timeout on SIGTERM / SIGINT. |
| `ATIHSRAH_MAX_BODY_BYTES` | No | `65536` | Maximum allowed HTTP request body size (in bytes). |
| `ATIHSRAH_PRIVATE_ORIGINS` | No | — | Comma-separated list of exact origins (`scheme://host[:port]`) permitted to resolve to private addresses. `http://` accepted only if written explicitly in this list. Cloud metadata (`169.254.0.0/16`) and multicast/broadcast remain strictly blocked. |
| `ATIHSRAH_PRIVATE_CA_FILE` | No | — | Path to a PEM-encoded internal root CA certificate file for TLS validation. TLS verification is always enforced. |
| `ATIHSRAH_AVIF_SPEED` | No | `6` | AVIF encoding speed preset (`1` to `10`). `1` is slowest with maximum compression; `10` is fastest. Default is `6`. |
| `ATIHSRAH_DISTINCT_COLOR_THRESHOLD` | No | `256` | Color palette threshold sampled from downscaled 64x64 PNG/WebP to classify photographic vs graphic content. |
| `ATIHSRAH_MAX_RETRY_PIXELS` | No | `16777216` | Maximum pixel count (width × height) eligible for larger-than-source retry attempts. |
| `ATIHSRAH_DEBUG_HEADERS` | No | `false` | When true, includes `X-Cache` in response headers. |
| `LOG_LEVEL` | No | `info` | Structured tracing JSON log filter (`trace`, `debug`, `info`, `warn`, `error`). |

---

### Internal / Private Origins with Custom CA

For internal VPC architectures (e.g., MinIO, internal object storage, Kubernetes cluster origins), use `ATIHSRAH_PRIVATE_ORIGINS` to allowlist specific private services, and mount your internal root CA certificate with `ATIHSRAH_PRIVATE_CA_FILE`:

```yaml
services:
  atihsrah:
    image: atihsrah:latest
    read_only: true
    cap_drop:
      - ALL
    tmpfs:
      - /tmp:rw,noexec,nosuid,size=64m
    ports:
      - "8080:8080"
    volumes:
      # Mount internal root CA to validate TLS connections to internal services
      - ./certs/internal-ca.crt:/etc/ssl/certs/internal-ca.crt:ro
    environment:
      - ALLOWED_ORIGINS=https://images.example.com
      # Explicit origins allowed to resolve to private networks (RFC 1918 / ULA)
      # http:// is accepted ONLY if explicitly listed with http://
      # Cloud metadata (169.254.0.0/16) and multicast/broadcast remain strictly blocked
      - ATIHSRAH_PRIVATE_ORIGINS=http://minio:9000,https://internal-assets.cluster.local:8443
      - ATIHSRAH_PRIVATE_CA_FILE=/etc/ssl/certs/internal-ca.crt
      - ATIHSRAH_AUTH_MODE=signed
      # Generate a cryptographically secure 32-byte key: openssl rand -hex 32
      - ATIHSRAH_SIGNING_KEY=${ATIHSRAH_SIGNING_KEY:?Generate with: openssl rand -hex 32}
```

---

## URL Signing Guide

When running in `ATIHSRAH_AUTH_MODE=signed`, all `/transform` requests must be signed with HMAC-SHA256.

### Canonical String Format
```text
GET\n
/transform\n
{exp}\n
{param1_key}={param1_encoded_val}\n
{param2_key}={param2_encoded_val}\n
...
{paramN_key}={paramN_encoded_val}
```

1. **Method & Path**: Uppercase `GET`, path `/transform`.
2. **Expiry Line**: Decimal integer Unix timestamp `{exp}`.
3. **Parameter Sorting**: All query parameters except `sig` sorted in ascending ASCII alphabetical order (`a.cmp(b)`).
4. **RFC 3986 Encoding**: Parameter values percent-encoded (unreserved characters `A-Z`, `a-z`, `0-9`, `-`, `_`, `.`, `~` remain unencoded).
5. **HMAC Calculation**: HMAC-SHA256 digest computed over canonical string bytes using raw binary secret key.

For comprehensive technical specifications, worked examples, and reference implementations, see [docs/SIGNING.md](docs/SIGNING.md).

### Client Examples

#### Python 3
```python
import hmac, hashlib, time, urllib.parse

def sign_url(base_url, secret_hex, params, ttl=3600):
    key = bytes.fromhex(secret_hex)
    exp = int(time.time()) + ttl
    params['exp'] = str(exp)

    # Sort parameters alphabetically by key
    sorted_items = sorted(params.items(), key=lambda x: x[0])
    encoded_pairs = [f"{k}={urllib.parse.quote(str(v), safe='~')}" for k, v in sorted_items]
    canonical_str = f"GET\n/transform\n{exp}\n" + "\n".join(encoded_pairs)

    sig = hmac.new(key, canonical_str.encode('utf-8'), hashlib.sha256).hexdigest()
    query = "&".join(encoded_pairs) + f"&sig={sig}"
    return f"{base_url}/transform?{query}"
```

#### Node.js / TypeScript
```typescript
import * as crypto from 'crypto';

export function signUrl(baseUrl: string, secretHex: string, params: Record<string, string>, ttl = 3600): string {
  const exp = Math.floor(Date.now() / 1000) + ttl;
  const allParams = { ...params, exp: String(exp) };

  const sortedKeys = Object.keys(allParams).sort();
  const encodedPairs = sortedKeys.map(k => `${k}=${encodeURIComponent(allParams[k]).replace(/[!'()*]/g, c => '%' + c.charCodeAt(0).toString(16).toUpperCase())}`);
  const canonicalStr = `GET\n/transform\n${exp}\n` + encodedPairs.join('\n');

  const key = Buffer.from(secretHex, 'hex');
  const sig = crypto.createHmac('sha256', key).update(canonicalStr, 'utf8').digest('hex');
  return `${baseUrl}/transform?${encodedPairs.join('&')}&sig=${sig}`;
}
```

---

## Format Policy & Image Codecs (Pure Rust)

Atihsrah is built strictly with pure-Rust image codecs (`image-rs` and `ravif`), eliminating external C image libraries (`libwebp-sys`, `libvips`, etc.).

### Automatic Negotiation (`format=auto`)
When `format=auto` (the default) is requested, the output format is determined dynamically:
- **Source has Alpha Transparency**: Evaluates lossless WebP and PNG, serving whichever delivers smaller bytes (if client's `Accept` allows WebP, otherwise PNG).
- **Photographic Content**: Evaluates AVIF if client's `Accept` allows it; otherwise falls back to JPEG. **Never lossless WebP for photos**, preventing size inflation on continuous-tone images.
- **Graphic / Flat Content**: Evaluates lossless WebP (if allowed) and PNG, serving whichever is smaller.
- **Photo vs Graphic Heuristic**: JPEG sources are treated as photos. For PNG/WebP sources without alpha, Atihsrah samples downscaled 64×64 pixels and counts distinct RGB colors against `ATIHSRAH_DISTINCT_COLOR_THRESHOLD` (default 256). An explicit `format=` query parameter always overrides the heuristic.

### Explicit Formats & Quality Targets
- **Explicit `format=webp`**: Serves pure-Rust lossless WebP. Note that `quality_target` does not affect WebP in this pure-Rust build.
- **`quality_target` (`draft`, `good`, `high`, `lossless`)**: Controls lossy quantization for JPEG and AVIF.
- **Larger-Than-Source Guard**: Atihsrah never returns raw unoptimized source bytes (preventing metadata leakage). If transformed output is larger than source, it evaluates up to 2 retry candidates (lower quality step or alternate format) bounded by `ATIHSRAH_MAX_RETRY_PIXELS` (default 16M pixels). If still larger, it serves the smallest candidate and records the Prometheus metric `atihsrah_output_larger_than_source_total`.

---

## Known Limitations

1. **AVIF Encoding CPU Overhead**:
   - Pure software AVIF encoding via `ravif` is CPU-intensive compared to C assembly accelerated `libheif`/`libaom`. Tune `ATIHSRAH_AVIF_SPEED=6..8` or use JPEG for continuous-tone fallbacks when AVIF encoding latency exceeds budgets.
2. **Lossless-Only WebP**:
   - Pure-Rust `image-rs` encodes WebP in lossless mode only. While lossless WebP provides superior compression for graphics, icons, and UI screenshots, it produces larger files on continuous-tone photographs. Under `format=auto`, Atihsrah avoids photo bloat by routing photos to AVIF or JPEG.
3. **In-Memory Only & Cache Lost on Restart**:
   - The LRU cache resides strictly in process RAM (`moka`) for sub-millisecond lookups without network hops. There is no distributed Redis or disk persistence tier.
   - Restarting or redeploying the Atihsrah container empties the cache. In production, run Atihsrah behind an edge CDN (e.g. Cloudflare, CloudFront, Fastly). In signed mode, `Cache-Control: max-age` is clamped by the signature expiration (`exp`), and edge-cached copies continue to be served by CDNs until the CDN edge TTL elapses. (In public mode without `exp`, `v` versioned URLs emit `Cache-Control: public, max-age=31536000, immutable`).
4. **Signature Expiration & Replay Window**:
   - Signature expiration (`exp`) **limits** the replay attack window, but does **not** provide single-use nonces.
   - Atihsrah is strictly stateless; URLs remain valid for multiple requests until the `exp` timestamp elapses (which is standard for CDN edge caching).
5. **Metadata Stripped by Re-Encoding**:
   - Image re-encoding strips EXIF, GPS, camera metadata, and ICC color profiles to protect user privacy and minimize payload bytes. Original binary source bytes are never passed through raw.

---

## Security & Verification

Atihsrah is built with automated CI security verification covering 22 threat vectors:
- **SSRF**: IPv4/IPv6 private subnets, carrier-grade NAT, IPv4-mapped IPv6, and DNS rebinding attacks.
- **Malformed Inputs**: Decompression bombs, truncated frame headers, and memory exhaustion vectors rejected prior to full decoding.
- **HTTP Hardening**: Content-Length smuggling, oversized headers (431), and parameter pollution rejected.
- **Fuzz Testing**: Automated continuous fuzzing targets covering the URL validator, query parser, canonical string generator, and image probe routines.

Detailed threat mitigation mappings are documented in [THREAT_MODEL.md](THREAT_MODEL.md).

```bash
# Run the automated test suite (144 tests passing)
cargo test

# Run static analysis and style checks
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
```

---

## License

Atihsrah is free and open-source software licensed under the **GNU Affero General Public License v3.0** ([AGPL-3.0](LICENSE)).
