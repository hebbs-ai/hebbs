# ──────────────────────────────────────────────────────────────────────────────
# HEBBS - Multi-stage Production Dockerfile
# ──────────────────────────────────────────────────────────────────────────────
# Produces a minimal container image (~80MB) with:
#   - hebbs        (unified binary: memory engine + vault operations)
#   - hebbs-bench  (in-situ performance validation)
#
# Build:
#   docker buildx build --platform linux/amd64,linux/arm64 -t hebbs-ai/hebbs .
#
# Run:
#   docker run -p 6380:6380 -p 6381:6381 -v hebbs-data:/data hebbs-ai/hebbs
# ──────────────────────────────────────────────────────────────────────────────

# ── Stage 1: Build ───────────────────────────────────────────────────────────
# Ubuntu 24.04 provides glibc 2.39, required by ort-sys pre-built ONNX Runtime.
FROM ubuntu:24.04 AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    curl ca-certificates build-essential g++ pkg-config \
    protobuf-compiler libprotobuf-dev libclang-dev libssl-dev \
    && rm -rf /var/lib/apt/lists/*

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | \
    sh -s -- -y --default-toolchain 1.91.0
ENV PATH="/root/.cargo/bin:${PATH}"

WORKDIR /build

COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/
COPY proto/ proto/

RUN cargo build --release \
    -p hebbs-vault \
    -p hebbs-bench && \
    strip target/release/hebbs && \
    strip target/release/hebbs-bench

# Verify the binary links only against glibc/libgcc (available in distroless cc)
RUN ldd target/release/hebbs || true

# ── Stage 2: Runtime ─────────────────────────────────────────────────────────
FROM gcr.io/distroless/cc-debian13:nonroot

LABEL org.opencontainers.image.title="HEBBS" \
      org.opencontainers.image.description="Cognitive memory engine for AI agents" \
      org.opencontainers.image.url="https://github.com/hebbs-ai/hebbs" \
      org.opencontainers.image.source="https://github.com/hebbs-ai/hebbs" \
      org.opencontainers.image.vendor="HEBBS" \
      org.opencontainers.image.licenses="BSL-1.1"

# Binaries
COPY --from=builder /build/target/release/hebbs /usr/local/bin/hebbs
COPY --from=builder /build/target/release/hebbs-bench /usr/local/bin/hebbs-bench

# Data directory (RocksDB) and model cache owned by nonroot (UID 65532)
# distroless:nonroot already runs as 65532, directories are created here
# so the PVC mount point exists even if no volume is attached.
VOLUME ["/data", "/models"]

# gRPC and HTTP ports
EXPOSE 6380 6381

# Environment defaults (overridable via Helm values / docker run -e)
ENV HEBBS_STORAGE_DATA_DIR=/data \
    HEBBS_SERVER_BIND_ADDRESS=0.0.0.0 \
    HEBBS_SERVER_GRPC_PORT=6380 \
    HEBBS_SERVER_HTTP_PORT=6381 \
    HEBBS_LOGGING_FORMAT=json \
    HEBBS_LOGGING_LEVEL=info

# Graceful shutdown: server handles SIGTERM (Phase 8)
STOPSIGNAL SIGTERM

# Health check: distroless has no curl/wget, so orchestrators (k8s, Compose)
# should use HTTP probes against the readiness endpoint:
#   http://localhost:6381/v1/health/ready
#
# For Docker standalone, override with:
#   docker run --health-cmd "wget -qO- http://localhost:6381/v1/health/ready || exit 1" ...

ENTRYPOINT ["/usr/local/bin/hebbs"]
CMD ["serve", "--foreground"]
