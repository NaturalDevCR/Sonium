mod session;
mod broadcaster;
mod streamreader;
mod encoder;
mod control_server;
mod metrics;

use anyhow::Context;
use clap::Parser;
use std::{path::PathBuf, sync::Arc};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use tokio::time::Duration;

use sonium_common::config::ServerConfig;
use sonium_control::{ServerState, EventBus, UserStore, PersistenceStore, ws::Event};

use broadcaster::{new_registry, register};

#[derive(Parser)]
#[command(name = "sonium-server", version, about = "Sonium multiroom audio server")]
struct Cli {
    /// Config file path.
    #[arg(short, long, value_name = "FILE", default_value = "sonium.toml", env = "SONIUM_CONFIG")]
    config: PathBuf,

    /// Override: TCP port for audio stream.
    #[arg(long, value_name = "PORT", env = "SONIUM_STREAM_PORT")]
    stream_port: Option<u16>,

    /// Override: HTTP port for the control API and web UI.
    #[arg(long, value_name = "PORT", env = "SONIUM_CONTROL_PORT")]
    control_port: Option<u16>,

    /// Override: bind address.
    #[arg(long, value_name = "ADDR", env = "SONIUM_BIND")]
    bind: Option<String>,

    /// Override: log level (trace/debug/info/warn/error).
    #[arg(short, long, value_name = "LEVEL", env = "SONIUM_LOG")]
    log: Option<String>,

    /// Disable mDNS advertisement.
    #[arg(long, env = "SONIUM_NO_MDNS")]
    no_mdns: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let mut cfg = ServerConfig::from_file_or_default(&cli.config);

    if let Some(p) = cli.stream_port  { cfg.server.stream_port  = p; }
    if let Some(p) = cli.control_port { cfg.server.control_port = p; }
    if let Some(b) = cli.bind         { cfg.server.bind         = b; }
    if let Some(l) = cli.log          { cfg.log.level           = l; }
    if cli.no_mdns                    { cfg.server.mdns         = false; }

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| cfg.log.level.parse().unwrap_or_default()),
        )
        .with_target(true)
        .with_thread_ids(false)
        .compact()
        .init();

    info!(
        version      = env!("CARGO_PKG_VERSION"),
        stream_port  = cfg.server.stream_port,
        control_port = cfg.server.control_port,
        streams      = cfg.streams.len(),
        "Sonium server starting"
    );

    // ── Shutdown coordination ─────────────────────────────────────────────
    let shutdown = CancellationToken::new();

    // Config directory — used for users.json and sonium-state.json.
    let config_dir = cli.config.parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from("."));

    // Auth: load users from config directory.
    let auth = UserStore::load_or_init(&config_dir);

    // State persistence: load from sonium-state.json.
    let persistence  = Arc::new(PersistenceStore::new(&config_dir));
    let (saved_groups, saved_clients) = persistence.load();

    let events   = Arc::new(EventBus::new());
    let state    = Arc::new(ServerState::new(events, Some(persistence), saved_clients));
    let registry = new_registry();

    // Restore persisted groups before any clients connect.
    state.restore_groups(saved_groups);

    // ── One stream reader per configured source ───────────────────────────
    for stream_cfg in &cfg.streams {
        let bc = Arc::new(broadcaster::Broadcaster::new(
            &stream_cfg.id,
            stream_cfg.buffer_ms,
        ));
        register(&registry, bc.clone());

        // Register the stream in ServerState (so the REST API exposes it).
        state.register_stream(
            &stream_cfg.id,
            stream_cfg.display_name.clone(),
            &stream_cfg.codec,
            format!("{}", stream_cfg.sample_format),
        );

        let bc2      = bc.clone();
        let stream_c = stream_cfg.clone();
        let state2   = state.clone();
        let state3   = state.clone();
        let reg2     = registry.clone();
        let cancel   = shutdown.clone();
        tokio::spawn(async move {
            metrics::STREAM_STATUS.with_label_values(&[&stream_c.id]).set(1);
            state2.set_stream_status(&stream_c.id, sonium_control::state::StreamStatus::Playing);
            tokio::select! {
                result = streamreader::run(bc2, stream_c.clone(), state2, reg2) => {
                    if let Err(e) = result {
                        warn!("[{}] Stream reader exited: {e}", stream_c.id);
                        metrics::STREAM_STATUS.with_label_values(&[&stream_c.id]).set(-1);
                    } else {
                        metrics::STREAM_STATUS.with_label_values(&[&stream_c.id]).set(0);
                    }
                    state3.set_stream_status(&stream_c.id, sonium_control::state::StreamStatus::Idle);
                }
                _ = cancel.cancelled() => {
                    info!("[{}] Stream reader shutting down", stream_c.id);
                    metrics::STREAM_STATUS.with_label_values(&[&stream_c.id]).set(0);
                    state3.set_stream_status(&stream_c.id, sonium_control::state::StreamStatus::Idle);
                }
            }
        });
    }

    // ── Heartbeat task (pushes uptime to connected web UIs every 5 s) ─────
    {
        let state  = state.clone();
        let cancel = shutdown.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(5));
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        let uptime = state.uptime_secs();
                        state.events().emit(Event::Heartbeat { uptime_s: uptime });
                        metrics::UPTIME_SECONDS.set(uptime);
                    }
                    _ = cancel.cancelled() => break,
                }
            }
        });
    }

    // ── HTTP control server (REST API + embedded web UI) ──────────────────
    {
        let state       = state.clone();
        let auth        = auth.clone();
        let config_path = cli.config.clone();
        let port        = cfg.server.control_port;
        let cancel      = shutdown.clone();
        tokio::spawn(async move {
            tokio::select! {
                result = control_server::run(state, auth, config_path, port) => {
                    if let Err(e) = result {
                        warn!("Control server error: {e}");
                    }
                }
                _ = cancel.cancelled() => {
                    info!("Control server shutting down");
                }
            }
        });
    }

    // ── mDNS advertisement ────────────────────────────────────────────────
    if cfg.server.mdns {
        let host       = hostname::get()
            .map(|h| h.to_string_lossy().to_string())
            .unwrap_or_else(|_| "sonium".into());
        let sp         = cfg.server.stream_port;
        let cp         = cfg.server.control_port;
        let cfg_compat = cfg.server.snapcast_compat;
        let cancel     = shutdown.clone();
        tokio::spawn(async move {
            tokio::select! {
                _ = sonium_control::discovery::advertise(&host, sp, cp, cfg_compat) => {}
                _ = cancel.cancelled() => {
                    info!("mDNS advertisement stopped");
                }
            }
        });
    }

    // ── TCP listener for audio clients ────────────────────────────────────
    let addr = format!("{}:{}", cfg.server.bind, cfg.server.stream_port);
    let listener = TcpListener::bind(&addr)
        .await
        .with_context(|| format!("cannot bind to {addr}"))?;
    info!("Listening for audio clients on {addr}");

    // ── Accept loop with graceful shutdown ─────────────────────────────────
    loop {
        tokio::select! {
            accept = listener.accept() => {
                let (stream, peer) = accept?;
                info!(%peer, "New client connected");
                let registry = registry.clone();
                let cfg      = cfg.clone();
                let state    = state.clone();
                let cancel   = shutdown.clone();
                tokio::spawn(async move {
                    tokio::select! {
                        result = session::handle(stream, peer, registry, cfg, state) => {
                            if let Err(e) = result {
                                warn!(%peer, "Session error: {e}");
                            }
                        }
                        _ = cancel.cancelled() => {
                            info!(%peer, "Session cancelled by shutdown");
                        }
                    }
                    info!(%peer, "Client disconnected");
                });
            }

            _ = shutdown_signal() => {
                info!("Shutdown signal received — stopping server");
                shutdown.cancel();
                // Give spawned tasks a moment to clean up
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                info!("Sonium server stopped cleanly");
                break;
            }
        }
    }

    Ok(())
}

/// Wait for SIGINT (Ctrl-C) or SIGTERM.
async fn shutdown_signal() {
    let ctrl_c = tokio::signal::ctrl_c();

    #[cfg(unix)]
    {
        let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler");
        tokio::select! {
            _ = ctrl_c => {}
            _ = sigterm.recv() => {}
        }
    }

    #[cfg(not(unix))]
    {
        ctrl_c.await.ok();
    }
}
