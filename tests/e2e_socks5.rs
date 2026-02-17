use std::sync::Arc;
use std::time::Duration;

use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use x2ssh::socks;
use x2ssh::transport::Transport;
use x2ssh_test_utils::Socks5Client;
use x2ssh_test_utils::SshContainer;
use x2ssh_test_utils::container_echo_addr;

#[tokio::test]
async fn socks5_handshake_success() {
    let container = SshContainer::start().await;
    let config = container.transport_config();
    let transport = Arc::new(Transport::connect(config).await.expect("Failed to connect"));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let socks_addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        loop {
            if let Ok((socket, _)) = listener.accept().await {
                let t = transport.clone();
                tokio::spawn(async move {
                    let _ = socks::serve(t, socket).await;
                });
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let client = Socks5Client::new(socks_addr);
    let echo_addr = container_echo_addr();

    let result = client.connect(echo_addr).await;
    assert!(result.is_ok(), "SOCKS5 handshake should succeed");
}

#[tokio::test]
async fn socks5_connect_tcp_forward() {
    let container = SshContainer::start().await;
    let config = container.transport_config();
    let transport = Arc::new(Transport::connect(config).await.expect("Failed to connect"));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let socks_addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        loop {
            if let Ok((socket, _)) = listener.accept().await {
                let t = transport.clone();
                tokio::spawn(async move {
                    let _ = socks::serve(t, socket).await;
                });
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let echo_addr = container_echo_addr();
    let client = Socks5Client::new(socks_addr);

    let mut stream = client.connect(echo_addr).await.unwrap();

    stream.write_all(b"hello world").await.unwrap();

    let mut response = [0u8; 11];
    stream.read_exact(&mut response).await.unwrap();

    assert_eq!(&response, b"hello world");
}

#[tokio::test]
async fn socks5_multiple_concurrent_connections() {
    let container = SshContainer::start().await;
    let config = container.transport_config();
    let transport = Arc::new(Transport::connect(config).await.expect("Failed to connect"));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let socks_addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        loop {
            if let Ok((socket, _)) = listener.accept().await {
                let t = transport.clone();
                tokio::spawn(async move {
                    let _ = socks::serve(t, socket).await;
                });
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let echo_addr = container_echo_addr();

    let mut handles = vec![];

    for i in 0..5 {
        let client = Socks5Client::new(socks_addr);

        handles.push(tokio::spawn(async move {
            let mut stream = client.connect(echo_addr).await.unwrap();

            let msg = format!("message {}", i);
            stream.write_all(msg.as_bytes()).await.unwrap();

            let mut response = vec![0u8; msg.len()];
            stream.read_exact(&mut response).await.unwrap();

            assert_eq!(response, msg.as_bytes());
        }));
    }

    for handle in handles {
        handle.await.unwrap();
    }
}

#[tokio::test]
async fn socks5_large_data_transfer() {
    let container = SshContainer::start().await;
    let config = container.transport_config();
    let transport = Arc::new(Transport::connect(config).await.expect("Failed to connect"));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let socks_addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        loop {
            if let Ok((socket, _)) = listener.accept().await {
                let t = transport.clone();
                tokio::spawn(async move {
                    let _ = socks::serve(t, socket).await;
                });
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let echo_addr = container_echo_addr();
    let client = Socks5Client::new(socks_addr);

    let mut stream = client.connect(echo_addr).await.unwrap();

    let data = vec![0xABu8; 100_000];
    stream.write_all(&data).await.unwrap();

    let mut response = vec![0u8; 100_000];
    let mut offset = 0;
    while offset < 100_000 {
        let n = stream.read(&mut response[offset..]).await.unwrap();
        offset += n;
    }

    assert_eq!(response, data);
}
