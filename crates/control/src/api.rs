//! `axum` REST handlers for the control API.
//!
//! Mount with [`router`] inside the server's `axum` application.
//! All handlers share [`AppState`] via `axum::extract::State`.

use crate::auth::{UserStore, WsTicketIssueError};
use crate::auth_api::{AuthUser, RawToken};
use crate::state::ServerState;
use axum::{
    extract::{Path, Query, Request, State, WebSocketUpgrade},
    http::{header, HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Json, Response},
    routing::{delete, get, patch, post},
    Extension, Router,
};
use serde::{Deserialize, Serialize};
use sonium_transport::TransportMode;
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
        .route("/events/ticket", post(post_ws_ticket))
        .layer(middleware::from_fn(require_viewer));

    // Browsers cannot attach Authorization headers to WebSocket upgrades, so
    // they first exchange the JWT for a short-lived, one-use subprotocol ticket.
    let ws_routes = Router::new().route("/events", get(ws_handler));

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
        .route("/server/transport", get(get_transport))
        .route("/server/transport", patch(patch_transport))
        .route("/discover/scan", get(get_discover_scan))
        .route("/discover/local-subnet", get(get_discover_local_subnet))
        .layer(middleware::from_fn(require_operator));

    Router::new()
        .merge(read_routes)
        .merge(ws_routes)
        .merge(write_routes)
        .with_state(state)
}

// ── Auth middleware ───────────────────────────────────────────────────────

fn extract_token(req: &Request) -> Option<String> {
    req.headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .map(String::from)
}

async fn require_viewer(
    Extension(auth): Extension<Arc<UserStore>>,
    mut req: Request,
    next: Next,
) -> Response {
    let token = extract_token(&req);
    match token.as_deref().and_then(|t| auth.verify_token(t)) {
        Some(claims) => {
            req.extensions_mut()
                .insert(RawToken(token.expect("token just verified")));
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

// ── Transport ─────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct TransportResponse {
    mode: String,
    server_udp_port: u16,
}

async fn get_transport(State(s): State<AppState>) -> impl IntoResponse {
    Json(TransportResponse {
        mode: s.transport_mode().to_string(),
        server_udp_port: s.server_udp_port(),
    })
}

#[derive(Deserialize)]
struct PatchTransportBody {
    mode: String,
}

async fn patch_transport(
    State(s): State<AppState>,
    Json(body): Json<PatchTransportBody>,
) -> Response {
    let mode = match body.mode.as_str() {
        "tcp" => TransportMode::Tcp,
        "rtp_udp" => TransportMode::RtpUdp,
        "rist" => TransportMode::Rist,
        "quic_dgram" => TransportMode::QuicDgram,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                format!(
                    "unknown transport mode {:?}; valid: tcp, rtp_udp, rist, quic_dgram",
                    body.mode
                ),
            )
                .into_response()
        }
    };
    s.set_transport_mode(mode);
    StatusCode::NO_CONTENT.into_response()
}

// ── Streams ───────────────────────────────────────────────────────────────

async fn get_streams(State(s): State<AppState>) -> impl IntoResponse {
    Json(s.all_streams())
}

// ── WebSocket events ──────────────────────────────────────────────────────

#[derive(Serialize)]
struct WsTicketResponse {
    ticket: String,
}

async fn post_ws_ticket(
    Extension(auth): Extension<Arc<UserStore>>,
    Extension(raw): Extension<RawToken>,
) -> Response {
    match auth.issue_ws_ticket(&raw.0) {
        Ok(ticket) => (StatusCode::CREATED, Json(WsTicketResponse { ticket })).into_response(),
        Err(WsTicketIssueError::InvalidToken) => {
            (StatusCode::UNAUTHORIZED, "invalid or expired token").into_response()
        }
        Err(WsTicketIssueError::CapacityExceeded) => (
            StatusCode::TOO_MANY_REQUESTS,
            "WebSocket ticket capacity reached; retry shortly",
        )
            .into_response(),
    }
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(s): State<AppState>,
    Extension(auth): Extension<Arc<UserStore>>,
    headers: HeaderMap,
) -> Response {
    let Some(ticket) = headers
        .get(header::SEC_WEBSOCKET_PROTOCOL)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| {
            let mut protocols = value.split(',').map(str::trim);
            let ticket = protocols.next()?;
            (protocols.next().is_none() && !ticket.is_empty()).then_some(ticket)
        })
        .map(str::to_owned)
    else {
        return (StatusCode::UNAUTHORIZED, "missing WebSocket ticket").into_response();
    };
    let Some(admitted) = auth.consume_ws_ticket(&ticket) else {
        return (
            StatusCode::UNAUTHORIZED,
            "invalid or expired WebSocket ticket",
        )
            .into_response();
    };

    ws.protocols([ticket])
        .on_upgrade(move |socket| handle_ws(socket, s, auth, admitted))
}

async fn handle_ws(
    mut socket: axum::extract::ws::WebSocket,
    state: AppState,
    auth: Arc<UserStore>,
    admitted: crate::auth::WsTicketClaims,
) {
    use axum::extract::ws::Message as WsMsg;
    use tokio::time::Duration;

    let mut rx = state.events().subscribe();
    let mut session_check = tokio::time::interval(Duration::from_secs(1));
    session_check.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            _ = session_check.tick() => {
                if !auth.verify_ws_ticket_claims(&admitted) {
                    let _ = socket.close().await;
                    break;
                }
            }
            event = rx.recv() => {
                match event {
                    Ok(ev) => {
                        if !auth.verify_ws_ticket_claims(&admitted) {
                            let _ = socket.close().await;
                            break;
                        }
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
