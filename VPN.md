# VPN Tunnel Design

This document describes the VPN tunnel feature for x2ssh.

## Overview

VPN mode provides system-level tunnel for all TCP and UDP traffic, routing the entire network stack through SSH. Inspired by WireGuard's configuration model with PostUp/PreDown hooks for maximum flexibility.

**Key Features:**
- Full tunnel (default route) with configurable exclusions
- WireGuard-style configuration with PostUp hooks
- User-configurable server-side setup (iptables, routing, forwarding)
- Cross-platform client (Linux + Windows)
- Linux server (requires root/sudo for iptables and IP forwarding)
- Automatic agent deployment and lifecycle management

## How It Works

The agent (`x2ssh-agent`) is deployed to the server and creates its own TUN interface via `tun-rs`. When the agent process exits (on disconnect or error), the OS automatically tears down the TUN interface — no cleanup commands needed.

**Lifecycle:**
1. x2ssh connects via SSH
2. Deploys agent binary to server (raw bytes via SSH exec: `cat > /tmp/x2ssh-agent`)
3. Starts agent via SSH exec
   - Agent creates TUN, assigns IP (e.g., 10.8.0.1/24), brings it up
4. Runs PostUp commands (IP forwarding, iptables NAT)
   - If ANY PostUp command fails, abort and kill agent
5. VPN forwarding begins
6. On disconnect or error:
   - x2ssh runs PreDown commands via SSH exec (one-by-one, errors ignored)
     - Cleans up iptables rules (while SSH connection still alive)
   - x2ssh closes agent SSH exec channel → agent exits
   - OS destroys TUN automatically

## Configuration

### Config File Location

**Note:** Platform-specific config directory auto-discovery is deferred to Phase 6. For MVP, config files must be specified explicitly via `--config <FILE>` flag.

When implemented (Phase 6), config will be loaded from:
- **Linux**: `~/.config/x2ssh/config.toml`
- **macOS**: `~/Library/Application Support/x2ssh/config.toml`
- **Windows**: `C:\Users\<user>\AppData\Roaming\x2ssh\config.toml`

### Example Config File

```toml
# ~/.config/x2ssh/config.toml

[vpn]
# VPN client address with prefix (client IP + subnet)
client_address = "10.8.0.2/24"

# VPN server address with prefix (server IP + subnet)
server_address = "10.8.0.1/24"

# Client-side TUN interface name
client_tun = "tun-x2ssh"

# MTU for TUN interface
mtu = 1400

# CIDRs to exclude from VPN routing
exclude = ["192.168.0.0/16", "172.16.0.0/12"]

# PostUp: Commands run on server AFTER agent is ready
# Used for iptables NAT and IP forwarding — NOT for TUN setup (agent handles that)
# MVP: Use hardcoded values (variable substitution in Phase 6)
post_up = [
    "sysctl -w net.ipv4.ip_forward=1",
    "iptables -t nat -I POSTROUTING -o eth0 -j MASQUERADE",
]

# PreDown: Commands run on server BEFORE agent stops
# Used to clean up iptables rules — NOT for TUN deletion (OS handles that when agent exits)
# Executed one-by-one even if some fail
pre_down = [
    "iptables -t nat -D POSTROUTING -o eth0 -j MASQUERADE",
]

[connection]
# SSH connection settings (can be overridden per-connection via CLI)
port = 22

[retry]
# Retry policy for SSH reconnection
max_attempts = "inf"  # Use "inf" or a positive number
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
| `{CLIENT_ADDRESS}` | Client address with prefix | `10.8.0.2/24` |
| `{CLIENT_IP}` | Client TUN IP address | `10.8.0.2` |
| `{SERVER_ADDRESS}` | Server address with prefix | `10.8.0.1/24` |
| `{SERVER_IP}` | Server TUN IP address | `10.8.0.1` |
| `{SUBNET}` | VPN subnet CIDR (derived from client_address) | `10.8.0.0/24` |
| `{INTERFACE}` | Server outbound interface | `eth0` (auto-detected or from config) |

**Auto-detection (Phase 6):**
- `{INTERFACE}` is auto-detected via `ip route get 8.8.8.8` if not specified
- Can override with `server_interface = "eth0"` in config

## CLI

```bash
x2ssh --vpn [OPTIONS] <USER@HOST>

VPN Options:
      --config <FILE>              Config file (MVP: must specify explicitly)
      --vpn                        Enable VPN mode (requires root/sudo on client)
      
  # Override config file settings:
      --vpn-client-address <ADDR>  Client IP with prefix, e.g. 10.8.0.2/24 [config: vpn.client_address]
      --vpn-server-address <ADDR>  Server IP with prefix, e.g. 10.8.0.1/24 [config: vpn.server_address]
      --vpn-client-tun <NAME>      Client TUN name [config: vpn.client_tun]
      --vpn-mtu <BYTES>            TUN MTU [config: vpn.mtu]
      --vpn-exclude <CIDR>         Exclude CIDR (can repeat) [config: vpn.exclude]
      --vpn-server-interface <IF>  Server outbound interface [Phase 6]
      
  # Override PostUp/PreDown entirely (all flags in a group replace config):
      --vpn-post-up <CMD>          PostUp command (can repeat)
      --vpn-pre-down <CMD>         PreDown command (can repeat)

Connection Options:
  -p, --port <PORT>                SSH port [default: 22]
  -i, --identity <FILE>            SSH private key

Examples:
  # Use config file defaults
  sudo x2ssh --vpn user@server.com

  # Override client and server addresses
  sudo x2ssh --vpn --vpn-client-address 10.9.0.2/24 --vpn-server-address 10.9.0.1/24 user@server.com

  # Use custom config
  sudo x2ssh --vpn --config /etc/x2ssh/work-vpn.toml user@server.com
  
  # Override PostUp/PreDown entirely
  sudo x2ssh --vpn \
    --vpn-post-up "sysctl -w net.ipv4.ip_forward=1" \
    --vpn-post-up "iptables -t nat -I POSTROUTING -o eth0 -j MASQUERADE" \
    --vpn-pre-down "iptables -t nat -D POSTROUTING -o eth0 -j MASQUERADE" \
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
│  │   Agent owns TUN lifecycle: creates on startup,   │      │
│  │   OS destroys on agent exit (no cleanup needed)   │      │
│  └────────────────────────────────────────────────────┘      │
│                                                              │
│  PostUp hooks enable IP forwarding and iptables NAT          │
│  PreDown hooks remove iptables rules before agent stops      │
└──────────────────────────────────────────────────────────────┘
```

### Design: Agent-Owned TUN

**Key insight:** The agent creates and owns the TUN interface. When the agent process exits (for any reason), the OS automatically destroys the TUN. This eliminates an entire class of cleanup problems.

**Why this works:**
- Agent creates TUN via `tun-rs` on startup, assigns subnet IP (e.g., 10.8.0.1)
- Client packets arrive with source IP in VPN subnet (e.g., 10.8.0.2)
- iptables MASQUERADE rewrites source IP when packets leave server TUN → Internet
- Responses come back, iptables rewrites destination IP → 10.8.0.2
- Kernel routes packets to server TUN interface
- Agent reads from server TUN, sends to client via stdout
- **No manual TUN creation or deletion in PostUp/PreDown — the agent handles it all!**

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

PostUp/PreDown hooks are for **network configuration only** — iptables, IP forwarding, firewall rules. TUN creation and deletion are handled automatically by the agent.

**Lifecycle:**

```
1. x2ssh connects via SSH
2. Deploys agent binary (raw bytes via SSH exec: `cat > /tmp/x2ssh-agent`)
3. Starts agent via SSH exec
   - Agent creates TUN, assigns IP (e.g., 10.8.0.1/24), brings it up
4. Runs PostUp commands (IP forwarding, iptables NAT)
   - If ANY PostUp command fails, abort and kill agent
5. VPN forwarding begins
...
(On disconnect or error)
6. x2ssh runs PreDown commands via SSH exec (one-by-one, errors ignored)
    - Cleans up iptables rules (while SSH connection still alive)
7. x2ssh closes agent SSH exec channel → agent exits
8. OS destroys TUN automatically
9. Cleanup complete
```

**Example PostUp (iptables) - MVP:**

```toml
# MVP: Hardcoded values (adjust eth0 to match your server's interface)
# Note: No TUN commands here — the agent handles TUN creation automatically
post_up = [
    "sysctl -w net.ipv4.ip_forward=1",
    "iptables -t nat -I POSTROUTING -o eth0 -j MASQUERADE",
]

pre_down = [
    "iptables -t nat -D POSTROUTING -o eth0 -j MASQUERADE",
]
```

**Example PostUp (nftables) - Phase 6 with variables:**

```toml
# Phase 6: With variable substitution
post_up = [
    "sysctl -w net.ipv4.ip_forward=1",
    "nft add table inet x2ssh",
    "nft add chain inet x2ssh postrouting { type nat hook postrouting priority 100 \\; }",
    "nft add rule inet x2ssh postrouting ip saddr {SUBNET} oif {INTERFACE} masquerade",
]

pre_down = [
    "nft delete table inet x2ssh",
]
```

**Example PostUp (with ufw) - Phase 6 with variables:**

```toml
# Phase 6: With variable substitution
post_up = [
    "sysctl -w net.ipv4.ip_forward=1",
    "ufw route allow in on tun0 out on {INTERFACE}",
    "iptables -t nat -I POSTROUTING -o {INTERFACE} -j MASQUERADE",
]

pre_down = [
    "iptables -t nat -D POSTROUTING -o {INTERFACE} -j MASQUERADE",
    "ufw route delete allow in on tun0 out on {INTERFACE}",
]
```

### 3. VPN Agent (Server-Side)

**Binary:** `x2ssh-agent` (statically compiled with musl for Linux)

**Creates and owns the TUN interface — no external setup needed:**

```rust
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Agent receives subnet IP from x2ssh (e.g., "10.8.0.1/24")
    let subnet_ip = std::env::args().nth(2).expect("Usage: x2ssh-agent --ip <SUBNET_IP>");

    // Create and configure TUN device (agent owns this — dies with the process)
    let tun = tun_rs::DeviceBuilder::new()
        .address_with_prefix(subnet_ip.parse()?)
        .up()
        .build_async()?;
    let tun = Arc::new(tun);

    let tun_for_write = Arc::clone(&tun);
    let mut stdin = tokio::io::stdin();

    // Client → Server TUN: Read framed packet from stdin, send to TUN
    let client_to_tun = tokio::spawn(async move {
        loop {
            let packet = proto::read_framed(&mut stdin).await?;
            tun_for_write.send(&packet).await?;
        }
    });

    let tun_for_read = Arc::clone(&tun);
    let mut stdout = tokio::io::stdout();

    // Server TUN → Client: Receive from TUN, write framed to stdout
    let tun_to_client = tokio::spawn(async move {
        let mut buf = vec![0u8; 2048];
        loop {
            let n = tun_for_read.recv(&mut buf).await?;
            proto::write_framed(&mut stdout, &buf[..n]).await?;
        }
    });

    tokio::select! {
        _ = client_to_tun => {},
        _ = tun_to_client => {},
    }

    Ok(())
    // TUN destroyed automatically when process exits
}
```

**Agent privileges:**
- Needs permission to create TUN (`/dev/net/tun`) — usually via `sudo` in SSH exec command
- User controls this via their SSH/sudo configuration

### 4. Protocol

Extremely simple length-prefixed framing:

```
Wire format: [4-byte BE length][raw IP packet]
```

No serialization framework needed. Both client and agent implement the same trivial framing.

## Cleanup Strategy

### Automatic Cleanup

The agent-owned TUN approach means most cleanup is automatic:

- **Agent exits** (normal or crash) → OS destroys TUN immediately
- **SSH connection drops** → agent's stdin/stdout close → agent exits → TUN destroyed
- **x2ssh killed** → SSH connection closes → agent exits → TUN destroyed

**The only thing requiring explicit cleanup** is the iptables/firewall rules configured in PostUp. These are handled by PreDown commands.

### Explicit Cleanup (PreDown Hooks)

PreDown runs before the agent exits, cleaning up iptables rules while the SSH connection is still alive:

```rust
// In client VPN session
async fn run_vpn_session(config: VpnConfig) -> Result<()> {
    // Deploy and start agent (agent creates TUN)
    deploy_and_start_agent(&transport, &config).await?;

    // Run PostUp (iptables/forwarding setup)
    run_post_up_hooks(&config).await?;  // Fails if any PostUp fails

    let result = async {
        // Main VPN loop
        forward_packets().await
    }.await;

    // Run PreDown before stopping agent (SSH connection still alive)
    for cmd in &config.pre_down {
        let _ = run_ssh_command(cmd);
    }

    // Now stop agent
    stop_agent().await;
    // Agent exits → OS destroys TUN automatically

    result
}
```

**When cleanup runs:**
- ✅ Normal exit (Ctrl+C, user quits)
- ✅ Error/panic in x2ssh
- ⚠️ SIGKILL (process killed) — no cleanup possible; iptables rules remain

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

Note: Since TUN is automatically destroyed when the agent exits, the only thing to clean up is iptables rules.

## Project Structure

### Cargo Workspace

```
x2ssh/
├── Cargo.toml                    # Workspace root
├── proto/                        # Shared protocol code
│   ├── Cargo.toml
│   └── src/
│       └── framing.rs            # Length-prefixed packet framing
│
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
│           ├── agent.rs          # Agent deployment
│           ├── tun.rs            # Client TUN (Linux impl, Windows stubs)
│           ├── routing.rs        # Client routing (Linux impl, Windows stubs)
│           └── session.rs        # VPN session management + explicit cleanup
│
└── x2ssh-agent/                  # Server-side agent
    ├── Cargo.toml
    └── src/
        └── main.rs               # TUN bridge (~100 lines); creates and owns TUN
```

## Implementation Phases

### Phase 1: Foundation & Agent (MVP)

**Goal:** Server-side agent + basic config parsing

**Tasks:**
- [x] Create workspace structure (`x2ssh`, `x2ssh-agent`)
- [x] ~~Add `directories` crate for config path~~ (deferred to Phase 6, `--config` only for MVP)
- [x] Implement TOML config parsing (with CLI override)
- [x] Implement simple TUN bridge agent (using `tun-rs`) — agent creates its own TUN
- [x] Build script for agent embedding (via `build.rs`)
- [x] Agent deployment stub (full SSH exec implementation in Phase 3)
- [x] Unit tests for config parsing

**Deliverables:** Config file working, agent compiles and can be deployed

**Deferred to Phase 6:**
- Variable substitution for hooks (use hardcoded values for MVP)

---

### Phase 2: Client TUN & Routing (Linux Only)

**Goal:** TUN device + routing on Linux client

**Tasks:**
- [x] Add `tun-rs` dependency
- [x] Implement Linux TUN creation (src/vpn/tun.rs)
- [x] Add stub for Windows: `todo!("Windows TUN not yet implemented")`
- [x] Add `rtnetlink` dependency
- [x] Implement Linux routing configuration (src/vpn/routing.rs)
- [x] Add stub for Windows: `todo!("Windows routing not yet implemented")`
- [x] CLI integration (`--vpn` flag)
- [x] Root privilege checking
- [x] Integration test fixtures: Dockerfile.vpn-client, Dockerfile.vpn-server-target

**Deliverables:** Client TUN + routing working on Linux

---

### Phase 3: Integration & Server Hooks (MVP)

**Goal:** Complete VPN flow with PostUp/PreDown

**Tasks:**
- [ ] Implement SSH command execution for hooks (simple string execution)
- [ ] Implement PostUp execution (abort on failure)
- [ ] Implement PreDown execution (ignore failures)
- [ ] Implement agent deployment + startup (agent creates TUN autonomously)
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
- [ ] Support `{SUBNET}`, `{SERVER_IP}`, `{CLIENT_IP}`, `{INTERFACE}`
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
serde = { version = "1.0", features = ["derive"] }
toml = "0.8"
ipnet = "2.11"

# VPN
tun-rs = { version = "2.8", features = ["async"] }

[target.'cfg(target_os = "linux")'.dependencies]
libc = "0.2"
rtnetlink = "0.17"

[target.'cfg(target_os = "windows")'.dependencies]
windows-sys = { version = "0.59", features = [
    "Win32_NetworkManagement_IpHelper",
    "Win32_Foundation",
] }
```

### Agent Binary (`x2ssh-agent`)

```toml
[dependencies]
tokio = { version = "1.45", features = ["rt-multi-thread", "io-std", "macros"] }
anyhow = "1.0"
tun-rs = { version = "2.8", features = ["async"] }  # Async TUN device
proto = { path = "../proto" }  # Shared framing code

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
- [ ] Agent runs with permissions needed to create TUN (usually via `sudo`)
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
- [x] Config file parsing
- [x] Hook command building (no substitution)
- [x] Framing/deframing

**Later (Phase 6):**
- [ ] Variable substitution

### Integration Tests (Python)

**Test Architecture:**

All tests run inside Docker containers (no root required on host):

```
Docker Network: x2ssh-vpn-test-net (10.10.0.0/24)

┌─────────────────────────┐        ┌──────────────────────────────────┐
│  Container: vpn-client  │  SSH   │  Container: vpn-server-target    │
│  IP: 10.10.0.10         │◄─────►│  IP: 10.10.0.20                  │
│  (privileged)           │        │  (privileged)                    │
│                         │        │                                  │
│  - x2ssh binary (mount) │        │  - sshd                          │
│  - TUN: 10.8.0.2/24     │        │  - x2ssh-agent (deployed at runtime)│
│  - Test tools           │        │  - TUN: 10.8.0.1/24 (agent-owned)│
│                         │        │  - iptables MASQUERADE           │
│                         │        │  - TCP echo (socat port 8080)    │
│                         │        │  - UDP echo (socat port 8081)    │
└─────────────────────────┘        └──────────────────────────────────┘
```

**Test Files:**
```
tests/
├── conftest.py                      # VPN fixtures (vpn_env, vpn_session)
├── vpn_client.py                    # VpnTestEnv, VpnSession classes
├── tests/
│   └── test_vpn.py                  # VPN integration tests
└── fixtures/
    ├── Dockerfile.vpn-client        # Client container
    ├── Dockerfile.vpn-server-target # Server + echo services
    └── vpn-test-config.toml         # Test VPN config
```

**Test Scenarios (tests/tests/test_vpn.py):**

```python
# Phase 2 Tests: Container Setup (enabled)
def test_vpn_client_container_has_required_tools(vpn_env):
    """Verify client container has iproute2, iptables, nc, ping, ssh."""
    
def test_vpn_server_container_has_sshd(vpn_env):
    """Verify server container has sshd running."""

def test_vpn_server_tcp_echo_service(vpn_env):
    """Verify TCP echo service responds on server port 8080."""

def test_vpn_client_can_ssh_to_server(vpn_env):
    """Verify client can SSH to server."""

def test_vpn_client_has_tun_device_access(vpn_env):
    """Verify client container can access /dev/net/tun."""

def test_vpn_server_has_tun_device_access(vpn_env):
    """Verify server container can access /dev/net/tun."""

def test_vpn_x2ssh_binary_exists(vpn_env):
    """Verify x2ssh binary is mounted in client container."""

# Phase 3 Tests: VPN Tunnel (disabled until Phase 3)
@pytest.mark.skip(reason="Phase 3 - requires VPN tunnel implementation")
def test_vpn_tunnel_establishment(vpn_session):
    """Verify VPN tunnel is established."""

@pytest.mark.skip(reason="Phase 3 - requires VPN tunnel implementation")
def test_vpn_tcp_through_tunnel(vpn_session):
    """Test TCP traffic through VPN tunnel."""

@pytest.mark.skip(reason="Phase 3 - requires VPN tunnel implementation")
def test_vpn_udp_through_tunnel(vpn_session):
    """Test UDP traffic through VPN tunnel."""

@pytest.mark.skip(reason="Phase 3 - requires VPN tunnel implementation")
def test_vpn_ping_through_tunnel(vpn_session):
    """Test ICMP traffic through VPN tunnel."""

@pytest.mark.skip(reason="Phase 3 - requires PostUp/PreDown implementation")
def test_vpn_post_up_hooks_executed(vpn_session):
    """Verify PostUp hooks set up iptables rules."""

@pytest.mark.skip(reason="Phase 3 - requires PostUp/PreDown implementation")
def test_vpn_pre_down_cleanup(vpn_session):
    """Test PreDown hooks execute on disconnect."""

@pytest.mark.skip(reason="Phase 3 - requires routing implementation")
def test_vpn_default_route_via_tun(vpn_session):
    """Verify default route points to TUN interface."""
```

**Helper Modules:**

`vpn_client.py` - VPN test environment and session management:
```python
class VpnTestEnv:
    """Manages Docker containers for VPN integration tests."""
    
    def start(self) -> None:
        """Create network and start containers."""
    
    def stop(self) -> None:
        """Stop and remove all containers and network."""
    
    def exec_client(self, cmd: str) -> tuple[int, str]:
        """Execute command in client container."""
    
    def exec_server(self, cmd: str) -> tuple[int, str]:
        """Execute command in server container."""


class VpnSession:
    """Manages a VPN session for testing."""
    
    def start_vpn(self) -> None:
        """Start x2ssh --vpn in client container."""
    
    def stop_vpn(self) -> None:
        """Stop x2ssh process in client container."""
    
    def is_vpn_running(self) -> bool:
        """Check if x2ssh process is running."""
```

**Pytest Fixtures (in conftest.py):**

```python
@pytest.fixture(scope="session")
def vpn_env(project_root: Path) -> Iterator[VpnTestEnv]:
    """Provide a running VPN test environment (containers + network)."""

@pytest.fixture
def vpn_session(vpn_env: VpnTestEnv) -> Iterator[VpnSession]:
    """Provide a running VPN session."""
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
