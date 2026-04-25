//! HTTP/WebSocket control server — serves the REST API and the embedded web UI.

use std::sync::Arc;
use axum::{Router, routing::get};
use tracing::info;

use sonium_control::{ServerState, api};

/// Start the HTTP control server on `port`.
///
/// Serves:
/// - `/api/*`   — REST control API (see [`api::router`])
/// - `/api/events` — WebSocket event stream
/// - `/`         — Embedded SvelteKit SPA (served from `web/dist/` or embedded binary)
pub async fn run(state: Arc<ServerState>, port: u16) -> anyhow::Result<()> {
    let app = Router::new()
        .nest("/api", api::router(state))
        .route("/health", get(|| async { "ok" }));
        // TODO Fase 7: serve embedded SPA via rust-embed

    let addr = format!("0.0.0.0:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    info!("Control API listening on http://{addr}");
    axum::serve(listener, app).await?;
    Ok(())
}
