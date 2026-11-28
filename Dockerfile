# syntax=docker/dockerfile:1.7

# Stage 1: Build the React/Vite frontend bundle
FROM oven/bun:1.4-alpine AS frontend-builder
WORKDIR /app/frontend

COPY frontend/package.json frontend/bun.lock ./
RUN bun install --frozen-lockfile

COPY frontend/ ./
RUN bun run build

# Stage 2: Build the single Rust release binary with embedded frontend assets
FROM rust:1.92-bookworm AS backend-builder
WORKDIR /app

# Pre-compile Cargo dependencies for Docker layer caching
COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src \
    && printf 'fn main() {}\n' > src/main.rs \
    && printf '' > src/lib.rs \
    && cargo build --release --locked \
    && rm -rf src target/release/bunamateur* target/release/deps/bunamateur* target/release/deps/libbunamateur*

COPY src ./src
COPY migrations ./migrations
COPY content ./content
COPY --from=frontend-builder /app/frontend/dist ./frontend/dist

RUN cargo build --release --locked

# Stage 3: Minimal production runtime image
FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --system bunamateur \
    && useradd --system --gid bunamateur --home-dir /nonexistent --shell /usr/sbin/nologin bunamateur

COPY --from=backend-builder /app/target/release/bunamateur /usr/local/bin/bunamateur

USER bunamateur

ENV HOST=0.0.0.0 \
    PORT=8080 \
    LOG_FORMAT=json

EXPOSE 8080

HEALTHCHECK --interval=30s --timeout=5s --start-period=15s --retries=3 \
    CMD curl -fsS http://127.0.0.1:8080/healthz || exit 1

CMD ["/usr/local/bin/bunamateur"]
