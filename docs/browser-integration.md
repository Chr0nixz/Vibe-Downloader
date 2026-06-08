# Browser Integration

Vibe Downloader 当前使用 Native Messaging 作为浏览器到桌面应用的主要交接通道。localhost/WebSocket API 暂不实现，后续可用于扩展内实时任务面板或自动化集成。

## 当前状态

已实现：

- 共享 WebExtension 源码：`browser/extension-core`。
- 扩展开发包构建：`pnpm build:extensions`。
- 独立 Rust native host：`vibe-native-host`。
- Settings 页面展示浏览器检测、manifest 路径、安装/卸载状态。
- Tauri 命令：获取集成状态、安装/卸载 manifest、创建 browser handoff task。
- 单实例转发：应用已运行时，第二次启动参数会转发给现有实例。
- SQLite `browser_messages` 表：request id 去重和错误诊断。

扩展当前入口：

- 右键链接：`Download with Vibe Downloader`。
- 右键选中文本：从文本中提取第一个 HTTP/HTTPS URL。
- popup：发送当前 tab URL。

尚未实现：

- 自动接管浏览器原生下载。
- Cookie/header 转发。
- 站点规则和自动大文件提示。
- 扩展内实时任务状态面板。
- 生产 Safari Web Extension wrapper。
- Chrome Web Store / Edge Add-ons / Firefox AMO 的正式 ID、签名和审核流程。

## 支持浏览器

代码中定义的目标浏览器：

- Google Chrome
- Microsoft Edge
- Mozilla Firefox
- Safari
- Brave
- Opera
- Vivaldi
- Chromium

当前开发包：

- Chrome、Edge、Brave、Vivaldi、Chromium 复用 Chromium 包。
- Firefox 使用 Firefox 包和 `vibe-downloader@local` 开发 ID。
- Opera 使用 Opera 包。
- Safari 仅在 macOS 上显示为平台支持目标，生产 wrapper 尚未完成。

## 构建扩展开发包

```bash
pnpm build:extensions
```

输出：

- `browser/dist/chromium`
- `browser/dist/firefox`
- `browser/dist/opera`

建议验证：

```bash
pnpm build:extensions
pnpm typecheck
pnpm build
pnpm check:bindings
pnpm test:rust
cargo check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
```

## Native host

Native host 二进制名为 `vibe-native-host`。

工作流程：

1. 浏览器通过 Native Messaging 向 host 发送 payload。
2. Host 校验 payload 版本、action、browser、URL scheme 和内嵌凭据。
3. Host 将 payload 写入 handoff JSON 文件。
4. Host 启动 `vibe-downloader --browser-handoff-file <path>`。
5. 如果应用已运行，Tauri single-instance 插件把参数转发给现有实例。
6. 应用读取 handoff 文件，创建任务，聚焦主窗口，成功后删除 handoff 文件。

Native host 不向 stdout 写诊断日志，因为 stdout 属于 Native Messaging 协议。日志见 [debug-logging.md](debug-logging.md)。

开发环境变量：

- `VIBE_DOWNLOADER_APP_EXE`：桌面应用可执行文件绝对路径。
- `VIBE_DOWNLOADER_HANDOFF_DIR`：handoff JSON 文件写入目录。

## 安装 Native Messaging manifest

在应用内打开 Settings -> Browser integration，对目标浏览器执行 Install/Uninstall。

当前 manifest 行为：

- Windows：写入应用配置目录下的 manifest 文件，并写入 HKCU NativeMessagingHosts registry key。
- macOS/Linux：写入各浏览器约定的 NativeMessagingHosts 目录。
- Safari：macOS-only 目标，当前仍是生产 wrapper 之前的占位。

开发 ID：

- Chromium manifest 当前使用固定开发 ID：`abcdefghijklmnopabcdefghijklmnop`。
- Firefox manifest 当前使用：`vibe-downloader@local`。

正式发布前必须替换为商店扩展 ID，并区分开发/生产 manifest。

## Handoff payload

```json
{
  "version": 1,
  "requestId": "uuid",
  "browser": "chrome",
  "action": "download_url",
  "url": "https://example.com/file.zip",
  "pageUrl": "https://example.com/page",
  "referrer": "https://example.com/page",
  "userAgent": "optional",
  "suggestedFileName": "optional.zip"
}
```

安全规则：

- 只接受 `http` 和 `https` URL。
- 拒绝带内嵌用户名或密码的 URL。
- 扩展不能指定本地保存路径。
- Stage 5 基础实现不转发 Cookie 和敏感 header。
- URL 日志会去掉 query string 和凭据。

## 手动端到端验证

1. 运行：

   ```bash
   pnpm build:extensions
   pnpm tauri dev
   ```

2. 打开 Settings -> Browser integration，安装目标浏览器的 native host manifest。
3. 在浏览器加载开发扩展：
   - Chrome/Edge/Brave/Vivaldi/Chromium：加载 `browser/dist/chromium`。
   - Firefox：临时加载 `browser/dist/firefox`。
   - Opera：加载 `browser/dist/opera`。
4. 右键 HTTP/HTTPS 链接，选择 `Download with Vibe Downloader`。
5. 确认 Vibe 中出现新的 queued/downloading task。
6. 确认 `browser_messages` 记录 request id、browser、url、status。

## 排查清单

如果扩展提示 native host 失败，依次检查：

- 扩展 ID 是否和 native host manifest 中的 allowed origin/extension 一致。
- manifest path 是否存在。
- manifest 中的 host path 是否指向有效 `vibe-native-host`。
- 开发环境中 `VIBE_DOWNLOADER_APP_EXE` 是否指向桌面应用可执行文件。
- `native-host*` 是否有 request id、payload 校验或启动失败信息。
- `vibe.log` 是否有 handoff 文件读取或任务创建失败信息。

## 发布前待办

- 为 Chromium 系浏览器和 Firefox 分离 development ID 与 release ID。
- 建立扩展签名、商店审核和版本同步流程。
- 完成 Chrome/Edge/Firefox 的端到端验证矩阵。
- Safari 需要单独 macOS wrapper 和签名/审核流程。
- 在 Settings 中增加复制诊断信息入口。
