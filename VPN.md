# VPN Tunnel Design

This document describes the VPN tunnel feature for x2ssh.

## Overview

VPN mode provides system-level tunnel for all TCP and UDP traffic, routing the entire network stack through SSH with zero server-side setup beyond standard SSH.

**Key Features:**
- Full tunnel (default route) with configurable exclusions
- TCP and UDP forwarding via unified agent protocol
- Single lightweight server-side agent handles all IP packets
- Cross-platform client (Linux + Windows)
- Linux server (standard SSH server, no special setup)
- Automatic agent deployment and lifecycle management

## CLI

```bash
sudo x2ssh --vpn <USER@HOST> [OPTIONS]

VPN Options:
      --vpn                    Enable VPN tunnel mode (requires root/sudo)
      --vpn-exclude <CIDR>     Exclude CIDR from VPN tunnel (can be repeated)
                               Example: --vpn-exclude 192.168.1.0/24
      --vpn-mtu <BYTES>        TUN interface MTU [default: 1400]
      --vpn-tun-name <NAME>    TUN interface name [default: tun-x2ssh]

Examples:
  sudo x2ssh --vpn user@server.com
  sudo x2ssh --vpn --vpn-exclude 10.0.0.0/8 user@server.com
  sudo x2ssh --vpn --vpn-mtu 1500 --vpn-tun-name mytun user@server.com
```

**Note:** VPN mode requires elevated privileges (root on Linux, Administrator on Windows) to create TUN interface and modify routing tables.

## Architecture

### High-Level Data Flow

```
┌─────────────────────────────────────────────────────────────────┐
│                        CLIENT (x2ssh)                           │
│                                                                 │
│  ┌─────────────┐     ┌──────────────┐     ┌─────────────────┐   │
│  │ TUN Device  │────▶│   Protocol   │───▶│  SSH Channel    │───┼──▶ SSH Server
│  │ (tun-rs)    │     │   Encoder    │     │  (exec stdin)   │   │    (exec)
│  └─────────────┘     └──────────────┘     └─────────────────┘   │
│         │                    │                      │           │
│         │              All IP packets          Single SSH       │
│    All network         (TCP + UDP)            exec channel      │
│     traffic            encoded in                               │
│   (via routing)        binary protocol                          │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────┐
│                      SERVER (Linux)                             │
│                                                                 │
│  ┌─────────────┐                                                │
│  │   SSHD      │                                                │
│  │ (existing)  │                                                │
│  └──────┬──────┘                                                │
│         │                                                       │
│         │ exec channel (stdin/stdout)                           │
│         │                                                       │
│  ┌──────▼──────────────────────────────────────────────────┐   │
│  │             VPN Agent (x2ssh-vpn)                        │   │
│  │                                                          │   │
│  │  ┌────────────┐      ┌──────────────┐                    │   │
│  │  │  Protocol  │─────▶│  Raw Socket  │───────────────────┼──▶ Internet
│  │  │  Decoder   │      │   Sender     │    IP packets      │   │
│  │  └────────────┘      └──────────────┘                    │   │
│  │         │                    │                           │   │
│  │    Parse frames         Send via raw                     │   │
│  │    from stdin           IP socket                        │   │
│  │                                                          │   │
│  │  ┌────────────┐      ┌──────────────┐                    │   │
│  │  │  Protocol  │◀─────│  Raw Socket  │◀──────────────────┼─── Internet
│  │  │  Encoder   │      │  Receiver    │   IP packets       │   │
│  │  └────────────┘      └──────────────┘                    │   │
│  │         │                                                │   │
│  │    Write frames                                          │   │
│  │    to stdout                                             │   │
│  │                                                          │   │
│  └──────────────────────────────────────────────────────────┘   │
│                                                                 │
│  Single process handles ALL traffic (TCP + UDP + ICMP)          │
│  via raw IP sockets                                             │
└─────────────────────────────────────────────────────────────────┘
```

### Design Decisions

| Aspect | Decision | Rationale |
|--------|----------|-----------|
| **Client Platform** | Linux + Windows | Cross-platform abstraction layer, test on Linux during dev |
| **Server Platform** | Linux only | Simplifies agent, matches typical SSH server deployment |
| **Agent Architecture** | Single agent with raw sockets | Handles all IP packets uniformly (TCP, UDP, ICMP) |
| **Packet Forwarding** | Raw IP packets via agent | Unified approach for TCP and UDP, simpler than separate paths |
| **Routing** | Full tunnel + exclusions | Default route through VPN, exclude SSH + user CIDRs |
| **Privileges** | Client: root/sudo, Server: CAP_NET_RAW | Client needs TUN, server needs raw socket |
| **Agent Protocol** | Length-prefixed raw IP packets | 4-byte BE length + raw IP packet, minimal overhead |
| **Agent Channel** | SSH exec (stdin/stdout) | Simple, reuses existing SSH session |
| **DNS** | Forward as IP packets | Automatic, no special handling needed |
| **MTU** | 1400 bytes | Conservative default, accounts for SSH + protocol overhead |
| **Agent Deployment** | Base64 over SSH exec | No SFTP dependency, works everywhere |
| **Error Handling** | Retry with backoff | Reuse existing retry policy for resilience |

### Why This Design?

**Raw IP Socket Forwarding:**
- ✅ Kernel handles all protocol logic (TCP state, UDP ports, ICMP, etc.)
- ✅ Agent is stateless and simple (~150 lines)
- ✅ Supports ALL protocols (TCP, UDP, ICMP, IPv6, future protocols)
- ✅ Minimal processing overhead - just framing and forwarding
- ✅ No packet parsing or protocol-specific logic needed
- ✅ Automatic support for advanced features (TCP options, fragmentation, etc.)

## Components

### 1. Client TUN Interface

**Linux:**
- Use `tun-rs` crate (v2.8+) for TUN device creation
- Requires root or `CAP_NET_ADMIN` capability
- Device: `/dev/net/tun`

**Windows:**
- Use `tun-rs` with Wintun driver
- Requires Administrator privileges
- Wintun driver must be installed separately

**Abstraction:**
```rust
pub trait TunDevice: AsyncRead + AsyncWrite {
    async fn create(name: &str, mtu: u16) -> Result<Self>;
    fn name(&self) -> &str;
    fn set_ip(&self, ip: IpAddr, netmask: IpAddr) -> Result<()>;
}
```

### 2. Protocol Encoder/Decoder

**Client Side:**

No parsing needed on client - just frame raw IP packets from TUN and send to agent.

```rust
async fn send_packet_to_agent(agent: &mut AgentHandle, packet: &[u8]) -> Result<()> {
    // Simple length-prefixed framing
    let len = (packet.len() as u32).to_be_bytes();
    agent.stdin.write_all(&len).await?;
    agent.stdin.write_all(packet).await?;
    agent.stdin.flush().await?;
    Ok(())
}

async fn recv_packet_from_agent(agent: &mut AgentHandle) -> Result<Vec<u8>> {
    // Read length prefix
    let mut len_buf = [0u8; 4];
    agent.stdout.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;
    
    // Read packet
    let mut packet = vec![0u8; len];
    agent.stdout.read_exact(&mut packet).await?;
    Ok(packet)
}
```

**Server Side (Agent):**

Same framing, but sends packets via raw IP socket.

```rust
async fn forward_packet(packet: &[u8], raw_socket: &RawSocket) -> Result<()> {
    // Parse just enough to get destination IP
    let ip_version = packet[0] >> 4;
    let dest_ip = match ip_version {
        4 => {
            // IPv4: bytes 16-19 are destination
            IpAddr::V4(Ipv4Addr::new(packet[16], packet[17], packet[18], packet[19]))
        }
        6 => {
            // IPv6: bytes 24-39 are destination
            // ... parse IPv6 address
        }
        _ => return Err(anyhow!("Invalid IP version")),
    };
    
    // Send raw IP packet
    raw_socket.send_to(packet, dest_ip).await?;
    Ok(())
}
```

### 3. Routing Configuration

**Linux:** Use `rtnetlink` for route manipulation

```rust
async fn setup_vpn_routes(
    tun_name: &str,
    exclusions: &[IpNetwork],
) -> Result<()> {
    // Add default route through TUN
    // Add exclusion routes (SSH server IP + user CIDRs)
    // Save original routes for cleanup
}
```

**Windows:** Use Windows routing APIs via `windows-sys`

**Abstraction:**
```rust
pub trait RouteManager {
    async fn add_default_route(&self, interface: &str) -> Result<()>;
    async fn add_exclusion(&self, network: IpNetwork) -> Result<()>;
    async fn cleanup(&self) -> Result<()>;
}
```

### 4. Server-Side VPN Agent

**Binary:** `x2ssh-vpn-agent` (statically compiled with musl for Linux)

**Architecture:**
```
Agent Process (single, long-lived)
    │
    ├─ stdin/stdout ←→ SSH channel (x2ssh client)
    │
    ├─ Raw IP Socket (requires CAP_NET_RAW or root)
    │      Sends and receives raw IP packets
    │
    └─ Tasks:
        ├─ stdin reader: receive framed IP packets, forward to network
        └─ socket reader: receive IP packets from network, frame and send to stdout
```

**Protocol:**

Extremely simple - just length-prefixed raw IP packets:

```rust
// Wire format: [4-byte BE length][raw IP packet]
// No complex serialization needed - just framing!

async fn run_agent() -> Result<()> {
    // Create raw IP socket (AF_INET, SOCK_RAW, IPPROTO_RAW)
    let raw_socket = create_raw_socket()?;
    
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    
    loop {
        tokio::select! {
            // Client → Network: Read packet from stdin, send via raw socket
            packet = read_framed_packet(&mut stdin) => {
                let packet = packet?;
                forward_to_network(&raw_socket, &packet).await?;
            }
            
            // Network → Client: Receive packet from network, write to stdout
            packet = recv_from_network(&raw_socket) => {
                let packet = packet?;
                write_framed_packet(&mut stdout, &packet).await?;
            }
        }
    }
}

async fn read_framed_packet(reader: &mut impl AsyncRead) -> Result<Vec<u8>> {
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;
    
    let mut packet = vec![0u8; len];
    reader.read_exact(&mut packet).await?;
    Ok(packet)
}

async fn write_framed_packet(writer: &mut impl AsyncWrite, packet: &[u8]) -> Result<()> {
    let len = (packet.len() as u32).to_be_bytes();
    writer.write_all(&len).await?;
    writer.write_all(packet).await?;
    writer.flush().await?;
    Ok(())
}

fn create_raw_socket() -> Result<RawSocket> {
    // socket(AF_INET, SOCK_RAW, IPPROTO_RAW)
    // Requires CAP_NET_RAW capability or root
}
```

**Key Simplifications:**
- No need to parse packet types (TCP/UDP/ICMP) - kernel handles it
- No need to maintain flow state - stateless forwarding
- No need for complex protocol - just framing
- Client-side IP stack handles TCP state, UDP ports, etc.

**Deployment Strategy:**
- Binary embedded in x2ssh at compile time using `include_bytes!`
- Uploaded to `/tmp/.x2ssh-vpn-<random>` via base64 encoding over SSH exec
- Made executable with `chmod +x`
- Started via SSH exec channel (stdin/stdout communication)
- Deleted on clean shutdown

### 5. VPN Session Lifecycle

```
1. User runs: sudo x2ssh --vpn user@server.com
2. x2ssh connects via SSH (existing transport)
3. Create TUN interface (platform-specific)
4. Deploy VPN agent to server
5. Start agent via SSH exec
6. Configure routing (default route + exclusions)
7. Main loop:
   - Read IP packets from TUN
   - Frame and send to agent (stdin)
   - Read framed packets from agent (stdout)
   - Write IP packets back to TUN
8. On shutdown:
   - Restore original routing
   - Stop agent
   - Delete agent binary
   - Close TUN interface
```

**Main Loop Implementation:**

```rust
async fn run_vpn_session(
    mut tun: impl TunDevice,
    mut agent: AgentHandle,
) -> Result<()> {
    let (mut tun_rx, mut tun_tx) = tun.split();
    let mut packet_buf = vec![0u8; 2048]; // MTU + IP headers
    
    loop {
        tokio::select! {
            // TUN → Agent: Read from TUN, send to agent
            n = tun_rx.read(&mut packet_buf) => {
                let packet = &packet_buf[..n?];
                
                // Frame and send to agent
                let len = (packet.len() as u32).to_be_bytes();
                agent.stdin.write_all(&len).await?;
                agent.stdin.write_all(packet).await?;
                agent.stdin.flush().await?;
            }
            
            // Agent → TUN: Read from agent, write to TUN
            packet = read_framed_packet(&mut agent.stdout) => {
                let packet = packet?;
                tun_tx.write_all(&packet).await?;
            }
            
            // Ctrl+C or disconnect signal
            _ = signal::ctrl_c() => {
                break;
            }
        }
    }
    
    cleanup_vpn_session().await?;
    Ok(())
}
```

**Key Benefits of This Approach:**

1. **Simplicity**: No need to distinguish TCP vs UDP on client or server
2. **Completeness**: Automatically supports ICMP, IPv6, and any future protocols
3. **Stateless**: Agent doesn't track flows, connections, or state
4. **Performance**: Minimal processing - just framing and raw socket I/O
5. **Correctness**: Kernel handles all protocol logic (TCP state machine, UDP ports, checksums, etc.)

## Project Structure

### Cargo Workspace

```
x2ssh/
├── Cargo.toml                    # Workspace root
├── crates/
│   ├── x2ssh/                    # Main binary (current src/)
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── main.rs
│   │       ├── lib.rs
│   │       ├── socks.rs
│   │       ├── transport.rs
│   │       ├── retry.rs
│   │       └── vpn/              # VPN module
│   │           ├── mod.rs
│   │           ├── tun.rs        # TUN abstraction
│   │           ├── router.rs     # Routing abstraction
│   │           ├── framing.rs    # Length-prefixed framing (shared with agent)
│   │           ├── session.rs    # VPN session management
│   │           └── agent.rs      # Agent deployment & communication
│   │
│   └── x2ssh-vpn-agent/          # Server-side VPN agent
│       ├── Cargo.toml
│       └── src/
│           ├── main.rs           # Simple raw socket forwarder (~150 lines)
│           └── raw_socket.rs     # Raw IP socket wrapper
│
├── tests-e2e/
│   └── tests/
│       └── test_vpn.py           # VPN E2E tests
```

**Note:** No shared `x2ssh-common` crate needed! The protocol is so simple (just length-prefixed framing) that both sides can implement it independently.

## Implementation Phases

### Phase 1: Foundation & Agent

**Goal:** Server-side agent + protocol + basic client scaffolding

**Tasks:**
- [ ] Create workspace structure
- [ ] Define protocol in `x2ssh-common`
- [ ] Implement UDP agent
- [ ] Build script for musl static linking
- [ ] Agent deployment logic
- [ ] Unit tests for agent

**Deliverables:** Working agent binary, deployment + communication working

---

### Phase 2: TUN Interface & Routing (Linux Only)

**Goal:** TUN device creation + routing configuration on Linux client

**Tasks:**
- [ ] Add `tun-rs` dependency
- [ ] Implement `LinuxTun` wrapper
- [ ] Add `rtnetlink` dependency
- [ ] Implement `LinuxRouteManager`
- [ ] CLI integration (`--vpn` flag)
- [ ] Privilege checking
- [ ] E2E test: TUN creation + routing

**Deliverables:** TUN interface + routing working on Linux

---

### Phase 3: Agent Integration & Packet Forwarding

**Goal:** Simple packet forwarding through agent (all protocols unified)

**Tasks:**
- [ ] Implement length-prefixed framing (client + agent)
- [ ] Implement agent raw socket logic
- [ ] Build agent with musl static linking
- [ ] Integrate agent deployment into VPN session
- [ ] Implement TUN → Agent → Network flow
- [ ] Implement Network → Agent → TUN flow
- [ ] Agent error handling + reconnection
- [ ] E2E tests: HTTP (TCP), DNS (UDP), ping (ICMP)

**Deliverables:** Full VPN with TCP + UDP + ICMP on Linux

---

### Phase 4: Windows Support

**Goal:** Cross-platform TUN and routing for Windows client

**Tasks:**
- [ ] Add Wintun driver dependency
- [ ] Implement `WindowsTun` wrapper
- [ ] Implement `WindowsRouteManager`
- [ ] Administrator privilege checking
- [ ] Wintun driver installation check
- [ ] E2E tests on Windows VM

**Deliverables:** VPN working on Windows client

---

### Phase 5: Polish & Optimization

**Goal:** Production-ready VPN mode

**Tasks:**
- [ ] Performance optimization
- [ ] Logging and diagnostics
- [ ] Configuration file support
- [ ] Error message improvements
- [ ] Documentation
- [ ] Security audit

**Deliverables:** Production-ready VPN

## Dependencies

### Main Binary (`crates/x2ssh`)

```toml
[dependencies]
# Existing
anyhow = "1.0"
tokio = { version = "1.45", features = ["full"] }
russh = "0.57"
tracing = "0.1"
clap = { version = "4.5", features = ["derive"] }

# VPN
tun-rs = { version = "2.8", features = ["async"] }
base64 = "0.22"
rand = "0.8"

[target.'cfg(target_os = "linux")'.dependencies]
rtnetlink = "0.20"
libc = "0.2"

[target.'cfg(target_os = "windows")'.dependencies]
windows-sys = { version = "0.59", features = [
    "Win32_NetworkManagement_IpHelper",
    "Win32_Foundation",
    "Win32_Security",
] }
```

### Agent Binary (`crates/x2ssh-vpn-agent`)

```toml
[dependencies]
tokio = { version = "1.45", features = ["rt", "net", "io-std", "macros"] }
anyhow = "1.0"
tracing = "0.1"
tracing-subscriber = "0.3"
libc = "0.2"  # For raw socket creation

[profile.release]
strip = true
lto = true
codegen-units = 1
panic = "abort"
opt-level = "z"  # Optimize for size
```

**Note:** Agent is extremely lightweight - no serialization framework needed, just raw sockets and framing!

### Why These Crates?

**`tun-rs`** (v2.8+):
- Best performance (70.6 Gbps peak, 2.3x faster than Go)
- Cross-platform (Linux, Windows, macOS, BSD, iOS, Android)
- Hardware offload support (TSO/GSO)
- Active maintenance (last release Feb 2026)
- Excellent documentation

**`rtnetlink`** (v0.20):
- Linux netlink API for routing
- Async API (tokio compatible)
- Part of well-maintained rust-netlink ecosystem
- Clean abstraction over low-level netlink

## Security Considerations

### Agent Isolation

- [ ] Agent requires `CAP_NET_RAW` capability (or root) for raw sockets
- [ ] Agent only accepts connections from x2ssh (via SSH exec channel)
- [ ] Agent cleans up temp files on exit
- [ ] Agent validates packet lengths (prevent buffer overflow)

### Route Leak Prevention

- [ ] SSH server IP excluded from VPN routing
- [ ] Exclusion for default gateway
- [ ] Test: Kill VPN, verify traffic doesn't leak

### DNS Leak Prevention

- [ ] DNS queries go through VPN tunnel
- [ ] Test with dnsleaktest.com
- [ ] Future: Add `--vpn-dns` flag for remote DNS

### Cleanup on Failure

- [ ] Routing restored on Ctrl+C
- [ ] Routing restored on panic
- [ ] Agent stopped on disconnect
- [ ] Temp files deleted
- [ ] TUN interface destroyed

## Performance Targets

| Metric | Target | Notes |
|--------|--------|-------|
| TCP Throughput | >100 Mbps | Limited by SSH encryption overhead |
| UDP Throughput | >100 Mbps | Same path as TCP, should be similar |
| Latency Overhead | <5ms | Minimal processing (just framing) |
| Connection Setup | <2s | SSH + agent deploy + routing |
| Memory Usage | <20 MB | Client + agent combined (minimal buffering) |
| CPU Usage | <5% | At 100 Mbps throughput (no parsing/reassembly) |

**Note:** Performance should be significantly better than complex approaches since we're just framing and forwarding raw packets.

## Future Enhancements

### Post-MVP Features

1. **IPv6 Support**
   - Add IPv6 routing configuration
   - Agent already supports IPv6 (raw socket handles both)
   - Just need routing table updates

2. **ICMP Already Supported!**
   - Ping works out of the box (raw socket forwards ICMP)
   - Traceroute works automatically
   - No additional work needed

3. **Split DNS**
   - `--vpn-dns` flag for remote DNS
   - DNS interception and forwarding
   - Prevent DNS leaks

4. **Connection Persistence**
   - Buffer packets during reconnects
   - Seamless agent reconnection
   - TCP connection preservation

5. **Performance Optimization**
   - Kernel bypass (io_uring on Linux)
   - Hardware offload (TSO/GSO)
   - Packet batching

6. **macOS Support**
   - TUN interface via tun-rs
   - Routing configuration
   - Keychain integration

7. **Configuration Profiles**
   - Save VPN exclusions per server
   - Auto-connect to VPN
   - Profile switching

## Testing Strategy

### Unit Tests

- [ ] Agent protocol serialization
- [ ] UDP flow management
- [ ] Packet parsing
- [ ] Route calculation
- [ ] TUN interface mock

### E2E Tests

- [ ] VPN TCP forwarding
- [ ] VPN UDP forwarding
- [ ] DNS queries
- [ ] Route exclusions
- [ ] Agent reconnection
- [ ] Cleanup on failure

### Manual Tests

- [ ] Linux client → Linux server
- [ ] Windows client → Linux server
- [ ] DNS leak test (dnsleaktest.com)
- [ ] IP leak test (ipleak.net)
- [ ] Routing cleanup on Ctrl+C
- [ ] Agent crash recovery

## References

- **tun-rs**: https://github.com/tun-rs/tun-rs
- **etherparse**: https://github.com/JulianSchmid/etherparse
- **rtnetlink**: https://github.com/rust-netlink/rtnetlink
- **WireGuard**: Reference for VPN routing patterns
- **DESIGN.md**: Current x2ssh architecture
