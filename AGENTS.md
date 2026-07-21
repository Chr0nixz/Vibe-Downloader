# AGENTS.md

This file gives coding agents the local project context and working rules for Vibe Downloader.

## Project Snapshot

Vibe Downloader is a desktop download manager built with Tauri 2, React 19, TypeScript, Rust, SQLite, and WebExtension Native Messaging.

The project is currently at `0.3.0`. It is not a finished IDM replacement or a stable public release. Treat HTTP/HTTPS as the most mature path, with FTP/FTPS, SFTP, BitTorrent, HLS, DASH, WebDAV, and Metalink entry points present at varying maturity levels.

Before fixing or describing current gaps, read [docs/project-improvement-audit.md](docs/project-improvement-audit.md). It is the canonical active-risk register and provides stable IDs, acceptance criteria, and repair order. Historical audit documents are point-in-time snapshots and must not override current code or the main audit.

Implemented today:

- HTTP/HTTPS task creation and resource probing through HEAD with Range GET fallback.
- SQLite persistence for tasks, segments, settings, browser handoff messages, credentials, proxy settings, checksums, and SFTP known hosts.
- Single-stream downloads, unknown-size downloads, segmented Range downloads, auto-acceleration (dynamic segment splitting), resume validation, segment retry, checkpoint-based progress persistence, and final file auto-renaming.
- Queue scheduling with max active task count, per-host connection slot limits, scheduled download windows, timed speed throttling, and completion actions (cancellable app exit, confirmed shutdown).
- Global speed limiting through a Rust token bucket.
- Per-task speed limits enforced across HTTP/FTP/SFTP/BT, combined with global limit (minimum wins).
- Task priorities (high/normal/low) used by the queue scheduler to dispatch tasks in priority order.
- Per-task proxy override models, encrypted settings, protocol-aware validation, and task detail controls. The HTTP-derived runtime path currently ignores the resolved task proxy; see `FUN-02` before claiming end-to-end support.
- FTP/FTPS task creation and downloads with dynamic parallel segments, SOCKS5 proxy support, encrypted credential storage, and directory probing.
- SFTP task creation and single-file downloads with password or OpenSSH private-key credentials, encrypted credential storage, local-temp pause/resume, directory probing, SOCKS5 proxy support, and TOFU host-key fingerprint verification.
- BitTorrent task creation from magnet links, HTTP/HTTPS `.torrent` URLs, and local `file://*.torrent` files, with multi-file selection, runtime snapshots (piece map, peers, configured trackers, DHT, seeding), SOCKS5 proxy support, and persisted seeding policy. The time limit and session ownership still have active gaps.
- HLS/m3u8 streaming engine with master playlist variant selection, AES-128-CBC decryption, init map (EXT-X-MAP) support, byte range segments, concurrent segment downloads, live polling, and ffmpeg-based MP4 remuxing.
- DASH (MPEG-DASH / MPD) first-pass engine for a limited static/VOD subset, with ffmpeg-based download, MP4 remuxing, and progress monitoring. Dynamic/live, SegmentTimeline, and several inheritance/template cases are unsupported.
- WebDAV/WebDAVS engine mapping to HTTP/HTTPS with Basic Auth credentials, PROPFIND directory probing, and delegation to the HTTP engine.
- Metalink4 engine with manifest parsing, multi-file selection, HTTP/HTTPS mirror failover by priority, per-file progress, and checksum persistence/verification. Multi-hash priority and cross-mirror resume validation remain active gaps.
- Encrypted task credential storage (ChaCha20-Poly1305) for FTP/FTPS, SFTP, and WebDAV, with legacy plaintext migration on startup.
- React task list with store decomposition (task-data, task-ui, speed-history stores), virtualized infinite scroll, cursor pagination, status filters, search, sorting, multi-select, batch actions, command palette, settings page with 7 collapsible sections and search, task details, Chunks/Connections/Requests/Logs views, toast, delete confirmation, recovery actions, 8 accent color themes, floating status window (ball and bar modes), and 7 locales.
- Clipboard link monitoring for all supported protocols (HTTP/HTTPS, FTP/FTPS, SFTP, WebDAV/WebDAVS, magnet, local manifests) while the desktop app is running.
- Browser Native Messaging host, local WebSocket bridge, manifest install/uninstall diagnostics, duplicate request handling, Tauri single-instance forwarding, and manual HTTP/HTTPS handoff. Automatic takeover and Cookie/header forwarding are experimental dev-profile capabilities and are removed from candidate/release packages.
- CI, Tauri build matrix, Release workflow, Specta bindings, and Tauri updater configuration.
- Vitest coverage for pure frontend logic plus Rust unit/integration tests.

Not implemented yet:

- Cloud drive parsing, video sniffing, cloud accounts/sync, and plugin protocols.
- Safari wrapper, browser store submission IDs, production extension signing, and final browser permission review copy.
- BT/FTP/SFTP/Metalink/HLS/DASH/WebDAV reliability gaps vs HTTP/HTTPS. TaskDetails Phase 1–2 diagnostics parity is in place (protocol-aware Requests/Logs, BT hides placeholder Segments, HLS/DASH real segment lists, Metalink per-file Overview, FTP/SFTP mini panels); Phase 3 items such as BT live tracker status remain deferred.
- Site-rule runtime hit telemetry (settings already cover conflict analysis, import/export, and URL try-run). Classification create-dialog live preview and dynamic subdir templates (settings try-run is available).
- OS code-signed production distribution.

Active release blockers:

- `UX-01`: ordinary startup failures leave the app in an unrecoverable loading state.
- `FUN-01`: direct HTTP Basic Auth is used for probe but lost before the actual download.
- `FUN-02`: HTTP/HLS/DASH/Metalink/WebDAV ignore the resolved per-task proxy at runtime, and proxy overrides cannot participate in create-time probe.
- `ARC-01`: the active `source_key` UNIQUE index incorrectly prevents different URLs on the same host from coexisting.
- `ARC-02`: output paths are not atomically reserved, so concurrent same-name tasks can share or overwrite files.
- `ARC-03`: nested download workers, limiter waits, and ffmpeg children do not have reliable cancellation ownership.

Do not weaken, hide, or document around these blockers. Fix them with the acceptance tests specified in the main audit, and update the audit status only after those tests pass.

## Key Directories

```text
src/                         React frontend, app shell, stores, i18n, Tauri adapters
src-tauri/src/               Rust backend commands, download engine, DB, events, logging, platform code
src-tauri/src/db/migrations/ SQLite migrations
src-tauri/src/bin/           vibe-native-host and export-bindings binaries
browser/extension-core/      Shared WebExtension source and manifest template
scripts/                     Extension build and version sync scripts
docs/                        Browser integration, logging, release, roadmap, audit docs
.github/workflows/           CI, Tauri build, and Release workflows
```

## Development Commands

Install and run:

```bash
pnpm install
pnpm tauri dev
```

Useful checks:

```bash
pnpm typecheck      # TypeScript type checking (tsc --noEmit)
pnpm lint           # Biome static analysis (NOT type checking)
pnpm check          # typecheck + lint + i18n completeness
pnpm check:i18n
pnpm test:frontend
pnpm build
pnpm check:bindings
pnpm test:rust
cargo check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
pnpm build:extensions
pnpm verify:extensions
pnpm verify:protocol-matrix
pnpm test:release-tools
```

Note: `pnpm lint` runs Biome (linting/formatting), not TypeScript type checking. Use `pnpm typecheck` or `pnpm check` for type errors; `pnpm check` also verifies i18n completeness.

Run `pnpm build:extensions` when touching `browser/extension-core`, Native Messaging behavior, or related documentation.

Run `pnpm specta` and `pnpm check:bindings` after Rust command or model changes that affect frontend IPC types.

## Architecture Notes

- Frontend state is decomposed into three Zustand stores under `src/stores`: `task-data-store.ts` (task data, indexes, stats, progress patching), `task-ui-store.ts` (selection, nav, search, sort, filter facets), and `speed-history-store.ts` (per-task speed samples). `task-store.ts` is the facade that re-exports from all three plus `task-query.ts`.
- Native command wrappers live in `src/lib/tauri.ts`; browser preview mocks live in `src/lib/tauri-browser.ts`.
- Rust command registration and Specta export live in `src-tauri/src/lib.rs`.
- Download behavior is organized under `src-tauri/src/download/` with a trait-based `EngineRegistry` that routes URLs to the correct engine. Each protocol has its own module: `http/` (segmented coordinator, worker, direct, file), `ftp.rs`, `sftp.rs`, `bt.rs`, `hls.rs`, `dash.rs`, `webdav.rs`, `metalink.rs`.
- SQLite access lives under `src-tauri/src/db/`; `db/mod.rs` is the re-export and shared-constant entry point. Protocol-specific DB modules include `db/metalink.rs`, `db/sftp.rs`, `db/task_credentials.rs`, `db/task_proxy.rs`, `db/task_checksums.rs`, `db/task_files.rs`, and `db/task_state.rs`.
- Task commands are split under `src-tauri/src/commands/tasks/` for create/import, query/detail paging, actions/hash, and debug mock seed helpers.
- Tauri events are defined in `src-tauri/src/events/mod.rs`; `TaskProgressEmitGate` throttles high-frequency progress updates to 250ms minimum intervals.
- Browser handoff commands live in `src-tauri/src/commands/browser.rs`.
- The Native Messaging host binary lives in `src-tauri/src/bin/vibe-native-host.rs`.
- Credential encryption uses ChaCha20-Poly1305 via `secure_headers` helpers; passwords are stored as ciphertext + nonce in SQLite.
- The segment planner (`db/segment_planner.rs`) determines segment count and type based on protocol characteristics and user settings.
- Settings span 29 keys covering downloads, scheduling, proxy, UI (accent colors, sidebar/titlebar options), and desktop integration.

Important current constants:

- Multi-connection threshold defaults to 16 MB.
- Initial segment count defaults to 4 and is clamped to 1-8.
- Max active tasks defaults to 2 and is clamped to 1-8.
- Max connections per host defaults to 8 and is clamped to 1-16.
- HTTP auto-acceleration: max 8 segments, 10s warmup, 5s evaluation, 8 MB minimum remaining.
- FTP dynamic parallel: max 4 segments, 8s warmup, 5s interval, 16 MB minimum split remaining.
- HLS segment retries: 2; the configured live idle threshold is 6 polls, but its current exit condition is ineffective (`ARC-11`).
- BT metadata timeout: 90s; progress interval: 10s.
- DASH progress interval: 500ms.
- SFTP read buffer: 64 KB; progress interval: 300ms.
- Metalink hash buffer: 1 MB.
- Clipboard max text length: 64 KB; poll interval: 1s.
- WebSocket bridge port: 48365.
- Event throttle (`TaskProgressEmitGate`): 250ms minimum interval.
- Speed history limit: 60 samples per task.

## Documentation Rules

- Keep [README.md](README.md) as the concise current-state entry point.
- Keep [docs/ROADMAP.md](docs/ROADMAP.md) as the forward plan, not a changelog.
- Keep [docs/project-improvement-audit.md](docs/project-improvement-audit.md) as the canonical active-risk, priority, acceptance, and repair-order document. Use its IDs in fixes and update status only after its acceptance criteria pass.
- Keep [PRODUCT.md](PRODUCT.md) and [DESIGN.md](DESIGN.md) as product and design constraints.
- Treat `docs/architecture-audit.md`, `docs/cross-platform-audit.md`, `docs/dependency-modernization-audit.md`, `docs/engineering-quality-audit.md`, and `docs/rust-backend-audit.md` as historical snapshots. Preserve their dated findings, but do not use them as current status when they conflict with code or the main audit.
- Do not reintroduce deleted duplicate docs: `docs/functional-design.md` and `docs/ui-design-style.md`.
- Do not describe planned features as implemented. Current gaps are documented explicitly in README and roadmap.

## Coding Rules For Agents

- Read the local implementation before changing behavior. Some older documentation may describe the repository incorrectly.
- When a request names an audit ID, revalidate the cited code, implement the complete acceptance criteria for that ID, add the required tests, and update the audit entry without deleting its historical rationale.
- Do not overwrite or revert unrelated working-tree changes. The workspace may be dirty.
- Keep changes scoped to the requested area.
- Prefer existing patterns over new abstractions.
- Use generated Specta bindings rather than hand-writing IPC types when Rust models or commands change.
- Supported locales: `en`/`zh-CN` (stable, fully translated) and `zh-TW`/`ja`/`ko`/`ru`/`es` (beta, fully translated but marked with a Beta badge in the language selector). Auto-detection only picks stable locales; beta locales require explicit user selection. When adding new i18n keys, update all 7 locale files and run `pnpm check:i18n` to verify completeness.
- Preserve the current browser handoff security boundary: browser handoff is HTTP/HTTPS only, browser handoff URLs must not contain embedded credentials (rejected at the handoff boundary), browsers do not control local save paths, and Cookie/header forwarding must stay explicit, allowlisted, and encrypted when persisted. Candidate/release extensions are minimal-permission manual-handoff builds; automatic capture and header forwarding are dev-only experimental capabilities. Note: direct task creation (UI and clipboard) does extract embedded credentials from HTTP/HTTPS URLs via `legacy_credentials_from_url`, encrypts them, and sanitizes the task URL. That storage behavior is intentional, but the current HTTP runtime consumption bug is tracked as `FUN-01` and must not be documented as working until fixed.
- Keep debug-only mock behavior out of production builds. `seed_mock_tasks` is intentionally debug-only.
- When changing download or resume logic, add or update Rust tests under `src-tauri/tests`.
- When changing frontend behavior, run at least `pnpm typecheck` and `pnpm test:frontend`; run `pnpm build` for UI or bundling changes.

### Comment Rules

All code comments (Rust, TypeScript/React, scripts) must follow these conventions:

**Language**

- All comments must be in English. Do not write Chinese (or other non-English) comments in code files. Locale string literals in `src/i18n/locales/` are content, not comments, and are exempt.

**Style**

- Explain WHY, not WHAT. A comment that restates the code adds noise; a comment that explains the intent, invariant, or trade-off adds value.
- Keep comments concise (2-4 lines for inline, 4-8 lines for doc comments). If more is needed, link to a doc or issue.
- Single space after `//`, `///`, `//!`, and `/**` (e.g., `// comment`, not `//comment` or `/**  comment */`).
- Use `//!` for module-level docs (file purpose, architecture context) and `///` for item-level docs (functions, structs, enums, constants). Not every file needs `//!`, but complex modules (download engines, scheduler, DB layer, security) should have one.
- Prefer comments at the decision point (above the branch/loop/calculation), not at the top of a long function describing everything.

**Audit tags**

- Existing audit tags (`R-1` through `R-4`, `E-1` through `E-12`, `S-1.1`/`S-2.1`/`S-2.2`, `UX-1`) reference review items and must be preserved when editing tagged comments. New comments do not require a tag unless tied to an audit item.
- Tags go at the start: `// R-3: ...`, `/// E-4: ...`, `//! S-1.1: ...`.

**When to add comments**

- Security invariants: why a check exists, what attack it prevents, and what must NOT be "fixed" (e.g., TOFU policy, embedded-credential rejection, header blocklist).
- Concurrency invariants: why a silent `Ok(())` is load-bearing, why a conditional UPDATE uses double-bind, why a CAS single-winner pattern matters.
- Algorithm rationale: magic numbers and thresholds (e.g., `0.8` yield factor, `15%` stability band, `2×` minimum guard) must explain why the value was chosen.
- Edge cases: sentinel values (e.g., `downloaded_until = range_end + 1` means completed), off-by-one conventions, soft vs hard limits.
- Business rules: error code → status mappings, limit combination policies (minimum wins), retry/backoff priorities.

**TODO/FIXME**

- Use `// TODO:` or `// FIXME:` with a brief description. No author tags or timestamps — git blame provides history.
- TODO/FIXME comments should describe what needs to be done, not just that something is incomplete.

**Auto-generated files**

- `src/generated/bindings.ts` is generated by Specta from Rust doc comments. Never hand-edit it. Run `pnpm specta` to regenerate after changing Rust models or command signatures.
- Doc comments on Rust structs/enums/functions that participate in Specta bindings will appear in `bindings.ts` as JSDoc — keep them accurate and in English.

## UX Direction

The UI should stay a dense, calm desktop utility, not a marketing page or card-heavy dashboard.

Preserve:

- Collapsible left navigation (three responsive tiers: mobile bottom bar, tablet compact, desktop expandable), central virtualized task list with infinite scroll, optional right detail panel/drawer, floating status window (ball or bar mode), and bottom status bar.
- Icon buttons with accessible labels and tooltips.
- OKLCH-based color system with 8 accent color themes (blue, purple, teal, green, orange, rose, indigo, amber), each with light/dark variants.
- Clear state colors without turning the app into a noisy neon theme.
- Advanced engine details inside expanded rows or details tabs.
- Keyboard access without hiding primary mouse paths.
- `prefers-reduced-motion` respected across all animations.

## Release Notes For Agents

Release configuration exists but should still be treated as needing end-to-end verification before public release:

- Tauri updater endpoint points at GitHub Release `latest.json`.
- Updater public key is configured.
- Release workflow builds macOS arm64/x64, Linux x64, and Windows x64.
- OS code signing secrets are still reserved for later.

Do not claim OS-signed production distribution unless signing is actually configured and verified.
