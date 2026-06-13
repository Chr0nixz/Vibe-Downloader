# AGENTS.md

This file gives coding agents the local project context and working rules for Vibe Downloader.

## Project Snapshot

Vibe Downloader is a desktop download manager built with Tauri 2, React 19, TypeScript, Rust, SQLite, and WebExtension Native Messaging.

The project is currently at `0.1.0`. It is not a finished IDM replacement yet. Treat HTTP/HTTPS as the most mature path, with lower-maturity FTP/FTPS and BitTorrent entry points already present.

Implemented today:

- HTTP/HTTPS task creation and resource probing through HEAD with Range GET fallback.
- SQLite persistence for tasks, segments, settings, and browser handoff messages.
- Single-stream downloads, unknown-size downloads, segmented Range downloads, resume validation, segment retry, and final file auto-renaming.
- Queue scheduling with max active task count and per-host connection slot limits.
- Global speed limiting through a Rust token bucket.
- React task list, status filters, search, sorting, multi-select, batch actions, command palette, settings page, task details, Chunks/Connections/Requests/Logs views, toast, delete confirmation, recovery actions, and en/zh-CN i18n.
- Clipboard link monitoring while the desktop app is running.
- FTP/FTPS task creation and downloads, plus BitTorrent task creation from magnet links, HTTP/HTTPS `.torrent` URLs, and local `file://*.torrent` files.
- Browser Native Messaging host, local WebSocket bridge, manifest install/uninstall diagnostics, duplicate request handling, Tauri single-instance forwarding, browser download takeover, and optional allowlisted Cookie/header forwarding.
- CI, Tauri build matrix, Release workflow, Specta bindings, and Tauri updater configuration.
- Vitest coverage for pure frontend logic plus Rust unit/integration tests.

Not implemented yet:

- HLS/m3u8 parsing, SFTP, cloud drive parsing, video sniffing, cloud accounts/sync, and plugin protocols.
- Safari wrapper, browser store submission IDs, production extension signing, and final browser permission review copy.
- FTP credential hardening through encrypted task credentials.
- BT/FTP reliability and diagnostics parity with the HTTP/HTTPS path.
- Per-task speed limits, task priorities, and full file classification automation.
- OS code-signed production distribution.

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
pnpm typecheck
pnpm test:frontend
pnpm build
pnpm check:bindings
pnpm test:rust
cargo check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
pnpm build:extensions
```

Run `pnpm build:extensions` when touching `browser/extension-core`, Native Messaging behavior, or related documentation.

Run `pnpm specta` and `pnpm check:bindings` after Rust command or model changes that affect frontend IPC types.

## Architecture Notes

- Frontend state lives in Zustand stores under `src/stores`.
- Native command wrappers live in `src/lib/tauri.ts`; browser preview mocks live in `src/lib/tauri-browser.ts`.
- Rust command registration and Specta export live in `src-tauri/src/lib.rs`.
- Download behavior is organized under `src-tauri/src/download/`; HTTP segmented downloads keep coordinator, worker, and diagnostics helpers separated.
- SQLite access lives under `src-tauri/src/db/`; `db/mod.rs` is the re-export and shared-constant entry point.
- Task commands are split under `src-tauri/src/commands/tasks/` for create/import, query/detail paging, actions/hash, and debug mock seed helpers.
- Tauri events are defined in `src-tauri/src/events/mod.rs`.
- Browser handoff commands live in `src-tauri/src/commands/browser.rs`.
- The Native Messaging host binary lives in `src-tauri/src/bin/vibe-native-host.rs`.

Important current constants:

- Multi-connection threshold defaults to 16 MB.
- Initial segment count defaults to 4 and is clamped to 1-8.
- Max active tasks defaults to 2 and is clamped to 1-8.
- Max connections per host defaults to 8 and is clamped to 1-16.

## Documentation Rules

- Keep [README.md](README.md) as the concise current-state entry point.
- Keep [docs/ROADMAP.md](docs/ROADMAP.md) as the forward plan, not a changelog.
- Keep [docs/project-improvement-audit.md](docs/project-improvement-audit.md) as the risk and prioritization document.
- Keep [PRODUCT.md](PRODUCT.md) and [DESIGN.md](DESIGN.md) as product and design constraints.
- Do not reintroduce deleted duplicate docs: `docs/functional-design.md` and `docs/ui-design-style.md`.
- Do not describe planned features as implemented. Current gaps are documented explicitly in README and roadmap.

## Coding Rules For Agents

- Read the local implementation before changing behavior. Some older documentation may describe the repository incorrectly.
- Do not overwrite or revert unrelated working-tree changes. The workspace may be dirty.
- Keep changes scoped to the requested area.
- Prefer existing patterns over new abstractions.
- Use generated Specta bindings rather than hand-writing IPC types when Rust models or commands change.
- Treat English and Simplified Chinese (`en`/`zh-CN`) as the only actively supported locales for now. Add other locale translations later when explicitly prioritized.
- Preserve the current browser handoff security boundary: browser handoff is HTTP/HTTPS only, URLs may not contain embedded credentials, browsers do not control local save paths, and Cookie/header forwarding must stay explicit, allowlisted, and encrypted when persisted.
- Keep debug-only mock behavior out of production builds. `seed_mock_tasks` is intentionally debug-only.
- When changing download or resume logic, add or update Rust tests under `src-tauri/tests`.
- When changing frontend behavior, run at least `pnpm typecheck` and `pnpm test:frontend`; run `pnpm build` for UI or bundling changes.

## UX Direction

The UI should stay a dense, calm desktop utility, not a marketing page or card-heavy dashboard.

Preserve:

- Left navigation, central task list, optional details panel/drawer, and bottom status bar.
- Icon buttons with accessible labels and tooltips.
- Clear state colors without turning the app into a noisy neon theme.
- Advanced engine details inside expanded rows or details tabs.
- Keyboard access without hiding primary mouse paths.

## Release Notes For Agents

Release configuration exists but should still be treated as needing end-to-end verification before public release:

- Tauri updater endpoint points at GitHub Release `latest.json`.
- Updater public key is configured.
- Release workflow builds macOS arm64/x64, Linux x64, and Windows x64.
- OS code signing secrets are still reserved for later.

Do not claim OS-signed production distribution unless signing is actually configured and verified.
