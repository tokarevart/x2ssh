use std::net::IpAddr;

use ipnet::IpNet;

use crate::config::VpnConfig;

pub struct RoutingState {
    original_default_route: Option<RouteInfo>,
    exclusion_routes: Vec<RouteInfo>,
}

#[derive(Debug, Clone)]
pub struct RouteInfo {
    pub destination: IpNet,
    pub gateway: Option<IpAddr>,
    pub interface: String,
}

pub struct RoutingManager {
    #[cfg(target_os = "linux")]
    #[allow(dead_code)]
    handle: rtnetlink::Handle,
    state: RoutingState,
}

impl RoutingManager {
    #[cfg(target_os = "linux")]
    pub async fn new() -> anyhow::Result<Self> {
        let (connection, handle, _) = rtnetlink::new_connection()?;
        tokio::spawn(connection);
        Ok(Self {
            handle,
            state: RoutingState {
                original_default_route: None,
                exclusion_routes: Vec::new(),
            },
        })
    }

    #[cfg(target_os = "windows")]
    pub async fn new() -> anyhow::Result<Self> {
        todo!("Windows routing not yet implemented - Phase 4")
    }

    #[cfg(target_os = "linux")]
    pub async fn setup(&mut self, config: &VpnConfig, ssh_server_ip: IpAddr) -> anyhow::Result<()> {
        let tun_name = &config.client_tun;
        let server_ip = config.server_ip()?;

        self.save_original_default_route().await?;

        self.route_ssh_server_via_original_gateway(ssh_server_ip)
            .await?;

        self.set_default_route_via_tun(tun_name, server_ip).await?;

        for exclusion in &config.exclude {
            let net: IpNet = exclusion.parse()?;
            self.add_exclusion_route(net).await?;
        }

        Ok(())
    }

    #[cfg(target_os = "windows")]
    pub async fn setup(
        &mut self,
        _config: &VpnConfig,
        _ssh_server_ip: IpAddr,
    ) -> anyhow::Result<()> {
        todo!("Windows routing not yet implemented - Phase 4")
    }

    #[cfg(target_os = "linux")]
    async fn save_original_default_route(&mut self) -> anyhow::Result<()> {
        let route = get_default_route().await?;
        self.state.original_default_route = route;
        Ok(())
    }

    #[cfg(target_os = "linux")]
    async fn route_ssh_server_via_original_gateway(
        &mut self,
        ssh_ip: IpAddr,
    ) -> anyhow::Result<()> {
        if let Some(ref original) = self.state.original_default_route {
            add_route_via_gateway(ssh_ip, original.gateway, &original.interface).await?;
        }
        Ok(())
    }

    #[cfg(target_os = "linux")]
    async fn set_default_route_via_tun(
        &mut self,
        tun_name: &str,
        gateway: IpAddr,
    ) -> anyhow::Result<()> {
        delete_default_route().await?;
        add_default_route(gateway, tun_name).await?;
        Ok(())
    }

    #[cfg(target_os = "linux")]
    async fn add_exclusion_route(&mut self, net: IpNet) -> anyhow::Result<()> {
        if let Some(ref original) = self.state.original_default_route {
            add_route_via_gateway(net, original.gateway, &original.interface).await?;
            self.state.exclusion_routes.push(RouteInfo {
                destination: net,
                gateway: original.gateway,
                interface: original.interface.clone(),
            });
        }
        Ok(())
    }

    #[cfg(target_os = "linux")]
    pub async fn cleanup(&mut self) -> anyhow::Result<()> {
        delete_default_route().await?;

        if let Some(ref original) = self.state.original_default_route
            && let Some(gw) = original.gateway
        {
            add_default_route(gw, &original.interface).await?;
        }

        for route in &self.state.exclusion_routes {
            delete_route(route.destination).await?;
        }
        self.state.exclusion_routes.clear();

        Ok(())
    }

    #[cfg(target_os = "windows")]
    pub async fn cleanup(&mut self) -> anyhow::Result<()> {
        todo!("Windows routing not yet implemented - Phase 4")
    }
}

#[cfg(target_os = "linux")]
async fn get_default_route() -> anyhow::Result<Option<RouteInfo>> {
    use std::net::Ipv4Addr;

    let output = tokio::process::Command::new("ip")
        .args(["route", "show", "default"])
        .output()
        .await?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let line = stdout.lines().next();

    if let Some(line) = line {
        let parts: Vec<&str> = line.split_whitespace().collect();
        let mut gateway = None;
        let mut interface = None;

        for i in 0..parts.len() {
            if parts[i] == "via" && i + 1 < parts.len() {
                gateway = parts[i + 1].parse::<Ipv4Addr>().ok().map(IpAddr::V4);
            }
            if parts[i] == "dev" && i + 1 < parts.len() {
                interface = Some(parts[i + 1].to_string());
            }
        }

        if let Some(iface) = interface {
            return Ok(Some(RouteInfo {
                destination: "0.0.0.0/0".parse()?,
                gateway,
                interface: iface,
            }));
        }
    }

    Ok(None)
}

#[cfg(target_os = "linux")]
async fn delete_default_route() -> anyhow::Result<()> {
    tokio::process::Command::new("ip")
        .args(["route", "del", "default"])
        .output()
        .await?;
    Ok(())
}

#[cfg(target_os = "linux")]
async fn add_default_route(gateway: IpAddr, interface: &str) -> anyhow::Result<()> {
    tokio::process::Command::new("ip")
        .args([
            "route",
            "add",
            "default",
            "via",
            &gateway.to_string(),
            "dev",
            interface,
        ])
        .output()
        .await?;
    Ok(())
}

#[cfg(target_os = "linux")]
async fn add_route_via_gateway(
    dest: impl Into<IpNet>,
    gateway: Option<IpAddr>,
    interface: &str,
) -> anyhow::Result<()> {
    let dest = dest.into();

    if let Some(gw) = gateway {
        tokio::process::Command::new("ip")
            .args([
                "route",
                "add",
                &dest.to_string(),
                "via",
                &gw.to_string(),
                "dev",
                interface,
            ])
            .output()
            .await?;
    } else {
        tokio::process::Command::new("ip")
            .args(["route", "add", &dest.to_string(), "dev", interface])
            .output()
            .await?;
    }

    Ok(())
}

#[cfg(target_os = "linux")]
async fn delete_route(dest: IpNet) -> anyhow::Result<()> {
    tokio::process::Command::new("ip")
        .args(["route", "del", &dest.to_string()])
        .output()
        .await?;
    Ok(())
}
