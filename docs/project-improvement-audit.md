# 项目改进审计

最后更新：2026-06-28

本审计基于当前仓库代码、配置、测试和文档状态，按风险和用户影响列出仍需处理的问题。它不替代 [ROADMAP.md](ROADMAP.md)；路线图描述方向，本文件描述优先级、风险和验证重点。

> 详细四维复核见 [architecture-audit.md](architecture-audit.md)：从用户交互便捷性、功能完整性、架构鲁棒性、运行效率四个维度展开，并包含旧结论修正清单。

## 总体结论

Vibe Downloader `0.2.0` 已经远超最早 HTTP MVP：HTTP/HTTPS 下载核心、SQLite 持久化、队列调度、限速、浏览器 Native Messaging、WebSocket 实时桥、剪贴板监控、设置页、命令面板、任务详情诊断、虚拟滚动、浮动状态窗口、多协议入口和 CI 验证都已经落地。

当前主要风险不再是基础能力缺失，而是发布级成熟度不足：

1. **发布信任链未闭环**：浏览器商店身份、扩展签名、Safari wrapper、OS 代码签名、权限文案和 updater 端到端演练仍是公开发布阻断。
2. **非 HTTP 协议成熟度不均**：FTP/FTPS、SFTP、BitTorrent、HLS、DASH、WebDAV、Metalink 已接入，但可靠性、诊断、恢复能力和测试覆盖仍低于 HTTP/HTTPS。
3. **关键交互闭环仍可补强**：URL 探测阶段反馈、错误详情复制、磁盘空间不足恢复、队列重排、每任务计划窗口开关、多文件筛选还不够顺手。
4. **规模化性能缺少实测基线**：任务列表已有 store 分解、游标分页和虚拟滚动，但 1k/10k/50k 历史任务、100 活跃任务、多协议混合下载、长诊断历史等场景仍缺少可追踪指标。

## 已确认优势

- HTTP probe 支持 HEAD 与 Range GET fallback，并识别文件名、大小、content type、Range 能力和来源 host。
- HTTP 下载引擎支持未知大小单连接、Range 分段、自动加速、分段 retry、断点续传校验、checkpoint 进度持久化、最终文件自动重命名和全局 token bucket 限速。
- FTP/FTPS 支持动态并行分段、SOCKS5 代理、加密凭据存储和目录探测。
- SFTP 支持密码凭据、加密存储、本地临时文件续传、目录探测、SOCKS5 代理和 TOFU 主机密钥验证。
- BitTorrent 支持 magnet、HTTP/HTTPS `.torrent` URL、本地 `file://*.torrent`、多文件选择、运行时快照、SOCKS5 代理和可配置做种。
- HLS 支持 master variant、AES-128-CBC、init map、byte range、并发分段、直播轮询和 ffmpeg MP4 remux。
- DASH 支持静态/VOD MPD 解析、ffmpeg 下载和 MP4 remux，并明确拒绝 dynamic/live。
- WebDAV/WebDAVS 支持 Basic Auth、PROPFIND 目录探测和 HTTP 引擎委托。
- Metalink4 支持多文件选择、HTTP/HTTPS 镜像 failover 和按最强可用算法校验。
- 加密凭据存储使用 ChaCha20-Poly1305，覆盖 FTP/FTPS、SFTP、WebDAV，并支持旧明文迁移。
- 逐任务代理覆盖支持继承/关闭/自定义，并按协议验证兼容性。
- UI 已具备 data/ui/speed-history store 分解、虚拟无限滚动、游标分页、状态筛选、搜索、排序、多选、批量动作、命令面板、设置搜索、任务详情、Chunks/Connections/Requests/Logs、toast、删除确认和恢复动作。
- 浏览器集成已有 Native Messaging host、本地 WebSocket bridge、manifest 安装诊断、下载接管、重复请求处理、单实例转发和显式 Cookie/header 转发。
- 多项旧高风险问题已经修复或缓解：browser handoff SSRF 防护（含 DNS rebinding/重定向每跳复验，A-2 已修复）、BT session 释放、WAL checkpoint、状态机事务化、`files_version` 缓存、HTTP client pool 参数、完成后 `sync_all`、单一 StatusBar、`Mod+R` 重试、onboarding 扩展引导、i18n 检查和 toast hidden count。
- 2026-06-30 修复批次：A-1 调度器槽位泄漏、A-2 SSRF 纵深、A-5 DNS resolver panic、E-1 queue-changed 增量刷新、E-2 统计快照节流、F-1 BT 种子率执行、F-2/F-3 Metalink 并行续传与进度聚合、F-4 Metalink 端到端测试、UX-1 i18n 自动检测限制到稳定语言。
- 2026-06-30 Batch 1 修复：A-3 退出 flush 并发 join_all、A-4 worker 完成路径 evict 运行时锁、A-6 Realtime 广播 lag 重发 snapshot、UX-2 搜索去除双防抖、UX-3 mod+f 聚焦搜索、UX-4 空列表区分筛选/无下载、UX-5 队列重排键盘快捷键 + 乐观更新。
- 2026-06-30 Batch 2 已完成 F-5：手动校验输入扩展到 SHA-256/SHA-512/SHA-1/MD5 四算法，`CreateTaskInput` 新增 `expected_hash` + `expected_hash_algorithm`，`normalize_expected_hash` 校验长度与 hex，UI 提供算法下拉，弱算法标注 weak；5 个 Rust 单元测试覆盖。F-7（BT 上传限速）与 F-6（HLS 多音轨/字幕）已完成（已修复 2026-06-30）。
- 2026-06-30 Batch 3 已完成 F-6/F-7：F-6 HLS 多音轨/字幕选择、F-7 BitTorrent 上传限速（已修复 2026-06-30）。

## P0：发布前阻断

### 1. 发布信任链端到端闭环

公开发布前必须完成或明确取舍，否则安装可信度和升级体验都会受影响。

- 替换 Chrome/Edge/Firefox release placeholder ID。
- 完成正式扩展签名、Firefox signed XPI、权限说明和 store review copy。
- 明确 Safari wrapper 是本版本不支持还是进入发布范围。
- 配置 macOS Developer ID / Windows signing；如果短期不签名，release notes 必须明确 unsigned 风险。
- 用测试 tag 完整演练 updater：旧版安装、检查更新、下载、relaunch、版本校验。

## P1：核心体验、可靠性和性能风险

### 1. 非 HTTP 协议可靠性补齐

HTTP/HTTPS 是当前最成熟路径，其他协议不能按同等成熟度宣传。

- SFTP 仍为单流下载，known-size 大文件需要评估多 channel 或多连接并行读取。
- Metalink 并行镜像 range 加速路径已修复续传与进度聚合（F-2/F-3），并补齐端到端测试（F-4）。
- HLS/DASH 依赖外部 ffmpeg，缺少路径设置 UI、版本诊断和跨平台安装引导。
- FTP/FTPS、SFTP、WebDAV、HLS 需要补齐 fake server 集成测试（Metalink 已于 2026-06-30 补齐）。
- 每个协议至少覆盖创建、暂停、恢复、凭据失败、代理失败、校验失败和错误恢复动作。

### 2. 用户交互闭环

基础 UI 已经完整，但一些高频失败和高级控制仍需要更直接的入口。

- 新建下载增加阶段化 `probePhase`：识别协议、读取清单、解析分片、检查运行时、完成、失败。
- 失败详情增加“一键复制错误详情”，包含 task id、URL、错误码、恢复动作和最近请求诊断。
- 磁盘空间不足时显示当前剩余、任务所需、差额和建议动作。
- 任务列表增加队列置顶、上移、下移、移到底部；中期支持拖拽重排。
- TaskDetails 增加“遵守计划下载窗口”开关，暴露已有 `obey_schedule` 能力。
- BT/Metalink 多文件选择增加搜索、类型筛选、按扩展名筛选、只选最大文件、全选当前筛选结果。

### 3. 架构恢复和安全审计

系统已有关键保护，但发布级恢复体验和审计清单仍不足。

- 数据库迁移失败时提供恢复对话框：备份路径、错误类型、重建/退出选择。
- CI 增加旧版本数据库快照迁移测试，覆盖 `Dirty`、版本缺失、版本不匹配等场景。
- 浏览器 handoff/header forwarding 做发布前权限审计：manifest 权限、host permissions、header allowlist、Authorization 拒绝、过期策略、卸载回滚、dev/release ID 差异。
- 关闭流程增加慢盘/高并发压测，验证 3 秒等待是否足够 flush checkpoint 和收敛子任务。

### 4. 性能热路径和规模基线

已有虚拟滚动、游标分页、事件节流和 store 分解，但仍需要生产规模数据验证。

- HLS 大分片改为流式写盘；AES-128-CBC 中期实现 block streaming 解密，避免完整 `Vec` 峰值。
- 剪贴板监控增加文本 hash 短路，避免重复文本每秒重新扫描。
- 窗口不可见时降低 progress flush 频率到 250-500ms，并合并每 task 最新 payload。
- `queue-changed` 事件已携带变更 task ids 做增量 upsert（E-1 已修复 2026-06-30）；后续可继续优化后端按批合并发射。
- 建立 `docs/performance-baseline.md`，记录 1k/10k/50k 任务库的启动首屏、滚动 FPS、筛选响应、详情页打开耗时和内存峰值。

## P2：功能完整性和中期增强

- DASH 增加 `SegmentTimeline` 支持；live DASH 排在核心可靠性之后。
- HLS 支持字幕/多音轨选择，并在文档中明确 DRM 不支持。
- 新建下载校验区支持 SHA-256/SHA-512/SHA-1/MD5 多算法输入，并标注弱校验。
- 增加本地 JSON-RPC 或 REST API，服务脚本、NAS、自动化和其他工具控制下载。
- 增加 PAC 代理支持，明确脚本执行沙箱。
- 增加完成后整理规则，支持按 `{category}`、`{host}`、`{date}`、`{name}` 等模板移动或重命名。
- 补设置页、新建下载、命令面板、详情诊断的前端组件或集成测试。
- 补 Native Messaging、WebSocket bridge、单实例转发和剪贴板捕获的端到端验证。

## 建议修复顺序

1. **发布可信度**：扩展 ID/签名、权限文案、OS signing 或 unsigned 策略、updater 演练。
2. **交互闭环**：probe 阶段反馈、复制错误详情、磁盘空间恢复、队列重排。
3. **协议成熟度**：SFTP 并行、Metalink 并行镜像、ffmpeg 管理、跨协议 fake server 测试。
4. **恢复和安全**：数据库恢复 UI、迁移快照测试、浏览器权限审计、关闭压测。
5. **性能基线**：HLS 内存峰值优化、剪贴板短路、后台 progress 降频、queue-changed 增量刷新、规模化基准文档。

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
