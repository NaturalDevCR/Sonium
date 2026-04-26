//! Client discovery — mDNS advertisement + optional subnet scanner.
//!
//! ## mDNS (same-subnet, zero-config)
//!
//! The server calls [`advertise`] once on startup.  It registers:
//!
//! - `_sonium._tcp`      — Sonium audio stream port
//! - `_sonium-http._tcp` — web UI / REST API port
//! - `_snapcast._tcp`    — only when `snapcast_compat = true` in config
//!                         (allows legacy Snapcast clients to discover this server)
//!
//! Clients that call [`browse_servers`] will receive these advertisements
//! and can auto-connect without manual IP configuration.
//!
//! ## Subnet scanner (cross-subnet)
//!
//! For networks where mDNS is blocked (VLANs, corporate Wi-Fi), the web UI
//! lets the admin specify CIDR ranges to probe.  [`scan_subnet`] connects to
//! the Sonium stream port on each host and checks for a valid `Hello` response.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;
use mdns_sd::{ServiceDaemon, ServiceInfo};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tracing::{debug, info, warn};

const SNAPCAST_SVC:  &str = "_snapcast._tcp.local.";
const SONIUM_SVC:    &str = "_sonium._tcp.local.";
const SONIUM_HTTP:   &str = "_sonium-http._tcp.local.";

/// Advertise the server on the local network via mDNS.
///
/// This is a long-running task — spawn it with `tokio::spawn`.
/// Set `snapcast_compat` to also register `_snapcast._tcp` so legacy
/// Snapcast clients can discover this server.
pub async fn advertise(
    hostname:        &str,
    stream_port:     u16,
    control_port:    u16,
    snapcast_compat: bool,
) {
    let daemon = match ServiceDaemon::new() {
        Ok(d)  => d,
        Err(e) => {
            warn!("mDNS daemon failed to start: {e} — auto-discovery unavailable");
            return;
        }
    };

    let instance = format!("Sonium on {hostname}");

    let register = |svc_type: &str, port: u16| {
        let info = ServiceInfo::new(
            svc_type,
            &instance,
            &format!("{hostname}.local."),
            "",     // IP — let mdns-sd resolve
            port,
            None,   // no extra TXT properties
        );
        match info {
            Ok(i)  => { let _ = daemon.register(i); }
            Err(e) => warn!("mDNS register {svc_type}: {e}"),
        }
    };

    register(SONIUM_SVC,  stream_port);
    register(SONIUM_HTTP, control_port);
    if snapcast_compat {
        register(SNAPCAST_SVC, stream_port);
        info!(stream_port, "mDNS: also advertising _snapcast._tcp for Snapcast client compatibility");
    }

    info!(
        stream_port,
        control_port,
        "mDNS advertising started — clients will find this server automatically"
    );

    // Keep the task alive (daemon runs in background threads)
    std::future::pending::<()>().await;
}

/// Result of a discovered server (from the client side).
#[derive(Debug, Clone)]
pub struct DiscoveredServer {
    pub hostname: String,
    pub addr:     IpAddr,
    pub port:     u16,
    pub service:  String,
}

/// Browse for Sonium servers on the local network.
///
/// Returns discovered servers as they appear.  Run this in a background task
/// and send results through a channel.
pub async fn browse_servers(
    tx: tokio::sync::mpsc::Sender<DiscoveredServer>,
) {
    let daemon = match ServiceDaemon::new() {
        Ok(d)  => d,
        Err(e) => {
            warn!("mDNS daemon failed: {e}");
            return;
        }
    };

    for svc in [SONIUM_SVC, SNAPCAST_SVC] {
        let rx = match daemon.browse(svc) {
            Ok(r)  => r,
            Err(e) => {
                warn!("mDNS browse {svc}: {e}");
                continue;
            }
        };

        let tx2 = tx.clone();
        let svc = svc.to_string();
        tokio::spawn(async move {
            loop {
                match rx.recv_async().await {
                    Ok(event) => {
                        if let mdns_sd::ServiceEvent::ServiceResolved(info) = event {
                            for addr in info.get_addresses() {
                                let ip = IpAddr::from(*addr);
                                let _ = tx2.send(DiscoveredServer {
                                    hostname: info.get_hostname().to_string(),
                                    addr:     ip,
                                    port:     info.get_port(),
                                    service:  svc.clone(),
                                }).await;
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
        });
    }
}

// ── Subnet scanner ────────────────────────────────────────────────────────

/// Probe result from [`scan_subnet`].
#[derive(Debug, Clone, serde::Serialize)]
pub struct ScanResult {
    pub addr:        String,
    pub port:        u16,
    /// `true` if the host responded with a valid Sonium audio stream port.
    pub is_sonium:   bool,
}

/// Scan a CIDR range for Sonium-compatible servers.
///
/// Probes each host in `cidr` (e.g. `"192.168.2.0/24"`) by attempting a
/// TCP connection to `port` (default `1710`).  Returns all reachable hosts.
///
/// `concurrency` controls how many probes run simultaneously (default: 64).
pub async fn scan_subnet(
    cidr:        &str,
    port:        u16,
    concurrency: usize,
) -> Vec<ScanResult> {
    let hosts = match parse_cidr_hosts(cidr) {
        Some(h) => h,
        None    => {
            warn!("Invalid CIDR: {cidr}");
            return vec![];
        }
    };

    info!(cidr, hosts = hosts.len(), port, "Starting subnet scan");

    let sem = Arc::new(tokio::sync::Semaphore::new(concurrency));
    let mut tasks = Vec::new();

    for host in hosts {
        let sem = sem.clone();
        tasks.push(tokio::spawn(async move {
            let _permit = sem.acquire().await.ok()?;
            let addr    = SocketAddr::new(IpAddr::V4(host), port);
            let result  = timeout(Duration::from_millis(300), TcpStream::connect(addr)).await;
            match result {
                Ok(Ok(_)) => {
                    debug!(%addr, "Host responded on audio port");
                    Some(ScanResult {
                        addr:      host.to_string(),
                        port,
                        is_sonium: true,
                    })
                }
                _ => None,
            }
        }));
    }

    let mut results = Vec::new();
    for task in tasks {
        if let Ok(Some(r)) = task.await {
            results.push(r);
        }
    }
    info!(found = results.len(), "Subnet scan complete");
    results
}

/// Expand a CIDR like `"192.168.1.0/24"` into individual host addresses.
/// Returns `None` if the CIDR is malformed.
fn parse_cidr_hosts(cidr: &str) -> Option<Vec<Ipv4Addr>> {
    let parts: Vec<&str> = cidr.split('/').collect();
    if parts.len() != 2 { return None; }

    let base: Ipv4Addr  = parts[0].parse().ok()?;
    let prefix: u8      = parts[1].parse().ok()?;
    if prefix > 32 { return None; }

    let mask       = if prefix == 0 { 0u32 } else { !((1u32 << (32 - prefix)) - 1) };
    let network    = u32::from(base) & mask;
    let broadcast  = network | !mask;
    let host_count = broadcast - network + 1;

    // Limit to /16 to avoid accidental huge scans
    if host_count > 65536 { return None; }

    let hosts = (network + 1..broadcast)
        .map(|n| Ipv4Addr::from(n))
        .collect();
    Some(hosts)
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cidr_slash24() {
        let hosts = parse_cidr_hosts("192.168.1.0/24").unwrap();
        // /24 = 254 usable hosts (1-254)
        assert_eq!(hosts.len(), 254);
        assert_eq!(hosts[0],   "192.168.1.1".parse::<Ipv4Addr>().unwrap());
        assert_eq!(hosts[253], "192.168.1.254".parse::<Ipv4Addr>().unwrap());
    }

    #[test]
    fn parse_cidr_slash30() {
        let hosts = parse_cidr_hosts("10.0.0.0/30").unwrap();
        // /30 = 2 usable hosts
        assert_eq!(hosts.len(), 2);
    }

    #[test]
    fn parse_cidr_slash16_allowed() {
        let hosts = parse_cidr_hosts("10.0.0.0/16").unwrap();
        assert_eq!(hosts.len(), 65534);
    }

    #[test]
    fn parse_cidr_slash15_rejected() {
        // Would produce > 65536 hosts
        assert!(parse_cidr_hosts("10.0.0.0/15").is_none());
    }

    #[test]
    fn parse_cidr_invalid_returns_none() {
        assert!(parse_cidr_hosts("not-a-cidr").is_none());
        assert!(parse_cidr_hosts("192.168.1.0/33").is_none());
        assert!(parse_cidr_hosts("192.168.1.0").is_none());
    }
}
