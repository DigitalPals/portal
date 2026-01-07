# SSH test server for Portal integration tests
FROM alpine:latest

# Install OpenSSH server and utilities
RUN apk add --no-cache openssh openssh-server bash netcat-openbsd

# Create test user with password
ARG TEST_USER=testuser
ARG TEST_PASSWORD=testpass123
RUN adduser -D -s /bin/bash ${TEST_USER} && \
    echo "${TEST_USER}:${TEST_PASSWORD}" | chpasswd

# Generate host keys
RUN ssh-keygen -A

# Create SSH directory for test user
RUN mkdir -p /home/${TEST_USER}/.ssh && \
    chmod 700 /home/${TEST_USER}/.ssh && \
    chown -R ${TEST_USER}:${TEST_USER} /home/${TEST_USER}/.ssh

# Copy sshd configuration
COPY sshd_config /etc/ssh/sshd_config

# Copy entrypoint script
COPY entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh

# Expose SSH port
EXPOSE 22

# Health check
HEALTHCHECK --interval=2s --timeout=3s --start-period=5s --retries=5 \
    CMD nc -z localhost 22 || exit 1

ENTRYPOINT ["/entrypoint.sh"]
