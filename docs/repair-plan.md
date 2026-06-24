# Vibe Downloader — 综合修复计划

基于三份审计报告的交叉合并与去重：

| 来源 | 简称 | 视角 | 最后更新 |
|------|------|------|----------|
| [project-improvement-audit.md](project-improvement-audit.md) | **PROJ** | 发布风险与协议成熟度 | 2026-06-17 |
| [architecture-audit.md](architecture-audit.md) | **ARCH** | 架构/引擎/调度/效率/交互 | 2026-06-24 |
| [audit-report.md](audit-report.md) | **A11Y** | 前端可访问性与主题 | — |

**2026-06-24 更新**：ARCH 审计新增了一批此前未覆盖的发现（标记 `NEW(0624)`），本计划已同步纳入。关键新增：BT private torrent 安全隐患（P0）、HTTP Client 无连接池复用（P0）、生产迁移失败无恢复路径（P1）、BT sessions 无淘汰（P1）、reqwest 无 timeout（P1）、SSRF 防护缺失（P2）、下载历史归档缺失（P1）、onboarding/扩展 i18n/toast 聚合（P0）等。

**审计冲突说明**：A11Y 报告标记的 4 项问题在 ARCH 2026-06-21 的源码核实中已被确认修复——FloatingStatusWindow 键盘交互（Escape/Enter）、Toast hover 暂停、批量删除对话框（已用设计系统替代 window.confirm）、TaskDetails aside 可访问名。本计划以 ARCH 的源码核实为准，不重复列出这些项。

---

## 总体评估

代码库骨架扎实：8 协议 trait 统一路由、panic 安全（裸 unwrap = 0）、结构化错误码端到端保真、Zustand 三层分解 + 虚拟化 + cursor 分页 + rAF 批处理、凭据 ChaCha20-Poly1305 + OS keyring 失败关闭、WS 桥仅绑 127.0.0.1 + 随机 token。

短板集中在**编排与闭环**，而非骨架：

1. **假特性与死代码**：优先级不生效、分类规则表无人读、站点规则无 UI、队列重排无调用方
2. **数据一致性缺口**：多表写入未包事务、去重 TOCTOU 无 UNIQUE、凭据迁移非事务
3. **效率热点**：调度器 ~240 次 DB 往返/burst、checkpoint 写放大、逐 chunk syscall、逐 chunk 加锁
4. **可访问性**：亮色模式对比度全面不达标、i18n lang 硬编码、动态消息缺 ARIA live region

---

## 阶段 1 — 闭环既有功能 + 消除崩溃 + 发布门槛（P0，~1 周）

目标：让"看起来能用"的功能真正生效，消除唯一进程崩溃隐患，修复最严重的可访问性违规，完成发布前必要准备。

### 1.1 任务优先级闭环 [ARCH F-1] ✅ 已完成

**问题**：调度分发 SQL `ORDER BY queue_position ASC, created_at ASC` 完全忽略 priority 字段。UI/命令/DB 列齐备但调度严格 FIFO——用户信任一个无效的功能。

**定位**：`task_records.rs:569`

**修复**：分发 SQL 已改为 `ORDER BY CASE priority WHEN 'high' THEN 0 WHEN 'normal' THEN 1 ELSE 2 END, queue_position ASC, created_at ASC`。priority 现在正确参与调度顺序。

### 1.2 ffmpeg 路径 TOCTOU → 进程崩溃 [ARCH A-1]

**问题**：`dash.rs:143` 和 `:168` 各自独立调用 `ffmpeg_path()`，中间隔着多个 `.await`。若 ffmpeg 在期间被删除，`expect` panic 在 `panic=abort` 下导致**整个应用崩溃**。对比 `hls.rs:883` 已正确使用 `ok_or_else`。

**修复**：单次 `ffmpeg_path()` 绑定 `PathBuf` 贯穿使用，或改 `ok_or_else`。

### 1.3 亮色模式对比度全面不达标 [A11Y P0]

**问题**：`tokens.css` 亮色模式几乎所有语义色对不达标 WCAG AA 4.5:1：text.secondary 2.81:1、text.muted 2.32:1、accent.primary 2.51:1、status.danger 2.67:1。根因：tokens 偏离 DESIGN.md（L=0.50 vs 设计的 L=0.58），且设计值本身也可能不够暗。

**修复**：暗化所有亮色模式色度 token。body 文本目标 L ≤ 0.42（secondary）、L ≤ 0.48（muted）；文本用途 accent 色 L ≤ 0.40；仅用于不透明背景填充按钮的 accent 保持当前值。

### 1.4 语言选择器收口 [ARCH UX-1]

**问题**：en/zh-CN 完整（670 键），es/ja/ko/ru/zh-TW 各缺 ~220 键（~33%），但仍全暴露在选择器中。

**修复**：补齐 220 键前，选择器仅暴露 en/zh-CN，其余标 Beta。`SUPPORTED_LOCALES` 与选择器统一为单一数据源（当前各自硬编码，有漂移风险）。

### 1.5 发布链路端到端演练 [PROJ P0.1]

用测试 tag 触发 Release workflow → 确认 latest.json / .sig / 安装包 / 版本号一致 → 验证更新→安装→relaunch → 发布说明明确 unsigned 风险。

### 1.6 浏览器扩展发布身份 [PROJ P0.2]

替换 Chrome/Edge/Firefox release placeholder ID → 完成正式签名和权限文案 → 建立安装/接管/回退/卸载验证矩阵。Safari wrapper 继续标记未实现。

### 1.7 文档同步 [ARCH Claim-vs-Reality]

- "单任务限速未实现" → 实际已端到端实现（反向漏报），更正
- "任务优先级未实现" → 脚手架齐全但调度未生效，修复后更新文档
- Metalink "验证 MD5/SHA-1/SHA-256/SHA-512" → 每文件仅验最强一个（轻度夸大），措辞修正

### 1.8 BT private torrent 安全修复 [ARCH F-9] NEW(0624)

**问题**：`bt.rs:872` 硬编码 `private: false`，所有 BT 任务强制非私有。私有种子的 `info.private` 字段未被读取，DHT/PEX 不会被禁用——**DHT 泄露私有种子**。

**修复**：从 `.torrent` metadata 读取 `info.private` 字段，private torrent 禁用 DHT/PEX。**最高优先级安全修复**。

### 1.9 onboarding 向导和帮助文档入口 [ARCH UX-9] NEW(0624)

**问题**：全项目无 onboarding/welcome/tour 实现，用户首次启动看不到产品介绍、关键功能位置说明、扩展安装引导。全项目无 help/docs 入口。

**修复**：首次启动显示 3-4 步浮层向导（新建/剪贴板/扩展/快捷键），可跳过，状态存 localStorage；TitleBar 或 Sidebar 底部加"帮助"按钮。

### 1.10 浏览器扩展 UI 国际化 [ARCH UX-10] NEW(0624)

**问题**：`browser/extension-core/src/popup.html`、`options.html`、`background.js` 所有文案硬编码英文，中文用户安装扩展后看到全英文。

**修复**：扩展引入 `chrome.i18n` API + `_locales/` 目录，至少覆盖 en/zh-CN。

### 1.11 批量操作 toast 聚合 [ARCH UX-11] NEW(0624)

**问题**：`toast-store.ts:28-45` 简单 prepend + slice(0,4)，相同错误重复出现；批量删除 50 个任务若 10 个失败会弹 10 个错误 toast。

**修复**：相同 key 的 toast 去重/更新而非新增；批量操作只发最终结果聚合 toast。

### 阶段 1 验证

```bash
pnpm typecheck && pnpm test:frontend && pnpm build
cargo check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
pnpm check:bindings && pnpm test:rust
# 可访问性：手动验证亮色模式所有文本对比度
```

---

## 阶段 2 — 效率与并发（P1，~1–2 周）

目标：消除主要性能瓶颈，让调度、磁盘 I/O、批量操作在高负载下流畅运行。

### 2.1 调度器查询优化 [ARCH E-1]（最高 ROI）

**问题**：`get_settings` 内部是 29 次独立 `SELECT value FROM settings WHERE key=?`。调度循环每次迭代重查 settings + queued list，全程持全局 `scheduler` mutex。填满 8 槽 ≈ **~240 次 DB 往返**全部串行。

**修复**：
1. `get_settings` 改单查询 `SELECT key, value FROM settings` + HashMap（29→1，惠及所有调用方）
2. settings / queued list 提到 loop 外读一次
3. 活跃数 / 每 host 槽位每迭代快照一次
4. spawn IO 前释放全局锁

### 2.2 Checkpoint 写放大 [ARCH E-2]

**问题**：每秒 tick `force=true` 绕过所有 dirty 检查，`UPDATE tasks` + `UPDATE task_files` 无条件执行。8 任务 × 8 段 ≈ **8 commits/s、~80 UPDATE/s**，全部争抢 5 连接池。

**修复**：
1. `RuntimeProgress` 记 `last_written_downloaded/speed`，未变则跳过 tasks + task_files
2. force 路径也尊重 per-segment dirty，仅终态 checkpoint 全量刷
3. 单文件 HTTP 的 task_files 仅完成/选择变更时写

### 2.3 磁盘写 BufWriter [ARCH E-3]

**问题**：裸 `tokio::fs::File` 逐网络 chunk `write_all`，100 MB/s 下每 segment ~1,600–6,400 次 write(2)/s。Windows 上 tokio fs 经 `spawn_blocking`，每 chunk 多一次阻塞线程池派发。

**修复**：每 segment 句柄包 `tokio::io::BufWriter`（256 KiB–1 MiB）；完成/取消前 flush。同构修改 `direct.rs:65,84` 和 `segmented.rs:179`。

### 2.4 限速器原子化 [ARCH E-4]

**问题**：`state.lock().await` 每个 reqwest chunk 取一次，跨并发段共享串行点。250ms cap 未扣减 remaining，低限速下单大 chunk 自旋多次。设限速时 10 MB/s ≈ **160–640 次锁获取/s**。

**修复**：
1. 无锁原子令牌桶（`AtomicI64` + 后台 ticker 补充，CAS 取用）
2. worker 累积每 ~64–256 KiB 或 50ms 调一次 throttle
3. 修 250ms cap 预扣令牌
4. self/parent 只一方有正限速时跳过禁用方加锁

### 2.5 批量操作后端化 [ARCH UX-2/E-5]

**问题**：N 任务 = N 次串行 IPC。删除返回 void → 每次触发全量 `refreshTasks()` → 批量删除 = **2N 串行往返 + N 次全列表重建**。

**修复**：后端 `bulk_task_action(ids, action)` / `bulk_delete_tasks(ids)`，单事务 + 单事件。前端单次 IPC + 单次刷新。

### 2.6 i18n 懒加载 [ARCH E-6]

**问题**：7 种语言全量 static import 进首屏，locale 数据 181KB/52KB gzip，用户只用 1 种。

**修复**：`resources` 初始仅注册 en + 检测到的 locale，其余 `addResourceBundle` 懒加载（切换时 `import('./locales/xx')`）。可从首屏移除 ~150–200 KB raw。

### 2.7 详情面板事件化 [ARCH UX-3] + [PROJ P3.1]

**问题**：5 个二级面板走 10s 拉取。Connections 顶部汇总速度实时跳动，下面每条连接进度条最多滞后 10s。Logs 用户期待流式，实际每 10s 才刷。

**修复**：tab 打开时一次性 fetch，刷新改由该 task.id 的 progress 事件驱动（debounce）；setState 前按 id diff 跳过无变化；定时器保留为 30s fallback。

### 2.8 DASH 三项短板标注 [ARCH F-2]

**问题**：DASH 无续传（暂停=整段重下）、媒体下载绕过代理（泄露真实 IP）、限速被丢弃。三项核心能力同时缺失。

**短期修复**：UI 明确标注 DASH 不支持续传/限速/代理隔离（新建对话框 + 详情面板）。
**长期**：自研分段下载替代 ffmpeg 直拉 MPD（如 HLS 仅用 ffmpeg 做 remux）。

### 2.9 HTTP Client 连接池复用 [ARCH E-8] NEW(0624)

**问题**：`http/mod.rs:155-168` 每次下载都 `build_client` 重建 `reqwest::Client`，同主机多任务重复 TCP/TLS 握手。

**修复**：在 `HttpEngine` 中按 proxy fingerprint 缓存 `Client`（类似 `BtEngine::api_for_output_folder` 的做法），维护 `Arc<RwLock<HashMap<ProxyFingerprint, Client>>>`。预期同主机多任务加速 30-50%。

### 2.10 DNS 缓存 [ARCH E-9] NEW(0624)

**问题**：`build_client` 用系统 DNS resolver，多 segment 下载（8 worker 连同一主机）重复解析 8 次 DNS。

**修复**：用 `hickory-resolver` 作为自定义 resolver 缓存 DNS 结果，reqwest 支持 `.resolver(resolver)`。

### 2.11 speed-history appendBatch 优化 [ARCH E-11] NEW(0624)

**问题**：`speed-history-store.ts:32-41` 每次 `patchTasksBatch` 都 `{ ...current }` 浅拷贝整个 map，对每个 active task 都新建数组即使没有新 sample。

**修复**：只对有新 sample 的 task 创建新数组；或用 immer 风格的 structural sharing。

### 2.12 reqwest Client timeout [ARCH A-9] NEW(0624)

**问题**：`build_client` 设置了 `connect_timeout(30s)` 但无整体 `timeout`，网络挂起时下载 future 永久阻塞。

**修复**：加 `.timeout(Duration::from_secs(60))`（针对无数据传输的整体超时）。

### 2.13 CancellationToken 即时取消 [ARCH A-10] NEW(0624)

**问题**：`DownloadContext.cancel: Arc<AtomicBool>` 协作式轮询，若引擎在 `reqwest::chunk().await` 阻塞，取消信号要等下一个 chunk 才生效。

**修复**：引入 `tokio_util::sync::CancellationToken`，配合 `tokio::select!` 与 `cancel.cancelled()` 实现即时取消。

### 2.14 IME 合成状态守卫 [ARCH UX-12] NEW(0624)

**问题**：`AppShell.tsx:673-697` `isInput` 判断未检查 `event.isComposing`，中文输入法合成拼音时按 K 可能误触发 Mod+K。

**修复**：所有 `matchesShortcut` 调用前加 `event.isComposing` 守卫。

### 2.15 Shift+点击范围选择 [ARCH UX-13] NEW(0624)

**问题**：`TaskRow.tsx:141-144` `onClick` 只调 `onSelect` 单选，选 100 个任务必须逐个点 checkbox。

**修复**：支持 Shift+点击从上次选择到当前的连续范围选择。

### 2.16 完成任务双击打开文件 [ARCH UX-14] NEW(0624)

**问题**：`TaskRow.tsx:141-162` `onClick` 只触发 `onSelect`，下载完成后想打开文件须点击行内按钮或右键。

**修复**：完成状态任务双击直接打开文件。

### 2.17 错误消息本地化 [ARCH UX-15] NEW(0624)

**问题**：`errors.ts:34-39` 直接返回 Rust 英文技术字符串，除 `NewDownloadDialog` 外其他地方直接塞进 toast。

**修复**：扩展 `localizedErrorMessage`（`errors.ts:41-51`）覆盖更多错误码，所有 toast 调用处使用它。

### 2.18 设置搜索定位字段 [ARCH UX-16] NEW(0624)

**问题**：`settings-search.ts:13-22` 只匹配 section 级别，搜"代理端口"会展开 network section 但不会高亮具体字段。

**修复**：为每个字段分配 `data-search-key`，搜索时滚动并高亮匹配字段。

### 阶段 2 验证

```bash
pnpm typecheck && pnpm test:frontend && pnpm build
pnpm check:bindings && pnpm test:rust
cargo check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
# 性能：8 任务并发下载，观察 DB 查询频率、checkpoint UPDATE 频率、syscall 频率
# 批量：选 50 任务批量删除，确认无卡顿
```

---

## 阶段 3 — 数据一致性与安全（P2，~1 周）

目标：消除硬崩溃时的状态不一致风险，堵住凭据和去重的安全缺口。

### 3.1 多表写入事务化 [ARCH A-2]

**问题**：`task_state.rs` 中 `update_task_progress`、`complete_task`（4 个独立写）、`complete_unknown_size_task`（5 个独立写）、`reset_task_download_state` 等函数对 tasks + task_files + task_work_units 发出多条独立 autocommit 语句，无 `BEGIN/COMMIT`。对比 `clear_tasks`、`delete_task_record` 已正确使用事务。

**修复**：比照 `clear_tasks` 模式，将上述函数包进 `pool.begin()` 事务。

### 3.2 去重 UNIQUE 约束 [ARCH A-3]

**问题**：`find_duplicate_task_record` 与 `insert_task_record` 之间隔着多个 `.await`。`tasks` 表对 url/final_url/source_key 无 UNIQUE 约束（仅 BT 的 `info_hash` 有）。并发创建同一 URL 可双双通过检测 → 两个 writer 写同一 `.vibe-downloading` → **文件损坏**。

**修复**：对去重键加 UNIQUE（或活动状态上的 partial unique）索引；或检测+插入包进事务并用 `INSERT...ON CONFLICT`。新增 migration `012_dedup_unique.sql`。

### 3.3 凭据加密加固 [ARCH A-4]

**问题**：
- 加密未绑定 task_id 作为 AAD → 本地有 DB 写权限者可跨任务置换密文
- 明文迁移非事务 → 崩溃后凭据已加密但 URL 仍留明文
- 密文无版本前缀 → 将来换算法无判别位

**修复**：
1. 加密绑定 task_id 作为 AAD
2. 迁移的加密+清洗包进一个事务
3. 加 1 字节版本前缀

### 3.4 启动非阻塞化 [ARCH A-5]

**问题**：setup() 内 9 处串行 `block_on`（DB connect、header 清理、凭据迁移、settings、reset tasks、proxy、browser realtime、schedule），窗口默认可见。慢启动下窗口已出现但内容被阻塞 → 白屏。

**修复**：
1. `tauri.conf.json` 窗口设 `"visible": false`，前端 ready 后再 show
2. 非关键步骤（header 清理、凭据迁移、调度）移到 `tokio::spawn`，仅 DB connect + settings 保留同步

### 3.5 其他鲁棒性修补 [ARCH A-6]

| 子项 | 问题 | 修复 |
|------|------|------|
| DB 池 | 固定 5 < max_active_tasks(8)，争用超 busy_timeout 变错误 | 提到 12–16，或读写分池 |
| set_limit | `try_lock` 静默跳过，争用时不重置 tokens | 改 `lock().await` 或记 warn |
| 跨盘 rename | `fs::rename` C:→D: 直接失败 | 失败回退 copy + fsync + 删除 |
| WS 桥 | 无入站限流 + 默认帧 64MiB + token 明文临时文件 | token 文件设 0600/ACL + 每连接限流 + 显式帧上限 |
| 文件名 | Windows 保留设备名未处理 + 无长度上限 + 双 sanitizer 分歧 | 加保留名检查 + 长度 clamp ~200 + 统一 sanitizer |

### 3.6 生产迁移失败恢复 [ARCH A-7] NEW(0624)

**问题**：`connection.rs:78-84` `should_rebuild_database_after_migration_error` 仅 `cfg!(debug_assertions)` 为 true。生产构建迁移失败直接拒绝启动，用户只能手动删库。

**修复**：实现迁移失败后的备份恢复流程：失败时自动备份损坏库到 `.db.pre-migration-backup`，尝试从备份恢复；迁移前自动备份。

### 3.7 BT sessions LRU 淘汰 [ARCH A-8] NEW(0624)

**问题**：`bt.rs:90-116` `api_for_output_folder` 按 `output_folder|proxy:fingerprint` 缓存 `Arc<Api>`，永不淘汰。任务删除时不清理空 session。

**修复**：引入 LRU 淘汰；或在任务删除时（`delete_runtime_task`）移除空 session。

### 3.8 SSRF 防护 [ARCH A-11] NEW(0624)

**问题**：`browser.rs:600-621` `validate_handoff` 不拒绝 `http://127.0.0.1`、`http://localhost`、`http://169.254.169.254`（云元数据端点）等私有/环回地址。

**修复**：在 handoff 路径增加 SSRF 检查（拒绝私有/链路本地/环回地址），至少对浏览器 handoff 强制。

### 3.9 HLS JoinSet abort_all [ARCH A-12] NEW(0624)

**问题**：`hls.rs:501-506` `workers.join_next().await` 在 `?` 错误传播时未 `abort_all`，可能留下僵尸任务。

**修复**：所有 `?` 错误传播路径统一 `workers.abort_all()`，或用 RAII guard 包裹 JoinSet。

### 3.10 WAL checkpoint 调度 [ARCH A-15] NEW(0624)

**问题**：`connection.rs:50-72` WAL 模式但无 `PRAGMA wal_autocheckpoint` 配置，WAL 文件可能无限增长。

**修复**：启动时若 WAL > 100MB 执行 `PRAGMA wal_checkpoint(TRUNCATE)`；配置 `wal_autocheckpoint`。

### 3.11 临时文件清理 [ARCH A-16] NEW(0624)

**问题**：`segmented.rs:530` 仅在 `initial_downloaded == 0` 时删除 temp；`bt.rs:143-148` probe 目录不清理；HLS staging_dir 删除任务时不清理。

**修复**：定期扫描清理孤儿临时文件；任务删除时清理关联临时目录。

### 3.12 进程退出清理 [ARCH A-17] NEW(0624)

**问题**：`lib.rs:296-321` `on_window_event` 仅设置 `quit_requested` 标志，不等待活跃下载完成或清理。`panic = "abort"` 下依赖 OS 清理。

**修复**：注册 ctrlc handler，退出前 `cancel.store(true)` 所有活跃任务并 `join` 等待 5s。

### 3.13 配置跨字段校验 [ARCH A-18] NEW(0624)

**问题**：`settings.rs:49-150` 数值型 clamp 完善但无跨字段校验：`schedule_download_window_start/end` 不校验 start < end；`completion_run_command` 任意字符串无校验。

**修复**：`update_settings` 时校验跨字段约束。

### 3.14 关键模块测试 [ARCH A-19] NEW(0624)

**问题**：调度器零测试、加密模块零测试、迁移零测试、SFTP TOFU 零测试、HLS/DASH/FTP/Metalink/WebDAV 引擎零集成测试、E2E 完全缺失。

**修复**：优先补调度器、加密、迁移测试；为"创建 HTTP 任务 → 下载 → 完成"主路径加 E2E。

### 阶段 3 验证

```bash
pnpm typecheck && pnpm test:frontend && pnpm build
pnpm check:bindings && pnpm test:rust
cargo check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
# 一致性：模拟进程中断（kill -9），确认 DB 状态一致
# 去重：并发创建同 URL 任务，确认第二个被拒绝
# 启动：大 DB（1k 任务）启动无白屏
```

---

## 阶段 4 — 前端可访问性与主题硬化（P1–P2，~3–5 天）

目标：补全 ARIA 语义、对比度和键盘交互，消除系统性可访问性缺口。

### 4.1 动态内容 ARIA live regions [A11Y P1]

**问题**：多处动态错误/状态消息缺 `role="alert"` 或 `aria-live`：`NewDownloadDialog` 错误 div / submit status / batch results、`TaskRecoveryActions` 错误容器、`TaskDetails` 各面板错误显示。系统性缺口，非孤立遗漏。

**修复**：错误容器统一加 `role="alert"`，状态指示器加 `role="status"` + `aria-live="polite"`。建议建立 utility wrapper 或约定。

### 4.2 index.html lang 动态同步 [A11Y P1]

**问题**：`index.html` 硬编码 `lang="en"`，7 语言应用读屏器用英语发音所有语言内容。

**修复**：`setLocale` 动态设置 `document.documentElement.lang`。

### 4.3 暗色模式 text.muted 对比度 [A11Y P1]

**问题**：暗色模式 `text.muted`（L=0.62）在 `surface.raised`（L=0.235）上 3.44:1，不达标 4.5:1。

**修复**：暗化 `text.muted` 到 L ≥ 0.68，或确保 muted 文本仅用于大尺寸（≥18px 或 bold ≥14px）。

### 4.4 TrayMenu ARIA 菜单语义 [A11Y P2]

**修复**：加 `role="menu"` + `role="menuitem"` + 方向键导航。

### 4.5 StatusBar 播报风暴 [ARCH UX-8a] + [A11Y P2]

**问题**：整条状态栏 `aria-live="polite"`，总速度每秒变化 → 读屏被持续打断。

**修复**：高频数值移出 live region，仅活动/排队计数保留并节流。

### 4.6 Recovery action icons aria-hidden [A11Y P2]

**修复**：装饰性 icon 加 `aria-hidden`。

### 4.7 Tab active 非色彩指示 [A11Y P2]

**问题**：active tab 仅靠微妙背景色变化，低视力用户不可感知。违反 WCAG 1.4.1。

**修复**：加 bottom border、font-weight 增加或 underline。

### 4.8 Settings 硬编码英文 aria-label [ARCH UX-8b]

**修复**：搜索清除按钮、起止时间输入改 `t()` 国际化键。

### 4.9 resize handler debounce [A11Y P2]

**修复**：`use-shell-layout.ts` 加 `requestAnimationFrame` 或 100ms 节流。

### 4.10 notifiedStatuses 清理 [A11Y P2]

**修复**：`use-task-events.ts` 任务移除时 prune Set，或 cap Set size。

### 4.11 主题一致性修补 [A11Y P2–P3]

| 问题 | 修复 |
|------|------|
| TaskRow selected `ring-1` + `border` 双描边 | 选一种 |
| Tooltip/speed menu `shadow-*` + `border` 同时 | tooltip 去 border 留 shadow |
| Palette `opacity-65` vs Button `opacity-50` | 统一为一个 token |
| FloatingStatusWindow 硬编码 oklch shadow | 改命名 token |

### 阶段 4 验证

```bash
pnpm typecheck && pnpm test:frontend && pnpm build
# 可访问性：用读屏器（NVDA/VoiceOver）遍历主要流程
# 对比度：手动验证亮/暗模式所有文本 pair
```

---

## 阶段 5 — 功能补全（P2–P3，~2–3 周）

目标：补完半成品功能，清理死代码，扩展协议能力。

### 5.1 删除可逆 [ARCH UX-4]

**修复**：删文件走系统回收站（`trash` crate）；或延迟物理删除 + 带"撤销"按钮的 toast。

### 5.2 新建对话框补字段 [ARCH UX-5]

**修复**：
- 协议为 FTP/SFTP/WebDAV 时显示凭据字段（密码 `type=password`）
- Advanced 区暴露连接数 / 优先级 / 每任务限速 / 代理覆盖
- 批量结果加"展开全部失败项"

### 5.3 探测错误结构化 [ARCH UX-6]

**修复**：`probeErrorHintKey` 改 `switch (parseAppError(err)?.code)` 复用 `errors.ts` 结构化路径，替代英文子串正则。

### 5.4 限速运行中可改 [ARCH UX-7]

**修复**：允许运行中热改（后端 watch 配置动态 `set_limit`）；右键菜单/行操作加"限速"快捷入口。

### 5.5 死后端能力清理 [ARCH F-3]

**问题**：`classification_rules` 表、`BrowserSiteRule` 模型、`queue_position` 重排、`obey_schedule` 字段全部有 schema 无行为。

**决策**：要么接通（分类规则引擎、站点规则编辑器、队列拖拽重排、obey_schedule UI），要么从 schema/类型移除以免误导。

### 5.6 SFTP 能力补齐 [ARCH F-4]

**修复**：优先补公钥/ssh-agent 认证（创建对话框新增密钥文件选择）；并行分段和递归目录探测作后续。

### 5.7 计划窗口抢占 [ARCH F-5]

**修复**：增加周期性 tick：窗口关闭时暂停 `obey_schedule=true` 的运行任务、窗口内动态 `set_limit`。TaskDetails 暴露 `obey_schedule` 开关。

### 5.8 完成动作扩展 [ARCH F-6]

**修复**：增加"下载完成运行命令/脚本"（全局 + 按任务，传文件路径占位符）；可选 webhook POST。

### 5.9 浏览器集成贯通 [ARCH F-7]

**修复**：打通已嗅探媒体候选到 popup 的一键下载（基础设施已在，死端未接通）；考虑批量链接抓取；站点规则补可视化编辑 UI。

### 5.10 非 HTTP 协议测试 [PROJ P0.3]

为 FTP/FTPS、SFTP、BT、HLS、DASH、WebDAV、Metalink 补充集成测试，覆盖认证失败、代理失败、断点恢复、ffmpeg 缺失等边界场景。统一协议错误分类和恢复动作，继续减少裸字符串错误。验证 ffmpeg 在不同 OS 上的可用性。

### 5.11 重试策略集中化 [PROJ P2.1]

建立共享 retry policy 和错误分类：记录 retry-after、退避原因、最终失败阶段；UI 诊断中展示协议、阶段、重试次数和下一步建议。

### 5.12 其他功能补全 [ARCH F-8]

| 子项 | 修复 |
|------|------|
| HLS 变体选择 | 创建对话框加码率下拉（`hls_variants` 快照已采集但无前端消费） |
| 通用多算法校验 | 创建对话框支持 MD5/SHA-1/SHA-512（后端已支持，UI 只给 SHA-256） |
| 任务导入导出 | 任务列表导出/导入/备份 |
| Metalink 文件改选 | 泛化 BT 的暂停后改选到 Metalink |

### 5.13 下载历史归档 [ARCH F-10] NEW(0624)

**问题**：任务删除即丢失，无历史记录表，无回收站恢复 UI，无按日期/URL/文件名搜索历史。

**修复**：增加 `task_history` 归档表，删除任务时归档元数据；设置页增加历史查看/搜索/恢复 UI。

### 5.14 HLS 字幕/多音轨 [ARCH F-11] NEW(0624)

**问题**：`hls.rs:1264-1289` 仅支持 `NONE` 和 `AES-128`，无 DRM，无字幕注入，无多音轨选择，重试次数低（2 vs HTTP 5）。

**修复**：`finalize_hls_task` 增加 `-map` 选择字幕/音轨；DRM 因法律/技术复杂度暂列长期。

### 5.15 DASH 限制标注 + 长期自研 [ARCH F-12] NEW(0624)

**问题**：`dash.rs:407-415` 硬性拒绝 `type="dynamic"`；完全外包 ffmpeg 无分段管理/并发控制/重试；无续传。

**修复**：短期 UI 明确标注 DASH 不支持续传/限速/代理隔离；长期自研分段下载替代 ffmpeg 直拉。

### 5.16 Metalink 并行镜像 [ARCH F-13] NEW(0624)

**问题**：仅 failover（当前镜像失败才切下一个），aria2 支持 `--mirror` 并行多源下载。

**修复**：实现并行镜像下载（多源同时请求不同字节范围或竞争下载）。

### 5.17 BT Tracker 实时状态 + 做种限制 UI [ARCH F-14] NEW(0624)

**问题**：`tracker_statuses_from_uri` 仅从 magnet URL 解析，status 固定 "configured"，无实时连接状态；做种限制 DB 支持但无前端配置入口。

**修复**：从 librqbit 获取真实 tracker 连接状态；暴露做种比例/时间限制 UI。

### 5.18 代理支持增强 [ARCH F-15] NEW(0624)

**问题**：FTP 无 HTTP 代理支持；FTP ImplicitTls over SOCKS5 不支持；无 PAC 脚本/代理认证/链式/健康检查。

**修复**：短期补 FTP over HTTP 代理；长期考虑 PAC 和代理链。

### 5.19 效率优化批次 [ARCH E-10/13/14/15/16] NEW(0624)

| 子项 | 问题 | 修复 |
|------|------|------|
| HTTP/2 keepalive | 未配置 `.http2_keep_alive_interval` | 加 15s interval + 5s timeout |
| 自动加速参数 | warmup 10s + 15% 波动容忍，真实网络几乎永不触发 | warmup 降到 5s，stability 放宽到 25% |
| ACCEPT_ENCODING | 一刀切 `identity` 禁用压缩 | probe 检测 content-type，text/* 允许压缩 |
| connect_timeout | 30s 偏长，8 worker 各等 30s = 240s | 降到 10-15s |
| cursor total | `total = items.len() + (has_more ? 1 : 0)` 永远 0 或 1 | 用 `SELECT COUNT(*)` 或去掉数字 |

### 5.20 架构改进 [ARCH A-13/14] NEW(0624)

**问题**：错误处理用 `Result<T, String>` 而非类型化错误；日志 guard 遗忘 + 双系统 + 无 metrics。

**修复**：至少为 db 模块引入 `thiserror::Error`；native host guard 持有到进程退出；统一日志系统；为关键函数加 `#[tracing::instrument]`。

### 5.21 效率微修 [ARCH E-17/18/19/20] NEW(0624)

| 子项 | 问题 | 修复 |
|------|------|------|
| BT piece bitfield | 每 10s 全量重写 base64 upsert | 只在 piece count 变化或完成度跨越阈值时更新 |
| 后台 flush | rAF 被节流，fallback 80ms = 每秒 12 次 store 更新 | 后台标签页提到 250ms |
| queue-changed | 100ms debounce 仍可能连续拉取 | debounce 提到 300ms |
| preallocate | 失败只 warn，稀疏文件碎片化 | 对 <16GB 文件强制重试一次 |

### 5.13 E2E 测试补强 [PROJ P2.3]

补设置页、新建下载、命令面板、详情诊断的组件/集成测试；补 Native Messaging、WebSocket bridge、单实例转发、剪贴板捕获的端到端验证。

### 阶段 5 验证

```bash
pnpm typecheck && pnpm test:frontend && pnpm build
pnpm check:bindings && pnpm test:rust
cargo check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
pnpm build:extensions
# 协议：逐协议跑集成测试
# 发布：pnpm tauri build --config src-tauri/tauri.ci.conf.json
```

---

## 穿插修复（可随任意阶段顺手处理）

### 效率微修 [ARCH E-7]

| 问题 | 定位 | 修复 |
|------|------|------|
| HLS 逐段无门发射 + 每发 2 次 DB 写 | `hls.rs:511,941` | 接 `TaskProgressEmitGate` + 1s checkpoint |
| `task_requests` 诊断逐行 insert 且永不清理 | `request_diagnostics.rs:5` | 加保留上限/定时清理 |
| speed/total_size 排序键无索引 → filesort | `task_records.rs:416-418` | 加 `idx_tasks_speed_bps_id`、`idx_tasks_total_size_id` |
| `task_stats` 全表聚合扫描 | `task_records.rs:81` | 改 `GROUP BY status` 走 `idx_tasks_status` |
| TaskRow `describeSpeedTrend` 未 memoize | `TaskRow.tsx:98` | `useMemo` 包裹 |
| `failureOptions` 在 .map 内调 `getState()` | `TaskList.tsx:226` | 循环外 hoist |
| 死代码 `task-live-progress.ts` | — | 删除 |

### 构建与规模化 [PROJ P3.2–P3.3]

- 跑 `pnpm build` 记录前端 bundle 体积，检查字体/图标/语言资源/扩展包大小
- 用 1k/10k 历史任务测试浏览器扩展启动速度和内存占用

---

## 优先级总览

| 阶段 | 预估工时 | 核心风险 | 关键产出 |
|------|---------|---------|---------|
| 1 — P0 闭环 + 崩溃 + 发布 + 安全 | ~1.5 周 | 低 | 优先级生效、无 panic 隐患、亮色对比度合规、发布可演练、BT private 修复、onboarding、扩展 i18n、toast 聚合 |
| 2 — 效率与并发 | ~2 周 | 中（调度器/限速器/HTTP Client 核心路径） | 调度 ~240→~10 DB 往返、checkpoint 写放大消除、批量秒级响应、HTTP 连接池复用、DNS 缓存、即时取消、IME 守卫、Shift 选择 |
| 3 — 数据一致性 + 安全 | ~1.5 周 | 中（DB migration 需旧数据升级测试） | 硬崩溃状态一致、去重 DB 级兜底、凭据安全加固、迁移失败恢复、SSRF 防护、WAL checkpoint、临时文件清理、关键模块测试 |
| 4 — 可访问性 | ~3–5 天 | 低 | ARIA live regions、lang 同步、暗色对比度、菜单语义 |
| 5 — 功能补全 | ~3–4 周 | 高（协议引擎改动） | SFTP 公钥、计划抢占、完成动作扩展、协议测试覆盖、下载历史归档、HLS 字幕/多音轨、Metalink 并行镜像、代理增强、效率优化批次、架构改进 |
| 穿插 | 持续 | 低 | 效率微修、主题一致性、构建体积审计 |
| **合计** | **~8–10 周** | | |

---

## 每阶段通用验证命令

```bash
pnpm typecheck
pnpm test:frontend
pnpm build
pnpm check:bindings
pnpm test:rust
cargo check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings

# 浏览器相关追加
pnpm build:extensions

# 发布相关追加
pnpm tauri build --config src-tauri/tauri.ci.conf.json
```
