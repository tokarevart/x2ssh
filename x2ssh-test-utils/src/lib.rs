use std::path::PathBuf;
use std::time::Duration;

use testcontainers::GenericImage;
use testcontainers::core::WaitFor;
use testcontainers::runners::AsyncRunner;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use x2ssh::retry::RetryPolicy;
use x2ssh::transport::TransportConfig;

pub struct SshContainer {
    pub port: u16,
    _container: testcontainers::ContainerAsync<GenericImage>,
}

impl SshContainer {
    pub async fn start() -> Self {
        let container = GenericImage::new("x2ssh-test-sshd", "latest")
            .with_wait_for(WaitFor::message_on_stderr("Server listening on"))
            .start()
            .await
            .expect("Failed to start container");

        let port = container
            .get_host_port_ipv4(22)
            .await
            .expect("Failed to get port");

        tokio::time::sleep(Duration::from_millis(500)).await;

        Self {
            port,
            _container: container,
        }
    }

    pub fn host(&self) -> &str {
        "127.0.0.1"
    }

    pub fn transport_config(&self) -> TransportConfig {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let key_path = manifest_dir.join("../tests/fixtures/keys/id_ed25519");

        TransportConfig {
            retry_policy: RetryPolicy {
                max_attempts: Some(3),
                initial_delay: Duration::from_millis(100),
                backoff: 2.0,
                max_delay: Duration::from_secs(5),
            },
            health_interval: Duration::from_secs(1),
            key_path: Some(key_path),
            user: "root".to_string(),
            host: self.host().to_string(),
            port: self.port,
        }
    }
}

pub struct Socks5Client {
    addr: std::net::SocketAddr,
}

impl Socks5Client {
    pub fn new(addr: std::net::SocketAddr) -> Self {
        Self { addr }
    }

    pub async fn connect(&self, target: std::net::SocketAddr) -> std::io::Result<TcpStream> {
        let mut stream = TcpStream::connect(self.addr).await?;

        stream.write_all(&[0x05, 0x01, 0x00]).await?;
        stream.flush().await?;

        let mut response = [0u8; 2];
        stream.read_exact(&mut response).await?;

        if response[0] != 0x05 || response[1] != 0x00 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::ConnectionRefused,
                "SOCKS5 handshake failed",
            ));
        }

        let port_bytes = target.port().to_be_bytes();
        if target.ip().is_ipv4() {
            let ip = match target.ip() {
                std::net::IpAddr::V4(v4) => v4,
                _ => unreachable!(),
            };
            let mut req = vec![0x05, 0x01, 0x00, 0x01];
            req.extend_from_slice(&ip.octets());
            req.extend_from_slice(&port_bytes);
            stream.write_all(&req).await?;
        } else {
            let ip = match target.ip() {
                std::net::IpAddr::V6(v6) => v6,
                _ => unreachable!(),
            };
            let mut req = vec![0x05, 0x01, 0x00, 0x04];
            req.extend_from_slice(&ip.octets());
            req.extend_from_slice(&port_bytes);
            stream.write_all(&req).await?;
        }
        stream.flush().await?;

        let mut response = [0u8; 10];
        stream.read_exact(&mut response).await?;

        if response[1] != 0x00 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::ConnectionRefused,
                "SOCKS5 connect failed",
            ));
        }

        Ok(stream)
    }
}

pub fn container_echo_addr() -> std::net::SocketAddr {
    std::net::SocketAddr::new(
        std::net::IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1)),
        8080,
    )
}
