#!/bin/sh
set -e

echo "Building x2ssh-test-sshd Docker image..."
docker build -t x2ssh-test-sshd tests-e2e/fixtures/

echo "Done. You can now run E2E tests with 'uv run pytest'."
