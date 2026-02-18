#!/bin/sh
set -e

KEYS_DIR="tests-e2e/fixtures/keys"
mkdir -p "$KEYS_DIR"

# Generate ED25519 key pair (no passphrase for automated testing)
ssh-keygen -t ed25519 -f "$KEYS_DIR/id_ed25519" -N "" -C "x2ssh-test@localhost"

echo "Keys generated in $KEYS_DIR:"
ls -la "$KEYS_DIR"
