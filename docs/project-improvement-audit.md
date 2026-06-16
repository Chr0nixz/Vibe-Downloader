# 项目改进审计

最后更新：2026-06-17

本审计基于当前仓库代码、配置、测试和文档状态。它不替代 [ROADMAP.md](ROADMAP.md)，而是按风险和用户影响列出仍需处理的问题。

## 总体结论

Vibe Downloader 已经远超最早 HTTP MVP：HTTP/HTTPS 下载核心（含自动加速分段）、SQLite 持久化（11 个迁移）、FTP/FTPS 动态并行、SFTP (TOFU)、BitTorrent (运行时快照)、HLS (AES-128-CBC)、DASH (ffmpeg)、WebDAV (PROPFIND)、Metalink (镜像故障转移 + 校验和)、加密凭据存储、逐任务代理、定时调度、浮动状态窗口、8 色主题、设置搜索、store 分解、虚拟无限滚动、浏览器 Native Messaging、WebSocket 实时桥、剪贴板监控、批量导入、命令面板和 CI 验证都已经落地。

当前主要风险集中在四类：

1. 发布前信任链路：README、路线图、审计和发布说明必须持续与代码一致。
2. 非 HTTP 协议成熟度：FTP/FTPS、SFTP、BitTorrent、HLS、DASH、WebDAV 和 Metalink 已可用，但可靠性、诊断和恢复能力仍低于 HTTP/HTTPS。HLS 和 DASH 依赖外部 ffmpeg。
3. 发布级安全和分发：浏览器商店身份、扩展签名、Safari wrapper、权限文案、OS 代码签名和 updater 端到端演练仍未完成。
4. 规模化性能：任务列表已 store 分解 + 虚拟化 + 游标分页，详情诊断已节流，但仍需要生产规模数据库和多任务压测。

## 已确认优势

- HTTP probe 支持 HEAD 与 Range GET fallback，并识别文件名、大小、content type、Range 能力和来源 host。
- HTTP 下载引擎支持未知大小单连接、Range 分段、自动加速（动态分裂，最多 8 段）、分段 retry、断点续传校验、checkpoint 进度持久化和全局 token bucket 限速。
- FTP/FTPS 支持动态并行分段（最多 4 连接）、SOCKS5 代理、加密凭据存储和目录探测。
- SFTP 支持密码认证、加密凭据、本地临时文件断点续传、目录探测、SOCKS5 代理和 TOFU 主机密钥验证。
- BitTorrent 支持 magnet、HTTP/HTTPS `.torrent` URL 和本地 `file://*.torrent`，含多文件选择、运行时快照、SOCKS5 代理、可配置做种和全局限速。
- HLS/m3u8 支持主播放列表变体选择、AES-128-CBC 解密、并发分段、直播轮询和 ffmpeg MP4 封装。
- DASH/MPD 支持清单解析（拒绝 live）、ffmpeg 下载和 MP4 封装。
- WebDAV/WebDAVS 支持 Basic Auth、PROPFIND 目录探测和 HTTP 引擎委托。
- Metalink4 支持多文件选择、HTTP/HTTPS 镜像故障转移和 MD5/SHA-1/SHA-256/SHA-512 校验和验证。
- 加密凭据存储使用 ChaCha20-Poly1305，覆盖 FTP/FTPS、SFTP 和 WebDAV，启动时自动迁移旧明文。
- 逐任务代理覆盖（继承/关闭/自定义），按协议验证兼容性。
- 定时下载窗口、定时限速窗口、完成动作（可取消退出 + 确认关机）。
- UI 具备 store 分解（data/ui/speed-history）、虚拟无限滚动、游标分页、8 色 OKLCH 主题、浮动状态窗口（球/条形）、可折叠侧边栏（三档响应式）、设置页 7 分区 + 搜索、命令面板、详情面板、Chunks/Connections/Requests/Logs、toast、删除确认和恢复动作。
- 浏览器集成已具备 Native Messaging、实时桥、manifest 安装诊断、下载接管、显式 Cookie/header 转发和 request id 去重。
- CI 覆盖前端 typecheck/build、Rust check/clippy/test、Specta 绑定漂移和三平台 Tauri build。

## P0：发布前必须处理

### 1. 发布链路端到端演练

配置已经存在，但正式发布前仍需要实际验证。

- 用测试 tag 触发 Release workflow。
- 确认 `latest.json`、`.sig`、安装包和版本号一致。
- 验证打包应用能检查更新、安装并 relaunch。
- 在 Windows/macOS 未配置代码签名前，发布说明明确 unsigned 风险。

### 2. 浏览器扩展发布身份

开发包和本地集成已经可用，但商店版仍缺少发布身份。

- 替换 Chrome/Edge/Firefox release placeholder ID。
- 完成正式扩展签名和权限文案。
- 建立 Chrome/Edge/Firefox 的安装、接管、回退、卸载验证矩阵。
- Safari wrapper 继续标记为未实现。

### 3. 非 HTTP 协议可靠性

FTP/FTPS、SFTP、BitTorrent、HLS、DASH、WebDAV 和 Metalink 已接入，但都不能按 HTTP 路径宣传为成熟。

- 为 FTP/FTPS 增加匿名、带凭据、显式 FTPS、隐式 FTPS、代理失败、断点恢复测试。
- 为 SFTP 增加密码认证失败、TOFU 主机密钥不匹配、代理连接失败、断点续传测试。
- 为 BT 增加限速、暂停、恢复、文件选择和元数据超时测试。
- 为 HLS 增加 AES 解密失败、直播轮询超时、ffmpeg 缺失/失败测试。
- 为 DASH 增加 ffmpeg 缺失、空输出、live 拒绝测试。
- 为 WebDAV 增加认证失败、PROPFIND 解析、大目录测试。
- 为 Metalink 增加镜像全部失败、校验和不匹配、路径安全测试。
- 统一协议错误分类和恢复动作（已有 `TaskFailureCategory` 15 类和 `RecoveryAction` 8 种），继续减少裸字符串错误。
- 验证 ffmpeg 在不同 OS 上的可用性（PATH 查找 vs `VIBE_FFMPEG_PATH`），确保缺失时给出明确诊断。

## P1：核心体验完善

### 1. 重复任务防护

已增加跨已有任务的重复判断：手动新建、批量导入和浏览器交接共享后端判重策略，按脱敏 URL、final URL 和 BT info hash 防止误建重复任务。

- 手动新建遇到重复任务时会提示，并允许用户明确选择“仍然创建副本”。
- 批量导入会把已有任务计入 duplicate，而不是 failed。
- 后续仍可补充浏览器 request id 与剪贴板来源的更细粒度提示文案。

### 2. 设置和新建任务继续降噪

设置页已全面重构为 7 个可折叠分区 + 搜索栏 + 自动保存（1000ms 防抖）；连续调整时成功反馈收敛到页内保存状态，不再产生连续成功 toast。

- 保持合并保存节流，避免连续 toast。
- 新建下载继续强化批量入口和错误文案。
- 加密凭据存储已覆盖 FTP/FTPS、SFTP 和 WebDAV，旧明文会在启动时自动迁移；对迁移失败的任务仍需提供明确重试或重新创建提示。

### 3. 启动恢复策略

设置页已增加“启动后续传中断任务”开关。默认继续保守关闭；开启后，启动时会把上次关闭时处于 downloading/retrying 的任务重新排队。

- 重新排队后仍走现有续传校验，不绕过本地临时文件与远端元数据检查。
- 手动暂停的任务不会被自动恢复。
- 后续可补充启动恢复摘要和失败原因统计。

## P2：可靠性和诊断

### 1. 重试策略集中化

HTTP（最多 5 段重试）、FTP（最多 2 worker 重试）、SFTP、BT（90s 元数据超时）、HLS（2 段重试 + 指数退避）、DASH、WebDAV 和 Metalink 当前各有重试/超时策略。

- 建立共享 retry policy 和错误分类。
- 记录 retry-after、退避原因、最终失败阶段。
- 在 UI 诊断中展示协议、阶段、重试次数和下一步建议。

### 2. 数据库迁移规范

项目已累积到 011 号迁移，包括一个涉及表重建的 `009_metalink.sql`（task_checksums 新增 file_id 列）。

- 后续只新增 additive migration，不重写历史 migration。
- 复杂迁移必须有旧数据升级测试。
- 发布前准备备份、失败提示和恢复文档。

### 3. 端到端覆盖

Rust 单元/集成测试较强，前端和桌面/浏览器 E2E 仍偏弱。

- 补设置页、新建下载、命令面板、详情诊断的组件或集成测试。
- 补 Native Messaging、WebSocket bridge、单实例转发和剪贴板捕获的端到端验证。
- 保留发布前手动验证清单，直到自动化覆盖足够。

## P3：性能

### 1. 详情诊断推送化

详情页诊断轮询已降频，但本质仍是定时拉取。

- 优先用事件更新摘要。
- 仅在 tab 可见时拉取分页数据。
- 对大 segment/request 历史保留分页，避免一次返回过多。

### 2. 浏览器实时桥规模化

实时桥初始同步已限制为活跃任务加最近历史，扩展侧 live task map 已设置最大容量并优先保留活跃任务；WebSocket pending request 也会在响应、超时或断线时清理，但仍需要压测。

- 用 1k、10k 历史任务测试扩展启动速度。
- 如果仍慢，增加专用 active/recent 查询而不是从完整列表过滤。
- 继续观察 popup 打开耗时和长时间运行后的内存占用。

### 3. 构建体积审计

目前只维护 English 和 简体中文，其他语言资源暂不暴露。

- 跑 `pnpm build` 后记录前端 bundle 体积。
- 检查字体、图标、语言资源和扩展包输出大小。
- 对非必要资源做懒加载或移出默认包。

## 建议验证命令

常规变更：

```bash
pnpm typecheck
pnpm test:frontend
pnpm build
pnpm check:bindings
pnpm test:rust
cargo check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
```

浏览器相关：

```bash
pnpm build:extensions
```

发布相关：

```bash
pnpm tauri build --config src-tauri/tauri.ci.conf.json
```
# Protocol and engine update

2026-06-17: HLS/m3u8 streaming engine (AES-128-CBC, live polling, ffmpeg remux), DASH/MPD engine (ffmpeg), WebDAV/WebDAVS engine (PROPFIND, Basic Auth), Metalink4 engine (mirror failover, checksum verification), SFTP engine (TOFU, SOCKS5), FTP/FTPS dynamic parallel segments, HTTP auto-acceleration, encrypted credential storage (ChaCha20-Poly1305) with legacy migration, per-task proxy overrides, scheduled download windows, floating status window, store decomposition, accent color themes, and settings page overhaul.
