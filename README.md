# x2ssh

A SOCKS5 proxy and VPN tunnel that uses SSH as the transport layer.

## Features

- **SOCKS5 Proxy**: Route application traffic through an SSH server — no server-side setup required
- **VPN Tunnel** *(in development)*: Route all system traffic through SSH via a TUN interface
- **Robust Retry Logic**: Configurable backoff, max attempts, and health checks
- **Automatic Agent Deployment**: VPN mode deploys the agent binary to the server over SSH
- **Cross-Platform**: Linux and Windows support

## Installation

```bash
cargo build --release
```

The build produces two binaries:
- `x2ssh` — the main client (SOCKS5 proxy + VPN)
- `x2ssh-agent` — the server-side VPN agent (statically linked with musl; embedded in `x2ssh` and deployed automatically)

## Usage

### SOCKS5 Proxy

No server setup required — works with any standard SSH server.

```bash
x2ssh -D 127.0.0.1:1080 user@server.com
```

Configure your application to use `127.0.0.1:1080` as a SOCKS5 proxy.

### VPN Mode *(CLI args implemented; full tunnel forwarding in Phase 3)*

Routes all system traffic through SSH. Requires root on the client and sudo access on the server for iptables/forwarding.

```bash
sudo x2ssh --vpn --config vpn.toml user@server.com
```

**Config file (`vpn.toml`):**

```toml
[vpn]
subnet = "10.8.0.0/24"       # Client gets .2, server gets .1
client_tun = "tun-x2ssh"     # Client TUN interface name
mtu = 1400

# PostUp: run on server after agent starts (iptables NAT, IP forwarding)
# TUN creation is automatic — the agent handles it
post_up = [
    "sysctl -w net.ipv4.ip_forward=1",
    "iptables -t nat -I POSTROUTING -o eth0 -j MASQUERADE",
]

# PreDown: run on server before agent stops (iptables cleanup)
# TUN deletion is automatic — OS handles it when agent exits
pre_down = [
    "iptables -t nat -D POSTROUTING -o eth0 -j MASQUERADE",
]
```

**How it works:**
1. x2ssh deploys `x2ssh-agent` to the server over SSH
2. The agent creates a TUN interface and starts bridging packets
3. x2ssh sets up a TUN on the client and adjusts routing
4. All traffic flows through the SSH tunnel
5. On disconnect, PreDown cleans up iptables rules, then the agent exits and the OS automatically destroys the server TUN

## Options

### SOCKS5 Mode

| Option | Description |
|--------|-------------|
| `-D, --socks <ADDR>` | Start SOCKS5 proxy on specified address (e.g., `127.0.0.1:1080`) |
| `-p, --port <PORT>` | SSH port [default: 22] |
| `-i, --identity <FILE>` | Identity file (private key) |

### VPN Mode

| Option | Description |
|--------|-------------|
| `--vpn` | Enable VPN mode (requires root/sudo) |
| `--config <FILE>` | Config file path |
| `--vpn-subnet <CIDR>` | VPN subnet [default: 10.8.0.0/24] |
| `--vpn-client-tun <NAME>` | Client TUN name [default: tun-x2ssh] |
| `--vpn-mtu <BYTES>` | TUN MTU [default: 1400] |
| `--vpn-exclude <CIDR>` | Exclude CIDR from VPN (can repeat) |
| `--vpn-post-up <CMD>` | PostUp command override (can repeat) |
| `--vpn-pre-down <CMD>` | PreDown command override (can repeat) |

### Retry Policy

| Option | Description |
|--------|-------------|
| `--retry-max <N>` | Maximum retry attempts [default: infinite] |
| `--retry-delay <MS>` | Initial retry delay in ms [default: 1000] |
| `--retry-backoff <N>` | Backoff multiplier [default: 2] |
| `--retry-max-delay <MS>` | Maximum retry delay [default: 30000] |
| `--health-interval <MS>` | Connection health check interval [default: 5000] |

## Examples

```bash
# SOCKS5 proxy
x2ssh -D 127.0.0.1:1080 user@server.com

# SOCKS5 with shorthand port
x2ssh -D 1080 user@server.com

# SOCKS5 with custom SSH key
x2ssh -D 127.0.0.1:1080 -i ~/.ssh/id_ed25519 user@server.com

# SOCKS5 with custom retry policy
x2ssh -D 127.0.0.1:1080 --retry-max 10 --retry-delay 500 user@server.com

# VPN with config file
sudo x2ssh --vpn --config ~/.config/x2ssh/vpn.toml user@server.com

# VPN with inline PostUp/PreDown (no config file)
sudo x2ssh --vpn \
  --vpn-post-up "sysctl -w net.ipv4.ip_forward=1" \
  --vpn-post-up "iptables -t nat -I POSTROUTING -o eth0 -j MASQUERADE" \
  --vpn-pre-down "iptables -t nat -D POSTROUTING -o eth0 -j MASQUERADE" \
  user@server.com
```

## Testing

### Unit Tests

```bash
cargo test
```

### Integration Tests

Integration tests use Docker to spin up SSH containers and test the actual binary:

```bash
# Build the Docker test image (one-time setup)
./scripts/build-test-image.sh

# Run all integration tests
uv run pytest

# Run SOCKS5 tests only
uv run pytest tests/tests/test_socks5.py
```

### Full Project Check

Runs all checks: build, unit tests, integration tests, formatting, linting, type checking:

```bash
./scripts/check.sh
./scripts/check.sh -v    # verbose output
```

## Architecture

See [DESIGN.md](DESIGN.md) for architecture details and [VPN.md](VPN.md) for the VPN tunnel design.

## License

MIT
