#!/bin/sh
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
FIXTURES_DIR="$PROJECT_ROOT/tests/fixtures"

echo "Building x2ssh-test-sshd Docker image..."
docker build -t x2ssh-test-sshd "$FIXTURES_DIR"

echo ""
echo "Building VPN test Docker images..."
echo "  Building x2ssh-vpn-client..."
docker build -t x2ssh-vpn-client:latest -f "$FIXTURES_DIR/Dockerfile.vpn-client" "$FIXTURES_DIR"

echo "  Building x2ssh-vpn-server-target..."
docker build -t x2ssh-vpn-server-target:latest -f "$FIXTURES_DIR/Dockerfile.vpn-server-target" "$FIXTURES_DIR"

echo ""
echo "Done. You can now run integration tests with 'uv run pytest'."
