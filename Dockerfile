# === Stage 1: Frontend dist (prebuilt) ===
# This Dockerfile expects `dist/` to be present in the build context.
# In CI, it is produced by the Pipeline workflow and downloaded as an artifact.
FROM scratch AS frontend-dist
COPY dist /dist

# === Stage 2: Build Backend (Axum) ===
FROM rust:1.91-bookworm AS backend-builder

# System deps for crates that link native libraries (e.g. OpenSSL, V8 tooling)
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    protobuf-compiler \
    python3 \
    ca-certificates \
    curl \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app

# Cargo network tuning / mirrors (optional).
# - Default keeps using crates.io; override CARGO_REGISTRY to use a faster mirror (e.g. sparse+https://rsproxy.cn/index/)
# - Increase low-speed timeouts to avoid flaky networks breaking Docker builds.
ARG CARGO_REGISTRY="sparse+https://index.crates.io/"
ARG CARGO_HTTP_TIMEOUT="600"
ARG CARGO_HTTP_LOW_SPEED_LIMIT="1"
ARG CARGO_HTTP_LOW_SPEED_TIME="600"
ENV CARGO_HTTP_TIMEOUT=${CARGO_HTTP_TIMEOUT}
ENV CARGO_HTTP_LOW_SPEED_LIMIT=${CARGO_HTTP_LOW_SPEED_LIMIT}
ENV CARGO_HTTP_LOW_SPEED_TIME=${CARGO_HTTP_LOW_SPEED_TIME}
RUN mkdir -p /root/.cargo && \
    printf "[source.crates-io]\nreplace-with = 'mirror'\n\n[source.mirror]\nregistry = \"%s\"\n" "${CARGO_REGISTRY}" > /root/.cargo/config.toml

# Cache dependencies: build with a dummy backend first (only invalidated by Cargo.* changes)
COPY Cargo.toml Cargo.lock ./
COPY backend/Cargo.toml backend/Cargo.toml
RUN mkdir -p backend/src backend/src/bin && \
    printf '%s\n' 'fn main() {}' > backend/src/main.rs && \
    printf '%s\n' 'fn main() {}' > backend/src/bin/renderd.rs && \
    cargo build --release -p backend --bin backend --bin renderd && \
    rm -f target/release/backend target/release/renderd && \
    rm -rf target/release/deps/backend* target/release/deps/renderd* \
           target/release/.fingerprint/backend-* target/release/.fingerprint/renderd-* && \
    rm -rf backend/src

# Copy real sources and build
COPY backend/src backend/src
RUN cargo build --release -p backend --bin backend --bin renderd && \
    grep -a -q "启动 nBot 后端" target/release/backend && \
    grep -a -q "renderd listening" target/release/renderd

# === Stage 3: Runtime (Bot) ===
FROM debian:bookworm-slim AS bot-runtime

# Install minimal runtime dependencies
RUN set -eux; \
    apt-get update; \
    apt-get install -y --no-install-recommends \
      libssl3 ca-certificates ffmpeg \
      curl gnupg; \
    install -m 0755 -d /etc/apt/keyrings; \
    curl -fsSL https://download.docker.com/linux/debian/gpg | gpg --dearmor -o /etc/apt/keyrings/docker.gpg; \
    chmod a+r /etc/apt/keyrings/docker.gpg; \
    echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.gpg] https://download.docker.com/linux/debian bookworm stable" > /etc/apt/sources.list.d/docker.list; \
    apt-get update; \
    apt-get install -y --no-install-recommends docker-ce-cli docker-compose-plugin; \
    rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy ONLY the final artifacts
COPY --from=backend-builder /app/target/release/backend /app/server
COPY --from=backend-builder /app/target/release/renderd /app/renderd
COPY --from=frontend-dist /dist /app/dist
# Copy assets for help image template
COPY assets /app/assets
# Built-in definitions (modules/commands/plugins). Runtime state is persisted via volume.
COPY data /app/data.seed
# Image slimming: do not bundle built-in plugin code in the bot image (official plugins are served by nbot-site).
RUN rm -rf /app/data.seed/plugins || true
COPY docker/entrypoint.sh /app/entrypoint.sh
RUN chmod +x /app/entrypoint.sh

# Expose port
EXPOSE 32100

# Set environment to production
ENV RUST_LOG=info
ENV NBOT_PORT=32100

# Init data volume then run server
ENTRYPOINT ["/app/entrypoint.sh"]
CMD ["/app/server"]

# === Stage 4: Runtime (Renderer Tool Container) ===
FROM debian:bookworm-slim AS render-runtime

RUN apt-get update && apt-get install -y \
    ca-certificates \
    wkhtmltopdf \
    fonts-noto-cjk \
    fonts-noto-color-emoji \
    fontconfig \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=backend-builder /app/target/release/renderd /app/renderd
EXPOSE 8080
ENV RUST_LOG=info
ENV PORT=8080
CMD ["/app/renderd"]
