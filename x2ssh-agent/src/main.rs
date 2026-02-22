use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 || args[1] != "--ip" {
        eprintln!("Usage: x2ssh-agent --ip <SUBNET_IP/PREFIX>");
        eprintln!("Example: x2ssh-agent --ip 10.8.0.1/24");
        std::process::exit(1);
    }
    let subnet_ip = &args[2];

    let tun = create_tun(subnet_ip).await?;
    let tun = Arc::new(tun);

    let tun_for_write = Arc::clone(&tun);
    let mut stdin = tokio::io::stdin();

    // Client → Server TUN: Read framed packet from stdin, write to TUN
    let client_to_tun = tokio::spawn(async move {
        loop {
            match proto::read_framed(&mut stdin).await {
                Ok(packet) => {
                    if let Err(e) = tun_for_write.send(&packet).await {
                        eprintln!("TUN send error: {}", e);
                        return Err::<(), anyhow::Error>(e.into());
                    }
                }
                Err(e) => {
                    eprintln!("stdin read error: {}", e);
                    return Err::<(), anyhow::Error>(e);
                }
            }
        }
    });

    let tun_for_read = Arc::clone(&tun);
    let mut stdout = tokio::io::stdout();

    // Server TUN → Client: Read from TUN, write framed to stdout
    let tun_to_client = tokio::spawn(async move {
        let mut buf = vec![0u8; 2048];
        loop {
            match tun_for_read.recv(&mut buf).await {
                Ok(n) => {
                    eprintln!("TUN→CLIENT: sending {} bytes", n);
                    if let Err(e) = proto::write_framed(&mut stdout, &buf[..n]).await {
                        eprintln!("stdout write error: {}", e);
                        return Err::<(), anyhow::Error>(e);
                    }
                }
                Err(e) => {
                    eprintln!("TUN recv error: {}", e);
                    return Err::<(), anyhow::Error>(e.into());
                }
            }
        }
    });

    tokio::select! {
        result = client_to_tun => {
            if let Err(e) = result {
                eprintln!("Client->TUN task failed: {}", e);
            }
        }
        result = tun_to_client => {
            if let Err(e) = result {
                eprintln!("TUN->Client task failed: {}", e);
            }
        }
    }

    Ok(())
    // TUN is destroyed automatically when the process exits — no cleanup needed
}

/// Create a TUN interface with the given subnet IP, configure it, and bring it
/// up. The OS destroys this interface automatically when the process exits.
async fn create_tun(subnet_ip: &str) -> anyhow::Result<tun_rs::AsyncDevice> {
    // Parse "addr/prefix" — e.g. "10.8.0.1/24"
    let (addr_str, prefix_str) = subnet_ip
        .split_once('/')
        .ok_or_else(|| anyhow::anyhow!("expected ADDR/PREFIX, got: {subnet_ip}"))?;
    let prefix: u8 = prefix_str.parse()?;

    let dev = tun_rs::DeviceBuilder::new()
        .ipv4(addr_str, prefix, None)
        .mtu(1400)
        .build_async()?;
    Ok(dev)
}
