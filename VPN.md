# VPN Tunnel Design (Future)

This document describes the planned VPN tunnel feature for x2ssh.

## Overview

VPN mode provides system-level tunnel for all TCP and UDP traffic, routing the entire network stack through SSH.

## CLI

```
x2ssh --vpn <USER@HOST>

VPN Options:
      --vpn-dns <ADDR>      DNS server to use in VPN mode [default: remote]
      --vpn-exclude <CIDR>  Exclude CIDR from VPN tunnel (can repeat)

Examples:
  x2ssh --vpn user@server.com                    # Full VPN tunnel
  x2ssh --vpn --vpn-exclude 10.0.0.0/8 user@host # VPN with exclusions
```

## Architecture

VPN mode requires a server-side agent for UDP support:

1. **First Connection**:
   - Connect via SSH
   - Detect server OS
   - Upload appropriate agent binary
   - Start agent via SSH command

2. **Data Flow**:
   - TUN interface captures all traffic
   - TCP: Forward via SSH `direct-tcpip` channels
   - UDP: Forward via custom protocol to server agent
   - Server agent forwards UDP to actual destinations

3. **Agent Lifecycle**:
   - Auto-started when VPN mode initiates
   - Auto-stopped on disconnect
   - Cleanup on unexpected exit

## Server-Side Agent

The agent is a minimal binary that:

1. Listens on a Unix socket / named pipe for commands from x2ssh
2. Receives UDP packets over the SSH channel
3. Forwards to target destinations
4. Returns responses back through SSH channel

**Deployment Strategy**:
- Binary embedded in x2ssh at compile time
- Extracted to `/tmp/.x2ssh-agent-<random>` on Unix
- Extracted to `%TEMP%\.x2ssh-agent-<random>` on Windows
- Deleted on clean shutdown

## Cross-Platform Considerations

| Aspect | Linux | Windows |
|--------|-------|---------|
| TUN Device | `/dev/net/tun` | Wintun driver |
| Agent Transport | Unix socket | Named pipe |
| Privileges | root/CAP_NET_ADMIN | Administrator |
| Agent Binary | ELF | PE/COFF |

**Windows VPN Requirements**:
- Wintun driver must be installed (bundled or auto-installed)
- Run as Administrator
- Use `wintun.dll` via FFI

## Reliability

**Graceful Reconnection**:
- Re-establish VPN routes automatically
- Re-deploy server agent if needed

## Implementation Phases

### Phase 3: VPN Tunnel (Linux)
- [ ] TUN interface setup
- [ ] TCP forwarding through SSH
- [ ] Server agent for UDP forwarding
- [ ] Auto-deployment mechanism

### Phase 4: VPN Tunnel (Windows)
- [ ] Wintun integration
- [ ] Named pipe agent transport
- [ ] Windows-specific privilege handling

## Dependencies

| Crate | Purpose |
|-------|---------|
| `tun` (Linux) | TUN interface |
| `wintun` (Windows) | Windows TUN |

## Security Considerations

1. **Agent Isolation**: Server agent runs with user privileges, accepts connections only from x2ssh
