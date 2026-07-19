# Dependency Modernization Audit

> Historical snapshot: this review records dependency decisions as of 2026-06-29. Recheck `package.json`, Cargo manifests, lockfiles, advisories, and current code before applying any recommendation. Active project priorities remain in [project-improvement-audit.md](project-improvement-audit.md).

Date: 2026-06-29

This document summarizes the current dependency review for Vibe Downloader and
turns the findings into follow-up development guidance. It is intentionally
separate from `README.md`, `ROADMAP.md`, and `project-improvement-audit.md` so
dependency decisions can evolve without bloating the project entry point.

## Scope

Reviewed areas:

- Frontend runtime and tooling dependencies in `package.json`.
- Rust dependencies in `src-tauri/Cargo.toml`.
- Actual source usage for UI primitives, toast, virtualized task lists, protocol
  engines, proxy support, magnet parsing, disk-space checks, and crypto helpers.
- Build output shape under `dist/assets`.
- npm audit result for production dependencies.
- Cargo tree duplication and crate metadata for the main protocol/runtime crates.

Commands used during the audit included:

```bash
pnpm outdated --format json
pnpm audit --prod --json
pnpm list --depth 0
cargo tree --manifest-path src-tauri/Cargo.toml --depth 1
cargo tree --manifest-path src-tauri/Cargo.toml -d
cargo info <crate>
cargo search <crate>
rg <dependency-or-api>
```

Rust CVE audit was not completed because `cargo-audit` is not installed in the
local environment.

## Executive Summary

The project is not generally dependency-stale. The frontend stack is very modern
and healthy: React 19, Vite 8, Tailwind 4, Biome 2, Zustand 5, Radix primitives,
TanStack Virtual, and current Tauri 2 JavaScript packages. `pnpm audit --prod`
reported zero vulnerabilities.

The Rust stack is also mostly current. `reqwest`, `sqlx`, `tokio`, `tauri`,
`russh`, and `suppaftp` are on current major lines. The bigger risks are not old
versions, but:

- RC/pre-release dependencies (`librqbit 9.0.0-rc.0`, `specta/tauri-specta
  2.0.0-rc.25`).
- Duplicate heavy transitive dependencies from BitTorrent (`reqwest 0.12` via
  `librqbit` plus project `reqwest 0.13`).
- A few small stale or unnecessary dependencies (`fs2`, likely-unused `cbc`,
  `magnet-url`).
- Some hand-written protocol parsing where a dedicated parser or deeper use of
  an existing parser may reduce long-term edge-case risk.

## Priority Plan

### P0 - Low-Risk Cleanup

1. Move pnpm build settings out of `package.json`.

   `pnpm` warns that the `pnpm.onlyBuiltDependencies` field in `package.json` is
   no longer read. Move this setting to the current pnpm-supported settings
   location, such as `pnpm-workspace.yaml`.

   Current file: `package.json`

2. Remove `cbc` if verification passes.

   HLS currently implements streaming AES-128-CBC directly via `aes` and
   `aes::cipher` traits. Source search found no real `cbc::` usage in production
   or tests, only comments referencing previous/reference behavior.

   Action:

   ```bash
   cargo check --manifest-path src-tauri/Cargo.toml
   cargo test --manifest-path src-tauri/Cargo.toml hls
   ```

   If those pass after removing `cbc`, drop it from `Cargo.toml`.

3. Replace `magnet-url` with `librqbit`'s built-in `Magnet`.

   `librqbit` re-exports `librqbit_core::magnet::Magnet`, and that parser
   supports `btih`, `btmh`, `dn`, `tr`, `so`, and raw 40-character hashes.
   Since BitTorrent downloads already depend on `librqbit`, using its parser
   avoids parser drift between probing and actual torrent handling.

   Current usage:

   - `src-tauri/src/download/bt.rs`

   Tradeoff: `magnet-url` exposes `length()`. `librqbit`'s parser does not appear
   to expose an equivalent `xl` helper. This is acceptable because magnet total
   size is usually unknown before metadata and the app already falls back to
   `0`. If needed, parse only the optional `xl` query value manually with
   `url::Url`.

4. Review `suppaftp`'s `deprecated` feature.

   `suppaftp` itself is current, but the enabled feature is named `deprecated`.
   Confirm whether the current FTP implementation still needs legacy APIs. If
   not, remove that feature and run FTP tests/checks.

### P1 - Runtime and Bundle Improvements

1. Replace hand-written SOCKS5 handshake with `tokio-socks`.

   Current implementation in `src-tauri/src/proxy.rs` manually implements SOCKS5
   greeting, username/password auth, CONNECT, and response parsing. FTP and SFTP
   use this helper.

   `tokio-socks 0.5.3` supports:

   - TCP CONNECT.
   - Username/password authentication.
   - Domain-name targets.
   - Tokio `TcpStream`.

   It does not support GSSAPI or UDP ASSOCIATE, which are not needed for the
   current FTP/SFTP proxy path.

   Keep existing tests:

   - `socks5_connect_supports_no_auth`
   - `socks5_connect_supports_username_password`

   Add or keep failure tests for auth failure and proxy failure status.

2. Consider `fs2 -> fs4`, not `fd-lock`.

   Current usage is only:

   - `fs2::free_space`
   - `fs2::available_space`

   in `src-tauri/src/commands/system.rs`.

   `fd-lock` is not a replacement because it only provides file descriptor locks
   and does not expose disk-capacity APIs.

   `fs2` is old but small. `fs4 1.1.0` is the modern successor-style candidate
   and describes itself as the original `fs2` modernized with rustix and async
   support. This is a reasonable cleanup, but not urgent.

3. Share HTTP client construction across HTTP/HLS/DASH/Metalink.

   `HttpEngine` already caches `reqwest::Client` by proxy fingerprint, but HLS,
   DASH, and Metalink currently call `build_client` separately. `build_client`
   also creates a new Hickory resolver each time.

   Recommendation: extract a shared `HttpClientPool` keyed by proxy fingerprint
   and backed by a reusable resolver. This should reduce repeated TCP/TLS setup,
   resolver setup, and code duplication.

   Relevant files:

   - `src-tauri/src/download/http/mod.rs`
   - `src-tauri/src/download/hls.rs`
   - `src-tauri/src/download/dash.rs`
   - `src-tauri/src/download/metalink.rs`

4. Review `motion` bundle cost.

   Build output shows a dedicated motion chunk of roughly 133 KB uncompressed.
   The project uses `motion/react` in dialogs, popovers, select, toast, and task
   row animations.

   Options:

   - Keep full `motion` where layout animation is needed.
   - Use CSS/Tailwind animations for simple opacity/translate transitions.
   - Evaluate `motion/react-mini` where it covers the API surface.

   Be careful with Toast and task row behavior because `layout` and
   `AnimatePresence` are currently used for polished transitions.

5. Fix motion chunk naming in Vite config.

   `vite.config.ts` checks for `framer-motion`, while the dependency is now
   `motion`. The current build still emits a `framer-motion-*.js` chunk name.
   Rename the condition/chunk to match the actual dependency.

### P2 - Framework/Parser Evaluation

1. Evaluate `dash-mpd` or an ffmpeg-first DASH path.

   DASH MPD parsing is currently hand-written with `quick-xml` and rejects
   dynamic/live MPDs and `SegmentTimeline`. This is acceptable for the current
   scoped feature, but DASH manifests are complex enough that a parser crate may
   reduce edge-case risk.

   Candidate:

   - `dash-mpd = 0.20.3`

   Alternative: delegate more DASH resolution to ffmpeg and keep Rust focused on
   task lifecycle, diagnostics, and progress.

2. Use `hls_m3u8` more deeply.

   The HLS engine already uses `hls_m3u8` for playlist syntax validation, but
   still hand-parses master variants, media segments, byte ranges, keys, and init
   maps. This is understandable because the engine needs custom progress,
   concurrency, AES handling, and ffmpeg remuxing, but deeper structured parsing
   should be considered as HLS support expands.

3. Keep Metalink custom for now.

   The available `metalink` crate is very old (`0.1.0`). Vibe's Metalink logic
   includes multi-file selection, mirror priority/failover, range workers,
   cooldowns, and checksum verification. Current custom parsing and orchestration
   are justified.

4. Keep WebDAV custom for now.

   The current WebDAV surface is small: PROPFIND directory probing plus
   HTTP/WebDAVS delegation. A large WebDAV client crate is not justified unless
   the app expands into deeper WebDAV operations.

## Frontend Dependency Decisions

### Keep

- `@radix-ui/*`: strongly recommended to keep. Dialog, Select, Popover,
  Tooltip, Tabs, ContextMenu, ScrollArea, Slot, and Separator are used on real
  accessibility-critical paths. Rebuilding these primitives is not worth it.
- `@tanstack/react-virtual`: keep. It is central to the virtualized task list.
- `zustand`: keep. The current task-data/task-ui/speed-history split fits the
  event-driven desktop UI well.
- `i18next` and `react-i18next`: keep. The eager `en`/`zh-CN` plus lazy-loaded
  beta locale (`zh-TW`/`ja`/`ko`/`ru`/`es`) structure is reasonable. All 7
  locales are now fully translated and kept structurally in sync via
  `pnpm check:i18n` (any key mismatch fails CI).
- `lucide-react`: keep. Used broadly for icon buttons and task/status UI.
- `next-themes`: keep unless the app moves theme state fully into its own
  settings store. Current usage is small and harmless.
- `react-error-boundary`: keep. Used for the app-level error boundary.

### Optional: `toast.tsx -> sonner`

`sonner 2.0.7` is modern, MIT licensed, supports React 18/19, and has no runtime
dependencies beyond React peers. It can reduce custom toast maintenance.

However, the current toast stack has useful app-specific behavior:

- Zustand-backed central store.
- Business-key deduplication.
- `updateToast` for long-running actions.
- Action buttons.
- Hidden-count handling and clear behavior.
- Styling aligned with app tokens.

If adopting Sonner, keep a local wrapper that preserves the current `addToast`
and `updateToast` API so callers do not spread Sonner-specific calls throughout
the app. This avoids replacing one local abstraction with a vendor API leak.

Recommendation: optional P1/P2, not required for stability.

## Rust Dependency Decisions

### Keep

- `reqwest`: correct choice for HTTP/HTTPS. Do not replace with raw Hyper for
  this app.
- `sqlx`: correct choice for async SQLite and migrations.
- `tokio` and `tokio-util`: keep.
- `russh` and `russh-sftp`: keep. Do not self-implement SFTP.
- `suppaftp`: keep, but review the `deprecated` feature.
- `librqbit`: keep, but isolate. Do not self-implement BitTorrent.
- `quick-xml`: keep for Metalink/WebDAV and possibly until DASH parsing is
  replaced or delegated.
- `keyring` + `chacha20poly1305`: keep for credential and header encryption.

### Watch

- `librqbit 9.0.0-rc.0`: largest stability and transitive dependency risk. It
  also brings `reqwest 0.12` while the project uses `reqwest 0.13`.
- `specta/tauri-specta 2.0.0-rc.25`: useful, but still RC. Watch for stable
  release and migration notes.
- `chacha20poly1305`: project uses `0.10.x`; `0.11.0` exists. Treat upgrade as
  a focused security-sensitive PR with encryption/decryption regression tests.

### Remove or Replace Candidates

- `cbc`: likely removable.
- `magnet-url`: replace with `librqbit::Magnet`.
- `fs2`: optional replacement with `fs4`; not urgent.
- Hand-written SOCKS5 helper: replace with `tokio-socks`.

## Suggested Verification Matrix

After frontend dependency changes:

```bash
pnpm typecheck
pnpm test:frontend
pnpm build
```

After Rust dependency changes:

```bash
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
```

After Rust command/model changes:

```bash
pnpm specta
pnpm check:bindings
```

After browser handoff or extension-related changes:

```bash
pnpm build:extensions
```

Recommended extra tool:

```bash
cargo install cargo-audit
cargo audit --manifest-path src-tauri/Cargo.toml
```

## Suggested Implementation Order

1. Remove `cbc` if checks pass.
2. Replace `magnet-url` with `librqbit::Magnet`.
3. Move pnpm settings out of `package.json`.
4. Replace custom SOCKS5 handshake with `tokio-socks`.
5. Review/remove `suppaftp`'s `deprecated` feature if possible.
6. Optionally replace `fs2` with `fs4`.
7. Fix Vite motion chunk naming.
8. Evaluate Toast wrapper migration to Sonner.
9. Extract shared HTTP client pool.
10. Evaluate `dash-mpd` or an ffmpeg-first DASH parser strategy.

This order front-loads low-risk dependency cleanup and parser drift reduction,
then moves into runtime refactors and UI vendor decisions.
