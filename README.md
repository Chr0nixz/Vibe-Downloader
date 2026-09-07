# Vibe Downloader

Vibe Downloader 是一个使用 Tauri 2、React 19、TypeScript、Rust 和 SQLite 构建的桌面下载管理器。项目当前版本为 `0.5.0`，HTTP/HTTPS 是最成熟的路径；FTP/FTPS、SFTP、BitTorrent、HLS、DASH、WebDAV 和 Metalink 已有可运行入口，但成熟度和诊断覆盖不一致。

项目仍处于积极开发阶段，不是已经完整替代 IDM 的稳定成品。当前代码存在若干发布阻断问题，开发和试用前请先阅读 [项目改进审计](docs/project-improvement-audit.md)。

> 本项目主要通过 AI 辅助开发。产品方向、取舍和验收由人工把关，代码和文档中的能力声明以当前实现、自动化证据和审计结论为准。

## 当前定位

Vibe Downloader 的目标是提供清晰、可靠、可诊断的大文件下载、断点续传、任务队列和浏览器交接体验，同时保持密集、安静的桌面工具界面。

当前适合：

- 开发和验证 HTTP/HTTPS 下载、分段、限速、队列和恢复逻辑。
- 试用多协议下载入口及其诊断界面。
- 参与 Tauri、Rust、React、SQLite 和 WebExtension 集成开发。

当前不适合：

- 作为唯一的生产下载工具处理不可替代文件。
- 假设所有协议与 HTTP/HTTPS 具有相同可靠性。
- 假设开发扩展中的实验能力会出现在正式商店包中。

## 已实现能力

### HTTP/HTTPS 主路径

- HEAD 探测，并在需要时回退到 Range GET。
- 已知大小、未知大小、单流和 Range 分段下载。
- 默认 16 MB 以上启用多连接，初始 4 段，可按稳定性和剩余大小动态拆分到最多 8 段。
- 暂停、继续、重试、checkpoint、远端 validator 校验和断点续传。
- 全局限速、逐任务限速、每主机连接槽和任务优先级。
- 请求诊断，包括 Range、If-Range、ETag、状态码、耗时和重试信息。
- 下载完成后的文件发布、打开文件和打开所在目录。

HTTP Basic Auth（`FUN-01`）和 HTTP 系逐任务代理（`FUN-02`）已修复并有集成测试覆盖。剩余缺口是 DASH/FTP/SFTP 的**探测**路径尚未接入逐任务代理，见 `FUN-20`。

### 其他协议

| 协议 | 当前能力 | 主要边界 |
| --- | --- | --- |
| FTP/FTPS | 单文件、动态并行分段、加密凭据、目录探测（支持对话框凭据与代理）、SOCKS5 | implicit FTPS over SOCKS5 不支持；取消时未等待 worker 落盘（`ARC-19`）；无连接超时（`ARC-26`）；探测未接逐任务代理（`FUN-20`） |
| SFTP | 单文件、密码和 OpenSSH 私钥认证、加密凭据、本地临时文件续传、SOCKS5、TOFU host key 及 list/forget UI | 同 FTP 的 `ARC-19` / `ARC-26` / `FUN-20` |
| BitTorrent | magnet、远程和本地 `.torrent`、多文件选择、piece/peer/DHT/做种快照、SOCKS5、ratio/时间做种限制 | tracker 为配置快照而非实时健康；probe 每次新建 session（`ARC-28`）；限速不实时同步且不计入全局桶（`ARC-29`）；`.torrent` 抓取绕过 SSRF 守卫与代理策略（`SEC-03`） |
| HLS | 主变体选择、AES-128-CBC、EXT-X-MAP、byte range、并发分片、外部音轨/字幕、live 轮询与空闲收敛、ffmpeg MP4 remux | 不支持 SAMPLE-AES/DRM；staging 目录完成后不清理；live 轮询每 100ms 查库（`PERF-15`） |
| DASH | 静态/VOD first-pass：单 Period、`$Number$` SegmentTemplate / SegmentList / SegmentBase、分段下载、进度监控、ffmpeg MP4 remux；任务可暂停后续传 | 明确拒绝 dynamic/live、SegmentTimeline、多 Period、未实现的模板变量（如 `$Time$`）；分片数无上限（`ARC-22`）；探测未接逐任务代理（`FUN-20`） |
| WebDAV | WebDAV/WebDAVS 映射、Basic Auth、Depth-1 PROPFIND（支持对话框凭据与代理）、委托 HTTP 下载 | PROPFIND 无整体超时；目录探测绕过客户端缓存 |
| Metalink4 | 本地/远程 manifest、多文件选择、HTTP/HTTPS 镜像 failover、文件级进度、strongest-hash 与跨镜像续传校验 | 并行下载任一镜像失败会删除全部 part（`ARC-24`）；读循环无空闲超时且取消不及时（`ARC-25`） |

详细状态见 [协议可靠性矩阵](docs/protocol-reliability-matrix.md)。

### 队列、桌面与界面

- SQLite 持久化任务、文件、work unit、设置、事件、请求诊断、凭据、代理、校验和和 SFTP known hosts。
- 默认最多 2 个活动任务，可配置为 1 至 8；每主机连接上限可配置为 1 至 16。
- 下载时间窗、时间段限速和完成动作。时间窗会暂停和恢复服从计划的任务；关闭该功能会正确恢复由计划暂停的任务，且不会误恢复手动暂停的任务。
- 虚拟化任务列表、游标分页、搜索、排序、状态和多维筛选、多选、批量操作和任务重排序。
- 命令面板、任务详情、Chunks、Connections、Requests、Logs 和恢复动作。
- Attention Center 与 Queue Center 两个专用工作区，分别处理需要关注的任务和队列编排。
- 站点规则与分类规则编辑器，含冲突分析、导入导出和 URL 试运行。
- 设置页 11 个可折叠分区，含设置搜索、环境健康检查和数据备份/校验/恢复。
- 系统托盘、通知、开机启动、关闭到托盘、剪贴板监控和启动恢复。
- 浮动状态窗口，支持球形和条形模式。
- 8 种 OKLCH 强调色，系统、亮色和暗色主题。
- 英文和简体中文为稳定语言；繁体中文、日文、韩文、俄文和西班牙文为 Beta，需手动选择。

### 浏览器集成

- Native Messaging host 和本地 WebSocket bridge。
- Chrome、Edge、Firefox、Opera 及 Chromium 系开发包。
- HTTP/HTTPS 链接的工具栏、右键菜单和 popup 手动交接。
- request ID 去重、handoff 文件、单实例转发、实时任务快照和诊断。

浏览器能力按构建 profile 区分：

- `release` 和 `candidate`：最小权限手动交接，不包含 `downloads`、`cookies`、`webRequest` 或全站 host permissions。
- `dev`：只有显式设置 `VIBE_BROWSER_EXPERIMENTAL_CAPTURE=true` 才包含自动接管和 Cookie/header 转发。
- 站点规则中的 `ask` 不会弹出确认；界面文案为「不接管/不转发（不提示）」，实际表现为被动跳过。
- Header 过期后可从浏览器重新发送并恢复到原任务（`FUN-03`）。
- WebSocket bridge 目前没有速率限制，引导文件在 Windows 上仅设只读属性而非 ACL，见 `SEC-04`。

更多说明见 [浏览器集成](docs/browser-integration.md) 和 [Header 转发](docs/browser-header-forwarding.md)。

## 当前发布阻断

初版的 6 项阶段 A 阻断（`UX-01`、`FUN-01`、`FUN-02`、`ARC-01`、`ARC-02`、`ARC-03`）均已修复并有测试覆盖。以下是 2026-08-13 复审确认的新阻断：

| ID | 问题 |
| --- | --- |
| `ARC-19` | FTP/SFTP 取消时中止未落盘的 worker，续传会写出零字节空洞 |
| `ARC-20` | 多文件任务的文件行在事务外插入，冲突会留下文件列表残缺的任务 |
| `ARC-21` | 写事务普遍使用 DEFERRED，读后写路径在 WAL 下会命中 `SQLITE_BUSY_SNAPSHOT` |
| `ARC-22` | DASH 分片数完全由清单决定且无上限，恶意 MPD 可耗尽内存 |
| `SEC-01` | Tauri `fs` 能力被授予 `{"path": "**"}` 全盘读写 |
| `SEC-02` | 备份恢复不校验恢复内容，构造的 `.vibe-backup` 可让应用向任意路径写文件 |

完整证据、验收条件和修复顺序见 [项目改进审计](docs/project-improvement-audit.md)。在这些问题关闭前，不应发布稳定版本。

## 尚未实现或未完成验收

- 云盘解析、完整视频嗅探、云账号同步和插件协议。
- Safari WebExtension wrapper。
- 浏览器商店正式 ID、签名、审核和 capture 权限最终方案。
- 正式操作系统代码签名和 notarization。
- GUI 端到端测试、浏览器扩展行为测试和真实安装包自动化。
- 生产规模的 10k、50k 和更大任务库性能基线（本机 harness 已覆盖到 50k）。
- 测试覆盖率度量、macOS 的 Rust CI 与依赖更新自动化，见 `ENG-01` 和 `ENG-04`。

## 未签名安装包

当前 Release workflow 配置了 Tauri updater 签名，但没有 Apple Developer ID 或 Windows Authenticode 代码签名。

- macOS 可能被 Gatekeeper 拦截，需要在 Finder 中右键选择“打开”，或在“系统设置 -> 隐私与安全性”中允许。
- Windows 可能显示 SmartScreen，需要选择“更多信息 -> 仍要运行”。
- Linux 安装包同样未签名，建议校验 GitHub Release 资产的 SHA-256。

在完成三平台 candidate 升级演练前，不应把 updater 描述为已完成生产验证。详见 [发布指南](docs/RELEASE.md)。

## 本地开发

### 环境要求

- Rust stable。
- Node.js 20+，CI 使用 Node.js 22。
- pnpm 10+。
- Windows 需要 WebView2；macOS 需要 Xcode Command Line Tools。
- Linux 需要 WebKitGTK 4.1、AppIndicator、librsvg 和 patchelf。
- HLS/DASH MP4 输出需要 PATH、`VIBE_FFMPEG_PATH` 或应用设置中可用的 ffmpeg。

Ubuntu/Debian 依赖：

```bash
sudo apt update
sudo apt install -y libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev patchelf
```

安装并运行桌面开发版：

```bash
pnpm install
pnpm tauri dev
```

只运行前端浏览器预览：

```bash
pnpm dev
```

浏览器预览使用 mock 数据，不执行真实 Tauri 下载。

## 浏览器扩展开发包

普通开发包：

```bash
pnpm build:extensions
```

启用实验性自动接管和 Header 转发：

```bash
VIBE_BROWSER_EXPERIMENTAL_CAPTURE=true pnpm build:extensions
```

Windows PowerShell：

```powershell
$env:VIBE_BROWSER_EXPERIMENTAL_CAPTURE = "true"
pnpm build:extensions
```

输出位于 `browser/dist/`。正式 release profile 不允许启用实验性 capture。

## 开发命令

| 命令 | 用途 |
| --- | --- |
| `pnpm dev` | 启动 Vite 浏览器预览 |
| `pnpm tauri dev` | 启动桌面开发版 |
| `pnpm dev:tauri` | 带详细 Rust 日志启动桌面应用 |
| `pnpm typecheck` | TypeScript 类型检查 |
| `pnpm lint` | Biome lint 和格式检查，不包含类型检查 |
| `pnpm check:i18n` | 检查 7 个 locale 的 key、插值占位符和未翻译英文 |
| `pnpm check` | typecheck、lint 和 i18n 组合检查 |
| `pnpm test:frontend` | 运行 Vitest |
| `pnpm test:rust` | 运行 Rust 单元和集成测试 |
| `pnpm build` | TypeScript 编译和前端生产构建 |
| `pnpm specta` | 从 Rust 生成 Specta bindings |
| `pnpm check:bindings` | 检查生成 bindings 是否漂移 |
| `pnpm build:extensions` | 构建浏览器扩展 |
| `pnpm verify:extensions` | 构建并校验扩展 manifest |
| `pnpm verify:protocol-matrix` | 校验协议矩阵结构 |
| `pnpm test:release-tools` | 运行发布脚本测试 |

推荐完整检查：

```bash
pnpm check
pnpm test:frontend
pnpm build
pnpm check:bindings
pnpm verify:extensions
pnpm verify:protocol-matrix
pnpm test:release-tools
cargo test --manifest-path src-tauri/Cargo.toml -j 1
cargo check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
```

## 项目结构

```text
src/                         React 前端、状态管理、i18n、Tauri adapter
src-tauri/src/               Rust 后端、命令、下载引擎、数据库、事件和平台代码
src-tauri/src/db/migrations/ SQLite migration
src-tauri/src/bin/           主应用辅助二进制和 vibe-native-host
browser/extension-core/      WebExtension 共享源码和 manifest 模板
scripts/                     构建、版本、扩展和发布脚本
docs/                        当前说明、运行手册、路线图和历史审计
.github/workflows/           CI、Tauri Build 和 Release workflows
```

## 技术栈

- 桌面：Tauri 2。
- 前端：React 19、TypeScript、Zustand、Tailwind CSS 4、Radix、Motion、i18next。
- 后端：Rust、Tokio、reqwest、sqlx SQLite、tracing。
- 协议：suppaftp、russh/russh-sftp、librqbit、hls_m3u8、quick-xml、aes/cbc。
- 安全：ChaCha20-Poly1305、OS key store、SSRF 和 header allowlist。
- 类型：Specta 和 tauri-specta。
- 发布：GitHub Actions、Tauri updater 和 WebExtension packages。

## 文档

- [PRODUCT.md](PRODUCT.md)：产品定位和体验原则。
- [DESIGN.md](DESIGN.md)：设计系统和 UI 约束。
- [AGENTS.md](AGENTS.md)：代码代理必须遵守的当前项目规则。
- [docs/project-improvement-audit.md](docs/project-improvement-audit.md)：当前风险、优先级、验收和修复顺序。
- [docs/ROADMAP.md](docs/ROADMAP.md)：前向计划，不作为已实现能力清单。
- [docs/protocol-reliability-matrix.md](docs/protocol-reliability-matrix.md)：非 HTTP 协议自动化证据。
- [docs/performance-baseline.md](docs/performance-baseline.md)：性能测量方法和待填基线。
- [docs/browser-integration.md](docs/browser-integration.md)：浏览器集成和构建 profile。
- [docs/RELEASE.md](docs/RELEASE.md)：发布、签名和 updater 流程。

## License

Vibe Downloader 使用 GNU General Public License v3.0 only（`GPL-3.0-only`）。商业授权可由版权持有人另行提供。

详见 [LICENSE](LICENSE) 和 [CONTRIBUTING.md](CONTRIBUTING.md)。
