mod cli;
mod retry;
mod socks;
mod transport;

use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use tokio::net::TcpListener;
use tokio::sync::watch;
use tracing::error;
use tracing::info;
use tracing::warn;

use crate::cli::Cli;
use crate::transport::Transport;
use crate::transport::TransportConfig;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let cli = Cli::parse();

    let socks_addr = cli
        .socks_socket_addr()
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    let config = TransportConfig::from_cli(&cli).map_err(|e| anyhow::anyhow!("{}", e))?;
    let health_interval = config.health_interval;

    info!(
        "Connecting to {}@{}:{}",
        config.user, config.host, config.port
    );
    info!("SOCKS5 proxy listening on {}", socks_addr);

    let transport = Arc::new(Transport::connect(config.clone()).await?);
    info!("SSH session established");

    let (_shutdown_tx, shutdown_rx) = watch::channel(false);

    let health_transport = transport.clone();
    tokio::spawn(async move {
        health_monitor(health_transport, health_interval, shutdown_rx).await;
    });

    let listener = TcpListener::bind(socks_addr).await?;

    loop {
        match listener.accept().await {
            Ok((socket, client_addr)) => {
                let transport = transport.clone();
                tokio::spawn(async move {
                    if let Err(e) = socks::serve(transport, socket).await {
                        error!("SOCKS5 error for {}: {:#}", client_addr, e);
                    }
                });
            }
            Err(err) => {
                error!("accept error: {:?}", err);
            }
        }
    }
}

async fn health_monitor(
    transport: Arc<Transport>,
    interval: Duration,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut ticker = tokio::time::interval(interval);

    loop {
        tokio::select! {
            _ = ticker.tick() => {
                if transport.check_alive().await.is_err() {
                    warn!("SSH connection lost, attempting reconnect...");
                    if let Err(e) = transport.reconnect().await {
                        error!("Reconnect failed: {}", e);
                    }
                }
            }
            _ = shutdown.changed() => {
                break;
            }
        }
    }
}
