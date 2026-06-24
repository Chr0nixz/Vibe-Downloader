# 架构与工程审计

最后更新：2026-06-24

本审计从**用户交互便捷性**、**程序功能丰富性与完整性**、**项目架构的鲁棒性与稳定性**、**程序运行效率**四个维度，对 Vibe Downloader `0.1.1` 的当前代码库进行评估。所有结论均基于**当前工作树的真实源码**逐行核实，行号有效；构建数据来自实跑 `pnpm build`（vite 8 / rolldown）。

- 本文档**不重复** [project-improvement-audit.md](project-improvement-audit.md)（按发布风险组织的前向清单）和 [audit-report.md](audit-report.md)（前端可访问性与主题审计）。
- 本文档聚焦**架构、引擎、调度、效率与交互闭环**，给出可执行的修复计划。
- 每个问题标注：`P0/P1/P2/P3` 优先级、维度（UX/功能/架构/效率）、定位代码、以及核实标签（CONFIRMED 当前代码确认 / FIXED 旧审计项已修复 / CHANGED 结论较旧审计修正 / NEW 本次新增）。

## 2026-06-24 补充审计说明

本次在 2026-06-21 版本基础上，对四个维度进行了更广泛的代码调研，新增了一批此前未覆盖的发现，标记为 `NEW(0624)`。关键新增项：

- **安全 P0**：BT private torrent 标记硬编码 `false`（`bt.rs:872`），可能导致 DHT 泄露私有种子——此前未识别。
- **效率 P0**：HTTP Client 每次下载重建无连接池复用（`http/mod.rs:155-168`），同主机多任务重复 TCP/TLS 握手。
- **架构 P1**：生产环境迁移失败直接拒绝启动且无恢复路径（`connection.rs:78-84` 仅 debug 构建重建）；BT sessions HashMap 永不淘汰（`bt.rs:90-116`）；reqwest Client 未配置 timeout。
- **UX P0**：无 onboarding 向导和帮助文档入口；浏览器扩展 UI 完全无国际化；批量操作 toast 刷屏。
- **功能 P1**：下载历史归档缺失（删除即丢失）；HLS 无 DRM/字幕/多音轨；DASH 硬性拒绝 live。

以下各维度章节中，`NEW(0624)` 标签的条目为本次补充审计新增。

## 本次审计相对上一版的重要修正

代码库自上一版审计后被**显著加固**，旧 `architecture-audit.md` 的多项核心论断已不再成立，特此先行列明，避免误导：

| 旧审计论断 | 当前实测（grep / 源码核实） | 判定 |
|---|---|---|
| 全仓 255 处 `unwrap/expect/panic` 散布 36 文件 | 裸 `.unwrap()` = **0**；`panic!` = 1（仅测试）；生产可达 `expect` = 5（均可证不可达） | **FIXED** |
| 前端 `errors.ts` 主要靠字符串 `.includes()` 反查错误码 | 以 JSON 解析结构化 `code` 为主通道（`errors.ts:27`），字符串匹配仅 JSON 解析失败时兜底 | **CHANGED** |
| 调度器嵌套锁有**死锁**风险 | 全库无 `std::sync::Mutex`；锁序无反转（不存在"持 downloads 取 scheduler"）→ **无经典死锁**，真实风险是吞吐 | **CHANGED** |
| 详情面板 4× 轮询、批量串行、checkpoint 写放大、优先级未闭环 | 当前代码仍存在 | **CONFIRMED** |
| 无前端全局错误边界 | `AppErrorBoundary.tsx` 已挂载于 `main.tsx:16` 包裹全应用 | **FIXED** |
| cursor 分页 / work_units / requests 缺索引 | `010_task_query_indexes.sql` + `001_init.sql` 已提供对应 keyset 复合索引 | **FIXED** |

**`panic = "abort"` 放大器（`Cargo.toml:75`）**：release 构建下任何可达 panic = 整个进程崩溃（`catch_unwind` 失效，tokio 任务内 panic 在 `JoinError` 被观测前即 abort 进程）。这是评估所有 panic 类问题严重度的前提。

## 总体结论

协议广度已达 IDM 级（8 引擎经 trait 统一路由），实时更新链路（rAF 事件批处理 + 每行独立订阅 + 250ms 事件门 + WAL/NORMAL + cursor 分页索引）架构优秀，panic 安全与错误体系远好于旧文档描述。**真正的短板不在"骨架"，而在"编排与闭环"**，集中在四类：

1. **半成品脚手架未闭环**（功能/UX P0）：任务优先级字段/UI/命令齐备，但调度 SQL 不按其排序——是"看起来能用、实则无效"的假特性；分类规则表、站点规则模型、队列重排、`obey_schedule` 均为有数据无行为的死能力。
2. **数据一致性缺口**（架构 P2）：多表进度/完成写入未包事务、去重检测 TOCTOU 且无 UNIQUE 约束、明文凭据迁移非事务且密文无 AAD 绑定。
3. **效率热点**（效率 P1）：checkpoint 写放大、磁盘写无缓冲（逐 chunk syscall）、调度器单 burst ~240 次 DB 往返、限速器逐 chunk 加锁。
4. **交互编排**（UX P1）：翻译缺口 33%（5 语言仍全暴露）、批量操作串行、二级面板 10s 轮询、删除不可逆。

## 已确认优势（作为基线，不展开）

- trait 化 `EngineRegistry` 统一 8 协议；HTTP probe（HEAD + Range GET fallback）、动态加速分段（≤8 段）、续传防损坏（IF_RANGE + Content-Range 三值校验 + 强 ETag/Last-Modified 比对）、checkpoint 事务化持久化、完成态原子改名 + size 校验。
- 取消/暂停先 flush 文件再写 checkpoint，状态干净落盘。
- 全库无阻塞锁跨 await；无锁序死锁；前后端崩溃边界齐备；结构化错误码端到端保真（16 类 `TaskFailureCategory`）。
- 凭据 ChaCha20-Poly1305 + OS keyring（keyring 不可用时失败关闭，无硬编码后备密钥）+ 随机 nonce。
- 单任务限速已端到端实现并跨 HTTP/FTP/SFTP/BT 强制执行（父子令牌桶链）。
- Zustand 三层分解；`@tanstack/react-virtual` 虚拟化 + cursor 分页 + rAF 批处理；speed-history 封顶 60 并清理；lazy route 拆分（设置/详情/对话框非首屏）。
- WS 桥仅绑 127.0.0.1 + per-process 随机 token；native messaging 长度前缀先 bounds-check 再分配；转发头白名单 + CRLF 防注入 + Authorization 拒绝 + 默认关闭。

---

## 一、用户交互便捷性（UX）

### UX-1 五语言缺 ~33% 键仍全暴露在选择器中【P1】CONFIRMED

- **定位**：选择器 `src/components/settings/SettingsPage.tsx:1371-1377`；注册 `src/i18n/index.ts:58-67`；`fallbackLng:"en"` 于 `:69`。
- **现状**：按叶子键精确计数，`en`=670、`zh-CN`=670（完整），`es/ja/ko/ru/zh-TW` **各仅 450 键，缺同一组 ~220 键（~33%）**。`completionDialog`（完成动作对话框）与 `shortcuts`（快捷键面板）两整段在 5 语言中完全缺失。因 `fallbackLng:"en"` 未配 `returnEmptyString`，缺失键回退英文原文（非裸 key），故切到日/韩/俄/西/繁体后这些块整段显示英文。
- **影响**："支持 7 种语言"是误导，实际仅 2 种完整；非英语用户体验割裂。
- **建议**：补齐 220 键；或在补齐前让选择器仅暴露 en/zh-CN，其余标 Beta。注意 `SUPPORTED_LOCALES` 与选择器目前各自硬编码，存在漂移风险，应统一为单一来源。

### UX-2 批量操作串行执行，无后端批量命令【P1】CONFIRMED

- **定位**：`src/components/shell/AppShell.tsx:240-246`（`runBulkTaskAction` 的 `for...of + await`）、`:319-325`（批量删除）。后端无任何 bulk 命令。
- **现状**：`for (const task of selectedTasks) { await runTaskAction(...) }`——N 任务 = N 次串行 IPC。删除返回 void 致每次触发全量 `refreshTasks()`（`listTasksCursor` + store 全替换 + 集合重建）；批量删除 = **2N 串行往返 + N 次全列表重建**。
- **影响**：选 50–100 任务批量删除/暂停明显卡顿，期间不可中断，列表 thrash N 次。
- **建议**：后端 `bulk_task_action(ids, action)` / `bulk_delete_tasks(ids)`，单事务 + 单事件；前端单次 IPC + 单次刷新。

### UX-3 任务详情面板 4× 固定 10s 轮询，二级面板最高滞后 ~10s【P1】CONFIRMED

- **定位**：`SEGMENT_REFRESH_MS=10_000` 于 `src/components/shell/TaskDetails.tsx:46`；4 个定时器 `:289`（segments→Chunks/Connections）、`:326`（Requests）、`:362`（Torrent 快照）、`:458`（Logs）。
- **现状**：核心进度（速度/百分比/ETA）是事件驱动的；但 5 个二级面板走 10s 拉取。定时器已正确门控（仅对应 tab 打开且任务 downloading/retrying 时 arm，首次立即拉一次），故非永久滞后，而是"正在看的那个二级面板最高 ~10s 滞后"。最违和的是 Connections——顶部汇总速度实时跳动，下面每条连接进度条最多滞后 10s；Logs 用户期待流式，实际每 10s 才刷。
- **影响**：观察分块/连接/日志/peer 时数据陈旧，体感卡住；持续无谓 IPC + DB 重扫 + 整数组 setState 触发全量 re-render。
- **建议**：tab 打开时一次性 fetch，刷新改由该 task.id 的 progress 事件驱动（debounce）；至少在 setState 前按 id diff 跳过无变化。

### UX-4 删除不可逆，无回收站/撤销【P1】CONFIRMED

- **定位**：`src-tauri/src/commands/tasks/actions.rs:265-306`（`delete_task` 直接删 DB 记录 +（可选）`std::fs::remove_file`）。全库无 `trash/recycle/undo/soft_delete`。
- **现状**：确认后立即硬删，无软删、无系统回收站、无撤销 toast。
- **影响**：误删（尤其勾"删除文件"且批量时）不可恢复，数据丢失风险高。
- **建议**：删文件优先走系统回收站（`trash` crate）；或删除后给带"撤销"按钮的 toast（延迟物理删除数秒）。

### UX-5 新建对话框缺凭据/代理/连接数/优先级字段【P2】NEW

- **定位**：`src/components/shell/NewDownloadDialog.tsx` 内无 `username/password/proxy/connections` 字段；创建时 `taskSpeedLimitBps/priority/categoryKey` 硬编码为 null（`:340-342`）。
- **现状**：对话框支持 FTP/SFTP/WebDAV 协议探测，却不提供凭据字段——用户只能把密码明文塞进 URL（`sftp://user:pass@host/`），既不安全也难发现；无每任务代理覆盖、连接数、优先级、分类入口（后端均接受）。批量结果仅显示前 5 条，大批量失败项被隐藏。
- **建议**：协议为 FTP/SFTP/WebDAV 时显示凭据字段（密码 `type=password`）；Advanced 暴露连接数/优先级/每任务限速/代理覆盖；批量结果加"展开全部失败项"。

### UX-6 探测错误用英文子串正则分类，而非已有结构化码【P2】CONFIRMED

- **定位**：`NewDownloadDialog.tsx:62-99`（`probeErrorHintKey` 用 `lower.includes("dns")` / `/\b403\b/` 等）。对照：同一对话框重复任务判断已用结构化 `parseAppError(err)?.code`（`:352-353`）。
- **现状**：`errors.ts` 本身设计良好（结构化 code 优先），但 NewDownloadDialog 自建了一套独立英文子串匹配分类探测错误。后端文案一旦本地化或微调，DNS/403/404/429/超时分支会静默退化为通用提示；`includes("rate")` 误命中 "accelerate"、`includes("connect")` 过宽。
- **建议**：改为 `switch (parseAppError(err)?.code)` 选提示文案，复用现成结构化路径。

### UX-7 每任务限速藏在详情面板，运行中只读【P2】NEW

- **定位**：`TaskDetails.tsx` 的 `TaskTransferPanel`（`:891-945`），限速输入 `:966-991`；可编辑条件 `editable = status !== "downloading" && status !== "retrying"`（`:902`）。
- **现状**：每任务限速确实实现了，但不在 Settings、不在新建对话框，只在详情面板；且任务下载中该面板只读，须先暂停才能改。
- **影响**：给正在跑的大文件限速须先暂停→改→恢复，反直觉；在 Settings 里也找不到单任务限速。
- **建议**：允许运行中热改（后端 watch 配置）；右键菜单/行操作加"限速"快捷入口。

### UX-8 其他交互缺口【P3】

- **StatusBar 播报风暴**（NEW，`StatusBar.tsx:33-45`）：整条状态栏是 `aria-live="polite"`，而总速度每秒变化，读屏被持续打断播报速度数字。建议把高频数值移出 live region，仅对活动/排队计数保留并节流。
- **Settings 硬编码英文 aria-label**（NEW，`SettingsPage.tsx:911,1132,1141`）：搜索清除按钮、起止时间输入的可访问名写死英文，破坏 i18n。改为 `t()` 键。
- **完成动作仅 3 种**（见功能 F-6）。
- **无下载统计/历史看板**：仅实时计数与 60 样本内存级 speed-history，无持久化时序/图表。

### UX-9 无 onboarding 向导和帮助文档入口【P0】NEW(0624)

- **定位**：全项目搜索 `onboarding|welcome|getting.started|tour|walkthrough` 仅命中 `CommandBar.tsx:76` 的 firstRunTip（仅指向"新建下载"按钮，4s 自动消失）。
- **现状**：用户首次启动看不到产品介绍、关键功能位置说明、浏览器扩展安装引导。全项目无 `help|documentation|docs` 入口（src/ 下无匹配）。
- **影响**：新用户不知道命令面板、批量导入、扩展集成、快捷键面板等核心功能的存在。
- **建议**：首次启动显示 3-4 步浮层向导（新建/剪贴板/扩展/快捷键），可跳过，状态存 localStorage；TitleBar 或 Sidebar 底部加"帮助"按钮。

### UX-10 浏览器扩展 UI 完全无国际化【P0】NEW(0624)

- **定位**：`browser/extension-core/src/popup.html`、`options.html`、`popup.js`、`options.js`、`background.js`。
- **现状**：popup/options 页面所有文案硬编码英文（"Send current page"、"Auto capture"、"Options"、"Browser Capture Settings"、"Site Rules" 等）；`background.js:111-118` contextMenu title 也硬编码英文。
- **影响**：中文用户安装扩展后看到全英文界面，与桌面 app 的 zh-CN 体验割裂。
- **建议**：扩展引入 `chrome.i18n` API + `_locales/` 目录，至少覆盖 en/zh-CN。

### UX-11 批量操作 toast 刷屏，无去重/聚合【P0】NEW(0624)

- **定位**：`src/stores/toast-store.ts:28-45`（`addToast` 简单 prepend + slice(0,4)）；`AppShell.tsx:236-259`（`runBulkTaskAction` 逐个 `runTaskAction`，每个失败弹一个 toast）。
- **现状**：相同错误的 toast 重复出现；批量删除 50 个任务若 10 个失败会弹 10 个错误 toast，且因 slice(0,4) 只能看到前 4 个。
- **影响**：批量操作失败时用户被 toast 轰炸，且看不到全部失败原因。
- **建议**：相同 key 的 toast 去重/更新而非新增；批量操作只发最终结果聚合 toast（"10 个任务删除失败，点击查看详情"）。

### UX-12 IME 合成状态未守卫，中文输入法误触发快捷键【P1】NEW(0624)

- **定位**：`AppShell.tsx:673-697`（`isInput` 判断只检查 `INPUT/TEXTAREA/contentEditable`，未检查 `event.isComposing`）。
- **现状**：用户在中文输入法合成拼音时按 K，可能误触发 Mod+K 命令面板。
- **建议**：在所有 `matchesShortcut` 调用前加 `event.isComposing` 守卫。

### UX-13 任务列表不支持 Shift+点击范围选择【P1】NEW(0624)

- **定位**：`TaskRow.tsx:141-144`（`onClick` 只调 `onSelect` 单选）；`:178-186`（多选只能通过 checkbox 逐个点）。
- **现状**：选 100 个任务必须逐个点 checkbox。
- **建议**：支持 Shift+点击从上次选择到当前的连续范围选择。

### UX-14 完成任务不支持双击打开文件【P1】NEW(0624)

- **定位**：`TaskRow.tsx:141-162`（`onClick` 只触发 `onSelect`，无双击逻辑）。
- **现状**：下载完成后想打开文件，必须点击行 → 点击行内"打开文件"按钮，或右键 → 打开文件。
- **建议**：完成状态任务双击直接打开文件。

### UX-15 错误消息直接展示 Rust 技术细节，本地化覆盖不足【P1】NEW(0624)

- **定位**：`errors.ts:34-39`（`errorMessage` 直接返回 `payload.message` 或 `error.message`）；对比 `NewDownloadDialog.tsx:63-141` 仅探测错误做了分类提示。
- **现状**：除 `NewDownloadDialog` 外，其他地方（如 `AppShell.tsx:184-192` 的 `runTaskAction`）直接把原始 Rust 英文技术字符串塞进 toast（如 `"failed to probe: connection refused: dns error:..."`）。
- **建议**：扩展 `localizedErrorMessage`（`errors.ts:41-51`）覆盖更多错误码，所有 toast 调用处使用它。

### UX-16 设置搜索只匹配 section 级别，不能定位到具体字段【P1】NEW(0624)

- **定位**：`settings-search.ts:13-22`（`settingsSectionMatchesQuery` 只检查 section 的 id/title/description/summary/terms）。
- **现状**：搜"代理端口"会展开 network section，但不会高亮或滚动到具体字段。
- **建议**：为每个字段分配 `data-search-key`，搜索时滚动并高亮匹配字段。

### UX-17 其他 UX 缺口（P2/P3）NEW(0624)

- **命令面板不支持拼音/模糊匹配**（P2，`Palette.tsx:1063-1068`）：`commandMatches` 只做 `toLowerCase().includes`，输入"xiazai"无法匹配"新建下载"。建议引入简单拼音索引或 fuzzy 匹配。
- **任务行展开状态不持久化**（P2，`task-data-store.ts` 的 `expandedTaskIds`）：刷新页面后所有展开状态丢失。建议按任务 ID 持久化到 localStorage。
- **设置项缺少逐项"重置为默认"**（P2，`SettingsPage.tsx:162-163`）：只有 `showResetDialog` 全局重置，改错某个值后无法单独恢复。
- **拖拽不支持 URL 文本**（P2，`AppShell.tsx:607-654`）：`onFileDrop` 只处理文件路径，不支持从浏览器地址栏拖入 URL 文本。
- **排序方向不可切换**（P2，`CommandBar.tsx:223-246`）：排序下拉只有 6 个固定选项，不能切换同字段的升序/降序。
- **URL 输入框不支持粘贴多链接智能识别**（P2，`NewDownloadDialog.tsx:633-652`）：无 `onPaste` 处理，粘贴含换行的多 URL 被当作单个 URL 提交。
- **浏览器扩展安装无引导**（P2，`SettingsPage.tsx` browser-integration section）：有"安装"按钮但用户不知道下一步该做什么。
- **扩展 service worker 持久重连失效**（P3，`background.js:456-462`）：用 `setTimeout(2_000)` 重连，但 MV3 service worker 30s 不活动会被终止，应用 `api.alarms` API 替代。

**旧审计已修复（如实记录）**：浮窗键盘交互（`FloatingStatusWindow.tsx:91-104` Escape/Enter）、Toast hover 暂停（`toast.tsx:55-101`）、批量删除已用设计系统对话框（非 `window.confirm`）、TaskDetails `<aside>` 可访问名、命令面板/虚拟列表 ARIA（roving tabindex + `aria-activedescendant`）、`prefers-reduced-motion` 全覆盖、设置自动保存三态反馈——均已存在。

---

## 二、程序功能丰富性与完整性

### 协议能力矩阵

| 协议 | 续传 | 并行分段 | 代理 | 鉴权 | 目录 | 完整性校验 | 单任务限速 |
|---|---|---|---|---|---|---|---|
| **HTTP** | ✅ Range | ✅ 动态多连接 | ✅ HTTP/HTTPS/SOCKS5 | URL 内嵌 | ❌ | ⚠️ 仅 SHA-256（完成后） | ✅ |
| **FTP** | ✅ REST | ✅ ≤4，运行中切大段 | ✅ 仅 SOCKS5（隐式 FTPS over SOCKS5 拒绝） | user/pass/匿名；无客户端证书 | ⚠️ 一级，无递归 | ❌ 仅字节核对 | ✅ |
| **SFTP** | ✅ remote seek | ❌ **单流** | ✅ 仅 SOCKS5 | ❌ **仅密码，无密钥/agent** | ⚠️ 一级 | ❌ 仅尺寸核对 | ✅ |
| **BT** | ✅ 重哈希 | ✅ 多 peer | ✅ 仅 SOCKS5 | 磁力/DHT/trackers | n/a | ✅ piece SHA-1 | ✅ |
| **HLS** | ✅ 从已完成分片 | ✅ 并发段 | ✅ | 转发头/cookie | n/a | ❌（仅 AES 解密非校验） | ⚠️ 经分片继承 |
| **DASH** | ❌ **每次从头** | ❌ ffmpeg 内部 | ⚠️ **仅 manifest 走代理，媒体绕过** | `-headers` | n/a | ❌ 仅空输出守卫 | ❌ **被丢弃** |
| **WebDAV** | ✅ 委托 HTTP | ✅ 委托 HTTP | ✅ 委托 HTTP | HTTP Basic | ⚠️ Depth:1 | ❌ | ✅ |
| **Metalink** | ✅ 逐文件 Range | ❌ **全串行**（`supports_parallel:true` 是假标志） | ✅ | 转发头；仅 http(s) 镜像 | n/a | ✅ MD5/SHA1/256/512（仅验最强一个） | ⚠️ 单连接 |

### F-1 任务优先级是"假特性"——对调度零影响【P0】FIXED

- **定位**：队列分发 SQL `src-tauri/src/db/task_records.rs:569`；调度循环 `commands/tasks.rs:314,332`；优先级写入 `commands/tasks/actions.rs:51,69`；`next_queue_position = MAX+1000` 于 `task_records.rs:594-600`；UI `TaskDetails.tsx:992-1009`。
- **现状**：**已修复。** 分发 SQL 现为 `ORDER BY CASE priority WHEN 'high' THEN 0 WHEN 'normal' THEN 1 ELSE 2 END, queue_position ASC, created_at ASC`，priority 正确参与调度顺序。high 优先级任务现在会排在 normal/low 之前派发。
- **影响**：用户切优先级现在能正确插队。

### F-2 DASH 无续传 + 媒体下载绕过代理 + 限速被丢弃【P1】NEW

- **定位**：`src-tauri/src/download/dash.rs:108`（`supports_resume:false`）、`:137-138`（`speed_limiter:_, connection_limit:_` 显式丢弃）、`:161-166`（启动删 temp，`-y` 覆盖）、`:168-191`（ffmpeg 仅带 `-headers`，无 `-http_proxy`）。
- **现状**：DASH 由 ffmpeg 全量下载。暂停 = 整段重下；代理仅用于 manifest 探测，真正媒体流直连不走代理；全局/单任务限速对 DASH 完全无效。
- **影响**：大体积 DASH 视频在代理环境下**泄露真实 IP**（媒体直连）、断点不可恢复、无法限速——三项核心能力同时缺失。
- **建议**：至少 UI 明确标注 DASH 不支持续传/限速/代理隔离；长期考虑自研分段下载替代 ffmpeg 直拉 MPD（如 HLS 那样仅用 ffmpeg 做 remux）。

### F-3 大量"死后端能力"——schema/迁移/模型齐全但无人调用【P2】NEW

均经 grep 确认无读取方：

- **`classification_rules` 表**（`migrations/005_task_transfer_and_integrity.sql:37-50`）：自动分类规则表存在，但 `category_key` 只由用户显式输入设置，无代码读取该表或按文件类型自动归类。
- **`BrowserSiteRule` / 站点规则**：完整模型 + 设置透传齐备（`bindings.ts:281-291`），但无任何前端构造/编辑站点规则 UI。
- **`queue_position` 重排**：`update_task_transfer_options` 接受 `queue_position`，但所有调用方传 null（`TaskDetails.tsx:935`），队列位置只读、无"上移/下移"。
- **`obey_schedule` 按任务豁免**：字段持久化但无 UI（见 F-5）。
- **影响**：迁移与类型成本已付却无行为，对维护者具误导性，对用户是隐形缺失。
- **建议**：要么接通（分类规则引擎、站点规则编辑器、队列拖拽重排），要么从 schema/类型移除以免误导。

### F-4 SFTP 协议能力严重不对称【P2】CONFIRMED

- **定位**：单流 `sftp.rs:154`；仅 `authenticate_password`（`:605-614`，全文无 `authenticate_publickey/agent`）；空用户名直接拒绝（无匿名）`:184-190`；目录仅一级 `:232-282`。
- **现状**：相较 FTP 动态 4 路并行，SFTP 永远单连接；仅用户名/密码，无 SSH 私钥/ssh-agent；目录探测一级、手动逐个挑选、不递归。
- **影响**：SFTP 是企业/Linux 主力协议，缺密钥认证使其在禁用密码登录的服务器上完全不可用；单流使大文件 SFTP 慢于 FTP。
- **建议**：优先补公钥/agent 认证（创建对话框需新增密钥文件字段）；并行分段可作后续。

### F-5 计划窗口只门控新启动，不抢占已运行任务；`obey_schedule` 无 UI【P2】CONFIRMED

- **定位**：窗口判断仅在调度分发循环 `tasks.rs:333-345`（窗口外 `continue`）；限速窗口仅在任务启动时取一次 `:506-523`；无后台窗口监视器；`obey_schedule` 创建时硬编码 true（`create.rs:617`），UI 无开关。
- **现状**：窗口关闭时正在下载的任务继续运行不中断；限速窗口在下载中途开/关不会动态生效（启动时快照一次）；用户无法让单任务豁免计划窗口。
- **影响**：用户设"夜间下载窗口"，白天窗口关闭时已启动的下载照常占用带宽。
- **建议**：增加周期性 tick，窗口关闭时暂停 `obey_schedule=true` 的运行任务、窗口内动态 `set_limit`；TaskDetails 暴露 `obey_schedule` 开关。

### F-6 完成动作仅 None/退出/关机，无 webhook/脚本/按任务后处理【P2】CONFIRMED

- **定位**：`models/task.rs:893-897`（`CompletionAction` 仅三值）；触发 `tasks.rs:621-654`（全局，全部任务完成后触发一次）；设置 UI `SettingsPage.tsx:1206-1219`。
- **现状**：无"运行脚本/命令"、无 webhook、无按任务后处理。全库无 `webhook/run_script/exec_command/post_download`。
- **影响**：相比 aria2 `--on-download-complete`、IDM 完成后运行程序，自动化能力薄弱，无法集成工作流。
- **建议**：增加"下载完成运行命令/脚本"（全局 + 按任务，传文件路径占位符）；可选 webhook POST。

### F-7 浏览器集成：流嗅探是死路、无 POST 鉴权下载、能力多藏在实验开关后【P2】NEW

- **定位**：自动拦截/转发头/cookie/嗅探全部受 `VIBE_BROWSER_EXPERIMENTAL_CAPTURE` 双端门控（`background.js:6`；后端 `browser.rs:383-392` 强制复位）；媒体候选 `recordMediaCandidate` 仅存入 `popupStatus().mediaCandidates`（`background.js:531-551`），**popup.js 从不渲染、`sendDownloadUrl` 从不被媒体候选调用**（死端）；无 `onBeforeRequest/requestBody`；转发头仅 9 项白名单（`browser.rs:47-57`，显式丢弃 authorization/range）；商店 ID 仍占位符。
- **现状**：默认构建下浏览器集成实质仅手动单 URL 移交。HLS/DASH 流被嗅探到却无"一键下载该流"路径；无批量抓取；无 POST 表单体捕获 → 需 POST 鉴权的下载不可用。
- **建议**：打通已嗅探媒体候选到 popup 的一键下载（基础设施已在）；考虑批量链接抓取（content script）；站点规则补可视化编辑 UI。

### F-8 流媒体/多文件选择不可编辑、通用校验仅 SHA-256、无导入导出【P3】

- **HLS 强制最高码率**（NEW，`hls.rs:1103-1123,282`）：`choose_master_variant` 按 `max_by_key` 自动选最高，创建/事后都不可改；`hls_variants` 被采集存快照却无前端消费（死数据）。建议创建对话框加变体下拉。
- **HLS/Metalink 文件选择创建后不可改**（CONFIRMED，`tasks.rs:87-89` 唯一选择命令硬拒非 BT）：仅 BT 支持暂停后改选。建议泛化到 Metalink。
- **通用校验仅 SHA-256**（CONFIRMED，`actions.rs:468`）：MD5/SHA-1/SHA-512 后端支持但创建对话框只给单个 SHA-256 框；Metalink 虽解析四算法但每文件只验最强一个（文档"verifies MD5/SHA-1/SHA-256/SHA-512"措辞暗示全验，实为单验）。
- **无任务列表导出/导入/备份**（NEW）：换机/重装无法迁移下载队列。

### F-9 BT private torrent 标记硬编码 false，DHT 泄露私有种子【P0】NEW(0624)

- **定位**：`src-tauri/src/download/bt.rs:872`（`private: false` 硬编码）。
- **现状**：所有 BT 任务强制标记为非私有。私有种子的 `.torrent` metadata 中 `info.private` 字段未被读取，DHT/PEX 不会被禁用。
- **影响**：**安全隐患**——私有 tracker 的种子可能通过 DHT 泄露给非授权用户，违反 private torrent 协议规范。
- **建议**：从 `.torrent` metadata 读取 `info.private` 字段，private torrent 禁用 DHT/PEX。这是本次审计中**最高优先级的安全修复项**。

### F-10 下载历史归档缺失，删除即丢失【P1】NEW(0624)

- **定位**：`commands/tasks/actions.rs:265-306`（`delete_task` 直接删 DB 记录）；全库无 `history|archive` 表。
- **现状**：任务删除即丢失，无历史记录表，无回收站恢复 UI，无按日期/URL/文件名搜索历史。
- **影响**：误删任务后无法找回下载记录；无法回顾历史下载。
- **建议**：增加 `task_history` 归档表，删除任务时归档元数据；设置页增加历史查看/搜索/恢复 UI。

### F-11 HLS 流媒体功能薄弱：无 DRM/字幕/多音轨【P2】NEW(0624)

- **定位**：`src-tauri/src/download/hls.rs:1264-1289`（`reject_unsupported_media_playlist` 仅支持 `NONE` 和 `AES-128`，显式拒绝 `SAMPLE-AES`）；`finalize_hls_task` 仅 `-c copy` remux。
- **现状**：
  - 仅支持 `NONE` 和 `AES-128` 加密，无 DRM（Widevine/PlayReady/FairPlay），仅接受 `identity` keyformat。
  - 无字幕处理（WebVTT/TTML 不注入）。
  - 无多音轨选择（master playlist 仅按带宽选 variant，无 audio group 选择）。
  - 无直播录制时长限制（仅 `HLS_LIVE_MAX_IDLE_POLLS = 6` 空闲轮询）。
  - 重试次数低（`HLS_SEGMENT_RETRIES = 2` vs HTTP `MAX_SEGMENT_RETRIES = 5`）。
- **建议**：短期补字幕注入和多音轨选择；DRM 因法律/技术复杂度暂列长期。

### F-12 DASH 硬性拒绝 live，完全外包 ffmpeg【P2】NEW(0624)

- **定位**：`src-tauri/src/download/dash.rs:407-415`（`parse_dash_manifest` 硬性拒绝 `type="dynamic"`）；`:177-200`（ffmpeg `-map 0 -c copy` 全量下载）。
- **现状**：
  - 不支持 live DASH（硬性拒绝 `type="dynamic"`）。
  - 完全外包给 ffmpeg：无分段管理、无并发控制、无重试机制。
  - 无字幕/多音轨选择（`-map 0` 全部映射）。
  - 无 DRM 支持（无 ContentProtection 解析）。
  - 无检查点/断点续传（ffmpeg 进程中断即丢失）。
- **建议**：短期在 UI 明确标注 DASH 限制；长期自研分段下载替代 ffmpeg 直拉。

### F-13 Metalink 无并行镜像下载，仅 failover【P2】NEW(0624)

- **定位**：`src-tauri/src/download/metalink.rs`（`usable_metalink_resource` 仅接受 http/https，`resources.sort_by_key` 按 priority 排序后串行 failover）。
- **现状**：仅 failover（当前镜像失败才切下一个），aria2 支持 `--mirror` 并行多源下载。
- **建议**：实现并行镜像下载（多源同时请求不同字节范围或竞争下载）。

### F-14 BT Tracker 状态非实时，做种限制 UI 缺失【P2】NEW(0624)

- **定位**：`bt.rs`（`tracker_statuses_from_uri` 仅从 magnet URL 解析 trackers，status 固定 "configured"）；`bt.rs:875-876`（DB schema 支持 `seed_ratio_limit`/`seed_time_limit_seconds` 但默认 None）。
- **现状**：
  - Tracker 状态非实时：无连接状态/错误/seeders/leechers 实时数据。
  - 做种限制 UI 缺失：DB 支持但无前端配置入口。
  - 无 PEX 显式 API（依赖 librqbit 内部）。
  - 无 DHT 显式配置（仅查询状态，无法配置端口/bootstrap 节点）。
- **建议**：从 librqbit 获取真实 tracker 连接状态；暴露做种比例/时间限制 UI。

### F-15 代理支持差异：FTP 无 HTTP 代理，无 PAC/认证/链式【P2】NEW(0624)

- **定位**：`db/task_proxy.rs:158-180`（`validate_task_proxy_protocol`：BT/FTP/SFTP 仅允许 SOCKS5）；`ftp.rs:1115-1122`（ImplicitTls over SOCKS5 不支持）。
- **现状**：
  - FTP 无 HTTP 代理支持（IDM/aria2 支持 FTP over HTTP 代理）。
  - FTP ImplicitTls over SOCKS5 不支持（仅 ExplicitTls 支持）。
  - 无代理 PAC 脚本支持。
  - 无代理认证 UI（仅 Basic Auth，无 NTLM/Kerberos）。
  - 无代理链式（proxy chain）。
  - 无代理健康检查/自动 failover。
- **建议**：短期补 FTP over HTTP 代理；长期考虑 PAC 和代理链。

### F-16 凭据管理孤立，无 OS keyring 集成/共享/导入导出【P2】NEW(0624)

- **定位**：`db/task_credentials.rs`（ChaCha20-Poly1305 加密，keyring crate 存储 32 字节密钥）。
- **现状**（与 A-4 交叉）：
  - 加密密钥通过 keyring crate 存储，但**无 OS 原生 keyring 直接存储凭据**（凭据本身仍加密存 SQLite）。
  - 无密码管理器集成（1Password/Bitwarden/KeePass）。
  - 无凭据共享（每个任务独立存储，无"凭据库"复用）。
  - 无凭据导入/导出。
- **建议**：增加凭据库（per-host 复用）；考虑密码管理器集成。

### F-17 其他功能缺口（P3）NEW(0624)

- **HLS 强制最高码率且不可改**（P3，`hls.rs:1103-1123`）：`choose_master_variant` 按 `max_by_key` 自动选最高，创建/事后都不可改；`hls_variants` 被采集存快照却无前端消费（死数据）。
- **批量 URL 导入缺失**（P3）：无"粘贴 URL 列表"批量导入 UI，无 URL 自动识别协议。
- **完成动作无"打开文件/文件夹"**（P3）：IDM/aria2 常见功能缺失。
- **文件冲突仅自动重命名**（P3）：无"覆盖/跳过/追加"选项，无文件存在性预检查 UI。

### 声明 vs 现实（Claim-vs-Reality）

| 文档声明 | 出处 | 判定 | 证据 |
|---|---|---|---|
| "单任务限速"未实现 | README:80、AGENTS.md:38 | **已更正** | 实际已端到端实现并强制执行（`speed.rs:41-87`, `tasks.rs:505-528`）；README/AGENTS 已在本批次更正为"已实现" |
| "任务优先级"未实现 | README:80、AGENTS.md:38 | **已修复** | 分发 SQL 已加入 `ORDER BY CASE priority`（`task_records.rs:569`）；README/AGENTS 已更正为"已实现" |
| Metalink 验证 MD5/SHA-1/SHA-256/SHA-512 | README:14,46 | 轻度夸大 | 四算法支持，但每文件仅验最强一个（`metalink.rs:417-435`） |
| "完整文件分类自动化"未实现 | AGENTS.md:38 | 确认，但留死表 | `classification_rules` 表无人读取 |
| 计划窗口不抢占运行传输 | ROADMAP:78 | CONFIRMED | 仅分发循环判断，无抢占路径 |

---

## 三、项目架构的鲁棒性与稳定性

### A-1 ffmpeg 路径 TOCTOU `expect` → 进程崩溃【P2】NEW

- **定位**：`src-tauri/src/download/dash.rs:143`（`ensure_ffmpeg_available()`）→ `:168`（`Command::new(ffmpeg_path().expect("checked above"))`）。
- **现状**：`ensure_ffmpeg_available()` 与第 168 行各自独立调用 `ffmpeg_path()`（后者重读 `VIBE_FFMPEG_PATH` 并 `path.exists()`），两次之间有多个 `.await`（`create_dir_all`/`remove_file`）。对照 `hls.rs:883` 同场景用的是 `ok_or_else`（正确）。
- **影响**：若 ffmpeg 在两次调用之间被删除/移动，第二次返回 None → `expect` panic → **panic=abort 下整个应用崩溃**（非仅该任务）。时序依赖、低概率，但后果是进程级崩溃——本审计中**唯一真实可达的生产 panic 隐患**。
- **建议**：单次 `ffmpeg_path()` 绑定 `PathBuf` 贯穿使用，或改 `ok_or_else`。

### A-2 多表进度/完成/状态写入未包事务【P2】NEW

- **定位**：`src-tauri/src/db/task_state.rs` — `update_task_progress`(75)、`update_task_and_segment_progress`(174)、`update_task_status`(211)、`complete_task`(417)、`complete_unknown_size_task`(492)、`reset_task_download_state`(373) 等。
- **现状**：这些函数对 tasks + task_files + task_work_units（+ event + header 删除）发出多条独立 autocommit 语句，无 `BEGIN/COMMIT`。`complete_task` 含 4 个独立写，`complete_unknown_size_task` 含 5 个。对比 `clear_tasks`(8)、`delete_task_record`(554) **正确**用了事务，注释还明说"否则会留下部分擦除的不一致状态"——同样理由未施于上述函数。
- **缓解**：热路径 `checkpoint_runtime_progress`（`segmented.rs:1285`）是事务化的，故分段下载周期性 checkpoint 一致；完成态原子改名也正确。
- **影响**：硬崩溃时 tasks 计数/状态与 task_files/work_units 可能不一致（如 completed 任务残留 request_headers）。落地范围有限（启动 reset 部分自愈），但严格非原子。
- **建议**：将完成/状态变更函数比照 `clear_tasks` 包进 `pool.begin()`。

### A-3 重复任务检测 TOCTOU + 无 UNIQUE 约束【P2】NEW

- **定位**：检测 `create.rs:510-516`（`find_duplicate_task_record`）→ 插入 `create.rs:632`（`insert_task_record`）；schema `001_init.sql:41` 仅 `CREATE INDEX idx_tasks_source_key`（**非 UNIQUE**）。
- **现状**：检测与插入之间隔着多个 `.await`（`get_settings`、`create_dir_all`、`next_queue_position`）。`tasks` 表对 url/final_url/source_key 无任何 UNIQUE 约束（全表唯一约束仅 `torrent_tasks.info_hash`，只覆盖 BT）。
- **影响**：两个并发 `create_task`（或批量导入循环）对同一 URL 可双双通过检测并插入 → 同一文件两个任务，两个 writer 可能算出相同 `final_path` 并写同一 `.vibe-downloading` 临时文件 → **文件损坏**。HTTP/FTP/SFTP/WebDAV/HLS/DASH 均无 DB 级兜底。
- **建议**：对去重键加 UNIQUE（或活动状态上的 partial unique）索引；或将检测+插入包进事务并用 `INSERT...ON CONFLICT`。

### A-4 凭据加密无 AAD 绑定 + 明文迁移非事务【P2】NEW

- **定位**：加密 `src-tauri/src/secure_headers.rs:22`；迁移 `db/task_credentials.rs:93-160`。
- **现状（逐项核实）**：
  - 密钥/Nonce：`generate_key(&mut OsRng)` 存 keyring，keyring 不可用失败关闭，无硬编码后备；每次 96-bit 随机 nonce 无复用——**良好**。
  - **AAD 缺失**：`cipher.encrypt(&nonce, value.as_bytes())`，`label` 仅用于错误文案，**未绑定 task_id** → 本地有 DB 写权限者可把 task A 的 `(ciphertext, nonce)` 整列搬到 task B 正常解密（密文置换/confused-deputy）。
  - **迁移非事务**：加密插入 `task_credentials`（`:116`）与清洗 URL 列 `update_task_urls`（`:157`）是两条独立 `execute(pool)`，进程在两者之间崩溃 → 凭据已加密存储且 `tasks.url` 仍留明文密码。
  - 密文无版本前缀，将来换算法/加 AAD 无判别位。
- **建议**：(a) 迁移的加密+清洗包进一个事务；(b) 加密绑定 task_id 作为 AAD；(c) 加 1 字节版本前缀。

### A-5 启动期 9 处串行 `block_on` 阻塞主线程 → 白屏【P2】CONFIRMED

- **定位**：`src-tauri/src/lib.rs` setup() 内 `block_on`：416(DB connect)、419(过期 header 清理 + 凭据迁移)、428(get_settings)、431(reset_interrupted_tasks)、438(set_proxy)、456(browser_realtime)、461(schedule_queued_tasks + retry)；窗口配置 `tauri.conf.json:14-23` 无 `"visible": false`。
- **现状**：`setup()` 在主线程串行跑完所有 `block_on` 才返回，而 Tauri 事件循环（渲染 webview）在 `setup()` 返回后才启动；窗口默认可见。其中 `db::connect` 含全部迁移（含 `009` 表重建），`reset_interrupted_tasks` 全表 UPDATE。
- **影响**：慢启动（大 DB、慢迁移、keyring 弹窗、慢盘）下窗口已出现但内容被阻塞 → 白屏/无响应。
- **建议**：窗口设 `"visible": false`，前端 ready 后再 show；或把非关键步骤（header 清理、凭据迁移、调度）移到 `tokio::spawn`，仅 DB connect/settings 保留同步。

### A-6 其他鲁棒性缺口【P3】

- **DB 池固定 5 < max_active_tasks(8)**（CONFIRMED，`connection.rs:51`）：8 任务 + UI 查询竞争 5 连接，WAL 写串行化，超 `busy_timeout=5000` 的争用变成错误而非等待。建议提到 12–16，或读写分池。
- **`set_limit` 用 `try_lock` 静默跳过**（CONFIRMED，`speed.rs:58`）：原子 `limit_bps` 必生效，但争用时不重置 tokens/last_refill，限速短暂 no-op（自愈，无日志）。建议改 `lock().await` 或记 warn。
- **完成态 `fs::rename` 跨盘失效**（NEW，`file.rs:13`）：临时在 C: 保存到 D: 时 Windows `rename` 直接失败 → 报 `disk_write_failed`（可恢复但跨盘下载无法完成）。建议失败回退 copy+fsync+删除。
- **WS 桥无入站限流 + 默认帧 64MiB + token 明文临时文件**（NEW，`browser_realtime.rs:164,184,328`）：持 token 客户端可用不同 request_id 绕过去重洪泛 `createDownload`；帧上限仅靠 axum/tungstenite 默认 64MiB（native-host 路径已限 1MiB，不对称）；token 文件 `std::fs::write` 无受限权限。建议 token 文件设 0600/Windows ACL、加每连接限流、显式设 WS 帧上限。
- **Windows 保留设备名未处理 + 文件名无长度上限 + 双 sanitizer 分歧**（NEW，`sanitize.rs:40-57`）：`CON/PRN/NUL/AUX/COM1-9/LPT1-9`（及 `CON.txt`）原样通过；服务器超长 Content-Disposition 文件名无截断致 `MAX_PATH` 失败；`sanitize.rs` 与 `task_file_planning.rs:158` 两套逻辑略异。建议加保留名检查 + 长度 clamp（~200 字符）+ 统一到单一 sanitizer。
- **部分协议续传跳过远端重校验**（NEW，`tasks.rs:907-915`）：BT/HLS/DASH/Metalink/SFTP 续传前跳过重校验；当服务器既无 ETag 又无 Last-Modified、仅字节数匹配时仍允许续传（`task_resume.rs:158`，仅记 event），同尺寸原地改动会静默损坏（已文档化的权衡）。

### A-7 生产环境迁移失败直接拒绝启动，无恢复路径【P1】NEW(0624)

- **定位**：`src-tauri/src/db/connection.rs:78-84`（`should_rebuild_database_after_migration_error` 仅 `cfg!(debug_assertions)` 为 true）。
- **现状**：生产构建（`debug_assertions = false`）中迁移失败直接返回 `Err(format!("Migration failed: {error}"))`，应用无法启动，用户只能手动删库。Debug 构建中迁移历史不匹配时删除整个数据库重建（`connection.rs:30-41`），数据全部丢失。
- **影响**：生产环境迁移失败 = 用户数据不可恢复，只能手动删库重装。
- **建议**：实现迁移失败后的备份恢复流程：失败时自动备份损坏库到 `.db.pre-migration-backup`，尝试从备份恢复；迁移前自动备份。

### A-8 BT sessions HashMap 永不淘汰，长期运行内存增长【P1】NEW(0624)

- **定位**：`src-tauri/src/download/bt.rs:90-116`（`api_for_output_folder` 按 `output_folder|proxy:fingerprint` 缓存 `Arc<Api>`）。
- **现状**：每个唯一输出目录 + 代理组合累积一个 librqbit session，**永不淘汰**。任务删除时不清理空 session。
- **影响**：长期运行（用户频繁切换输出目录或代理）导致内存持续增长。
- **建议**：引入 LRU 淘汰；或在任务删除时（`delete_runtime_task`）移除空 session。

### A-9 reqwest Client 未配置 timeout，网络挂起永久阻塞调度器【P1】NEW(0624)

- **定位**：`src-tauri/src/download/http/mod.rs:200-231`（`build_client` 设置了 `connect_timeout(30s)` 但无整体 `timeout`）。
- **现状**：连接建立后若服务器挂起（不发数据也不断开），下载 future 永久阻塞，取消信号要等下一个 chunk 才生效。
- **影响**：网络挂起时调度器被阻塞，该任务占用 active slot 却无进展。
- **建议**：加 `.timeout(Duration::from_secs(60))`（针对无数据传输的整体超时）；引入 `tokio_util::sync::CancellationToken` + `tokio::select!` 实现即时取消。

### A-10 取消机制延迟不可控，无 CancellationToken【P1】NEW(0624)

- **定位**：`src-tauri/src/download/engine.rs:57`（`DownloadContext.cancel: Arc<AtomicBool>`）；各引擎在循环中 `cancel.load(Ordering::SeqCst)` 轮询。
- **现状**：协作式 AtomicBool 轮询，若引擎在 `reqwest::chunk().await` 阻塞，取消信号要等下一个 chunk 才生效；网络挂起时可能永久阻塞（与 A-9 叠加）。
- **影响**：用户点暂停/取消后响应延迟不可控。
- **建议**：引入 `tokio_util::sync::CancellationToken`，配合 `tokio::select!` 与 `cancel.cancelled()` 实现即时取消。

### A-11 SSRF 防护缺失，浏览器 handoff 不拒绝私有 IP【P2】NEW(0624)

- **定位**：`src-tauri/src/commands/browser.rs:600-621`（`validate_handoff` 仅校验 HTTP/HTTPS 和无嵌入凭据）；`engine.rs:112-151`（`engine_for_uri` 不校验主机）。
- **现状**：`validate_handoff` 不拒绝 `http://127.0.0.1`、`http://localhost`、`http://169.254.169.254`（云元数据端点）、`http://192.168.*` 等私有/环回地址。
- **影响**：恶意扩展可通过 handoff 让桌面 app 访问内网服务或云元数据端点（SSRF）。
- **建议**：在 handoff 路径增加 SSRF 检查（拒绝私有/链路本地/环回地址），至少对浏览器 handoff 强制。

### A-12 HLS JoinSet 任务泄漏，错误路径未 abort_all【P2】NEW(0624)

- **定位**：`src-tauri/src/download/hls.rs:501-506`（`workers.join_next().await` 在 `?` 错误传播时未 `abort_all`）。
- **现状**：`:515` 和 `:525` 有 abort_all，但 `:510` 的 `db::update_hls_last_media_sequence(...)?` 错误路径未 abort，可能留下僵尸任务。
- **影响**：HLS 下载错误后残留后台任务继续消耗网络资源。
- **建议**：所有 `?` 错误传播路径统一 `workers.abort_all()`，或用 RAII guard 包裹 JoinSet。

### A-13 错误处理用 `Result<T, String>` 而非类型化错误【P2】NEW(0624)

- **定位**：全项目 `Cargo.toml` 无 `anyhow`/`thiserror` 依赖；`src-tauri/src/` 全文搜索无匹配。所有公共 API 使用 `Result<T, String>`，错误信息通过 `.map_err(|e| e.to_string())?` 或 `format!("...: {e}")` 构造。
- **现状**：前端只能字符串匹配错误类型（`errors.ts` 的 `parseAppError` 做 JSON 解析 + 字符串回退）。底层错误是裸字符串，无法区分 `NotFound`/`Conflict`/`Constraint`/`Pool`。
- **影响**：错误处理脆弱，后端文案微调即破坏前端分类。
- **建议**：至少为 db 模块引入 `thiserror::Error` 类型化错误；保留 `AppErrorPayload` 结构化错误码作为前后端契约。

### A-14 日志架构问题：guard 遗忘 + 双系统 + 无 metrics【P2】NEW(0624)

- **定位**：`src-tauri/src/logging.rs:77`（`std::mem::forget(guard)`）；`lib.rs:243-265`（`tauri-plugin-log` 配置）。
- **现状**：
  - native host 的 `non_blocking` guard 被遗忘（`tracing-appender` 文档明确警告），进程退出时缓冲日志可能丢失。
  - 主应用日志无文件 appender，`tracing` 与 `tauri-plugin-log` 是两套系统，可能重复或遗漏。
  - 无 metrics 采集（无 `metrics` crate、无 Prometheus、无 OpenTelemetry）。
  - 无 `#[tracing::instrument]` span 跟踪单任务生命周期。
- **建议**：native host guard 改为持有到进程退出；统一日志系统；为关键函数加 `#[tracing::instrument]`。

### A-15 WAL checkpoint 未调度，WAL 文件可能无限增长【P2】NEW(0624)

- **定位**：`src-tauri/src/db/connection.rs:50-72`（WAL 模式但无 `PRAGMA wal_autocheckpoint` 配置）。
- **现状**：WAL 文件可能随长时间运行无限增长，拖慢启动和备份。
- **建议**：启动时若 WAL > 100MB 执行 `PRAGMA wal_checkpoint(TRUNCATE)`；配置 `wal_autocheckpoint`。

### A-16 临时文件清理不完整【P2】NEW(0624)

- **定位**：`segmented.rs:530`（仅在 `initial_downloaded == 0` 时删除 temp）；`bt.rs:143-148`（`probe_dir = std::env::temp_dir().join("vibe-downloader-bt-probe")`，probe 后不清理）；HLS `staging_dir` 删除任务时不清理。
- **现状**：下载中途失败留下 `.vibe-downloading` 文件；BT probe 目录泄漏；HLS 分段文件残留。
- **建议**：定期扫描清理孤儿临时文件；任务删除时清理关联临时目录。

### A-17 进程退出清理不完整，依赖 OS 清理【P2】NEW(0624)

- **定位**：`src-tauri/src/lib.rs:296-321`（`on_window_event` 仅设置 `quit_requested` 标志，不等待活跃下载完成或清理）。
- **现状**：`panic = "abort"` 下 tokio runtime 直接终止，所有 `tokio::spawn` 任务被丢弃，临时文件、网络连接、子进程（ffmpeg）依赖 OS 清理。
- **建议**：注册 ctrlc handler，退出前 `cancel.store(true)` 所有活跃任务并 `join` 等待 5s。

### A-18 配置无跨字段校验【P2】NEW(0624)

- **定位**：`src-tauri/src/db/settings.rs:49-150`（数值型 clamp 和枚举规范化完善，但无跨字段校验）。
- **现状**：
  - `schedule_download_window_start/end` 不校验 start < end。
  - `schedule_speed_limit_bps` 不校验非空当 window enabled。
  - `completion_run_command` 任意字符串无校验（执行时风险高）。
  - `default_save_dir` 不检查可写/绝对路径。
- **建议**：`update_settings` 时校验跨字段约束。

### A-19 关键模块测试覆盖缺失【P1】NEW(0624)

- **定位**：`src-tauri/tests/`（仅 HTTP/proxy/clipboard/segments 集成测试）。
- **现状**：
  - **调度器零测试**：`schedule_queued_tasks_inner`、`start_task_download`、`check_schedule_preemption` 无任何测试。
  - **加密模块零测试**：`secure_headers.rs` 无 `#[cfg(test)]`，ChaCha20-Poly1305 加解密、keyring fallback、版本字节分发无测试。
  - **迁移零测试**：12 个迁移文件无测试，`012_dedup_unique.sql` 249 行 13 个 DELETE 无覆盖。
  - **SFTP TOFU 零测试**：`verify_or_record_sftp_host_key` 无测试。
  - **HLS/DASH/FTP/Metalink/WebDAV 引擎零集成测试**。
  - **E2E 测试完全缺失**：无 Playwright/Cypress/WebDriver，无 Tauri `tauri-driver`。
- **建议**：优先补调度器、加密、迁移测试；为"创建 HTTP 任务 → 下载 → 完成"主路径加 E2E。

**核实为良好（非问题）**：HTTP 续传整体防损坏到位；取消/暂停状态干净落盘；全库无 `std::sync::Mutex`（无阻塞锁跨 await）；无锁序死锁；前后端崩溃边界齐备；协议引擎（bt/hls/metalink/ftp/sftp/probe）panic 面全部 SAFE，唯一隐患是 A-1。

---

## 四、程序运行效率

构建实测（`pnpm build`，vite 8/rolldown，3.18s）：

| 块 | raw | gzip | 首屏 eager |
|---|---|---|---|
| react-vendor | 182.4 KB | 57.9 KB | ✅ |
| **utils（含 7 国语言全量）** | **181.2 KB** | **52.3 KB** | ✅ |
| index | 153.9 KB | 42.9 KB | ✅ |
| radix-ui | 149.6 KB | 46.4 KB | ✅ |
| framer-motion | 133.0 KB | 43.5 KB | ✅ |
| **首屏 eager JS 合计** | **~861 KB** | **~261 KB** | — |
| SettingsPage/TaskDetails/NewDownloadDialog/Palette | 42.8/34.5/20.3/17.7 KB | — | ❌ 已懒加载 |

lazy route 拆分良好；首屏 261 KB gzip 中，6 种未用语言 + framer-motion(43.5KB gzip) 为可削减死重。

### E-1 调度循环重查 + get_settings 29 次单 key 往返，全程持全局锁【P1】CONFIRMED

- **定位**：`commands/tasks.rs:286`（`scheduler.lock().await`）、`:288`（`loop {`）、`:289-296`（`get_settings`）、`:314`（`list_queued_task_records`）；`db/settings.rs:45-180`。
- **现状**：调度是事件驱动非定时轮询（无空转）——设计正确。但 `loop {}` 每次迭代重查 settings 与 queued list，且 `get_settings` 内是 **29 次独立 `SELECT value FROM settings WHERE key=?`**。整个 `schedule_queued_tasks_inner` 持全局 `scheduler` mutex 含全部 DB 与 `start_task_download` IO；`host_connection_slots` 对每个 queued 任务重锁 downloads 并遍历全部值 → O(active×queued)。
- **影响**：填满 8 槽的一次调度 ≈ 8 迭代 × (29 settings + 1 list) ≈ **~240 次 DB 往返**，全部串行在一把锁后；8 任务同时完成 → 8 次 re-kick × 29 = 232 次额外 settings 查询。
- **建议**：① settings/queued list 提到 loop 外读一次；② `get_settings` 改单查询 `SELECT key,value FROM settings` + HashMap（29→1，惠及所有调用方）；③ 活跃数/每 host 槽位每迭代快照一次；④ spawn IO 前释放全局锁。**最高 ROI 项**。

### E-2 Checkpoint 写放大：每秒无条件重写 tasks + task_files，force 路径忽略 dirty【P1】CONFIRMED

- **定位**：`download/http/segmented.rs:1262-1343`；驱动 `:591`（`interval(from_secs(1))`）、`:263`。
- **现状**：两道 guard 仅在 `!force` 时生效；每秒 tick `force=true` 绕过，`UPDATE tasks` 与 `UPDATE task_files ... WHERE selected=1` 每次无条件执行，无"值未变则跳过"；per-segment 过滤器 `force || segment.dirty`（`:1321`）在 force 下退化为写全部 segment。停滞/恒速/暂停中的任务也照写。
- **影响**：每活跃任务 1 事务/秒 = `1+1+M` 行写，N=8×M=8 → **8 commits/s、~80 UPDATE/s**，全部争抢 5 连接池。
- **建议**：① `RuntimeProgress` 记 `last_written_downloaded/speed`，未变则跳过 tasks+task_files；② force 路径也尊重 per-segment dirty，仅终态 checkpoint 全量刷；③ 单文件 HTTP 的 task_files 冗余，仅完成/选择变更时写。

### E-3 磁盘写无 BufWriter：逐网络 chunk 一次 write_all syscall【P1】NEW

- **定位**：`download/http/segmented/worker.rs:273`（裸 `tokio::fs::File`）、`:345`（`write_all` 在 chunk 循环内）；同构于 `direct.rs:65,84`、`segmented.rs:179`。HTTP 路径全程无 BufWriter。
- **现状**：`while let Some(chunk) = response.chunk().await` → 每帧一次 `write_all` 直写 OS 句柄；flush 仅在取消与循环结束调用。
- **影响**：100 MB/s 下每 segment **~1,600–6,400 次 write(2)/秒** × M 段；Windows 上 tokio fs 经 `spawn_blocking`，每 chunk 多一次阻塞线程池派发。
- **建议**：每 segment 句柄包 `tokio::io::BufWriter`（256 KiB–1 MiB）；segment range 互不重叠各自安全；完成/取消前 flush。对高速大文件 CPU/syscall 收益最大。

### E-4 限速器逐 chunk 共享 Mutex + 250ms cap 未扣减重循环【P1】CONFIRMED

- **定位**：`download/speed.rs:82-119`；调用 `segmented/worker.rs:344`、`segmented.rs:178`、`direct.rs:83`。
- **现状**：`state.lock().await`（`:99`）每个 reqwest chunk 取一次，limiter 是单个 `Arc` 克隆进所有 worker 故跨并发段共享串行点；有 parent 时双重加锁。250ms cap（`:116`）走 else 分支返回 `Some(wait)` 时未扣减 `remaining`，低限速下单大 chunk 自旋多次。`limit<=0` 在 `:93` 提前返回——不限速的快速下载完全跳过锁与 sleep（干净快路径）。
- **影响**：设限速时 10 MB/s 约 **160–640 次锁获取/秒**全部漏斗进一个 mutex，跨每个并发段。
- **建议**：① 无锁原子令牌桶（`AtomicI64` + 后台 ticker 补充，CAS 取用）；② worker 累积每 ~64–256 KiB 或 50ms 调一次 throttle；③ 修 250ms-cap 预扣令牌；④ self/parent 只一方有正限速时跳过禁用方加锁。

### E-5 批量操作串行 per-task IPC【P1】CONFIRMED

见 UX-2（同一问题的效率视角）：批量删除 = 2N 串行往返 + N 次全列表重建。建议后端批量命令；若暂不动后端，至少 `Promise.allSettled` 并行 + 删除循环后只 refetch 一次。

### E-6 i18n 7 国语言全量 eager 进首屏【P1】CONFIRMED

- **定位**：`src/i18n/index.ts:4-10`（7 个静态 import）、`:58-67`（全量注册）。locale 数据落在 `utils-*.js`（181KB/52KB gzip），`dist/index.html` 对其 `modulepreload` eager 预载。源码 locale 合计 ~245 KB（4465 行），用户只用 1 种。
- **影响**：首屏多解析 ~6 种语言数据；i18next 运行时遍历全部 resources。
- **建议**：`resources` 初始仅注册 en + 检测到的 locale，其余 `addResourceBundle` 懒加载（切换时 `import('./locales/xx')`）。可从首屏移除 ~150–200 KB raw。

### E-7 其他效率热点【P2/P3】

- **HLS 逐段无门发射 + 每发 2 次 DB 写**（P2 NEW，`hls.rs:511,941`）：不经 `TaskProgressEmitGate`，每完成一段发一次事件 + 写 2 次 DB；K 并发段 → K emit + 2K 写/批。建议接 EmitGate + 1s checkpoint 节奏；DASH/SFTP/Metalink/BT 各自临时常量也建议统一走 gate。
- **`task_requests` 诊断逐行 insert 且永不清理**（P2 NEW，`request_diagnostics.rs:5`）：每请求一行独立隐式事务，长 HLS/live 或高重试下无界增长，拖慢 checkpoint 与备份。建议加保留上限/定时清理。
- **speed/total_size 排序键无索引 → filesort**（P2 NEW，`task_records.rs:416-418`）：浮窗默认按 speed 排序，10k+ 行每页 O(n log n)。建议加 `idx_tasks_speed_bps_id`、`idx_tasks_total_size_id`。
- **TaskRow `describeSpeedTrend` 未 memoize**（P3 NEW，`TaskRow.tsx:98`）：~15 活跃行每帧重算 slice/两次 reduce/min-max + i18n 查找。`useMemo` 包裹（活跃列表最高价值的一行修复）。
- **failureOptions 在 .map 内调 getState()**（P3 CONFIRMED，`TaskList.tsx:226`）：每 taskId 取全 store 快照。循环外 hoist 即可。
- **task_stats 全表聚合扫描**（P3 NEW，`task_records.rs:81`）：下载中约 1Hz 全表 `COUNT/SUM`。改 `GROUP BY status` 走 `idx_tasks_status`。
- **死代码 `task-live-progress.ts`**：重复实现 speed-history（第二处真相源），建议删除。

### E-8 HTTP Client 每次下载重建，无连接池复用【P0】NEW(0624)

- **定位**：`src-tauri/src/download/http/mod.rs:155-168`（`HttpEngine::download()` 每次调用 `build_client(&context.proxy_config)?`）；`:200-231`（`build_client` 构造新 `reqwest::Client`）。
- **现状**：`reqwest::Client` 内部持有连接池和 keep-alive 状态，但每次下载都重建。每个任务、每次重试都重新 TCP/TLS 握手；同主机多任务无法复用连接。自动加速 split 的新 segment worker 在 `run_segmented_download` 内共享同一 client，但跨任务完全不共享。
- **影响**：同主机多任务重复 TCP/TLS 握手，延迟和 CPU 开销显著增加。
- **建议**：在 `HttpEngine` 中按 proxy fingerprint 缓存 `Client`（类似 `BtEngine::api_for_output_folder` 在 `bt.rs:76-116` 的做法），维护 `Arc<RwLock<HashMap<ProxyFingerprint, Client>>>`。预期同主机多任务加速 30-50%。

### E-9 DNS 缓存缺失，多段下载重复解析【P1】NEW(0624)

- **定位**：`src-tauri/src/download/http/mod.rs:200-231`（`build_client` 用系统 DNS resolver，无自定义 resolver）。
- **现状**：多 segment 下载（8 个 worker 连同一主机）会重复解析 8 次 DNS。
- **建议**：用 `hickory-resolver` 作为自定义 resolver 缓存 DNS 结果，reqwest 支持 `.resolver(resolver)`。

### E-10 HTTP/2 keepalive 未配置，可能回退 HTTP/1.1【P2】NEW(0624)

- **定位**：`src-tauri/src/download/http/mod.rs:200-231`（`build_client` 无 `.http2_keep_alive_interval`）。
- **现状**：reqwest 默认 ALPN 协商 HTTP/2，但未显式配置 keepalive，某些场景回退 HTTP/1.1，丢失多路复用收益。
- **建议**：加 `.http2_keep_alive_interval(Duration::from_secs(15))` 和 `.http2_keep_alive_timeout(Duration::from_secs(5))`。

### E-11 speed-history appendBatch 每次新建顶层对象，无 structural sharing【P1】NEW(0624)

- **定位**：`src/stores/speed-history-store.ts:32-41`（`appendBatch` 每次 `{ ...current }` 浅拷贝整个 map，`next[taskId] = [...]` 对每个 active task 都新建数组）。
- **现状**：每次 `patchTasksBatch`（每帧最多一次）都让 `history` 引用变化。`appendBatch` 内部 `[...existing.slice(1), sample]` 对每个 active task 都新建数组，即使该 task 这一帧没有新 sample 也会被波及（因为 `next[taskId]` 被重新赋值）。
- **建议**：在 for 循环里先判断 `entries` 是否包含该 taskId，只对有新 sample 的 task 创建新数组；或用 immer 风格的 structural sharing。

### E-12 TaskList subscribe 每次遍历所有 taskIds 两次【P2】NEW(0624)

- **定位**：`src/components/tasks/TaskList.tsx:115-137`（`useTaskDataStore.subscribe` 在每次 store 变化时遍历 `state.taskIds` 两次构建状态播报）。
- **现状**：每次 `patchTasksBatch`（每帧）触发 subscriber，1000 任务 = 2000 次循环 + Map 查找。
- **建议**：用 `subscribeWithSelector` 中间件，只在 `state.taskIds` 或 `state.taskById` 引用变化时触发；或把状态变化检测收敛到 `patchTasksBatch` 内部。

### E-13 自动加速参数过于保守，真实网络几乎永不触发【P2】NEW(0624)

- **定位**：`src-tauri/src/download/http/segmented.rs:1133-1150`（`AUTO_ACCELERATION_WARMUP = 10s` + `STABILITY_WINDOW = 5` + 15% 波动容忍）；`:974-1131`（`maybe_accelerate_segments`）。
- **现状**：`speed_is_stable` 要求 5 个采样全部 >0 且 `(max-min) <= average * 0.15`，对波动稍大的真实网络几乎永远不满足。每次 split 后等 `AUTO_ACCELERATION_EVALUATION = 5s` 评估，最多 8 段意味着 15 + 5*7 = 50s 才能加速到上限。
- **建议**：warmup 降到 5s，stability 放宽到 25% 或用中位数替代 min/max，evaluation 降到 3s。

### E-14 ACCEPT_ENCODING: identity 一刀切禁用压缩【P2】NEW(0624)

- **定位**：`src-tauri/src/download/http/request.rs:20,45`（所有 HTTP 请求强制 `ACCEPT_ENCODING: identity`）；`hls.rs:643` 同样。
- **现状**：对文本类下载（HTML/JSON/CSV）浪费带宽。虽然对二进制文件压缩无意义甚至有害（无法分段），但一刀切禁用不合适。
- **建议**：在 probe 阶段检测 content-type，对 text/* 和 application/json 允许压缩；对 binary 保持 identity。

### E-15 connect_timeout 30s 偏长，不可达主机无效等待【P2】NEW(0624)

- **定位**：`src-tauri/src/download/http/mod.rs:204`（`.connect_timeout(Duration::from_secs(30))`）。
- **现状**：不可达主机 30s 才超时，8 个 segment worker 各等 30s = 240s 无效等待。
- **建议**：降到 10-15s，或对重试过的 segment 用更短的超时。

### E-16 cursor 分页 total 估算粗糙，前端"还有 N 条"永远 0 或 1【P2】NEW(0624)

- **定位**：`src-tauri/src/db/task_records.rs:297-301`（`total = items.len() + (has_more ? 1 : 0)`）；`TaskList.tsx:586`（`count: Math.max(0, total - filtered.length)`）。
- **现状**：前端显示的"还有 N 条"永远是 0 或 1，无法显示真实剩余数量。
- **建议**：用 `SELECT COUNT(*)` 单独算 total（cursor 模式下可缓存），或前端把 "loadingMore" 文案改为不带数字。

### E-17 BT piece bitfield 每 10s 全量重写【P3】NEW(0624)

- **定位**：`src-tauri/src/download/bt.rs:790-801`（`torrent_piece_bitfield` 调用 `api_dump_haves` 拿 `Vec<bool>` 编码 base64 upsert）。
- **现状**：每次 BT 进度更新（10s 间隔）都重新 dump + base64 + upsert。100GB 种子（256KB piece）= 400000 pieces = 50KB bitfield = 67KB base64。
- **建议**：只在 piece count 变化或完成度跨越阈值时更新 bitfield。

### E-18 useTaskEvents progress flush 后台标签页 80ms 太短【P3】NEW(0624)

- **定位**：`src/hooks/use-task-events.ts:153-199`（`scheduleProgressFlush` 用 `requestAnimationFrame` + 80ms fallback timer）。
- **现状**：后台标签页 rAF 被节流到 1Hz，fallback 80ms 接管 = 每秒 12 次 store 更新。
- **建议**：后台标签页时把 fallback 提到 250ms（与后端 emit gate 对齐），用 `document.visibilityState` 检测。

### E-19 queue-changed 事件触发全量 listTasksCursor 重新加载【P3】NEW(0624)

- **定位**：`src/hooks/use-task-events.ts:227-250`（`onQueueChanged` → `setTimeout(100ms)` → `listTasksCursor(null)`）。
- **现状**：快速 pause/resume 多个任务会触发多次 queue-changed，100ms debounce 仍可能连续拉取。
- **建议**：debounce 提到 300ms，或在 patchTasksBatch 已知变化时跳过全量刷新。

### E-20 preallocate 失败只 warn 不重试，稀疏文件碎片化【P3】NEW(0624)

- **定位**：`src-tauri/src/download/http/file.rs:63-78`（`set_len` 失败时只 `tracing::warn!` 然后继续）。
- **现状**：后续多 worker 并发写未预分配稀疏文件，NTFS/ext4 上造成大量碎片，降低顺序写性能。
- **建议**：失败时记录任务标记，下载完成后做碎片整理提示；或对 <16GB 文件强制重试一次。

**核实为良好（避免误报）**：cursor 分页索引齐全（`010_*`）；work_units/requests 有 task_id 索引；实时 UI 走 cursor 不含 count()；`synchronous=NORMAL`+WAL 最优；speed-history 已封顶 60 并清理；progress 事件 rAF 批处理 ≤1 set/帧；每行独立订阅只重渲变更行；列表已虚拟化。

---

## 优先级总览

| 优先级 | 编号 | 问题 | 维度 | 标签 |
|--------|------|------|------|------|
| 🔴 P0 | F-1 | 任务优先级字段存在但调度不按其排序（假特性） | 功能 | CONFIRMED |
| 🔴 P1 | E-1 | 调度循环重查 + get_settings 29 次往返/持全局锁 | 效率/架构 | CONFIRMED |
| 🔴 P1 | E-2 | checkpoint 写放大（无条件 tasks/files + force 忽略 dirty） | 效率 | CONFIRMED |
| 🔴 P1 | E-3 | 磁盘写无 BufWriter（逐 chunk syscall） | 效率 | NEW |
| 🔴 P1 | E-4 | 限速器逐 chunk 加锁 + 250ms 重循环 | 效率 | CONFIRMED |
| 🔴 P1 | UX-2/E-5 | 批量操作串行 IPC，无 bulk 命令 | UX/效率 | CONFIRMED |
| 🔴 P1 | UX-1 | 5 语言缺 33% 键仍全暴露 | UX | CONFIRMED |
| 🔴 P1 | UX-3 | 详情面板 4×10s 轮询未事件化 | UX/效率 | CONFIRMED |
| 🔴 P1 | UX-4 | 删除不可逆，无回收站/撤销 | UX | CONFIRMED |
| 🔴 P1 | F-2 | DASH 无续传 + 媒体绕过代理 + 丢弃限速 | 功能 | NEW |
| 🔴 P1 | E-6 | 7 国语言全量 eager 进首屏 | 效率 | CONFIRMED |
| 🟡 P2 | A-1 | ffmpeg 路径 TOCTOU expect（进程崩溃） | 架构 | NEW |
| 🟡 P2 | A-2 | 多表进度/完成写入未包事务 | 架构 | NEW |
| 🟡 P2 | A-3 | 重复任务 TOCTOU + 无 UNIQUE 约束 | 架构 | NEW |
| 🟡 P2 | A-4 | 凭据无 AAD 绑定 + 明文迁移非事务 | 架构 | NEW |
| 🟡 P2 | A-5 | 启动 9 处 block_on 阻塞主线程 | UX/架构 | CONFIRMED |
| 🟡 P2 | F-3 | 死后端能力（分类/站点规则/队列重排/obey_schedule） | 功能 | NEW |
| 🟡 P2 | F-4 | SFTP 单流 + 仅密码 + 无递归 | 功能 | CONFIRMED |
| 🟡 P2 | F-5 | 计划窗口不抢占运行任务 + 无 obey_schedule UI | 功能 | CONFIRMED |
| 🟡 P2 | F-6 | 完成动作仅退出/关机，无 webhook/脚本 | 功能 | CONFIRMED |
| 🟡 P2 | F-7 | 浏览器流嗅探死路、无 POST 鉴权 | 功能 | NEW |
| 🟡 P2 | UX-5/6/7 | 新建缺字段 + 探测字符串分类 + 限速藏深 | UX | NEW/CONFIRMED |
| 🟡 P2 | E-7a/b/c | HLS 无门发射 / task_requests 无清理 / 排序无索引 | 效率 | NEW |
| 🟢 P3 | A-6 | DB 池 5 / set_limit try_lock / 跨盘 rename / WS 限流 / 文件名 sanitizer | 架构 | 混合 |
| 🟢 P3 | F-8 | HLS 变体不可选 / 通用校验仅 SHA-256 / 无导入导出 | 功能 | 混合 |
| 🟢 P3 | E-7d/e/f | TaskRow memo / failureOptions / task_stats / 死代码 | 效率 | 混合 |
| 🔴 P0 | F-9 | BT private torrent 硬编码 false，DHT 泄露私有种子 | 功能/安全 | NEW(0624) |
| 🔴 P0 | E-8 | HTTP Client 每次重建无连接池复用 | 效率 | NEW(0624) |
| 🔴 P0 | UX-9 | 无 onboarding 向导和帮助文档入口 | UX | NEW(0624) |
| 🔴 P0 | UX-10 | 浏览器扩展 UI 完全无国际化 | UX | NEW(0624) |
| 🔴 P0 | UX-11 | 批量操作 toast 刷屏，无去重/聚合 | UX | NEW(0624) |
| 🔴 P1 | A-7 | 生产迁移失败直接拒绝启动，无恢复路径 | 架构 | NEW(0624) |
| 🔴 P1 | A-8 | BT sessions HashMap 永不淘汰 | 架构 | NEW(0624) |
| 🔴 P1 | A-9 | reqwest Client 未配置 timeout | 架构 | NEW(0624) |
| 🔴 P1 | A-10 | 取消机制延迟不可控，无 CancellationToken | 架构 | NEW(0624) |
| 🔴 P1 | A-19 | 关键模块测试覆盖缺失（调度器/加密/迁移/E2E） | 架构 | NEW(0624) |
| 🔴 P1 | F-10 | 下载历史归档缺失，删除即丢失 | 功能 | NEW(0624) |
| 🔴 P1 | E-9 | DNS 缓存缺失，多段下载重复解析 | 效率 | NEW(0624) |
| 🔴 P1 | E-11 | speed-history appendBatch 无 structural sharing | 效率 | NEW(0624) |
| 🔴 P1 | UX-12 | IME 合成状态未守卫，误触发快捷键 | UX | NEW(0624) |
| 🔴 P1 | UX-13 | 任务列表不支持 Shift+点击范围选择 | UX | NEW(0624) |
| 🔴 P1 | UX-14 | 完成任务不支持双击打开文件 | UX | NEW(0624) |
| 🔴 P1 | UX-15 | 错误消息直接展示 Rust 技术细节 | UX | NEW(0624) |
| 🔴 P1 | UX-16 | 设置搜索只匹配 section 级别 | UX | NEW(0624) |
| 🟡 P2 | A-11 | SSRF 防护缺失，handoff 不拒绝私有 IP | 架构/安全 | NEW(0624) |
| 🟡 P2 | A-12 | HLS JoinSet 任务泄漏，错误路径未 abort_all | 架构 | NEW(0624) |
| 🟡 P2 | A-13 | 错误处理用 Result<T,String> 而非类型化错误 | 架构 | NEW(0624) |
| 🟡 P2 | A-14 | 日志架构：guard 遗忘 + 双系统 + 无 metrics | 架构 | NEW(0624) |
| 🟡 P2 | A-15 | WAL checkpoint 未调度，WAL 文件无限增长 | 架构 | NEW(0624) |
| 🟡 P2 | A-16 | 临时文件清理不完整 | 架构 | NEW(0624) |
| 🟡 P2 | A-17 | 进程退出清理不完整，依赖 OS 清理 | 架构 | NEW(0624) |
| 🟡 P2 | A-18 | 配置无跨字段校验 | 架构 | NEW(0624) |
| 🟡 P2 | F-11 | HLS 无 DRM/字幕/多音轨 | 功能 | NEW(0624) |
| 🟡 P2 | F-12 | DASH 硬性拒绝 live，完全外包 ffmpeg | 功能 | NEW(0624) |
| 🟡 P2 | F-13 | Metalink 无并行镜像下载，仅 failover | 功能 | NEW(0624) |
| 🟡 P2 | F-14 | BT Tracker 状态非实时，做种限制 UI 缺失 | 功能 | NEW(0624) |
| 🟡 P2 | F-15 | 代理支持差异：FTP 无 HTTP 代理，无 PAC/认证/链式 | 功能 | NEW(0624) |
| 🟡 P2 | F-16 | 凭据管理孤立，无共享/导入导出 | 功能 | NEW(0624) |
| 🟡 P2 | E-10 | HTTP/2 keepalive 未配置 | 效率 | NEW(0624) |
| 🟡 P2 | E-12 | TaskList subscribe 每次遍历所有 taskIds 两次 | 效率 | NEW(0624) |
| 🟡 P2 | E-13 | 自动加速参数过于保守 | 效率 | NEW(0624) |
| 🟡 P2 | E-14 | ACCEPT_ENCODING identity 一刀切禁用压缩 | 效率 | NEW(0624) |
| 🟡 P2 | E-15 | connect_timeout 30s 偏长 | 效率 | NEW(0624) |
| 🟡 P2 | E-16 | cursor 分页 total 估算粗糙 | 效率 | NEW(0624) |
| 🟡 P2 | UX-17 | 拼音匹配/展开持久化/逐项重置/URL拖拽/排序方向等 | UX | NEW(0624) |
| 🟢 P3 | F-17 | HLS 强制最高码率/批量URL导入/完成动作/文件冲突 | 功能 | NEW(0624) |
| 🟢 P3 | E-17 | BT piece bitfield 每 10s 全量重写 | 效率 | NEW(0624) |
| 🟢 P3 | E-18 | useTaskEvents progress flush 后台 80ms 太短 | 效率 | NEW(0624) |
| 🟢 P3 | E-19 | queue-changed 事件触发全量刷新 | 效率 | NEW(0624) |
| 🟢 P3 | E-20 | preallocate 失败只 warn，稀疏文件碎片化 | 效率 | NEW(0624) |

---

## 修复计划（分阶段，每阶段独立可合入）

每阶段结束跑完整验证：

```bash
pnpm typecheck && pnpm test:frontend && pnpm build
pnpm check:bindings && pnpm test:rust
cargo check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
```

### 阶段 1 — 闭环既有功能 + 消除进程崩溃（P0 + 关键 P2，~1 周，多为小改动）

目标：让"看起来能用的功能真正生效"，消除唯一进程崩溃隐患。

1. **F-1 优先级闭环**：分发 SQL 补 `ORDER BY CASE priority...`，加单测，UI 实测插队。一行 SQL。
2. **A-1 ffmpeg expect**：`dash.rs:168` 改 `ok_or_else`，单次 `ffmpeg_path()` 绑定贯穿。
3. **E-1 调度器查询优化**：`get_settings` 改单查询、settings/queued 提出 loop 外、spawn IO 前释放锁。
4. **UX-1 语言收口**：选择器仅暴露 en/zh-CN（补齐 220 键前），统一 `SUPPORTED_LOCALES` 单一来源。
5. **同步文档**：更正 README/AGENTS"单任务限速未实现"的反向漏报；Metalink 校验措辞改"验最强一个"。
6. **F-9 BT private torrent 安全修复** [NEW(0624)]：从 `.torrent` metadata 读取 `info.private` 字段，private torrent 禁用 DHT/PEX。**最高优先级安全修复**。
7. **UX-9 onboarding 向导** [NEW(0624)]：首次启动 3-4 步浮层向导 + 帮助文档入口。
8. **UX-10 扩展 UI 国际化** [NEW(0624)]：扩展引入 `chrome.i18n` API + `_locales/` 目录，覆盖 en/zh-CN。
9. **UX-11 toast 聚合** [NEW(0624)]：相同 key 去重/更新；批量操作只发最终结果聚合 toast。

### 阶段 2 — 效率与并发（P1，~1–2 周）

1. **E-2 checkpoint 值变才写**：记 last_written，未变跳过 tasks+task_files，force 尊重 dirty。
2. **E-3 BufWriter**：每 segment 句柄包 256KiB–1MiB BufWriter。
3. **UX-2/E-5 批量命令**：后端 `bulk_task_action`/`bulk_delete_tasks`（单事务单事件），前端单次 IPC。
4. **E-4 限速器**：原子令牌桶 + 大粒度领取 + 修 250ms-cap。
5. **E-6 locale 懒加载**：动态 `import()` 非默认语言。
6. **UX-3 详情面板事件化**：tab 打开一次 fetch，刷新由 progress 事件驱动（定时器仅 30s fallback）。
7. **E-8 HTTP Client 连接池复用** [NEW(0624)]：按 proxy fingerprint 缓存 `reqwest::Client`，同主机多任务复用连接。预期加速 30-50%。
8. **E-9 DNS 缓存** [NEW(0624)]：用 `hickory-resolver` 自定义 resolver 缓存 DNS 结果。
9. **E-11 speed-history appendBatch 优化** [NEW(0624)]：只对有新 sample 的 task 创建新数组。
10. **A-9 reqwest timeout** [NEW(0624)]：加 `.timeout(Duration::from_secs(60))`。
11. **A-10 CancellationToken** [NEW(0624)]：引入 `tokio_util::sync::CancellationToken` + `tokio::select!` 实现即时取消。
12. **UX-12 IME 守卫** [NEW(0624)]：所有 `matchesShortcut` 调用前加 `event.isComposing` 守卫。
13. **UX-13 Shift+范围选择** [NEW(0624)]：支持 Shift+点击连续范围选择。
14. **UX-14 双击打开文件** [NEW(0624)]：完成状态任务双击直接打开文件。
15. **UX-15 错误消息本地化** [NEW(0624)]：扩展 `localizedErrorMessage` 覆盖更多错误码。
16. **UX-16 设置搜索定位字段** [NEW(0624)]：为每个字段分配 `data-search-key`，搜索时滚动并高亮。

### 阶段 3 — 数据一致性与安全（P2，~1 周）

1. **A-2 完成/状态写入事务化**（比照 `clear_tasks`）。
2. **A-3 去重 UNIQUE 约束**（或检测+插入包事务 + `ON CONFLICT`）。
3. **A-4 凭据迁移事务化 + AAD 绑定 task_id + 版本前缀**。
4. **A-5 启动非阻塞化**：窗口 `visible:false`，非关键步骤移 spawn。
5. **A-6 限速器 set_limit 改 lock / 跨盘 rename 回退 / WS 限流 + token 文件权限 / 文件名保留名 + 长度 clamp + 统一 sanitizer**。
6. **A-7 生产迁移失败恢复** [NEW(0624)]：实现备份-重建流程，迁移前自动备份。
7. **A-8 BT sessions LRU 淘汰** [NEW(0624)]：任务删除时清理空 session。
8. **A-11 SSRF 防护** [NEW(0624)]：`validate_handoff` 拒绝私有/链路本地/环回地址。
9. **A-12 HLS JoinSet abort_all** [NEW(0624)]：所有 `?` 错误传播路径统一 `workers.abort_all()`。
10. **A-15 WAL checkpoint 调度** [NEW(0624)]：启动时若 WAL > 100MB 执行 `PRAGMA wal_checkpoint(TRUNCATE)`。
11. **A-16 临时文件清理** [NEW(0624)]：定期扫描清理孤儿临时文件；任务删除时清理关联临时目录。
12. **A-17 进程退出清理** [NEW(0624)]：注册 ctrlc handler，退出前 cancel 所有活跃任务并 join 等待 5s。
13. **A-18 配置跨字段校验** [NEW(0624)]：`update_settings` 时校验 schedule_start < end 等。
14. **A-19 关键模块测试** [NEW(0624)]：优先补调度器、加密、迁移测试。

### 阶段 4 — 功能补全（P2/P3，按需排期）

1. **F-3 清理死能力**：分类规则引擎、站点规则编辑器、队列拖拽重排——接通或从 schema 移除。
2. **F-4 SFTP 公钥/agent 认证**（创建对话框加密钥字段）。
3. **F-5 计划窗口抢占运行任务 + obey_schedule UI**。
4. **F-6 完成动作扩展**（脚本/webhook，传文件路径占位符）。
5. **F-7/F-8 浏览器流一键下载贯通 / HLS 变体选择 / 通用多算法校验 / 任务导入导出**。
6. **E-7 收尾**：HLS 接 EmitGate、task_requests 保留策略、排序索引、TaskRow/failureOptions/task_stats 微优化、删死代码。
7. **F-10 下载历史归档** [NEW(0624)]：增加 `task_history` 表，删除任务时归档元数据；历史查看/搜索/恢复 UI。
8. **F-11 HLS 字幕/多音轨** [NEW(0624)]：`finalize_hls_task` 增加 `-map` 选择；DRM 暂列长期。
9. **F-12 DASH 限制标注** [NEW(0624)]：UI 明确标注 DASH 不支持续传/限速/代理隔离；长期自研分段。
10. **F-13 Metalink 并行镜像** [NEW(0624)]：实现 aria2 `--mirror` 等价功能。
11. **F-14 BT Tracker 实时状态 + 做种限制 UI** [NEW(0624)]：从 librqbit 获取真实连接状态；暴露做种配置。
12. **F-15 代理支持增强** [NEW(0624)]：短期补 FTP over HTTP 代理；长期考虑 PAC 和代理链。
13. **E-10/13/14/15/16 效率优化** [NEW(0624)]：HTTP/2 keepalive、自动加速参数调优、ACCEPT_ENCODING 按内容类型、connect_timeout 降到 10-15s、cursor total 估算。
14. **A-13/14 架构改进** [NEW(0624)]：引入 thiserror 类型化错误；统一日志系统；加 `#[tracing::instrument]`。
15. **E-17/18/19/20 效率微修** [NEW(0624)]：BT bitfield 增量更新、后台 flush 250ms、queue-changed debounce 300ms、preallocate 重试。

## 相关文档

- [project-improvement-audit.md](project-improvement-audit.md)：按发布风险组织的前向清单。
- [audit-report.md](audit-report.md)：前端可访问性与主题审计（WCAG / OKLCH token）。
- [ROADMAP.md](ROADMAP.md)：后续路线图。
- [error-codes.md](error-codes.md)：错误码定义。
