# VPN Tunnel Design

This document describes the VPN tunnel feature for x2ssh.

## Overview

VPN mode provides system-level tunnel for all TCP and UDP traffic, routing the entire network stack through SSH with zero server-side setup beyond standard SSH.

**Key Features:**
- Full tunnel (default route) with configurable exclusions
- TCP forwarding via existing SSH `direct-tcpip` channels
- UDP forwarding via lightweight server-side agent
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
│  ┌─────────────┐     ┌──────────────┐     ┌─────────────────┐  │
│  │ TUN Device  │────▶│ Packet Parse │────▶│  SSH Transport  │──┼──▶ SSH Server
│  │ (tun-rs)    │     │ (etherparse) │     │    (russh)      │  │
│  └─────────────┘     └──────────────┘     └─────────────────┘  │
│         │                    │                      │           │
│         │                    │                      │           │
│    All network          TCP ──────▶ direct-tcpip   │           │
│     traffic                                         │           │
│   (via routing)         UDP ──────▶ Agent Channel  │           │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────┐
│                      SERVER (Linux)                             │
│                                                                 │
│  ┌─────────────┐                ┌──────────────────────────┐   │
│  │   SSHD      │───direct-tcpip─▶│   Target TCP Endpoints   │   │
│  │ (existing)  │                └──────────────────────────┘   │
│  └──────┬──────┘                                                │
│         │                                                       │
│         │ exec channel                                          │
│         │ (stdin/stdout)                                        │
│         │                                                       │
│  ┌──────▼──────┐                ┌──────────────────────────┐   │
│  │ UDP Agent   │───UDP packets──▶│   Target UDP Endpoints   │   │
│  │ (x2ssh-vpn) │                └──────────────────────────┘   │
│  └─────────────┘                                                │
│   Single process,                                               │
│   manages all UDP flows                                         │
└─────────────────────────────────────────────────────────────────┘
```

### Design Decisions

| Aspect | Decision | Rationale |
|--------|----------|-----------|
| **Client Platform** | Linux + Windows | Cross-platform abstraction layer, test on Linux during dev |
| **Server Platform** | Linux only | Simplifies agent, matches typical SSH server deployment |
| **Agent Architecture** | Single long-lived agent | More efficient for VPN (many concurrent UDP flows) |
| **IP Processing** | Parse packets, forward raw | Use etherparse, simple and low overhead |
| **Routing** | Full tunnel + exclusions | Default route through VPN, exclude SSH + user CIDRs |
| **Privileges** | Require root/sudo | Simple approach, clearly documented |
| **Agent Protocol** | Length-prefixed binary | 4-byte BE length + bincode payload, fast and compact |
| **Agent Channel** | SSH exec (stdin/stdout) | Simple, reuses existing SSH session |
| **DNS** | Forward as UDP packets | Automatic, no special handling needed |
| **MTU** | 1400 bytes | Conservative default, accounts for SSH overhead |
| **Agent Deployment** | Base64 over SSH exec | No SFTP dependency, works everywhere |
| **Error Handling** | Retry with backoff | Reuse existing retry policy for resilience |

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

### 2. Packet Parser

**Library:** `etherparse` (zero-copy, no_std compatible)

**Logic:**
```rust
async fn process_tun_packet(packet: &[u8]) -> Result<PacketAction> {
    let parsed = etherparse::PacketHeaders::from_ip_slice(packet)?;
    
    match parsed.transport {
        Some(TransportHeader::Tcp(tcp)) => {
            let dest = SocketAddr::new(parsed.ip.unwrap().destination_addr(), tcp.destination_port);
            Ok(PacketAction::ForwardTcp { dest, payload: packet })
        }
        Some(TransportHeader::Udp(udp)) => {
            let dest = SocketAddr::new(parsed.ip.unwrap().destination_addr(), udp.destination_port);
            Ok(PacketAction::ForwardUdp { dest, payload: packet })
        }
        _ => Ok(PacketAction::Drop), // ICMP, etc. - future enhancement
    }
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

### 4. Server-Side UDP Agent

**Binary:** `x2ssh-vpn-agent` (statically compiled with musl for Linux)

**Architecture:**
```
Agent Process (single, long-lived)
    │
    ├─ stdin/stdout ←→ SSH channel (x2ssh client)
    │
    ├─ UDP Flow Map: HashMap<FlowId, UdpSocket>
    │      FlowId = (src_addr, dst_addr)
    │
    └─ Tasks:
        ├─ stdin reader: receive commands, spawn UDP flows
        ├─ UDP receivers: per-flow tasks that read UDP and write to stdout
        └─ Cleanup task: remove idle flows after timeout
```

**Protocol:**
```rust
// Wire format: [4-byte BE length][bincode payload]

#[derive(Serialize, Deserialize)]
enum AgentCommand {
    SendUdp {
        flow_id: u64,
        dest: SocketAddr,
        payload: Vec<u8>,
    },
    CloseFlow {
        flow_id: u64,
    },
    Shutdown,
}

#[derive(Serialize, Deserialize)]
enum AgentResponse {
    UdpReceived {
        flow_id: u64,
        from: SocketAddr,
        payload: Vec<u8>,
    },
    FlowClosed {
        flow_id: u64,
    },
    Error {
        message: String,
    },
}
```

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
4. Deploy UDP agent to server
5. Start agent via SSH exec
6. Configure routing (default route + exclusions)
7. Main loop:
   - Read packets from TUN
   - Parse and classify (TCP vs UDP)
   - Forward TCP via direct-tcpip
   - Forward UDP via agent
   - Write responses back to TUN
8. On shutdown:
   - Restore original routing
   - Stop agent
   - Delete agent binary
   - Close TUN interface
```

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
│   │           ├── packet.rs     # Packet parsing
│   │           ├── session.rs    # VPN session management
│   │           └── agent.rs      # Agent communication
│   │
│   ├── x2ssh-vpn-agent/          # Server-side UDP agent
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── main.rs
│   │       ├── protocol.rs       # Shared protocol types
│   │       └── udp_flow.rs       # UDP flow management
│   │
│   └── x2ssh-common/             # Shared types (protocol definitions)
│       ├── Cargo.toml
│       └── src/
│           ├── lib.rs
│           └── protocol.rs       # AgentCommand, AgentResponse
│
├── tests-e2e/
│   └── tests/
│       └── test_vpn.py           # VPN E2E tests
```

## Implementation Phases

### Phase 1: Foundation & Agent (2-3 weeks)

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

### Phase 2: TUN Interface & Routing (Linux Only) (2 weeks)

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

### Phase 3: Packet Processing & TCP Forwarding (2 weeks)

**Goal:** Parse packets from TUN, forward TCP via SSH

**Tasks:**
- [ ] Add `etherparse` dependency
- [ ] Implement packet parser
- [ ] Refactor TCP forwarding for VPN mode
- [ ] Implement TUN → TCP → SSH flow
- [ ] E2E test: HTTP request through VPN

**Deliverables:** TCP traffic working through VPN

---

### Phase 4: UDP Forwarding via Agent (2 weeks)

**Goal:** Forward UDP packets through agent

**Tasks:**
- [ ] Integrate agent deployment
- [ ] Implement UDP packet forwarding
- [ ] Handle DNS queries (port 53)
- [ ] Agent error handling + reconnection
- [ ] E2E tests: DNS, UDP echo, concurrent flows

**Deliverables:** Full VPN with TCP + UDP on Linux

---

### Phase 5: Windows Support (3 weeks)

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

### Phase 6: Polish & Optimization (2 weeks)

**Goal:** Production-ready VPN mode

**Tasks:**
- [ ] Performance optimization
- [ ] Logging and diagnostics
- [ ] Configuration file support
- [ ] Error message improvements
- [ ] Documentation
- [ ] Security audit

**Deliverables:** Production-ready VPN

---

**Total Timeline:** 13-14 weeks (~3 months)

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
etherparse = "0.19"
base64 = "0.22"
rand = "0.8"
x2ssh-common = { path = "../x2ssh-common" }

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
tokio = { version = "1.45", features = ["rt", "net", "io-std", "time", "macros"] }
anyhow = "1.0"
bincode = "1.3"
serde = { version = "1.0", features = ["derive"] }
tracing = "0.1"
tracing-subscriber = "0.3"
x2ssh-common = { path = "../x2ssh-common" }

[profile.release]
strip = true
lto = true
codegen-units = 1
panic = "abort"
```

### Why These Crates?

**`tun-rs`** (v2.8+):
- Best performance (70.6 Gbps peak, 2.3x faster than Go)
- Cross-platform (Linux, Windows, macOS, BSD, iOS, Android)
- Hardware offload support (TSO/GSO)
- Active maintenance (last release Feb 2026)
- Excellent documentation

**`etherparse`** (v0.19):
- Zero-copy packet parsing
- Minimal dependencies (just `arrayvec`)
- no_std compatible
- Active maintenance (last release Aug 2025)
- Clean API for IP/TCP/UDP parsing

**`rtnetlink`** (v0.20):
- Linux netlink API for routing
- Async API (tokio compatible)
- Part of well-maintained rust-netlink ecosystem
- Clean abstraction over low-level netlink

## Security Considerations

### Agent Isolation

- [ ] Agent runs with user privileges (no root)
- [ ] Agent only accepts connections from x2ssh (via SSH)
- [ ] Agent cleans up temp files on exit
- [ ] Agent validates all inputs from protocol

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
| TCP Throughput | >100 Mbps | Limited by SSH encryption |
| UDP Throughput | >50 Mbps | Limited by agent protocol |
| Latency Overhead | <10ms | Compared to direct connection |
| Connection Setup | <2s | SSH + agent deploy + routing |
| Memory Usage | <50 MB | Client + agent combined |
| CPU Usage | <10% | At 50 Mbps throughput |

## Future Enhancements

### Post-MVP Features

1. **IPv6 Support**
   - Full IPv6 packet forwarding
   - IPv6 routing configuration
   - Dual-stack handling

2. **ICMP Support**
   - Ping through VPN
   - Traceroute support
   - Requires raw socket on server

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
