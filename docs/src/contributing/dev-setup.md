# Development Setup

## Prerequisites

| Tool | Version | Install |
|------|---------|---------|
| Rust | ≥ 1.75 | `rustup update stable` |
| cargo-nextest | latest | `cargo install cargo-nextest` |
| mdBook | latest | `cargo install mdbook` |
| libopus | system | see [Installation](../getting-started/installation.md) |

## Clone and build

```bash
git clone https://github.com/NaturalDevCR/Sonium
cd sonium
cargo build
```

## Run the test suite

```bash
# All tests, all crates
cargo test --workspace

# Faster with nextest
cargo nextest run --workspace

# A single crate
cargo test -p sonium-protocol
cargo test -p sonium-sync
```

## Linting

```bash
cargo clippy --workspace -- -D warnings
cargo fmt --check
```

CI runs both on every pull request.

## Build the docs locally

```bash
cd docs
mdbook serve --open
```

The docs are served at `http://localhost:3000` with live-reload.

## Project conventions

### Commit messages

```
feat(protocol): add Hello message round-trip test
fix(sync): prevent negative sample count in SyncBuffer
docs: add PTP architecture section
chore: bump tokio to 1.36
```

### Adding a new message type

1. Create `crates/protocol/src/messages/<name>.rs` implementing `decode` / `encode`.
2. Add a variant to `Message` in `messages/mod.rs`.
3. Add the type to `MessageType` in `header.rs`.
4. Write at least: round-trip test, truncated-payload error test.
5. Document the payload format in `docs/src/reference/protocol.md`.

### Adding a new codec

1. Add a struct in `crates/codec/src/<codec>.rs` implementing `Encoder` and/or `Decoder`.
2. Wire it into `make_encoder` / `make_decoder` in `crates/codec/src/lib.rs`.
3. Add round-trip tests.

## Architecture decisions

Key decisions are recorded in `docs/src/architecture/`.  When making a
non-trivial design change, update the relevant doc before opening a PR.
