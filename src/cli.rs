use std::net::SocketAddr;
use std::path::PathBuf;

use clap::Parser;

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
pub struct Cli {
    #[arg(value_name = "USER@HOST")]
    pub destination: String,

    #[arg(short = 'D', long = "socks", value_name = "ADDR")]
    pub socks_addr: Option<String>,

    #[arg(short = 'p', long = "port", default_value = "22")]
    pub port: u16,

    #[arg(short = 'i', long = "identity", value_name = "FILE")]
    pub identity: Option<PathBuf>,

    #[arg(long = "retry-max", value_name = "N")]
    pub retry_max: Option<u32>,

    #[arg(long = "retry-delay", value_name = "MS", default_value = "1000")]
    pub retry_delay: u64,

    #[arg(long = "retry-backoff", value_name = "N", default_value = "2")]
    pub retry_backoff: f64,

    #[arg(long = "retry-max-delay", value_name = "MS", default_value = "30000")]
    pub retry_max_delay: u64,

    #[arg(long = "health-interval", value_name = "MS", default_value = "5000")]
    pub health_interval: u64,
}

impl Cli {
    pub fn user_host(&self) -> Result<(String, String), String> {
        parse_user_host(&self.destination)
    }

    pub fn socks_socket_addr(&self) -> Result<SocketAddr, String> {
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
