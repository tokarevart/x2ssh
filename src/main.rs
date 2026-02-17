use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use tokio::net::TcpListener;
use tokio::sync::watch;
use tracing::error;
use tracing::info;
use tracing::warn;
use x2ssh::retry::RetryPolicy;
use x2ssh::socks;
use x2ssh::transport::Transport;
use x2ssh::transport::TransportConfig;

fn parse_user_host(s: &str) -> Result<(String, String), String> {
    let parts: Vec<&str> = s.splitn(2, '@').collect();
    if parts.len() != 2 {
        return Err("Expected format: USER@HOST".to_string());
    }
    Ok((parts[0].to_string(), parts[1].to_string()))
}

#[derive(Parser, Debug)]
#[command(name = "x2ssh")]
#[command(about = "SOCKS5 proxy over SSH with robust retry logic")]
struct Cli {
    #[arg(value_name = "USER@HOST")]
    destination: String,

    #[arg(short = 'D', long = "socks", value_name = "ADDR")]
    socks_addr: Option<String>,

    #[arg(short = 'p', long = "port", default_value = "22")]
    port: u16,

    #[arg(short = 'i', long = "identity", value_name = "FILE")]
    identity: Option<PathBuf>,

    #[arg(long = "retry-max", value_name = "N")]
    retry_max: Option<u32>,

    #[arg(long = "retry-delay", value_name = "MS", default_value = "1000")]
    retry_delay: u64,

    #[arg(long = "retry-backoff", value_name = "N", default_value = "2")]
    retry_backoff: f64,

    #[arg(long = "retry-max-delay", value_name = "MS", default_value = "30000")]
    retry_max_delay: u64,

    #[arg(long = "health-interval", value_name = "MS", default_value = "5000")]
    health_interval: u64,
}

impl Cli {
    fn user_host(&self) -> Result<(String, String), String> {
        parse_user_host(&self.destination)
    }

    fn socks_socket_addr(&self) -> Result<SocketAddr, String> {
        let addr = match &self.socks_addr {
            Some(a) => a.clone(),
            None => return Err("SOCKS address is required (-D, --socks)".to_string()),
        };

        if let Ok(port) = addr.parse::<u16>() {
            return Ok(SocketAddr::from(([127, 0, 0, 1], port)));
        }

        addr.parse::<SocketAddr>()
            .map_err(|e| format!("Invalid SOCKS address '{}': {}", addr, e))
    }

    fn transport_config(&self) -> Result<TransportConfig, String> {
        let (user, host) = self.user_host()?;

        let retry_policy = RetryPolicy {
            max_attempts: self.retry_max,
            initial_delay: Duration::from_millis(self.retry_delay),
            backoff: self.retry_backoff,
            max_delay: Duration::from_millis(self.retry_max_delay),
        };

        Ok(TransportConfig {
            retry_policy,
            health_interval: Duration::from_millis(self.health_interval),
            key_path: self.identity.clone(),
            user,
            host,
            port: self.port,
        })
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let cli = Cli::parse();

    let socks_addr = cli
        .socks_socket_addr()
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    let config = cli
        .transport_config()
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    let health_interval = config.health_interval;

    info!(
        "Connecting to {}@{}:{}",
        config.user, config.host, config.port
    );
    info!("SOCKS5 proxy listening on {}", socks_addr);

    let transport = Arc::new(Transport::connect(config).await?);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_argument_parsing() {
        let cli = Cli::try_parse_from(["x2ssh", "-D", "1080", "user@host.com"]);
        assert!(cli.is_ok());
        let cli = cli.unwrap();
        assert_eq!(cli.destination, "user@host.com");
        assert_eq!(cli.port, 22);
    }

    #[test]
    fn test_user_host_parsing() {
        let (user, host) = parse_user_host("alice@server.com").unwrap();
        assert_eq!(user, "alice");
        assert_eq!(host, "server.com");
    }

    #[test]
    fn test_socks_addr_port_only() {
        let cli = Cli::try_parse_from(["x2ssh", "-D", "1080", "user@host.com"]).unwrap();

        let addr = cli.socks_socket_addr().unwrap();
        assert_eq!(addr.port(), 1080);
    }

    #[test]
    fn test_socks_addr_full() {
        let cli = Cli::try_parse_from(["x2ssh", "-D", "127.0.0.1:8080", "user@host.com"]).unwrap();

        let addr = cli.socks_socket_addr().unwrap();
        assert_eq!(addr.port(), 8080);
    }
}
