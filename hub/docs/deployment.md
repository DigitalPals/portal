# Manual Portal Hub Deployment

> **Recommended:** let Portal install and manage Hub for you (Settings → Portal
> Hub → Set up on a server → Deploy Hub); see the
> [Portal README](../../README.md#portal-hub-beta) and
> [the deployment protocol](../../deploy/hub/README.md). This guide covers running
> the Hub binary by hand. The `install-debian.sh` installer of the retired
> standalone repository is not part of this repository and is unsupported.
> Current runtime behavior: [integrated-runtime.md](integrated-runtime.md).

This guide targets a Debian or Ubuntu host or LXC reachable through Tailscale.

## Suggested LXC Size

For personal use:

```text
1 vCPU
512 MB to 1 GB RAM
10 GB disk
512 MB swap
```

Disk is the main resource to watch because Portal Hub records terminal output
for reconnect replay and thumbnails.

## Packages

```sh
sudo apt update
sudo apt install -y openssh-server openssh-client dtach util-linux
```

`script(1)` is provided by util-linux. Install Tailscale from its
[official Linux packages](https://tailscale.com/download/linux).

## Tailscale

Enable Tailscale on the server:

```sh
sudo tailscale up
```

If Tailscale cannot access `/dev/net/tun`, either allow the device in the LXC
configuration or run Tailscale in userspace networking mode. Enable MagicDNS and
HTTPS certificates for the tailnet in the Tailscale admin console; passkeys
need the Hub's HTTPS name.

Restrict access with Tailscale ACLs. Portal Hub assumes the SSH service is not
reachable from the public internet.

## User And State

```sh
sudo useradd --system --create-home --shell /bin/sh portal-hub
sudo install -d -o portal-hub -g portal-hub -m 0700 /var/lib/portal-hub
sudo install -d -o portal-hub -g portal-hub -m 0700 /home/portal-hub/.ssh
```

## Build And Install

Build the Hub from the root of this repository (a plain `cargo build` there
builds the desktop) and install the binary:

```sh
cargo build --locked --release -p portal-hub
sudo install -m 0755 target/release/portal-hub /usr/local/bin/portal-hub
```

Run:

```sh
sudo -u portal-hub portal-hub doctor
```

The doctor command checks dependencies, state directory permissions, and whether
the process is running as a non-root user.

## Web Service

Portal desktop uses the web service for passkey and OAuth sign-in, sync, session
listing, and persistent terminal WebSocket streams. Service installation can use
`examples/systemd/portal-hub-web.service`. The service listens on HTTP on
loopback; publish it privately over HTTPS with Tailscale Serve.

Set `PORTAL_HUB_PUBLIC_URL` to the Serve origin (uncomment it in the service
file) and bind the Hub web service to loopback:

```sh
PORTAL_HUB_PUBLIC_URL=https://portal-hub.example.ts.net \
  portal-hub web --bind 127.0.0.1:8080
sudo tailscale serve --bg http://127.0.0.1:8080
```

## Owner Passkey

The owner signs in with passkeys only. Passkeys belong to the Hub's HTTPS host
name, so create the owner once the Hub answers on it. Create a one-time link
(valid 30 minutes) as the Hub user and open it in your browser:

```sh
sudo -u portal-hub env PORTAL_HUB_PUBLIC_URL=https://portal-hub.example.ts.net \
  portal-hub owner-setup --username your-name
```

Save the passkey in a password manager that syncs (Google Password Manager,
iCloud Keychain, 1Password, Bitwarden) or on a security key to use one passkey
on all your devices. Later, `--mode add` adds another passkey and `--mode reset`
replaces all of them if you lose yours; SSH access to the server is the
recovery path.

## Connect Portal

In Portal, open Settings → Portal Hub → **Set up Portal Hub**, choose **Connect
an existing Hub** and enter the HTTPS address, for example
`https://portal-hub.example.ts.net`. Select **Check setup**, then **Sign in**
with your passkey in the browser. Choose which hosts route through the Hub and
what to sync, and save. This computer must be on the same tailnet.

Portal Hub currently supports SSH terminal sessions only.

## Log Retention

Portal Hub stores terminal output in:

```text
/var/lib/portal-hub/logs
```

The web service prunes at startup and hourly. To prune by hand:

```sh
sudo -u portal-hub portal-hub prune --dry-run
sudo -u portal-hub portal-hub prune --ended-older-than-days 14 --max-log-bytes 16777216
```

Live session logs are retained as a moving window capped by
`PORTAL_HUB_MAX_LOG_BYTES` by default. Older replay output is discarded when
needed, but the target session keeps running.

The example systemd timer in `examples/systemd` runs pruning daily as well.

## Optional Target Allowlist

For shared proxy hosts, restrict which target hosts Portal Hub may connect to:

```text
PORTAL_HUB_ALLOWED_TARGETS=*.internal,192.0.2.0/24
```

Patterns support exact hostnames, `*` wildcards, and IP CIDR ranges.

## Optional Logging Disable Mode

To avoid storing terminal output:

```text
PORTAL_HUB_LOGGING_MODE=disabled
```

Sessions still persist across disconnects, but replay and thumbnails are not
available.

## Optional: Legacy SSH Forced Command

The web transport is the primary Portal workflow. The OpenSSH forced-command
mode remains available for manual operation and older clients.

Add the Portal client public key to:

```text
/home/portal-hub/.ssh/authorized_keys
```

Recommended key entry:

```text
restrict,pty,agent-forwarding,command="/usr/local/bin/portal-hub serve --stdio" ssh-ed25519 AAAA...
```

Make sure permissions are strict:

```sh
sudo chown -R portal-hub:portal-hub /home/portal-hub/.ssh
sudo chmod 0700 /home/portal-hub/.ssh
sudo chmod 0600 /home/portal-hub/.ssh/authorized_keys
```

Restrict the `portal-hub` user with `examples/sshd_config.d/portal-hub.conf`
and add the recommended port config to `sshd_config`:

```text
Port 22
Port 2222
```

Reload SSH:

```sh
sudo systemctl reload ssh
```

Smoke-test it from the Portal machine:

```sh
ssh -A -tt -p 2222 portal-hub@TAILSCALE_NAME -- portal-hub doctor
```

Then test a target attach:

```sh
ssh -A -tt -p 2222 portal-hub@TAILSCALE_NAME -- portal-hub attach \
  --session-id 00000000-0000-0000-0000-000000000001 \
  --target-host TARGET_HOST \
  --target-user TARGET_USER
```

Close the SSH client without typing `exit`, reconnect with the same session ID,
and confirm the shell is still alive. Typing `exit` in the target shell should
end the `dtach` session.

Target SSH authentication in forced-command mode is non-interactive and depends
on SSH agent forwarding from the Portal client. Portal desktop's web transport
instead sends a temporary identity file for public-key hosts and uses
interactive SSH auth inside the Hub terminal PTY when needed.
