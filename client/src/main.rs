mod controller;
mod player;
mod decoder;

use anyhow::Context;
use tracing::info;

use sonium_common::config::ClientConfig;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cfg = ClientConfig::default();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| cfg.log.level.parse().unwrap_or_default()),
        )
        .init();

    let server_addr = format!("{}:{}", cfg.server_host, cfg.server_port);
    info!(%server_addr, "Sonium client connecting");

    controller::run(server_addr, cfg).await
        .context("client controller error")
}
