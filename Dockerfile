# syntax=docker/dockerfile:1.7
# ============================================================================
# EventHorizon — multi-stage Dockerfile
# ============================================================================
# Stage 1 (build) — pinned Rust toolchain, cached deps via Cargo's lockfile.
# Stage 2 (runtime) — minimal Debian slim, non-root user, just the two bins.
# ============================================================================

ARG RUST_VERSION=1.85

# ---------------------------------------------------------------------------
# Build stage
# ---------------------------------------------------------------------------
FROM rust:${RUST_VERSION}-slim-bookworm AS build

WORKDIR /src

RUN apt-get update \
 && apt-get install -y --no-install-recommends \
        pkg-config \
        libssl-dev \
        ca-certificates \
 && rm -rf /var/lib/apt/lists/*

# Copy the workspace manifests + toolchain pin first so cargo can resolve
# dependencies before source changes invalidate the cache.
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates ./crates

# Build the two release binaries.
RUN cargo build --release --bin eh-bin --bin eh-ctl

# ---------------------------------------------------------------------------
# Runtime stage
# ---------------------------------------------------------------------------
FROM debian:bookworm-slim AS runtime

RUN apt-get update \
 && apt-get install -y --no-install-recommends \
        ca-certificates \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --create-home --uid 10001 --user-group --shell /sbin/nologin eh

COPY --from=build /src/target/release/eh-bin /usr/local/bin/eh-bin
COPY --from=build /src/target/release/eh-ctl /usr/local/bin/eh-ctl

USER 10001:10001
WORKDIR /home/eh

ENV RUST_LOG=info \
    EH_PORT=8080

EXPOSE 8080

HEALTHCHECK --interval=10s --timeout=3s --start-period=10s --retries=3 \
    CMD ["/usr/local/bin/eh-ctl", "--help"]

ENTRYPOINT ["/usr/local/bin/eh-bin"]
