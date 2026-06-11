# Browser Header Forwarding

This document is the current implementation note for browser Cookie/header forwarding.

## Modes

- `ask`: default for new installs. The extension does not read or send Cookie/header values. It only sends metadata such as URL, host, and `headersAvailable`.
- `enabled`: the extension may read and forward allowlisted request headers for matching HTTP/HTTPS downloads.
- `disabled`: no browser headers are read or sent. Disabling forwarding clears saved per-task headers and in-memory header cache.

Legacy boolean settings migrate as `true -> enabled` and `false -> disabled`.

## Site Rules

Settings includes a minimal site-rule editor:

- host pattern, for example `example.com` or `*.example.com`
- capture mode: `auto`, `ask`, or `never`
- header override: inherit, forward, or block

Site header permission is not reused across unrelated host patterns.

## Storage

Only backend-allowlisted headers are persisted. Persisted headers are scoped to one task, expire after 24 hours, and are encrypted before writing to SQLite. The per-install encryption secret is stored in the OS key store.

If the OS key store is unavailable, Vibe keeps headers in memory only and records a structured warning event. It does not fall back to plaintext persistence.

## Recovery

When persisted headers expire or cannot be decrypted, the task enters `needs_attention` with:

- `auth_headers_expired` or `auth_headers_unavailable`
- recovery actions: `check_url`, `restart`

The user must send the download from the browser again so the extension can supply fresh authentication headers.
