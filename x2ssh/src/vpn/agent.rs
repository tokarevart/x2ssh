use crate::transport::Transport;

/// Embedded x2ssh-agent binary (compiled via build.rs)
pub const AGENT_BINARY: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/x2ssh-agent"));

/// Deploy the agent binary to the remote host and start it
///
/// **Note:** Full implementation deferred to Phase 3 when VPN integration
/// begins. This stub verifies the agent binary is embedded correctly.
pub async fn deploy_and_start(_transport: &Transport, _tun_name: &str) -> anyhow::Result<()> {
    // Stub: Full SSH exec channel implementation in Phase 3
    // For now, just verify the binary is embedded
    if AGENT_BINARY.is_empty() {
        return Err(anyhow::anyhow!("Agent binary not embedded"));
    }

    todo!("Full agent deployment via SSH exec channel - implement in Phase 3")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_binary_embedded() {
        // Verify the binary is embedded and has reasonable size
        assert!(!AGENT_BINARY.is_empty());
        assert!(AGENT_BINARY.len() > 1000); // Should be at least 1KB
    }
}
