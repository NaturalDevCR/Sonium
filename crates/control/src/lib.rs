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

pub mod api;
pub mod auth;
pub mod auth_api;
pub mod config_api;
pub mod discovery;
pub mod persistence;
pub mod state;
pub mod system_api;
pub mod ws;

pub use auth::UserStore;
pub use persistence::PersistenceStore;
pub use state::ServerState;
pub use ws::EventBus;
