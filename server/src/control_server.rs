use axum::{
    http::{header, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use rust_embed::RustEmbed;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::info;

use crate::metrics;
use sonium_control::{api, auth_api, config_api, system_api, ServerState, UserStore};

/// Embedded web UI — built from `web/dist/` at compile time.
#[derive(RustEmbed)]
#[folder = "../web/dist/"]
struct WebAssets;

pub async fn run(
    state: Arc<ServerState>,
    auth: Arc<UserStore>,
    config_path: PathBuf,
    reload_tx: Option<mpsc::Sender<config_api::ReloadRequest>>,
    port: u16,
) -> anyhow::Result<()> {
    let config_state = config_api::ConfigApiState {
        config_path,
        auth: auth.clone(),
        reload_tx,
    };

    let app = Router::new()
        .nest("/api", api::router(state))
        .nest("/api", auth_api::router(auth.clone()))
        .nest("/api", config_api::router(config_state))
        .nest("/api", system_api::router(auth.clone()))
        .route("/health", get(|| async { "ok" }))
        .route("/metrics", get(metrics_handler))
        .fallback(spa_handler)
        // Make UserStore available as Extension to all nested routers,
        // including the api::router middleware that guards read/write endpoints.
        .layer(axum::Extension(auth));

    let addr = format!("0.0.0.0:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    info!("Control API + Web UI on http://{addr}");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn metrics_handler() -> Response {
    let body = metrics::gather();
    (
        [(
            header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        body,
    )
        .into_response()
}

async fn spa_handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    if path == "api" || path.starts_with("api/") {
        return (StatusCode::NOT_FOUND, "API route not found").into_response();
    }
    let path = if path.is_empty() { "index.html" } else { path };

    match WebAssets::get(path) {
        Some(file) => {
            let mime = mime_guess::from_path(path)
                .first_or_octet_stream()
                .to_string();
            ([(header::CONTENT_TYPE, mime)], file.data).into_response()
        }
        None => {
            // SPA fallback — Vue Router handles client-side routing.
            match WebAssets::get("index.html") {
                Some(index) => (
                    [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
                    index.data,
                )
                    .into_response(),
                None => (StatusCode::NOT_FOUND, "index.html not found").into_response(),
            }
        }
    }
}
