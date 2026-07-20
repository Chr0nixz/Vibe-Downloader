# Browser Integration

最后更新：2026-07-19

Vibe Downloader 使用 Native Messaging 启动和引导浏览器集成，再通过本地 WebSocket 提供实时任务状态和设置同步。没有建立 WebSocket 时，扩展仍会回退到 Native Messaging handoff。

所有 profile 都只接受 HTTP/HTTPS handoff。FTP/FTPS、SFTP、WebDAV scheme、magnet 和本地 `file://` 资源必须通过桌面应用的手动新建或剪贴板确认流程进入。

浏览器构建 profile 的能力不同：

- `candidate` 和 `release` 是最小权限手动交接包，不包含自动接管、Cookie/header 转发或全站 host permissions。
- `dev` 默认同样是手动交接。只有显式设置 `VIBE_BROWSER_EXPERIMENTAL_CAPTURE=true` 才加入 `downloads`、`cookies`、`webRequest` 和 HTTP/HTTPS host permissions。
- 当前站点规则中的 `ask`（界面文案为「不捕获/不转发（不提示）」）不会弹出确认；行为是被动跳过自动接管或 header 转发（`FUN-14`）。

## 当前状态

已实现：

- 共享 WebExtension 源码：`browser/extension-core`。
- 扩展开发包构建：`pnpm build:extensions`。
- 独立 Rust native host：`vibe-native-host`。
- Settings 页面展示浏览器检测、manifest 路径、安装/卸载状态。
- Tauri 命令：获取集成状态、安装/卸载 manifest、创建 HTTP/HTTPS browser handoff task。
- 单实例转发：应用已运行时，第二次启动参数会转发给现有实例。
- SQLite `browser_messages` 表：request id 去重和错误诊断。
- 本地 WebSocket bridge：扩展可读取实时任务快照、任务进度和队列变化。
- dev-profile 实验性自动接管：在显式启用 capture 权限的开发包中接管浏览器原生 HTTP/HTTPS 下载，并在 Vibe 创建成功后取消浏览器下载。
- dev-profile 实验性 Cookie/header 转发：由设置控制，并只转发受控 allowlist header。
- 浏览器捕获设置：自动接管、header 转发、最小文件大小、扩展名和站点规则模型。

扩展当前入口：

- 右键 HTTP/HTTPS 链接：`Download with Vibe Downloader`。
- 右键选中文本：从文本中提取第一个 HTTP/HTTPS URL。
- popup：发送当前 tab URL、查看 bridge 状态、查看实时任务与最近 handoff；capture-enabled dev 包还可切换自动接管和 Cookie/header 转发。

尚未实现：

- 完整站点规则管理 UI。
- 自动大文件提示 UI。
- FTP/FTPS、magnet 和 `.torrent` 的浏览器自动接管。
- 生产 Safari Web Extension wrapper。
- Chrome Web Store / Edge Add-ons / Firefox AMO 的正式 ID、签名和审核流程。
- `ask` 模式的即时确认 UI。
- Header 过期后把浏览器重新发送的凭据绑定回原 recoverable task 的闭环恢复。

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

安装包通过 Tauri `externalBin` 将 host 与主程序一起交付。manifest 安装命令只会写入已验证存在的绝对路径；解包或安装验收可执行 `vibe-native-host --self-check`，检查 host 版本、协议版本和兄弟主程序路径。

传统 handoff 工作流程：

1. 浏览器通过 Native Messaging 向 host 发送 payload。
2. Host 校验 payload 版本、action、browser、URL scheme 和内嵌凭据。
3. Host 将 payload 写入 handoff JSON 文件。
4. Host 启动 `vibe-downloader --browser-handoff-file <path>`。
5. 如果应用已运行，Tauri single-instance 插件把参数转发给现有实例。
6. 应用读取 handoff 文件，创建任务，聚焦主窗口，成功后删除 handoff 文件。

实时 bridge 工作流程：

1. 扩展通过 Native Messaging 发送 `bootstrap`。
2. Host 启动或唤醒桌面应用。
3. 应用启动本地 WebSocket bridge，并写入临时 bootstrap 文件。
4. Host 返回 `wsUrl` 和一次性 token。
5. 扩展连接 WebSocket，订阅任务状态，并通过 bridge 创建下载任务或同步捕获设置。

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

正式发布前必须替换为商店扩展 ID，并区分开发/生产 manifest。正式商店包保持最小权限手动交接，除非未来 capture 权限通过独立审核。

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
  "suggestedFileName": "optional.zip",
  "source": "browser_capture",
  "browserDownloadId": 123,
  "totalBytes": 104857600,
  "mime": "application/zip",
  "forwardedHeaders": [
    { "name": "cookie", "value": "session=..." },
    { "name": "accept-language", "value": "zh-CN,en;q=0.9" }
  ]
}
```

安全规则：

- 只接受 `http` 和 `https` URL。
- FTP/FTPS、SFTP、WebDAV scheme、magnet 和本地 `file://` 不进入浏览器 handoff payload；HTTP/HTTPS `.torrent` URL 仍属于允许的 HTTP/HTTPS handoff，并由任务创建路由决定协议。
- 拒绝带内嵌用户名或密码的 URL。
- 扩展不能指定本地保存路径。
- Cookie/header 转发必须由设置开启，并经过后端 allowlist 过滤。
- 当前 allowlist 包括 `cookie`、`user-agent`、`referer`、`origin`、`accept`、`accept-language`、`dnt`、`cache-control`、`pragma`。
- `authorization`、`proxy-authorization`、`set-cookie`、`range`、`accept-encoding`、`host` 和 `connection` 会被明确拒绝。
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
5. 在 popup 中确认 bridge 为 connected，并能看到实时任务状态。
6. 对手动交接 profile，确认工具栏、右键菜单和 popup 可以创建任务，且 manifest 不包含 capture 权限。
7. 只有在 dev profile 显式启用 `VIBE_BROWSER_EXPERIMENTAL_CAPTURE=true` 时，才开启 Auto capture 并验证浏览器下载被暂停/取消、Vibe 中出现 queued/downloading task。
8. 确认 `browser_messages` 记录 request id、browser、url、status。

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
- 保持商店包为最小权限手动交接；若未来发布 capture 变体，单独完成权限、隐私和商店审核。
- 修复 `FUN-03` 的过期 Header 恢复路径后再扩展相关产品文案。
