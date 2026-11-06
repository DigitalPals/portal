# Security Policy

Portal Hub is designed to run behind Tailscale and should not be exposed to
the public internet.

## Supported Versions

Only the latest `0.x` release receives security fixes while the project is
pre-1.0.

## Reporting Vulnerabilities

Do not open public issues for vulnerabilities. Report privately to the project
maintainer with:

- affected version
- deployment model
- reproduction steps
- expected impact

## Security Notes

- Session logs may contain secrets. Protect `/var/lib/portal-hub`.
- Synced hosts, settings, and snippets are readable by anyone who can read the
  Hub state directory.
- Vault private keys are stored as client-side encrypted blobs. Hub must never
  receive the vault passphrase, derived key, or decrypted private key material.
- Sync and vault operations are written to `/var/lib/portal-hub/sync/audit.log`
  without private key blobs or terminal previews.
- Web authentication stores users, OAuth tokens, profiles, and web audit events
  in `/var/lib/portal-hub/hub.db`. Protect this database like credential data.
- The owner signs in with passkeys only. Creating the owner, adding a passkey
  and recovery all need a one-time setup link from `portal-hub owner-setup`,
  which only the local service administrator can run; SSH access to the server
  is the recovery path. Links are single use, expire after 30 minutes and are
  stored only as hashes.
- Use a dedicated non-root user.
- Use forced-command SSH keys.
- Restrict access with Tailscale ACLs.
- Consider `PORTAL_HUB_ALLOWED_TARGETS` in shared environments.
- Set a finite `PORTAL_HUB_MAX_LOG_BYTES`.
- Passkeys are bound to the Hub's HTTPS host name and require HTTPS, except for
  localhost development domains such as `portal-hub.localhost`. Renaming the
  Hub's host or tailnet invalidates its passkeys; use `owner-setup --mode reset`.
  Place the Hub behind HTTPS and Tailscale or another private network boundary.
