use std::net::IpAddr;
use std::sync::Arc;

use tracing::debug;
use tracing::error;
use tracing::info;

use super::agent;
use super::hooks;
use super::routing::RoutingManager;
use super::tun::TunDevice;
use crate::config::VpnConfig;
use crate::transport::Transport;

pub struct VpnSession {
    tun: Arc<TunDevice>,
    routing: RoutingManager,
    agent: agent::AgentChannel,
    #[allow(dead_code)]
    ssh_server_ip: IpAddr,
    cleaned_up: bool,
}

impl VpnSession {
    pub async fn start(
        transport: &Transport,
        config: &VpnConfig,
        ssh_server_ip: IpAddr,
    ) -> anyhow::Result<Self> {
        info!("Creating TUN device: {}", config.client_tun);
        let tun = TunDevice::create(config).await?;

        info!("Setting up routing");
        let mut routing = RoutingManager::new().await?;
        routing.setup(config, ssh_server_ip).await?;

        info!("Deploying VPN agent");
        agent::deploy(transport).await?;

        info!("Starting VPN agent");
        let agent = agent::start(transport, &config.server_address).await?;

        info!("Running PostUp hooks");
        hooks::run_post_up(transport, config).await?;

        info!("VPN session started");

        Ok(Self {
            tun: Arc::new(tun),
            routing,
            agent,
            ssh_server_ip,
            cleaned_up: false,
        })
    }

    pub async fn forward(&self) -> anyhow::Result<()> {
        info!("Starting packet forwarding");

        let tun = Arc::clone(&self.tun);
        let agent = self.agent.clone();

        let mut tun_to_agent = tokio::spawn(async move {
            let mut buf = vec![0u8; 2048];
            loop {
                match tun.recv(&mut buf).await {
                    Ok(n) => {
                        debug!("TUN→Agent: {} bytes", n);
                        if let Err(e) = agent.send_packet(&buf[..n]).await {
                            error!("Failed to send packet to agent: {}", e);
                            return Err(e);
                        }
                    }
                    Err(e) => {
                        error!("TUN recv error: {}", e);
                        return Err(e);
                    }
                }
            }
        });

        let tun = Arc::clone(&self.tun);
        let agent = self.agent.clone();

        let mut agent_to_tun = tokio::spawn(async move {
            loop {
                match agent.recv_packet().await {
                    Ok(Some(packet)) => {
                        debug!("Agent→TUN: {} bytes", packet.len());
                        if let Err(e) = tun.send(&packet).await {
                            debug!("TUN send failed (continuing): {}", e);
                        }
                    }
                    Ok(None) => {
                        info!("Agent channel closed");
                        return Ok(());
                    }
                    Err(e) => {
                        error!("Agent recv error: {}", e);
                        return Err(e);
                    }
                }
            }
        });

        let result = tokio::select! {
            result = &mut tun_to_agent => {
                info!("TUN→Agent task finished");
                agent_to_tun.abort();
                result
            }
            result = &mut agent_to_tun => {
                info!("Agent→TUN task finished");
                tun_to_agent.abort();
                result
            }
        };

        result?
    }

    pub async fn cleanup(
        &mut self,
        transport: &Transport,
        config: &VpnConfig,
    ) -> anyhow::Result<()> {
        if self.cleaned_up {
            return Ok(());
        }

        info!("Cleaning up VPN session");

        hooks::run_pre_down(transport, config).await;

        if let Err(e) = self.agent.close().await {
            error!("Agent close error: {}", e);
        }

        if let Err(e) = self.routing.cleanup().await {
            error!("Routing cleanup error: {}", e);
        }

        self.cleaned_up = true;
        info!("VPN session cleaned up");
        Ok(())
    }

    #[cfg(target_os = "linux")]
    pub fn tun(&self) -> &tun_rs::AsyncDevice {
        self.tun.inner()
    }

    pub fn agent(&self) -> &agent::AgentChannel {
        &self.agent
    }
}

impl Drop for VpnSession {
    fn drop(&mut self) {
        if !self.cleaned_up {
            #[cfg(target_os = "linux")]
            {
                if let Ok(rt) = tokio::runtime::Handle::try_current() {
                    rt.block_on(async {
                        if let Err(e) = self.routing.cleanup().await {
                            error!("VPN cleanup error during drop: {}", e);
                        }
                    });
                }
            }
        }
    }
}
