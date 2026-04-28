//! REST handlers for authentication and user management.
//!
//! Routes:
//!   POST   /api/auth/login          — returns JWT (no auth required)
//!   POST   /api/auth/setup          — creates first admin (only if no users)
//!   POST   /api/auth/logout         — revokes the current token
//!   GET    /api/auth/me             — current user info (any auth)
//!   GET    /api/users               — list users (admin)
//!   POST   /api/users               — create user (admin)
//!   PUT    /api/users/:id           — update role / password (admin, or own password)
//!   DELETE /api/users/:id           — delete user (admin)

use std::sync::Arc;

use axum::{
    extract::{Path, Request, State},
    http::{header, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Json, Response},
    routing::{get, post, put},
    Router,
};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::auth::{Claims, Role, UserStore, UserView};

// ── Shared state ──────────────────────────────────────────────────────────

pub type AuthState = Arc<UserStore>;

// ── Router ────────────────────────────────────────────────────────────────

pub fn router(store: AuthState) -> Router {
    let protected = Router::new()
        .route("/auth/me", get(get_me))
        .route("/auth/logout", post(logout))
        .route("/users", get(list_users).post(create_user))
        .route("/users/:id", put(update_user).delete(delete_user))
        .layer(middleware::from_fn_with_state(store.clone(), require_auth));

    Router::new()
        .route("/auth/login", post(login))
        .route("/auth/setup", post(setup))
        .merge(protected)
        .with_state(store)
}

// ── JWT extractor ─────────────────────────────────────────────────────────

/// Axum request extension populated by `require_auth` — the validated JWT claims.
#[derive(Clone)]
pub struct AuthUser(pub Claims);

/// Axum request extension populated by `require_auth` — the raw token string.
/// Used by the logout handler to revoke the caller's own token.
#[derive(Clone)]
pub struct RawToken(pub String);

async fn require_auth(State(store): State<AuthState>, mut req: Request, next: Next) -> Response {
    let token = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "));

    let token_owned = token.map(str::to_owned);
    match token_owned {
        Some(t) => match store.verify_token(&t) {
            Some(claims) => {
                req.extensions_mut().insert(AuthUser(claims));
                req.extensions_mut().insert(RawToken(t));
                next.run(req).await
            }
            None => (StatusCode::UNAUTHORIZED, "invalid or missing token").into_response(),
        },
        None => (StatusCode::UNAUTHORIZED, "missing authorization header").into_response(),
    }
}

#[allow(clippy::result_large_err)]
fn require_admin(user: &AuthUser) -> Result<(), Response> {
    if user.0.role == "admin" {
        Ok(())
    } else {
        Err((StatusCode::FORBIDDEN, "admin role required").into_response())
    }
}

// ── Handlers ──────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct LoginBody {
    username: String,
    password: String,
}

#[derive(Serialize)]
struct LoginResponse {
    token: String,
    user: UserInfo,
}

#[derive(Serialize)]
struct UserInfo {
    id: String,
    username: String,
    role: Role,
    must_change_password: bool,
}

#[derive(Serialize)]
struct TokenResponse {
    token: String,
    user: UserView,
}

async fn login(State(store): State<AuthState>, Json(body): Json<LoginBody>) -> Response {
    if let Some(user) = store.authenticate(&body.username, &body.password) {
        let token = store.create_token(&user, 24);

        info!("User logged in: {} ({})", user.username, user.role);

        Json(LoginResponse {
            token,
            user: UserInfo {
                id: user.id,
                username: user.username,
                role: user.role,
                must_change_password: user.must_change_password,
            },
        })
        .into_response()
    } else {
        warn!("Failed login attempt for username: '{}'", body.username);
        (StatusCode::UNAUTHORIZED, "invalid credentials").into_response()
    }
}

#[derive(Deserialize)]
struct SetupBody {
    username: String,
    password: String,
}

async fn setup(State(store): State<AuthState>, Json(body): Json<SetupBody>) -> Response {
    if !store.is_setup_needed() {
        return (StatusCode::CONFLICT, "already initialized").into_response();
    }
    match store.create_user(&body.username, &body.password, Role::Admin) {
        Some(user) => {
            let full_user = store.authenticate(&body.username, &body.password).unwrap();
            let token = store.create_token(&full_user, 24);
            (StatusCode::CREATED, Json(TokenResponse { token, user })).into_response()
        }
        None => (StatusCode::CONFLICT, "username taken").into_response(),
    }
}

async fn get_me(
    State(store): State<AuthState>,
    axum::Extension(user): axum::Extension<AuthUser>,
) -> Response {
    if let Some(u) = store.get_user(&user.0.sub) {
        Json(serde_json::json!({
            "id":       u.id,
            "username": u.username,
            "role":     u.role,
            "must_change_password": u.must_change_password,
        }))
        .into_response()
    } else {
        (StatusCode::NOT_FOUND, "user not found").into_response()
    }
}

async fn logout(
    axum::Extension(raw): axum::Extension<RawToken>,
    State(store): State<AuthState>,
) -> StatusCode {
    store.revoke_token(&raw.0);
    StatusCode::NO_CONTENT
}

async fn list_users(
    axum::Extension(caller): axum::Extension<AuthUser>,
    State(store): State<AuthState>,
) -> Response {
    if let Err(e) = require_admin(&caller) {
        return e;
    }
    Json(store.all_users()).into_response()
}

#[derive(Deserialize)]
struct CreateUserBody {
    username: String,
    password: String,
    role: Role,
}

async fn create_user(
    axum::Extension(caller): axum::Extension<AuthUser>,
    State(store): State<AuthState>,
    Json(body): Json<CreateUserBody>,
) -> Response {
    if let Err(e) = require_admin(&caller) {
        return e;
    }
    match store.create_user(&body.username, &body.password, body.role) {
        Some(u) => (StatusCode::CREATED, Json(u)).into_response(),
        None => (StatusCode::CONFLICT, "username already taken").into_response(),
    }
}

#[derive(Deserialize)]
struct UpdateUserBody {
    role: Option<Role>,
    password: Option<String>,
}

async fn update_user(
    axum::Extension(caller): axum::Extension<AuthUser>,
    State(store): State<AuthState>,
    Path(id): Path<String>,
    Json(body): Json<UpdateUserBody>,
) -> Response {
    // Admin can update anyone; non-admin can only update their own password.
    let is_admin = caller.0.role == "admin";
    let is_self = caller.0.sub == id;

    if !is_admin && !is_self {
        return (StatusCode::FORBIDDEN, "cannot update other users").into_response();
    }
    // Non-admin cannot change roles.
    if !is_admin && body.role.is_some() {
        return (StatusCode::FORBIDDEN, "role change requires admin").into_response();
    }

    if store.update_user(&id, body.role, body.password.as_deref()) {
        StatusCode::NO_CONTENT.into_response()
    } else {
        (StatusCode::NOT_FOUND, "user not found").into_response()
    }
}

async fn delete_user(
    axum::Extension(caller): axum::Extension<AuthUser>,
    State(store): State<AuthState>,
    Path(id): Path<String>,
) -> Response {
    if let Err(e) = require_admin(&caller) {
        return e;
    }
    if caller.0.sub == id {
        return (StatusCode::BAD_REQUEST, "cannot delete your own account").into_response();
    }
    if store.delete_user(&id) {
        StatusCode::NO_CONTENT.into_response()
    } else {
        (StatusCode::NOT_FOUND, "user not found or last admin").into_response()
    }
}
