mod broadcaster;
mod control_server;
mod encoder;
mod metrics;
mod session;
mod streamreader;

use anyhow::Context;
use clap::Parser;
use std::{collections::HashMap, path::PathBuf, sync::Arc};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use tokio::time::Duration;

use sonium_common::config::{ServerConfig, StreamSource};
use sonium_control::{config_api, ws::Event, EventBus, PersistenceStore, ServerState, UserStore};

use broadcaster::{new_registry, register, unregister, BroadcasterRegistry};

type StreamHandles = HashMap<String, (CancellationToken, JoinHandle<()>)>;

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

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| cfg.log.level.parse().unwrap_or_default()),
        )
        .with_target(true)
        .with_thread_ids(false)
        .compact()
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

    // Config directory — used for users.json and sonium-state.json.
    let config_dir = cli
        .config
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from("."));

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
    let live_cfg = Arc::new(parking_lot::RwLock::new(cfg.clone()));
    let mut stream_handles = StreamHandles::new();
    for stream_cfg in &cfg.streams {
        let (cancel, handle) = spawn_stream(
            stream_cfg.clone(),
            registry.clone(),
            state.clone(),
            shutdown.clone(),
        );
        stream_handles.insert(stream_cfg.id.clone(), (cancel, handle));
    }

    let (reload_tx, reload_rx) = mpsc::channel(8);
    spawn_reload_manager(
        reload_rx,
        cli.config.clone(),
        live_cfg.clone(),
        stream_handles,
        registry.clone(),
        state.clone(),
        shutdown.clone(),
    );
    spawn_config_watcher(cli.config.clone(), reload_tx.clone(), shutdown.clone());

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
        let reload_tx = reload_tx.clone();
        let port = cfg.server.control_port;
        let cancel = shutdown.clone();
        tokio::spawn(async move {
            tokio::select! {
                result = control_server::run(state, auth, config_path, Some(reload_tx), port) => {
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
                info!(%peer, "New client connected");
                let registry = registry.clone();
                let cfg      = live_cfg.read().clone();
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

fn spawn_stream(
    stream_cfg: StreamSource,
    registry: Arc<BroadcasterRegistry>,
    state: Arc<ServerState>,
    shutdown: CancellationToken,
) -> (CancellationToken, JoinHandle<()>) {
    let bc = Arc::new(broadcaster::Broadcaster::new(
        &stream_cfg.id,
        stream_cfg.buffer_ms,
    ));
    register(&registry, bc.clone());

    state.register_stream(
        &stream_cfg.id,
        stream_cfg.display_name.clone(),
        &stream_cfg.codec,
        format!("{}", stream_cfg.sample_format),
        &stream_cfg.source,
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

fn spawn_reload_manager(
    mut reload_rx: mpsc::Receiver<config_api::ReloadRequest>,
    config_path: PathBuf,
    live_cfg: Arc<parking_lot::RwLock<ServerConfig>>,
    mut handles: StreamHandles,
    registry: Arc<BroadcasterRegistry>,
    state: Arc<ServerState>,
    shutdown: CancellationToken,
) {
    tokio::spawn(async move {
        loop {
            tokio::select! {
                Some(req) = reload_rx.recv() => {
                    let result = reload_config(
                        &config_path,
                        &live_cfg,
                        &mut handles,
                        registry.clone(),
                        state.clone(),
                        shutdown.clone(),
                    );
                    let _ = req.respond_to.send(result);
                }
                _ = shutdown.cancelled() => break,
            }
        }
    });
}

fn reload_config(
    config_path: &std::path::Path,
    live_cfg: &Arc<parking_lot::RwLock<ServerConfig>>,
    handles: &mut StreamHandles,
    registry: Arc<BroadcasterRegistry>,
    state: Arc<ServerState>,
    shutdown: CancellationToken,
) -> Result<config_api::ConfigReloadReport, String> {
    let mut next_cfg = ServerConfig::from_file(config_path).map_err(|e| e.to_string())?;
    let current_cfg = live_cfg.read().clone();

    let mut restart_required = Vec::new();
    if next_cfg.server.bind != current_cfg.server.bind {
        restart_required.push("server.bind".into());
        next_cfg.server.bind = current_cfg.server.bind.clone();
    }
    if next_cfg.server.stream_port != current_cfg.server.stream_port {
        restart_required.push("server.stream_port".into());
        next_cfg.server.stream_port = current_cfg.server.stream_port;
    }
    if next_cfg.server.control_port != current_cfg.server.control_port {
        restart_required.push("server.control_port".into());
        next_cfg.server.control_port = current_cfg.server.control_port;
    }
    if next_cfg.server.mdns != current_cfg.server.mdns {
        restart_required.push("server.mdns".into());
        next_cfg.server.mdns = current_cfg.server.mdns;
    }
    if next_cfg.server.snapcast_compat != current_cfg.server.snapcast_compat {
        restart_required.push("server.snapcast_compat".into());
        next_cfg.server.snapcast_compat = current_cfg.server.snapcast_compat;
    }
    if next_cfg.log.level != current_cfg.log.level {
        restart_required.push("log.level".into());
        next_cfg.log.level = current_cfg.log.level.clone();
    }

    let old_streams: HashMap<_, _> = current_cfg
        .streams
        .iter()
        .map(|s| (s.id.clone(), s.clone()))
        .collect();
    let new_streams: HashMap<_, _> = next_cfg
        .streams
        .iter()
        .map(|s| (s.id.clone(), s.clone()))
        .collect();

    let mut report = config_api::ConfigReloadReport {
        added: Vec::new(),
        removed: Vec::new(),
        restarted: Vec::new(),
        unchanged: Vec::new(),
        restart_required,
    };

    for (id, old_stream) in &old_streams {
        match new_streams.get(id) {
            None => {
                if let Some((cancel, handle)) = handles.remove(id) {
                    cancel.cancel();
                    handle.abort();
                }
                unregister(&registry, id);
                state.unregister_stream(id);
                state.events().emit(Event::StreamRemoved {
                    stream_id: id.clone(),
                });
                report.removed.push(id.clone());
            }
            Some(new_stream) if stream_requires_restart(old_stream, new_stream) => {
                if let Some((cancel, handle)) = handles.remove(id) {
                    cancel.cancel();
                    handle.abort();
                }
                unregister(&registry, id);
                let (cancel, handle) = spawn_stream(
                    new_stream.clone(),
                    registry.clone(),
                    state.clone(),
                    shutdown.clone(),
                );
                handles.insert(id.clone(), (cancel, handle));
                state.events().emit(Event::StreamRestarted {
                    stream_id: id.clone(),
                });
                report.restarted.push(id.clone());
            }
            Some(new_stream) => {
                state.register_stream(
                    id,
                    new_stream.display_name.clone(),
                    &new_stream.codec,
                    format!("{}", new_stream.sample_format),
                    &new_stream.source,
                );
                report.unchanged.push(id.clone());
            }
        }
    }

    for (id, new_stream) in &new_streams {
        if !old_streams.contains_key(id) {
            let (cancel, handle) = spawn_stream(
                new_stream.clone(),
                registry.clone(),
                state.clone(),
                shutdown.clone(),
            );
            handles.insert(id.clone(), (cancel, handle));
            report.added.push(id.clone());
        }
    }

    *live_cfg.write() = next_cfg;
    info!(
        added = report.added.len(),
        removed = report.removed.len(),
        restarted = report.restarted.len(),
        unchanged = report.unchanged.len(),
        "Config reloaded"
    );
    Ok(report)
}

fn stream_requires_restart(old: &StreamSource, new: &StreamSource) -> bool {
    old.source != new.source
        || old.codec != new.codec
        || old.sample_format != new.sample_format
        || old.buffer_ms != new.buffer_ms
        || old.idle_timeout_ms != new.idle_timeout_ms
        || old.silence_on_idle != new.silence_on_idle
}

fn spawn_config_watcher(
    config_path: PathBuf,
    reload_tx: mpsc::Sender<config_api::ReloadRequest>,
    shutdown: CancellationToken,
) {
    tokio::spawn(async move {
        use notify::{EventKind, RecursiveMode, Watcher};

        let (fs_tx, mut fs_rx) = mpsc::channel(8);
        let mut watcher = match notify::recommended_watcher(move |result| {
            let _ = fs_tx.blocking_send(result);
        }) {
            Ok(w) => w,
            Err(e) => {
                warn!("Config watcher unavailable: {e}");
                return;
            }
        };

        if let Err(e) = watcher.watch(&config_path, RecursiveMode::NonRecursive) {
            warn!(path = %config_path.display(), "Config watcher unavailable: {e}");
            return;
        }

        loop {
            tokio::select! {
                Some(result) = fs_rx.recv() => {
                    let Ok(event) = result else { continue; };
                    if !matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_)) {
                        continue;
                    }
                    tokio::time::sleep(Duration::from_millis(500)).await;
                    while fs_rx.try_recv().is_ok() {}

                    let (respond_to, response) = tokio::sync::oneshot::channel();
                    if reload_tx.send(config_api::ReloadRequest { respond_to }).await.is_err() {
                        break;
                    }
                    match response.await {
                        Ok(Ok(report)) => info!(
                            added = report.added.len(),
                            removed = report.removed.len(),
                            restarted = report.restarted.len(),
                            "Config file change reloaded"
                        ),
                        Ok(Err(e)) => warn!("Config file change could not be reloaded: {e}"),
                        Err(_) => break,
                    }
                }
                _ = shutdown.cancelled() => break,
            }
        }
    });
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
