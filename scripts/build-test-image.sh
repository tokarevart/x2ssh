#!/bin/sh
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
FIXTURES_DIR="$PROJECT_ROOT/tests/fixtures"

echo "Building x2ssh-test-sshd Docker image (for SOCKS5 tests)..."
docker build -t x2ssh-test-sshd "$FIXTURES_DIR"

echo ""
echo "VPN test images are built automatically by docker compose when running VPN tests."
echo "To build them manually: docker compose -f $FIXTURES_DIR/docker-compose.vpn.yaml build"

echo ""
echo "Done. You can now run integration tests with 'uv run pytest'."
