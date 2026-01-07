#!/bin/bash
set -e

# Start SSH daemon in foreground
exec /usr/sbin/sshd -D -e
