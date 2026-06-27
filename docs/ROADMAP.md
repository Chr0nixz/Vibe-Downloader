# Vibe Downloader Roadmap

Last updated: 2026-06-25

This roadmap reflects the current repository state. Product and design constraints live in [PRODUCT.md](../PRODUCT.md) and [DESIGN.md](../DESIGN.md). Error and browser header-forwarding details live in [error-codes.md](error-codes.md) and [browser-header-forwarding.md](browser-header-forwarding.md).

## Current Baseline

Version: `0.2.0`.

The app now includes a working HTTP/HTTPS desktop download manager, plus lower-maturity FTP/FTPS, SFTP, BitTorrent, HLS, DASH, WebDAV, and Metalink entry points, with:

- Tauri 2 + React 19 + Rust shell for Windows/macOS/Linux.
- HTTP probe, unknown-size single stream downloads, Range segmented downloads, resume validation, segment retry, global speed limit, per-host scheduling, and queue persistence.
- FTP/FTPS task creation and downloads, with credential-bearing URLs moved into encrypted task credentials and sanitized URLs persisted for task records, events, logs, and diagnostics.
- FTP/FTPS directory probing is exposed through the New download flow so directory URLs can show diagnostics and file candidates without creating recursive directory tasks.
- SFTP first-pass task creation and single-file downloads with password credentials, sanitized task URLs, encrypted task credentials, local-temp pause/resume, one-level directory probing, request diagnostics, SOCKS5-only proxy support, and SQLite-backed TOFU host-key fingerprint checks.
- BitTorrent task creation from magnet links, HTTP/HTTPS `.torrent` URLs, and local `file://*.torrent` files. HTTP/HTTPS `.torrent` URLs are routed as BitTorrent tasks by default, selected-file tasks apply file selection before starting, magnet metadata can stop in `needs_attention` for multi-file selection, and BitTorrent runtime snapshots expose piece, tracker, DHT, seeding, and recent-error data.
- Metalink task creation from HTTP/HTTPS and local `file://*.meta4` / `file://*.metalink` manifests. The first pass parses manifest files, persists mirrors and per-file checksums, supports multi-file selection, downloads selected files through HTTP/HTTPS mirror fallback, and verifies the strongest available checksum per file.
- HLS/m3u8 streaming engine with master playlist variant selection, AES-128-CBC decryption (explicit IV or sequence-derived), init map (EXT-X-MAP) support with deduplication, byte range segments with automatic contiguous offsets, concurrent segment downloads with configurable connection limit, live event polling (every `target_duration`, max 6 idle polls), segment persistence to `hls_segments`, and ffmpeg-based local playlist + MP4 remuxing.
- DASH (MPEG-DASH / MPD) engine with `quick_xml` manifest parsing (rejects dynamic/live profiles), ffmpeg-based download with `-c copy -movflags +faststart`, 500ms progress monitoring, and structured error codes for missing ffmpeg, empty output, or invalid manifests.
- WebDAV/WebDAVS engine mapping `webdav://` to `http://` and `webdavs://` to `https://`, with Basic Auth credentials (from URL or encrypted DB storage), PROPFIND depth-1 directory probing (up to 200 entries), multistatus XML parsing, and delegation to the HTTP engine for actual downloads.
- HTTP segmented download auto-acceleration: evaluates speed stability (coefficient of variation within 15%) after a 10s warmup, then splits the largest remaining segment every 5s if conditions are met, up to 8 segments with 8 MB minimum remaining.
- Encrypted task credential storage using ChaCha20-Poly1305 for FTP/FTPS, SFTP, and WebDAV protocols, with automatic legacy plaintext migration on startup.
- Per-task proxy overrides are persisted separately from global proxy settings. HTTP/HTTPS supports HTTP, HTTPS, and SOCKS5 task proxies; BitTorrent, FTP/explicit FTPS, and SFTP support SOCKS5 only and return structured diagnostics for unsupported combinations.
- Global scheduled-download settings cover queued-task download windows, timed stricter global throttling, and completion actions. App exit uses a cancellable countdown; shutdown requires explicit confirmation.
- SQLite persistence for tasks, files, work units, events, request diagnostics, settings, browser handoff messages, hash verification state, task credentials, per-task proxy settings, metalink resources, and SFTP known hosts.
- Task list with Zustand store decomposition (task-data, task-ui, speed-history stores as separate modules behind a facade), virtualized infinite scroll via `@tanstack/react-virtual` with cursor-based pagination, search, filtering, sorting, multi-select, batch actions, command palette, task details, Chunks, Connections, Requests, Logs, toast notifications, recovery actions, and English/Simplified Chinese i18n.
- Settings page overhauled with 7 collapsible sections (Downloads, Advanced Downloads, Scheduled Downloads, Network, Interface, Desktop Integration, Browser Integration), sticky search bar with IntersectionObserver scroll-spy, auto-save with 1000ms debounce, accent color picker (8 colors), scheduled download windows, proxy configuration, and reset-to-defaults dialog.
- Floating status window as a separate Tauri window with ball mode (84px circular SVG progress ring with aurora glow) and bar mode (240px edge-docked pill), drag-to-move, edge snapping, double-click to focus main window, and completion burst animation.
- OKLCH-based color system with 8 accent color themes (blue, purple, teal, green, orange, rose, indigo, amber), each with light/dark variants and three energy levels (primary, energy, peak).
- Collapsible sidebar with three responsive tiers (mobile bottom bar, tablet compact vertical, desktop expandable with labels), mica-style surface with backdrop blur, optional accent stripe indicator, and activity dots for downloading/failed states.
- Browser Native Messaging handoff plus local WebSocket bridge for HTTP/HTTPS URLs, with manifest install/uninstall diagnostics, dev/release extension identity support, popup live status, automatic browser download takeover, optional Cookie/header forwarding, request id de-duplication, and atomic handoff files.
- Clipboard monitoring for supported manual links while the desktop app is running, including HTTP/HTTPS, FTP/FTPS, SFTP, WebDAV/WebDAVS, magnet, HTTP/HTTPS `.torrent`, HTTP/HTTPS Metalink manifests, and local `file://*.torrent` / `file://*.meta4` / `file://*.metalink` / `file://*.mpd`, with user confirmation through the New download flow. Embedded credentials in URLs are extracted, encrypted, and stored separately.
- Batch URL import preview/create flow, cross-task duplicate detection with explicit manual duplicate override, and SHA-256 integrity verification.

## Completed: P0/P1/P2/P3/P4 First Pass

### P0/P1

- Command palette and toolbar actions cover new download, pause, resume, retry, delete, open file/folder, view switching, settings, and speed limit presets.
- New download flow performs automatic probe and supports optional SHA-256 input.
- Task events are written for lifecycle, resume checks, and hash verification.
- Task list supports multi-select, batch actions, sorting, and filters by file type, source, failure reason, and resume capability.
- Settings support friendly speed/size units, advanced download grouping, system notifications, tray behavior, close-to-tray, autostart, clipboard monitoring, and optional startup auto-resume for interrupted tasks.

### P2

- Request diagnostics are persisted in `task_requests` and exposed through paged task request commands.
- Task details include a Requests tab with URL, method, status, Range, If-Range, content length, ETag, duration, retry count, and errors.
- Segment runtime speed is persisted on `task_work_units.speed_bps`; Connections uses real per-segment speed instead of averaging task speed.
- Segment cursor/page and summary commands are available through task detail APIs.
- Resume validation distinguishes strong ETag, weak/missing validators, Last-Modified changes, Range support loss, and local temp/segment corruption, with diagnostic task events.
- Native Messaging handoff files use create-new temp files plus atomic rename; invalid handoff files are logged and cleaned up after read failure.

### P3

- Browser integration exposes dev/release profile, extension id, native host path, manifest path, extension path, and copyable diagnostics in Settings.
- Native Messaging manifests derive allowed origins/extensions from the active browser profile.
- Extension build output syncs the extension version from `package.json` and emits Chrome/Edge/Firefox/Opera variants.
- Extension popup includes bridge status, capture toggles, live tasks, and a recent handoff panel backed by extension local storage.
- Automatic browser download takeover, ask/enabled/disabled Cookie/header forwarding, browser task status, encrypted per-task header restore, and minimal site-rule management are implemented behind explicit settings.

### P4 First Pass

- Batch URL import supports multi-line input, de-duplication, optional probe preview, and partial-success task creation.
- SHA-256 can be supplied at task creation; completed files are verified automatically and can be rechecked manually. Sidecar checksum files (`.sha256`, `.sha512`, `.sha1`, `.md5`) are auto-discovered during probe.
- Hash verification records expected hash, actual hash, status, error, and verification timestamp without deleting failed files.
- HLS/m3u8 streaming engine is implemented as a first pass. DASH/MPD and WebDAV/WebDAVS are implemented as first passes. Cloud drive parsing, video sniffing, cloud accounts/sync, and plugin protocols remain deferred. Metalink and SFTP are implemented as first passes, not as a general protocol plugin framework or a full SSH account browser.
- BT, FTP/FTPS, SFTP, Metalink, HLS, DASH, and WebDAV have stronger diagnostics than the initial entry points, but they are still below the HTTP/HTTPS path in maturity.

## Known Boundaries

- Safari wrapper/signing/review is not implemented.
- Browser store IDs are represented by release placeholders and must be replaced before store submission.
- Browser capture still needs final store review copy and a full end-to-end permission review before public extension submission.
- Browser handoff remains HTTP/HTTPS only; FTP/FTPS, SFTP, WebDAV/WebDAVS, magnet, local torrent files, local Metalink files, and local MPD files are manual/clipboard flows.
- Scheduled download windows currently gate queued task starts; they do not preemptively pause every already-running transfer when the window closes.
- BitTorrent tracker status currently reports configured tracker entries from task metadata; deeper live tracker health depends on engine API support.
- Implicit FTPS over SOCKS5 remains unsupported and returns a diagnostic instead of silently bypassing the task proxy.
- HLS and DASH downloads require `ffmpeg` on the system PATH (or `VIBE_FFMPEG_PATH`); the app reports a structured error if missing.
- Task list uses backend cursor pagination plus frontend windowing for large histories; browser realtime snapshots send active tasks plus a bounded recent history, and the extension caps its live task cache. Future work should benchmark production-scale databases on each target OS.
- BT/FTP/SFTP/Metalink/HLS/DASH/WebDAV hardening and any future plugin protocol work should use mature engines/adapters when scheduled.

## Verification Baseline

For important changes run:

```bash
pnpm typecheck
pnpm test:frontend
pnpm build
pnpm specta
pnpm check:bindings
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
