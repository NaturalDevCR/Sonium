//! REST handlers for reading and writing `sonium.toml`.
//!
//! Routes (admin-only):
//!   GET  /api/config/raw   — return current TOML as text
//!   PUT  /api/config/raw   — write new TOML (validates before saving)

use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    extract::{Request, State},
    http::{header, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Router,
};
use serde::Serialize;
use tokio::sync::{mpsc, oneshot};

use crate::auth::{Claims, UserStore};
use crate::auth_api::AuthUser;

#[derive(Clone)]
pub struct ConfigApiState {
    pub config_path: PathBuf,
    pub auth: Arc<UserStore>,
    pub reload_tx: Option<mpsc::Sender<ReloadRequest>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConfigReloadReport {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub restarted: Vec<String>,
    pub unchanged: Vec<String>,
    pub restart_required: Vec<String>,
}

pub struct ReloadRequest {
    pub respond_to: oneshot::Sender<Result<ConfigReloadReport, String>>,
}

pub fn router(state: ConfigApiState) -> Router {
    Router::new()
        .route("/config/raw", get(get_config_raw).put(put_config_raw))
        .route("/config/validate", post(post_config_validate))
        .route("/config/reload", post(post_config_reload))
        .layer(middleware::from_fn_with_state(
            state.auth.clone(),
            require_admin_auth,
        ))
        .with_state(state)
}

async fn require_admin_auth(
    State(store): State<Arc<UserStore>>,
    mut req: Request,
    next: Next,
) -> Response {
    let token = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "));

    let claims: Option<Claims> = token.and_then(|t| store.verify_token(t));
    match claims {
        Some(c) if c.role == "admin" => {
            req.extensions_mut().insert(AuthUser(c));
            next.run(req).await
        }
        Some(_) => (StatusCode::FORBIDDEN, "admin role required").into_response(),
        None => (StatusCode::UNAUTHORIZED, "missing or invalid token").into_response(),
    }
}

async fn post_config_validate(body: String) -> Response {
    if let Err(e) = toml::from_str::<toml::Value>(&body) {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            format!("invalid TOML: {e}"),
        )
            .into_response();
    }
    if let Err(e) = toml::from_str::<sonium_common::config::ServerConfig>(&body) {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            format!("config error: {e}"),
        )
            .into_response();
    }
    StatusCode::NO_CONTENT.into_response()
}

async fn get_config_raw(State(s): State<ConfigApiState>) -> Response {
    match std::fs::read_to_string(&s.config_path) {
        Ok(content) => (
            [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
            content,
        )
            .into_response(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (
            StatusCode::NOT_FOUND,
            "no config file found (server is using defaults)",
        )
            .into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn put_config_raw(State(s): State<ConfigApiState>, body: String) -> Response {
    // Validate TOML before saving.
    if let Err(e) = toml::from_str::<toml::Value>(&body) {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            format!("invalid TOML: {e}"),
        )
            .into_response();
    }
    // Also validate against ServerConfig shape.
    if let Err(e) = toml::from_str::<sonium_common::config::ServerConfig>(&body) {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            format!("config error: {e}"),
        )
            .into_response();
    }

    match std::fs::write(&s.config_path, &body) {
        Ok(_) => {
            tracing::info!(path = %s.config_path.display(), "Config file updated via API");
            StatusCode::NO_CONTENT.into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn post_config_reload(State(s): State<ConfigApiState>) -> Response {
    let Some(tx) = &s.reload_tx else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "config hot reload is disabled; restart Sonium server to apply changes",
        )
            .into_response();
    };

    let (respond_to, response) = oneshot::channel();
    if tx.send(ReloadRequest { respond_to }).await.is_err() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "config reload worker is not running",
        )
            .into_response();
    }

    match response.await {
        Ok(Ok(report)) => axum::Json(report).into_response(),
        Ok(Err(e)) => (StatusCode::UNPROCESSABLE_ENTITY, e).into_response(),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "config reload worker dropped response",
        )
            .into_response(),
    }
}
