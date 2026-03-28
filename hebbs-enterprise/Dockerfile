# HEBBS Enterprise - Phase 1 Dockerfile
# Builds the hebbs binary (vault daemon + Memory Palace) with OpenAI embeddings.
#
# Build context: parent directory (hebbs-repos/) so we can access hebbs/ source.
# Build:  docker compose build
# Run:    docker compose up

# ── Stage 1: Build ───────────────────────────────────────────────────────────
FROM ubuntu:24.04 AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    curl ca-certificates build-essential g++ pkg-config \
    protobuf-compiler libprotobuf-dev libclang-dev libssl-dev \
    && rm -rf /var/lib/apt/lists/*

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | \
    sh -s -- -y --default-toolchain 1.91.0
ENV PATH="/root/.cargo/bin:${PATH}"

WORKDIR /build

COPY hebbs/Cargo.toml hebbs/Cargo.lock ./
COPY hebbs/crates/ crates/
COPY hebbs/proto/ proto/

RUN cargo build --release -p hebbs-vault && \
    strip target/release/hebbs

# ── Stage 2: Runtime ─────────────────────────────────────────────────────────
FROM ubuntu:24.04

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

RUN useradd -m -u 1001 hebbs

COPY --from=builder /build/target/release/hebbs /usr/local/bin/hebbs
COPY hebbs-enterprise/entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh

RUN mkdir -p /data && chown hebbs:hebbs /data

USER hebbs
WORKDIR /data

VOLUME ["/data"]
EXPOSE 6381

ENV HEBBS_LLM_PROVIDER=openai \
    HEBBS_LLM_MODEL=gpt-4o-mini \
    HEBBS_LLM_API_KEY="" \
    HEBBS_EMBED_PROVIDER=openai \
    HEBBS_EMBED_MODEL=text-embedding-3-small \
    HEBBS_EMBED_DIMENSIONS=1536 \
    HEBBS_PANEL_PORT=6381

STOPSIGNAL SIGTERM

ENTRYPOINT ["/entrypoint.sh"]
