# syntax=docker/dockerfile:1.7
FROM node:22-bookworm-slim AS web
WORKDIR /build/frontend
COPY frontend/package*.json ./
RUN --mount=type=cache,target=/root/.npm npm ci --no-audit --no-fund
COPY frontend/ ./
RUN npm run build

FROM rust:1-bookworm AS rust
RUN apt-get update && apt-get install -y --no-install-recommends pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*
WORKDIR /build
COPY backend/Cargo.toml backend/Cargo.lock ./
COPY backend/src ./src
RUN --mount=type=cache,target=/usr/local/cargo/registry --mount=type=cache,target=/usr/local/cargo/git --mount=type=cache,target=/build/target \
    cargo build --release --locked && cp target/release/bucket /usr/local/bin/bucket

FROM debian:bookworm-slim AS production
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates libssl3 curl && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 10001 bucket && useradd --uid 10001 --gid 10001 --no-create-home bucket \
    && mkdir /data && chown bucket:bucket /data && chmod 700 /data
COPY --from=rust /usr/local/bin/bucket /usr/local/bin/bucket
COPY --from=web /build/frontend/dist /app/static
USER 10001:10001
WORKDIR /app
ENV BUCKET_BIND=0.0.0.0:8080 BUCKET_DATABASE=/data/bucket.db BUCKET_FRONTEND_DIR=/app/static RUST_LOG=bucket=info
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 CMD curl --fail --silent http://127.0.0.1:8080/api/health || exit 1
STOPSIGNAL SIGTERM
ENTRYPOINT ["/usr/local/bin/bucket"]
