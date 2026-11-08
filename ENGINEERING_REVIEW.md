# HISTORICAL ENGINEERING & ARCHITECTURE REVIEW

> [!NOTE]
> **AUDIT NOTE ON PURE-RUST ARCHITECTURE (OPTION A)**:
> In accordance with user policy directives, Atihsrah is built strictly with pure-Rust image codecs (`image-rs` lossless WebP, `ravif` AVIF) without external C libraries (`libvips`, `libwebp-sys`). The binary compiled for the container dynamically links glibc (`libc.so.6`, `libm.so.6`) provided by the `gcr.io/distroless/cc-debian12` rootfs in a minimal `FROM scratch` container. For verified empirical facts, see [`docs/GROUND_TRUTH.md`](./docs/GROUND_TRUTH.md) and [`docs/CLAIMS.md`](./docs/CLAIMS.md).

---

# Atihsrah Engineering & Architecture Review

**Date**: November 2026  
**Auditor**: Principal Rust Engineer, Performance Engineer, Infrastructure Architect  
**Repository**: Atihsrah (`lumina`)  
**Commit**: Master Branch (`remediated`)  
**License**: AGPL-3.0  

---

## 1. Architectural Overview & System Design

Atihsrah is built on modern idiomatic Rust with Axum, Tokio, Reqwest (Rustls), Moka, Governor, and tracing. It strictly adheres to unix philosophy: single-purpose, self-contained, stateless, and secure.

```
                           +---------------------------+
                           |   Client HTTP Request     |
                           +-------------+-------------+
                                         |
                                         v
                      +-------------------------------------+
                      |   1. Inflight & Limits Middleware   |
                      |   (Header size, 64 header count,    |
                      |    Inflight HTTP request count)     |
                      +------------------+------------------+
                                         |
                                         v
                      +-------------------------------------+
                      |   2. Pre-Auth Rate Limiter (IP)     |
                      |   (Blocks brute-force & kid spray)  |
                      +------------------+------------------+
                                         |
                                         v
                      +-------------------------------------+
                      |   3. Cryptographic Signature Check  |
                      |   (HMAC-SHA256, constant-time eq,   |
                      |    exp TTL + clock skew validation) |
                      +------------------+------------------+
                                         |
                                         v
                      +-------------------------------------+
                      |   4. Candidate Cache Fast-Path      |
                      |   (Auto format RFC9110 negotiation, |
                      |    Moka in-memory byte cache lookup)|
                      +--------+--------------------+-------+
                               | Hit                | Miss
                               v                    v
                      +----------------+   +-------------------------------------+
                      | 200 OK Cached  |   |   5. Single-Flight Coalescing       |
                      | (Clamped max-  |   |   (InflightManager Leader/Follower) |
                      |  age by exp)   |   +------------------+------------------+
                      +----------------+                      |
                                                              v
                                           +-------------------------------------+
                                           |   6. Safe DNS & Origin Fetch        |
                                           |   (SafeDnsResolver, SSRF block,     |
                                           |    No redirects, 15s timeout)       |
                                           +------------------+------------------+
                                                              |
                                                              v
                                           +-------------------------------------+
                                           |   7. Pre-Decode Probe & Memory Gate |
                                           |   (Header-only dimension validation,|
                                           |    max memory bomb protection)      |
                                           +------------------+------------------+
                                                              |
                                                              v
                                           +-------------------------------------+
                                           |   8. Worker Concurrency Gate        |
                                           |   (TransformQueueGuard -> Semaphore |
                                           |    -> spawn_blocking image process) |
                                           +------------------+------------------+
                                                              |
                                                              v
                                           +-------------------------------------+
                                           |   9. Cache Insert & Stream Response |
                                           |   (Moka byte-size check, Vary:Accept|
                                           |    nosniff, metrics increment)      |
                                           +-------------------------------------+
```

---

## 2. Concurrency, Thread Safety & Cancellation Safety

### 2.1 Single-Flight Coalescing (`InflightManager`)
- **Mechanism**: The single-flight manager prevents cache stampedes when 100+ concurrent requests target the same uncached image.
- **Cancellation Safety**: Previous implementations suffered from deadlocks and poisoned hash maps when the leader task disconnected. The remediation introduced `InflightLeaderGuard`:
  - Utilizes RAII `Drop` semantics to guarantee that if a leader future is aborted, the key is unconditionally evicted from the shared map.
  - Broadcast receiver channels are closed, signaling waiting followers.
  - Followers intercept `RecvError::Closed` and automatically transition to become the new Leader without returning HTTP 500 errors.

### 2.2 Concurrency Guards & Semaphore Protection
- **Mechanism**: CPU-intensive decoding and encoding are bounded by `ATIHSRAH_MAX_CONCURRENT_TRANSFORMS` using `tokio::sync::Semaphore`.
- **Cancellation Safety**: A major vulnerability identified in the audit was counter leakage when clients disconnected while waiting in queue.
  - Implemented `TransformQueueGuard` and `ActiveTransformGuard`.
  - If a client connection drops while awaiting `semaphore.acquire_owned().await`, `TransformQueueGuard::drop` atomically decrements `queued_transforms`.
  - This prevents `queued_transforms` from ever falsely triggering `/ready` saturation (503 Service Unavailable).

### 2.3 CPU-Bound Image Execution
- All decoding, resizing, color quantization, and encoding operations run exclusively within `tokio::task::spawn_blocking` worker threads.
- Tokio's asynchronous reactor threads are never blocked by image crunching.

---

## 3. Memory Safety & Resource Management

### 3.1 Pre-Decode Dimension & Memory Estimation
- Before allocating image buffers or parsing compressed streams, Atihsrah probes image headers:
  - Validates `info.width <= max_source_dimension` and `info.height <= max_source_dimension`.
  - Computes `estimated_memory_mb = (width * height * 4) / (1024 * 1024)`.
  - If `estimated_memory_mb > max_processing_memory_mb`, the image is rejected with HTTP 422 before full decode.
  - Prevents decompression bombs (e.g. 100,000 x 100,000 pixel PNGs).

### 3.2 Byte-Bounded In-Memory Caching (`Moka`)
- Cache capacity is bounded by byte size (`cache_max_size_mb`), not entry count.
- Each entry is weighed by its actual serialized output byte length.
- Cache entries larger than `cache_single_entry_max_mb` are served directly to the client without being placed in cache, preventing cache thrashing.

### 3.3 Zero-Allocation & Memory Pruning in Rate Limiters
- Governor in-memory key stores are bounded by periodic retention pruning:
  `limiter.retain_recent()` is automatically called every 30 seconds to evict expired IP and key buckets.

---

## 4. Cryptography & Authentication Architecture

### 4.1 Cryptographic Algorithms
- Signatures use **HMAC-SHA256**.
- Implemented with the pure-Rust `hmac` and `sha2` crates.
- Verification uses constant-time comparison via `subtle::ConstantTimeEq` to eliminate timing side-channels.

### 4.2 Canonical String Specification
The canonical signing string is strictly defined as:
```
GET\n/transform\n{exp}\n{param1=val1}\n{param2=val2}...
```
- Parameters are sorted alphabetically by key.
- Values are encoded using strict RFC 3986 percent-encoding (unreserved set `A-Z a-z 0-9 - _ . ~`).
- Multiple keys and rotating key IDs (`kid`) are fully supported.

---

## 5. Observability & Operational Readiness

### 5.1 Privacy-Preserving JSON Logs
- Structured JSON output formatted by `tracing-subscriber`.
- **Privacy Redaction Rule**: Raw source URLs, secret keys, signatures, tokens, and `Authorization` headers are never emitted to logs.
- Source URLs are represented solely by `sha256_prefix(&params.src)` (16 hex characters).

### 5.2 Isolated Metrics Listener
- Prometheus metrics are served on a completely separate listener (`ATIHSRAH_METRICS_BIND`, default `127.0.0.1:9090`).
- The metrics port is inaccessible from public traffic on `ATIHSRAH_BIND_ADDR`.

### 5.3 Health & Readiness Lifecycle
- `/health`: Liveness probe. Always returns 200 OK unless the runtime is wedged. High traffic load or full queues do not fail liveness.
- `/ready`: Readiness probe. Returns 503 during startup, during graceful drain on `SIGTERM` / `SIGINT`, or when `is_saturated()` is true.

---

## 6. Static Analysis & Verification Summary

- **Clippy**: 0 warnings across all targets (`--all-targets -- -D warnings`).
- **Formatting**: 100% compliant with `rustfmt` (`cargo fmt --check`).
- **Test Suite**: 142 tests passing across all targets (unit, security, integration, and remediation suites).
- **Container Build**: Multi-stage minimal distroless build with non-root execution and 0 HIGH/CRITICAL findings in Trivy container scan (scope: 10 Debian base packages, 0 language files).

