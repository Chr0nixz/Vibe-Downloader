# Vibe Downloader Roadmap

Last updated: 2026-06-09

This roadmap reflects the current repository state. Product and design constraints live in [PRODUCT.md](../PRODUCT.md) and [DESIGN.md](../DESIGN.md).

## Current Baseline

Version: `0.1.0`.

The app now includes a working HTTP/HTTPS desktop download manager with:

- Tauri 2 + React 19 + Rust shell for Windows/macOS/Linux.
- HTTP probe, unknown-size single stream downloads, Range segmented downloads, resume validation, segment retry, global speed limit, per-host scheduling, and queue persistence.
- SQLite persistence for tasks, files, work units, events, request diagnostics, settings, browser handoff messages, and hash verification state.
- Task list search, filtering, sorting, multi-select, batch actions, command palette, task details, Chunks, Connections, Requests, Logs, toast notifications, recovery actions, and English/Simplified Chinese i18n.
- Browser Native Messaging handoff with manifest install/uninstall diagnostics, dev/release extension identity support, popup recent handoff panel, request id de-duplication, and atomic handoff files.
- Batch URL import preview/create flow and SHA-256 integrity verification.

## Completed: P0/P1/P2/P3/P4 First Pass

### P0/P1

- Command palette and toolbar actions cover new download, pause, resume, retry, delete, open file/folder, view switching, settings, and speed limit presets.
- New download flow performs automatic probe and supports optional SHA-256 input.
- Task events are written for lifecycle, resume checks, and hash verification.
- Task list supports multi-select, batch actions, sorting, and filters by file type, source, failure reason, and resume capability.
- Settings support friendly speed/size units, advanced download grouping, system notifications, tray behavior, close-to-tray, and autostart.

### P2

- Request diagnostics are persisted in `task_requests` and exposed through `list_task_requests`.
- Task details include a Requests tab with URL, method, status, Range, content length, ETag, duration, retry count, and errors.
- Segment runtime speed is persisted on `task_work_units.speed_bps`; Connections uses real per-segment speed instead of averaging task speed.
- Segment pagination and summary commands are available through `list_segments` and `get_segment_summary`.
- Resume validation distinguishes strong ETag, weak/missing validators, Last-Modified changes, Range support loss, and local temp/segment corruption, with diagnostic task events.
- Native Messaging handoff files use create-new temp files plus atomic rename; invalid handoff files are logged and cleaned up after read failure.

### P3

- Browser integration exposes dev/release profile, extension id, native host path, manifest path, extension path, and copyable diagnostics in Settings.
- Native Messaging manifests derive allowed origins/extensions from the active browser profile.
- Extension build output syncs the extension version from `package.json` and emits Chrome/Edge/Firefox/Opera variants.
- Extension popup includes a recent handoff panel backed by extension local storage.
- Cookie/header forwarding and automatic browser download takeover remain intentionally out of scope until a separate privacy design exists.

### P4 First Pass

- Batch URL import supports multi-line input, de-duplication, optional probe preview, and partial-success task creation.
- SHA-256 can be supplied at task creation; completed files are verified automatically and can be rechecked manually.
- Hash verification records expected hash, actual hash, status, error, and verification timestamp without deleting failed files.
- HLS, BT, cloud drive parsing, video sniffing, cloud accounts/sync, and plugin protocols remain deferred.

## Known Boundaries

- Safari wrapper/signing/review is not implemented.
- Browser store IDs are represented by release placeholders and must be replaced before store submission.
- Cookie/header forwarding, site rules, and auto-takeover require a separate privacy and permissions design.
- Task list virtualization for very large history is still a follow-up performance item.
- HLS/BT and plugin protocol work should use mature engines/adapters when scheduled.

## Verification Baseline

For important changes run:

```bash
pnpm typecheck
pnpm build
pnpm specta
pnpm test:rust
cargo check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
```

For browser extension changes also run:

```bash
pnpm build:extensions
```

For release changes also run:

```bash
pnpm tauri build --config src-tauri/tauri.ci.conf.json
```
