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
# Available variables: {TUN}, {SUBNET}, {SERVER_IP}, {CLIENT_IP}, {INTERFACE}
post_up = [
    "ip tuntap add mode tun name {TUN}",
    "ip addr add {SERVER_IP}/24 dev {TUN}",
    "ip link set {TUN} up",
    "sysctl -w net.ipv4.ip_forward=1",
    "iptables -t nat -I POSTROUTING -o {INTERFACE} -j MASQUERADE",
]

# PreDown: Commands run on server BEFORE TUN is destroyed (after agent stops)
# Executed one-by-one even if some fail
pre_down = [
    "iptables -t nat -D POSTROUTING -o {INTERFACE} -j MASQUERADE",
    "ip link delete {TUN}",
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

### Variable Substitution

Available variables in `post_up` and `pre_down` commands:

| Variable | Description | Example Value |
|----------|-------------|---------------|
| `{TUN}` | Server TUN interface name | `x2ssh0` |
| `{SUBNET}` | VPN subnet CIDR | `10.8.0.0/24` |
| `{SERVER_IP}` | Server TUN IP address | `10.8.0.1` |
| `{CLIENT_IP}` | Client TUN IP address | `10.8.0.2` |
| `{INTERFACE}` | Server outbound interface | `eth0` (auto-detected or from config) |

**Auto-detection:**
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
│  │         VPN Agent (x2ssh-vpn)                      │      │
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
2. Detects server outbound interface (ip route get 8.8.8.8)
3. Runs PostUp commands with variable substitution
   - PostUp creates TUN, sets up iptables
   - If ANY PostUp command fails, abort startup
4. Deploys agent binary (base64 over SSH)
5. Starts agent via SSH exec
6. VPN forwarding begins
...
(On disconnect or error)
7. Stops agent
8. Runs PreDown commands (one-by-one, errors ignored)
9. Cleanup complete
```

**Example PostUp (iptables):**

```toml
post_up = [
    "ip tuntap add mode tun name {TUN}",
    "ip addr add {SERVER_IP}/24 dev {TUN}",
    "ip link set {TUN} up",
    "sysctl -w net.ipv4.ip_forward=1",
    "iptables -t nat -I POSTROUTING -o {INTERFACE} -j MASQUERADE",
]

pre_down = [
    "iptables -t nat -D POSTROUTING -o {INTERFACE} -j MASQUERADE",
    "ip link delete {TUN}",
]
```

**Example PostUp (nftables):**

```toml
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

**Example PostUp (with ufw):**

```toml
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

**Binary:** `x2ssh-vpn` (statically compiled with musl for Linux)

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
├── crates/
│   ├── x2ssh/                    # Main binary
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── main.rs
│   │       ├── lib.rs
│   │       ├── config.rs         # Config file parsing (TOML)
│   │       ├── socks.rs          # SOCKS5 mode
│   │       ├── transport.rs      # SSH transport
│   │       ├── retry.rs          # Retry policy
│   │       └── vpn.rs            # VPN module (declares submodules)
│   │       └── vpn/
│   │           ├── tun.rs        # Client TUN (Linux/Windows)
│   │           ├── routing.rs    # Client routing (Linux/Windows)
│   │           ├── framing.rs    # Length-prefixed framing
│   │           ├── session.rs    # VPN session management
│   │           ├── hooks.rs      # PostUp/PreDown execution
│   │           └── agent.rs      # Agent deployment
│   │
│   └── x2ssh-vpn/                # Server-side agent
│       ├── Cargo.toml
│       └── src/
│           └── main.rs           # Simple TUN bridge (~100 lines)
│
├── tests-e2e/
│   └── tests/
│       └── test_vpn.py           # VPN E2E tests
```

## Implementation Phases

### Phase 1: Foundation & Agent

**Goal:** Server-side agent + config parsing

**Tasks:**
- [ ] Create workspace structure
- [ ] Add `directories` crate for config path
- [ ] Implement TOML config parsing (with CLI override)
- [ ] Implement variable substitution for hooks
- [ ] Implement simple TUN bridge agent
- [ ] Build script for musl static linking
- [ ] Agent deployment logic (base64 upload)
- [ ] Unit tests for config parsing and variable substitution

**Deliverables:** Config file working, agent compiles and can be deployed

---

### Phase 2: Client TUN & Routing (Linux Only)

**Goal:** TUN device + routing on Linux client

**Tasks:**
- [ ] Add `tun-rs` dependency
- [ ] Implement Linux TUN creation
- [ ] Add `rtnetlink` dependency
- [ ] Implement Linux routing configuration (default route + exclusions)
- [ ] CLI integration (`--vpn` flag)
- [ ] Root privilege checking
- [ ] E2E test: TUN creation + routing

**Deliverables:** Client TUN + routing working on Linux

---

### Phase 3: Integration & Server Hooks

**Goal:** Complete VPN flow with PostUp/PreDown

**Tasks:**
- [ ] Implement SSH command execution for hooks
- [ ] Implement PostUp execution (abort on failure)
- [ ] Implement PreDown execution (ignore failures)
- [ ] Auto-detect server outbound interface
- [ ] Implement agent deployment + startup
- [ ] Implement packet forwarding (TUN ↔ Agent ↔ Server TUN)
- [ ] Implement cleanup on disconnect
- [ ] E2E tests: HTTP (TCP), DNS (UDP), ping (ICMP)

**Deliverables:** Full working VPN on Linux

---

### Phase 4: Windows Support

**Goal:** Windows client support

**Tasks:**
- [ ] Add Wintun driver dependency
- [ ] Implement Windows TUN creation
- [ ] Implement Windows routing (via `windows-sys`)
- [ ] Administrator privilege checking
- [ ] Wintun driver installation check (fail with clear message)
- [ ] E2E tests on Windows

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

# Config
directories = "5.0"
serde = { version = "1.0", features = ["derive"] }
toml = "0.8"

# VPN
tun-rs = { version = "2.8", features = ["async"] }
base64 = "0.22"

[target.'cfg(target_os = "linux")'.dependencies]
rtnetlink = "0.20"

[target.'cfg(target_os = "windows")'.dependencies]
windows-sys = { version = "0.59", features = [
    "Win32_NetworkManagement_IpHelper",
    "Win32_Foundation",
] }
```

### Agent Binary (`crates/x2ssh-vpn`)

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

### Unit Tests

- [ ] Config file parsing
- [ ] Variable substitution
- [ ] Hook command building
- [ ] Framing/deframing

### E2E Tests

- [ ] VPN connection establishment
- [ ] TCP traffic (HTTP request)
- [ ] UDP traffic (DNS query)
- [ ] ICMP traffic (ping)
- [ ] PostUp hook failure aborts startup
- [ ] PreDown hooks run on disconnect
- [ ] Route exclusions work
- [ ] Manual cleanup command

### Manual Tests

- [ ] Linux client → Linux server
- [ ] Windows client → Linux server
- [ ] DNS leak test
- [ ] IP leak test
- [ ] Routing cleanup on Ctrl+C
- [ ] Multiple concurrent connections

## References

- **WireGuard**: Configuration inspiration (PostUp/PreDown hooks)
- **tun-rs**: https://github.com/tun-rs/tun-rs
- **rtnetlink**: https://github.com/rust-netlink/rtnetlink
- **directories**: https://crates.io/crates/directories
- **DESIGN.md**: Current x2ssh architecture
