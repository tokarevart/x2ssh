use std::net::IpAddr;
use std::net::Ipv4Addr;

use crate::config::VpnConfig;

pub struct TunDevice {
    #[cfg(target_os = "linux")]
    inner: tun_rs::AsyncDevice,
}

impl TunDevice {
    #[cfg(target_os = "linux")]
    pub async fn create(config: &VpnConfig) -> anyhow::Result<Self> {
        let client_ip = config.client_ip()?;
        let mtu = config.mtu;
        let tun_name = &config.client_tun;

        let device = create_linux_tun(client_ip, mtu, tun_name).await?;
        Ok(Self { inner: device })
    }

    #[cfg(target_os = "windows")]
    pub async fn create(_config: &VpnConfig) -> anyhow::Result<Self> {
        todo!("Windows TUN not yet implemented - Phase 4")
    }

    #[cfg(target_os = "linux")]
    pub fn inner(&self) -> &tun_rs::AsyncDevice {
        &self.inner
    }

    #[cfg(target_os = "linux")]
    pub async fn recv(&self, buf: &mut [u8]) -> anyhow::Result<usize> {
        self.inner.recv(buf).await.map_err(Into::into)
    }

    #[cfg(target_os = "linux")]
    pub async fn send(&self, packet: &[u8]) -> anyhow::Result<()> {
        self.inner.send(packet).await?;
        Ok(())
    }

    #[cfg(target_os = "windows")]
    pub async fn recv(&self, _buf: &mut [u8]) -> anyhow::Result<usize> {
        todo!("Windows TUN recv - Phase 4")
    }

    #[cfg(target_os = "windows")]
    pub async fn send(&self, _packet: &[u8]) -> anyhow::Result<()> {
        todo!("Windows TUN send - Phase 4")
    }
}

#[cfg(target_os = "linux")]
async fn create_linux_tun(ip: IpAddr, mtu: u16, name: &str) -> anyhow::Result<tun_rs::AsyncDevice> {
    let ip = match ip {
        IpAddr::V4(ip) => ip,
        IpAddr::V6(_) => anyhow::bail!("IPv6 not yet supported"),
    };

    let (addr, prefix) = ip_to_addr_prefix(ip);

    let device = tun_rs::DeviceBuilder::new()
        .name(name)
        .ipv4(addr, prefix, None)
        .mtu(mtu)
        .build_async()?;

    Ok(device)
}

#[cfg(target_os = "linux")]
fn ip_to_addr_prefix(ip: Ipv4Addr) -> (Ipv4Addr, u8) {
    (ip, 24)
}

#[cfg(target_os = "windows")]
fn ip_to_addr_prefix(ip: std::net::Ipv4Addr) -> (std::net::Ipv4Addr, u8) {
    (ip, 24)
}
