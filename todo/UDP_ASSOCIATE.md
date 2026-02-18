# UDP Associate Design for SOCKS5

## Overview

SOCKS5 UDP Associate allows clients to send/receive UDP packets through the proxy. Since SSH only supports TCP channels (`direct-tcpip`), we need a server-side agent to bridge UDP ↔ TCP over SSH.

## Problem Statement

- **SOCKS5 UDP Associate**: Client requests ability to send/receive UDP packets through proxy
- **SSH limitation**: SSH protocol only supports TCP channels, no native UDP forwarding
- **Solution approach**: Deploy a server-side agent that bridges UDP ↔ TCP over SSH

## Design Options

### Option 1: Single Long-Lived Agent (Connection Pool Model)

**Architecture:**
```
Client UDP → SOCKS5 → x2ssh → SSH TCP tunnel → Server Agent → Target UDP endpoints
                                                    ↓
                                            Manages N UDP sockets
                                            (one per active association)
```

**Characteristics:**
- **One agent process** handles all UDP associations for this x2ssh instance
- Agent listens on a Unix socket or TCP port bound to localhost
- Custom protocol over SSH channel: `{association_id, operation, target_addr, udp_payload}`
- Agent maintains state: `HashMap<association_id, UdpSocket>`

**Tradeoffs:**

| Pros | Cons |
|------|------|
| ✅ Resource efficient (one process) | ❌ More complex state management |
| ✅ Lower startup overhead | ❌ All associations fail if agent crashes |
| ✅ Single binary to deploy | ❌ Need careful association lifecycle management |
| ✅ Can share NAT mappings | ❌ More complex custom protocol |

---

### Option 2: Per-Association Agent (Process Pool Model)

**Architecture:**
```
Client UDP → SOCKS5 → x2ssh → SSH TCP tunnel → Dedicated Agent → Single UDP socket
                              ↓
                          SSH TCP tunnel → Dedicated Agent → Single UDP socket
                              ↓
                          SSH TCP tunnel → Dedicated Agent → Single UDP socket
```

**Characteristics:**
- **One agent process per UDP association**
- Each agent is simple: read from stdin → send UDP, receive UDP → write to stdout
- Agent spawned via SSH exec channel: `ssh user@host './agent'`
- Protocol: raw UDP packets (no framing needed if using length-prefix or SOCKS5 UDP format)

**Tradeoffs:**

| Pros | Cons |
|------|------|
| ✅ Simple agent (stateless, single UDP socket) | ❌ Higher resource usage (N processes) |
| ✅ Isolation (one association crash ≠ all fail) | ❌ Repeated agent deployment overhead |
| ✅ Trivial protocol (stdin/stdout + UDP) | ❌ Cannot share NAT mappings |
| ✅ No association tracking needed | ❌ More SSH channels to manage |

---

### Option 3: Hybrid (Manager + Workers)

**Architecture:**
```
Client → x2ssh → SSH channel → Agent Manager
                                    ↓
                              Spawns worker threads/tasks
                              (one per association)
```

**Characteristics:**
- One agent process with internal task pool
- Manager accepts commands, spawns tokio tasks for each UDP association
- Workers are lightweight async tasks, not processes

**Tradeoffs:**

| Pros | Cons |
|------|------|
| ✅ Balance of simplicity and efficiency | ❌ More complex agent implementation |
| ✅ Shared process, isolated failure domains | ❌ Still needs state management |
| ✅ One binary deployment | ❌ Agent must be robust (all eggs in one basket) |

---

## Custom Protocol Design (for Options 1 & 3)

```rust
// Wire format (length-delimited messages)
Message {
    association_id: u32,
    operation: enum {
        Associate { bind_addr: SocketAddr },  // Create new UDP association
        SendTo { target: SocketAddr, payload: Vec<u8> },
        Close,
    }
}

Response {
    association_id: u32,
    result: enum {
        Associated { local_addr: SocketAddr },
        Received { from: SocketAddr, payload: Vec<u8> },
        Error(String),
    }
}
```

### Protocol Notes

- Use length-prefixed framing (4 bytes BE length + payload)
- Messages are bincode/postcard serialized for efficiency
- Bidirectional: both sides can initiate (client sends, server receives UDP)

---

## Agent Binary Requirements

**Must-haves:**
- Statically linked with musl (`x86_64-unknown-linux-musl` target)
- No dynamic dependencies (libc, openssl, etc.)
- Small binary size (<5 MB compressed)
- Embedded in x2ssh binary at compile-time
- Auto-deployed to `/tmp/.x2ssh-udp-agent-<random>` on first use
- Cleaned up on disconnect

**Deployment strategy:**
```rust
// In x2ssh
const AGENT_BINARY: &[u8] = include_bytes!(env!("X2SSH_AGENT_PATH"));

async fn deploy_agent(session: &Transport) -> Result<PathBuf> {
    let remote_path = format!("/tmp/.x2ssh-agent-{}", random_hex());
    // Upload via SFTP or cat + base64
    // Make executable: chmod +x
    Ok(remote_path)
}
```

**Build process:**
```bash
# Build agent with musl for static linking
cargo build --release --target x86_64-unknown-linux-musl -p x2ssh-agent

# Compress (optional)
upx --best target/x86_64-unknown-linux-musl/release/x2ssh-agent

# Embed in main binary at compile time
export X2SSH_AGENT_PATH=target/x86_64-unknown-linux-musl/release/x2ssh-agent
```

---

## Recommended Design: Option 2 (Per-Association Agent)

**Rationale:**
1. **Simplicity**: Agent is ~100 lines of Rust, just stdin/stdout ↔ UDP socket
2. **Testing**: Easy to test agent independently (pipe data in, check UDP packets out)
3. **Reliability**: No shared state, no complex lifecycle management
4. **SOCKS5 alignment**: SOCKS5 UDP Associate is typically short-lived anyway
5. **Future migration path**: Can optimize to Option 1 later if profiling shows bottleneck

**Implementation roadmap:**
```
1. Create agent crate (workspace member: `crates/agent/`)
2. Implement simple UDP relay: stdin→UDP, UDP→stdout
3. Add agent binary embedding in x2ssh
4. Implement agent deployment via SSH
5. Modify SOCKS5 handler to spawn agent per UDP Associate
6. Add E2E tests for UDP proxy
```

---

## Open Questions

### 1. Association Lifetime

How long should UDP associations stay alive? SOCKS5 spec is vague.

**Options:**
- **A**: Keep alive until client closes TCP control connection
- **B**: Timeout after N seconds of inactivity (e.g., 60s)
- **C**: Both (timeout OR explicit close)

**Recommendation**: Option C (whichever comes first)

### 2. Agent Cleanup

When to delete the agent binary from server?

**Options:**
- **A**: On disconnect (requires tracking in-use binaries)
- **B**: On next connect (cleanup old, deploy new)
- **C**: Never (treat as cache, reuse existing if present)

**Recommendation**: Option C with version check (reuse if hash matches, redeploy if different)

### 3. DNS Resolution

Who resolves target hostnames in UDP packets?

**Options:**
- **A**: Client-side (x2ssh resolves before sending to agent)
- **B**: Server-side (agent resolves, more accurate but complex)

**Note**: SOCKS5 UDP packets can contain IP or hostname

**Recommendation**: Option A for simplicity (client-side resolution)

### 4. Binary Size

Is 1-2 MB agent binary acceptable for embedding?

- Compression (zstd/xz) can reduce to ~300-500 KB
- UPX can compress to ~200 KB
- Pure Rust with no_std could be <100 KB

**Recommendation**: Start without compression, optimize if needed

### 5. Multi-Platform

Start with Linux-only, or also plan for Windows from day 1?

- Windows would need different deployment (PowerShell/CMD, PE binary)
- Windows has different temp paths (`%TEMP%`)

**Recommendation**: Linux-only for initial implementation, Windows in Phase 2

---

## Implementation Phases

### Phase 1: Agent Binary
- [ ] Create `crates/agent/` workspace member
- [ ] Implement UDP relay (stdin/stdout ↔ UDP socket)
- [ ] Add length-prefixed framing for UDP packets
- [ ] Build script for musl target
- [ ] Unit tests for agent

### Phase 2: Agent Deployment
- [ ] Embed agent binary in x2ssh at compile-time
- [ ] Implement agent upload via SSH (SFTP or base64 + cat)
- [ ] Agent version checking/caching
- [ ] Agent cleanup on disconnect

### Phase 3: SOCKS5 Integration
- [ ] Modify `src/socks.rs` to handle `UDPAssociate` command
- [ ] Spawn agent via SSH exec channel
- [ ] Bidirectional UDP packet relay (SOCKS5 ↔ agent ↔ UDP)
- [ ] Association lifecycle management (timeout + explicit close)

### Phase 4: Testing
- [ ] E2E tests for UDP proxy (DNS queries, simple UDP echo)
- [ ] Test agent failure scenarios
- [ ] Test concurrent UDP associations
- [ ] Performance benchmarking

### Phase 5: Documentation
- [ ] Update DESIGN.md with UDP architecture
- [ ] Update README.md with UDP support info
- [ ] Add troubleshooting guide for UDP issues

---

## Security Considerations

1. **Agent isolation**: Agent runs with user privileges (no root needed)
2. **Temp file cleanup**: Ensure agent binary is deleted on disconnect
3. **Path traversal**: Validate remote paths when deploying agent
4. **Resource limits**: Set max UDP packet size (e.g., 64 KB) to prevent DoS
5. **Association limits**: Limit concurrent UDP associations per session (e.g., 100)

---

## Alternative Approaches Considered

### A. Use SSH port forwarding for UDP (rejected)

SSH doesn't support UDP forwarding natively. Would require custom SSH server.

### B. VPN mode instead of SOCKS5 UDP (deferred)

Full VPN tunnel is overkill for SOCKS5 UDP. See VPN.md for future plans.

### C. Use existing tools like socat (rejected)

Requires server-side setup, violates "zero server setup" principle.

---

## References

- **SOCKS5 RFC**: https://datatracker.ietf.org/doc/html/rfc1928
- **SOCKS5 UDP**: Section 7 of RFC 1928
- **VPN.md**: Future VPN tunnel design (more comprehensive UDP support)
- **DESIGN.md**: Current x2ssh architecture

---

## Next Steps

1. Discuss open questions with team
2. Prototype agent binary (simple stdin/stdout relay)
3. Test agent independently before integration
4. Implement Phase 1 (Agent Binary)

