mod broadcaster;
mod control_server;
mod encoder;
mod metrics;
mod session;
mod streamreader;

use anyhow::Context;
use clap::Parser;
use socket2::{SockRef, TcpKeepalive};
use std::{path::PathBuf, sync::Arc};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};
use tracing_subscriber::prelude::*;

use tokio::time::Duration;

use sonium_common::config::{ServerConfig, StreamSource};
use sonium_control::{ws::Event, EventBus, PersistenceStore, ServerState, UserStore};

use broadcaster::{new_registry, register, BroadcasterRegistry};

#[derive(Parser)]
#[command(
    name = "sonium-server",
    version,
    about = "Sonium multiroom audio server"
)]
struct Cli {
    /// Config file path.
    #[arg(
        short,
        long,
        value_name = "FILE",
        default_value = "sonium.toml",
        env = "SONIUM_CONFIG"
    )]
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

    /// Initialize admin password and exit (only if no users exist).
    #[arg(long)]
    init_admin: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let mut cfg = ServerConfig::from_file_or_default(&cli.config);

    if let Some(p) = cli.stream_port {
        cfg.server.stream_port = p;
    }
    if let Some(p) = cli.control_port {
        cfg.server.control_port = p;
    }
    if let Some(b) = cli.bind {
        cfg.server.bind = b;
    }
    if let Some(l) = cli.log {
        cfg.log.level = l;
    }
    if cli.no_mdns {
        cfg.server.mdns = false;
    }

    let config_dir = cli
        .config
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let log_path = config_dir.join("sonium.log");
    let log_dir = log_path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    if let Err(e) = std::fs::create_dir_all(&log_dir) {
        warn!(path = %log_dir.display(), "Could not create log directory: {e}");
    }
    std::env::set_var("SONIUM_LOG_FILE", &log_path);
    let log_file_name = log_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("sonium.log");
    let file_appender = tracing_appender::rolling::never(&log_dir, log_file_name);
    let (file_writer, _log_guard) = tracing_appender::non_blocking(file_appender);

    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        format!("{},mdns_sd::service_daemon=off", cfg.log.level)
            .parse()
            .unwrap_or_else(|_| {
                tracing_subscriber::EnvFilter::new("info,mdns_sd::service_daemon=off")
            })
    });
    let stdout_layer = tracing_subscriber::fmt::layer()
        .with_timer(tracing_subscriber::fmt::time::ChronoLocal::rfc_3339())
        .with_target(false)
        .with_thread_ids(false)
        .with_ansi(false)
        .compact();
    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(file_writer)
        .with_timer(tracing_subscriber::fmt::time::ChronoLocal::rfc_3339())
        .with_ansi(false)
        .with_target(false)
        .with_thread_ids(false)
        .compact();
    tracing_subscriber::registry()
        .with(env_filter)
        .with(stdout_layer)
        .with(file_layer)
        .init();

    let local_ip = local_ip_address::local_ip()
        .map(|ip| ip.to_string())
        .unwrap_or_else(|_| "unknown".into());

    info!(
        version = env!("CARGO_PKG_VERSION"),
        local_ip = local_ip,
        stream_port = cfg.server.stream_port,
        control_port = cfg.server.control_port,
        streams = cfg.streams.len(),
        "Sonium server starting"
    );

    // ── Shutdown coordination ─────────────────────────────────────────────
    let shutdown = CancellationToken::new();

    // One-time initialization if requested.
    if let Some(password) = cli.init_admin {
        let _ = UserStore::load_or_init(&config_dir, Some(password));
        info!("Admin account initialized (if it didn't exist).");
        return Ok(());
    }

    // Auth: load users from config directory.
    let auth = UserStore::load_or_init(&config_dir, None);

    // State persistence: load from sonium-state.json.
    let persistence = Arc::new(PersistenceStore::new(&config_dir));
    let (saved_groups, saved_clients, saved_streams) = persistence.load();

    let events = Arc::new(EventBus::new());
    let state = Arc::new(ServerState::new(
        events,
        Some(persistence),
        saved_clients,
        saved_streams,
    ));
    let registry = new_registry();

    // Restore persisted groups before any clients connect.
    state.restore_groups(saved_groups);

    // ── One stream reader per configured source ───────────────────────────
    for stream_cfg in &cfg.streams {
        let buffer_ms_overridden = stream_cfg.buffer_ms.is_some();
        let chunk_ms_overridden = stream_cfg.chunk_ms.is_some();
        let effective_buffer_ms = cfg.effective_buffer_ms(stream_cfg);
        let effective_chunk_ms = cfg.effective_chunk_ms(stream_cfg);
        let mut runtime_stream = stream_cfg.clone();
        runtime_stream.buffer_ms = Some(effective_buffer_ms);
        runtime_stream.chunk_ms = Some(effective_chunk_ms);
        let (cancel, handle) = spawn_stream(
            runtime_stream,
            StreamRuntimeConfig {
                effective_buffer_ms,
                buffer_ms_overridden,
                effective_chunk_ms,
                chunk_ms_overridden,
            },
            registry.clone(),
            state.clone(),
            shutdown.clone(),
        );
        drop((cancel, handle));
    }

    // ── Heartbeat task (pushes uptime to connected web UIs every 5 s) ─────
    {
        let state = state.clone();
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
        let state = state.clone();
        let auth = auth.clone();
        let config_path = cli.config.clone();
        let port = cfg.server.control_port;
        let cancel = shutdown.clone();
        tokio::spawn(async move {
            tokio::select! {
                result = control_server::run(state, auth, config_path, None, port) => {
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
        let host = hostname::get()
            .map(|h| h.to_string_lossy().to_string())
            .unwrap_or_else(|_| "sonium".into());
        let sp = cfg.server.stream_port;
        let cp = cfg.server.control_port;
        let cfg_compat = cfg.server.snapcast_compat;
        let cancel = shutdown.clone();
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
    let shutdown_signal = shutdown_signal();
    tokio::pin!(shutdown_signal);

    loop {
        tokio::select! {
            accept = listener.accept() => {
                let (stream, peer) = accept?;
                let _ = stream.set_nodelay(true);
                configure_tcp_stream(&stream);
                info!(%peer, "New client connected");
                let registry = registry.clone();
                let session_cfg = cfg.clone();
                let state    = state.clone();
                let cancel   = shutdown.clone();
                tokio::spawn(async move {
                    tokio::select! {
                        result = session::handle(stream, peer, registry, session_cfg, state) => {
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

            _ = &mut shutdown_signal => {
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

fn configure_tcp_stream(stream: &TcpStream) {
    let sock = SockRef::from(stream);
    if let Err(e) = sock.set_keepalive(true) {
        warn!("Could not enable TCP keepalive: {e}");
    }

    // Expedited Forwarding DSCP (46) shifted into the IPv4 TOS byte. Routers
    // may ignore it, but honoring networks can prioritize latency-sensitive audio.
    if let Err(e) = sock.set_tos_v4(46 << 2) {
        warn!("Could not set TCP DSCP/TOS priority: {e}");
    }

    let keepalive = TcpKeepalive::new()
        .with_time(Duration::from_secs(30))
        .with_interval(Duration::from_secs(10));
    if let Err(e) = sock.set_tcp_keepalive(&keepalive) {
        warn!("Could not configure TCP keepalive: {e}");
    }
}

struct StreamRuntimeConfig {
    effective_buffer_ms: u32,
    buffer_ms_overridden: bool,
    effective_chunk_ms: u32,
    chunk_ms_overridden: bool,
}

fn spawn_stream(
    stream_cfg: StreamSource,
    runtime: StreamRuntimeConfig,
    registry: Arc<BroadcasterRegistry>,
    state: Arc<ServerState>,
    shutdown: CancellationToken,
) -> (CancellationToken, JoinHandle<()>) {
    let bc = Arc::new(broadcaster::Broadcaster::new(
        &stream_cfg.id,
        runtime.effective_buffer_ms,
    ));
    register(&registry, bc.clone());

    state.register_stream(
        &stream_cfg.id,
        stream_cfg.display_name.clone(),
        &stream_cfg.codec,
        format!("{}", stream_cfg.sample_format),
        &stream_cfg.source,
        runtime.effective_buffer_ms,
        runtime.buffer_ms_overridden,
        runtime.effective_chunk_ms,
        runtime.chunk_ms_overridden,
        stream_cfg.idle_timeout_ms,
        stream_cfg.silence_on_idle,
    );

    let stream_id = stream_cfg.id.clone();
    let local_cancel = CancellationToken::new();
    let task_cancel = local_cancel.clone();
    let state2 = state.clone();
    let state3 = state.clone();
    let reg2 = registry.clone();

    let handle = tokio::spawn(async move {
        metrics::STREAM_STATUS
            .with_label_values(&[&stream_cfg.id])
            .set(1);
        state2.set_stream_status(&stream_cfg.id, sonium_control::state::StreamStatus::Playing);
        tokio::select! {
            result = streamreader::run(bc, stream_cfg.clone(), state2, reg2) => {
                if let Err(e) = result {
                    warn!("[{}] Stream reader exited: {e}", stream_cfg.id);
                    metrics::STREAM_STATUS.with_label_values(&[&stream_cfg.id]).set(-1);
                } else {
                    metrics::STREAM_STATUS.with_label_values(&[&stream_cfg.id]).set(0);
                }
                state3.set_stream_status(&stream_cfg.id, sonium_control::state::StreamStatus::Idle);
            }
            _ = task_cancel.cancelled() => {
                info!("[{}] Stream reader reloading", stream_cfg.id);
                metrics::STREAM_STATUS.with_label_values(&[&stream_cfg.id]).set(0);
                state3.set_stream_status(&stream_cfg.id, sonium_control::state::StreamStatus::Idle);
            }
            _ = shutdown.cancelled() => {
                info!("[{}] Stream reader shutting down", stream_cfg.id);
                metrics::STREAM_STATUS.with_label_values(&[&stream_cfg.id]).set(0);
                state3.set_stream_status(&stream_cfg.id, sonium_control::state::StreamStatus::Idle);
            }
        }
    });

    info!(stream = %stream_id, "Stream reader started");
    (local_cancel, handle)
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
