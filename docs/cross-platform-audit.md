# 跨平台能力审计

> 历史快照：本文是 `0.2.0` 阶段的跨平台静态审计。当前发布阻断和验收顺序以 [project-improvement-audit.md](project-improvement-audit.md) 为准，实际发布操作以 [RELEASE.md](RELEASE.md) 为准。

最后更新：2026-07-05

本文基于当前仓库的静态代码与配置核查，聚焦 Vibe Downloader `0.2.0` 在 Windows、macOS、Linux 三个平台上的运行、打包、系统集成、浏览器集成、协议引擎与发布验证能力。本文不替代 [project-improvement-audit.md](project-improvement-audit.md) 的全局风险排序，也不替代 [RELEASE.md](RELEASE.md) 的发布操作说明；它只回答一个问题：当前项目离“可信的三平台桌面下载器”还有多远。

审计方式：只读核查 `src-tauri/`、`src/`、`browser/`、`.github/workflows/`、`scripts/` 与现有文档；未运行完整测试或三平台安装包实机验证。因此，本文对打包产物、Native Messaging host 随包可用性、OS 签名与 updater 端到端升级链路均使用“需要实测闭环”的审计口径。

## 总体结论

项目已经具备真实的三平台桌面应用基础：Tauri 2 配置启用 bundle targets `all`，CI 有 Windows/Linux Rust 校验和三平台 Tauri build matrix，release workflow 覆盖 macOS arm64/x64、Linux x64、Windows x64；Rust 后端、前端平台适配、系统命令、文件图标、浏览器 Native Messaging manifest 路径都存在平台分支。

但它还不能被描述为“发布完成的跨平台产品”。主要差距不在能否启动，而在发布信任链和跨平台外部集成闭环：

- OS 代码签名尚未配置；Tauri updater 签名已经配置，但不同于 macOS Developer ID / Windows Authenticode。
- 浏览器扩展仍缺正式 store ID、签名包、Safari wrapper 和权限文案闭环。
- Native Messaging host 二进制存在，但当前可见的 Tauri 配置没有声明 `externalBin`、sidecar 或等价资源打包规则；release 安装后 host 是否位于 manifest 期望路径，必须用安装包实测确认。
- HTTP/HTTPS 是最成熟的协议路径；FTP/FTPS、SFTP、BT、HLS、DASH、WebDAV、Metalink 已接入，但可靠性、诊断、恢复体验和跨平台长测仍低于 HTTP 主路径。
- Linux 的 keyring/桌面环境、trash、系统电源命令、文件图标、Native Messaging 路径在不同发行版和桌面会有更多边界，需要专项验收矩阵。

## 平台能力矩阵

| 维度 | Windows | macOS | Linux | 结论 |
| --- | --- | --- | --- | --- |
| 桌面壳 | Tauri 2 + WebView2；自定义标题栏分支 | Tauri 2 + WebKit；traffic-light inset 和私有 API | Tauri 2 + WebKitGTK；系统装饰分支 | 三平台基础成立 |
| 打包 | release matrix 有 x64 Windows | release matrix 有 arm64/x64 macOS | release matrix 有 x64 Linux | 构建矩阵成立，仍需安装包实测 |
| OS 签名 | Authenticode secrets 注释保留，未启用 | Apple signing secrets 注释保留，未启用 | 无主流 OS 签名链 | 发布信任链未闭环 |
| Updater | 配置 endpoint/pubkey/passive install | 配置 endpoint/pubkey | 配置 endpoint/pubkey | Tauri updater 配置存在，需端到端演练 |
| 系统命令 | shutdown/sleep/hibernate/lock 分支 | shutdown/sleep/hibernate/lock 分支 | shutdown/sleep/hibernate/lock 分支 | 已实现，但 Linux 分布差异需实测 |
| 文件图标 | Windows Shell API + image | AppKit NSWorkspace | freedesktop icon lookup | 三平台实现存在 |
| 删除文件 | `trash` crate / 可回收站 | `trash` crate / 废纸篓 | `trash` crate / 桌面 trash | 失败时不会静默永久删除，较安全 |
| 凭据保护 | keyring + ChaCha20-Poly1305 | keyring + ChaCha20-Poly1305 | keyring + ChaCha20-Poly1305 | Linux headless/minimal desktop 是主要风险 |
| 浏览器集成 | Chrome/Edge/Firefox/Brave/Opera/Vivaldi/Chromium manifest + registry | 主流浏览器 manifest 路径；Safari 仅占位 | 主流浏览器 manifest 路径 | 主流路径有代码，Safari 未完成 |

## 运行时与系统集成

### 已具备的能力

- `src-tauri/src/platform/mod.rs` 对日志目录、窗口装饰、文件打开、关机、睡眠、休眠、锁屏、命令执行提供 Windows/macOS/Linux 分支。
- `src-tauri/src/commands/system.rs` 对磁盘空间查询和系统文件图标提取提供平台实现。
- `src/lib/platform.ts` 对 Windows 自定义标题栏、macOS traffic-light inset、Linux 系统装饰做了前端侧适配。
- `src-tauri/Cargo.toml` 使用 target-specific dependencies：Windows 拉取 `windows`/`image`，macOS 拉取 `objc2-*`，Linux 拉取 `freedesktop`/`image`，依赖边界比较清晰。

### 主要风险

- Linux 不是单一平台。`loginctl`、`xdg-open`、freedesktop icon、keyring、trash、WebKitGTK 依赖在 GNOME/KDE/XFCE、Wayland/X11、服务器最小安装环境下表现可能不同，需要至少 Ubuntu GNOME、KDE/Wayland、无 keyring 或 headless-like 环境的负面测试。
- macOS 使用 `macos-private-api` 与透明标题栏相关能力，打包和 notarization 前要重点验证窗口行为、权限提示和 Gatekeeper 体验。
- Windows 自定义标题栏与 WebView2 是成熟路径，但 SmartScreen 未签名提示会影响首次安装信任。

## 打包与发布

### 已具备的能力

- `src-tauri/tauri.conf.json` 启用 bundle，`targets: "all"`，启用 updater artifacts，并把 `browser/extension-core` 作为资源打入应用资源目录。
- `.github/workflows/tauri-build.yml` 在 Windows/macOS/Linux 三个平台运行 `pnpm tauri build --config src-tauri/tauri.ci.conf.json`。
- `.github/workflows/release.yml` 覆盖 macOS arm64/x64、Linux x64、Windows x64，并启用 `includeUpdaterJson: true`。
- CI 已有 `pnpm typecheck`、`pnpm lint`、`pnpm test:frontend`、`pnpm check:i18n`、版本一致性检查、`pnpm build`、扩展构建、`pnpm verify:manifest`、Rust `cargo fmt`、`cargo deny`、`cargo check`、`cargo clippy`、`pnpm test:rust`、`pnpm check:bindings`。

### 发布缺口

- OS 代码签名未启用。release workflow 中 macOS 和 Windows 签名 secrets 仍是注释示例，因此公开发布时仍会遇到 Gatekeeper/SmartScreen 信任问题。
- Release workflow 的 Linux 构建依赖未安装 `ffmpeg`。这不阻塞构建，因为 HLS/DASH 运行时依赖用户系统 ffmpeg；但 release smoke test 必须覆盖“未配置 ffmpeg”和“配置 ffmpeg 后可下载”的体验。
- 需要用真实 tag 演练 updater：旧版本安装、检测更新、下载、签名校验、安装、重启、版本确认、失败回滚提示。

## 浏览器集成

### 已具备的能力

- `src-tauri/src/commands/browser.rs` 实现浏览器集成安装/卸载、diagnostics、extension package 生成、manifest 模板处理、Native Messaging manifest 路径和 Windows registry 写入。
- `src-tauri/src/bin/vibe-native-host.rs` 存在 Native Messaging host，可读取浏览器消息并转发到桌面应用。
- 浏览器 handoff 安全边界较清楚：handoff 仅 HTTP/HTTPS，拒绝 embedded credentials，Cookie/header forwarding 走 allowlist，SSRF guard 覆盖 handoff 与 HTTP 引擎防线。
- `scripts/verify-extension-manifest.mjs` 已接入 CI/release，能检查权限和 Rust/JS 转发 header allowlist 一致性。

### 主要风险

- 当前可见配置没有发现 `vibe-native-host(.exe)` 的 Tauri `externalBin`/sidecar/资源打包声明。Cargo 有 `src-tauri/src/bin/vibe-native-host.rs`，但这不等价于安装包一定随带该二进制并位于 `native_host_path()` 期望的兄弟路径。此项是浏览器集成最关键的发布阻断风险。
- Safari 只在路径和枚举上有占位/限制，没有 Safari WebExtension wrapper、签名和安装链路。应明确标为“不支持”或从当前发布范围移出。
- Chrome/Edge/Firefox store ID 与签名包仍未闭环。开发包可用不等于商店可发布。

## 协议跨平台成熟度

| 协议 | 当前能力 | 跨平台成熟度 | 风险 |
| --- | --- | --- | --- |
| HTTP/HTTPS | HEAD + Range GET fallback、单流、未知大小、Range 分段、自动加速、断点校验、诊断、SSRF 防护 | 高 | 仍需大规模长测和代理矩阵 |
| FTP/FTPS | 动态并行分段、SOCKS5、加密凭据、目录探测 | 中 | FTPS/代理/断线恢复跨平台实测不足 |
| SFTP | 密码凭据、TOFU host key、SOCKS5、目录探测、分段/动态分裂路径 | 中 | keyring、host key 迁移、服务器差异需更多长测 |
| BitTorrent | magnet、HTTP/HTTPS `.torrent`、本地 torrent、多文件选择、peer/tracker/DHT 快照、SOCKS5、做种配置 | 中 | NAT、磁盘压力、长时间做种、暂停恢复仍需压测 |
| HLS | master variant、AES-128-CBC、EXT-X-MAP、byte range、并发分片、live polling、ffmpeg remux | 中偏低 | 本地 ffmpeg 解析链未复用共享 DB setting；加密分片仍有内存峰值风险 |
| DASH | 静态 MPD、ffmpeg 下载/remux、进度监控；拒绝 dynamic/live | 中 | live/dynamic 不支持；依赖 ffmpeg 外部环境 |
| WebDAV/WebDAVS | Basic Auth、PROPFIND、映射到 HTTP 引擎 | 中 | 服务器兼容性和认证变体有限 |
| Metalink | Metalink4、多文件、HTTP/HTTPS mirror failover、并行 range、checksum | 中 | 镜像失败编排和跨平台文件路径边界需继续实测 |

特别注意：`src-tauri/src/download/ffmpeg.rs` 已建立共享 ffmpeg 解析链，顺序为 `VIBE_FFMPEG_PATH`、SQLite `ffmpeg_path` setting、系统 `PATH`。DASH 已使用该共享链；HLS 当前仍保留本地 `ffmpeg_path()`，只检查 `VIBE_FFMPEG_PATH` 和 `PATH`。这会导致 Settings 中配置的 ffmpeg 路径可能对 DASH 生效、对 HLS 不生效，是应优先修复的一致性问题。

## 安全与数据

### 已具备的能力

- 凭据存储使用 ChaCha20-Poly1305，覆盖 FTP/FTPS、SFTP、WebDAV、代理密码和转发 header；旧 plaintext URL credential 有迁移逻辑。
- 文件名清洗采用 Windows-strict 保守策略，能避开保留名、路径穿越和控制字符，利于三平台一致性。
- HTTP/浏览器 handoff 有 SSRF 防护，包含私有/保留地址、DNS 解析和 redirect 相关防线。
- 删除到回收站失败时返回错误，不会在用户期待 trash 的情况下静默永久删除。

### 主要风险

- Linux keyring 在无桌面密钥环、锁定会话、容器、SSH/headless 环境下可能不可用。需要把“凭据加密不可用”的错误恢复路径作为 Linux 验收项。
- OS 级签名缺失不是代码安全漏洞，但会显著影响用户信任和安装转化。
- 浏览器权限文案和商店审核材料未闭环，安全能力无法转化为发布可信度。

## 测试与 CI 覆盖

当前 CI 基础已经比较完整，尤其是：

- 前端：typecheck、Biome、Vitest、i18n completeness、版本一致性、production build、extension build、manifest verification。
- Rust：Ubuntu/Windows matrix，安装 ffmpeg，fmt、cargo-deny、check、clippy、test、Specta bindings drift check。
- Tauri build：Windows/macOS/Linux 三平台构建。
- Release：四目标产物矩阵和扩展产物上传。

仍建议补齐的验证：

- macOS Rust test/clippy 目前不在 CI rust matrix 中，macOS 专属代码主要靠 Tauri build 编译兜底。建议至少周期性运行 macOS `cargo test`。
- Release 产物安装级 smoke test：安装后启动、创建 HTTP 下载、配置 ffmpeg 后创建 HLS/DASH、安装浏览器 manifest、浏览器 handoff、卸载 manifest、updater 演练。
- Native Messaging host 打包检查：对每个平台 release artifact 解包或安装后断言 host 二进制存在、可执行、路径与 manifest 一致。
- Linux 桌面矩阵：GNOME/KDE、Wayland/X11、缺失 keyring、trash 不可用、`xdg-open` 不可用。

## 发布阻断项

### P0：公开发布前必须处理

1. 确认并修复 `vibe-native-host(.exe)` 随包分发方式。若使用 Tauri sidecar/externalBin，需要在配置和 release 验证中明确；若使用资源复制，需要确保 `native_host_path()` 与安装路径一致。
2. 完成或明确放弃本版本 OS 代码签名。若短期不签名，README/RELEASE 必须明确 unsigned 安装体验和校验方法。
3. 用真实 tag 做 updater 端到端演练，并记录结果。
4. 浏览器扩展发布策略定稿：Chrome/Edge/Firefox ID、签名、权限文案、store review copy；Safari 明确不支持或建立 wrapper 计划。

### P1：跨平台可靠性优先修复

1. 让 HLS 使用 `download::ffmpeg::{ensure_ffmpeg_available, ffmpeg_path}` 共享解析链，确保 Settings 中的 `ffmpeg_path` 对 HLS/DASH 一致生效。
2. 建立三平台安装包 smoke test checklist，至少覆盖启动、下载、暂停/恢复、删除、打开文件/目录、系统通知、托盘、浏览器 handoff。
3. 为 Linux keyring/trash/system-command 失败路径补充可观测错误和恢复建议。
4. 对 FTP/FTPS、SFTP、BT、HLS、DASH、WebDAV、Metalink 做协议级长测，重点是断网、代理、凭据失败、磁盘满、恢复动作。

### P2：发布质量增强

1. 增加 macOS Rust test/clippy 或周期性 scheduled workflow。
2. 为 release artifact 增加自动化资产检查：产物命名、updater JSON、Native Messaging host、扩展 zip/xpi、版本号一致性。
3. 形成 `docs/release-smoke-test.md` 或在 `docs/RELEASE.md` 中增加跨平台验收矩阵。
4. 给浏览器集成 diagnostics 增加“host binary packaged / manifest points to existing host / host executable responds to bootstrap”的一键检查。

## 最终判断

Vibe Downloader 当前的跨平台能力可以概括为：三平台应用骨架和核心下载能力已经成立，CI/构建矩阵也已经不是空壳；但公开发布所需的“安装后一定可用、浏览器一定连得上、用户能信任安装包、升级链路可恢复”还没有闭环。

如果只面向开发者预览，它已经具备较强的跨平台可试用价值；如果面向普通用户公开发布，应先把 Native Messaging host 打包、OS 签名/unsigned 策略、updater 演练、HLS ffmpeg 配置一致性和三平台安装 smoke test 做完。
