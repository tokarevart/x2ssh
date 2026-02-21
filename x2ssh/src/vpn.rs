pub mod agent;
pub mod hooks;
pub mod routing;
pub mod session;
pub mod tun;

use std::net::IpAddr;

use session::VpnSession;
use tracing::info;

use crate::config::VpnConfig;
use crate::transport::Transport;

pub fn check_root() -> anyhow::Result<()> {
    #[cfg(target_os = "linux")]
    {
        let uid = unsafe { libc::getuid() };
        if uid != 0 {
            return Err(anyhow::anyhow!(
                "VPN mode requires root privileges. Run with sudo."
            ));
        }
    }

    #[cfg(target_os = "windows")]
    {
        todo!("Windows administrator check - Phase 4")
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        return Err(anyhow::anyhow!(
            "VPN mode is only supported on Linux and Windows"
        ));
    }

    Ok(())
}

pub async fn run_vpn(
    transport: &Transport,
    config: &VpnConfig,
    ssh_server_ip: IpAddr,
) -> anyhow::Result<()> {
    check_root()?;

    info!("Starting VPN session");
    let mut session = VpnSession::start(transport, config, ssh_server_ip).await?;

    info!("VPN tunnel active. Press Ctrl+C to disconnect.");

    tokio::select! {
        result = session.forward() => {
            info!("Forwarding ended: {:?}", result);
        }
        _ = tokio::signal::ctrl_c() => {
            info!("Received shutdown signal");
        }
    }

    session.cleanup(transport, config).await?;

    Ok(())
}
