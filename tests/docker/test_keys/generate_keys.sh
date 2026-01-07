#!/bin/bash
# Generate SSH test keys for Portal integration tests
set -e

cd "$(dirname "$0")"

echo "Generating test SSH keys..."

# Generate unencrypted ed25519 key for testing
if [ ! -f id_ed25519 ]; then
    ssh-keygen -t ed25519 -f id_ed25519 -N "" -C "test@portal"
    echo "Created: id_ed25519 (unencrypted)"
fi

# Generate encrypted ed25519 key for passphrase testing
if [ ! -f id_ed25519_encrypted ]; then
    ssh-keygen -t ed25519 -f id_ed25519_encrypted -N "testpassphrase" -C "encrypted@portal"
    echo "Created: id_ed25519_encrypted (passphrase: testpassphrase)"
fi

# Create authorized_keys from both public keys
cat id_ed25519.pub > authorized_keys
cat id_ed25519_encrypted.pub >> authorized_keys
echo "Created: authorized_keys"

# Set permissions
chmod 600 id_ed25519 id_ed25519_encrypted
chmod 644 id_ed25519.pub id_ed25519_encrypted.pub authorized_keys

echo "Test SSH keys generated successfully!"
