# Vibe Downloader

Current clipboard workflow: while the desktop app is running, Vibe can monitor
plain-text clipboard content for supported download links and open the New
download flow for confirmation. Clipboard monitoring is enabled by default and
can be disabled in Settings > Desktop integration.

Protocol hardening note: FTP/FTPS/SFTP credential-bearing URLs are sanitized
into task records and encrypted task credentials; BitTorrent selected-file
tasks apply file selection before starting the runtime torrent; HTTP request
diagnostics include Range and If-Range data for resume troubleshooting.
Metalink `.meta4`/`.metalink` manifests are parsed into manifest tasks with
multi-file selection, HTTP/HTTPS mirror fallback, per-file progress, and
manifest-provided checksum verification (strongest available algorithm per file).
SFTP is implemented as a first-pass single-file engine with password
credentials, pause/resume from the local temp file, one-level directory probing,
SOCKS5-only proxy support, and TOFU host-key fingerprint checks.

Scheduling/proxy note: queued tasks can opt into a global download window,
timed global throttling can apply a stricter limit, and completion actions
support cancellable app exit plus explicitly confirmed shutdown. Per-task proxy
overrides are available for supported protocol/proxy combinations: HTTP(S) can
use HTTP(S)/SOCKS5, while BT, FTP/explicit FTPS, and SFTP are limited to SOCKS5.

Runtime dependency note: HLS and DASH downloads require `ffmpeg` to be available
on the system PATH (or via the `VIBE_FFMPEG_PATH` environment variable) for
final MP4 remuxing. The app checks for ffmpeg availability before starting these
downloads and reports a structured error if missing.

Vibe Downloader 是一款现代桌面下载管理器，目标是让大文件下载、断点续传、任务队列和浏览器交接变得更清楚、更可靠，也比传统下载工具更符合现在的桌面审美。

项目目前处于早期开发阶段，但已经可以作为以 HTTP/HTTPS 为主的下载器运行；FTP/FTPS、SFTP、BitTorrent、HLS/m3u8、DASH/MPD、WebDAV 和 Metalink 已接入，成熟度各不同。它不是一个已经完整替代 IDM 的成品，适合尝鲜、参与开发、验证下载引擎和桌面体验。

> 说明：这个项目基本是通过 vibe coding 完成的。产品方向、取舍和验收由人工把关，大部分代码、文档和迭代实现由 AI 辅助完成。

## 你可以用它做什么

当前版本已经支持：

- 新建 HTTP/HTTPS 下载任务。
- 手动新建和剪贴板确认流程支持 FTP/FTPS、SFTP、WebDAV/WebDAVS、magnet、HTTP/HTTPS `.torrent` URL、本地 `file://*.torrent`、`.meta4`/`.metalink` 和 `.mpd`。
- HTTP/HTTPS `.torrent` URL 默认创建 BitTorrent 任务，而不是把 `.torrent` 文件本身当作普通文件下载。
- HLS/m3u8 流媒体下载：主播放列表变体选择、AES-128-CBC 解密、并发分段下载、直播轮询、ffmpeg 封装为 MP4。
- DASH/MPD 流媒体下载：清单解析、ffmpeg 下载并封装为 MP4。
- WebDAV/WebDAVS 文件下载：Basic Auth 认证、PROPFIND 目录探测、委托 HTTP 引擎下载。
- Metalink4 清单下载：多文件选择、HTTP/HTTPS 镜像故障转移、校验和验证（按可用算法优先级取最强一项验证）。
- SFTP 单文件下载：密码认证、加密凭据存储、本地临时文件断点续传、目录探测、SOCKS5 代理、TOFU 主机密钥验证。
- BitTorrent 下载：magnet 和 `.torrent` 支持、多文件选择、运行时快照（piece、peer、tracker、DHT、做种）、SOCKS5 代理、可配置做种。
- FTP/FTPS 下载：动态并行分段、SOCKS5 代理、加密凭据存储、目录探测。
- 自动探测文件名、文件大小、来源 host 和 Range 支持情况。
- 大文件分段下载，默认 16 MB 以上启用多连接；HTTP 支持自动加速（动态分裂分段）。
- 暂停、继续、重试、删除任务。
- 断点续传校验，避免远端文件变化后继续写坏文件。
- 队列调度，默认同时运行 2 个下载任务。
- 定时下载窗口、定时限速窗口和完成动作（可取消的退出应用、需确认的关机）。
- 全局速度限制，覆盖所有协议下载。
- 单任务限速，与全局限速取较小值强制执行。
- 任务优先级（高/普通/低），调度器按优先级派发队列任务。
- 逐任务代理覆盖（继承/关闭/自定义），HTTP(S) 支持 HTTP/HTTPS/SOCKS5，BT/FTP/SFTP 仅 SOCKS5。
- 下载任务搜索、状态筛选、排序、文件类型/来源/失败原因/续传能力筛选。
- 多选任务和批量暂停、继续、重试、删除。
- 批量 URL 导入预览和创建。
- 手动新建、批量导入和浏览器交接会提示重复任务；手动新建可明确选择创建副本。
- 命令面板覆盖新建下载、任务操作、视图切换、排序筛选和限速预设。
- 查看任务详情、分段进度、连接摘要、请求诊断和日志。
- 下载完成后打开文件或所在目录。
- 基础错误恢复动作，例如重试、稍后重试、另存为、更换目录、重新开始、检查链接。
- 系统托盘、完成通知、开机启动、关闭到托盘、剪贴板监控和启动后续传中断任务开关。
- 浮动状态窗口（球形和条形模式），置顶显示下载进度。
- 8 种主题色（蓝、紫、青、绿、橙、玫红、靛蓝、琥珀），系统/亮/暗主题，可选字体。
- 简体中文和英文界面。
- 浏览器扩展开发包，通过 Native Messaging 和本地 WebSocket 把 HTTP/HTTPS 链接、下载捕获和实时任务状态连接到桌面应用。
- 浏览器原生下载自动接管基础能力，可在设置中开启或关闭。
- Cookie/header 转发基础能力，可在设置中开启或关闭，并只转发受控 allowlist header。
- 自动更新基础配置，正式发布前仍需要完整端到端验证。

## 当前还不支持

这些功能还在后续计划中，不要把当前版本当成完整下载器使用：

- 网盘解析、视频嗅探和插件协议。
- 完整站点规则管理 UI。
- 商店版浏览器扩展、Safari wrapper 和正式扩展签名。
- 操作系统代码签名的生产安装包。
- BT/FTP/SFTP/Metalink/HLS/DASH/WebDAV 的可靠性和诊断成熟度仍低于 HTTP/HTTPS。

## 界面和体验方向

Vibe Downloader 不是营销页，也不是大卡片仪表盘。它的主界面面向真实下载任务：

- 可折叠左侧导航查看不同状态的任务（移动端底部栏、平板紧凑侧栏、桌面展开面板）。
- 中间是虚拟化高密度任务列表，支持无限滚动。
- 右侧或抽屉中查看任务详情。
- 浮动状态窗口置顶显示下载进度（球形或条形模式）。
- 底部状态栏显示总速度、活跃任务和队列情况。
- 8 种主题色、系统/亮/暗主题自动跟随、OKLCH 色彩系统。
- 高阶信息放在展开行和详情页里，默认视图保持安静、清楚。

## 本地运行

### 环境要求

Windows：

- Rust
- Node.js 20+（CI 使用 Node 22）
- pnpm 10+
- WebView2（Windows 11 通常已预装）

macOS：

- Xcode Command Line Tools：`xcode-select --install`
- Rust
- Node.js 20+
- pnpm 10+

Linux：

```bash
sudo apt update
sudo apt install -y libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev patchelf
```

### 启动开发版

```bash
pnpm install
pnpm tauri dev
```

只运行前端浏览器预览：

```bash
pnpm dev
```

浏览器预览会使用 mock 数据，不会执行真实 Tauri 下载能力。

## 浏览器扩展开发包

生成扩展开发包：

```bash
pnpm build:extensions
```

输出目录：

- `browser/dist/chromium`
- `browser/dist/firefox`
- `browser/dist/opera`

Chrome、Edge、Brave、Vivaldi、Chromium 可以加载 Chromium 包。Firefox 使用 Firefox 包。Safari 目前还没有生产 wrapper。

更多说明见 [docs/browser-integration.md](docs/browser-integration.md)。

## 开发者信息

常用命令：

| 命令 | 用途 |
| --- | --- |
| `pnpm dev` | 启动 Vite dev server |
| `pnpm build` | TypeScript 编译并构建前端 |
| `pnpm typecheck` | TypeScript 类型检查 |
| `pnpm lint` | Biome 静态分析（lint + 格式检查，不含类型检查） |
| `pnpm check` | typecheck + lint 组合检查 |
| `pnpm test:frontend` | 运行 Vitest 纯 TS 逻辑测试 |
| `pnpm tauri dev` | 启动桌面应用 |
| `pnpm dev:tauri` | 带 Rust 调试日志启动桌面应用 |
| `pnpm specta` | 从 Rust 导出 `src/generated/bindings.ts` |
| `pnpm check:bindings` | 检查 Specta bindings 是否漂移 |
| `pnpm test:rust` | 运行 Rust 测试 |
| `pnpm build:extensions` | 构建浏览器扩展开发包 |
| `pnpm sync-version <tag>` | 同步版本号到 package、Tauri config 和 Cargo |

推荐验证：

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

## 项目结构

```text
src/                         React 前端、状态管理、i18n、Tauri adapter
src-tauri/src/               Rust 后端、命令、下载引擎、数据库、日志、平台适配
src-tauri/src/db/migrations/ SQLite migration
src-tauri/src/bin/           vibe-native-host、export-bindings
browser/extension-core/      WebExtension 源码和 manifest 模板
scripts/                     扩展构建、版本同步脚本
docs/                        发布、日志、浏览器集成、路线图、审计文档
.github/workflows/           CI、Tauri build、Release workflows
```

## 技术栈

- 桌面框架：Tauri 2
- 前端：React 19、TypeScript、Zustand、Tailwind v4、Radix primitives、Motion、i18next
- 后端：Rust、tokio、reqwest、sqlx SQLite、tracing
- 协议引擎：suppaftp (FTP/FTPS)、russh + russh-sftp (SFTP)、librqbit (BitTorrent)、hls_m3u8 + aes/cbc (HLS)、quick-xml (Metalink/DASH)、chacha20poly1305 (凭据加密)
- 类型导出：tauri-specta
- 浏览器交接：WebExtension + Native Messaging
- 发布：GitHub Actions + Tauri updater

## 文档

- [PRODUCT.md](PRODUCT.md)：产品定位和体验原则。
- [DESIGN.md](DESIGN.md)：设计系统和 UI 方向。
- [docs/ROADMAP.md](docs/ROADMAP.md)：后续路线图。
- [docs/project-improvement-audit.md](docs/project-improvement-audit.md)：当前不足和优先级建议。
- [docs/browser-integration.md](docs/browser-integration.md)：浏览器扩展和 Native Messaging。
- [docs/debug-logging.md](docs/debug-logging.md)：日志和排障。
- [docs/RELEASE.md](docs/RELEASE.md)：发布和自动更新。
- [AGENTS.md](AGENTS.md)：给后续代码代理的项目上下文。

## License

Vibe Downloader 使用 GNU General Public License v3.0 only（`GPL-3.0-only`）。

商业授权可由版权持有人另行提供。公开源码保持 GPL-3.0-only，同时保留对需要不同授权条款的使用场景提供商业许可的可能。

详见 [LICENSE](LICENSE) 和 [CONTRIBUTING.md](CONTRIBUTING.md)。
