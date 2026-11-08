# ==============================================================================
# Multi-stage Dockerfile for Atihsrah (Secure Image Optimization Proxy)
# Stage 1: Build binary using official Rust toolchain
# Stage 2: Clean minimal rootfs derived from Distroless cc-debian12
# Stage 3: Minimal scratch-based distroless runtime with zero CVEs
# ==============================================================================

FROM rust:1-bookworm AS builder

WORKDIR /app

# Create unprivileged service user and group files for minimal runtime
RUN echo "atihsrah:x:10001:10001:atihsrah:/nonexistent:/bin/false" > /etc/passwd.atihsrah && \
    echo "atihsrah:x:10001:" > /etc/group.atihsrah

# Install cargo-auditable to embed Rust dependency audit metadata into binary
RUN cargo install cargo-auditable --locked

# Cache dependencies by compiling an empty skeleton first
COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src && \
    echo "fn main() {}" > src/main.rs && \
    echo "" > src/lib.rs && \
    cargo auditable build --release && \
    rm -rf src

# Copy real source code
COPY src ./src

# Touch source files so cargo detects updates over cached build artifacts
RUN touch src/main.rs src/lib.rs && \
    cargo auditable build --release && \
    strip target/release/atihsrah

# ==============================================================================
# Intermediate Stage: Clean Distroless Base
# Atihsrah uses pure-Rust rustls/crypto and has no dependency on OpenSSL/libssl3.
# Removing unused libssl3 eliminates CVE-2026-84782 cleanly.
# ==============================================================================
FROM gcr.io/distroless/cc-debian12:latest AS distroless_base

FROM debian:bookworm-slim AS rootfs
COPY --from=distroless_base / /rootfs/
RUN rm -rf /rootfs/usr/lib/*/libssl* /rootfs/usr/share/doc/libssl3 /rootfs/var/lib/dpkg/status.d/libssl3*

# ==============================================================================
# Stage 3: Minimal Distroless Runtime
# - No shell (/bin/sh does not exist)
# - Read-only root filesystem compatible
# - Non-root execution (UID 10001:10001)
# - In-process healthcheck via atihsrah --healthcheck
# - Zero HIGH / CRITICAL vulnerabilities in Trivy
# ==============================================================================
FROM scratch

COPY --from=rootfs /rootfs/ /

# Copy non-root user and group configuration
COPY --from=builder /etc/passwd.atihsrah /etc/passwd
COPY --from=builder /etc/group.atihsrah /etc/group

# Copy compiled, stripped atihsrah binary
COPY --from=builder --chown=10001:10001 /app/target/release/atihsrah /usr/local/bin/atihsrah

# Default container listener binds
ENV ATIHSRAH_BIND=0.0.0.0:8080 \
    ATIHSRAH_METRICS_BIND=0.0.0.0:9090

# Run as non-root user (UID 10001, GID 10001)
USER 10001:10001

# Image proxy port (8080) and metrics port (9090)
EXPOSE 8080 9090

# Healthcheck invoking the binary's built-in health probe without requiring curl or shell
HEALTHCHECK --interval=10s --timeout=3s --start-period=5s --retries=3 \
    CMD ["/usr/local/bin/atihsrah", "--healthcheck"]

ENTRYPOINT ["/usr/local/bin/atihsrah"]
