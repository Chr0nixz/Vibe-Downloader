# Browser Extension Permissions Review

最后更新：2026-07-19

本文档说明 Vibe Downloader 浏览器扩展申请的每个权限、用途、最小化原则和审核回复模板，供 Chrome Web Store / Edge Add-ons / Firefox AMO 审核团队参考，也作为发布前权限审计的内部清单。

## 权限清单

扩展 manifest 模板位于 [browser/extension-core/manifest.template.json](../browser/extension-core/manifest.template.json)，由 [scripts/build-browser-extensions.mjs](../scripts/build-browser-extensions.mjs) 根据 profile 和实验性 capture 开关生成各浏览器变体。

### 基础权限（所有 profile 始终包含）

| 权限 | 类型 | 用途 | 是否必需 |
| --- | --- | --- | --- |
| `nativeMessaging` | API 权限 | 通过 Native Messaging Host 与桌面应用通信，把浏览器中的 HTTP/HTTPS 链接交给 Vibe Downloader 创建下载任务。这是扩展的核心功能，没有它扩展无法与桌面应用通信。 | 必需 |
| `contextMenus` | API 权限 | 注册"Download with Vibe Downloader"右键菜单项，用户可以从链接或选中文本直接创建下载任务。 | 必需 |
| `activeTab` | API 权限 | 在用户主动点击扩展图标或右键菜单时，读取当前标签页的 URL 发送给桌面应用。不持续监听用户浏览。 | 必需 |
| `tabs` | API 权限 | 在用户点击扩展图标或右键菜单时，读取当前标签页的 URL 和标题。用于把页面 URL 发送给桌面应用创建下载任务。 | 必需 |
| `storage` | API 权限 | 持久化扩展本地设置（自动接管开关、Cookie/header 转发模式、最近 handoff 历史）。不存储敏感凭据；敏感凭据由桌面应用的 SQLite 加密存储。 | 必需 |

### 实验性 Capture 权限（仅在 `VIBE_BROWSER_EXPERIMENTAL_CAPTURE=true` 时包含）

这些权限在 release profile 下默认**不开启**，只在 dev profile 或显式设置环境变量时启用。Chrome Web Store 审核时如果未启用实验性 capture，权限声明中不会包含这些项。

| 权限 | 类型 | 用途 | 是否必需 |
| --- | --- | --- | --- |
| `downloads` | API 权限 | 自动接管浏览器原生下载，并在桌面应用创建任务成功后取消浏览器下载对话框。仅当用户在扩展设置中开启"自动接管"时生效。 | 可选（实验性） |
| `cookies` | API 权限 | 当用户在扩展设置中开启 Cookie/header 转发时，读取与目标 URL 关联的 Cookie 值并转发给桌面应用，使下载任务能通过身份认证。Cookie 只在用户主动发起下载时读取，不持续监听。 | 可选（实验性） |
| `webRequest` | API 权限 | 当用户在扩展设置中开启 Cookie/header 转发时，从即将发出的请求中读取 allowlist header（如 User-Agent、Referer、Accept-Language）转发给桌面应用。扩展**不修改、不阻止**任何请求，只读取。 | 可选（实验性） |
| `host_permissions: http://*/*` 和 `https://*/*` | 主机权限 | 实验性 capture 模式下，需要在任意 HTTP/HTTPS 页面读取 Cookie 和 header 转发给桌面应用。不会主动访问页面内容。 | 可选（实验性） |

### Manifest V3 兼容性说明

所有权限均符合 Manifest V3 规范：

- `webRequest` 在 MV3 下只能使用观察者模式（`webRequest.onBeforeRequest` 的 `"blocking"` 不再允许用于内容拦截，但读取 header 的 listener 仍可用）。
- 扩展**不使用** `webRequestBlocking` 来阻止或修改请求。
- background 使用 service worker 而非持久化 background page。
- 不使用 `chrome.declarativeNetRequest` 动态规则。

## 安全边界

### 数据流向

扩展只做以下数据传递：

1. **用户主动发起**：用户点击扩展图标、右键菜单、或浏览器原生下载触发时，扩展才会读取当前标签页 URL 和（如启用）相关 Cookie/header。
2. **本地传递**：所有数据通过 Native Messaging（stdin/stdout）或本地 WebSocket（端口 48365，仅 `127.0.0.1`）传给桌面应用。
3. **不外传**：扩展**不会**把用户数据发送到任何远程服务器。所有数据只发往本机的 Vibe Downloader 进程。

### Header 转发的 Allowlist

即使启用了 Cookie/header 转发，扩展也只转发以下 allowlist 中的 header（定义于 [src-tauri/src/commands/browser.rs](../src-tauri/src/commands/browser.rs)）：

- `cookie`
- `user-agent`
- `referer`
- `origin`
- `accept`
- `accept-language`
- `dnt`
- `cache-control`
- `pragma`

**显式拒绝**转发 `Authorization` header，以避免 Bearer token 或 Basic Auth 凭据泄露。

### 持久化策略

- 扩展本地 `storage` 只保存用户设置和最近 handoff 历史（URL、时间戳、状态），不保存任何密码、Cookie 值或 header 内容。
- Cookie/header 值只通过 Native Messaging 或 WebSocket 临时传给桌面应用，桌面应用使用 ChaCha20-Poly1305 加密后存入 SQLite，24 小时后过期。
- 如果 OS key store 不可用，桌面应用不会持久化 header，只在内存中保留并记录结构化警告。

### 内网地址保护

浏览器 handoff 默认**拒绝**私有/环回/链路本地地址（`127.0.0.0/8`、`10.0.0.0/8`、`172.16.0.0/12`、`192.168.0.0/16`、`169.254.0.0/16`、`::1`、`fc00::/7`、`fe80::/10`）。需要下载内网资源时，用户必须在桌面应用设置中显式开启 `allow_intranet_handoff`，防止扩展被恶意页面用于 SSRF。

### URL 限制

- 浏览器 handoff 只接受 HTTP/HTTPS URL。
- URL 中不能包含嵌入式凭据（如 `https://user:pass@example.com`），handoff 边界会拒绝此类 URL。
- FTP/FTPS、SFTP、WebDAV、magnet、本地 `file://` URL 不通过浏览器 handoff 传递，只能通过桌面应用的手动新建或剪贴板监控流程创建。

## 权限审计清单（发布前）

每次发布前应执行以下审计：

### 1. Manifest 权限验证

```bash
# 构建 release profile 扩展
VIBE_BROWSER_PROFILE=release pnpm build:extensions

# 检查生成的 manifest.json 是否符合预期权限
# Chromium: 不应包含 downloads/cookies/webRequest/host_permissions（除非启用实验性 capture）
# Firefox: 检查 browser_specific_settings.gecko.id 是否为正式 ID
```

### 2. Extension ID 一致性

- 确认 GitHub Secrets 中 `VIBE_CHROME_EXTENSION_ID`、`VIBE_EDGE_EXTENSION_ID`、`VIBE_FIREFOX_EXTENSION_ID` 已设置且与商店注册一致。
- Rust build script、扩展构建和校验脚本必须解析为同一组三平台 ID；release 不存在 placeholder fallback，缺任一 ID 会在编译/构建前失败。
- **自动化校验**：candidate 从提交到仓库的 Chromium 测试公钥推导确定性 ID并与 Rust 常量比对；release 会校验三个正式 ID 的格式和完整性。Debug 默认 dev，非 Debug 默认 candidate，正式 workflow 必须显式指定 release。

### 3. Header Allowlist 一致性

- 比对 [src-tauri/src/commands/browser.rs](../src-tauri/src/commands/browser.rs) 中的 `FORWARDED_HEADER_ALLOWLIST` 与 [browser/extension-core/src/background.js](../browser/extension-core/src/background.js) 中的 `ALLOWED_HEADER_NAMES`。
- 确认 `Authorization` 始终被拒绝。
- 确认 allowlist 没有意外加入新 header。
- **自动化校验**：`pnpm verify:extensions`（等价于 `pnpm build:extensions && pnpm verify:manifest`）会读取 Rust 与 JS 源码并断言两侧 allowlist 集合相等，已在 CI 中接入。Rust 端的 `tests/browser_handoff.rs` 还会断言 `FORWARDED_HEADER_ALLOWLIST` 与本地固定集合一致、所有 entry 小写、`Authorization` 不在列表中，作为第二道防线。

### 4. Native Messaging Host 注册

- 确认 [src-tauri/src/bin/vibe-native-host.rs](../src-tauri/src/bin/vibe-native-host.rs) 的 manifest 中 `allowed_origins` 和 `allowed_extensions` 仅包含正式 extension ID。
- 确认 dev/candidate identity 不会泄露到 release 构建；release 只生成 Chrome、Edge、Firefox 三种商店包。

### 5. 隐私政策

- 使用 [browser-extension-privacy.md](browser-extension-privacy.md) 的隐私政策，明确说明 URL 只在本机传递、不进行远程收集，用户可随时卸载扩展和桌面应用清除数据。
- Chrome Web Store 要求隐私政策 URL；Firefox AMO 在申请敏感权限时要求隐私政策。
- Edge Add-ons 隐私政策要求与 Chrome 一致。

### 6. 权限 Justification（商店审核回复模板）

提交到 Chrome Web Store 时，对于敏感权限需要在提交流程中提供 justification。模板：

```
nativeMessaging:
  Required for the extension's core functionality. The extension uses
  Native Messaging to communicate with the Vibe Downloader desktop
  application (installed separately) to hand off HTTP/HTTPS download
  links. No data is sent to remote servers; all communication is local
  (stdio between the extension and the native host process).

contextMenus:
  Required to let users right-click a link or selected text and choose
  "Download with Vibe Downloader" to create a download task in the
  desktop application.

activeTab and tabs:
  Required to read the URL of the current tab when the user clicks the
  extension icon or the context menu item, so it can be sent to the
  desktop application. The extension does not monitor browsing history
  or persist tab data.

storage:
  Required to persist user preferences (auto-capture toggle, header
  forwarding mode) and recent handoff history (URL, timestamp, status).
  No credentials, cookies, or header values are stored in extension
  storage; sensitive data is encrypted and stored by the desktop
  application.

downloads (experimental, only in capture-enabled builds):
  Required to intercept the browser's native download dialog when the
  user has explicitly enabled "auto-capture" in extension settings. The
  extension cancels the browser download after the desktop application
  has created a corresponding task. No download content is read.

cookies (experimental, only in capture-enabled builds):
  Required to read cookies for the target URL when the user has
  explicitly enabled "Cookie/header forwarding" in extension settings.
  Cookies are forwarded to the desktop application via Native Messaging
  or local WebSocket (127.0.0.1:48365) and are not sent to any remote
  server. Cookies are not persisted in extension storage.

webRequest (experimental, only in capture-enabled builds):
  Required to read allowlisted request headers (User-Agent, Referer,
  Accept-Language, etc.) when the user has explicitly enabled
  "Cookie/header forwarding" in extension settings. The extension only
  reads headers; it does not modify or block any request. The
  Authorization header is explicitly never forwarded.

host_permissions (experimental, only in capture-enabled builds):
  Required only when Cookie/header forwarding is enabled. The extension
  needs to read cookies and headers for any HTTP/HTTPS URL the user
  initiates a download from. The extension does not access page
  content, DOM, or inject scripts.
```

## 各商店特殊要求

### Chrome Web Store

- 提交时选择"工具"类别。
- 使用 `single purpose` 描述：下载管理器，将浏览器下载交接给桌面应用。
- 敏感权限（`cookies`、`webRequest`、`host_permissions`）可能触发人工审核，需在 justification 中强调用户主动开启和数据不外传。
- Chrome Web Store 不允许 `webRequestBlocking` 用于 MV3，本扩展不使用。

### Edge Add-ons

- 与 Chrome 一致的权限要求。
- Edge 也接受 MV3 扩展。
- 提交流程类似 Chrome Web Store。

### Firefox AMO

- Firefox 使用 `browser_specific_settings.gecko.id` 标识扩展，需要邮箱格式 ID（如 `vibe-downloader@your-domain.com`）。
- Firefox 的 `webRequest` 在 MV3 下仍支持 blocking（与 Chrome 不同），但本扩展不使用 blocking。
- AMO 审核更严格地检查 `webRequest` 和 `host_permissions` 的用途，需准备详细的源码审查说明。
- Firefox 要求 `strict_min_version` ≥ 109.0（MV3 支持），已在 manifest 中设置。

## 权限变更流程

任何 manifest 权限变更必须：

1. 更新本文档的权限清单和用途说明。
2. 更新 [docs/browser-header-forwarding.md](browser-header-forwarding.md)（如果涉及 header 转发）。
3. 更新商店审核 justification 模板。
4. 增加或更新单元测试覆盖新权限的使用场景。
5. 在 release notes 中明确说明权限变更和原因（Chrome Web Store 要求用户重新确认权限变更）。

## 相关文档

- [browser-integration.md](browser-integration.md)：浏览器集成整体架构和当前状态。
- [browser-header-forwarding.md](browser-header-forwarding.md)：Cookie/header 转发的实现细节。
- [error-codes.md](error-codes.md)：错误码定义，包括浏览器 handoff 相关错误。
- [RELEASE.md](RELEASE.md)：发布流程和签名状态。
