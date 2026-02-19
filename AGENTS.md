# AGENTS.md - x2ssh Development Guide

## Quick Reference

**Project**: SOCKS5 proxy and VPN tunnel using SSH transport (Rust + Python E2E tests)

**Key Principle**: Split testing - Rust unit tests (fast) + Python E2E tests (Docker-based)

**Current Phase**: Phase 2 complete (SOCKS5), Phase 3 planned (VPN - see VPN.md)

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

# E2E tests (requires Docker, run from repo root)
./scripts/build-test-image.sh         # One-time setup
uv run pytest                         # Run all E2E tests
uv run ty check                       # Type check with ty (Rust-based, fast)
```

### Full Project Check

```bash
./scripts/check.sh                # Run all checks (Rust + Python)
./scripts/check.sh -v             # Verbose mode with full output
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
3. **Keep fixtures in `tests-e2e/fixtures/`** - SSH keys, Dockerfile
4. **Run `./scripts/build-test-image.sh` before first E2E test**
5. **After making changes, run `./scripts/check.sh`** - Verifies all quality checks pass
6. **Module structure: Use `module.rs` instead of `module/mod.rs`** - For cleaner file organization

## Project Structure

```
x2ssh/
├── src/                      # Rust source (main, lib, retry, socks, transport)
├── tests-e2e/                # Python E2E tests (uv workspace member)
│   ├── tests/                # Test files
│   └── fixtures/             # SSH keys, Dockerfile
├── scripts/                  # check.sh, build-test-image.sh, generate-test-keys.sh
└── pyproject.toml            # uv workspace root
```

## When to Add Tests

- **Rust**: Pure logic, no network needed
- **Python**: Full workflows, network behavior, binary testing

## Troubleshooting

- E2E fails? Check: `docker ps`, `./scripts/build-test-image.sh`, `tests-e2e/fixtures/keys/`

## Release Checklist

- [ ] `./scripts/check.sh` passes (runs all checks below automatically)
- [ ] AGENTS.md, README.md and DESIGN.md updated

## See Also

- **DESIGN.md** - Architecture, testing strategy, implementation details
- **VPN.md** - VPN tunnel design and implementation plan
- **README.md** - User documentation
- **todo/UDP_ASSOCIATE.md** - UDP Associate design analysis (deferred)
