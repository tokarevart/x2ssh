# x2ssh

A CLI tool that provides SOCKS5 proxy functionality using SSH as the transport protocol.

## Features

- **SOCKS5 Proxy**: Route network traffic through an SSH server
- **Robust Retry Logic**: Configurable backoff, max attempts, and health checks
- **Zero Server Setup**: Works with any standard SSH server
- **Cross-Platform**: Linux and Windows support

## Installation

```bash
cargo build --release
```

## Usage

```bash
x2ssh [OPTIONS] <USER@HOST>
```

### Options

| Option | Description |
|--------|-------------|
| `-D, --socks <ADDR>` | Start SOCKS5 proxy on specified address (e.g., `127.0.0.1:1080`) |
| `-p, --port <PORT>` | SSH port [default: 22] |
| `-i, --identity <FILE>` | Identity file (private key) |
| `--retry-max <N>` | Maximum retry attempts [default: infinite] |
| `--retry-delay <MS>` | Initial retry delay in ms [default: 1000] |
| `--retry-backoff <N>` | Backoff multiplier [default: 2] |
| `--retry-max-delay <MS>` | Maximum retry delay [default: 30000] |
| `--health-interval <MS>` | Connection health check interval [default: 5000] |

### Examples

```bash
# Start SOCKS5 proxy
x2ssh -D 127.0.0.1:1080 user@server.com

# Shorthand notation
x2ssh -D 1080 user@server.com

# With custom SSH key
x2ssh -D 127.0.0.1:1080 -i ~/.ssh/id_ed25519 user@server.com

# With custom retry policy
x2ssh -D 127.0.0.1:1080 --retry-max 10 --retry-delay 500 user@server.com
```

## Testing

Build the Docker test image:
```bash
./scripts/setup-tests.sh
```

Run tests:
```bash
cargo test
```

## License

MIT
