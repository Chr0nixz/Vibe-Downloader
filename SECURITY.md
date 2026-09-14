# Security Policy

## Supported versions

Vibe Downloader is in active development. Only the latest release line receives security fixes.

| Version | Supported |
| ------- | --------- |
| 0.5.x   | Yes       |
| < 0.5   | No        |

## Reporting a vulnerability

Use GitHub's private vulnerability reporting: open this repository's **Security** tab and choose **Report a vulnerability**. Do not open a public issue for anything you believe is exploitable.

When reporting, include as much of the following as you can:

- Affected version (or commit) and platform (Windows / macOS / Linux).
- The component involved: the desktop app (Rust backend / React frontend), the `vibe-native-host` Native Messaging host, the browser extension, or the local WebSocket bridge.
- Steps or a script that reproduces the issue.
- Your assessment of the impact.

This is a single-maintainer project, so triage and fixes are best-effort. You will receive an acknowledgement, and once an issue is confirmed, a fix and public credit if desired.

## Scope notes

The areas below carry the most security-relevant surface and document the intended invariants:

- **Browser handoff boundary.** Handoff is HTTP/HTTPS only; handoff URLs with embedded credentials are rejected at the boundary; browsers never control local save paths; Cookie/header forwarding stays explicit, allowlisted, and encrypted when persisted. Dev-profile-only capabilities (automatic takeover, header forwarding) are excluded from candidate/release extension builds.
- **Credential storage.** ChaCha20-Poly1305 encryption with the key held in the OS keyring; legacy plaintext records are migrated on startup.
- **SFTP host keys.** Trust-on-first-use fingerprint verification is intentional; host-key changes must surface a recoverable prompt, never a silent override.

Out of scope: reports about modified or repackaged builds, social engineering of end users, and the absence of OS code signing (a known gap, tracked on the roadmap).
