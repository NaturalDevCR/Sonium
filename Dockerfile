# ── Stage 1: builder ─────────────────────────────────────────────────────────
FROM rust:1.81-alpine AS builder

RUN apk add --no-cache musl-dev pkgconf

WORKDIR /build
COPY . .

# Build only the server binary (client needs CPAL/audio device access, not suitable for containers)
RUN cargo build --release --bin sonium-server

# ── Stage 2: runtime ──────────────────────────────────────────────────────────
FROM alpine:3.20

RUN apk add --no-cache ca-certificates

COPY --from=builder /build/target/release/sonium-server /usr/local/bin/sonium-server

# Audio stream port + HTTP control/web UI port
EXPOSE 1710 1711

# Config directory — mount a host volume here
VOLUME ["/etc/sonium"]

CMD ["sonium-server", "--config", "/etc/sonium/sonium.toml"]
