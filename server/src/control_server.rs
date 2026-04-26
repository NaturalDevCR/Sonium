use std::sync::Arc;
use axum::{
    Router,
    routing::get,
    http::{Uri, StatusCode, header},
    response::{IntoResponse, Response},
};
use rust_embed::RustEmbed;
use tracing::info;

use sonium_control::{ServerState, api};

/// Embedded web UI — built from `web/dist/` at compile time.
#[derive(RustEmbed)]
#[folder = "../web/dist/"]
struct WebAssets;

pub async fn run(state: Arc<ServerState>, port: u16) -> anyhow::Result<()> {
    let app = Router::new()
        .nest("/api", api::router(state))
        .route("/health", get(|| async { "ok" }))
        .fallback(spa_handler);

    let addr = format!("0.0.0.0:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    info!("Control API + Web UI on http://{addr}");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn spa_handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    match WebAssets::get(path) {
        Some(file) => {
            let mime = mime_guess::from_path(path)
                .first_or_octet_stream()
                .to_string();
            ([(header::CONTENT_TYPE, mime)], file.data).into_response()
        }
        None => {
            // SPA fallback: unknown paths get index.html so Vue Router handles routing.
            match WebAssets::get("index.html") {
                Some(index) => (
                    [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
                    index.data,
                ).into_response(),
                None => (StatusCode::NOT_FOUND, "index.html not found").into_response(),
            }
        }
    }
}
