use russh::Channel;
use russh::ChannelMsg;
use russh::client::Msg;
use tokio::sync::Mutex;
use tracing::debug;
use tracing::info;

use crate::transport::Transport;

pub const AGENT_BINARY: &[u8] = include_bytes!(env!("X2SSH_AGENT_PATH"));
const AGENT_PATH: &str = "/tmp/x2ssh-agent";

pub struct AgentChannel {
    channel: Mutex<Channel<Msg>>,
}

impl AgentChannel {
    pub async fn send_packet(&self, packet: &[u8]) -> anyhow::Result<()> {
        let channel = self.channel.lock().await;
        let mut framed = Vec::with_capacity(4 + packet.len());
        framed.extend_from_slice(&(packet.len() as u32).to_be_bytes());
        framed.extend_from_slice(packet);
        channel.data(&framed[..]).await?;
        Ok(())
    }

    pub async fn recv_packet(&self) -> anyhow::Result<Option<Vec<u8>>> {
        let mut channel = self.channel.lock().await;

        let mut len_buf = [0u8; 4];
        let mut len_read = 0;

        while len_read < 4 {
            match channel.wait().await {
                Some(ChannelMsg::Data { data }) => {
                    let remaining = 4 - len_read;
                    if data.len() >= remaining {
                        len_buf[len_read..].copy_from_slice(&data[..remaining]);
                        len_read = 4;
                    } else {
                        len_buf[len_read..len_read + data.len()].copy_from_slice(&data);
                        len_read += data.len();
                    }
                }
                Some(ChannelMsg::Eof) => return Ok(None),
                Some(msg) => debug!("Ignoring channel message while reading length: {:?}", msg),
                None => return Ok(None),
            }
        }

        let len = u32::from_be_bytes(len_buf) as usize;
        let mut packet = vec![0u8; len];
        let mut read = 0;

        while read < len {
            match channel.wait().await {
                Some(ChannelMsg::Data { data }) => {
                    let remaining = len - read;
                    if data.len() >= remaining {
                        packet[read..].copy_from_slice(&data[..remaining]);
                        read = len;
                    } else {
                        packet[read..read + data.len()].copy_from_slice(&data);
                        read += data.len();
                    }
                }
                Some(ChannelMsg::Eof) => return Ok(None),
                Some(msg) => debug!("Ignoring channel message while reading packet: {:?}", msg),
                None => return Ok(None),
            }
        }

        Ok(Some(packet))
    }

    pub async fn close(&self) -> anyhow::Result<()> {
        let channel = self.channel.lock().await;
        channel.close().await?;
        Ok(())
    }
}

pub async fn deploy(transport: &Transport) -> anyhow::Result<()> {
    info!("Deploying agent binary ({} bytes)", AGENT_BINARY.len());

    let encoded = base64_encode(AGENT_BINARY);
    let cmd = format!(
        "echo '{}' | base64 -d > {} && chmod +x {}",
        encoded, AGENT_PATH, AGENT_PATH
    );

    transport.exec_success(&cmd).await?;

    info!("Agent binary deployed to {}", AGENT_PATH);
    Ok(())
}

pub async fn start(transport: &Transport, server_address: &str) -> anyhow::Result<AgentChannel> {
    info!("Starting agent with IP {}", server_address);

    let channel = transport.open_session_channel().await?;

    let cmd = format!("sudo {} --ip {}", AGENT_PATH, server_address);
    channel.exec(true, cmd.as_bytes()).await?;

    info!("Agent started, channel ready for packet forwarding");

    Ok(AgentChannel {
        channel: Mutex::new(channel),
    })
}

fn base64_encode(data: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_binary_embedded() {
        assert!(!AGENT_BINARY.is_empty());
        assert!(AGENT_BINARY.len() > 1000);
    }
}
