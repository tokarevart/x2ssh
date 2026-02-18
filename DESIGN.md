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
│                         CLIENT                              │
│  ┌─────────────┐    ┌─────────────┐    ┌─────────────────┐  │
│  │   CLI App   │───▶│  Transport  │──▶│  SSH Connection │──┼──▶ SSH Server
│  │  (x2ssh)    │    │   Layer     │    │   (russh)       │  │
│  └──────┬──────┘    └─────────────┘    └─────────────────┘  │
│         │                                                   │
│  ┌──────▼──────┐                                            │
│  │  SOCKS5     │                                            │
│  │  Server     │                                            │
│  └─────────────┘                                            │
└─────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────┐
│                         SERVER                              │
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
- [x] CLI argument parsing with clap
- [x] SSH connection management (russh)
- [x] Retry policy implementation
- [x] Health monitoring

### Phase 2: SOCKS5 Proxy
- [x] SOCKS5 server implementation
- [x] SSH channel forwarding
- [x] DNS resolution handling

### Phase 3: Polish
- [ ] Configuration file support
- [ ] Logging and diagnostics
- [ ] Performance optimization
- [ ] Documentation

## Dependencies

**Rust:**
| Crate | Purpose |
|-------|---------|
| `russh` | SSH client implementation |
| `tokio` | Async runtime with multi-threaded executor |
| `clap` | CLI argument parsing with derive macros |
| `fast-socks5` | SOCKS5 protocol implementation |
| `tracing` | Structured logging |
| `tracing-subscriber` | Logging output formatting |
| `anyhow` | Error handling |

**Python (E2E tests):**
| Package | Purpose |
|---------|---------|
| `pytest` | Test framework |
| `pytest-asyncio` | Async test support |
| `testcontainers` | Docker container management |
| `pysocks` | SOCKS5 client for testing |
| `ty` | Fast Rust-based type checker |
| `ruff` | Fast Python linter and formatter |

## Security Considerations

1. **Key Management**: Support SSH agent, key files, encrypted keys
2. **Server Verification**: Strict host key checking (with option to disable)
3. **No Secrets in Logs**: Sanitize sensitive data from log output

## Testing Strategy

### Approach

Tests are split into two separate projects:

- **Rust Unit Tests**: Fast, in-process tests for pure logic (retry calculations, CLI parsing, transport internals)
- **Python E2E Tests**: Full black-box tests using the compiled binary with Docker SSH containers

This separation:
- Keeps Rust code clean (no testcontainers dependency)
- Enables faster Rust builds
- Tests the actual binary behavior, not library internals
- Leverages Python's rich testing ecosystem

### Project Structure

```
# Rust Project (unit tests only)
src/
├── retry.rs                 # Unit tests for retry logic
├── transport.rs             # Unit tests for transport (no Docker needed)
├── socks.rs                 # SOCKS5 server implementation
├── main.rs                  # CLI and main application logic
└── lib.rs                   # Library entry point

# Python E2E Project (separate uv-managed project)
tests-e2e/
├── pyproject.toml           # uv project configuration
├── ssh_server.py            # Docker container wrapper
├── socks5_client.py         # SOCKS5 test client
├── tests/
│   ├── test_socks5.py       # SOCKS5 proxy tests
│   └── test_transport.py    # Transport/connection tests
├── conftest.py              # pytest fixtures
└── fixtures/                # Test fixtures
    ├── Dockerfile           # SSH server image with echo server
    └── keys/                # Pre-generated test keys

scripts/
├── check.sh                 # Run all checks (Rust + Python)
├── build-test-image.sh      # Build Docker test image
└── generate-test-keys.sh    # Generate SSH keys for testing
```

### Running Tests

**Unit Tests (Rust):**
```bash
cargo test
```

**E2E Tests (Python):**
```bash
# One-time setup
./scripts/build-test-image.sh

# Run from repo root (uses uv workspace)
uv run pytest
uv run ty check           # Type check with ty (Rust-based, fast)
```

**Full Project Check:**
```bash
./scripts/check.sh        # Run all checks (Rust + Python)
./scripts/check.sh -v     # Verbose mode with full output
```

### UV Workspace

The project uses a **uv workspace** (similar to Cargo workspaces) to manage the Python E2E tests:

```
x2ssh/
├── pyproject.toml          # Workspace root configuration
├── uv.lock                 # Shared lockfile for entire workspace
└── tests-e2e/
    ├── pyproject.toml      # Package configuration (member of workspace)
    └── src/x2ssh_e2e/      # Package source
```

**Key workspace features:**
- Single lockfile (`uv.lock` at root) ensures consistent dependencies
- Run commands from repo root with `uv run <command>`
- No need to `cd` into tests-e2e directory
- Works like `cargo` - workspace-aware commands from anywhere in the repo

### Docker Fixture

- Per-test container (isolated, parallel-safe)
- Pre-baked SSH keys for deterministic auth
- Random host port mapping to avoid conflicts
- Auto-cleanup on test completion

### SSH Keys Generation

To regenerate the test SSH keys in `tests-e2e/fixtures/keys/`:

```bash
./scripts/generate-test-keys.sh
```

This creates a deterministic ED25519 key pair for automated testing.
