mod config;
mod session;
mod broadcaster;
mod streamreader;
mod encoder;
mod control_server;

use anyhow::Context;
use std::sync::Arc;
use tokio::net::TcpListener;
use tracing::{info, warn};

use sonium_common::config::ServerConfig;
use sonium_control::{ServerState, EventBus};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cfg = ServerConfig::from_file_or_default(std::path::Path::new("sonium.toml"));

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| cfg.log.level.parse().unwrap_or_default()),
        )
        .init();

    info!(
        stream_port  = cfg.server.stream_port,
        control_port = cfg.server.control_port,
        codec        = %cfg.stream.codec,
        format       = %cfg.stream.sample_format,
        "Sonium server starting"
    );

    let events = Arc::new(EventBus::new());
    let state  = Arc::new(ServerState::new(events));

    // ── Audio broadcaster ─────────────────────────────────────────────────
    let broadcaster = Arc::new(broadcaster::Broadcaster::new());

    // ── HTTP control server (REST API + Web UI) ───────────────────────────
    {
        let state_clone = state.clone();
        let port        = cfg.server.control_port;
        tokio::spawn(async move {
            if let Err(e) = control_server::run(state_clone, port).await {
                warn!("Control server error: {e}");
            }
        });
    }

    // ── mDNS advertisement ────────────────────────────────────────────────
    if cfg.server.mdns {
        let host = hostname::get()
            .map(|h| h.to_string_lossy().to_string())
            .unwrap_or_else(|_| "sonium".into());
        let sp = cfg.server.stream_port;
        let cp = cfg.server.control_port;
        tokio::spawn(async move {
            sonium_control::discovery::advertise(&host, sp, cp).await;
        });
    }

    // ── Stream reader (stdin/FIFO → encode → broadcast) ───────────────────
    {
        let bc    = broadcaster.clone();
        let cfg2  = cfg.clone();
        let state = state.clone();
        tokio::spawn(async move {
            state.set_stream_status("default", sonium_control::state::StreamStatus::Playing);
            if let Err(e) = streamreader::run(bc, cfg2).await {
                warn!("Stream reader exited: {e}");
                state.set_stream_status("default", sonium_control::state::StreamStatus::Idle);
            }
        });
    }

    // ── TCP listener for audio clients ────────────────────────────────────
    let addr = format!("{}:{}", cfg.server.bind, cfg.server.stream_port);
    let listener = TcpListener::bind(&addr)
        .await
        .with_context(|| format!("cannot bind to {addr}"))?;
    info!("Listening on {addr}");

    loop {
        let (stream, peer) = listener.accept().await?;
        info!(%peer, "New client connected");
        let bc    = broadcaster.clone();
        let cfg   = cfg.clone();
        let state = state.clone();
        tokio::spawn(async move {
            if let Err(e) = session::handle(stream, peer, bc, cfg, state).await {
                warn!(%peer, "Session error: {e}");
            }
            info!(%peer, "Client disconnected");
        });
    }
}
