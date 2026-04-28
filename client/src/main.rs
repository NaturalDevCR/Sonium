use sonium_client_lib::{controller, setup};
use std::io::Write;
use std::time::Duration;

use anyhow::Context;
use clap::Parser;
use tracing::info;

use sonium_common::config::ClientConfig;
use sonium_control::discovery::{self, DiscoveredServer};

#[derive(Parser)]
#[command(
    name = "sonium-client",
    version,
    about = "Sonium multiroom audio client"
)]
struct Cli {
    /// Config file path.
    #[arg(short, long, value_name = "FILE", env = "SONIUM_CLIENT_CONFIG")]
    config: Option<std::path::PathBuf>,

    /// Client instance ID (for running multiple clients on the same host).
    #[arg(short = 'i', long, value_name = "ID", env = "SONIUM_INSTANCE")]
    instance: Option<u32>,

    /// Server hostname or IP address.
    /// When --discover is used, this is ignored and mDNS discovery is used instead.
    #[arg(value_name = "SERVER", env = "SONIUM_SERVER")]
    server: Option<String>,

    /// Server stream port.
    #[arg(short, long, value_name = "PORT", env = "SONIUM_PORT")]
    port: Option<u16>,

    /// Extra playout latency offset in milliseconds (useful for Bluetooth sinks).
    #[arg(short, long, value_name = "MS", env = "SONIUM_LATENCY")]
    latency: Option<i32>,

    /// Client display name shown in the web UI (defaults to hostname).
    #[arg(short, long, value_name = "NAME", env = "SONIUM_NAME")]
    name: Option<String>,

    /// Audio output device (substring match, case-insensitive).
    /// Example: --device "BlackHole 2ch"
    #[arg(short = 'd', long, value_name = "DEVICE", env = "SONIUM_DEVICE")]
    device: Option<String>,

    /// Log level (trace/debug/info/warn/error).
    #[arg(long, value_name = "LEVEL", env = "SONIUM_LOG")]
    log: Option<String>,

    /// Auto-discover servers on the local network via mDNS.
    /// If a single server is found it is used automatically.
    /// If multiple servers are found an interactive selection menu is shown.
    #[arg(long, env = "SONIUM_DISCOVER")]
    discover: bool,

    /// How long to wait for mDNS discovery (seconds). Only used with --discover.
    #[arg(
        long,
        value_name = "SECS",
        default_value_t = 3,
        env = "SONIUM_DISCOVER_TIMEOUT"
    )]
    discover_timeout: u64,

    /// Launch the interactive setup wizard to install and configure instances.
    #[arg(long)]
    setup: bool,

    /// Uninstall Sonium Client and remove all background services.
    #[arg(long)]
    uninstall: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    if cli.setup {
        return setup::run().await;
    }

    if cli.uninstall {
        return setup::uninstall().await;
    }

    let config_path = cli
        .config
        .unwrap_or_else(|| std::path::PathBuf::from("sonium-client.toml"));

    let mut cfg = ClientConfig::from_file_or_default(&config_path);

    if let Some(s) = cli.server {
        cfg.server_host = s;
    }
    if let Some(p) = cli.port {
        cfg.server_port = p;
    }
    if let Some(l) = cli.latency {
        cfg.latency_ms = l;
    }
    if let Some(n) = cli.name {
        cfg.client_name = Some(n);
    }
    if let Some(d) = cli.device {
        cfg.device = Some(d);
    }
    if let Some(i) = cli.instance {
        cfg.instance = i;
    }
    if let Some(log) = cli.log {
        cfg.log.level = log;
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| cfg.log.level.parse().unwrap_or_default()),
        )
        .init();

    // ── Server resolution ────────────────────────────────────────────────
    let (server_host, server_port) = if cli.discover {
        discover_server(cli.discover_timeout).await?
    } else {
        (cfg.server_host.clone(), cfg.server_port)
    };

    cfg.server_host = server_host.clone();
    cfg.server_port = server_port;

    let server_addr = format!("{server_host}:{server_port}");
    info!(
        %server_addr,
        instance = cfg.instance,
        latency_ms = cfg.latency_ms,
        "Sonium client starting"
    );

    controller::run(server_addr, cfg)
        .await
        .context("client controller error")
}

/// Run mDNS discovery for `timeout_secs` and return the chosen server.
///
/// - 0 servers found → error
/// - 1 server found  → auto-select
/// - N servers found → interactive terminal menu
async fn discover_server(timeout_secs: u64) -> anyhow::Result<(String, u16)> {
    eprintln!("🔍 Discovering Sonium servers on the local network...");

    let (tx, mut rx) = tokio::sync::mpsc::channel::<DiscoveredServer>(32);
    tokio::spawn(discovery::browse_servers(tx));

    // Collect servers for the discovery period.
    let mut servers: Vec<DiscoveredServer> = Vec::new();
    let deadline = tokio::time::sleep(Duration::from_secs(timeout_secs));
    tokio::pin!(deadline);

    loop {
        tokio::select! {
            result = rx.recv() => {
                match result {
                    Some(s) => {
                        // Deduplicate by addr+port
                        let key = format!("{}:{}", s.addr, s.port);
                        if !servers.iter().any(|x| format!("{}:{}", x.addr, x.port) == key) {
                            eprintln!("   ✓ Found: {} ({}:{}, {})", s.hostname, s.addr, s.port, s.service);
                            servers.push(s);
                        }
                    }
                    None => break,
                }
            }
            _ = &mut deadline => break,
        }
    }

    if servers.is_empty() {
        anyhow::bail!(
            "No servers found via mDNS after {timeout_secs}s.\n\
             Specify a server address manually: sonium-client <SERVER>"
        );
    }

    if servers.len() == 1 {
        let s = &servers[0];
        eprintln!("   → Auto-connecting to {}", s.hostname);
        return Ok((s.addr.to_string(), s.port));
    }

    // Multiple servers — interactive selection
    select_server_interactive(&servers)
}

/// Present a numbered list of servers and let the user pick one.
fn select_server_interactive(servers: &[DiscoveredServer]) -> anyhow::Result<(String, u16)> {
    eprintln!();
    eprintln!("Multiple servers found. Select one:");
    eprintln!();
    for (i, s) in servers.iter().enumerate() {
        eprintln!(
            "  [{n}] {host} — {addr}:{port}  ({svc})",
            n = i + 1,
            host = s.hostname,
            addr = s.addr,
            port = s.port,
            svc = s.service.trim_end_matches(".local."),
        );
    }
    eprintln!();

    loop {
        eprint!("Enter number [1-{}]: ", servers.len());
        std::io::stderr().flush()?;

        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;

        match input.trim().parse::<usize>() {
            Ok(n) if n >= 1 && n <= servers.len() => {
                let s = &servers[n - 1];
                eprintln!("   → Selected: {} ({}:{})", s.hostname, s.addr, s.port);
                return Ok((s.addr.to_string(), s.port));
            }
            _ => {
                eprintln!("   Invalid choice. Try again.");
            }
        }
    }
}
