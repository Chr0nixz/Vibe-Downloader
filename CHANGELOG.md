# Changelog

All notable changes to this project are documented in this file. The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html). `docs/ROADMAP.md` remains the forward plan; this file records what actually shipped.

## [Unreleased]

### Added

- Unified network client factory with connect-time SSRF guards and probe budgets shared across protocol engines.
- Metalink parallel plan persistence with strict range validation; signature-safe DASH resume.
- Task lifecycle hardening: stuck queued starts surface as visible failures, completion actions wait for hash verification, and restart quiesces the worker before deleting temp files.

### Fixed

- Security: keyring-failure safety, cross-volume backup, restore scrub, URL sanitization, and BT sessionless probe handling.
- FTP/SFTP: remote identity revalidated before a blind resume; cancellation drains workers before checkpointing.
- HLS: external track playlists ordered by declared sequence; the finish signal wakes via `Notify` instead of polling the database.
- DASH: printf-padded `$Number` templates are now substituted; manifest integer math hardened.
- UI: toast eviction no longer hides pending deletes, scroll position survives append loads, clipboard/drop listeners stay installed, and file pickers report real failures.

## [0.5.0] — 2026-09-08

### Changed

- Release maintenance: version bump to 0.5.0 with a release-tooling encoding fix. No functional changes over 0.4.0.

## [0.4.0] — 2026-07-21

### Added

- Site-rule diagnostics and classification try-run; environment health check in Settings.
- TaskDetails protocol diagnostics for HLS/DASH/Metalink/FTP.
- Reproducible performance baseline harness (PERF-11).

### Fixed

- Protocol matrix closures for completion actions, backup/restore loop, and non-HTTP reliability (FUN-07/16/18).
- Scheduler double-dispatch reservation race (ARC-05); stage-B query consistency and scheduler concurrency.
- HLS key/init fetch coalescing and async hot-path filesystem access; bounded files-version cache and task-event pruning.
- Idle TaskDetails polling and O(N) progress toasts; `SqlitePool` close in the backup integrity error path.
- Segment workers flushed before the cancel checkpoint; completion-action commands made async with timeouts.

## [0.3.0] — 2026-07-18

### Added

- Browser profile system and file-type icons; multi-OS CI matrix and release pipeline hardening.

### Changed

- Major architecture overhaul with protocol engine expansion; responsive UI refinements, mobile nav overflow handling, and load-error retry.

### Fixed

- cargo-deny license and advisory configuration; macOS objc2 0.6.x API for icon extraction; HLS segment staging extension and ffmpeg flag compatibility; out-of-range settings values are clamped instead of rejected.

## [0.2.0] — 2026-06-25

### Added

- DASH and WebDAV protocol support; About page.

### Changed

- Phase-2 hardening pass: dependency upgrades and i18n expansion; documentation synced with the 0.1.1 codebase state.

### Fixed

- Schedule monitor now spawns on the Tauri async runtime (previously panicked in the Tokio reactor); sqlx 0.9 QueryBuilder lifetime and numeric-division errors; read-only attribute cleared before overwriting the bootstrap file; macro double-evaluation in `hash_file` and task diagnostics message priority.

## [0.1.1] — 2026-06-17

### Added

- Initial public release: HTTP/HTTPS, FTP, BitTorrent, HLS, and Metalink engines; segmented downloads with resume validation; browser integration via Native Messaging; encrypted credential storage; task scheduling and per-task proxy; 7-locale UI with a store-decomposed React frontend.
