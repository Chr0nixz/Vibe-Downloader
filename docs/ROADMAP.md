# Vibe Downloader Roadmap

Last updated: 2026-07-21

Current baseline: `0.4.0`

This document is the forward plan. It is not a changelog or a complete inventory of implemented features. Current user-facing capabilities live in [README.md](../README.md), active risks and acceptance criteria live in [project-improvement-audit.md](project-improvement-audit.md), and protocol evidence lives in [protocol-reliability-matrix.md](protocol-reliability-matrix.md).

## Product Direction

Vibe Downloader is moving toward a trustworthy desktop download manager with:

- an HTTP/HTTPS path that is safe under concurrency, cancellation, restart, and file conflicts;
- lower-maturity protocol engines that make their boundaries explicit and fail without corrupting data;
- dense, predictable desktop workflows with visible recovery paths;
- browser integration whose permissions and behavior match each published build profile;
- reproducible release, migration, and performance evidence.

Reliability takes precedence over adding more protocol or cloud-service entry points.

## Current Baseline

The repository contains:

- Tauri 2, React 19, Rust, Tokio, SQLite, Specta, and WebExtension Native Messaging infrastructure.
- A mature HTTP/HTTPS core with probe, single-stream and Range downloads, segmented resume, retry, checkpoint persistence, diagnostics, and speed limits.
- First-pass FTP/FTPS, SFTP, BitTorrent, HLS, static DASH, WebDAV, and Metalink engines.
- Queue scheduling, priority, per-host slots, scheduled windows, completion actions, encrypted credentials, task proxy records, checksum records, and SFTP TOFU storage.
- A cursor-paged and virtualized task UI with filtering, sorting, batch actions, command palette, details, diagnostics, settings, responsive navigation, themes, and seven locales.
- Native Messaging and WebSocket browser integration with manual HTTP/HTTPS hand-off in minimal-permission builds.
- CI, multi-platform Tauri build workflows, release tooling, updater configuration, and a substantial Rust and frontend test suite.

This baseline is not yet a stable release. Six active P0 issues can cause core workflow failure, policy mismatch, or data corruption. They are tracked below and in the main audit.

## Phase A: Release Blockers

Phase A must finish before any public stable release or expansion of product scope.

### A1. Database Deduplication Semantics

Audit ID: `ARC-01`

- Remove the host-level active-task UNIQUE constraint on `tasks(source_key)`.
- Keep BitTorrent info-hash uniqueness in the torrent-specific model.
- Add migration tests proving that different URLs on one host can coexist in queued, paused, and downloading states.

### A2. Atomic Output Reservation

Audit ID: `ARC-02`

- Give every task a UUID-scoped temporary path.
- Reserve final paths atomically across concurrent task creation.
- Publish completed files with no-clobber semantics on same-volume and cross-volume paths.
- Add concurrent same-name and external-conflict integration tests.

### A3. Download Ownership And Cancellation

Audit IDs: `ARC-03`, `ARC-04`

- Remove detached nested download workers or retain and terminate every child handle.
- Make speed-limiter waits cancellation-aware.
- Manage ffmpeg as a cancellable child with kill, wait, cleanup, and kill-on-drop behavior.
- Release scheduler slots only after workers have actually exited.

### A4. HTTP Authentication

Audit ID: `FUN-01`

- Carry direct HTTP Basic credentials from create and probe through scheduler, download, resume, and sidecar requests.
- Preserve URL and log sanitization and encrypted-at-rest credentials.
- Add protected Range-service lifecycle tests.

### A5. Per-Task Proxy Routing

Audit ID: `FUN-02`

- Add proxy override fields to create and directory-probe inputs.
- Use the same resolved proxy for initial probe and runtime download.
- Make HTTP, HLS, DASH, Metalink, and WebDAV select clients from the task-resolved proxy rather than the global proxy.
- Verify Inherit, Off, and Custom with a real proxy listener.

### A6. Startup Failure Recovery

Audit ID: `UX-01`

- Add a structured `startup_failed` state.
- Expose localized retry, restart, log, and data-directory actions.
- Make startup retry idempotent and prevent duplicate background services.

## Phase B: Primary Workflow Correctness

Phase B closes user-visible and cross-layer correctness gaps.

### B1. Query And Event Consistency

Audit IDs: `ARC-07`, `ARC-08`, `ARC-09`

- Separate the entity cache from query-keyed page membership.
- Ignore stale responses and preserve the latest pending query.
- Merge queue event IDs during debounce and upgrade mixed batches to full refresh when required.

### B2. Scheduler And State Concurrency

Audit IDs: `ARC-05`, `ARC-06`

- Keep remote probe work outside the global scheduler lock.
- Use immediate or single-statement conditional state transitions with bounded BUSY retry.
- Stress checkpoint, pause, retry, cancel, and completion races across multiple tasks.

### B3. Browser Settings And Recovery

Audit IDs: `UX-03`, `UX-06`, `UX-07`, `FUN-03`, `FUN-13`, `FUN-14`

- Introduce draft, validation, save, cancel, and undo behavior for site rules.
- Remove the duplicate two-state/three-state header-forwarding controls.
- Implement or rename the current passive `ask` mode.
- Allow a fresh hand-off to replace expired headers on the original recoverable task.
- Keep release builds minimal-permission until capture permissions complete store review.

### B4. Create And Bulk Workflows

Audit IDs: `UX-04`, `UX-05`, `FUN-04`, `FUN-05`, `FUN-06`, `FUN-17`

- Switch imported text files directly into bulk preview.
- Move pause-all and resume-all to backend-defined global operations.
- Use one create-draft contract for credentials, proxy, checksum, priority, category, duplicate policy, and media selection.
- Make authenticated directory probes use the same secure credential and proxy path as task creation.
- Remove checksum-discovery and MIME-classification races.

### B5. Recovery And Accessibility — audit Closed

Audit IDs: `UX-02`, `UX-08`, `UX-09`, `UX-10`, `UX-11`, `ARC-14`, `ARC-15` (all Closed in the main audit)

- Restore custom context menus without exposing unwanted WebView native menus.
- Add a visible desktop detail close action and correct Queue Center keyboard semantics.
- Preserve independent undo actions when multiple toasts are present.
- Localize all stable error codes.
- Replay cold-start browser hand-offs and add explicit SFTP known-host management.

## Phase C: Protocol Reliability

Phase C raises each advertised protocol from “entry point exists” to a tested lifecycle.

### C1. Metalink Integrity — audit Closed

Audit IDs: `FUN-08`, `FUN-09` (both Closed in the main audit)

- Define strongest-hash and multi-hash completion semantics.
- Persist remote validators and validate Content-Range on resume.
- Prevent cross-mirror resume without a trustworthy checksum or validator.

### C2. HLS And DASH

Audit IDs: `FUN-10`, `FUN-12`, `ARC-10`, `ARC-11` — **Closed**

- Reuse the main HLS pipeline for selected audio and subtitle tracks.
- Resolve relative track URIs and fail visibly when selected tracks cannot be produced.
- Bound manifest and playlist bodies with streaming reads and cancellation.
- Make live idle polling and target-duration sleeps bounded and cancellable.
- Build a representative static MPD corpus and explicitly reject unsupported inheritance, timeline, or multi-period cases.

### C3. BitTorrent

Audit IDs: `FUN-11`, `FUN-15`, `ARC-12`, `ARC-13` — **Closed**

- Enforce both seeding ratio and time limits without consuming ordinary download slots after completion.
- Make session ownership, reference counting, and speed policy explicit.
- Publish real per-file progress and clearly label configured-only tracker data.

### C4. FTP, SFTP, And WebDAV — audit Closed

Audit ID: `FUN-18` C4 subset (FTP/SFTP/WebDAV Retry + Diagnostics) — **Closed**

- Complete authenticated directory-probe workflows.
- Add credential rotation, restart, proxy-failure, permission-denied, and host-key-recovery tests.
- Keep implicit FTPS over SOCKS5 explicitly unsupported until a safe implementation exists.

### C5. Protocol Acceptance Matrix — audit Closed

Audit ID: `FUN-18` — **Closed**

Every stable protocol now has automated evidence for:

- create and probe;
- download and completion;
- pause and resume;
- retry and network recovery;
- process-interrupt restart (`reset_interrupted_tasks` + cold engine reentry for HLS/DASH/Metalink; BT Restart via `segments.rs` DB contract);
- delete and cleanup;
- proxy and credentials where applicable;
- checksum and remote-change protection where applicable (BT: piece verification contract, not task SHA sidecar);
- stable diagnostics and recovery actions.

The source of truth for current cells is [protocol-reliability-matrix.md](protocol-reliability-matrix.md).

## Phase D: Release And Data Portability

### D1. Browser Distribution

- Reserve formal Chrome Web Store, Edge Add-ons, and Firefox AMO identities.
- Keep public packages on the minimal manual-hand-off profile unless capture permissions pass review.
- Complete store submission, Firefox signing, install-from-store Native Messaging tests, and uninstall cleanup.
- Defer Safari until an Xcode wrapper, Apple account, signing, notarization, and API parity review are available.

### D2. Desktop Distribution

- Run `rc.0` to `rc.1` updater rehearsals on Windows x64, macOS arm64/x64, and Linux x64.
- Verify installation, first launch, database migration, update, relaunch, browser manifest, and uninstall behavior.
- Continue to label packages unsigned until Apple Developer ID and Windows Authenticode signing are configured and verified.

### D3. Backup And Restore — audit Closed

Audit ID: `FUN-16` — **Closed**

- Renamed the task list JSON/CSV output as report export.
- Added a versioned, checksummed `.vibe-backup` format with rollback-safe staged restore.
- Credentials policy: machine-bound ciphertext in the DB backup; global proxy password stays in the OS keyring and is not exported.

### D4. Environment Health Check — shipped (lightweight)

- Settings → Environment aggregates native host, browser manifests, ffmpeg, proxy handshake (custom only; no external business URL), save-dir writability, disk space, database integrity/bak scan, and updater (frontend plugin).
- Supports copyable diagnostic report and safe one-click fixes only (install Native Messaging manifests, open folders, focus settings, export backup, check for updates).
- Does not claim to fix full disks, bad proxy credentials, or a missing host binary without reinstalling the desktop app.

## Phase E: Performance And Maintainability

Performance work must start with measurements rather than assumptions.

### E1. Reproducible Baseline — audit Closed

Audit ID: `PERF-11` — **Closed**（50k+ / UI 冷启动·FPS / HLS·BT soak / CI 绝对门禁 **deferred**）

- Headless harness：`perf_baseline.rs` + `scripts/perf` + `pnpm perf:baseline`（1k smoke，可选 10k）。
- 本机 1k/10k search/filter/list cursor p50/p95 与 `EXPLAIN QUERY PLAN` 见 [performance-baseline-results.md](performance-baseline-results.md)。
- 仍延期：50k+ 全矩阵、release UI 冷启动/滚动 FPS/RSS 实测填表、HLS/BT 长跑、1k 批量删除 soak、CI 绝对数值门禁。

### E2. Known Hotspots

Audit IDs: `PERF-01` through `PERF-08`, `PERF-10`

- Benchmark FTS5 or a normalized search column for leading-wildcard search.
- Stop TaskDetails polling for invisible tabs and prevent overlapping requests.
- Make progress notification work proportional to changed task IDs.
- Cache shared HLS keys and init maps.
- Bound files-version and task-event retention.
- Move synchronous filesystem and user-command work off async workers.
- Use a monotonic, fair, cancellable speed limiter.
- Add bundle-size budgets tied to actual startup and interaction measurements.

### E3. Module Boundaries

Audit IDs: `ARC-16`, `ARC-17`

- Migrate download errors from strings to stable typed categories.
- Split large modules along parser, plan, transfer, process, persistence, and UI orchestration boundaries.
- Preserve existing public contracts and move tests toward extracted pure modules.

## Later Product Scope

These items remain intentionally deferred until Phase A through C are substantially complete:

- stable CLI, JSON-RPC, or REST automation;
- PAC/WPAD proxy support;
- cloud-drive parsing and cloud-account synchronization;
- complete media sniffing;
- plugin protocols;
- advanced site-rule runtime hit telemetry (settings UI now covers conflict analysis, import/export, and URL try-run);
- classification create-dialog live preview and dynamic `{host}/{date}` subdir templates (settings try-run is available);
- TaskDetails Phase 3 protocol diagnostics (BT live tracker status and related runtime depth);
- Safari WebExtension distribution.

## Stable Product Boundaries

The following are intentional unless a future product decision changes them:

- Browser hand-off accepts HTTP/HTTPS only.
- Browser hand-off rejects embedded URL credentials and browser-controlled local save paths.
- Header forwarding remains explicit, allowlisted, local-only, and encrypted when persisted.
- Direct UI and clipboard task creation may extract embedded credentials, encrypt them, and sanitize stored URLs.
- HLS AES-128 support does not imply SAMPLE-AES or DRM support.
- WebDAV Basic Auth does not imply complete enterprise WebDAV compatibility.
- Unsupported proxy/protocol combinations must fail explicitly rather than silently bypassing the configured proxy.

## Verification Baseline

For frontend and ordinary changes:

```bash
pnpm check
pnpm test:frontend
pnpm build
```

For Rust command, model, database, scheduler, or engine changes:

```bash
pnpm specta
pnpm check:bindings
cargo test --manifest-path src-tauri/Cargo.toml -j 1
cargo check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
```

For browser and release changes:

```bash
pnpm verify:extensions
pnpm test:release-tools
pnpm verify:protocol-matrix
```

Passing these general commands does not replace the scenario-specific acceptance tests in the main audit.
