# Vibe Downloader

Modern desktop download manager (Tauri 2 + React + Rust). This repository currently ships the app shell, design tokens, SQLite schema, event bridge, and a stage 2 HTTP engine with Range resume validation, fixed four-way segmented downloads for large Range-capable files, segment/connection details, and hash-backed regression coverage.

## Prerequisites

### Windows

- [Rust](https://www.rust-lang.org/tools/install)
- [Node.js](https://nodejs.org/) 20+
- [pnpm](https://pnpm.io/) 10+
- [WebView2](https://developer.microsoft.com/en-us/microsoft-edge/webview2/) (usually preinstalled on Windows 11)

### macOS

- Xcode Command Line Tools: `xcode-select --install`
- Rust, Node 20+, pnpm 10+

### Linux

```bash
sudo apt update
sudo apt install -y libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev patchelf
```

See [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for other distributions.

## Development

```bash
pnpm install
pnpm tauri dev
```

### Scripts

| Script | Description |
|--------|-------------|
| `pnpm dev` | Vite dev server (used by Tauri) |
| `pnpm build` | Production frontend build |
| `pnpm typecheck` | TypeScript check |
| `pnpm tauri dev` | Run desktop app |
| `pnpm specta` | Regenerate `src/generated/bindings.ts` from Rust (requires working test runtime) |
| `pnpm test:rust` | Run Rust integration tests |
| `pnpm check:bindings` | Regenerate bindings and fail if `bindings.ts` drifted |
| `pnpm build:extensions` | Build browser extension development packages into `browser/dist` |
| `pnpm sync-version <tag>` | Sync version into `package.json`, `tauri.conf.json`, `Cargo.toml` |

### Verification

```bash
pnpm typecheck
pnpm build
pnpm specta
cd src-tauri
cargo check
cargo clippy -- -D warnings
cargo test
```

## Architecture

- **Frontend**: React, Zustand, Tailwind v4, shadcn-style primitives, Framer Motion (command palette / details).
- **Backend**: Rust commands + SQLite (`sqlx` runtime queries), `reqwest` HTTP downloads, fixed four-way segmented Range downloads for files >= 16 MB, Range resume metadata checks, hardened segment resume validation, `segments` progress records, startup state reset for interrupted tasks, Tauri events (`task.progress`, `queue.changed`).
- **Types**: `tauri-specta` exports to `src/generated/bindings.ts`.
- **Window API**: `@tauri-apps/api/window` (`getCurrentWindow`) with `core:window:*` capabilities. No `@tauri-apps/plugin-window`.

### Current Status

- **Stage 1 HTTP MVP**: complete.
- **Stage 2 resumable segmented HTTP downloads**: accepted. Large Range-capable files use fixed four-way segments, Chunks/Connections show real segment data, and regression tests cover resume, failure, and SHA-256 integrity paths.
- **Stage 3/4 queue, settings, and polish**: in progress in the current working tree. Settings storage, max active task scheduling, queued task UI, Toast, and speed history are implemented.
- **Stage 5 browser handoff**: Native Messaging integration is in progress. See [docs/browser-integration.md](docs/browser-integration.md).

### Title bar (v0)

| Platform | Strategy |
|----------|----------|
| Windows | Custom title bar (`decorations: false` in setup) |
| macOS | Overlay native title bar + traffic-light safe area |
| Linux | System decorations (`decorations: true`) |

## CI / CD

- **CI (required)**: [`.github/workflows/ci.yml`](.github/workflows/ci.yml) — typecheck, lint, Vite build, `cargo check`, `clippy`, `cargo test`, specta bindings drift check.
- **Tauri Build (required)**: [`.github/workflows/tauri-build.yml`](.github/workflows/tauri-build.yml) — Windows / macOS / Linux `pnpm tauri build`.
- **Release**: [`.github/workflows/release.yml`](.github/workflows/release.yml) — triggered by `v*` tags; builds installers, uploads GitHub Release assets, and publishes `latest.json` for the in-app updater.

See [docs/RELEASE.md](docs/RELEASE.md) for secrets, tagging, and first-release checklist.

## Docs

- [docs/RELEASE.md](docs/RELEASE.md) — GitHub Release 与自动更新
- [docs/browser-integration.md](docs/browser-integration.md) — Native Messaging browser handoff
- [docs/ROADMAP.md](docs/ROADMAP.md) — 分阶段开发路线图
- [PRODUCT.md](PRODUCT.md) — 产品上下文（Impeccable）
- [DESIGN.md](DESIGN.md) — 设计系统（Impeccable）

说明：`docs/functional-design.md` 与 `docs/ui-design-style.md` 已恢复，后续开发应与 roadmap、产品文档和设计文档交叉校验。

## License

Vibe Downloader is licensed under the GNU General Public License v3.0 only (`GPL-3.0-only`).

Commercial licensing is available from the copyright holder. This means the public
source release remains under GPL-3.0-only, while separate commercial terms may be
offered for use cases that need a different license.

See [LICENSE](LICENSE) and [CONTRIBUTING.md](CONTRIBUTING.md).
