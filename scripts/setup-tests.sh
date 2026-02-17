#!/bin/sh
set -e

echo "Building x2ssh-test-sshd Docker image..."
docker build -t x2ssh-test-sshd tests/fixtures/

echo "Done. You can now run 'cargo test'."
