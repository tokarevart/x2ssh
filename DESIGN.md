# x2ssh Design Document

## What

x2ssh is a CLI tool that provides SOCKS5 proxy functionality using SSH as the transport protocol. It enables users to route network traffic through an SSH server without requiring any manual server-side setup beyond a standard SSH installation.

### Core Features

- **SOCKS5 Proxy**: Application-level proxy for SOCKS5-compatible applications

### Non-Features

- No raw port forwarding (`-L`, `-R`) - use standard SSH for that
- No shell/terminal access - use standard SSH for that
- No SSH server functionality - client only
- No VPN tunnel - see [VPN.md](./VPN.md) for future plans

## Why

### Problems with Standard SSH

1. **Unreliable retry logic**: SSH's built-in reconnection is janky and inflexible

### x2ssh Solutions

1. **Robust retry policies**: Configurable backoff, max attempts, health checks
2. **Zero server setup**: Works with any standard SSH server
3. **Cross-platform**: Linux and Windows support

## How

### Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                         CLIENT                               │
│  ┌─────────────┐    ┌─────────────┐    ┌─────────────────┐  │
│  │   CLI App   │───▶│  Transport  │───▶│  SSH Connection │──┼──▶ SSH Server
│  │  (x2ssh)    │    │   Layer     │    │   (russh)       │  │
│  └──────┬──────┘    └─────────────┘    └─────────────────┘  │
│         │                                                   │
│  ┌──────▼──────┐                                            │
│  │  SOCKS5     │                                            │
│  │  Server     │                                            │
│  └─────────────┘                                            │
└─────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────┐
│                         SERVER                               │
│  ┌─────────────┐                                            │
│  │   SSHD      │  (existing SSH server, no setup required)  │
│  │  (existing) │                                            │
│  └─────────────┘                                            │
└─────────────────────────────────────────────────────────────┘
```

### CLI Design

```
x2ssh [OPTIONS] <USER@HOST>

Modes:
  -D, --socks <ADDR>    Start SOCKS5 proxy on specified address (e.g., 127.0.0.1:1080)

Connection:
  -p, --port <PORT>     SSH port [default: 22]
  -i, --identity <FILE> Identity file (private key)

Retry Policy:
      --retry-max <N>       Maximum retry attempts [default: infinite]
      --retry-delay <MS>    Initial retry delay in ms [default: 1000]
      --retry-backoff <N>   Backoff multiplier [default: 2]
      --retry-max-delay <MS> Maximum retry delay [default: 30000]
      --health-interval <MS> Connection health check interval [default: 5000]

Examples:
  x2ssh -D 127.0.0.1:1080 user@server.com        # SOCKS5 proxy
  x2ssh -D 1080 user@server.com                  # SOCKS5 proxy (shorthand)
```

### Transport Layer

The transport layer abstracts the SSH connection and provides:

1. **Connection Pooling**: Reuse SSH sessions for multiple channels
2. **Health Monitoring**: Periodic keepalive checks
3. **Auto-Reconnection**: Transparent reconnection on failure
4. **Channel Management**: Multiplex SOCKS5 connections over single SSH session

### SOCKS5 Mode

- Uses SSH's built-in `direct-tcpip` channel type
- No server-side component needed
- Each SOCKS5 connection opens a new SSH channel

```
App → SOCKS5 → x2ssh → SSH channel → Server → Target
```

### Reliability Features

**Retry Policy**:
```
delay = min(initial_delay * backoff^attempt, max_delay)
```

**Health Checks**:
- Send SSH keepalive every `--health-interval`
- If no response within 3x interval, trigger reconnection
- Notify user of connection state changes

**Graceful Reconnection**:
- Preserve SOCKS5 connections during brief reconnects (buffering)

### Performance Considerations

1. **Async I/O**: Tokio runtime for all operations
2. **Zero-Copy**: Avoid unnecessary buffer copies
3. **Connection Multiplexing**: Single SSH session for all channels
4. **Efficient Polling**: Use epoll/kqueue/IOCP based on platform

## Implementation Phases

### Phase 1: Core Infrastructure
- [ ] CLI argument parsing with clap
- [ ] SSH connection management (russh)
- [ ] Retry policy implementation
- [ ] Health monitoring

### Phase 2: SOCKS5 Proxy
- [ ] SOCKS5 server implementation
- [ ] SSH channel forwarding
- [ ] DNS resolution handling

### Phase 3: Polish
- [ ] Configuration file support
- [ ] Logging and diagnostics
- [ ] Performance optimization
- [ ] Documentation

## Dependencies

| Crate | Purpose |
|-------|---------|
| `russh` | SSH client implementation |
| `tokio` | Async runtime |
| `clap` | CLI argument parsing |
| `fast-socks5` | SOCKS5 protocol |
| `tracing` | Logging |

## Security Considerations

1. **Key Management**: Support SSH agent, key files, encrypted keys
2. **Server Verification**: Strict host key checking (with option to disable)
3. **No Secrets in Logs**: Sanitize sensitive data from log output

## Testing Strategy

### Approach

- **Unit tests**: Built-in `#[test]` for sync pure logic (retry calculations, CLI parsing)
- **E2E tests**: `#[tokio::test]` for async network tests with Docker SSH containers
- **No mocks**: Keep main code simple, test with real components

### Test Structure

```
tests/
├── fixtures/
│   ├── Dockerfile           # SSH server image
│   └── keys/                # Pre-generated test keys
├── common/
│   └── mod.rs               # Container fixture, SSH client helpers
├── e2e_socks5.rs            # SOCKS5 connect/transfer tests
├── e2e_reconnect.rs         # Connection drop/reconnect tests
└── e2e_retry.rs             # Retry policy behavior tests

src/
├── retry.rs                 # Unit testable retry logic
└── cli.rs                   # Unit testable CLI parsing
```

### Docker Fixture

- Per-test container (isolated, parallel-safe)
- Pre-baked SSH keys for deterministic auth
- Random host port mapping to avoid conflicts
- Auto-cleanup on test completion

### Test Cases

**Unit Tests:**
- `retry::test_backoff_calculation`
- `retry::test_max_delay_cap`
- `retry::test_max_attempts`
- `cli::test_argument_parsing`

**E2E Tests:**
- `socks5_handshake_success`
- `socks5_connect_tcp_forward`
- `socks5_multiple_concurrent_connections`
- `reconnect_on_ssh_drop`
- `retry_exponential_backoff`
