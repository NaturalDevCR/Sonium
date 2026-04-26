//! # sonium-control
//!
//! Server-side state management, REST/WebSocket control API, and client
//! discovery.
//!
//! ## Responsibilities
//!
//! - **[`ServerState`]** — in-memory registry of connected clients, groups,
//!   and active streams.  Thread-safe; shared via `Arc<ServerState>`.
//! - **[`api`]** — `axum` HTTP handlers for the REST control API.
//! - **[`ws`]** — WebSocket event broadcaster (real-time push to web UI).
//! - **[`discovery`]** — mDNS advertisement + subnet scanner for finding
//!   Sonium clients and servers on the network.

pub mod state;
pub mod api;
pub mod ws;
pub mod discovery;

pub use state::ServerState;
pub use ws::EventBus;
