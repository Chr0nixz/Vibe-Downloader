# Vibe Downloader

Vibe Downloader 是一个现代桌面下载管理器，技术栈为 Tauri 2、React 19、TypeScript、Rust、SQLite 和 WebExtension Native Messaging。

当前仓库已经具备可运行的桌面应用、HTTP/HTTPS 下载核心、任务持久化、队列调度、设置页、诊断日志、基础浏览器扩展交接和发布流水线。项目仍处于 `0.1.0` 开发阶段，重点是把 HTTP 下载、续传、浏览器交接和桌面体验打磨稳定，再扩展更多协议。

## 当前实现状态

已实现：

- HTTP/HTTPS 任务创建、HEAD/Range GET 探测、文件名/大小/Range 支持识别。
- SQLite 持久化：`tasks`、`segments`、`settings`、`browser_messages`。
- 单连接下载、未知大小下载、Range 分段下载、`.vibe-downloading` 临时文件、完成后原子改名。
- 断点续传校验：本地临时文件、segment range、远端大小、ETag、Last-Modified、Range 支持变化。
- 大文件多连接：默认 16 MB 以上启用，默认 4 段，配置上限 8 段。
- 队列调度：默认最多 2 个活跃任务，支持 per-host 连接槽限制，队列按创建时间 FIFO 调度。
- 全局速度限制：后端 token bucket，设置页可配置 B/s。
- 基础恢复动作：重试、稍后重试、另存为、更换目录、重新开始、打开目录、检查 URL。
- 前端体验：任务列表、状态筛选、搜索、详情面板、Chunks/Connections、展开行、速度 sparkline、toast、删除确认、响应式抽屉、简体中文/英文界面。
- 浏览器集成基础：Chromium/Firefox/Opera 开发包、Native Messaging host、manifest 安装/卸载、request id 去重、单实例转发。
- 自动更新基础：Tauri updater endpoint、公钥、Release workflow 和状态栏安装入口。
- 质量基线：TypeScript 检查、Vite 构建、Rust check/clippy/test、Specta 绑定漂移检查、三平台 Tauri build workflow。

尚未完成或仍需打磨：

- 命令面板目前只提供开发环境 mock reset，还不是完整用户命令中心。
- 顶部速度限制按钮只有 UI 入口，实际限速仍在设置页配置。
- 新建下载需要手动 Detect，提交时后端会再次 probe。
- 详情页只有 Overview、Chunks、Connections，尚无 Logs/Request tab。
- `task_events` 表已存在，但任务生命周期日志尚未形成数据闭环。
- 浏览器扩展不自动接管浏览器下载，不转发 Cookie/header，不含商店 ID/签名/Safari wrapper。
- 未实现单任务限速、任务优先级、文件类型规则、批量导入、系统托盘、通知、开机启动、BT/HLS 等能力。

## 环境要求

### Windows

- Rust
- Node.js 20+（CI 使用 Node 22）
- pnpm 10+
- WebView2（Windows 11 通常已预装）

### macOS

- Xcode Command Line Tools：`xcode-select --install`
- Rust
- Node.js 20+
- pnpm 10+

### Linux

```bash
sudo apt update
sudo apt install -y libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev patchelf
```

## 本地开发

```bash
pnpm install
pnpm tauri dev
```

常用脚本：

| 命令 | 用途 |
| --- | --- |
| `pnpm dev` | 启动 Vite dev server |
| `pnpm build` | TypeScript 编译并构建前端产物 |
| `pnpm typecheck` | TypeScript 类型检查 |
| `pnpm lint` | 当前等同于 `tsc --noEmit` |
| `pnpm tauri dev` | 启动桌面应用 |
| `pnpm dev:tauri` | 带 `RUST_LOG` 调试预设启动桌面应用 |
| `pnpm specta` | 从 Rust 导出 `src/generated/bindings.ts` |
| `pnpm check:bindings` | 导出绑定并检查是否漂移 |
| `pnpm test:rust` | 运行 Rust 测试 |
| `pnpm build:extensions` | 生成浏览器扩展开发包到 `browser/dist` |
| `pnpm sync-version <tag>` | 同步版本到 package、Tauri config 和 Cargo |

推荐验证命令：

```bash
pnpm typecheck
pnpm build
pnpm check:bindings
pnpm test:rust
cargo check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
pnpm build:extensions
```

## 项目结构

```text
src/                         React 前端、状态管理、i18n、Tauri adapter
src-tauri/src/               Rust 后端、命令、下载引擎、数据库、日志、平台适配
src-tauri/src/db/migrations/ SQLite migration
src-tauri/src/bin/           独立二进制：vibe-native-host、export-bindings
browser/extension-core/      WebExtension 源码和 manifest 模板
scripts/                     扩展构建、版本同步脚本
docs/                        发布、日志、浏览器集成、路线图、审计文档
.github/workflows/           CI、Tauri build、Release workflows
```

## 架构摘要

- 前端使用 React、Zustand、Tailwind v4、Radix primitives、lucide-react、Framer Motion 和 i18next。
- 前后端命令类型通过 `tauri-specta` 导出到 `src/generated/bindings.ts`。
- 后端使用 `reqwest` 下载、`tokio` 异步任务、`sqlx` SQLite、`tracing` 日志。
- Tauri 事件包括 `task.progress`、`task.updated`、`queue.changed`、`settings.changed`、`browser.integration.changed`、`browser.handoff.*`。
- Windows 使用自绘标题栏；macOS 使用 overlay 标题栏；Linux 保留系统装饰。
- Tauri CSP 已配置为最小化的本地资源策略，生产自动更新通过 GitHub Release `latest.json`。

## 浏览器扩展

```bash
pnpm build:extensions
```

输出：

- `browser/dist/chromium`
- `browser/dist/firefox`
- `browser/dist/opera`

Chrome、Edge、Brave、Vivaldi、Chromium 开发验证可加载 Chromium 包。Firefox 使用 Firefox 包。Safari 目前仅保留平台和文档占位，尚未提供生产 wrapper。

更多细节见 [docs/browser-integration.md](docs/browser-integration.md)。

## CI / Release

- CI：`.github/workflows/ci.yml`，包含前端类型/构建、Rust check/clippy/test、Specta drift。
- Tauri Build：`.github/workflows/tauri-build.yml`，Windows/macOS/Linux 三平台构建，CI 配置关闭 updater artifacts。
- Release：`.github/workflows/release.yml`，由 `v*` tag 或手动触发，构建安装包并生成 updater `latest.json`。

发布流程见 [docs/RELEASE.md](docs/RELEASE.md)。

## 文档索引

- [PRODUCT.md](PRODUCT.md)：产品定位、用户、体验原则。
- [DESIGN.md](DESIGN.md)：设计系统、布局、视觉、组件和可访问性准则。
- [docs/ROADMAP.md](docs/ROADMAP.md)：按当前代码状态整理的后续路线图。
- [docs/project-improvement-audit.md](docs/project-improvement-audit.md)：当前不足和优先级建议。
- [docs/browser-integration.md](docs/browser-integration.md)：Native Messaging 与扩展开发验证。
- [docs/debug-logging.md](docs/debug-logging.md)：应用、native host、前端和扩展日志。
- [docs/RELEASE.md](docs/RELEASE.md)：GitHub Release 和自动更新发布流程。

旧的 `docs/functional-design.md` 和 `docs/ui-design-style.md` 已合并到产品、设计、路线图和审计文档中。

## License

Vibe Downloader 使用 GNU General Public License v3.0 only（`GPL-3.0-only`）。

商业授权可由版权持有人另行提供。公开源码保持 GPL-3.0-only，同时保留对需要不同授权条款的使用场景提供商业许可的可能。

详见 [LICENSE](LICENSE) 和 [CONTRIBUTING.md](CONTRIBUTING.md)。
