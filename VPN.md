# VPN Tunnel Design

This document describes the VPN tunnel feature for x2ssh.

## Overview

VPN mode provides system-level tunnel for all TCP and UDP traffic, routing the entire network stack through SSH. Inspired by WireGuard's configuration model with PostUp/PreDown hooks for maximum flexibility.

**Key Features:**
- Full tunnel (default route) with configurable exclusions
- WireGuard-style configuration with PostUp/PreDown hooks
- User-configurable server-side setup (TUN, iptables, routing)
- Cross-platform client (Linux + Windows)
- Linux server (requires root/sudo for TUN and iptables)
- Automatic agent deployment and lifecycle management

## Configuration

### Config File Location

Uses platform-appropriate config directory via `directories` crate:

- **Linux**: `~/.config/x2ssh/config.toml`
- **macOS**: `~/Library/Application Support/x2ssh/config.toml`
- **Windows**: `C:\Users\<user>\AppData\Roaming\x2ssh\config.toml`

Override with `--config <FILE>` flag.

### Example Config File

```toml
# ~/.config/x2ssh/config.toml

[vpn]
# VPN subnet (client will use .2, server will use .1)
subnet = "10.8.0.0/24"

# Server-side TUN interface name
server_tun = "x2ssh0"

# Client-side TUN interface name  
client_tun = "tun-x2ssh"

# MTU for TUN interface
mtu = 1400

# CIDRs to exclude from VPN routing
exclude = ["192.168.0.0/16", "172.16.0.0/12"]

# PostUp: Commands run on server AFTER TUN is ready (but before agent starts)
# MVP: Use hardcoded values (variable substitution in Phase 6)
post_up = [
    "ip tuntap add mode tun name x2ssh0",
    "ip addr add 10.8.0.1/24 dev x2ssh0",
    "ip link set x2ssh0 up",
    "sysctl -w net.ipv4.ip_forward=1",
    "iptables -t nat -I POSTROUTING -o eth0 -j MASQUERADE",
]

# PreDown: Commands run on server BEFORE TUN is destroyed (after agent stops)
# Executed one-by-one even if some fail
pre_down = [
    "iptables -t nat -D POSTROUTING -o eth0 -j MASQUERADE",
    "ip link delete x2ssh0",
]

[connection]
# SSH connection settings (can be overridden per-connection via CLI)
port = 22

[retry]
# Retry policy for SSH reconnection
max_attempts = 0  # 0 = infinite
initial_delay_ms = 1000
backoff = 2.0
max_delay_ms = 30000
health_interval_ms = 5000
```

### Variable Substitution (Phase 6 - Future)

**Note:** MVP (Phases 1-5) uses hardcoded values in PostUp/PreDown commands. Variable substitution will be added in Phase 6.

Available variables in `post_up` and `pre_down` commands (Phase 6):

| Variable | Description | Example Value |
|----------|-------------|---------------|
| `{TUN}` | Server TUN interface name | `x2ssh0` |
| `{SUBNET}` | VPN subnet CIDR | `10.8.0.0/24` |
| `{SERVER_IP}` | Server TUN IP address | `10.8.0.1` |
| `{CLIENT_IP}` | Client TUN IP address | `10.8.0.2` |
| `{INTERFACE}` | Server outbound interface | `eth0` (auto-detected or from config) |

**Auto-detection (Phase 6):**
- `{INTERFACE}` is auto-detected via `ip route get 8.8.8.8` if not specified
- Can override with `server_interface = "eth0"` in config

## CLI

```bash
x2ssh --vpn [OPTIONS] <USER@HOST>

VPN Options:
      --config <FILE>              Config file [default: platform-specific]
      --vpn                        Enable VPN mode (requires root/sudo on client)
      
  # Override config file settings:
      --vpn-subnet <CIDR>          VPN subnet [config: vpn.subnet]
      --vpn-server-tun <NAME>      Server TUN name [config: vpn.server_tun]
      --vpn-client-tun <NAME>      Client TUN name [config: vpn.client_tun]
      --vpn-mtu <BYTES>            TUN MTU [config: vpn.mtu]
      --vpn-exclude <CIDR>         Exclude CIDR (can repeat) [config: vpn.exclude]
      --vpn-server-interface <IF>  Server outbound interface [auto-detect]
      
  # Override PostUp/PreDown entirely (all flags in a group replace config):
      --vpn-post-up <CMD>          PostUp command (can repeat)
      --vpn-pre-down <CMD>         PreDown command (can repeat)

Connection Options:
  -p, --port <PORT>                SSH port [default: 22]
  -i, --identity <FILE>            SSH private key

Examples:
  # Use config file defaults
  sudo x2ssh --vpn user@server.com

  # Override subnet and exclusions
  sudo x2ssh --vpn --vpn-subnet 10.9.0.0/24 --vpn-exclude 192.168.1.0/24 user@server.com

  # Use custom config
  sudo x2ssh --vpn --config /etc/x2ssh/work-vpn.toml user@server.com
  
  # Override PostUp/PreDown entirely
  sudo x2ssh --vpn \
    --vpn-post-up "ip tuntap add mode tun name wg0" \
    --vpn-post-up "ip link set wg0 up" \
    --vpn-pre-down "ip link delete wg0" \
    user@server.com
```

## Architecture

### High-Level Data Flow

```
┌──────────────────────────────────────────────────────────────┐
│                     CLIENT (x2ssh)                           │
│                                                              │
│  ┌─────────────┐   ┌──────────────┐   ┌─────────────────┐    │
│  │ TUN Device  │──▶│   Framing    │──▶│  SSH Channel   │────┼──▶ SSH Server
│  │ (tun-rs)    │   │   (4B len)   │   │  (exec stdin)   │    │
│  └─────────────┘   └──────────────┘   └─────────────────┘    │
│         │                                       │            │
│    All network                            Single SSH         │
│     traffic                              exec channel        │
│   (via routing)                                              │
└──────────────────────────────────────────────────────────────┘

┌──────────────────────────────────────────────────────────────┐
│                     SERVER (Linux)                           │
│                                                              │
│  ┌─────────────┐                                             │
│  │   SSHD      │                                             │
│  └──────┬──────┘                                             │
│         │ exec channel (stdin/stdout)                        │
│         │                                                    │
│  ┌──────▼─────────────────────────────────────────────┐      │
│  │         VPN Agent (x2ssh-agent)                    │      │
│  │                                                    │      │
│  │   stdin ──▶ Deframe ──▶ TUN ──▶ Kernel ──▶ Net   │──────┼──▶ Internet
│  │                          ↕                         │      │
│  │   stdout ◀─ Frame ◀──── TUN ◀── Kernel ◀─ Net    │◀─────┼─── Internet
│  │                                                    │      │
│  └────────────────────────────────────────────────────┘      │
│                                                              │
│  PostUp hooks create TUN, set up iptables NAT                │
│  PreDown hooks tear down iptables, delete TUN                │
└──────────────────────────────────────────────────────────────┘
```

### Design: WireGuard-Style with Server TUN

**Key insight:** Instead of using raw sockets, we create a TUN interface on the **server** too. The agent simply bridges client packets (stdin/stdout) ↔ server TUN interface.

**Why this works:**
- Server TUN interface has an IP in the VPN subnet (e.g., 10.8.0.1)
- Client packets arrive with source IP in VPN subnet (e.g., 10.8.0.2)
- iptables MASQUERADE rewrites source IP when packets leave server TUN → Internet
- Responses come back, iptables rewrites destination IP → 10.8.0.2
- Kernel routes packets to server TUN interface
- Agent reads from server TUN, sends to client via stdout
- **Simple, stateless, kernel does all the work!**

## Components

### 1. Client TUN Interface

**Managed by x2ssh automatically** (not user-configurable via hooks).

**Linux:**
- Created via `tun-rs` crate
- Requires root or `CAP_NET_ADMIN`
- Assigned IP: `<subnet>.2` (e.g., 10.8.0.2/24)

**Windows:**
- Created via `tun-rs` with Wintun driver
- Requires Administrator privileges
- User must install Wintun driver separately

**Client routing:**
- x2ssh automatically sets up routing: default route → TUN interface
- Excludes SSH server IP and user-specified CIDRs
- Restored on disconnect

### 2. Server-Side Setup (User-Configurable)

**Lifecycle:**

```
1. x2ssh connects via SSH
2. Runs PostUp commands (hardcoded in MVP, variables in Phase 6)
   - PostUp creates TUN, sets up iptables
   - If ANY PostUp command fails, abort startup
3. Deploys agent binary (raw bytes via SSH exec: `cat > /tmp/x2ssh-agent`)
4. Starts agent via SSH exec
5. VPN forwarding begins
...
(On disconnect or error)
6. Stops agent
7. Runs PreDown commands (one-by-one, errors ignored)
8. Cleanup complete
```

**Example PostUp (iptables) - MVP:**

```toml
# MVP: Hardcoded values (adjust eth0 to match your server's interface)
post_up = [
    "ip tuntap add mode tun name x2ssh0",
    "ip addr add 10.8.0.1/24 dev x2ssh0",
    "ip link set x2ssh0 up",
    "sysctl -w net.ipv4.ip_forward=1",
    "iptables -t nat -I POSTROUTING -o eth0 -j MASQUERADE",
]

pre_down = [
    "iptables -t nat -D POSTROUTING -o eth0 -j MASQUERADE",
    "ip link delete x2ssh0",
]
```

**Example PostUp (nftables) - Phase 6 with variables:**

```toml
# Phase 6: With variable substitution
post_up = [
    "ip tuntap add mode tun name {TUN}",
    "ip addr add {SERVER_IP}/24 dev {TUN}",
    "ip link set {TUN} up",
    "sysctl -w net.ipv4.ip_forward=1",
    "nft add table inet x2ssh",
    "nft add chain inet x2ssh postrouting { type nat hook postrouting priority 100 \\; }",
    "nft add rule inet x2ssh postrouting ip saddr {SUBNET} oif {INTERFACE} masquerade",
]

pre_down = [
    "nft delete table inet x2ssh",
    "ip link delete {TUN}",
]
```

**Example PostUp (with ufw) - Phase 6 with variables:**

```toml
# Phase 6: With variable substitution
post_up = [
    "ip tuntap add mode tun name {TUN}",
    "ip addr add {SERVER_IP}/24 dev {TUN}",
    "ip link set {TUN} up",
    "sysctl -w net.ipv4.ip_forward=1",
    "ufw route allow in on {TUN} out on {INTERFACE}",
    "iptables -t nat -I POSTROUTING -o {INTERFACE} -j MASQUERADE",
]

pre_down = [
    "iptables -t nat -D POSTROUTING -o {INTERFACE} -j MASQUERADE",
    "ufw route delete allow in on {TUN} out on {INTERFACE}",
    "ip link delete {TUN}",
]
```

### 3. VPN Agent (Server-Side)

**Binary:** `x2ssh-agent` (statically compiled with musl for Linux)

**Simple TUN bridge - no complex logic:**

```rust
async fn run_agent(tun_name: &str) -> Result<()> {
    // Open server-side TUN interface (created by PostUp)
    let mut tun = tokio::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(format!("/dev/net/tun"))?;
    
    // Configure to use specific TUN device
    configure_tun(&mut tun, tun_name)?;
    
    let mut stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();
    let mut buf = vec![0u8; 2048];
    
    loop {
        tokio::select! {
            // Client → Server TUN: Read framed packet from stdin, write to TUN
            result = read_framed(&mut stdin) => {
                let packet = result?;
                tun.write_all(&packet).await?;
            }
            
            // Server TUN → Client: Read from TUN, write framed to stdout
            n = tun.read(&mut buf) => {
                let n = n?;
                write_framed(&mut stdout, &buf[..n]).await?;
            }
        }
    }
}

async fn read_framed(reader: &mut impl AsyncRead) -> Result<Vec<u8>> {
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;
    
    let mut packet = vec![0u8; len];
    reader.read_exact(&mut packet).await?;
    Ok(packet)
}

async fn write_framed(writer: &mut impl AsyncWrite, packet: &[u8]) -> Result<()> {
    let len = (packet.len() as u32).to_be_bytes();
    writer.write_all(&len).await?;
    writer.write_all(packet).await?;
    writer.flush().await?;
    Ok(())
}
```

**That's it!** Agent is ~100 lines of code. No NAT, no packet parsing, no state tracking.

**Agent privileges:**
- Needs permission to open `/dev/net/tun` and access the specific TUN device
- Usually runs via `sudo` in SSH exec command
- User controls this via their PostUp/PreDown scripts

### 4. Protocol

Extremely simple length-prefixed framing:

```
Wire format: [4-byte BE length][raw IP packet]
```

No serialization framework needed. Both client and agent implement the same trivial framing.

## Cleanup Strategy

### Best-Effort Self-Cleanup

Agent and client attempt cleanup on graceful shutdown:

```rust
// In client VPN session
async fn run_vpn_session(config: VpnConfig) -> Result<()> {
    // Setup
    create_client_tun().await?;
    setup_client_routing().await?;
    run_post_up_hooks(&config).await?;  // Fails if any PostUp fails
    
    // Register cleanup via scopeguard
    let _guard = scopeguard::guard((), |_| {
        tokio::task::block_in_place(|| {
            // Try to run PreDown hooks (ignore errors)
            for cmd in &config.pre_down {
                let _ = run_ssh_command(cmd);
            }
            
            // Clean up client side
            let _ = restore_client_routing();
            let _ = delete_client_tun();
        });
    });
    
    // Main VPN loop
    forward_packets().await?;
    
    Ok(())
}
```

**When cleanup runs:**
- ✅ Normal exit (Ctrl+C, user quits)
- ✅ Error/panic in x2ssh
- ⚠️ SIGKILL (process killed) - no cleanup possible

### Manual Cleanup Tool

For cases where cleanup doesn't run (power loss, kill -9, etc.):

```bash
x2ssh cleanup [OPTIONS] <USER@HOST>

Options:
  --config <FILE>    Config file (reads PreDown hooks)
  --dry-run          Show what would be cleaned up

Examples:
  # Run PreDown hooks from config
  x2ssh cleanup user@server.com
  
  # Use specific config
  x2ssh cleanup --config work.toml user@server.com
  
  # See what would be cleaned (don't execute)
  x2ssh cleanup --dry-run user@server.com
```

The cleanup command:
1. Reads config file
2. Connects via SSH
3. Runs all PreDown commands (ignoring errors)
4. Disconnects

## Project Structure

### Cargo Workspace

```
x2ssh/
├── Cargo.toml                    # Workspace root
├── x2ssh/                        # Main binary
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs
│       ├── lib.rs
│       ├── config.rs             # Config file parsing (TOML)
│       ├── socks.rs              # SOCKS5 mode
│       ├── transport.rs          # SSH transport
│       ├── retry.rs              # Retry policy
│       └── vpn.rs                # VPN module (declares submodules)
│       └── vpn/
│           ├── tun.rs            # Client TUN (Linux impl, Windows stubs)
│           ├── routing.rs        # Client routing (Linux impl, Windows stubs)
│           ├── framing.rs        # Length-prefixed framing
│           ├── session.rs        # VPN session management
│           ├── hooks.rs          # PostUp/PreDown execution
│           └── agent.rs          # Agent deployment
│
└── x2ssh-agent/                  # Server-side agent
    ├── Cargo.toml
    └── src/
        └── main.rs               # Simple TUN bridge (~100 lines)
│
├── tests/
│   ├── vpn_client.py             # VPN client wrapper
│   ├── tests/
│   │   └── test_vpn.py           # VPN integration tests
│   └── fixtures/
│       ├── Dockerfile.vpn-client        # Client container
│       ├── Dockerfile.vpn-server-target # Server + echo services
│       └── vpn-test-config.toml         # Test VPN config
```

## Implementation Phases

### Phase 1: Foundation & Agent (MVP)

**Goal:** Server-side agent + basic config parsing

**Tasks:**
- [x] Create workspace structure (`x2ssh`, `x2ssh-agent`)
- [ ] Add `directories` crate for config path
- [ ] Implement TOML config parsing (with CLI override)
- [ ] Implement simple TUN bridge agent
- [ ] Build script for musl static linking
- [ ] Agent deployment logic (raw bytes via `cat > /tmp/x2ssh-agent` over SSH exec stdin)
- [ ] Unit tests for config parsing

**Deliverables:** Config file working, agent compiles and can be deployed

**Deferred to Phase 6:**
- Variable substitution for hooks (use hardcoded values for MVP)

---

### Phase 2: Client TUN & Routing (Linux Only)

**Goal:** TUN device + routing on Linux client

**Tasks:**
- [ ] Add `tun-rs` dependency
- [ ] Implement Linux TUN creation (src/vpn/tun.rs)
- [ ] Add stub for Windows: `todo!("Windows TUN not yet implemented")`
- [ ] Add `rtnetlink` dependency
- [ ] Implement Linux routing configuration (src/vpn/routing.rs)
- [ ] Add stub for Windows: `todo!("Windows routing not yet implemented")`
- [ ] CLI integration (`--vpn` flag)
- [ ] Root privilege checking
- [ ] Integration test fixtures: Dockerfile.vpn-client, Dockerfile.vpn-server-target

**Deliverables:** Client TUN + routing working on Linux

---

### Phase 3: Integration & Server Hooks (MVP)

**Goal:** Complete VPN flow with PostUp/PreDown

**Tasks:**
- [ ] Implement SSH command execution for hooks (simple string execution)
- [ ] Implement PostUp execution (abort on failure)
- [ ] Implement PreDown execution (ignore failures)
- [ ] Implement agent deployment + startup
- [ ] Implement packet forwarding (TUN ↔ Agent ↔ Server TUN)
- [ ] Implement cleanup on disconnect
- [ ] Integration tests: TCP echo, UDP echo, ping (see Testing Strategy)
- [ ] Integration tests: PostUp/PreDown hooks, cleanup verification

**Deliverables:** Full working VPN on Linux (MVP - no variable substitution)

**Deferred to Phase 6:**
- Auto-detect server outbound interface
- Variable substitution in hooks

---

### Phase 4: Windows Support

**Goal:** Windows client support

**Tasks:**
- [ ] Replace `todo!()` stubs with real implementations
- [ ] Add Wintun driver dependency
- [ ] Implement Windows TUN creation (src/vpn/tun.rs)
- [ ] Implement Windows routing (src/vpn/routing.rs via `windows-sys`)
- [ ] Administrator privilege checking
- [ ] Wintun driver installation check (fail with clear message)
- [ ] Manual testing on Windows (automated tests deferred)

**Deliverables:** VPN working on Windows client

---

### Phase 5: Cleanup Command & Polish

**Goal:** Manual cleanup + production polish

**Tasks:**
- [ ] Implement `x2ssh cleanup` subcommand
- [ ] Error message improvements
- [ ] Logging and diagnostics
- [ ] Performance optimization (minimize copies)
- [ ] Documentation updates (README.md, examples/)

---

### Phase 6: Variable Substitution & Advanced Features

**Goal:** Flexible hook configuration

**Tasks:**
- [ ] Implement variable substitution for PostUp/PreDown hooks
- [ ] Auto-detect server outbound interface (`ip route get 8.8.8.8`)
- [ ] Support `{TUN}`, `{SUBNET}`, `{SERVER_IP}`, `{CLIENT_IP}`, `{INTERFACE}`
- [ ] Update config examples to use variables
- [ ] Unit tests for variable substitution
- [ ] Integration tests for different hook configurations
- [ ] Security audit

**Deliverables:** Production-ready VPN

## Dependencies

### Main Binary (`x2ssh`)

```toml
[dependencies]
# Existing
anyhow = "1.0"
tokio = { version = "1.45", features = ["full"] }
russh = "0.57"
tracing = "0.1"
clap = { version = "4.5", features = ["derive"] }

# Config
directories = "5.0"
serde = { version = "1.0", features = ["derive"] }
toml = "0.8"

# VPN
tun-rs = { version = "2.8", features = ["async"] }

[target.'cfg(target_os = "linux")'.dependencies]
rtnetlink = "0.20"

[target.'cfg(target_os = "windows")'.dependencies]
windows-sys = { version = "0.59", features = [
    "Win32_NetworkManagement_IpHelper",
    "Win32_Foundation",
] }
```

### Agent Binary (`x2ssh-agent`)

```toml
[dependencies]
tokio = { version = "1.45", features = ["rt", "io-std", "macros", "fs"] }
anyhow = "1.0"
libc = "0.2"  # For TUN ioctl

[profile.release]
strip = true
lto = true
codegen-units = 1
panic = "abort"
opt-level = "z"  # Optimize for size
```

## Security Considerations

### Server-Side

- [ ] PostUp/PreDown commands run as specified (user writes `sudo` if needed)
- [ ] Agent runs with permissions needed to access TUN (usually via `sudo`)
- [ ] PreDown commands always executed (even if some fail)
- [ ] Cleanup on crash (best-effort via scopeguard)

### Client-Side

- [ ] Requires root/Administrator (for TUN creation and routing)
- [ ] Routing restored on disconnect (prevents traffic leaks)
- [ ] SSH connection excluded from VPN routing (prevents lock-out)

### DNS Leak Prevention

- [ ] DNS queries go through VPN tunnel (automatic)
- [ ] Test with dnsleaktest.com

### Route Leak Prevention

- [ ] SSH server IP excluded from VPN routing
- [ ] User-specified exclusions work correctly
- [ ] Test: Kill VPN, verify traffic doesn't leak

## Performance Targets

| Metric | Target | Notes |
|--------|--------|-------|
| Throughput | >200 Mbps | Limited by SSH encryption + TUN overhead |
| Latency Overhead | <3ms | Just framing, no parsing |
| Connection Setup | <2s | SSH + hooks + agent deploy |
| Memory Usage | <10 MB | Client + agent combined (minimal buffering) |
| CPU Usage | <3% | At 100 Mbps (no parsing, kernel does work) |

## Future Enhancements

1. **IPv6 Support**
   - IPv6 routing configuration
   - Dual-stack support
   
2. **Split DNS**
   - `--vpn-dns` flag to override DNS server
   - Intercept DNS queries and redirect

3. **Connection Persistence**
   - Buffer packets during brief SSH reconnects
   - Seamless reconnection

4. **macOS Support**
   - TUN interface support (via tun-rs)
   - Routing configuration

5. **Config Profiles**
   - Multiple configs for different servers
   - `x2ssh --vpn --profile work user@server`

## Testing Strategy

### Unit Tests (Rust)

**MVP (Phase 1-5):**
- [ ] Config file parsing
- [ ] Hook command building (no substitution)
- [ ] Framing/deframing

**Later (Phase 6):**
- [ ] Variable substitution

### Integration Tests (Python)

**Test Architecture:**

All tests run inside Docker containers (no root required on host):

```
Docker Network: x2ssh-test-net (10.10.0.0/24)

┌─────────────────────────┐        ┌──────────────────────────────────┐
│  Container: client      │  SSH   │  Container: server-target        │
│  IP: 10.10.0.10         │◄─────►│  IP: 10.10.0.20                  │
│  (privileged)           │        │  (privileged)                    │
│                         │        │                                  │
│  - x2ssh --vpn          │        │  - sshd + x2ssh-agent            │
│  - TUN: 10.8.0.2/24     │        │  - TUN: 10.8.0.1/24              │
│  - Test tools           │        │  - iptables MASQUERADE           │
│                         │        │  - TCP echo (socat port 8080)    │
│                         │        │  - UDP echo (socat port 8081)    │
└─────────────────────────┘        │  - ping responder                │
                                   └──────────────────────────────────┘
```

**Test Files:**
```
tests/
├── conftest.py                      # Add VPN fixtures
├── vpn_client.py                    # VPN client wrapper (NEW)
├── tests/
│   └── test_vpn.py                  # VPN integration tests (NEW)
└── fixtures/
    ├── Dockerfile.vpn-client        # Client container (NEW)
    ├── Dockerfile.vpn-server-target # Server + echo services (NEW)
    └── vpn-test-config.toml         # Test VPN config (NEW)
```

**Test Scenarios (tests/tests/test_vpn.py):**

```python
# Basic connectivity
def test_vpn_tunnel_establishment(vpn_session):
    """Verify VPN tunnel is established."""
    # Check TUN interfaces exist (client + server)
    # Check routing table on client

def test_vpn_tcp_echo(vpn_session):
    """Test TCP traffic through VPN tunnel."""
    # From client: echo "test" | nc 10.10.0.20 8080
    # Verify echo response

def test_vpn_udp_echo(vpn_session):
    """Test UDP traffic through VPN tunnel."""
    # From client: echo "test" | nc -u 10.10.0.20 8081
    # Verify echo response

def test_vpn_ping(vpn_session):
    """Test ICMP traffic through VPN tunnel."""
    # From client: ping -c 4 10.10.0.20
    # Verify 0% packet loss

# Hooks & cleanup
def test_vpn_post_up_hooks_executed(vpn_session):
    """Verify PostUp hooks create TUN and iptables rules."""
    # Check TUN device exists on server
    # Check iptables rules present

def test_vpn_post_up_failure_aborts():
    """Test that failed PostUp prevents startup."""
    # Configure invalid PostUp command
    # Attempt VPN connection
    # Verify failure

def test_vpn_cleanup_on_disconnect(vpn_session):
    """Test PreDown hooks execute on disconnect."""
    # Start VPN, stop VPN (simulate Ctrl+C)
    # Verify TUN deleted, iptables cleaned

# Routing (basic verification)
def test_vpn_default_route_via_tun(vpn_session):
    """Verify default route points to TUN interface."""
    # Check routing table on client
    # Verify default via tun-x2ssh

def test_vpn_ssh_excluded_from_tunnel(vpn_session):
    """Verify SSH connection excluded from VPN routing."""
    # Check specific route for 10.10.0.20 bypasses TUN
```

**Helper Modules:**

`vpn_client.py` - VPN session management:
```python
class VpnClient:
    """Manages VPN client container and x2ssh process."""
    
    def start_vpn(self, server_ip: str, config_path: Path) -> None:
        """Start x2ssh --vpn inside client container."""
    
    def exec(self, cmd: str) -> tuple[int, str, str]:
        """Execute command inside client container."""
    
    def get_routing_table(self) -> list[str]:
        """Get routing table from client."""
    
    def stop_vpn(self) -> None:
        """Stop VPN and verify cleanup."""
```

**Pytest Fixtures (additions to conftest.py):**

```python
@pytest.fixture(scope="session")
def vpn_docker_network():
    """Create Docker network for VPN tests."""
    # Create network: x2ssh-test-net (10.10.0.0/24)

@pytest.fixture(scope="session")
def vpn_containers(vpn_docker_network):
    """Start VPN test containers."""
    # Start server-target container (10.10.0.20)
    # Start client container (10.10.0.10)
    # Build and copy x2ssh + x2ssh-agent binaries

@pytest.fixture
def vpn_session(vpn_containers):
    """Provide a running VPN session."""
    # Start x2ssh --vpn in client container
    # Yield VpnClient instance
    # Cleanup: stop VPN
```

**Run tests:**
```bash
# All tests (SOCKS5 + VPN)
uv run pytest

# VPN tests only
uv run pytest tests/tests/test_vpn.py
```

### Manual Tests

- [ ] Linux client → Linux server (real network)
- [ ] Windows client → Linux server
- [ ] DNS leak test (dnsleaktest.com)
- [ ] IP leak test (ipleak.net)
- [ ] Routing cleanup on Ctrl+C
- [ ] Multiple concurrent connections
- [ ] Large data transfer (sustained throughput)
- [ ] Manual cleanup command (`x2ssh cleanup`)

## References

- **WireGuard**: Configuration inspiration (PostUp/PreDown hooks)
- **tun-rs**: https://github.com/tun-rs/tun-rs
- **rtnetlink**: https://github.com/rust-netlink/rtnetlink
- **directories**: https://crates.io/crates/directories
- **DESIGN.md**: Current x2ssh architecture
