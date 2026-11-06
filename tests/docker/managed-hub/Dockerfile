FROM debian:trixie-slim
RUN apt-get update && apt-get install -y --no-install-recommends openssh-server sudo python3 ca-certificates && rm -rf /var/lib/apt/lists/*
# Deliberately public fixture password. This server is leased, local-only and
# permits SSH public keys only; no production credentials enter this image.
RUN useradd -m -s /bin/bash portalpass && useradd -m -s /bin/bash portalnopass && \
    printf 'portalpass:portal-fixture-password\nportalnopass:portal-fixture-password\n' | chpasswd && \
    printf 'Defaults timestamp_timeout=0\nportalpass ALL=(root) ALL\nportalnopass ALL=(root) NOPASSWD: ALL\n' > /etc/sudoers.d/portal-fixture && \
    chmod 0440 /etc/sudoers.d/portal-fixture && mkdir -p /run/sshd && \
    printf 'Port 22\nHostKey /etc/ssh/ssh_host_ed25519_key\nPermitRootLogin prohibit-password\nPasswordAuthentication no\nKbdInteractiveAuthentication no\nUsePAM no\nAllowTcpForwarding yes\nAuthorizedKeysFile /etc/ssh/portal_keys\nSubsystem sftp internal-sftp\n' > /etc/ssh/sshd_config && \
    printf 'raise RuntimeError("untrusted local module imported")\n' > /home/portalpass/hashlib.py && \
    cp /home/portalpass/hashlib.py /home/portalnopass/hashlib.py
CMD ["/bin/sh", "-c", "cp /fixture/authorized_keys /etc/ssh/portal_keys && chmod 0644 /etc/ssh/portal_keys && ssh-keygen -A >/dev/null && exec /usr/sbin/sshd -D -e"]
