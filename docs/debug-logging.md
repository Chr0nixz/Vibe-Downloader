# Debug Logging

Vibe Downloader 的诊断日志覆盖桌面应用、Rust native host、前端 WebView 和浏览器扩展。

## 日志位置

### 主应用：`vibe.log`

| 平台 | 路径 |
| --- | --- |
| Windows | `%LOCALAPPDATA%\com.vibe.downloader\logs\vibe.log` |
| macOS | `~/Library/Logs/com.vibe.downloader/vibe.log` |
| Linux | `~/.local/share/com.vibe.downloader/logs/vibe.log` |

Release build 没有控制台时仍会写入该日志。Debug build 还会输出到 stdout。

### Native Messaging host：`native-host*`

Native host 使用单独的 rolling file，和主应用日志在同一目录。

| 平台 | 路径 |
| --- | --- |
| Windows | `%LOCALAPPDATA%\com.vibe.downloader\logs\native-host*` |
| macOS | `~/Library/Logs/com.vibe.downloader/native-host*` |
| Linux | `~/.local/share/com.vibe.downloader/logs/native-host*` |

Native host 不会把诊断日志写到 stdout，因为 stdout 保留给 Native Messaging 协议响应。调试信息写入文件和 stderr。

## 日志级别

| Level | 用途 |
| --- | --- |
| `error` | 持久失败，例如下载失败、数据库写入失败、handoff 被拒绝 |
| `warn` | 可恢复问题，例如事件发送失败、刷新失败、网络重试 |
| `info` | 关键里程碑，例如任务创建/完成、handoff 接收、调度启动 |
| `debug` | 开发细节，例如 HTTP probe、命令调用、调度槽位 |
| `trace` | 高频细节，默认不建议打开 |

## RUST_LOG

可以通过 `RUST_LOG` 调整 Rust 日志过滤：

```powershell
$env:RUST_LOG="vibe_downloader=debug,tauri=warn,sqlx=debug"
pnpm tauri dev
```

macOS/Linux：

```bash
RUST_LOG=vibe_downloader=debug,tauri=warn,sqlx=debug pnpm tauri dev
```

也可以使用预设脚本：

```bash
pnpm dev:tauri
```

常用预设：

- `vibe_downloader=debug`：后端详细日志。
- `vibe_downloader=info`：release 默认关注级别。
- `vibe_downloader=debug,sqlx=debug`：包含 SQL 查询。

## 关联 ID

- 下载问题：搜索 `task_id=...`。
- 浏览器交接问题：搜索 `request_id=...`，并同时检查扩展 Service Worker console、`native-host*` 和 `vibe.log`。

URL 写入日志前会脱敏，query string 和内嵌凭据不会保留。

## 前端日志

React 前端使用 `[vibe:namespace]` 前缀：

- `info`、`warn`、`error` 会通过 `tauri-plugin-log` 写入 `vibe.log`。
- `debug` 只在 dev build 输出。
- 全局未处理错误和未处理 Promise rejection 使用 `[vibe:global]`。

浏览器预览模式不会写入 Tauri 日志文件，只在浏览器 console 中可见。

## 浏览器扩展日志

扩展日志使用 `[vibe-ext:namespace]` 前缀，并显示在浏览器扩展 Service Worker console 中。

常见入口：

1. 打开 `chrome://extensions` 或目标浏览器的扩展管理页面。
2. 找到 Vibe Downloader。
3. 点击 service worker / background page 的 inspect 入口。

扩展日志不会自动合并到 `vibe.log`，因为扩展运行在独立浏览器进程中。

## Bug report 建议

提交问题时请尽量附带：

1. 主应用日志 `vibe.log`。
2. 浏览器集成相关问题的 `native-host*`。
3. 扩展 Service Worker console 截图或日志。
4. 相关任务的 `task_id` 或 handoff 的 `request_id`。

分享日志前请自行打码本地路径、URL 或其他敏感信息。
