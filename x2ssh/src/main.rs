use std::net::IpAddr;
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
use x2ssh::config::AppConfig;
use x2ssh::retry::RetryPolicy;
use x2ssh::socks;
use x2ssh::transport::Transport;
use x2ssh::transport::TransportConfig;
use x2ssh::vpn;

fn parse_user_host(s: &str) -> Result<(String, String), String> {
    let parts: Vec<&str> = s.splitn(2, '@').collect();
    if parts.len() != 2 {
        return Err("Expected format: USER@HOST".to_string());
    }
    Ok((parts[0].to_string(), parts[1].to_string()))
}

#[derive(Parser, Debug)]
#[command(name = "x2ssh")]
#[command(about = "SOCKS5 proxy and VPN tunnel over SSH")]
struct Cli {
    #[arg(value_name = "USER@HOST")]
    destination: String,

    /// Enable VPN mode (requires root/sudo for TUN and routing)
    #[arg(long = "vpn")]
    vpn: bool,

    /// Config file path
    #[arg(long = "config", value_name = "FILE")]
    config: Option<PathBuf>,

    /// VPN client address with prefix (e.g., 10.8.0.2/24)
    #[arg(long = "vpn-client-address", value_name = "ADDR/PREFIX")]
    vpn_client_address: Option<String>,

    /// VPN server address with prefix (e.g., 10.8.0.1/24)
    #[arg(long = "vpn-server-address", value_name = "ADDR/PREFIX")]
    vpn_server_address: Option<String>,

    /// Client TUN interface name (e.g., tun-x2ssh)
    #[arg(long = "vpn-client-tun", value_name = "NAME")]
    vpn_client_tun: Option<String>,

    /// TUN MTU in bytes
    #[arg(long = "vpn-mtu", value_name = "BYTES")]
    vpn_mtu: Option<u16>,

    /// CIDR to exclude from VPN (can be specified multiple times)
    #[arg(long = "vpn-exclude", value_name = "CIDR")]
    vpn_exclude: Vec<String>,

    /// PostUp command (can be specified multiple times; overrides config)
    #[arg(long = "vpn-post-up", value_name = "CMD")]
    vpn_post_up: Vec<String>,

    /// PreDown command (can be specified multiple times; overrides config)
    #[arg(long = "vpn-pre-down", value_name = "CMD")]
    vpn_pre_down: Vec<String>,

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

    /// Build VPN config by merging config file with CLI overrides.
    /// CLI overrides take precedence over config file values.
    fn vpn_config(&self) -> anyhow::Result<x2ssh::config::VpnConfig> {
        // Start with defaults
        let mut config = x2ssh::config::VpnConfig::default();

        // Load config file if specified
        if let Some(config_path) = &self.config
            && config_path.exists()
        {
            let app_config = AppConfig::load(config_path)?;
            config = app_config.vpn;
        }

        // Apply CLI overrides
        if let Some(client_address) = &self.vpn_client_address {
            config.client_address = client_address.clone();
        }
        if let Some(server_address) = &self.vpn_server_address {
            config.server_address = server_address.clone();
        }
        if let Some(client_tun) = &self.vpn_client_tun {
            config.client_tun = client_tun.clone();
        }
        if let Some(mtu) = self.vpn_mtu {
            config.mtu = mtu;
        }
        if !self.vpn_exclude.is_empty() {
            config.exclude = self.vpn_exclude.clone();
        }
        // CLI PostUp/PreDown completely override config file if specified
        if !self.vpn_post_up.is_empty() {
            config.post_up = self.vpn_post_up.clone();
        }
        if !self.vpn_pre_down.is_empty() {
            config.pre_down = self.vpn_pre_down.clone();
        }

        Ok(config)
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let cli = Cli::parse();

    // Load VPN config if VPN mode is enabled
    let _vpn_config = if cli.vpn {
        let config = cli.vpn_config()?;
        info!("VPN mode enabled");
        info!("VPN client address: {}", config.client_address);
        info!("Client TUN: {}", config.client_tun);
        Some(config)
    } else {
        None
    };

    // SOCKS5 mode requires -D flag (for now, until VPN is fully implemented)
    if cli.socks_addr.is_none() && !cli.vpn {
        return Err(anyhow::anyhow!(
            "Either --socks (-D) or --vpn must be specified"
        ));
    }

    if cli.socks_addr.is_some() {
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
    } else {
        let vpn_config = cli.vpn_config()?;
        let transport_config = cli
            .transport_config()
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        let ssh_server_ip = resolve_host(&transport_config.host).await?;

        vpn::run_vpn(&vpn_config, ssh_server_ip).await?;
        Ok(())
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

async fn resolve_host(host: &str) -> anyhow::Result<IpAddr> {
    use tokio::net::lookup_host;

    let addr = format!("{}:22", host);
    let addrs: Vec<_> = lookup_host(&addr).await?.collect();

    addrs
        .into_iter()
        .next()
        .map(|a| a.ip())
        .ok_or_else(|| anyhow::anyhow!("Failed to resolve host: {}", host))
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
        assert!(!cli.vpn);
    }

    #[test]
    fn test_vpn_flag_parsing() {
        let cli = Cli::try_parse_from(["x2ssh", "--vpn", "user@host.com"]).unwrap();
        assert!(cli.vpn);
        assert!(cli.socks_addr.is_none());
    }

    #[test]
    fn test_vpn_with_overrides() {
        let cli = Cli::try_parse_from([
            "x2ssh",
            "--vpn",
            "--vpn-client-address",
            "10.9.0.2/24",
            "--vpn-server-address",
            "10.9.0.1/24",
            "--vpn-mtu",
            "1280",
            "--vpn-exclude",
            "192.168.0.0/16",
            "--vpn-exclude",
            "10.0.0.0/8",
            "user@host.com",
        ])
        .unwrap();

        assert!(cli.vpn);
        assert_eq!(cli.vpn_client_address, Some("10.9.0.2/24".to_string()));
        assert_eq!(cli.vpn_server_address, Some("10.9.0.1/24".to_string()));
        assert_eq!(cli.vpn_mtu, Some(1280));
        assert_eq!(cli.vpn_exclude, vec![
            "192.168.0.0/16".to_string(),
            "10.0.0.0/8".to_string()
        ]);
    }

    #[test]
    fn test_vpn_post_up_pre_down() {
        let cli = Cli::try_parse_from([
            "x2ssh",
            "--vpn",
            "--vpn-post-up",
            "sysctl -w net.ipv4.ip_forward=1",
            "--vpn-post-up",
            "iptables -t nat -I POSTROUTING -o eth0 -j MASQUERADE",
            "--vpn-pre-down",
            "iptables -t nat -D POSTROUTING -o eth0 -j MASQUERADE",
            "user@host.com",
        ])
        .unwrap();

        assert!(cli.vpn);
        assert_eq!(cli.vpn_post_up, vec![
            "sysctl -w net.ipv4.ip_forward=1".to_string(),
            "iptables -t nat -I POSTROUTING -o eth0 -j MASQUERADE".to_string(),
        ]);
        assert_eq!(cli.vpn_pre_down, vec![
            "iptables -t nat -D POSTROUTING -o eth0 -j MASQUERADE".to_string()
        ]);
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
