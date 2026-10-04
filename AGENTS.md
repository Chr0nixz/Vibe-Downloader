# AGENTS.md

This file is the operating guide for coding agents working on Vibe Downloader. It describes durable project rules and the current architecture. It is intentionally shorter than the audit and roadmap; detailed risk history and acceptance evidence belong in the linked documents.

## User Custom Rules and Notes

This section is maintained by the project owner. Agents must read it before making changes and must preserve its contents. Do not rewrite, remove, reorder, or normalize this section unless the user explicitly asks for that change.

<!-- USER_CUSTOM_RULES_START -->
<!-- Add project-specific rules, priorities, or cautions below. Keep each rule concrete and actionable. -->
<!-- Example: All new user-facing copy must be reviewed in Simplified Chinese before merge. -->
Maintain only English and Simplified Chinese unless specifically instructed to optimize i18n or update project translations. Ignore all other languages.
<!-- USER_CUSTOM_RULES_END -->

## Project Context

Vibe Downloader is a desktop download manager built with Tauri 2, React 19, TypeScript, Rust, SQLite, and WebExtension Native Messaging. The project is currently at `0.5.0` and is still under active development; it is not a stable public release or a complete IDM replacement.

Treat HTTP/HTTPS as the most mature path. FTP/FTPS, SFTP, BitTorrent, HLS, DASH, WebDAV, and Metalink are implemented to different depths. Before describing a gap or fixing an audit item, read [docs/project-improvement-audit.md](docs/project-improvement-audit.md) and then revalidate the current code. The audit is the canonical risk register, but its historical test results are snapshots and may not describe the current dirty worktree.

### Development-Test Caveat

The repository is frequently changed while development is in progress. A passing or failing local test run is evidence for one workspace snapshot, not a release claim. Whenever reporting verification, include the exact command, whether the worktree was dirty, and any environment limits such as Windows linker memory, missing ffmpeg, unavailable real servers, or absent GUI automation. Do not copy old test counts or old audit wording into a new status report.

The main audit distinguishes `Open`, `In progress`, `Fixed locally`, `Closed`, and `Boundary`. `Fixed locally` means implementation and local automation are complete while CI, a candidate package, a real external service, or another environment is still required. Do not change an item to `Closed` only because compilation passes.

## Current Capabilities

- HTTP/HTTPS supports HEAD probing with Range GET fallback, single-stream and segmented downloads, unknown-size handling, dynamic acceleration, validator-aware resume, per-segment retries, checkpoint persistence, no-clobber final publication, task-level automatic retries, and Retry-After deadlines.
- SQLite persists tasks, files, segments/work units, settings, events, diagnostics, credentials, proxies, request profiles, checksums, network policies, backups, and SFTP known hosts. Startup recovery and explicit backup restore paths are present.
- The scheduler supports active-task and per-host limits, priority ordering, queue windows, timed speed policies, persisted retry wakeups, and completion actions. Global and per-task limits are combined with the stricter limit winning.
- FTP/FTPS supports authenticated directory probes, SOCKS5, dynamic parallel workers, encrypted credentials, resume checks, and retries. SFTP supports password/private-key authentication, TOFU host keys, SOCKS5, directory probes, local temporary files, and resume checks.
- BitTorrent supports magnets, local or HTTP/HTTPS torrent files, file selection, piece/peer/DHT/seeding snapshots, SOCKS5, seeding policies, session cleanup, and shared global download accounting.
- HLS supports master-variant selection, AES-128-CBC, init maps, byte ranges, concurrent segments, external audio/subtitles, live polling, bounded reads, and ffmpeg remuxing. DASH supports a limited static/VOD subset, downloads media segments in Rust, and uses ffmpeg for final remuxing; dynamic/live, SegmentTimeline, multi-Period, and unsupported template cases remain boundaries.
- WebDAV/WebDAVS maps onto HTTP semantics with Basic Auth and PROPFIND directory probing. Metalink4 supports local/remote manifests, multi-file selection, mirror priority/failover, per-file progress, and checksum verification.
- The React desktop shell has decomposed Zustand stores, virtualized cursor-paginated task lists, search/sort/filter, batch actions, command palette, detail diagnostics, recovery and backup workspaces, settings search, floating status windows, seven locales, and responsive desktop/tablet/mobile navigation.
- Browser integration provides Native Messaging, a local WebSocket bridge, single-instance forwarding, handoff authorization, diagnostics, and manual HTTP/HTTPS handoff. Automatic capture and Cookie/header forwarding are experimental dev-profile features and are excluded from candidate/release packages.

## Explicit Boundaries

Cloud-drive parsing, full video sniffing, cloud sync/accounts, plugin protocols, Safari packaging, browser-store identities/signing, final permission-review copy, OS code signing/notarization, GUI E2E, and real external-server acceptance are not complete release capabilities. Do not present local fake-server coverage, a protocol matrix marked `automated`, or a successful development build as field verification.

The task-level intranet policy is explicit authorization bound to the source, authority, and resolved addresses for that task. Browser handoff permission is not download permission. Unapproved private targets, public-to-private redirects, DNS rebinding, link-local, multicast, metadata, and other forbidden addresses must remain blocked.

## Repository Map

```text
src/                         React UI, stores, i18n, Tauri adapters
src-tauri/src/               Rust commands, engines, database, scheduler, events, platform code
src-tauri/src/db/migrations/ SQLite migrations
src-tauri/src/bin/           Native host and Specta binding exporter
browser/extension-core/      Shared WebExtension source and manifest template
scripts/                     Build, version, documentation, release, and verification scripts
docs/                        Audit, roadmap, protocol, browser, performance, and release documents
.github/workflows/           CI, Tauri build, security, and release workflows
```

## Development Commands

Requirements: Node.js 20+ (CI uses 22), pnpm 10+, Rust stable, and platform Tauri prerequisites. HLS/DASH output also needs ffmpeg on `PATH`, `VIBE_FFMPEG_PATH`, or the Settings path.

```bash
pnpm install
pnpm tauri dev                 # desktop development
pnpm dev                       # browser preview with mock Tauri adapters
```

Focused checks:

```bash
pnpm typecheck                 # TypeScript only
pnpm lint                      # Biome lint/format check, not TypeScript
pnpm check                     # typecheck + lint + i18n
pnpm test:frontend             # Vitest
pnpm build                     # TypeScript + Vite production build
pnpm test:rust                 # cargo test --locked
pnpm verify:protocol-matrix
pnpm test:release-tools
pnpm build:extensions
pnpm verify:extensions
pnpm perf:baseline
```

Aggregate checks:

```bash
pnpm verify                    # verify:frontend followed by verify:rust
pnpm verify:frontend           # frontend, release tools, i18n, build, bundle, extensions
pnpm verify:rust               # fmt, locked all-target clippy, locked Rust tests
pnpm check:docs                # version, audit IDs, blocker wording, source-comment IDs
```

`pnpm verify` is the local aggregate gate, not a literal copy of every CI step. CI additionally collects frontend/Rust coverage, runs `cargo deny check licenses advisories bans sources`, and runs `pnpm check:bindings` on Ubuntu. After Rust command/model changes, run `pnpm specta`; then inspect `src/generated/bindings.ts` and run `pnpm check:bindings`. A pre-existing dirty diff can make the latter fail even when generation is correct, so separate generated drift from unrelated workspace changes before reporting the result.

On Windows, if Rust linking exhausts page-file or PDB resources, lower Cargo build parallelism, for example `cargo test --locked --manifest-path src-tauri/Cargo.toml -j 2`. `-j` controls compilation parallelism; it does not serialize test threads. Use `-- --test-threads=1` only when the test itself requires serialized execution.

Run `pnpm build:extensions` and `pnpm verify:extensions` when changing extension code, Native Messaging, browser permissions, or related documentation. Candidate/release builds must keep experimental capture disabled.

## Architecture Contracts

- Frontend state is split between `src/stores/task-data-store.ts`, `task-ui-store.ts`, and `speed-history-store.ts`; `task-store.ts` is the facade. Native wrappers live in `src/lib/tauri.ts`; browser mocks live in `src/lib/tauri-browser.ts`.
- Rust commands and Specta registration are centralized in `src-tauri/src/lib.rs`. Task creation/import and detail/action commands are split under `src-tauri/src/commands/tasks/`.
- `EngineRegistry` in `src-tauri/src/download/engine.rs` routes protocol engines. Shared download contracts live in `download/network_policy.rs`, `net_factory.rs`, `lifecycle.rs`, `owned_fs.rs`, `file_ops.rs`, `retry.rs`, and `http/request.rs`.
- All HTTP clients must come from `NetworkClientFactory`; all HTTP requests and redirects must use the origin-safe request path. Do not create an ad-hoc reqwest client or call `.send()` around the shared policy stack.
- `NetworkPolicy` is resolved from the task source, target authorization, authority, and allowed addresses. Every probe, manifest/segment fetch, redirect, retry, and derived resource must use the task policy.
- `lifecycle::run_owned`, its `JoinSet`, `owned_fs`, and `blocking` helpers keep spawned workers and submitted file I/O alive until they drain. Do not replace them with detached tasks or early cleanup after a timeout.
- `file_ops` owns durable sync, atomic no-replace publication, cross-volume staging, and final-path conflict behavior. Never check-then-overwrite a destination.
- The scheduler owns queue dispatch, task runtime controls, speed-policy refresh, retry wakeups, and completion rounds. User actions must not await dispatch while holding a per-task runtime lock.
- SQLite access is under `src-tauri/src/db/`; migrations are append-only. State transitions use the shared state machine and conditional updates. Preserve transaction boundaries for task, file/work-unit, retry, and event state.
- `TaskProgressEmitGate` limits high-frequency UI events to 250 ms. Do not add per-tick full-list queries or unbounded event/data retention.

Useful implementation defaults include a 16 MiB multi-connection threshold, 4 initial segments clamped to 1-8, 2 active tasks clamped to 1-8, 8 host connections clamped to 1-16, HTTP acceleration at most 8 segments, 10 s warmup and 5 s evaluation, a 10-retry task budget with a 30-minute wait cap, a 64 KiB SFTP buffer, a 1 MiB checksum buffer, a 64 KiB clipboard limit, and a 48365 WebSocket bridge port. Verify constants in source before relying on this summary.

## Security and Data Rules

- Browser handoff is HTTP/HTTPS only, rejects embedded credentials, never accepts a browser-controlled local save path, and keeps Cookie/header forwarding explicit, allowlisted, encrypted when persisted, origin-bound, and time-limited.
- Direct UI/clipboard task creation may extract HTTP credentials from a URL, encrypt them, sanitize the stored URL, and consume them at runtime. Do not confuse this with the stricter browser-handoff boundary.
- Task credentials and request-profile secrets use the existing ChaCha20-Poly1305/keyring helpers and zeroization paths. Do not log passwords, private keys, cookies, authorization headers, proxy secrets, or unsanitized URLs.
- User request profiles separate public headers from sensitive headers. Sensitive values expire and must be refreshed explicitly; cross-origin requests must strip them. Preserve header injection, framing-header, size, duplicate, and origin validation.
- Backup restore must validate format, checksum, credentials policy, database migrations, path policy, and machine-local network authorization. Restored private-target grants must not silently become valid.
- Keep SSRF checks at literal-URL, DNS/connection, redirect, and protocol-specific connector boundaries. Do not weaken a guard to make a local fixture pass; authorize the fixture through the test policy instead.
- Cancellation, pause, retry, delete, restart, application exit, and updater installation must converge through the shared lifecycle. A timeout means “stop is still pending”; it does not prove that workers or file I/O have exited.

## Coding Rules

- Read the local implementation before changing behavior. Keep edits scoped and work with unrelated user changes; never reset or revert them.
- Prefer established helpers and module boundaries. Do not duplicate proxy, SSRF, timeout, retry, credential, file-publication, or cancellation logic in a new engine.
- When an audit ID is named, revalidate its acceptance criteria, add the required regression/integration tests, and update the audit only after the criteria and evidence are complete. Preserve historical rationale.
- Rust download/resume/lifecycle changes require Rust tests under `src-tauri/tests` or the relevant module tests. Frontend changes require at least `pnpm typecheck` and `pnpm test:frontend`; UI or bundling changes also require `pnpm build`.
- Rust IPC changes require Specta regeneration and a binding review. Never hand-edit `src/generated/bindings.ts`.
- Backend user-facing errors must be stable codes plus parameters; translate them in the frontend. Raw paths, versions, and probe details may use the diagnostic `raw` escape hatch.
- Supported locales are `en` and `zh-CN` (stable), plus `zh-TW`, `ja`, `ko`, `ru`, and `es` (beta). Add new keys to all seven locale files, run `pnpm check:i18n`, use `TranslationKey` for keys held as data, and format dates/numbers through the application locale helpers.
- Comments are English, concise, and explain why. Use `//!`/`///` for Rust module/item documentation, preserve existing audit tags, and write TODOs as actionable `// TODO:` or `// FIXME:` comments. Keep security, concurrency, algorithm, and sentinel-value rationale at the decision point.
- Do not describe planned work as implemented. When reporting tests, distinguish local automation, CI, candidate-package checks, and real-environment acceptance.

## Documentation Rules

- Keep `README.md` as the concise current-state entry point.
- Keep `docs/ROADMAP.md` as forward planning, not a changelog or implementation inventory.
- Keep `docs/project-improvement-audit.md` as the canonical risk, priority, acceptance, and repair-order document.
- Use [docs/protocol-reliability-matrix.md](docs/protocol-reliability-matrix.md) for automated protocol evidence and [docs/b7-acceptance-matrix-2026-10-02.md](docs/b7-acceptance-matrix-2026-10-02.md) for real-environment and candidate-package evidence.
- Keep `PRODUCT.md` and `DESIGN.md` as product and UI constraints. Treat dated architecture, cross-platform, dependency, engineering, and Rust audits as historical snapshots.
- Do not reintroduce deleted duplicate docs: `docs/functional-design.md` and `docs/ui-design-style.md`.
- If this file gains a durable rule, prefer a short rule plus a link to the owning document. Keep the user custom section intact.

## UX Direction

The UI is a dense, calm desktop utility, not a marketing page or card-heavy dashboard. Preserve the collapsible navigation, virtualized task list, optional detail drawer, floating status window, bottom status bar, keyboard access, accessible icon buttons/tooltips, reduced-motion support, and the eight OKLCH accent themes. Advanced protocol details belong in expanded rows or detail views. Error state must remain visible, actionable, and not dependent on color alone.

## Release Notes

The repository has Tauri updater configuration, a multi-platform build matrix, release automation, extension packages, and a configured updater public key. Treat all of them as needing end-to-end verification while the project is under development. The release workflow does not provide OS code signing/notarization. Do not claim signed production distribution, completed upgrade validation, or field-verified protocol reliability without the required evidence.
