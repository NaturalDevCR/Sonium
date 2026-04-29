//! `axum` REST handlers for the control API.
//!
//! Mount with [`router`] inside the server's `axum` application.
//! All handlers share [`AppState`] via `axum::extract::State`.

use crate::auth::UserStore;
use crate::auth_api::AuthUser;
use crate::state::ServerState;
use axum::{
    extract::{Path, Query, Request, State, WebSocketUpgrade},
    http::{header, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Json, Response},
    routing::{delete, get, patch, post},
    Extension, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Shared state injected by `axum`.
pub type AppState = Arc<ServerState>;

/// Build the full REST + WebSocket router.
///
/// Mount this at `/api`:
/// ```rust,ignore
/// let app = Router::new()
///     .nest("/api", sonium_control::api::router(state.clone()))
///     .fallback(serve_spa);
/// ```
pub fn router(state: AppState) -> Router {
    // Any authenticated user (viewer+)
    let read_routes = Router::new()
        .route("/status", get(get_status))
        .route("/clients", get(get_clients))
        .route("/groups", get(get_groups))
        .route("/streams", get(get_streams))
        .route("/events", get(ws_handler)) // WS: also accepts ?token=
        .layer(middleware::from_fn(require_viewer));

    // Operator or admin only
    let write_routes = Router::new()
        .route("/clients/:id/volume", patch(patch_volume))
        .route("/clients/:id/latency", patch(patch_latency))
        .route("/clients/:id/observability", patch(patch_observability))
        .route("/clients/:id/group", patch(patch_client_group))
        .route("/clients/:id/name", patch(patch_client_name))
        .route("/streams/:id/eq", patch(patch_stream_eq))
        .route("/clients/:id", delete(delete_client))
        .route("/groups", post(post_group))
        .route("/groups/:id", delete(delete_group))
        .route("/groups/:id", patch(patch_group))
        .route("/groups/:id/stream", patch(patch_group_stream))
        .route("/discover/scan", get(get_discover_scan))
        .route("/discover/local-subnet", get(get_discover_local_subnet))
        .layer(middleware::from_fn(require_operator));

    Router::new()
        .merge(read_routes)
        .merge(write_routes)
        .with_state(state)
}

// ── Auth middleware ───────────────────────────────────────────────────────

fn extract_token(req: &Request) -> Option<String> {
    // 1. Authorization: Bearer <token>
    req.headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .map(String::from)
        // 2. ?token= query param (required for WebSocket — browsers can't set WS headers)
        .or_else(|| {
            req.uri().query()?.split('&').find_map(|pair| {
                let (k, v) = pair.split_once('=')?;
                (k == "token").then(|| v.to_owned())
            })
        })
}

async fn require_viewer(
    Extension(auth): Extension<Arc<UserStore>>,
    mut req: Request,
    next: Next,
) -> Response {
    match extract_token(&req)
        .as_deref()
        .and_then(|t| auth.verify_token(t))
    {
        Some(claims) => {
            req.extensions_mut().insert(AuthUser(claims));
            next.run(req).await
        }
        None => (StatusCode::UNAUTHORIZED, "missing or invalid token").into_response(),
    }
}

async fn require_operator(
    Extension(auth): Extension<Arc<UserStore>>,
    mut req: Request,
    next: Next,
) -> Response {
    match extract_token(&req)
        .as_deref()
        .and_then(|t| auth.verify_token(t))
    {
        Some(claims) if matches!(claims.role.as_str(), "admin" | "operator") => {
            req.extensions_mut().insert(AuthUser(claims));
            next.run(req).await
        }
        Some(_) => (StatusCode::FORBIDDEN, "operator or admin role required").into_response(),
        None => (StatusCode::UNAUTHORIZED, "missing or invalid token").into_response(),
    }
}

// ── Status ────────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct StatusResponse {
    version: &'static str,
    uptime_s: i64,
    clients: usize,
    groups: usize,
    streams: usize,
}

async fn get_status(State(s): State<AppState>) -> Json<StatusResponse> {
    Json(StatusResponse {
        version: env!("CARGO_PKG_VERSION"),
        uptime_s: s.uptime_secs(),
        clients: s.all_clients().len(),
        groups: s.all_groups().len(),
        streams: s.all_streams().len(),
    })
}

// ── Clients ───────────────────────────────────────────────────────────────

async fn get_clients(State(s): State<AppState>) -> impl IntoResponse {
    Json(s.all_clients())
}

#[derive(Deserialize)]
struct VolumeBody {
    volume: u8,
    muted: bool,
}

async fn patch_volume(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<VolumeBody>,
) -> Response {
    match s.set_volume(&id, body.volume, body.muted) {
        Some(_) => StatusCode::NO_CONTENT.into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

#[derive(Deserialize)]
struct LatencyBody {
    latency_ms: i32,
}

async fn patch_latency(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<LatencyBody>,
) -> Response {
    if s.set_latency(&id, body.latency_ms) {
        StatusCode::NO_CONTENT.into_response()
    } else {
        StatusCode::NOT_FOUND.into_response()
    }
}

#[derive(Deserialize)]
struct ObservabilityBody {
    enabled: bool,
}

async fn patch_observability(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<ObservabilityBody>,
) -> Response {
    if s.set_client_observability(&id, body.enabled) {
        StatusCode::NO_CONTENT.into_response()
    } else {
        StatusCode::NOT_FOUND.into_response()
    }
}

#[derive(Deserialize)]
struct GroupAssignBody {
    group_id: String,
}

async fn patch_client_group(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<GroupAssignBody>,
) -> Response {
    if s.set_client_group(&id, &body.group_id) {
        StatusCode::NO_CONTENT.into_response()
    } else {
        (StatusCode::NOT_FOUND, "client or group not found").into_response()
    }
}

#[derive(Deserialize)]
struct ClientNameBody {
    display_name: Option<String>,
}

async fn patch_client_name(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<ClientNameBody>,
) -> Response {
    if s.set_client_name(&id, body.display_name) {
        StatusCode::NO_CONTENT.into_response()
    } else {
        StatusCode::NOT_FOUND.into_response()
    }
}

#[derive(Deserialize)]
struct EqBody {
    bands: Vec<sonium_protocol::messages::EqBand>,
    enabled: bool,
}

async fn patch_stream_eq(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<EqBody>,
) -> Response {
    if s.set_eq(&id, body.bands, body.enabled) {
        StatusCode::NO_CONTENT.into_response()
    } else {
        StatusCode::NOT_FOUND.into_response()
    }
}

async fn delete_client(State(s): State<AppState>, Path(id): Path<String>) -> Response {
    if s.delete_client(&id) {
        StatusCode::NO_CONTENT.into_response()
    } else {
        (StatusCode::NOT_FOUND, "client not found or still connected").into_response()
    }
}

// ── Groups ────────────────────────────────────────────────────────────────

async fn get_groups(State(s): State<AppState>) -> impl IntoResponse {
    Json(s.all_groups())
}

#[derive(Deserialize)]
struct CreateGroupBody {
    name: String,
    stream_id: String,
}

#[derive(Serialize)]
struct CreateGroupResponse {
    id: String,
}

async fn post_group(
    State(s): State<AppState>,
    Json(body): Json<CreateGroupBody>,
) -> impl IntoResponse {
    let id = s.create_group(body.name, body.stream_id);
    (StatusCode::CREATED, Json(CreateGroupResponse { id }))
}

async fn delete_group(State(s): State<AppState>, Path(id): Path<String>) -> Response {
    if s.delete_group(&id) {
        StatusCode::NO_CONTENT.into_response()
    } else {
        (StatusCode::NOT_FOUND, "group not found or protected").into_response()
    }
}

#[derive(Deserialize)]
struct RenameGroupBody {
    name: String,
}

async fn patch_group(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<RenameGroupBody>,
) -> Response {
    if s.rename_group(&id, body.name) {
        StatusCode::NO_CONTENT.into_response()
    } else {
        (StatusCode::NOT_FOUND, "group not found").into_response()
    }
}

#[derive(Deserialize)]
struct StreamAssignBody {
    stream_id: String,
}

async fn patch_group_stream(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<StreamAssignBody>,
) -> Response {
    if s.set_group_stream(&id, &body.stream_id) {
        StatusCode::NO_CONTENT.into_response()
    } else {
        (StatusCode::NOT_FOUND, "group or stream not found").into_response()
    }
}

// ── Discovery ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct ScanQuery {
    cidr: String,
    #[serde(default = "default_scan_port")]
    port: u16,
}

fn default_scan_port() -> u16 {
    1710
}

async fn get_discover_scan(Query(q): Query<ScanQuery>) -> impl IntoResponse {
    let results = crate::discovery::scan_subnet(&q.cidr, q.port, 64).await;
    Json(results)
}

#[derive(Serialize)]
struct LocalSubnetResponse {
    cidr: Option<String>,
}

async fn get_discover_local_subnet() -> impl IntoResponse {
    Json(LocalSubnetResponse {
        cidr: crate::discovery::local_ipv4_subnet(),
    })
}

// ── Streams ───────────────────────────────────────────────────────────────

async fn get_streams(State(s): State<AppState>) -> impl IntoResponse {
    Json(s.all_streams())
}

// ── WebSocket events ──────────────────────────────────────────────────────

async fn ws_handler(ws: WebSocketUpgrade, State(s): State<AppState>) -> Response {
    ws.on_upgrade(move |socket| handle_ws(socket, s))
}

async fn handle_ws(mut socket: axum::extract::ws::WebSocket, state: AppState) {
    use axum::extract::ws::Message as WsMsg;
    let mut rx = state.events().subscribe();

    loop {
        tokio::select! {
            event = rx.recv() => {
                match event {
                    Ok(ev) => {
                        if let Ok(json) = serde_json::to_string(&ev) {
                            if socket.send(WsMsg::Text(json)).await.is_err() {
                                break;
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!("WS client lagged, dropped {n} events");
                    }
                    Err(_) => break,
                }
            }
            msg = socket.recv() => {
                // Close or error from client
                if msg.is_none() { break; }
            }
        }
    }
}
