mod controller;
mod player;
mod decoder;

use std::io::Write;
use std::time::Duration;

use anyhow::Context;
use clap::Parser;
use tracing::info;

use sonium_common::config::ClientConfig;
use sonium_control::discovery::{self, DiscoveredServer};

#[derive(Parser)]
#[command(name = "sonium-client", version, about = "Sonium multiroom audio client")]
struct Cli {
    /// Server hostname or IP address.
    /// When --discover is used, this is ignored and mDNS discovery is used instead.
    #[arg(value_name = "SERVER", default_value = "127.0.0.1", env = "SONIUM_SERVER")]
    server: String,

    /// Server stream port.
    #[arg(short, long, value_name = "PORT", default_value_t = 1710, env = "SONIUM_PORT")]
    port: u16,

    /// Extra playout latency offset in milliseconds (useful for Bluetooth sinks).
    #[arg(short, long, value_name = "MS", default_value_t = 0, env = "SONIUM_LATENCY")]
    latency: i32,

    /// Client display name shown in the web UI (defaults to hostname).
    #[arg(short, long, value_name = "NAME", env = "SONIUM_NAME")]
    name: Option<String>,

    /// Audio output device (substring match, case-insensitive).
    /// Example: --device "BlackHole 2ch"
    #[arg(short = 'd', long, value_name = "DEVICE", env = "SONIUM_DEVICE")]
    device: Option<String>,

    /// Log level (trace/debug/info/warn/error).
    #[arg(long, value_name = "LEVEL", default_value = "info", env = "SONIUM_LOG")]
    log: String,

    /// Auto-discover servers on the local network via mDNS.
    /// If a single server is found it is used automatically.
    /// If multiple servers are found an interactive selection menu is shown.
    #[arg(long, env = "SONIUM_DISCOVER")]
    discover: bool,

    /// How long to wait for mDNS discovery (seconds). Only used with --discover.
    #[arg(long, value_name = "SECS", default_value_t = 3, env = "SONIUM_DISCOVER_TIMEOUT")]
    discover_timeout: u64,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| cli.log.parse().unwrap_or_default()),
        )
        .init();

    // ── Server resolution ────────────────────────────────────────────────
    let (server_host, server_port) = if cli.discover {
        discover_server(cli.discover_timeout).await?
    } else {
        (cli.server.clone(), cli.port)
    };

    let cfg = ClientConfig {
        server_host: server_host.clone(),
        server_port,
        latency_ms:  cli.latency,
        client_name: cli.name,
        device:      cli.device,
        ..Default::default()
    };

    let server_addr = format!("{server_host}:{server_port}");
    info!(
        %server_addr,
        latency_ms = cfg.latency_ms,
        "Sonium client starting"
    );

    controller::run(server_addr, cfg).await
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
            n    = i + 1,
            host = s.hostname,
            addr = s.addr,
            port = s.port,
            svc  = s.service.trim_end_matches(".local."),
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
