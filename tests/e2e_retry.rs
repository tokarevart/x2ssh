mod common;

use std::path::PathBuf;
use std::time::Duration;

use common::SshContainer;
use x2ssh::retry::RetryPolicy;
use x2ssh::transport::Transport;
use x2ssh::transport::TransportConfig;

#[test]
fn retry_backoff_calculation() {
    let policy = RetryPolicy {
        max_attempts: Some(5),
        initial_delay: Duration::from_millis(100),
        backoff: 2.0,
        max_delay: Duration::from_millis(10000),
    };

    assert_eq!(policy.delay_for_attempt(0), Duration::from_millis(100));
    assert_eq!(policy.delay_for_attempt(1), Duration::from_millis(200));
    assert_eq!(policy.delay_for_attempt(2), Duration::from_millis(400));
    assert_eq!(policy.delay_for_attempt(3), Duration::from_millis(800));
}

#[test]
fn retry_should_retry_within_limit() {
    let policy = RetryPolicy {
        max_attempts: Some(3),
        ..Default::default()
    };

    assert!(policy.should_retry(0));
    assert!(policy.should_retry(1));
    assert!(policy.should_retry(2));
    assert!(!policy.should_retry(3));
    assert!(!policy.should_retry(4));
}

#[test]
fn retry_infinite_attempts() {
    let policy = RetryPolicy {
        max_attempts: None,
        ..Default::default()
    };

    assert!(policy.should_retry(0));
    assert!(policy.should_retry(100));
    assert!(policy.should_retry(1000));
}

#[tokio::test]
async fn retry_transport_connect_success() {
    let container = SshContainer::start().await;

    let config = container.transport_config();
    let result = Transport::connect(config).await;

    assert!(
        result.is_ok(),
        "Connection should succeed with valid config"
    );
}

#[tokio::test]
async fn retry_transport_connect_invalid_host() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let key_path = manifest_dir.join("tests/fixtures/keys/id_ed25519");

    let config = TransportConfig {
        retry_policy: RetryPolicy {
            max_attempts: Some(1),
            initial_delay: Duration::from_millis(10),
            backoff: 1.0,
            max_delay: Duration::from_millis(10),
        },
        health_interval: Duration::from_secs(1),
        key_path: Some(key_path),
        user: "root".to_string(),
        host: "255.255.255.255".to_string(),
        port: 22,
    };

    let result = Transport::connect(config).await;
    assert!(result.is_err(), "Connection to invalid host should fail");
}

#[tokio::test]
async fn retry_transport_connect_wrong_port() {
    let container = SshContainer::start().await;

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let key_path = manifest_dir.join("tests/fixtures/keys/id_ed25519");

    let config = TransportConfig {
        retry_policy: RetryPolicy {
            max_attempts: Some(1),
            initial_delay: Duration::from_millis(10),
            backoff: 1.0,
            max_delay: Duration::from_millis(10),
        },
        health_interval: Duration::from_secs(1),
        key_path: Some(key_path),
        user: "root".to_string(),
        host: container.host().to_string(),
        port: 9999,
    };

    let result = Transport::connect(config).await;
    assert!(result.is_err(), "Connection to wrong port should fail");
}
