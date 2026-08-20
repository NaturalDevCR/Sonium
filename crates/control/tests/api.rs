use std::net::SocketAddr;
/// Integration tests for the REST control API.
///
/// Each test builds the axum router in-process using `tower::ServiceExt::oneshot`
/// — no network or listening socket required, making these fast and hermetic.
use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::http::{header, Method, Request, StatusCode};
use serde_json::{json, Value};
use tower::ServiceExt;

use sonium_control::auth::Role;
use sonium_control::{api, EventBus, ServerState, UserStore};

// ── Helpers ───────────────────────────────────────────────────────────────

fn test_auth() -> (Arc<UserStore>, String) {
    let dir = std::env::temp_dir().join(format!("sonium-api-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let store = UserStore::load_or_init(&dir, Some("initial-admin-password".to_string())).unwrap();
    let _ = store
        .create_user("test-admin", "test-password", Role::Admin)
        .unwrap();
    let user = store.authenticate("test-admin", "test-password").unwrap();
    let token = store.create_token(&user, 1);
    (store, token)
}

fn make_app() -> (axum::Router, String) {
    let state = Arc::new(ServerState::new(
        Arc::new(EventBus::new()),
        None,
        vec![],
        vec![],
    ));
    let (auth, token) = test_auth();
    (api::router(state).layer(axum::Extension(auth)), token)
}

fn make_app_with_state() -> (axum::Router, Arc<ServerState>, String) {
    let state = Arc::new(ServerState::new(
        Arc::new(EventBus::new()),
        None,
        vec![],
        vec![],
    ));
    let (auth, token) = test_auth();
    (
        api::router(state.clone()).layer(axum::Extension(auth)),
        state,
        token,
    )
}

async fn json_body(body: Body) -> Value {
    let bytes = to_bytes(body, 1024 * 1024).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

fn get(uri: &str, token: &str) -> Request<Body> {
    Request::builder()
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap()
}

fn patch_json(uri: &str, body: Value, token: &str) -> Request<Body> {
    Request::builder()
        .method(Method::PATCH)
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

fn post_json(uri: &str, body: Value, token: &str) -> Request<Body> {
    Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

fn delete(uri: &str, token: &str) -> Request<Body> {
    Request::builder()
        .method(Method::DELETE)
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap()
}

fn post_empty(uri: &str, token: &str) -> Request<Body> {
    Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap()
}

fn ws_request(url: &str, ticket: &str) -> tokio_tungstenite::tungstenite::http::Request<()> {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    use tokio_tungstenite::tungstenite::http::{header::SEC_WEBSOCKET_PROTOCOL, HeaderValue};

    let mut request = url.into_client_request().unwrap();
    request.headers_mut().insert(
        SEC_WEBSOCKET_PROTOCOL,
        HeaderValue::from_str(ticket).unwrap(),
    );
    request
}

// ── /status ───────────────────────────────────────────────────────────────

#[tokio::test]
async fn status_ok() {
    let (app, token) = make_app();
    let res = app.oneshot(get("/status", &token)).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let json = json_body(res.into_body()).await;
    assert!(json["version"].is_string(), "version field missing");
    assert!(json["uptime_s"].is_number(), "uptime_s field missing");
}

// ── /clients ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn clients_empty_on_start() {
    let (app, token) = make_app();
    let res = app.oneshot(get("/clients", &token)).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let json = json_body(res.into_body()).await;
    assert_eq!(json, json!([]));
}

#[tokio::test]
async fn patch_volume_unknown_client_is_404() {
    let (app, token) = make_app();
    let res = app
        .oneshot(patch_json(
            "/clients/ghost/volume",
            json!({"volume": 50, "muted": false}),
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn patch_latency_unknown_client_is_404() {
    let (app, token) = make_app();
    let res = app
        .oneshot(patch_json(
            "/clients/ghost/latency",
            json!({"latency_ms": 100}),
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn patch_group_unknown_client_is_404() {
    let (app, token) = make_app();
    let res = app
        .oneshot(patch_json(
            "/clients/ghost/group",
            json!({"group_id": "default"}),
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn connected_client_appears_in_list() {
    let (app, state, token) = make_app_with_state();
    let addr: SocketAddr = "127.0.0.1:9999".parse().unwrap();
    state.client_connected("abc", "host", "Test", "linux", "x86_64", addr, 2);

    let res = app.oneshot(get("/clients", &token)).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let json = json_body(res.into_body()).await;
    let clients = json.as_array().unwrap();
    assert_eq!(clients.len(), 1);
    assert_eq!(clients[0]["id"], "abc");
    assert_eq!(clients[0]["status"], "connected");
}

#[tokio::test]
async fn patch_volume_connected_client_is_204() {
    let (app, state, token) = make_app_with_state();
    let addr: SocketAddr = "127.0.0.1:9001".parse().unwrap();
    state.client_connected("vol-test", "host", "Test", "linux", "x86_64", addr, 2);

    let res = app
        .oneshot(patch_json(
            "/clients/vol-test/volume",
            json!({"volume": 42, "muted": true}),
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let updated = state.all_clients();
    let c = updated.iter().find(|c| c.id == "vol-test").unwrap();
    assert_eq!(c.volume, 42);
    assert!(c.muted);
}

#[tokio::test]
async fn patch_eq_stream_is_204() {
    let (app, state, token) = make_app_with_state();
    // Default stream "default" exists by default.

    let bands = json!([
        {"filter_type": "peaking", "freq_hz": 100, "gain_db": 3.0, "q": 0.9},
        {"filter_type": "peaking", "freq_hz": 1000, "gain_db": -1.5, "q": 0.9},
        {"filter_type": "peaking", "freq_hz": 10000, "gain_db": 2.0, "q": 0.9}
    ]);
    let res = app
        .oneshot(patch_json(
            "/streams/default/eq",
            json!({"bands": bands, "enabled": true}),
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);

    let (eq_bands, eq_enabled) = state.get_stream_eq("default").unwrap();
    assert_eq!(eq_bands.len(), 3);
    assert_eq!(eq_bands[0].freq_hz, 100);
    assert_eq!(eq_bands[0].gain_db, 3.0);
    assert!(eq_enabled);
}

// ── /groups ───────────────────────────────────────────────────────────────

#[tokio::test]
async fn groups_has_default_on_start() {
    let (app, token) = make_app();
    let res = app.oneshot(get("/groups", &token)).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let json = json_body(res.into_body()).await;
    let groups = json.as_array().unwrap();
    assert!(!groups.is_empty(), "expected at least one group");
    assert!(
        groups.iter().any(|g| g["id"] == "default"),
        "default group missing"
    );
}

#[tokio::test]
async fn create_group_returns_201_with_id() {
    let (app, token) = make_app();
    let res = app
        .oneshot(post_json(
            "/groups",
            json!({"name": "Kitchen", "stream_id": ""}),
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let json = json_body(res.into_body()).await;
    assert!(
        json["id"].is_string(),
        "id field missing in create response"
    );
}

#[tokio::test]
async fn delete_default_group_is_forbidden() {
    let (app, token) = make_app();
    let res = app
        .oneshot(delete("/groups/default", &token))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn create_then_delete_group_ok() {
    let (app, state, token) = make_app_with_state();
    let id = state.create_group("Bedroom".to_string(), String::new());

    let res = app
        .oneshot(delete(&format!("/groups/{id}"), &token))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    assert!(!state.all_groups().iter().any(|g| g.id == id));
}

#[tokio::test]
async fn patch_group_stream_unknown_group_is_404() {
    let (app, token) = make_app();
    let res = app
        .oneshot(patch_json(
            "/groups/ghost/stream",
            json!({"stream_id": "default"}),
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

// ── /streams ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn streams_has_default_on_start() {
    let (app, token) = make_app();
    let res = app.oneshot(get("/streams", &token)).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let json = json_body(res.into_body()).await;
    let streams = json.as_array().unwrap();
    assert!(!streams.is_empty(), "expected default stream");
    assert!(
        streams.iter().any(|s| s["id"] == "default"),
        "default stream missing"
    );
    assert_eq!(streams[0]["status"], "idle");
}

// ── WS /events ────────────────────────────────────────────────────────────

async fn start_ws_test_server(
    state: Arc<ServerState>,
    auth: Arc<UserStore>,
) -> (u16, tokio::task::JoinHandle<()>) {
    use tokio::net::TcpListener;

    let app = api::router(state).layer(axum::Extension(auth));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (port, server)
}

#[tokio::test]
async fn ws_ticket_is_consumed_before_upgrade_and_cannot_be_replayed() {
    use futures_util::StreamExt;
    use tokio_tungstenite::connect_async;
    use tokio_tungstenite::tungstenite::Message;

    let state = Arc::new(ServerState::new(
        Arc::new(EventBus::new()),
        None,
        vec![],
        vec![],
    ));
    let (auth, token) = test_auth();
    let app = api::router(state.clone()).layer(axum::Extension(auth.clone()));
    let ticket_response = app
        .oneshot(post_empty("/events/ticket", &token))
        .await
        .expect("ticket endpoint responds");
    assert_eq!(ticket_response.status(), StatusCode::CREATED);
    let ticket = json_body(ticket_response.into_body()).await["ticket"]
        .as_str()
        .expect("ticket response contains a ticket")
        .to_owned();
    let (port, _server) = start_ws_test_server(state.clone(), auth).await;

    let url = format!("ws://127.0.0.1:{port}/events");
    let request = ws_request(&url, &ticket);
    let (mut ws, response) = connect_async(request)
        .await
        .expect("ticketed WS connect failed");
    assert_eq!(response.headers()["Sec-WebSocket-Protocol"], ticket);

    state
        .events()
        .emit(sonium_control::ws::Event::Heartbeat { uptime_s: 42 });
    let event = tokio::time::timeout(tokio::time::Duration::from_secs(1), ws.next())
        .await
        .expect("timeout waiting for authenticated event")
        .expect("WS closed before event")
        .expect("WS event failed");
    let Message::Text(event) = event else {
        panic!("expected text event");
    };
    assert_eq!(
        serde_json::from_str::<Value>(&event).unwrap()["type"],
        "heartbeat"
    );

    let replay = ws_request(&url, &ticket);
    assert!(
        connect_async(replay).await.is_err(),
        "a consumed WebSocket ticket must be rejected before upgrade"
    );
}

#[tokio::test]
async fn ws_does_not_accept_a_long_lived_jwt_from_the_url() {
    use tokio_tungstenite::connect_async;

    let state = Arc::new(ServerState::new(
        Arc::new(EventBus::new()),
        None,
        vec![],
        vec![],
    ));
    let (auth, token) = test_auth();
    let (port, _server) = start_ws_test_server(state.clone(), auth).await;
    let url = format!("ws://127.0.0.1:{port}/events");

    assert!(
        connect_async(format!("{url}?token={token}")).await.is_err(),
        "a JWT query parameter must be rejected before WebSocket upgrade"
    );
    assert!(
        connect_async(ws_request(&url, &token)).await.is_err(),
        "a long-lived JWT offered as a WebSocket subprotocol must be rejected before upgrade"
    );
}

#[tokio::test]
async fn websocket_ticket_capacity_is_reported_as_a_retryable_non_auth_error() {
    let state = Arc::new(ServerState::new(
        Arc::new(EventBus::new()),
        None,
        vec![],
        vec![],
    ));
    let (auth, token) = test_auth();
    for _ in 0..1024 {
        auth.issue_ws_ticket(&token)
            .expect("ticket capacity has not been reached yet");
    }

    let response = api::router(state)
        .layer(axum::Extension(auth))
        .oneshot(post_empty("/events/ticket", &token))
        .await
        .expect("ticket endpoint responds");

    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn ws_closes_before_forwarding_events_after_session_revocation() {
    use futures_util::StreamExt;
    use tokio_tungstenite::connect_async;
    use tokio_tungstenite::tungstenite::Message;

    let state = Arc::new(ServerState::new(
        Arc::new(EventBus::new()),
        None,
        vec![],
        vec![],
    ));
    let (auth, token) = test_auth();
    let ticket = auth.issue_ws_ticket(&token).expect("issue a ticket");
    let (port, _server) = start_ws_test_server(state.clone(), auth.clone()).await;
    let url = format!("ws://127.0.0.1:{port}/events");
    let request = ws_request(&url, &ticket);
    let (mut ws, _) = connect_async(request).await.expect("WS connect failed");

    auth.revoke_token(&token);
    state
        .events()
        .emit(sonium_control::ws::Event::Heartbeat { uptime_s: 99 });

    let reply = tokio::time::timeout(tokio::time::Duration::from_secs(1), ws.next())
        .await
        .expect("revoked WS was not closed promptly");
    assert!(
        !matches!(reply, Some(Ok(Message::Text(_)))),
        "a revoked WS must not receive another event"
    );
}
