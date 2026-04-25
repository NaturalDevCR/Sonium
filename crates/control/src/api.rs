//! `axum` REST handlers for the control API.
//!
//! Mount with [`router`] inside the server's `axum` application.
//! All handlers share [`AppState`] via `axum::extract::State`.

use std::sync::Arc;
use axum::{
    extract::{Path, State, WebSocketUpgrade},
    http::StatusCode,
    response::{IntoResponse, Json, Response},
    routing::{delete, get, patch, post},
    Router,
};
use serde::{Deserialize, Serialize};
use crate::state::{ServerState, StreamStatus};

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
    Router::new()
        .route("/status",                  get(get_status))
        .route("/clients",                 get(get_clients))
        .route("/clients/:id/volume",      patch(patch_volume))
        .route("/clients/:id/latency",     patch(patch_latency))
        .route("/clients/:id/group",       patch(patch_client_group))
        .route("/groups",                  get(get_groups).post(post_group))
        .route("/groups/:id",              delete(delete_group))
        .route("/groups/:id/stream",       patch(patch_group_stream))
        .route("/streams",                 get(get_streams))
        .route("/events",                  get(ws_handler))
        .with_state(state)
}

// ── Status ────────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct StatusResponse {
    version:   &'static str,
    uptime_s:  i64,
    clients:   usize,
    groups:    usize,
    streams:   usize,
}

async fn get_status(State(s): State<AppState>) -> Json<StatusResponse> {
    Json(StatusResponse {
        version:  env!("CARGO_PKG_VERSION"),
        uptime_s: s.uptime_secs(),
        clients:  s.all_clients().len(),
        groups:   s.all_groups().len(),
        streams:  s.all_streams().len(),
    })
}

// ── Clients ───────────────────────────────────────────────────────────────

async fn get_clients(State(s): State<AppState>) -> impl IntoResponse {
    Json(s.all_clients())
}

#[derive(Deserialize)]
struct VolumeBody { volume: u8, muted: bool }

async fn patch_volume(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<VolumeBody>,
) -> Response {
    match s.set_volume(&id, body.volume, body.muted) {
        Some(_) => StatusCode::NO_CONTENT.into_response(),
        None    => StatusCode::NOT_FOUND.into_response(),
    }
}

#[derive(Deserialize)]
struct LatencyBody { latency_ms: i32 }

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
struct GroupAssignBody { group_id: String }

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

// ── Groups ────────────────────────────────────────────────────────────────

async fn get_groups(State(s): State<AppState>) -> impl IntoResponse {
    Json(s.all_groups())
}

#[derive(Deserialize)]
struct CreateGroupBody { name: String, stream_id: String }

#[derive(Serialize)]
struct CreateGroupResponse { id: String }

async fn post_group(
    State(s): State<AppState>,
    Json(body): Json<CreateGroupBody>,
) -> impl IntoResponse {
    let id = s.create_group(body.name, body.stream_id);
    (StatusCode::CREATED, Json(CreateGroupResponse { id }))
}

async fn delete_group(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    if s.delete_group(&id) {
        StatusCode::NO_CONTENT.into_response()
    } else {
        (StatusCode::NOT_FOUND, "group not found or protected").into_response()
    }
}

#[derive(Deserialize)]
struct StreamAssignBody { stream_id: String }

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

// ── Streams ───────────────────────────────────────────────────────────────

async fn get_streams(State(s): State<AppState>) -> impl IntoResponse {
    Json(s.all_streams())
}

// ── WebSocket events ──────────────────────────────────────────────────────

async fn ws_handler(
    ws:    WebSocketUpgrade,
    State(s): State<AppState>,
) -> Response {
    ws.on_upgrade(move |socket| handle_ws(socket, s))
}

async fn handle_ws(
    mut socket: axum::extract::ws::WebSocket,
    state:      AppState,
) {
    use axum::extract::ws::Message as WsMsg;
    let mut rx = state.events().subscribe();

    loop {
        tokio::select! {
            event = rx.recv() => {
                match event {
                    Ok(ev) => {
                        if let Ok(json) = serde_json::to_string(&ev) {
                            if socket.send(WsMsg::Text(json.into())).await.is_err() {
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
