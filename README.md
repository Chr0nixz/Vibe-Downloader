# Vibe Downloader

Modern desktop download manager (Tauri 2 + React + Rust). This repository currently ships the app shell, design tokens, SQLite schema draft, mock-task dev helper, event bridge, and a stabilized **single-connection HTTP MVP** for creating, probing, pausing, resuming, retrying, deleting, and opening downloads.

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
- **Backend**: Rust commands + SQLite (`sqlx` runtime queries), single-connection `reqwest` HTTP downloads, startup state reset for interrupted tasks, Tauri events (`task.progress`, `queue.changed`).
- **Types**: `tauri-specta` exports to `src/generated/bindings.ts`.
- **Window API**: `@tauri-apps/api/window` (`getCurrentWindow`) with `core:window:*` capabilities. No `@tauri-apps/plugin-window`.

### Title bar (v0)

| Platform | Strategy |
|----------|----------|
| Windows | Custom title bar (`decorations: false` in setup) |
| macOS | Overlay native title bar + traffic-light safe area |
| Linux | System decorations (`decorations: true`) |

## CI

- **L1 (required)**: `.github/workflows/ci.yml` — typecheck, lint, Vite build, `cargo check`, `clippy`.
- **L2 (allowed-failure)**: `.github/workflows/tauri-build.yml` — Windows / macOS / Linux `pnpm tauri build`. Job names include `allowed-failure` until all platforms are green.

## Docs

- [docs/ROADMAP.md](docs/ROADMAP.md) — 分阶段开发路线图
- [PRODUCT.md](PRODUCT.md) — 产品上下文（Impeccable）
- [DESIGN.md](DESIGN.md) — 设计系统（Impeccable）

说明：`docs/functional-design.md` 与 `docs/ui-design-style.md` 已恢复，后续开发应与 roadmap、产品文档和设计文档交叉校验。

## License

TBD
