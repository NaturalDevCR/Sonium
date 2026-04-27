//! REST handlers for reading and writing `sonium.toml`.
//!
//! Routes (admin-only):
//!   GET  /api/config/raw   — return current TOML as text
//!   PUT  /api/config/raw   — write new TOML (validates before saving)

use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    Router,
    extract::{Request, State},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};

use crate::auth::{Claims, UserStore};
use crate::auth_api::AuthUser;

#[derive(Clone)]
pub struct ConfigApiState {
    pub config_path: PathBuf,
    pub auth:        Arc<UserStore>,
}

pub fn router(state: ConfigApiState) -> Router {
    Router::new()
        .route("/config/raw",      get(get_config_raw).put(put_config_raw))
        .route("/config/validate", post(post_config_validate))
        .layer(middleware::from_fn_with_state(state.auth.clone(), require_admin_auth))
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
        None    => (StatusCode::UNAUTHORIZED, "missing or invalid token").into_response(),
    }
}

async fn post_config_validate(body: String) -> Response {
    if let Err(e) = toml::from_str::<toml::Value>(&body) {
        return (StatusCode::UNPROCESSABLE_ENTITY, format!("invalid TOML: {e}")).into_response();
    }
    if let Err(e) = toml::from_str::<sonium_common::config::ServerConfig>(&body) {
        return (StatusCode::UNPROCESSABLE_ENTITY, format!("config error: {e}")).into_response();
    }
    StatusCode::NO_CONTENT.into_response()
}

async fn get_config_raw(State(s): State<ConfigApiState>) -> Response {
    match std::fs::read_to_string(&s.config_path) {
        Ok(content) => (
            [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
            content,
        ).into_response(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            (StatusCode::NOT_FOUND, "no config file found (server is using defaults)").into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn put_config_raw(
    State(s): State<ConfigApiState>,
    body: String,
) -> Response {
    // Validate TOML before saving.
    if let Err(e) = toml::from_str::<toml::Value>(&body) {
        return (StatusCode::UNPROCESSABLE_ENTITY, format!("invalid TOML: {e}")).into_response();
    }
    // Also validate against ServerConfig shape.
    if let Err(e) = toml::from_str::<sonium_common::config::ServerConfig>(&body) {
        return (StatusCode::UNPROCESSABLE_ENTITY, format!("config error: {e}")).into_response();
    }

    match std::fs::write(&s.config_path, &body) {
        Ok(_) => {
            tracing::info!(path = %s.config_path.display(), "Config file updated via API — restart to apply stream/port changes");
            StatusCode::NO_CONTENT.into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}
