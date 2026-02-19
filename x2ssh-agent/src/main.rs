use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 || args[1] != "--tun" {
        eprintln!("Usage: x2ssh-agent --tun <NAME>");
        std::process::exit(1);
    }
    let tun_name = &args[2];

    let tun = open_tun(tun_name).await?;
    let tun = Arc::new(tun);

    let tun_for_write = Arc::clone(&tun);
    let mut stdin = tokio::io::stdin();

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

    let tun_to_client = tokio::spawn(async move {
        let mut buf = vec![0u8; 2048];
        loop {
            match tun_for_read.recv(&mut buf).await {
                Ok(n) => {
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
}

async fn open_tun(name: &str) -> anyhow::Result<tun_rs::AsyncDevice> {
    let dev = tun_rs::DeviceBuilder::new().name(name).build_async()?;
    Ok(dev)
}
