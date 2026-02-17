mod common;

use std::sync::Arc;
use std::time::Duration;

use common::SshContainer;
use common::container_echo_addr;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use x2ssh::transport::Transport;

#[tokio::test]
async fn transport_check_alive_succeeds() {
    let container = SshContainer::start().await;
    let config = container.transport_config();
    let transport = Transport::connect(config).await.expect("Failed to connect");

    let result = transport.check_alive().await;
    assert!(
        result.is_ok(),
        "Health check should succeed on live connection"
    );
}

#[tokio::test]
async fn transport_reconnect_after_connection_loss() {
    let container = SshContainer::start().await;
    let config = container.transport_config();
    let transport = Arc::new(Transport::connect(config).await.expect("Failed to connect"));

    assert!(transport.check_alive().await.is_ok());

    let result = transport.reconnect().await;
    assert!(result.is_ok(), "Reconnect should succeed");
    assert!(transport.check_alive().await.is_ok());
}

#[tokio::test]
async fn transport_forward_to_echo_server() {
    let container = SshContainer::start().await;
    let config = container.transport_config();
    let transport = Arc::new(Transport::connect(config).await.expect("Failed to connect"));

    let echo_addr = container_echo_addr();

    let (client_read, mut client_write) = tokio::io::duplex(4096);
    let forward_handle = tokio::spawn({
        let transport = transport.clone();
        async move { transport.forward(echo_addr, client_read).await }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    client_write.write_all(b"hello").await.unwrap();
    client_write.flush().await.unwrap();

    let mut response = [0u8; 5];
    client_write.read_exact(&mut response).await.unwrap();

    assert_eq!(&response, b"hello");

    drop(client_write);
    let _ = forward_handle.await;
}

#[tokio::test]
async fn transport_forward_after_reconnect() {
    let container = SshContainer::start().await;
    let config = container.transport_config();
    let transport = Arc::new(Transport::connect(config).await.expect("Failed to connect"));

    let echo_addr = container_echo_addr();

    {
        let (client_read, mut client_write) = tokio::io::duplex(4096);
        let forward_handle = tokio::spawn({
            let transport = transport.clone();
            async move { transport.forward(echo_addr, client_read).await }
        });

        tokio::time::sleep(Duration::from_millis(50)).await;

        client_write.write_all(b"before").await.unwrap();
        client_write.flush().await.unwrap();

        let mut response = [0u8; 6];
        client_write.read_exact(&mut response).await.unwrap();
        assert_eq!(&response, b"before");

        drop(client_write);
        let _ = forward_handle.await;
    }

    transport
        .reconnect()
        .await
        .expect("Reconnect should succeed");

    {
        let (client_read, mut client_write) = tokio::io::duplex(4096);
        let forward_handle = tokio::spawn({
            let transport = transport.clone();
            async move { transport.forward(echo_addr, client_read).await }
        });

        tokio::time::sleep(Duration::from_millis(50)).await;

        client_write.write_all(b"after").await.unwrap();
        client_write.flush().await.unwrap();

        let mut response = [0u8; 5];
        client_write.read_exact(&mut response).await.unwrap();
        assert_eq!(&response, b"after");

        drop(client_write);
        let _ = forward_handle.await;
    }
}
