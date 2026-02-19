use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use russh::keys::PrivateKeyWithHashAlg;
use russh::keys::PublicKey;
use tokio::io::AsyncRead;
use tokio::io::AsyncWrite;
use tokio::net::ToSocketAddrs;
use tokio::sync::Mutex;
use tracing::debug;
use tracing::info;
use tracing::warn;

use crate::retry::RetryPolicy;

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[tokio::test]
    async fn transport_connect_invalid_host() {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let key_path = manifest_dir.join("../tests/fixtures/keys/id_ed25519");

        let config = TransportConfig {
            retry_policy: RetryPolicy {
                max_attempts: Some(1),
                initial_delay: Duration::from_millis(10),
                backoff: 1.0,
                max_delay: Duration::from_millis(10),
            },
            health_interval: Duration::from_secs(1),
            key_path: Some(key_path),
            user: "root".to_string(),
            host: "255.255.255.255".to_string(),
            port: 22,
        };

        let result = Transport::connect(config).await;
        assert!(result.is_err(), "Connection to invalid host should fail");
    }
}

struct Client;

impl russh::client::Handler for Client {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &PublicKey,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

pub struct Transport {
    session: Mutex<russh::client::Handle<Client>>,
    config: TransportConfig,
}

#[derive(Clone)]
pub struct TransportConfig {
    pub retry_policy: RetryPolicy,
    pub health_interval: Duration,
    pub key_path: Option<PathBuf>,
    pub user: String,
    pub host: String,
    pub port: u16,
}

impl Transport {
    pub async fn connect(config: TransportConfig) -> anyhow::Result<Self> {
        let session = Self::connect_once(&config).await?;
        Ok(Self {
            session: Mutex::new(session),
            config,
        })
    }

    async fn connect_once(
        config: &TransportConfig,
    ) -> anyhow::Result<russh::client::Handle<Client>> {
        let key_path = config
            .key_path
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No identity file specified"))?;

        let key_pair = russh::keys::load_secret_key(key_path, None)?;

        let ssh_config = Arc::new(russh::client::Config::default());
        let sh = Client;

        let addr = format!("{}:{}", config.host, config.port);
        let mut session = russh::client::connect(ssh_config, &addr, sh).await?;

        let auth_res = session
            .authenticate_publickey(
                &config.user,
                PrivateKeyWithHashAlg::new(
                    Arc::new(key_pair),
                    session.best_supported_rsa_hash().await?.flatten(),
                ),
            )
            .await?;

        if !auth_res.success() {
            anyhow::bail!("Authentication failed");
        }

        Ok(session)
    }

    pub async fn reconnect(&self) -> anyhow::Result<()> {
        let mut attempt = 0;
        loop {
            match Self::connect_once(&self.config).await {
                Ok(session) => {
                    *self.session.lock().await = session;
                    info!("SSH session reconnected");
                    return Ok(());
                }
                Err(e) => {
                    if !self.config.retry_policy.should_retry(attempt) {
                        return Err(e);
                    }

                    let delay = self.config.retry_policy.delay_for_attempt(attempt);
                    warn!(
                        "Connection attempt {} failed: {}. Retrying in {:?}...",
                        attempt, e, delay
                    );

                    tokio::time::sleep(delay).await;
                    attempt += 1;
                }
            }
        }
    }

    pub async fn check_alive(&self) -> anyhow::Result<()> {
        let session = self.session.lock().await;
        session
            .channel_open_session()
            .await
            .map(|ch| {
                tokio::spawn(async move {
                    let _ = ch.close().await;
                });
            })
            .map_err(|e| anyhow::anyhow!("Health check failed: {}", e))
    }

    pub async fn forward(
        &self,
        to: impl ToSocketAddrs,
        client: impl AsyncRead + AsyncWrite + Send + Unpin + 'static,
    ) -> anyhow::Result<()> {
        let to = tokio::net::lookup_host(to)
            .await?
            .next()
            .ok_or_else(|| anyhow::anyhow!("No address found"))?;

        let session = self.session.lock().await;
        let channel = session
            .channel_open_direct_tcpip(to.ip().to_string(), to.port() as _, "127.0.0.1", 0)
            .await?;

        let (ssh_rx, ssh_tx) = channel.split();
        let (client_rx, client_tx) = tokio::io::split(client);

        let jh = tokio::spawn(async move {
            use tokio::io::AsyncReadExt;

            let mut client_rx = client_rx;
            let mut buf = Vec::with_capacity(4096);
            loop {
                match client_rx.read_buf(&mut buf).await {
                    Ok(0) => {
                        let _ = ssh_tx.close().await;
                        return anyhow::Ok(());
                    }
                    Ok(_) => {
                        if ssh_tx.data(&*buf).await.is_err() {
                            return Ok(());
                        }
                        buf.clear();
                    }
                    Err(_) => return Ok(()),
                }
            }
        });

        use tokio::io::AsyncWriteExt;
        let mut client_tx = client_tx;
        let mut ssh_rx = ssh_rx;
        while let Some(msg) = ssh_rx.wait().await {
            match msg {
                russh::ChannelMsg::Data { ref data } => {
                    if client_tx.write_all(data).await.is_err() {
                        break;
                    }
                    if client_tx.flush().await.is_err() {
                        break;
                    }
                }
                russh::ChannelMsg::Eof => break,
                _ => debug!("Channel message: {:?}", msg),
            }
        }

        jh.abort();
        Ok(())
    }
}
