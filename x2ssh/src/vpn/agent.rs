use std::sync::Arc;

use bytes::BytesMut;
use russh::ChannelMsg;
use russh::ChannelReadHalf;
use russh::ChannelWriteHalf;
use russh::client::Msg;
use tokio::sync::Mutex;
use tracing::debug;
use tracing::info;

use crate::transport::Transport;

pub const AGENT_BINARY: &[u8] = include_bytes!(env!("X2SSH_AGENT_PATH"));
const AGENT_PATH: &str = "/tmp/x2ssh-agent";

#[derive(Clone)]
pub struct AgentChannel {
    reader: Arc<Mutex<(ChannelReadHalf, BytesMut)>>,
    writer: Arc<Mutex<ChannelWriteHalf<Msg>>>,
}

impl AgentChannel {
    pub async fn send_packet(&self, packet: &[u8]) -> anyhow::Result<()> {
        let writer = self.writer.lock().await;
        let mut framed = Vec::with_capacity(4 + packet.len());
        framed.extend_from_slice(&(packet.len() as u32).to_be_bytes());
        framed.extend_from_slice(packet);
        writer.data(&framed[..]).await?;
        Ok(())
    }

    pub async fn recv_packet(&self) -> anyhow::Result<Option<Vec<u8>>> {
        let mut guard = self.reader.lock().await;
        let (reader, buffer) = &mut *guard;

        // Read length prefix (4 bytes)
        while buffer.len() < 4 {
            match reader.wait().await {
                Some(ChannelMsg::Data { data }) => {
                    debug!("AGENT→CLIENT: {} bytes on channel", data.len());
                    buffer.extend_from_slice(&data);
                }
                Some(ChannelMsg::Eof) => {
                    info!("AGENT→CLIENT: EOF");
                    return Ok(None);
                }
                Some(msg) => {
                    debug!("AGENT→CLIENT: other message: {:?}", msg);
                }
                None => {
                    info!("AGENT→CLIENT: channel closed");
                    return Ok(None);
                }
            }
        }

        let len = u32::from_be_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]) as usize;
        debug!("AGENT→CLIENT: expecting {} byte packet", len);

        // Read packet data
        while buffer.len() < 4 + len {
            match reader.wait().await {
                Some(ChannelMsg::Data { data }) => {
                    debug!("AGENT→CLIENT: {} more bytes", data.len());
                    buffer.extend_from_slice(&data);
                }
                Some(ChannelMsg::Eof) => return Ok(None),
                Some(msg) => debug!("AGENT→CLIENT: other message: {:?}", msg),
                None => return Ok(None),
            }
        }

        // Extract packet and consume from buffer
        let packet = buffer[4..4 + len].to_vec();
        let _ = buffer.split_to(4 + len);

        Ok(Some(packet))
    }

    pub async fn close(&self) -> anyhow::Result<()> {
        let writer = self.writer.lock().await;
        writer.close().await?;
        Ok(())
    }
}

pub async fn deploy(transport: &Transport) -> anyhow::Result<()> {
    info!("Deploying agent binary ({} bytes)", AGENT_BINARY.len());

    let mut channel = transport.open_session_channel().await?;
    channel
        .exec(true, b"cat > /tmp/x2ssh-agent && chmod +x /tmp/x2ssh-agent")
        .await?;

    channel.data(AGENT_BINARY).await?;
    channel.eof().await?;

    let mut exit_code = 0u32;
    while let Some(msg) = channel.wait().await {
        match msg {
            ChannelMsg::ExitStatus { exit_status } => {
                exit_code = exit_status;
            }
            ChannelMsg::Eof => break,
            _ => {}
        }
    }

    if exit_code != 0 {
        anyhow::bail!("Agent deployment failed with exit code {}", exit_code);
    }

    info!("Agent binary deployed to {}", AGENT_PATH);
    Ok(())
}

pub async fn start(transport: &Transport, server_address: &str) -> anyhow::Result<AgentChannel> {
    info!("Starting agent with IP {}", server_address);

    let channel = transport.open_session_channel().await?;

    let cmd = format!("sudo {} --ip {}", AGENT_PATH, server_address);
    channel.exec(true, cmd.as_bytes()).await?;

    let (reader, writer) = channel.split();

    info!("Agent started, channel ready for packet forwarding");

    Ok(AgentChannel {
        reader: Arc::new(Mutex::new((reader, BytesMut::with_capacity(2048)))),
        writer: Arc::new(Mutex::new(writer)),
    })
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
