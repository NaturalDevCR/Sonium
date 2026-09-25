# ── Stage 1: web UI (embedded into the server binary) ─────────────────────────
FROM node:20-alpine AS web

RUN npm install -g pnpm@9

WORKDIR /web
COPY web/package.json web/pnpm-lock.yaml ./
RUN pnpm install --frozen-lockfile
COPY web/ ./
RUN pnpm build

# ── Stage 2: server binary ────────────────────────────────────────────────────
FROM rust:1-alpine AS builder

RUN apk add --no-cache musl-dev pkgconf opus-dev cmake make g++

# Link libopus dynamically (musl targets default to fully static binaries).
ENV RUSTFLAGS="-C target-feature=-crt-static"

WORKDIR /build
COPY . .
COPY --from=web /web/dist ./web/dist

# Build only the server binary (client needs CPAL/audio device access, not suitable for containers)
RUN cargo build --release --locked --bin sonium-server

# ── Stage 3: runtime ──────────────────────────────────────────────────────────
FROM alpine:3.20

# ffmpeg feeds pipe:// sources and decodes announcement/TTS URLs.
RUN apk add --no-cache ca-certificates libgcc opus ffmpeg

COPY --from=builder /build/target/release/sonium-server /usr/local/bin/sonium-server

# Audio stream port + HTTP control/web UI port
EXPOSE 1710 1711

# Config directory — mount a host volume here
VOLUME ["/etc/sonium"]

CMD ["sonium-server", "--config", "/etc/sonium/sonium.toml"]
