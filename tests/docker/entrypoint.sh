#!/bin/bash
set -e

# If authorized_keys is provided via volume mount, set permissions
if [ -f /home/testuser/.ssh/authorized_keys ]; then
    chmod 600 /home/testuser/.ssh/authorized_keys
    chown testuser:testuser /home/testuser/.ssh/authorized_keys
fi

# Start SSH daemon in foreground
exec /usr/sbin/sshd -D -e
