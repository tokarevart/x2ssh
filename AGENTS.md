# AGENTS.md - x2ssh Development Guide

## Quick Reference

**Project**: SOCKS5 proxy using SSH transport (Rust + Python E2E tests)

**Key Principle**: Split testing - Rust unit tests (fast) + Python E2E tests (Docker-based)

## Essential Commands

### Build & Run

```bash
cargo build
cargo run -- -D 127.0.0.1:1080 user@server.com
```

### Testing

```bash
# Unit tests (fast, no Docker)
cargo test

# E2E tests (requires Docker)
./scripts/setup-tests.sh              # One-time setup
uv run pytest                       # Run all E2E tests
uv run ty check                     # Type check with ty (Rust-based, fast)
```

### Code Quality

**Rust:**
```bash
cargo fmt
cargo clippy
```

**Python (E2E tests):**
```bash
uv run ruff format              # Format code
uv run ruff check               # Lint
uv run ty check                 # Type check
```

## Critical Rules

1. **NO testcontainers in Rust** - E2E testing moved to Python
2. **E2E tests use `cargo run`** - Test actual binary, not internals
3. **Keep fixtures in `tests/fixtures/`** - SSH keys, Dockerfile
4. **Run `./scripts/setup-tests.sh` before first E2E test**

## Project Structure

```
x2ssh/
├── src/              # Rust source (main, lib, retry, socks, transport)
├── e2e-tests/        # Python E2E tests (uv workspace member)
├── tests/fixtures/   # SSH keys, Dockerfile
└── scripts/          # setup-tests.sh, generate-test-keys.sh
```

## When to Add Tests

- **Rust**: Pure logic, no network needed
- **Python**: Full workflows, network behavior, binary testing

## Troubleshooting

- E2E fails? Check: `docker ps`, `./scripts/setup-tests.sh`, `tests/fixtures/keys/`

## Release Checklist

- [ ] `cargo test` passes
- [ ] `uv run pytest` passes
- [ ] `cargo clippy` && `cargo fmt` clean
- [ ] `uv run ruff check` clean
- [ ] `uv run ty check` clean
- [ ] README.md and DESIGN.md updated

## See Also

- **DESIGN.md** - Architecture, testing strategy, implementation details
- **README.md** - User documentation
