# Atihsrah Threat Model & Security Architecture

Atihsrah is a self-hosted, AGPL-3.0 image optimization reverse proxy designed for strict defense-in-depth operation. It fetches images solely from allowlisted HTTPS origins, transforms them in-memory, caches results in a byte-bounded store, and serves them.

This document describes the threat model, technical mitigations, the automated test verifying each mitigation, and known residual risks.

---

## 1. Threat Matrix & Test Traceability

| Threat ID | Threat Name | Category | Primary Mitigation | Proving Test Function & File | Residual Risks |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **TM-01** | SSRF via Private / Loopback IPs | SSRF | `SafeDnsResolver` verifies resolved IPs against strict denylist (IPv4 RFC1918, loopback, link-local, multicast, IPv6 ULA, link-local, IPv4-mapped IPv6) before connect | `test_ssrf_all_ip_classes_and_ipv4_mapped_ipv6_blocked` ([`tests/security/ssrf.rs`](tests/security/ssrf.rs)) | Upstream router NAT redirection outside host boundary |
| **TM-02** | DNS Rebinding Attacks | SSRF | DNS resolution checks target IP immediately prior to TCP handshake; private IPs returned after initial check trigger abort | `test_ssrf_dns_rebinding_blocked` ([`tests/security/ssrf.rs`](tests/security/ssrf.rs)), `test_dns_rebinding_second_lookup_private_blocked` ([`tests/fetch_security.rs`](tests/fetch_security.rs)) | Upstream forward proxies performing unverified DNS resolution |
| **TM-03** | HTTP Redirect Exploitation | SSRF | `reqwest::redirect::Policy::none()` unconditionally refuses to follow 301, 302, 307, 308 redirects | `test_redirect_rejection` ([`tests/security/ssrf.rs`](tests/security/ssrf.rs)), `test_redirect_rejected` ([`tests/fetch_security.rs`](tests/fetch_security.rs)) | None at proxy layer |
| **TM-04** | URL Parser Confusion (Credentials, Fragments, Non-443 Ports) | Parser Abuse | `validate_source_url` rejects userinfo (`user:pass@`), `#fragment`, non-HTTPS schemes, and non-443 ports | `test_url_credentials_fragment_port_tricks_rejected` ([`tests/security/ssrf.rs`](tests/security/ssrf.rs)) | Ambiguous origin web server URI parsing |
| **TM-05** | Lookalike & Suffix Hostnames | Auth / Origin | Hostnames parsed, lowercased, Punycode-canonicalized, and matched strictly against `ALLOWED_ORIGINS` exact authority matches | `test_lookalike_hostnames_rejected` ([`tests/security/ssrf.rs`](tests/security/ssrf.rs)) | Compromise of an explicitly allowlisted origin server |
| **TM-06** | Request Smuggling & Malformed Headers | Protocol Smuggling | `header_limits_middleware` caps header count (max 64) and size (32KB); Axum/Hyper rejects conflicting `Content-Length` | `test_conflicting_content_length_rejected`, `test_oversized_headers_rejected_with_431` ([`tests/security/http_smuggling.rs`](tests/security/http_smuggling.rs)) | Fronting legacy HTTP/1.0 proxies with non-compliant framing |
| **TM-07** | Invalid UTF-8 in Query Parameters | Input Validation | Safe percentage decoding and strict UTF-8 parsing reject corrupt bytes with 400/422 without panicking | `test_invalid_utf8_in_query_rejected` ([`tests/security/http_smuggling.rs`](tests/security/http_smuggling.rs)) | None |
| **TM-08** | Parameter Pollution & Unknown Injection | Parameter Tampering | `TransformParams::parse` strictly enforces single-occurrence per parameter and rejects unrecognized keys with 422 | `test_parameter_pollution_duplicate_and_unknown_rejected` ([`tests/security/auth_bypass.rs`](tests/security/auth_bypass.rs)), `test_reject_duplicate_and_unknown_params` ([`tests/transform_pipeline.rs`](tests/transform_pipeline.rs)) | None |
| **TM-09** | Signature Tampering & Parameter Reordering | Authentication | Canonical serialization sorts keys alphabetically and encodes values (RFC 3986); verified with constant-time equality | `test_signature_bypass_parameter_tampering_and_case` ([`tests/security/auth_bypass.rs`](tests/security/auth_bypass.rs)), `test_valid_signed_url_succeeds` ([`tests/auth_ratelimit.rs`](tests/auth_ratelimit.rs)) | Compromise of server secret signing key |
| **TM-10** | Unicode Normalization Signature Bypass | Authentication | Signatures computed over RFC 3986 percent-encoded UTF-8 bytes; distinct NFC/NFD encodings yield different signatures and fail validation | `test_signature_unicode_normalization_bypass_attempt` ([`tests/security/auth_bypass.rs`](tests/security/auth_bypass.rs)) | Clients must generate signatures using the exact byte representation requested |
| **TM-11** | Signature Replay & Stale Requests | Replay Attacks | Expiry parameter `exp` validated against clock with strict `MAX_SIGNATURE_TTL_SECS` and configurable clock skew tolerance | `test_expired_signature_returns_410`, `test_far_future_expiry_rejected_with_401` ([`tests/auth_ratelimit.rs`](tests/auth_ratelimit.rs)) | Replays valid within expiration window (no nonce store by design) |
| **TM-12** | Cache Key Collision | Cache Integrity | SHA-256 cache key generated from canonical URL, dimensions, fit mode, output format, quality target, and asset version | `test_cache_key_collision_resistance` ([`tests/security/cache_attacks.rs`](tests/security/cache_attacks.rs)), `test_cache_key_uniqueness` ([`src/cache/mod.rs`](src/cache/mod.rs)) | Cryptographic hash collision ($2^{-256}$ probability) |
| **TM-13** | Cache Poisoning via Differing `Accept` Headers | Cache Integrity | Auto format resolution evaluates client `Accept` headers into discrete target formats before cache lookup; sets `Vary: Accept` | `test_cache_poisoning_differing_accept_headers_prevented` ([`tests/security/cache_attacks.rs`](tests/security/cache_attacks.rs)) | Intermediate CDNs misconfigured to ignore `Vary: Accept` |
| **TM-14** | Rate Limit Bypass via Spoofed `X-Forwarded-For` | Denial of Service | `extract_client_ip` uses `X-Forwarded-For` only if connecting socket is in `TRUSTED_PROXY_CIDRS`; otherwise uses peer socket address | `test_rate_limit_bypass_spoofed_x_forwarded_for_ignored` ([`tests/security/rate_limit.rs`](tests/security/rate_limit.rs)) | Misconfigured `TRUSTED_PROXY_CIDRS` trusting public IPs |
| **TM-15** | Expensive Cache-Miss Flooding | Denial of Service | Dual-tier governor limiters: global requests limit + dedicated stricter limiter for expensive cache misses (`RATE_LIMIT_MISS_PER_MINUTE`) | `test_stricter_miss_limiter_blocks_expensive_work_but_serves_hits` ([`tests/auth_ratelimit.rs`](tests/auth_ratelimit.rs)) | Distributed botnet staying below per-IP thresholds |
| **TM-16** | Decompression Bombs (Pixel Floods) | Resource Exhaustion | Header-only `probe()` reads dimensions before memory allocation; rejects images exceeding `MAX_SOURCE_DIMENSION` or `MAX_PROCESSING_MEMORY_MB` | `test_decompression_bombs_rejected_at_probe_before_decode` ([`tests/security/malformed_inputs.rs`](tests/security/malformed_inputs.rs)), `test_decompression_bomb_rejected_header_only` ([`tests/transform_pipeline.rs`](tests/transform_pipeline.rs)) | Decoder library memory vulnerabilities |
| **TM-17** | Corrupted / Malformed Image Exploits | Memory Safety | Pure Rust image decoding running in `tokio::task::spawn_blocking` worker threads; returns structured 422 errors on decode failure without panics | `test_malformed_image_formats_rejected_with_structured_json` ([`tests/security/malformed_inputs.rs`](tests/security/malformed_inputs.rs)) | Vulnerabilities in external C libraries if `libvips` feature enabled |
| **TM-18** | High-Dimension Concurrency Exhaustion | Resource Exhaustion | Global concurrency semaphore (`MAX_CONCURRENT_TRANSFORMS`) and queue saturation threshold (`ATIHSRAH_QUEUE_SATURATION_THRESHOLD`) | `test_memory_exhaustion_prevented_by_pre_decode_bounds_check` ([`tests/security/dos_exhaustion.rs`](tests/security/dos_exhaustion.rs)) | Extremely constrained container memory allocations |
| **TM-19** | Cache Storage Amplification | Resource Exhaustion | Moka byte-bounded in-memory cache with tinyLFU eviction capped at `CACHE_MAX_SIZE_MB`; items exceeding single entry limit bypass cache | `test_cache_amplification_bounded_by_byte_capacity` ([`tests/security/dos_exhaustion.rs`](tests/security/dos_exhaustion.rs)), `test_byte_weighted_cache_eviction` ([`tests/transform_pipeline.rs`](tests/transform_pipeline.rs)) | Cache thrashing on high-cardinality unique requests |
| **TM-20** | Sensitive Information Leakage in Logs | Information Disclosure | `sha256_prefix` hashes URLs (16-char prefix); secrets, raw tokens, signatures, and Authorization headers are strictly redacted | `test_log_redaction_secrets_and_urls_never_logged` ([`tests/observability_ops.rs`](tests/observability_ops.rs)) | Upstream reverse proxy access logs logging raw query strings |
| **TM-21** | Slowloris / Slow Origin Starvation | Denial of Service | `SOURCE_CONNECT_TIMEOUT_SECS` and `SOURCE_FETCH_TIMEOUT_SECS` enforce strict socket connect and stream chunk read deadlines | `test_slow_loris_origin_times_out` ([`tests/fetch_security.rs`](tests/fetch_security.rs)) | High-latency origin connections timing out on large assets |
| **TM-22** | MIME Type Confusion & XSS | Content Sniffing | `X-Content-Type-Options: nosniff` header sent on all responses; strict canonical `Content-Type` mappings; SVG is completely excluded | `test_security_headers_and_body_limits` ([`tests/observability_ops.rs`](tests/observability_ops.rs)) | None |
| **TM-23** | SSRF via IPv6 Encapsulation (6to4, Teredo, SIIT, Discard, BMWG) | SSRF | `is_safe_public_ip` unwraps embedded IPv4 from 6to4 (`2002::/16`), Teredo (`2001::/32`), and SIIT (`::ffff:0:0/96`) and validates against IPv4 private list; blocks Discard and BMWG | `test_finding_i_ipv6_special_addresses_blocked` ([`tests/audit_remediation.rs`](tests/audit_remediation.rs)) | None |
| **TM-24** | Single-Flight Task Cancellation Map Poisoning | Denial of Service | `InflightLeaderGuard` automatically cleans up key on task drop; followers automatically retry and take over leadership | `test_finding_c_leader_abort_triggers_follower_takeover` ([`tests/audit_remediation.rs`](tests/audit_remediation.rs)) | None |
| **TM-25** | Concurrency Queue Counter Leakage on Request Abort | Denial of Service | `TransformQueueGuard` RAII decrement on drop prevents `queued_transforms` from leaking on client cancel | `test_finding_d_queue_cancellation_no_counter_leak` ([`tests/audit_remediation.rs`](tests/audit_remediation.rs)) | None |
| **TM-26** | HTTP Accept Format Negotiation Smuggling (`q=0` bypass) | Content Negotiation | RFC 9110 Section 12.5.1 media range parser evaluates `q` values and strictly rejects formats with `q=0` | `test_finding_j_accept_negotiation_respects_q_values` ([`tests/audit_remediation.rs`](tests/audit_remediation.rs)) | None |
| **TM-27** | Single-Flight In-Flight Format Collision | Cache Integrity | `inflight_key` separates candidate formats so concurrent clients with differing Accept headers (AVIF vs WebP) never receive mismatched formats | `test_finding_k_inflight_key_different_accept_headers` ([`tests/audit_remediation.rs`](tests/audit_remediation.rs)) | None |

---

## 2. Residual Risks & Security Considerations

1. **Native Image Decoder Libraries (libvips)**:
   - When compiled with the default pure-Rust `image-rs` engine, memory safety is enforced by Rust's compiler guarantees.
   - If compiled with the optional `libvips` C engine, vulnerabilities in native C libraries (libjpeg, libpng, libwebp) could lead to memory corruption. When deploying with `libvips`, running within an unprivileged Linux container with seccomp/AppArmor and read-only root filesystems is mandatory.

2. **Replay Window for Signed URLs**:
   - By design (PRODUCT RULES), Atihsrah does not implement a distributed nonce store or database. Signatures are valid until the `exp` timestamp is reached.
   - For applications with high security requirements, keep signature lifetimes short (`exp = now + 60` or `120` seconds).

3. **Shared Log Ingestion Proxies**:
   - Atihsrah redacts all URLs, signatures, and tokens from its application logs using SHA-256 prefixes.
   - However, if an upstream reverse proxy (e.g. AWS ALB, Cloudflare, NGINX) fronting Atihsrah logs raw incoming query strings, query tokens could be leaked at that edge tier. Upstream proxies must be configured to redact query parameters.

4. **Public Auth Mode**:
   - When `ATIHSRAH_AUTH_MODE=public`, endpoints are unauthenticated and rely solely on IP-based rate limiting. Operators running in public mode must ensure trusted reverse proxies are accurately declared in `TRUSTED_PROXY_CIDRS`.

---

## 3. Automated Continuous Fuzzing

Automated continuous fuzzing is integrated under `fuzz/` using `cargo-fuzz` and `libFuzzer`:
- `fuzz_query_parser`: Fuzzes query string parameter extraction and validation.
- `fuzz_canonical_builder`: Fuzzes canonical string construction and verifies invariance and determinism.
- `fuzz_url_validator`: Fuzzes URL validation and SSRF origin checks.
- `fuzz_image_probe`: Fuzzes pre-decode image header inspection on arbitrary byte streams.

The corpus for all fuzz targets is tracked and committed in `fuzz/corpus/`.

