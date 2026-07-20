# Browser Header Forwarding

Last updated: 2026-07-19

This document describes the current experimental Cookie/header-forwarding implementation. Candidate and release extension profiles remove `cookies`, `webRequest`, broad host permissions, and automatic capture. Header forwarding is available only in a dev build created with `VIBE_BROWSER_EXPERIMENTAL_CAPTURE=true`.

## Modes

- `ask` (UI label: **Do not forward (no prompt)**): default for new installs. The extension does not read or send Cookie/header values. It only sends metadata such as URL, host, and `headersAvailable`. There is **no** confirmation dialog — `ask` is a passive skip, not an interactive prompt (`FUN-14`).
- `enabled` (**Always forward**): the extension may read and forward allowlisted request headers for matching HTTP/HTTPS downloads.
- `disabled` (**Never forward**): no browser headers are read or sent. Disabling forwarding clears saved per-task headers and in-memory header cache.

Legacy boolean settings migrate as `true -> enabled` and `false -> disabled`.

## Site Rules

Settings includes a minimal site-rule editor:

- host pattern, for example `example.com` or `*.example.com`
- capture mode: `auto`, `ask` (UI: **Do not capture (no prompt)** — passive skip), or `never`
- header override: inherit, forward, or block

Site header permission is not reused across unrelated host patterns.

The capture-rule value `ask` does not prompt. It behaves as “do not auto-capture,” matching the passive header `ask` semantics.

## Allowlist

The extension and Rust backend accept only:

- `cookie`
- `user-agent`
- `referer`
- `origin`
- `accept`
- `accept-language`
- `dnt`
- `cache-control`
- `pragma`

`authorization`, `proxy-authorization`, `set-cookie`, `range`, `accept-encoding`, `host`, `connection`, `sec-*`, and values containing CR/LF are rejected. Browser handoff never accepts embedded URL credentials or a browser-selected local save path.

## Storage

Only backend-allowlisted headers are persisted. Persisted headers are scoped to one task, expire after 24 hours, and are encrypted before writing to SQLite. The per-install encryption secret is stored in the OS key store.

If the OS key store is unavailable, Vibe keeps headers in memory only and records a structured warning event. It does not fall back to plaintext persistence.

## Recovery

When persisted headers expire or cannot be decrypted, the task enters `needs_attention` with:

- `auth_headers_expired` or `auth_headers_unavailable`
- recovery actions: `check_url`, `restart`

The current recovery path is incomplete. Sending the same URL from the browser again is rejected by duplicate-task handling instead of replacing the expired headers on the original task. Until `FUN-03` is fixed, the UI must not promise that re-sending will recover the original task. The intended fix is to atomically replace headers only when the matching task is in a recoverable `auth_headers_*` state, refresh the TTL, and requeue that same task.
