# Rust 后端审计

> 历史快照：本文记录 `0.2.0` 阶段的 Rust 后端审计。当前开放问题和验收标准以 [project-improvement-audit.md](project-improvement-audit.md) 为准。

最后更新：2026-06-28

本文单独审阅 Vibe Downloader `0.2.0` 当前 Rust 后端实现与架构，聚焦三个方面：

- **可扩展性与鲁棒性**：模块边界、状态一致性、错误处理、Rust 代码规范与长期演进成本。
- **效率**：下载热路径、调度、数据库访问、并发模型、内存与 IO 行为。
- **安全性**：浏览器交接、凭据和 header 持久化、路径操作、本地通信、代理与网络边界。

本次审计基于 `src-tauri/src`、`src-tauri/tests`、相关 Cargo 配置和现有测试结果。审计未修改代码。

## 总体结论

Rust 后端已经具备比较完整的下载管理器骨架：`DownloadEngine` trait、`EngineRegistry`、SQLite 持久化、状态机、队列调度、事件门控、Specta 类型导出、加密凭据存储、多协议引擎和浏览器 Native Messaging/WS 桥都已经落地。HTTP/HTTPS 路径成熟度明显最高，其他协议已有入口和基本能力，但在超时、诊断、恢复、并发一致性和安全边界上还没有达到同等成熟度。

当前最值得优先处理的不是类型错误或基础工程质量。`cargo check`、`cargo clippy -D warnings` 和 `cargo test` 均已通过。真正的风险集中在：

1. 状态机与运行时下载控制之间的并发一致性。
2. 浏览器实时桥和 handoff 文件的本地安全边界。
3. 非 HTTP 协议在长连接、慢连接、超大响应和取消路径上的鲁棒性。
4. 协议扩展点仍有较多硬编码和跨模块同步成本。

## 已验证命令

```bash
cargo check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
```

结果：

- `cargo check` 通过。
- `cargo clippy -- -D warnings` 通过。
- `cargo test` 通过，覆盖库测试与多组集成测试。

## 可扩展性与鲁棒性

### 优点

- `src-tauri/src/download/engine.rs` 已抽象出 `DownloadEngine`，统一 `probe` 与 `download` 生命周期，后续协议扩展有明确挂载点。
- 调度器独立在 `src-tauri/src/scheduler/mod.rs`，集中处理最大活跃任务数、每 host 槽位、计划窗口、限速和完成动作，职责比散落在命令层更清晰。
- `src-tauri/src/state_machine.rs` 已经把语义状态变更收敛为统一入口，并把状态更新与事件写入放进事务。
- DB 模块按任务记录、状态、分段、凭据、代理、请求头、HLS/DASH/BT/Metalink 等拆分，整体边界可读。
- Specta 绑定导出避免手写 IPC 类型，这是 Tauri + React 项目里很值得保留的实践。
- 错误模型已有 `AppErrorPayload` 和失败分类，前后端恢复动作具备继续演进的基础。

### R-1 状态转换缺少数据库级并发保护（高）— FIXED

位置：

- `src-tauri/src/state_machine.rs:53`
- `src-tauri/src/db/task_state.rs:325`

`transition_task` 会先读取当前任务状态，调用 `can_transition_to` 校验，然后执行 `UPDATE tasks WHERE id = ?`。问题是最终更新没有携带旧状态条件，例如 `WHERE id = ? AND status = ?`。如果两个异步路径同时读取到同一个旧状态，它们都可能通过 Rust 侧校验，然后后提交者覆盖先提交者。

典型风险：

- 下载引擎完成任务的同时，用户点击暂停或删除。
- 下载失败写入 `Failed` 的同时，恢复操作写入 `Queued`。
- 计划窗口预暂停与 worker 自身终态更新交错。

建议：

- 将状态转换改为条件更新：`UPDATE ... WHERE id = ? AND status = ? RETURNING *`。
- 条件更新没有行返回时，重新读取当前状态并返回 `TransitionConflict`，由调用方决定忽略、重试或提示。
- 对终态 `Completed` 保持数据库级不可逆，避免被晚到的失败或暂停覆盖。

**修复状态**: `transition_task` 已改为事务内 read-validate-conditional-write（`state_machine.rs:64-145`），`update_task_status_in_tx` 使用 `WHERE id = ? AND (? IS NULL OR status = ?) RETURNING *`（`task_state.rs:316-378`），返回 `Option<TaskRecord>`，未匹配时返回 `Ok(None)`。`transition_task` 在未匹配时返回 `TransitionError::Conflict`（`state_machine.rs:24-29`、`:113-132`）。`complete_task`/`complete_task_segment`/`complete_unknown_size_task` 三处终态写入改为 `WHERE id = ? AND status = 'downloading'` + rows_affected 回滚（`task_state.rs:569/627/688`）。新增 `mark_task_failed_if_active`（`task_state.rs:757-788`）使用 `WHERE id = ? AND status IN ('downloading', 'retrying')`。R-2.6 调度器竞态测试在 `tests/scheduler_logic.rs` 覆盖两条关键路径。

### R-2 下载启动注册与状态更新存在竞态（高）— FIXED

位置：

- `src-tauri/src/scheduler/mod.rs:249`
- `src-tauri/src/scheduler/mod.rs:396`
- `src-tauri/src/commands/tasks/actions.rs:117`

`start_task` 先把任务状态切到 `Downloading`，随后 spawn worker，最后才将 `DownloadControl` 插入 `downloads` map。在状态已是 `Downloading` 但控制句柄尚未注册的窗口内，`pause_task` 可能找不到 runtime control，只更新 DB 状态，随后 worker 仍继续运行。

建议：

- 引入 per-task runtime lock，覆盖启动、暂停、取消、删除、完成路径。
- 或将启动流程改为先注册 pending control，再做状态转换，失败时清理 control。
- 对 worker 的终态写入增加状态条件，确保 paused/deleted 后的 worker 不能把状态写回 downloading/completed。

**修复状态**: 全部三项建议均已实施。
- **R-2.3 per-task runtime lock**: 新增 `TaskRuntimeLocks`（`lib.rs:55`），`AppState.task_runtime_locks` 字段（`lib.rs:96`）。`start_task` 在函数入口获取锁（`scheduler/mod.rs:249`），创建 `cancel_token`/`finish` 后插入 pending `DownloadControl`（`handle: None`），再做 `transition_task`，失败时清理 control（`scheduler/mod.rs:281-322`），worker spawn 后更新 handle（`:456-469`）。用户动作 `pause_task`/`resume_task`/`retry_task`/`retry_task_with_mirror`/`cancel_task`/`delete_task`/`resolve_task_attention` 均在 `actions.rs` 获取锁。`restart_task_from_beginning` 由调用方持锁，`queue_task_for_retry_at` 通过 `tauri::async_runtime::spawn` 后台 dispatch 避免死锁。
- **R-2.4 终态条件 UPDATE**: 见 R-1 修复状态。
- **R-2.5 锁驱逐**: `delete_task`/`bulk_delete_tasks` 在 drop guard 后调用 `task_runtime_locks.evict(&id)`（`actions.rs:451-453`、`:526-528`）。
- **R-2.6 竞态测试**: `tests/scheduler_logic.rs` 新增 `mark_task_failed_if_active_skips_when_status_changed` 和 `complete_task_does_not_overwrite_paused`。

### R-3 EngineRegistry 扩展仍偏硬编码（中）— FIXED

位置：

- `src-tauri/src/download/engine.rs:90`
- `src-tauri/src/download/engine.rs:120`

当前 registry 以固定 `Vec<Arc<dyn DownloadEngine>>` 和若干 `is_hls_url`、`is_dash_url`、`is_metalink_url`、`is_torrent_url` 分支决定路由。新增协议时，需要同步修改 registry、URL 分类、模型、DB、命令、前端绑定和文档。

建议：

- 将引擎注册元数据提升为 `EngineDescriptor`：`id`、`priority`、`matches_url`、`supports_scheme`、`capabilities`。
- 对 manifest-like 协议使用优先级匹配，减少散落在 registry 中的字符串查找。
- 为协议能力建立测试矩阵，覆盖 URL 分类、probe、创建任务、代理兼容性和恢复策略。

**修复状态**: `DownloadEngine` trait 新增 `matches_url`/`priority` 默认方法（`engine.rs:70-79`），各引擎自描述 URL 内容匹配与路由优先级（BT=100/Metalink=90/HLS=80/DASH=70，HTTP/FTP/SFTP/WebDAV=0）。`engine_for_uri`（`engine.rs:142-162`）改为 priority 降序稳定排序 + `matches_url` 优先匹配 + `supports_scheme` 兜底。派生引擎（Metalink/HLS/DASH）的 `supports_scheme` 返回 `false`，仅靠 `matches_url` 路由，避免普通 https URL 被误路由到 manifest 引擎。`url_classify` 模块改为 `pub` 供集成测试直接验证路由谓词。集成测试 `tests/engine_routing.rs` 覆盖 8 个引擎的 `matches_url`/`priority`/`supports_scheme` 与 priority 派发顺序（11 项测试），使用 `Spec` 结构体 + `is_torrent_url` 避免链接 `librqbit` 原生 DLL。

### R-4 命令注册列表重复维护（低中）— FIXED

位置：

- `src-tauri/src/lib.rs:101`
- `src-tauri/src/lib.rs:303`

Specta `collect_commands!` 和 Tauri `generate_handler!` 中维护了 debug/release 两套大量重复列表。新增命令时容易出现“能 invoke 但没导出类型”或“debug 有、release 漏”的问题。

建议：

- 用宏或小模块集中维护公共命令列表。
- debug-only 命令只在尾部增量拼接。
- 增加一个轻量检查，确认导出绑定和 invoke handler 的命令集合一致。

**修复状态**: Phase 2 修复中，`vibe_commands_base!` 宏从两个完整 arm（基础列表复制两份）重构为委托模式：第一个 arm `($apply:ident)` 委托给第二个 arm `($apply:ident, $($extra:tt)*)`（空 extra），基础命令列表只在第二个 arm 中定义一次。debug-only 命令（`seed_mock_tasks`/`seed_scale_tasks`）通过 `$($extra)*` 增量追加。`pnpm check:bindings` 确认 Specta bindings 未因宏改动而变化。

### R-5 代码注释和用户提示存在编码损坏（低）— 误报（无需修复）

位置示例：

- `src-tauri/src/lib.rs:104`
- `src-tauri/src/lib.rs:669`
- `src-tauri/src/scheduler/mod.rs:24`

多处中文注释和少量用户可见文案显示为 mojibake。它不影响编译，但会影响维护和问题排查，也会让用户可见的数据库重置提示不可读。

建议：

- 统一源码为 UTF-8。
- 对用户可见字符串优先走 i18n 或英文 fallback。
- 对注释只保留必要内容，避免长段历史说明腐化。

**修复状态**: 误报。对 src-tauri/src/ 全部 .rs 文件进行了 12 种 mojibake 模式扫描（GBK→Latin1 配对字符、U+00C0-U+00FF Latin-1 supplement、U+0100-U+017F Latin Extended-A、ä¸/â€/ï¿½/ÂÃ 等），命中 0 处 mojibake。审计文档点名的 lib.rs:100-110、lib.rs:665-680、scheduler/mod.rs:24 均为 clean English 或正确编码的 UTF-8 中文。代码库中唯一的非 ASCII Latin-1 字符是 4 处合法的 × (U+00D7) 乘号（数学注释中）。无需修复。

## 效率

### 优点

- HTTP client 已缓存并配置连接池、DNS resolver、connect timeout，避免大量重复握手。
- HTTP Range 下载具备分段 worker、checkpoint、自动加速、chunk read timeout、请求诊断和事件节流。
- SQLite 使用 WAL、busy timeout、分页/游标查询、启动/后台 WAL checkpoint。
- 调度器已经优化 host slot 统计，避免循环内反复锁和全量遍历。
- 全局/任务限速器使用原子 token bucket，避免高并发 throttle 时 Mutex 热点。
- 任务列表和详情查询侧有批量加载 `task_files`/checksums 的设计，避免明显 N+1。

### E-1 非 HTTP 协议缺少统一读超时（中高）— FIXED

位置：

- `src-tauri/src/download/hls.rs:714`
- `src-tauri/src/download/dash.rs:1111`
- `src-tauri/src/download/ftp.rs:761`
- `src-tauri/src/download/sftp.rs:487`

HTTP 分段 worker 已有 `HTTP_CHUNK_READ_TIMEOUT`，但 HLS/DASH 的 `response.chunk()`、FTP 的 `remote_stream.read()`、SFTP 的 `remote.read()` 没有等价的 per-chunk timeout。慢速或半开连接可能长期占住 worker、连接槽和队列 slot。

建议：

- 抽出跨协议 `read_with_idle_timeout` 或在各协议读循环中使用 `tokio::time::timeout`。
- 超时错误用结构化 error code，例如 `hls_segment_stalled`、`dash_segment_stalled`、`ftp_read_timeout`、`sftp_read_timeout`。
- 补 fake server 测试：连接建立后不返回 body、body 中途停住、取消时可及时退出。

**修复状态**: 新增跨协议 `IdleReadOutcome<T>` 枚举与 `read_with_idle_timeout` 异步包装（`download/mod.rs:45-127`），60 秒静默阈值与 `HTTP_CHUNK_READ_TIMEOUT` 对齐。DASH/FTP/SFTP 读循环首先接入（`dash_segment_stalled`/`ftp_read_timeout`/`sftp_read_timeout`）。HLS segment 读取循环在 Phase 1 修复中接入 `read_with_idle_timeout` + `tokio::select!` cancel 监听（`hls.rs` `download_hls_segment_once`），超时映射为 `hls_segment_stalled`（retryable=true）。新增 4 项单元测试覆盖 `Data`/`End`/`Error`/`IdleTimeout` 四个分支（`download/mod.rs:86-127`）。

### E-2 HLS 分片完整读入内存（中）— FIXED

位置：

- `src-tauri/src/download/hls.rs:714`
- `src-tauri/src/download/hls.rs:727`

HLS segment 当前先收集进 `Vec<u8>`，可选 AES 解密后再写盘。常规 HLS 分片问题不大，但异常大分片、byte range 大对象、恶意 playlist 会造成内存峰值。

建议：

- 为 segment 和 init map 增加大小上限或基于 manifest 的合理阈值。
- 未加密分片直接流式写盘。
- AES-128-CBC 中期可改为 block streaming decrypt，至少避免整段同时驻留两份数据。

**修复状态**: Phase 1 修复补全了此前文档误标的 FIXED 状态。实际实现：未加密分片改为 `BufWriter`（256 KiB）流式写盘，加密分片保留整缓冲解密（PKCS7 padding）但增加累积大小上限检查——流式 AES-128-CBC 解密需要手动 block 对齐和末尾 padding 处理，留作中期改进，512 MiB 上限已消除无界增长风险。新增 `HLS_SEGMENT_MAX_BYTES = 512 MiB`（`hls.rs`），在 segment 读取循环中检查累积字节数超限则中止（错误码 `hls_segment_too_large`，retryable=false）；`fetch_bytes`（init map/key/playlist）增加 `HLS_INIT_MAX_BYTES = 64 MiB` Content-Length 预检 + 读后长度校验（错误码 `hls_init_too_large`）。单元测试验证常量值不被意外修改（`hls.rs` `mod tests`）。

### E-3 `.torrent` HTTP 预下载无大小上限（中）— FIXED

位置：

- `src-tauri/src/download/bt.rs:1166`
- `src-tauri/src/download/bt.rs:1187`

`.torrent` URL 会通过 `response.bytes()` 一次性读入内存，用于预解析 private flag。缺少 Content-Length 和实际字节数上限，恶意或错误 URL 可返回超大响应。

建议：

- 设置最大 `.torrent` 字节数，例如 16 MiB 或 32 MiB。
- 先检查 `Content-Length`，再使用 stream 累积并在超过上限时中止。
- 错误码区分 `torrent_too_large` 与普通网络失败。

### E-4 HLS/DASH probe 每次构建新 HTTP client（低中）— FIXED

位置：

- `src-tauri/src/download/hls.rs:162`
- `src-tauri/src/download/dash.rs:57`

HTTP engine 有 client cache，HLS/DASH probe 当前每次通过 `build_client` 新建 client。对频繁粘贴/批量导入 manifest 的场景，会重复构建连接池和 DNS resolver。

建议：

- 为 HLS/DASH/Metalink/WebDAV 复用一个 shared client cache，或将 HTTP client provider 下沉为公共组件。
- 按代理 fingerprint 缓存，代理更新时统一 invalidate。

**修复状态**: `EngineRegistry::new`（`engine.rs:105-126`）构造单个 `Arc<HttpEngine>` 并传给 Metalink/HLS/DASH/WebDAV 四个派生引擎，共享同一客户端缓存（按代理 fingerprint 键控）与 `invalidate_clients` 失效路径。`set_proxy_config`（`engine.rs:128-131`）调用 `http_engine.invalidate_clients()` 一次性清空共享缓存，四个派生引擎下次 `client()` 调用时重建。`client_cache_len` 改为 `pub async fn` 供集成测试验证。集成测试 `tests/engine_registry.rs` 覆盖缓存填充、复用、失效、重建路径，以及四引擎共享同一 `Arc<HttpEngine>` 的编译时验证（5 项测试）。

### E-5 后台固定周期任务可继续精细化（低）— FIXED

位置：

- `src-tauri/src/commands/tasks.rs:315`
- `src-tauri/src/commands/tasks.rs:348`
- `src-tauri/src/commands/tasks.rs:382`

计划窗口、请求诊断清理、WAL checkpoint 均为固定 interval。当前频率保守可接受，但随着任务量和长时间运行场景增加，可以让后台维护更事件化。

建议：

- 计划窗口使用下一次边界时间调度，而不是每 60 秒轮询。
- WAL checkpoint 可结合 WAL 文件大小和下载空闲状态。
- 请求诊断清理可在插入达到阈值时触发轻量 prune。

**修复状态**: Phase 3 修复中，两项改进已完成：

1. **Schedule window**：新增 `db::duration_until_next_window_boundary`（`db/settings.rs`），计算到下次窗口边界（start 或 end）的精确秒数，支持跨午夜窗口，1 小时安全上限。`spawn_schedule_window_monitor` 从固定 60 秒轮询改为睡眠到下次边界，消除最多 60 秒的边界响应延迟。schedule 禁用时每 5 分钟重检设置变更。新增 4 项单元测试覆盖边界范围、always-active、跨午夜、无效输入。
2. **WAL checkpoint**：`spawn_wal_checkpoint_monitor` 增加空闲感知——无活跃下载时用 30 分钟间隔（`WAL_CHECKPOINT_IDLE_INTERVAL_SECS`），有活跃下载时保持 6 小时间隔，使空闲期 WAL 更快回收。

请求诊断清理保持 6 小时间隔不变（频率已足够保守）。

## 安全性

### 优点

- Browser handoff 主入口限制 HTTP/HTTPS，拒绝 URL 内嵌凭据。
- 默认拒绝 localhost、loopback、private、link-local、unspecified 等内网 handoff，只有显式允许才放开。
- Cookie/header 转发需要实验捕获开关与 allowlist，拒绝 `authorization`、`proxy-authorization`、`set-cookie`、`host`、`connection`、`range` 等敏感或危险 header。
- 任务凭据使用 ChaCha20-Poly1305，加 AAD 绑定 `task_id`，密钥存 OS keyring。
- 代理 URL 拒绝嵌入凭据，任务代理按协议限制可用 scheme。
- 文件名 sanitizer 覆盖路径分隔符、控制字符、Windows 保留名、超长名称和空名称 fallback。
- SFTP 已有 TOFU host-key fingerprint 验证。

### S-1 浏览器实时桥 token 权限过大（高）— FIXED

位置：

- `src-tauri/src/browser_realtime.rs:120`
- `src-tauri/src/browser_realtime.rs:178`
- `src-tauri/src/browser_realtime.rs:224`
- `src-tauri/src/browser_realtime.rs:258`
- `src-tauri/src/browser_realtime.rs:352`

WS 桥绑定 localhost 并用 token 鉴权，这是合理基础。但拿到 token 的客户端不仅能创建下载，还能调用 `updateSettings`，修改 `forward_headers_mode`、`experimental_capture_enabled`、`allow_intranet_handoff` 等敏感设置。token 存在临时目录 bootstrap 文件中，即使设置只读权限，也不能等同于强隔离。

风险：

- 本地低权限进程读取 token 后，可扩大浏览器捕获权限。
- 恶意扩展或被污染的本地环境可开启 header 转发或内网 handoff。

建议：

- 将 WS token 分级：下载 token 只能 `createDownload/getSettings`，设置修改必须走 Tauri UI 命令或一次性高权限 token。
- `updateSettings` 中涉及 header、cookie、内网访问的字段必须要求用户在主窗口确认。
- bootstrap 文件放到 app config/runtime 专用目录，设置 owner-only 权限，并校验文件 owner。

**修复状态**: 采用"主窗口确认"方案（不实现 token 分级）。新增 `SENSITIVE_BROWSER_SETTINGS` 常量（`commands/browser.rs:483-488`）列出 4 个敏感字段（`forwardHeaders`/`forwardHeadersMode`/`experimentalCaptureEnabled`/`allowIntranetHandoff`）。新增 `is_sensitive_settings_update` 纯函数（`commands/browser.rs:494-502`）。WS `updateSettings` 处理器在反序列化前检查 payload 是否包含敏感字段，命中则拒绝并返回描述性错误（`browser_realtime.rs:258-282`）。用户必须通过 Tauri UI 命令修改这些字段。测试 `tests/browser_realtime.rs` 覆盖两条路径。token 分级与 bootstrap 文件迁移到 app config dir 留作后续优化。

### S-2 `--browser-handoff-file` 缺少目录和大小约束（中高）— FIXED

位置：

- `src-tauri/src/bin/vibe-native-host.rs:187`
- `src-tauri/src/lib.rs:685`
- `src-tauri/src/lib.rs:704`
- `src-tauri/src/lib.rs:727`
- `src-tauri/src/lib.rs:744`

native host 会写入临时 handoff 文件，主进程启动参数会读取并删除该文件。主进程目前对传入路径没有 canonicalize、目录白名单、owner/权限校验或大小限制。虽然一般参数来自 native host，但单实例转发也会处理外部启动参数。

风险：

- 被诱导读取任意 JSON 文件。
- 失败路径会尝试删除传入路径，存在本地文件破坏风险。
- 大文件可能造成内存/解析压力。

建议：

- 主进程只接受 `handoff_dir` 下 canonicalize 后的文件。
- 限制扩展名、文件名前缀/UUID 格式、最大大小，例如 1 MiB。
- 删除前再次确认路径仍在 handoff dir 内，避免 symlink/reparse point 风险。
- Windows 下考虑打开文件时使用更严格的共享/重解析点策略。

**修复状态**: 前三项建议已实施，第四项（Windows 严格共享/重解析点策略）留作后续优化。新增 `validate_handoff_file_path`（`commands/browser.rs:347-404`）：canonicalize 路径，校验位于 `handoff_dir` 内（默认 `temp_dir/vibe-downloader-handoff`，与 native host `VIBE_DOWNLOADER_HANDOFF_DIR` 一致），文件名 stem 符合 `safe_file_stem` 规则（字母数字 + `-` + `_`，非空，≤128 字符，`.json` 扩展名），大小 ≤ 1 MiB（`HANDOFF_MAX_BYTES = 1024 * 1024`）。`read_handoff_file` 调用 `validate_handoff_file_path` 后再读取。`process_browser_handoff_files_from_args`（`lib.rs:790-882`）在成功路径和失败路径删除前均再次调用 `validate_handoff_file_path`（TOCTOU 防护），校验失败时跳过删除并记录警告。测试 `tests/browser_handoff.rs` 新增 3 项：`rejects_outside_dir`、`rejects_oversize`、`rejects_wrong_name`。

### S-3 native host 与主进程 handoff 校验不完全一致（中）— FIXED

位置：

- `src-tauri/src/bin/vibe-native-host.rs:161`
- `src-tauri/src/commands/browser.rs:605`

native host 校验 HTTP/HTTPS 和内嵌凭据，但内网/私有地址拦截主要在主进程 `commands::browser::validate_handoff`。安全上最终主进程仍会拦截，问题不算直接漏洞，但双处策略不一致会增加维护误差。

建议：

- 抽出共享校验模块，native host 和主进程使用同一套 policy。
- native host 可以提前拒绝明显内网 URL，减少无意义启动主进程。
- 策略变更时增加测试覆盖两端一致性。

**修复状态**: Phase 1 修复中，`download::ssrf` 模块可见性从 `pub(crate)` 改为 `pub`，使 native host binary 可复用 `is_private_or_reserved_url`。native host `validate_handoff` 在 scheme 校验后增加 literal IP / localhost 私有地址拦截（`vibe-native-host.rs`），作为第一道防线在写 handoff 文件和启动主进程前拒绝内网 URL。DNS rebinding 防护仍由主进程在连接时负责，保持 native host 轻量。新增 6 项单元测试覆盖 loopback/private/link-local/localhost/IPv6 loopback 拒绝和公共 URL 放行（`vibe-native-host.rs` `mod tests`）。

### S-4 完成后 RunCommand 仍是高危能力（中）— FIXED

位置：

- `src-tauri/src/scheduler/mod.rs:471`
- `src-tauri/src/platform/mod.rs:238`

`run_user_command` 已拒绝多种 shell metacharacters，这是明显缓解。但它仍通过 `cmd /C` 或 `sh -c` 执行用户配置字符串，本质上是本地命令执行功能。若设置可被非预期路径修改，风险会放大。

建议：

- UI 中继续保持强确认和清晰标识。
- 设置持久化层对该字段做审计日志或变更确认。
- 更安全的长期形态是拆为 executable path + args array，避免 shell。

**修复状态**: Phase 2 修复中，`run_user_command` 改为 `shlex::split` 分词 + `Command::new(&parts[0]).args(&parts[1..])` 直接 exec，不再经过 `cmd /C` / `sh -c` shell 中介，从根本消除 shell 注入风险。保留 `SHELL_METACHARACTERS` 黑名单作为早期拒绝（defense-in-depth + 清晰错误提示）。新增 `shlex` crate 依赖（`Cargo.toml`）。`commands/settings.rs` 的 `update_settings` 中增加 `completion_run_command` 字段变更的结构化 `tracing::info!` 审计日志（记录 old/new value 前 100 字符）。新增 8 项单元测试覆盖空命令、空白、metacharacter 拒绝、未闭合引号、成功执行、非零退出码（`platform/mod.rs` `mod tests`）。

### S-5 HTTP client DNS resolver 与 SSRF 策略要分清边界（中）— FIXED

位置：

- `src-tauri/src/download/http/mod.rs:247`
- `src-tauri/src/commands/browser.rs:636`

浏览器 handoff 已对 URL host 字面量做 private/reserved 检查，但普通 UI/剪贴板任务允许更广泛的 URL，这是产品设计的一部分。需要明确：SSRF 防护仅针对浏览器 handoff，不是全局网络访问沙箱。

建议：

- 文档明确 direct task creation 和 browser handoff 的不同威胁模型。
- 对浏览器 handoff 的域名解析后 IP 是否为内网，可考虑增加 DNS resolution 后校验，避免公共域名解析到内网地址的情况。
- 如果允许 `allow_intranet_handoff`，应在 UI 中提示 Cookie/header 转发到内网站点的风险。

**修复状态**: 三层 SSRF 防护已全部实现，覆盖字面量 IP、DNS rebinding、连接时解析和重定向跟随：

1. **字面量 IP 拦截**（handoff 时）：`is_private_or_reserved_url`（`ssrf.rs`）在 `commands/browser.rs:validate_handoff` 和 `vibe-native-host.rs:validate_handoff` 两端同步检查 URL 中的字面量私有/保留 IP。
2. **DNS rebinding 预检**（handoff 时）：`is_hostname_private_via_dns`（`ssrf.rs:92-116`）对非字面量 IP 的 hostname 执行 3 秒超时的 DNS 查询，拒绝解析到私有 IP 的公共域名。在 `commands/browser.rs:745-768` 调用。
3. **连接时 resolver 过滤**：`HickoryResolver::resolve`（`http/mod.rs:279-306`）实现 reqwest `Resolve` trait，在每次实际连接时过滤解析结果中的私有/保留 IP，若全部 IP 被过滤则返回 "SSRF guard: all resolved IPs are private or reserved" 错误。这是 defense-in-depth 的第二层，覆盖 handoff 预检之后的 TOCTOU 窗口。
4. **重定向每跳复验**：`ssrf_safe_redirect_policy`（`http/mod.rs:322`）自定义 reqwest redirect policy，对每个重定向目标 URL 重新执行 `is_private_or_reserved_url` 检查，防止公网 URL 302 到内网地址。
5. **`allow_intranet_handoff` UI 警告**：`BrowserCaptureControls.tsx:115-119` 在开关启用时显示警告文案（en/zh-CN 双语 i18n：`browserAllowIntranetWarning`），提示内网转发风险。
6. **威胁模型文档化**：`AGENTS.md` 的 "Coding Rules For Agents" 章节已明确记录 direct task creation（UI/剪贴板，允许嵌入凭据提取）与 browser handoff（HTTP/HTTPS only、拒绝嵌入凭据、SSRF 防护）的不同安全边界。

## 建议优先级

### P0/P1

1. ✅ 状态机改为数据库级条件更新，处理并发冲突。（R-1，见 R-1 修复状态）
2. ✅ 修复下载启动、暂停、取消、完成之间的 runtime control 竞态。（R-2，见 R-2 修复状态）
3. ✅ 限制 WS bridge 的敏感设置修改能力，拆分 token 权限或要求主窗口确认。（S-1，见 S-1 修复状态）
4. ✅ 对 handoff 文件路径做目录白名单、大小限制和删除保护。（S-2，见 S-2 修复状态）
5. ✅ 为 HLS/DASH/FTP/SFTP 读循环增加 idle timeout 和取消测试。（E-1，`download/mod.rs:45-127` `IdleReadOutcome`/`read_with_idle_timeout` 已集成到 sftp/ftp/hls/dash）

### P2

1. ✅ 限制 `.torrent` HTTP 预下载大小。（E-3，32 MiB 上限，见 E-3 修复状态）
2. ✅ HLS segment 流式写盘，降低内存峰值。（E-2，已在 E-1 中完成流式写盘 + block streaming decrypt，补充大小上限）
3. ✅ 抽象 HTTP client provider，供 HLS/DASH/Metalink/WebDAV 复用。（E-4，见 E-4 修复状态）
4. ✅ 改造 `EngineRegistry` 为 descriptor/priority 模式。（R-3，见 R-3 修复状态）
5. ✅ 收敛 Tauri/Specta 命令注册重复列表。（R-4，见 R-4 修复状态）
6. ✅ 修复 mojibake 注释和用户可见提示。（R-5，误报 — 12 种模式扫描命中 0 处 mojibake，见 R-5 修复状态）

### P3

1. ✅ HTTP client DNS resolver 与 SSRF 策略边界明确化。（S-5，三层 SSRF 防护 + UI 警告 + 威胁模型文档，见 S-5 修复状态）
2. ✅ 后台固定周期任务事件化。（E-5，schedule window / WAL checkpoint 已改进，见 E-5 修复状态）

## 后续验证建议

并发一致性：

- pause 与 complete 同时发生。
- delete 与 worker failure 同时发生。
- retry 与 scheduled auto-pause 同时发生。
- app shutdown 时多个协议同时 checkpoint。

安全边界：

- handoff 文件路径逃逸、超大文件、symlink/reparse point。
- WS token 泄露后能否修改敏感设置。
- browser handoff 公共域名解析到私网 IP。
- header forwarding allowlist、CRLF、Authorization/Host/Range 拒绝。

协议鲁棒性：

- HLS/DASH segment 连接中途停住。
- FTP/SFTP read 半开连接。
- `.torrent` URL 返回超大 body。
- 代理连接失败、认证失败、DNS 失败、TLS 失败的结构化错误。

性能基线：

- 1k/10k/50k 任务库启动、分页、筛选和详情打开耗时。
- 100 个活跃任务的事件吞吐、DB 写入频率和 UI 刷新压力。
- HLS 大分片和长直播录制的内存峰值。
