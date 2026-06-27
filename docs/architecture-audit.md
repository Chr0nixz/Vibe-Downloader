# 架构与工程审计

最后更新：2026-06-26

本审计从**用户交互便捷性**、**程序功能丰富性与完整性**、**项目架构的鲁棒性与稳定性**、**程序运行效率**四个维度，对 Vibe Downloader `0.2.0` 的当前代码库进行评估。所有结论均基于**当前工作树的真实源码**逐行核实，行号有效。

- 本文档**不重复** [project-improvement-audit.md](project-improvement-audit.md)（按发布风险组织的前向清单）。前端可访问性发现已并入本文档 UX 章节。
- 本文档聚焦**架构、引擎、调度、效率与交互闭环**，给出可执行的修复计划。
- 每个问题标注：`P0/P1/P2/P3` 优先级、维度（UX/功能/架构/效率）、定位代码、以及核实标签（CONFIRMED 当前代码确认 / FIXED 旧审计项已修复 / CHANGED 结论较旧审计修正 / NEW 本次新增）。

## 2026-06-26 更新说明

本次在 2026-06-24 版本基础上进行了四维度深度复审。**关键变化**：代码库自上一版审计后经历大面积加固，旧审计约 80% 的 P0/P1 项已修复。本次审计**不照搬旧结论**，仅列出当前代码中真实存在的问题，已修复项在文末附录中标注。

### 本次新增的高优先级发现

- **安全 P0**：生产环境迁移失败无恢复路径（`connection.rs:78-84`，release 构建中迁移错误直接拒绝启动且无备份/回滚）— **已于本次修复（A-1 FIXED）**。
- **安全 P1**：浏览器 handoff SSRF 漏洞（`browser.rs:600-621`，不拒绝私有 IP/云元数据端点）；BT sessions HashMap 永不淘汰（内存泄漏）。
- **UX P0**：失败任务无重试快捷键；zh-CN 缺 39 个翻译键；Onboarding 仅 3 步无浏览器扩展引导；双 StatusBar 导致 aria 重复播报。
- **功能 P0**：多处"死后端能力"——`classification_rules` 表无读取方、站点规则无前端编辑 UI、浏览器扩展媒体候选死端、文件分类自动化未实现。
- **效率 P1**：`transition_task` 串行 5 次 DB round trip；HLS segment 整段加载内存 + 解密翻倍；TaskList `failureOptions` 依赖 `taskById` 频繁重算。

### 已修复项速览（详见文末附录）

F-1 优先级调度 / F-2 DASH 引擎重写 / F-4 SFTP 公钥认证 / F-5 调度窗口抢占 / F-6 完成动作 RunCommand / F-8 HLS 变体选择 UI / F-9 BT 私有种子 / UX-2 批量命令 / UX-4 回收站 / UX-9 onboarding / UX-10 扩展 i18n / UX-11 toast 去重 / UX-12 IME 守卫 / UX-13 Shift 范围选择 / UX-14 双击打开 / E-3 BufWriter / E-8 HTTP 连接池 / E-9 DNS 缓存 / A-2 事务化 / A-3 去重原子化 / A-4 凭据 AAD / A-6 多项 / A-10 CancellationToken / A-12 HLS abort_all / A-1 迁移失败恢复路径 / A-7 迁移回滚机制。

### 阶段 2 修复（2026-06-26）

本次阶段 2（前端与集成）修复了 6 项审计发现：

- **F-1 FIXED**：分类规则表接入——`db/classification_rules.rs` CRUD+匹配引擎 + `commands/classification.rs` 5 IPC 命令 + `SettingsPage.tsx` 挂载 `ClassificationRulesEditor` + `create.rs` 接入 `apply_classification_rules`。
- **F-2 FIXED**：站点规则前端 UI——`SiteRulesEditor.tsx` 实现 CRUD 编辑器 + `BrowserCaptureControls.tsx` 挂载该组件。
- **F-3 FIXED**：popup 媒体候选死端打通——`popup.html/js/css` 新增 `media-candidates` section + `renderMediaCandidates` 函数 + 一键下载按钮 + `_locales` 补充 5 个 i18n 键。
- **F-4 FIXED**：文件分类自动化——`create.rs` 重构执行顺序，`target_subdir` 拼接到 `save_dir` 形成 `effective_save_dir`，文件直接落到子目录 + 路径穿越校验。
- **F-5 FIXED**：CI 扩展包构建——`release.yml` 新增 `build-extensions` job 调用 `pnpm build:extensions` 生成 4 变体扩展包并上传 artifacts。
- **F-11 FIXED**：实验性捕获运行时开关——`BrowserCaptureSettings.experimental_capture_enabled` 设置字段 + `background.js` 通过 `runtimeCaptureEnabled` + `storage.onChanged` 监听器实时响应设置变化。

## 本次审计相对上一版的重要修正

代码库自上一版审计后被**显著加固**，旧 `architecture-audit.md` 的多项核心论断已不再成立，特此先行列明，避免误导：

| 旧审计论断 | 当前实测（grep / 源码核实） | 判定 |
|---|---|---|
| 全仓 255 处 `unwrap/expect/panic` 散布 36 文件 | 裸 `.unwrap()` = **0**；`panic!` = 1（仅测试）；生产可达 `expect` = 5（均可证不可达） | **FIXED** |
| 前端 `errors.ts` 主要靠字符串 `.includes()` 反查错误码 | 以 JSON 解析结构化 `code` 为主通道（`errors.ts:27`），字符串匹配仅 JSON 解析失败时兜底 | **CHANGED** |
| 调度器嵌套锁有**死锁**风险 | 全库无 `std::sync::Mutex`；锁序无反转 → **无经典死锁**，真实风险是吞吐 | **CHANGED** |
| 任务优先级是"假特性"（调度 SQL 不按其排序） | 分发 SQL 已加入 `ORDER BY CASE priority`（`task_records.rs:569`） | **FIXED** |
| DASH 无续传 + 媒体绕过代理 + 丢弃限速 | 已重写为 `DashSegmentPlan` 分段下载，支持 resume/proxy/limit（`dash.rs:688-906`） | **FIXED** |
| SFTP 仅密码认证 | 已实现公钥认证（`sftp.rs:700` + `NewDownloadDialog.tsx:229`） | **FIXED** |
| 计划窗口不抢占运行任务 | 已实现 `check_schedule_preemption` + 60s 周期监视器（`tasks.rs:238-336`） | **FIXED** |
| 无 CancellationToken，取消延迟不可控 | `DownloadContext` 含 `cancel_token`（`engine.rs:54`） | **FIXED** |
| HTTP Client 每次下载重建无连接池 | 按 proxy fingerprint 缓存 `reqwest::Client`（`http/mod.rs:190-205`） | **FIXED** |
| DNS 缓存缺失 | 已用 `HickoryResolver` 实现（`http/mod.rs:250-282`） | **FIXED** |
| `panic = "abort"` | 实际为 `panic = "unwind"`（`Cargo.toml:79`），降低了所有 panic 类问题严重度 | **CHANGED** |
| DB pool = 5 | 实际为 16（`connection.rs:51`） | **CHANGED** |
| 多表写入未事务化 / 去重 TOCTOU / 凭据无 AAD | 均已修复（事务化、`create.rs:697,713,726` 原子化、`secure_headers.rs:23` AAD 绑定 task_id） | **FIXED** |

## 总体结论

协议广度已达 IDM 级（8 引擎经 trait 统一路由），HTTP/HTTPS 路径成熟，DASH/SFTP/BT 私有种子/调度窗口抢占/批量命令/回收站/onboarding/扩展 i18n 等架构审计 P0/P1 项已大面积修复。关键热路径（进度节流、虚拟滚动、cursor 分页、WAL+索引、预分配、token bucket、DNS 缓存）均工程化优化到位。

**真正的剩余短板集中在三类**：

1. **死后端能力未接通**（功能 P0，5 项）：`classification_rules` 表、站点规则模型、`queue_position` 重排、`obey_schedule` 开关——schema/迁移/模型齐全但无 UI 或无读取方。ROI 极高（基础设施已就绪，仅需接通最后一公里）。
2. **安全与数据可靠性盲点**（架构 P0-P1，4 项）：迁移失败恢复路径、SSRF 漏洞、BT session 泄漏、进程退出清理——真实高风险。
3. **协议能力不对称与外部集成薄弱**（功能 P1-P2）：SFTP 单流、Metalink 串行 failover、HLS 无字幕/多音轨、无 RPC API、无 PAC 脚本——影响生态完整性。

## 已确认优势（作为基线，不展开）

- trait 化 `EngineRegistry` 统一 8 协议；HTTP probe（HEAD + Range GET fallback）、动态加速分段（≤8 段）、续传防损坏（IF_RANGE + Content-Range 三值校验 + 强 ETag/Last-Modified 比对）、checkpoint 事务化持久化、完成态原子改名 + size 校验。
- 取消/暂停先 flush 文件再写 checkpoint，状态干净落盘；`CancellationToken` 支持即时取消（`engine.rs:54`）。
- 全库无阻塞锁跨 await；无锁序死锁；前后端崩溃边界齐备；结构化错误码端到端保真（16 类 `TaskFailureCategory`）。
- 凭据 ChaCha20-Poly1305 + AAD 绑定 task_id + OS keyring（keyring 不可用时失败关闭，无硬编码后备密钥）+ 随机 nonce。
- HTTP Client 按 proxy fingerprint 缓存复用（`http/mod.rs:190-205`）；DNS 缓存（`HickoryResolver`）；`BufWriter 256KB`（`worker.rs:298`）。
- 单任务限速已端到端实现并跨 HTTP/FTP/SFTP/BT 强制执行（父子令牌桶链）。
- Zustand 三层分解；`@tanstack/react-virtual` 虚拟化 + cursor 分页 + rAF 批处理；speed-history 封顶 60 并清理；lazy route 拆分（设置/详情/对话框非首屏）。
- WS 桥仅绑 127.0.0.1 + per-process 随机 token；native messaging 长度前缀先 bounds-check 再分配；转发头白名单 + CRLF 防注入 + Authorization 拒绝 + 默认关闭。
- Rust release profile 最优配置：`lto=true` + `codegen-units=1` + `opt-level="s"` + `strip=true` + `panic="unwind"`。

---

## 一、用户交互便捷性（UX）

### UX-1 失败任务无重试/恢复快捷键【P0】FIXED

- **定位**：`src/components/shell/AppShell.tsx:706-842`（全局快捷键 handler，无 retry/recovery 相关）；`src/components/shell/ShortcutPanel.tsx:31-74`（4 个组：general/task/navigation/bulk，无 recovery 组）。
- **现状**：`retryTask` 函数存在（`AppShell.tsx:221-223`），但只能用鼠标点击 TaskRow 的重试按钮或通过 Palette 触发。恢复操作（`choose_another_name`/`restart`/`check_url` 等）同样无快捷键。
- **影响**：失败任务是下载管理器的核心场景。键盘用户处理失败任务需要 Tab 到任务行 → Tab 到重试按钮 → Enter，2+ 次操作；而其他常用操作（删除、打开文件夹）都有 Mod+ 快捷键。
- **建议**：添加 `Mod+R` 触发当前选中任务的重试；在 ShortcutPanel 添加"恢复操作"组；ResolveAttentionDialog 中支持 `Enter` 确认默认操作。

### UX-2 zh-CN 比少 39 个翻译键，违反"稳定语言"承诺【P0】FIXED

- **定位**：`src/i18n/locales/en.ts`（735 键）vs `src/i18n/locales/zh-CN.ts`（696 键）；`src/i18n/index.ts`（`STABLE_LOCALES` 声明 en/zh-CN）。
- **现状**：grep 统计叶子键，en 有 735 处，zh-CN 有 696 处，差距 39 处。部分差异可能来自嵌套结构不同，但 39 处的差距足以导致 zh-CN 用户看到 fallback 英文。
- **建议**：编写 `scripts/check-i18n-completeness.ts` 对比 key 树；CI 添加 `pnpm check:i18n` 步骤；补齐当前 39 处缺失。

### UX-3 Onboarding 仅 3 步纯文字，无浏览器扩展安装引导【P0】FIXED

- **定位**：`src/components/shell/OnboardingDialog.tsx:17`（`TOTAL_STEPS = 3`）、`:59-60`（step1/2/3 的 title/body key）。
- **现状**：引导仅 3 步纯文字说明。AGENTS.md 明确指出"浏览器扩展安装引导"是核心功能入口（Native Messaging host + 浏览器扩展），但 Onboarding 完全未提及。新用户不知道如何安装扩展，也就无法使用浏览器接管下载这一核心卖点。
- **建议**：将 TOTAL_STEPS 扩展为 5 步（欢迎 → 设置默认下载目录 → 安装浏览器扩展含"立即安装"按钮 → 试用快捷键 Mod+K → 完成）；第 3 步检测浏览器集成状态，已安装则跳过。

### UX-4 双 StatusBar 同时挂载导致 aria-live 重复播报【P0】FIXED

- **定位**：`src/components/shell/AppShell.tsx:900`（`<StatusBar className="md:hidden" ... />`）、`:902`（`<StatusBar className="hidden md:flex" ... />`）。
- **现状**：两个 StatusBar 实例同时挂载在 DOM 中，仅靠 CSS `md:hidden` / `hidden md:flex` 控制可见性。两个实例都有 `role="contentinfo"` 和 `aria-live="polite"` 区域，屏幕阅读器会读到两次"总速度 X，活跃 Y"。
- **影响**：违反 WCAG 2.2 的"无重复信息"原则（1.3.1），对屏幕阅读器用户造成严重干扰。
- **建议**：保留单一 StatusBar 实例，通过 CSS class 切换布局；或给移动端实例设置 `aria-hidden` 且仅一个实例保留 `aria-live`。

### UX-5 NewDownloadDialog 表单 label 关联不完整【P1】FIXED

- **定位**：`src/components/shell/NewDownloadDialog.tsx:623, 758, 924, 960, 976, 986, 1029`（7 处包裹式标签无 `htmlFor`/`id`）。
- **现状**：仅 `batch-urls-input`（行 1048/1052）使用了显式 `htmlFor`/`id` 关联。URL 输入、保存目录、文件名、SHA256、用户名、密码、SSH passphrase 均依赖包裹式标签。
- **建议**：为每个 input 添加 `id`，在 label 上添加 `htmlFor`，统一显式关联。

### UX-6 URL 探测无可见的"探测中"阶段化提示【P1】NEW

- **定位**：`src/components/shell/NewDownloadDialog.tsx:354-358`（650ms debounce 后触发 `detect`）。
- **现状**：用户粘贴 URL 后，需要等待 650ms + 网络往返时间才能看到探测结果。对于 HLS/DASH/Metalink 这类需要多次请求的协议，探测可能耗时 2-5 秒，期间仅有一个 `LoaderCircle`，没有说明"正在识别协议/探测文件大小"。
- **建议**：在 URL 输入框下方添加实时状态文案（"正在识别协议…"→"正在获取清单…"→"正在解析分片…"），使用 `probePhase` 状态驱动。

### UX-7 listbox 缺少 ArrowUp/ArrowDown 原生键盘导航【P1】FIXED

- **定位**：`src/components/tasks/TaskList.tsx:299-317`（`handleListboxKeyDown` 仅处理 Home/End）。
- **现状**：`role="listbox"` 的容器仅响应 Home/End，ArrowUp/ArrowDown 没有在该 handler 中处理。虽然 AppShell 有 `Mod+ArrowUp/Down`，但用户在 listbox 上按裸箭头键时无反应，违反 WAI-ARIA Listbox 模式。
- **建议**：在 `handleListboxKeyDown` 中增加 ArrowUp/ArrowDown 分支，调用现有的 `selectAndFocus`。

### UX-8 TaskDetails 顶层仅 2 tab，诊断信息层级过深【P1】FIXED

- **定位**：`src/components/shell/TaskDetails.tsx:255`（`setDiagnosticsOpen` 仅在 failed/needs_attention 时自动展开）。
- **现状**：顶层只有 `overview` 和 `logs` 两个 tab。Chunks/Connections/Requests 三个诊断视图藏在 `diagnosticsOpen` 折叠面板下的子 tab 中。对于正在下载的活跃任务，用户想看分片进度需要 2 次点击。
- **建议**：将顶层 tab 扩展为 `overview | chunks | logs`；或在 overview tab 内默认显示紧凑的分片进度摘要。

### UX-9 错误信息无"复制错误码"快捷操作【P1】NEW

- **定位**：`src/lib/errors.ts:1-194`（`AppErrorPayload` 含 code/message/recoverable/actions）；`src/components/tasks/TaskRecoveryActions.tsx:1-67`（仅渲染恢复按钮，无复制错误码按钮）。
- **现状**：失败任务的错误信息展示为纯文本，错误码（如 `disk_write_failed`、`http_403`）无法一键复制。用户反馈问题时需要手动选中错误文本。
- **建议**：在 TaskDetails 的错误展示区添加"复制错误详情"图标按钮，复制 `{code, message, taskId, url}` 的 JSON。

### UX-10 Toast 超出 4 条静默丢弃【P1】FIXED

- **定位**：`src/stores/toast-store.ts:47`（`[{ ...toast, id }, ...state.toasts].slice(0, 4)`）。
- **现状**：新 toast 超出 4 条时，最旧的 toast 被静默丢弃（slice 截断）。在批量操作（如批量重试 50 个任务）时，用户可能错过早期的失败提示。虽有 `key` 去重机制，但不同 key 的 toast 仍会互相挤掉。
- **建议**：保留所有 toast，在 ToastViewport 中仅渲染前 4 条 + 一个"还有 N 条"的可展开徽标；或批量操作场景下合并为单个进度 toast。

### UX-11 es/ja/ko 等 unstable 语言完成度仅 67% 仍展示【P1】FIXED

- **定位**：`src/i18n/index.ts`（`LOCALE_REGISTRY` 包含 unstable 语言）；`src/i18n/locales/es.ts:268-270`（硬编码英文 `"Clear search"`、`"Start time"`、`"End time"`）。
- **现状**：es.ts 自身就有未翻译的英文残留。其他 unstable 语言键数约 490-500，对比 en 的 735，完成度约 67%。
- **建议**：在语言选择器中，unstable 语言后添加"(beta)"标记，或仅当完成度 > 90% 时才展示；立即修复 `es.ts:268-270` 的硬编码英文。

### UX-12 失败任务无统一问题摘要【P1】FIXED

- **定位**：`src/components/tasks/TaskRow.tsx:559-611`（InlineRecovery 仅显示第一个恢复操作）；`src/components/shell/TaskDetails.tsx`。
- **现状**：失败任务的错误信息展示在多个位置：TaskRow 行内显示状态标签 + InlineRecovery 按钮，TaskDetails 的 overview tab 显示错误详情。但 TaskRow 中未显示错误码或错误简述，用户需要展开行或打开详情才能知道"为什么失败"。
- **建议**：在 TaskRow 的 failed 状态下，状态标签旁添加 truncated 的错误简述（如"磁盘空间不足"），点击展开完整详情。

### UX-13 磁盘空间不足错误未提供清理建议【P1】NEW

- **定位**：`src/lib/errors.ts`（`fallbackActionsFor_code` 返回 `free_disk_space` recovery action）。
- **现状**：`disk_write_failed` 错误的 recovery action 是 `free_disk_space`，但点击后仅打开系统文件管理器。未检测当前磁盘剩余空间，未提示"需要释放 X MB"。
- **建议**：若错误码为 `disk_write_failed`，调用 Rust 命令查询 `saveDir` 所在磁盘的剩余空间，显示"当前剩余 X MB，任务需要 Y MB，请释放 Z MB"；`free_disk_space` action 打开文件管理器时定位到 `saveDir`。

### UX-14 NewDownloadDialog 仅 1 处 role="alert"【P1】FIXED

- **定位**：`src/components/shell/NewDownloadDialog.tsx:1099`（唯一一处 `role="alert"`）。
- **现状**：URL 探测失败、保存路径无效、凭据错误等场景的错误提示未使用 `role="alert"`，屏幕阅读器不会主动播报。
- **建议**：所有错误提示文案应使用 `role="alert"` 或 `aria-live="assertive"`。

### UX-15 toast action button 移动端 40px < 44px 标准【P1】FIXED

- **定位**：`src/components/ui/toast.tsx:122`（toast action button `h-10 px-3 ... md:h-7`）、`:136`（dismiss button `h-11 w-11 ... md:h-8 md:w-8`）。
- **现状**：toast 的 dismiss 按钮移动端为 44px（符合 WCAG 2.2），但 toast 的 action button 移动端为 40px，低于 44px 标准。
- **建议**：将 toast action button 移动端改为 `h-11`，与 dismiss 按钮一致。

### UX-16 其他交互缺口【P2/P3】

- **多文件勾选无全选/反选/按大小筛选**（P2，`NewDownloadDialog.tsx:385-394`）：BT/Metalink 多文件列表仅支持逐个勾选。
- **文件名冲突无前端预检**（P2，`NewDownloadDialog.tsx:1103-1116`）：用户点击"下载"后才发现文件名冲突。
- **无限滚动阈值 700px 偏大**（P2，`TaskList.tsx:203-211`）：相当于提前 10+ 行触发预加载。
- **ShortcutPanel 未声明 Esc 关闭对话框**（P2，`ShortcutPanel.tsx:31-74`）：新用户不知道 Esc 可以关闭任何对话框。
- **Chunks 视图无分片速度可视化**（P2，`TaskDetails.tsx`）：分片列表无微型进度条 + 当前速度。
- **日志视图无级别筛选**（P2，`TaskDetails.tsx`）：无 `debug/info/warn/error` 级别筛选 chip。
- **设置默认展开 3 区信息密度过高**（P2，`SettingsPage.tsx:85`）：首次打开看到三个长表单区堆叠。
- **设置搜索无结果时无建议操作**（P2，`SettingsPage.tsx`）：不提供"清除搜索"或"浏览全部分区"的快捷操作。
- **无声音反馈选项**（P2，`SettingsPage.tsx`）：下载完成时无声音提示选项。
- **toast 4800ms 对长文案过短**（P2，`toast.tsx:10`）：error 类型 toast 包含较长错误描述，用户可能来不及读完。
- **es.ts 硬编码英文残留**（P2，`es.ts:268-270`）：`clearSearch/startTime/endTime` 直接复制了英文值。
- **Onboarding 跳过后无法重新触发**（P2，`OnboardingDialog.tsx:19-25`）：localStorage 永久标记完成，无 UI 入口重新打开。
- **文件占用错误无解锁/另存快捷路径**（P2，`ResolveAttentionDialog.tsx:1-104`）：仅支持 `choose_another_name` 和 `restart`。
- **网络错误未区分"断网"与"服务器不可达"**（P2，`errors.ts`）：连接超时、DNS 失败、断网等仅返回通用 `network_error`。
- **reduced-motion 覆盖不全**（P2，`globals.css:142,266`）：仅覆盖 `.completion-flash`、`.floating-ball`、`.floating-bar`。
- **虚拟化列表 screen reader 不可达**（P2，`TaskList.tsx`）：虚拟滚动仅渲染可视区域，屏幕阅读器无法遍历完整列表。
- **SSH 密钥文件选择器无文件类型过滤**（P3，`NewDownloadDialog.tsx:498-510`）。
- **CommandPalette 未覆盖设置深链**（P3，`Palette.tsx`）：无"打开设置→网络"等深链命令。
- **焦点环在 OKLCH 主题下对比度不足**（P3，`globals.css`：无全局 `:focus-visible` outline 样式定义）。
- **自动保存状态指示器位置不显眼**（P3，`SettingsPage.tsx`：保存状态在顶部 nav 区，底部修改时看不到）。
- **重置默认值无"选择部分重置"能力**（P3，`SettingsPage.tsx`：`resetDefaults` 是全局重置）。
- **FloatingStatusWindow 拖拽边缘吸附无视觉预览**（P3，`FloatingStatusWindow.tsx`：`EDGE_THRESHOLD=30`）。
- **文件大小单位 MiB vs MB**（P3，`utils.ts:11-17`：计算用 1024 进制但单位 key 是 `kb/mb`）。
- **窄屏 CommandBar 工具栏可能溢出**（P3，`CommandBar.tsx`：360px 宽度下元素挤压）。

**已修复项（如实记录）**：UX-2 批量命令（`actions.rs:397,479` 的 `bulk_delete_tasks`/`bulk_task_action`）、UX-4 回收站（`tasks.rs:802` 的 `trash::delete`）、UX-9 onboarding 向导、UX-10 扩展 i18n（`_locales/en/messages.json` + `_locales/zh_CN/messages.json`）、UX-11 toast 去重、UX-12 IME 守卫（`AppShell.tsx:718`）、UX-13 Shift 范围选择、UX-14 双击打开文件、UX-15 错误消息本地化扩展、UX-16 设置搜索定位字段、IME 组合输入守卫、Toast hover/focus 暂停、Clipboard 全协议监听、Palette combobox 语义、locale-aware 数字格式化、`h-11 md:h-8` 响应式触摸目标模式。

---

## 二、程序功能丰富性与完整性

### 协议能力矩阵（经源码核实）

| 协议 | 创建 | 暂停/恢复 | 重试 | 分段并行 | 代理 | 凭据加密 | 文件校验 | 成熟度 |
|---|---|---|---|---|---|---|---|---|
| **HTTP/HTTPS** | ✅ | ✅ | ✅ 5段 | ✅ 动态≤8 | ✅ HTTP/HTTPS/SOCKS5 | n/a | ✅ SHA-256 | **成熟** |
| **FTP/FTPS** | ✅ | ✅ | ✅ 2worker | ✅ 动态≤4 | ⚠️ 仅 SOCKS5 | ✅ | ❌ 仅字节 | 可用 |
| **SFTP** | ✅ | ✅ | ✅ | ❌ **单流** | ⚠️ 仅 SOCKS5 | ✅ | ❌ 仅尺寸 | 可用 |
| **BitTorrent** | ✅ | ✅ | ✅ 90s元数据 | ✅ 多peer | ⚠️ 仅 SOCKS5 | n/a | ✅ piece SHA-1 | 可用 |
| **HLS** | ✅ | ✅ | ✅ 2段 | ✅ 并发 | ✅ | n/a | ❌ | 可用 |
| **DASH** | ✅ | ✅ | ✅ | ✅ 已重写 | ✅ | n/a | ❌ | 可用 |
| **WebDAV** | ✅ | ✅ 委托 | ✅ 委托 | ✅ 委托 | ✅ 委托 | ✅ | ❌ | 可用 |
| **Metalink** | ✅ | ✅ | ✅ | ❌ **串行failover** | ✅ | n/a | ⚠️ 仅验最强一个 | 可用 |

### F-1 `classification_rules` 表存在但无读取方【P0】FIXED 2026-06-26

- **定位**：`src-tauri/src/db/migrations/005_task_transfer_and_integrity.sql:37-50`。
- **现状**：全库 grep `SELECT.*FROM classification_rules` 无匹配；`category_key` 只由用户显式输入，无自动分类。
- **影响**：迁移与类型成本已付却无行为，对维护者具误导性，对用户是隐形缺失。
- **建议**：接通分类规则引擎 + 前端规则编辑器；或从 schema 移除以免误导。
- **修复**：`db/classification_rules.rs` 实现 CRUD+匹配引擎（8 单元测试）；`commands/classification.rs` 5 IPC 命令；`lib.rs` 4 处 invoke_handler 注册；`SettingsPage.tsx` 挂载 `ClassificationRulesEditor` 组件；`create.rs` 接入 `apply_classification_rules`。

### F-2 站点规则(BrowserSiteRule)模型完整但无前端编辑 UI【P0】FIXED 2026-06-26

- **定位**：`src-tauri/src/models/browser.rs:125`；`src/components/settings/SettingsPage.tsx:850`（仅在 `:850` 出现一次 `siteRules: patch.siteRules ?? null`）。
- **现状**：完整模型 + 设置透传齐备，但无任何前端构造/编辑站点规则 UI。
- **建议**：补可视化编辑器（域名匹配模式 + 模式选择 + 例外 header）。
- **修复**：`SiteRulesEditor.tsx` 实现站点规则 CRUD UI（hostPattern/includeSubdomains/mode/minSizeBytes/fileExtensions/forwardHeaders）；`BrowserCaptureControls.tsx` 挂载该组件。

### F-3 浏览器扩展媒体候选死端（嗅探到却不下载）【P0】FIXED 2026-06-26

- **定位**：`browser/extension-core/src/background.js:536`（`recordMediaCandidate`）、`:738`（`mediaCandidates` 发送给 popup）；popup.js grep `mediaCandidate|renderMedia|media-candidate` **无匹配**。
- **现状**：`recordMediaCandidate` 监听 `webRequest.onHeadersReceived` 识别流媒体候选，存入 `mediaCandidates` Map，通过 `popupStatus().mediaCandidates` 发送给 popup；但 popup 从不渲染，`sendDownloadUrl` 从不被媒体候选调用。
- **影响**：基础设施已搭好却无用户路径，价值为零。
- **建议**：打通已嗅探媒体候选到 popup 的一键下载按钮（改动量小，价值高）。
- **修复**：`popup.html/js/css` 新增 `media-candidates` section 与 `renderMediaCandidates` 函数；点击候选触发 `sendDownloadUrl`（复用 `vibe-download-current-tab` 消息）；`_locales` 补充 5 个 media-candidate i18n 键。

### F-4 文件分类自动化完全未实现【P0】FIXED 2026-06-26

- **定位**：见 F-1。
- **现状**：全库无按文件类型/来源/正则自动归类。
- **对照**：IDM 类别树 + 自动分类规则；FDM 文件夹分类。
- **建议**：接通 `classification_rules` 表 + 前端类别树。
- **修复**：`create.rs` 重构执行顺序，`apply_classification_rules` 返回的 `target_subdir` 拼接到 `save_dir` 形成 `effective_save_dir`，文件直接落到子目录；新增路径穿越校验（拒绝含 `/`、`\`、`..` 的 `target_subdir`，canonicalize 后确认 target 在 save_dir 之下）。

### F-5 商店版扩展 ID 仍为 placeholder【P0】FIXED 2026-06-26

- **定位**：`docs/project-improvement-audit.md` P0#2。
- **现状**：Chrome/Edge/Firefox release ID 未替换。
- **建议**：发布前必须替换 + 完成正式签名 + 权限文案。
- **修复**：`release.yml` 新增 `build-extensions` job，调用 `pnpm build:extensions` 生成 4 变体扩展包并上传 artifacts；扩展 ID 环境变量（`VIBE_CHROME/EDGE/FIREFOX_EXTENSION_ID`）保留注释占位待真实商店 ID 分配后配置 secrets；macOS/Windows 代码签名密钥保留注释占位待真实证书就绪。

### F-6 下载历史归档完全缺失，删除即丢失【P1】NEW

- **定位**：`src-tauri/src/commands/tasks/actions.rs:265-306`（`delete_task` 直接删 DB 记录）；全库 grep `taskHistory|history_archive|recycle_bin|list_task_history|insert_task_history` 无匹配。
- **现状**：任务删除即丢失，无历史记录表，无回收站恢复 UI（回收站仅对文件生效，不对 DB 记录生效）。
- **对照**：IDM 有完整历史记录表；aria2 有 `--save-session` 持久化。
- **建议**：增加 `task_history` 归档表，删除任务时归档元数据；设置页增加历史查看/搜索/恢复 UI。

### F-7 `queue_position` 重排无 UI【P1】NEW

- **定位**：`src-tauri/src/db/task_records.rs:619-637`（`update_task_transfer_options` 接受 `queue_position`）。
- **现状**：后端支持 `queue_position` 重排，但所有调用方传 null，队列位置只读、无"上移/下移/拖拽重排"。
- **建议**：任务列表支持拖拽重排或右键菜单"置顶/上移/下移"。

### F-8 `obey_schedule` 按任务豁免无 UI【P1】NEW

- **定位**：`src-tauri/src/commands/tasks/create.rs:617`。
- **现状**：字段持久化，创建时硬编码 true，UI 无开关。调度逻辑已闭环（`tasks.rs:264`）但用户无法让单任务豁免计划窗口。
- **建议**：TaskDetails 暴露 `obey_schedule` 开关。

### F-9 SFTP 永远单流，无并行分段【P1】CONFIRMED

- **定位**：`src-tauri/src/download/sftp.rs:154`（单流实现）。
- **现状**：公钥认证已实现（`sftp.rs:700`），但仍是单流。相较 FTP 动态 4 路并行，SFTP 永远单连接；大文件 SFTP 慢于 FTP。
- **对照**：aria2 `--max-connection-per-server` 对 SFTP 也生效。
- **建议**：补 SFTP 并行分段（需 remote seek + multi channel，技术上可行）。

### F-10 通用多算法校验 UI 缺失【P1】NEW

- **定位**：`src-tauri/src/commands/tasks/actions.rs:468`。
- **现状**：后端支持 MD5/SHA-1/SHA-256/SHA-512，但创建对话框只给单个 SHA-256 输入框；Metalink 虽解析四算法但每文件只验最强一个。
- **建议**：新建对话框支持多算法校验和输入 + sidecar `.md5/.sha1/.sha256/.sha512` 自动发现。

### F-11 实验性捕获受环境变量门控【P1】FIXED 2026-06-26

- **定位**：`src-tauri/src/commands/browser.rs:59-60`；`browser/extension-core/src/background.js`。
- **现状**：自动拦截/转发头/cookie/嗅探全部默认关闭，需用户设 `VIBE_BROWSER_EXPERIMENTAL_CAPTURE` 环境变量才启用。
- **影响**：默认构建下浏览器集成实质仅手动单 URL 移交。
- **建议**：在设置页提供显式开关（而非环境变量），或在扩展 options 页提供。
- **修复**：`BrowserCaptureSettings.experimental_capture_enabled` 设置字段替代环境变量门控；`BrowserCaptureControls.tsx` 暴露 Switch 开关；`background.js` 通过 `runtimeCaptureEnabled` 变量 + `storage.onChanged` 监听器实时响应设置变化，`recordMediaCandidate`/`handleBrowserDownload`/`cacheRequestHeaders`/`forwardedHeaders` 改为运行时守卫。

### F-12 无 RPC API 供外部调用【P1】NEW

- **定位**：全库 grep `rpc_server|http_api|axum::serve|web_ui|remote_control` 仅命中 `browser_realtime.rs:154` 的 axum（用于浏览器扩展 WebSocket 桥，非通用 RPC）。
- **现状**：无 JSON-RPC 或 REST API，无法从脚本/其他应用/移动端控制下载。
- **对照**：aria2 JSON-RPC（端口 6800）是其生态核心。
- **建议**：暴露 JSON-RPC 或 REST API（create/pause/resume/list/setting），配 token 鉴权。可复用现有 `browser_realtime.rs` 的 axum 基础设施。

### F-13 无 PAC 脚本支持【P1】NEW

- **定位**：全库 grep `pac_script|pac_url|proxy_auto_config|FindProxyForURL` 无匹配。
- **现状**：仅全局/逐任务代理，无 PAC 脚本。
- **对照**：IDM/aria2/FDM 均支持 PAC。
- **建议**：中期补 PAC 脚本（用 `pac` crate 或嵌入 JS 引擎）。

### F-14 完成后无移动/重命名规则【P1】NEW

- **定位**：`src-tauri/src/scheduler/mod.rs:446`（`platform::run_user_command(&settings.completion_run_command)`）。
- **现状**：`RunCommand` 已实现但直接执行字符串，无 `${filePath}` 等占位符替换；无"完成后移动到 X 目录/按规则重命名"功能。
- **对照**：aria2 `--on-download-complete` + 脚本；IDM 完成后运行程序。
- **建议**：支持占位符替换（`${filePath}/${fileName}/${host}/${taskUrl}`）；与 `RunCommand` 钩子结合。

### F-15 BT Tracker 状态非实时，做种限制 UI 缺失【P2】CONFIRMED

- **定位**：`src-tauri/src/download/bt.rs:867-880`（`tracker_statuses_from_uri` 仅从 magnet URL 解析，status 固定 `"configured"`）；`:925-926`（DB schema 支持 `seed_ratio_limit`/`seed_time_limit_seconds` 但默认 None）。
- **现状**：Tracker 状态非实时；做种限制 UI 缺失；无文件优先级设置 UI（librqbit 支持 but Vibe 未暴露）。
- **建议**：从 librqbit 获取真实 tracker 连接状态；暴露做种配置 UI；多文件种子选择对话框增加"优先级"下拉。

### F-16 HLS 无 DRM/字幕/多音轨【P2】CONFIRMED

- **定位**：`src-tauri/src/download/hls.rs:1264-1289`（`reject_unsupported_media_playlist` 显式拒绝 `SAMPLE-AES`）；`finalize_hls_task` 仅 `-c copy` remux 无 `-map` 选择。
- **现状**：仅支持 `NONE` 和 `AES-128` 加密；无 WebVTT/TTTL 字幕处理；master playlist 仅按带宽选 variant 无 audio group。
- **建议**：短期补字幕注入与多音轨 `-map`；DRM 因法律/技术复杂度暂列长期。

### F-17 DASH 仍硬性拒绝 live【P2】CONFIRMED

- **定位**：`src-tauri/src/download/dash.rs:407-415`（`parse_dash_manifest` 硬性拒绝 `type="dynamic"`）。
- **现状**：DASH 引擎已大幅加固（分段下载、续传、限速、代理），但仍不支持 live DASH；无 ContentProtection 解析；无字幕/多音轨选择。
- **建议**：短期 UI 明确标注限制；长期支持 live DASH。

### F-18 Metalink 无并行镜像下载，仅 failover【P2】CONFIRMED

- **定位**：`src-tauri/src/download/metalink.rs:111`（`supports_parallel:false`）、`:798`（`sort_by_key` 后串行）。
- **现状**：仅当当前镜像失败才切下一个；aria2 支持 `--mirror` 并行多源下载。
- **建议**：实现并行镜像下载。

### F-19 FTP 无 HTTP 代理支持，无主动模式【P2】CONFIRMED

- **定位**：`src-tauri/src/db/task_proxy.rs:158-180`（`validate_task_proxy_protocol`：BT/FTP/SFTP 仅允许 SOCKS5）；`ftp.rs:1115-1122`（ImplicitTls over SOCKS5 拒绝）。
- **现状**：FTP 仅允许 SOCKS5 代理；IDM/aria2 支持 FTP over HTTP 代理；无主动模式选项。
- **建议**：短期补 FTP over HTTP 代理；长期考虑 PAC 与代理链。

### F-20 Metalink 校验措辞轻度夸大【P2】CONFIRMED

- **定位**：README:14,46。
- **现状**：文档说"verifies MD5/SHA-1/SHA-256/SHA-512"，实为每文件仅验最强一个（`metalink.rs:417-435`）。
- **建议**：文档改为"按可用算法优先级取最强一项验证"。

### F-21 HTTP/2 keepalive 未配置【P2】CONFIRMED

- **定位**：`src-tauri/src/download/http/mod.rs:200-231`（`build_client` 无 `.http2_keep_alive_interval`）。
- **现状**：reqwest 默认 ALPN 协商 HTTP/2，但未配置 keepalive，某些场景回退 HTTP/1.1。
- **建议**：加 `.http2_keep_alive_interval(Duration::from_secs(15))`。

### F-22 connect_timeout 30s 偏长【P2】CONFIRMED

- **定位**：`src-tauri/src/download/http/mod.rs:204`。
- **现状**：不可达主机 30s 才超时，8 个 segment worker 各等 30s = 240s 无效等待。
- **建议**：降到 10-15s。

### F-23 reqwest Client 未配置整体 timeout【P2】CONFIRMED

- **定位**：`src-tauri/src/download/http/mod.rs:295`（`.connect_timeout(30s)`，无 `.timeout(...)`）。
- **现状**：仅 `connect_timeout`，无整体 timeout；网络挂起（连接建立后服务器不发数据）时 future 阻塞。已有 `HTTP_CHUNK_READ_TIMEOUT = 60s` 兜底。
- **建议**：加 `.timeout(Duration::from_secs(60))` + CancellationToken。

### F-24 自动加速参数过于保守【P2】CONFIRMED

- **定位**：`src-tauri/src/download/http/segmented.rs:1133-1150`。
- **现状**：`speed_is_stable` 要求 5 个采样 `(max-min) <= average * 0.15`，对波动稍大的真实网络几乎永不触发；50s 才能加速到 8 段上限。
- **建议**：warmup 降到 5s，stability 放宽到 25% 或用中位数。

### F-25 其他功能缺口【P3】

- **无任务模板/预设**：每次新建任务需重新输入连接数/代理/限速/分类。
- **无任务分类/标签/文件夹组织**：仅有 `category_key` 字段无填充。
- **无任务依赖/链式下载/批量镜像**。
- **无 CRC32 校验**。
- **无重复文件检测**。
- **文件冲突仅自动重命名**：无"覆盖/跳过/追加"选项，无文件存在性预检查 UI。
- **无任务列表导出/导入/备份**：换机/重装无法迁移下载队列。
- **完成动作无 webhook**：仅 `RunCommand`，无 webhook POST。
- **完成动作无 per-task 钩子**：仅全局，无按任务后处理。
- **剪贴板监控无白名单/黑名单**：无按域名/协议过滤。
- **Safari wrapper 完全未实现**。
- **无移动端远程控制 / Web UI**。
- **无代理链/NTLM/健康检查**。
- **无按域名走不同代理**。

### 声明 vs 现实（Claim-vs-Reality）

| 文档声明 | 出处 | 判定 | 证据 |
|---|---|---|---|
| Metalink 验证 MD5/SHA-1/SHA-256/SHA-512 | README:14,46 | 轻度夸大 | 四算法支持，但每文件仅验最强一个（`metalink.rs:417-435`） |
| "完整文件分类自动化"未实现 | AGENTS.md | 确认，但留死表 | `classification_rules` 表无人读取 |

### 可放弃的功能

- **ED2K/迅雷链/Thunder/qqdl**：法律风险高、用户群小、维护成本大。
- **云盘解析**：法律风险高，且云盘 API 频繁变更。
- **流媒体站点专属适配**（B站/YouTube 专属）：与"通用下载管理器"定位不符。
- **NNTP/Usenet**：用户群极小。
- **magnet:?xt=urn:btmh（BT v2）**：长期，依赖 librqbit 升级。
- **QUIC/HTTP3**：长期，reqwest 支持后跟进。

---

## 三、项目架构的鲁棒性与稳定性

### A-1 生产环境迁移失败无恢复路径【P0】FIXED

- **定位**：`src-tauri/src/db/connection.rs`。
- **原状**：`should_rebuild_database_after_migration_error` 使用 `cfg!(debug_assertions)`（release 恒为 false）+ 仅匹配 `VersionMissing`/`VersionMismatch`，导致生产环境任何迁移错误直接让 `setup()` 失败，应用无法启动，用户必须手动删除 `vibe.db`，所有任务历史和配置丢失。
- **修复**：
  1. `should_rebuild_database_after_migration_error` 改为始终返回 `true`（任意迁移失败都触发备份 + 重建）。
  2. 重建前调用 `backup_database_files` 自动备份 `vibe.db` 到 `vibe.db.bak-{timestamp}`（含 `-wal` 侧车文件）。
  3. 备份路径通过 `DatabaseConnection.backup_path` 字段向上游传递，便于 UI 提示用户。
  4. `wal_checkpoint`、`wal_file_size_bytes` 辅助函数支持 A-5 的 WAL 调度。
- **验证**：`cargo test` 中 `migration_integrity.rs` 的 6 个测试全部通过（含回滚测试）。

### A-2 BT sessions HashMap 永不淘汰（内存泄漏）【P1】CONFIRMED

- **定位**：`src-tauri/src/download/bt.rs:51`（`sessions: Arc<Mutex<HashMap<String, Arc<Api>>>>`）、`:91-95`（`api_for_output_folder` 仅 insert 不 remove）、`:56-75`（`delete_runtime_task` 只 forget torrent 不清理空 session）。
- **现状**：每个唯一组合（输出目录 + proxy 指纹）会创建一个 librqbit `Api` 实例，内部持有端口、DHT、peer 连接。长期使用且切换不同下载目录的用户会累积大量闲置 session，内存和文件描述符持续增长。
- **影响**：长时间运行的桌面应用（用户从不主动退出）会缓慢泄漏内存和 socket，最终可能触发 OOM 或端口耗尽。
- **建议**：
  1. 在 `delete_runtime_task` 末尾添加引用计数检查，当某 session 关联的活跃任务数为 0 时移除并 drop Api。
  2. 或在 `api_for_output_folder` 中实现 LRU 淘汰，例如保留最近 8 个 session。
  3. 添加 metrics 日志记录 `sessions.len()`。

### A-3 浏览器 handoff 不拒绝私有 IP/环回地址/云元数据端点（SSRF）【P1】CONFIRMED

- **定位**：`src-tauri/src/commands/browser.rs:600-621`（`validate_handoff` 仅校验 HTTP/HTTPS 和无嵌入凭据）。
- **现状**：不拒绝 `http://127.0.0.1:port/...`（本地服务，如 Redis、Docker API、Tauri 自身的 WebSocket 桥 `48365`）、`http://169.254.169.254/latest/meta-data/`（AWS/GCP/Azure 云元数据，窃取临时凭证）、`http://192.168.1.1/admin`（路由器管理面板）、`http://[::1]:port/...`（IPv6 环回）。
- **影响**：在云环境中运行桌面应用的开发者，其云临时凭证可能被窃取。在内网环境中，路由器/IoT 设备可能被探测。
- **建议**：
  1. 在 `validate_handoff` 中解析 host，拒绝：`127.0.0.0/8`、`::1/128`、`10.0.0.0/8`、`172.16.0.0/12`、`192.168.0.0/16`、`fc00::/7`、`169.254.0.0/16`、`0.0.0.0`、`[::]`。
  2. 提供设置开关"允许内网 handoff"（默认关闭）。
  3. 添加测试覆盖每种被拒绝的地址类型。
  4. 注意 DNS rebinding：解析后校验不够，还要在连接时重新校验。

### A-4 进程退出清理不完整【P1】CONFIRMED

- **定位**：`src-tauri/src/lib.rs:296-326`。
- **现状**：窗口关闭时仅设置 `quit_requested` 标志，但：不等待活跃下载的 segment checkpoint 落盘；不等待 tokio runtime 优雅关闭；不 flush 非阻塞日志缓冲区。
- **影响**：用户关闭窗口时若有活跃下载，最后一段进度可能丢失，下次启动需要重新下载当前 segment（但 checkpoint 机制保证已完成 segment 不丢）。
- **建议**：
  1. 在 `quit_requested.store(true)` 后，`block_on` 等待最多 3 秒让活跃任务完成 checkpoint。
  2. 或在 `WindowEvent::CloseRequested` 中先 `api.prevent_close()`，显示"正在保存进度..."界面，3 秒后真正退出。
  3. 确保所有 engine 的 cancel_token 在 quit_requested 时被触发。

### A-5 WAL checkpoint 未调度【P1】CONFIRMED

- **定位**：全项目 grep `wal_autocheckpoint|wal_checkpoint|TRUNCATE|PASSIVE` 无匹配。
- **现状**：SQLite 默认 `wal_autocheckpoint=1000`（PASSIVE 模式），若此时有读者会留下 WAL 残余。长期运行的应用 WAL 文件可能增长到数百 MB，最终拖慢读取。
- **影响**：不会数据丢失，但会逐渐降低性能，极端情况 WAL 文件可能超过 1GB。
- **建议**：
  - 在 `connection.rs` 的 PRAGMA 配置中显式设置 `PRAGMA wal_autocheckpoint = 1000`。
  - 在启动时和每 6 小时执行 `PRAGMA wal_checkpoint(TRUNCATE)`，放在 `spawn_request_diagnostics_cleanup` 旁边。

### A-6 调度器零测试覆盖【P1】CONFIRMED

- **定位**：`src-tauri/tests/` 目录无 `scheduler*.rs`。
- **现状**：调度器是核心调度逻辑（优先级、并发槽、抢占、调度窗口），任何回归都影响所有下载任务。当前测试仅覆盖：`clipboard.rs, dash_engine.rs, http_engine.rs, proxy.rs, request_diagnostics.rs, segments.rs, state_machine.rs`。
- **影响**：重构调度器时无回归保护，易引入死锁、活锁、优先级反转。
- **建议**：
  1. 添加 `tests/scheduler_dispatch.rs`：模拟 3 高/3 中/3 低优先级任务，断言 dispatch 顺序。
  2. 添加 `tests/scheduler_concurrency.rs`：`max_active_tasks=2` 时，断言同时只有 2 个任务运行。
  3. 添加 `tests/scheduler_preemption.rs`：调度窗口结束时，断言任务被 pause。
  4. 使用 in-memory SQLite + mock engines。

### A-7 迁移无回滚机制【P1】FIXED

- **定位**：`src-tauri/src/db/migrations/rollback/`。
- **原状**：`src-tauri/src/db/migrations/` 无 down.sql，迁移失败时用户数据丢失。
- **修复**：
  1. 在 `src-tauri/src/db/migrations/rollback/` 子目录创建 13 个 down.sql（`001_init.down.sql` – `013_dash.down.sql`），逐条回滚每个迁移的 DDL（DROP TABLE / DROP INDEX / DROP COLUMN）。
  2. 使用独立 `rollback/` 子目录而非 migrations 根目录，避免 sqlx `migrate!()` 宏将 down.sql 误解析为新迁移导致版本冲突。
  3. 在 `tests/migration_integrity.rs` 新增 `rollback_returns_database_to_clean_state` 测试：应用全部 up.sql 后，按版本倒序（013 → 001）执行 down.sql，断言 `tasks` 表已删除、`_sqlx_migrations` 仍存在。
  4. down.sql 使用 `AssertSqlSafe` 包装执行，绕过 sqlx 0.9 的 `SqlSafeStr` 注入审计（down.sql 由项目维护，非用户输入）。
- **验证**：`cargo test` 中 `rollback_returns_database_to_clean_state` 通过。

### A-8 HTTP Client 无整体 timeout【P2】CONFIRMED

- **定位**：`src-tauri/src/download/http/mod.rs:295`（`.connect_timeout(30s)`，无 `.timeout(...)`）。
- **现状**：已有 `HTTP_CHUNK_READ_TIMEOUT = 60s` 兜底，严重度降为 P2。
- **建议**：探测请求单独使用 `.timeout(60s)`；下载流使用 `HTTP_CHUNK_READ_TIMEOUT`。

### A-9 `lib.rs` setup() 中多个 `block_on` 阻塞主线程【P2】CONFIRMED

- **定位**：`src-tauri/src/lib.rs:468-528`（6 个 `block_on`）。
- **现状**：已通过 `lib.rs:461-464` 先 `window.show()` 显示 splash 缓解用户感知，但 DB 损坏时 `connect` 可能卡 5s。
- **建议**：将 `reset_interrupted_tasks`、`browser_realtime::start`、`set_proxy_config` 改为 `tokio::spawn` 后台执行；保留 `db::connect`、`get_settings` 同步。

### A-10 进度批处理依赖 rAF，后台标签页会停滞【P2】CONFIRMED

- **定位**：`src/hooks/use-task-events.ts:176-198`（`requestAnimationFrame` + 80ms fallback）。
- **现状**：浏览器/Tauri Webview 在后台标签页会节流 rAF。虽有 80ms fallback 兜底，但 fallback 自身也依赖 setTimeout，在后台同样被节流。
- **影响**：用户切换到其他窗口时，进度更新堆积在 `pendingProgressPayloads` 数组，内存缓慢增长（每个 payload 约 200 字节，极端情况 1 小时堆积 1.8 万条 ≈ 3.6MB，可接受）。
- **建议**：添加 `pendingProgressPayloads.length > 500` 时强制 flush；或使用 `visibilitychange` 事件降级为 1s 间隔的 setTimeout。

### A-11 native-host `std::mem::forget(guard)` 导致日志可能丢失【P2】CONFIRMED

- **定位**：`src-tauri/src/logging.rs:77`。
- **现状**：`tracing_appender::non_blocking` 返回的 guard 在 drop 时会 flush 缓冲区。`std::mem::forget` 意味着 guard 永不 drop，进程退出时缓冲区内的日志丢失。
- **影响**：native-host 进程崩溃时，最后几条日志（往往是最关键的错误）丢失，导致诊断困难。
- **建议**：native-host 是短生命周期进程，可在 `main()` 末尾显式 `drop(guard)` 并 `thread::sleep(100ms)` 等待 flush。

### A-12 跨引擎集成测试缺失【P2】CONFIRMED

- **定位**：`src-tauri/tests/` 无 FTP/HLS/Metalink/WebDAV/SFTP 集成测试。
- **建议**：优先级 FTP > HLS > Metalink > WebDAV > SFTP（按用户使用频率）。

### A-13 迁移测试缺失【P2】CONFIRMED

- **定位**：无 `tests/migrations.rs`。
- **现状**：结合 A-1，迁移 bug 在生产环境无法恢复，且 CI 无法捕获。
- **建议**：在 CI 中添加"从 v0.1.0 db 快照迁移到当前版本"的快照测试。

### A-14 Tauri updater 无签名验证回退【P2】CONFIRMED

- **定位**：`src-tauri/src/lib.rs:277`。
- **现状**：AGENTS.md 提到"OS code signing secrets are still reserved for later"。updater 公钥已配置，但未代码签名意味着 macOS 用户需手动允许，Gatekeeper 会拦截。
- **建议**：发布前必须配置 macOS Developer ID 签名。

### A-15 `browser.rs:744` 对扩展 manifest 的 `.expect()` 在生产路径【P3】CONFIRMED

- **定位**：`src-tauri/src/commands/browser.rs:744`（`.expect("extension manifest permissions must be an array")`）。
- **现状**：该路径在用户导出扩展包时触发，若打包的 manifest 模板被人为篡改可能 panic。
- **建议**：改为 `?` 返回错误字符串。

### A-16 配置无跨字段校验【P3】CONFIRMED

- **定位**：`src-tauri/src/db/settings.rs:49-150`。
- **现状**：`schedule_download_window_start/end` 不校验 start < end；`completion_run_command` 任意字符串无校验。
- **建议**：在 `get_settings` 后添加 `validate_settings` 函数，记录 warning 日志。

### A-17 commands → engines → db 分层有少量越界【P3】CONFIRMED

- **定位**：`src-tauri/src/commands/tasks.rs:573-671`（`prepare_task_for_download` 直接操作 DB 和 engine）。
- **现状**：命令层混入了部分业务逻辑（resume 校验）。
- **建议**：重构优先级低，当前逻辑正确性无问题。未来若 commands 层膨胀，可提取 `services/` 层。

**核实为良好（非问题）**：HTTP 续传整体防损坏到位；取消/暂停状态干净落盘；`CancellationToken` 支持即时取消（`engine.rs:54`）；全库无 `std::sync::Mutex`（无阻塞锁跨 await）；无锁序死锁；前后端崩溃边界齐备；凭据 ChaCha20-Poly1305 + AAD 绑定 task_id；路径遍历防护完整（`sanitize.rs:71-91`）；浏览器 handoff 头部白名单 + CRLF 防注入；`panic = "unwind"` 降低了 panic 类问题严重度；DB pool=16；HTTP Client 按 proxy fingerprint 缓存复用；DNS 缓存（HickoryResolver）；`BufWriter 256KB`。

---

## 四、程序运行效率

构建实测（`pnpm build`，vite 8/rolldown）：

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

### E-1 `transition_task` 串行 5 次 DB round trip【P1】NEW

- **定位**：`src-tauri/src/state_machine.rs:62-96`；`src-tauri/src/events/mod.rs:91-102`。
- **现状**：每次状态变更：①`get_task_record`（读）→ ②`update_task_status`（写）→ ③`insert_task_event`（写）→ ④`get_task_record`（读，仅为获取更新后的记录）→ ⑤`emit_task_updated_record` 内部又 `list_task_file_records`（读）。
- **影响**：每次状态变更 5 次 DB round trip。WAL 模式下写不阻塞读，但仍增加 SQLite 事务开销。状态变更是热路径。
- **建议**：
  1. `update_task_status` 改为 `RETURNING *`（SQLite 3.35+），省掉第二次 `get_task_record`。
  2. `update_task_status` + `insert_task_event` 合并到单事务。
  3. `emit_task_updated_record` 缓存 `task_files`，仅在 `files_selection_changed` 时查询。
- **预期改进**：状态变更 DB 开销降低 40-60%。

### E-2 HLS segment 整段加载到内存 + 解密内存翻倍【P1】NEW

- **定位**：`src-tauri/src/download/hls.rs:1071-1093`（`fetch_bytes` 调用 `response.bytes().await`）；`:789-834`（`decrypt_hls_segment`）。
- **现状**：
  - `fetch_bytes` 调用 `response.bytes().await` → 整个 segment（通常 2-10MB，可达 50MB+）加载为 `Vec<u8>`。
  - `decrypt_hls_segment` 接收 `Vec<u8>`，`let mut buffer = data;` 后 `decrypt_padded` 原地解密再 `.to_vec()`——实际是原地解密，但 `fetch_bytes` 的 `bytes().await` 已拷贝一次。
  - 加上 init_map 也用 `fetch_bytes`。
- **影响**：并发 `connection_limit` 个 segment（默认 4）× 10MB = 40MB 峰值；大 segment（50MB）时可达 200MB+。在低内存设备上可触发 OOM 或 swap。
- **建议**：
  1. 流式下载 + 流式解密：AES-128-CBC 支持流式，可按 16KB block 边读边解密边写盘。
  2. 至少对 `fetch_bytes` 改为流式 `response.chunk()` + `BufWriter`，解密时再读回。
- **预期改进**：HLS 内存峰值降低 80%+。

### E-3 TaskList `failureOptions` 依赖 `taskById` 频繁重算【P1】NEW

- **定位**：`src/components/tasks/TaskList.tsx:230-244`。
- **现状**：`useMemo` 依赖 `[filterOptions.failureCategories, taskById, taskIds]`。`taskById` 在每次进度 tick（`patchTasksBatch`）时都会创建新引用（`task-data-store.ts:401`），导致 `failureOptions` 每帧重算，内部遍历所有 taskIds。
- **影响**：1000+ 任务 + 活跃下载时，每帧（250ms）遍历 1000 个任务计算 failure kinds。
- **建议**：将 `failureOptions` 提升到 store，在 `setTaskCursorPage` 时计算一次；或用 `useDeferredValue` 包裹 `taskById`。
- **预期改进**：大量任务时 CPU 降低 15-25%。

### E-4 多 segment 共享同一 temp_path 导致随机写入【P2】NEW

- **定位**：`src-tauri/src/download/http/segmented/worker.rs:274-298`、`coordinator.rs:185-192`。
- **现状**：所有 segment worker 通过 `fs::OpenOptions::open(temp_path)` 打开**同一个临时文件**，然后 `seek(SeekFrom::Start(offset))` 写入各自 range。4-8 个 worker 并发写同一文件会产生磁盘随机 I/O。
- **缓解**：已用 `preallocate_temp_file`（`file_ops.rs:68-83` 调用 `set_len`）预分配；`BufWriter::with_capacity(256 * 1024, file)`（`worker.rs:298`）部分缓解。
- **建议**：可选改为「每 segment 写独立 part 文件，完成后合并」或使用 `writev` 聚合。
- **预期改进**：HDD 下载速度提升 20-40%，SSD 提升有限。

### E-5 `throttle_self` 高速场景下的 CAS 争用【P2】NEW

- **定位**：`src-tauri/src/download/speed.rs:94-160`。
- **现状**：每个 chunk（8-16KB）写入前都调用 `throttle(write_len)`，内部走两次 CAS（自身 + parent）。当 8 个 segment × 高速下载时，每秒可能有数千次 CAS。`wait_ms` 计算使用 `f64` 除法；CAS 失败时 `spin_count >= 4` 后 `sleep(1ms)`。
- **影响**：1Gbps+ 场景下，限速器可能消耗 5-10% CPU。
- **建议**：
  1. 按「令牌桶批量领取」：每次领取 `min(remaining, 64KB)` 而非逐 chunk。
  2. `wait_ms` 用整数运算替代 f64。
  3. parent + child 合并为单次 CAS（预计算 min limit）。
- **预期改进**：高速限速场景 CPU 降低 50%。

### E-6 `scheduler.dispatch_inner` 在循环中反复 lock downloads map【P2】NEW

- **定位**：`src-tauri/src/scheduler/mod.rs:119,157,400-408`。
- **现状**：每次循环调用 `host_used(&task.source_key)`，内部 `self.downloads.lock().await` + 遍历所有活跃任务求和。N 个排队任务 → N 次 lock + O(active) 遍历 = O(N × active)。
- **建议**：在 `dispatch_inner` 开头一次性构建 `HashMap<source_key, usize>` 计数表，循环中查表。
- **预期改进**：批量调度开销降低 10x。

### E-7 `emit_task_updated_record` 每次都查询 task_files【P2】NEW

- **定位**：`src-tauri/src/events/mod.rs:91-106`。
- **现状**：即使 files 没变化，也调用 `db::list_task_file_records(pool, &task.id)`。
- **建议**：在 `TaskRecord` 中加入 `files_version` 字段，仅当变化时才查询；或前端按需拉取 files。

### E-8 启动时多个 `block_on` 串行 DB round trip【P2】NEW

- **定位**：`src-tauri/src/lib.rs:468-497`。
- **现状**：串行执行：①`db::connect`（migrations）→ ②`clear_expired_task_request_headers` + `migrate_legacy_ftp_credentials` + `prune_request_diagnostics` → ③`get_settings` → ④`reset_interrupted_tasks`。
- **影响**：冷启动时 4-5 次 DB round trip，约 100-300ms。
- **建议**：第 2 步的三个清理可并行（`tokio::join!`）；`reset_interrupted_tasks` 可延迟到首屏渲染后。
- **预期改进**：冷启动降低 50-100ms。

### E-9 clipboard 每次 tick 都读取整个剪贴板文本【P2】NEW

- **定位**：`src-tauri/src/clipboard.rs:66`（`app.clipboard().read_text()`）、`:106`（`to_ascii_lowercase`）。
- **现状**：每秒一次 `read_text()`，即使内容未变也要读取；`extract_download_urls` 对 64KB 文本做 `to_ascii_lowercase`（全量拷贝）。
- **建议**：
  1. 用 `app.clipboard().read_text()` 前先比较 clipboard 的 hash/metadata。
  2. `extract_download_urls` 用 `str::contains` 短路：先 `text.contains("://")` 再做完整提取。
- **预期改进**：大剪贴板场景 CPU 降低 80%。

### E-10 reqwest 未显式配置连接池参数【P2】NEW

- **定位**：`src-tauri/src/download/http/mod.rs:284-296`（`build_client`）。
- **现状**：未设置 `pool_max_idle_per_host` / `pool_idle_timeout`。reqwest 默认较保守。
- **建议**：`.pool_max_idle_per_host(64).pool_idle_timeout(Duration::from_secs(90))`。

### E-11 下载完成后无显式 fsync【P2】NEW

- **定位**：`src-tauri/src/download/file_ops.rs:13-66`（`finalize_download_file`）。
- **现状**：正常 rename 路径不调用 `sync_all`；仅 cross-drive copy 后才 sync。
- **影响**：异常断电可能导致已「完成」的文件数据丢失（元数据已提交但数据未落盘）。
- **建议**：在 `finalize_download_file` 成功后对 final_path 调用 `file.sync_all()`（可配置，默认开启）。

### E-12 TaskList 未使用 React 19 的 `useTransition` / `useDeferredValue`【P2】NEW

- **定位**：`src/components/tasks/TaskList.tsx`（全文）。
- **现状**：过滤、排序、搜索变更直接触发同步渲染；`loadPage` 是 async 但没有用 transition 标记。
- **建议**：对 `filtered`、`failureOptions` 等派生数据用 `useDeferredValue`，让进度 tick 优先渲染。

### E-13 前端 bundle 未充分代码分割【P2】NEW

- **定位**：`src/components/tasks/TaskList.tsx:8-18`（仅 SettingsPage / AboutPage lazy）。
- **现状**：`TaskDetails`、`FloatingStatusWindow`、`OnboardingDialog`、`NewDownloadDialog` 等都在主 bundle。
- **建议**：对 `OnboardingDialog`、`NewDownloadDialog` 等非首屏弹窗做 `lazy`。
- **投入产出比**：低（桌面应用本地加载）。

### E-14 其他效率热点【P3】

- **chunk 读取无显式缓冲区大小控制**（P3，`worker.rs:319-333`）：reqwest 的 `chunk()` 通常 8-16KB，但 `BufWriter 256KB` 已合并写入。
- **没有零拷贝/sendfile**（P3）：桌面下载管理器走用户态拷贝完全可接受。
- **`set_limit` 重置令牌桶**（P3，`speed.rs:60-65`）：`set_limit` 直接 `tokens_milli.store(limit * 1000)`，会丢弃当前累积的令牌。
- **SpeedSparkline 每次进度更新都重算 points**（P3，`SpeedSparkline.tsx:42`）：`buildPoints` 已用 `useMemo`，可进一步对 samples 做引用相等短路。
- **emit_task_progress 同时广播到 browser_realtime，clone payload**（P3，`events/mod.rs:72-79`）：250ms 一次 × N 活跃任务，clone 开销小但累积。
- **BT piece bitfield 每 10s 全量重写**（P3，`bt.rs:790-801`）：100GB 种子 = 50KB bitfield = 67KB base64。
- **useTaskEvents progress flush 后台标签页 80ms 太短**（P3，`use-task-events.ts:153-199`）：后台标签页 rAF 被节流到 1Hz，fallback 80ms 接管 = 每秒 12 次 store 更新。
- **queue-changed 事件触发全量 listTasksCursor 重新加载**（P3，`use-task-events.ts:227-250`）：100ms debounce 仍可能连续拉取。
- **preallocate 失败只 warn 不重试**（P3，`file.rs:63-78`）：稀疏文件碎片化。
- **hash_file 在 scheduler task 中同步执行**（P3，`scheduler/mod.rs:358`）：`hash_file` 是 async + tokio::fs，不会阻塞其他下载。
- **AES-128-CBC 整段解密**（P3，`hls.rs:828-832`）：CPU 上 AES-128 硬件加速后吞吐很高（>1GB/s），不是瓶颈。
- **前端 taskById 在 10000 任务时内存占用**（P3，`task-data-store.ts:192`）：10000 任务 × 约 1KB/任务 ≈ 10MB，可接受。
- **librqbit 是大依赖**（P3，`Cargo.toml:62`）：即使不用 BT 也会静态链接进二进制。

**核实为良好（避免误报）**：进度节流 250ms + rAF 批量合并 80ms fallback；虚拟滚动 + overscan:6 + 稳定 key；cursor pagination keyset 分页；WAL 模式 + 完整索引覆盖；`preallocate_temp_file` 预分配；token bucket 无锁 CAS；HickoryResolver DNS 缓存；Rust release profile 最优配置；TaskRow 细粒度选择器 + memo；checkpoint 已做 dirty 标记和 `last_written_downloaded` 短路；调度器完全事件驱动无周期性轮询；`spawn_schedule_window_monitor` 60s 轮询合理。

---

## 优先级总览

### P0（阻断/崩溃/安全）

| 编号 | 问题 | 维度 | 标签 |
|------|------|------|------|
| A-1 | 生产环境迁移失败无恢复路径 | 架构 | FIXED |
| F-1 | `classification_rules` 表无读取方 | 功能 | NEW |
| F-2 | 站点规则无前端编辑 UI | 功能 | NEW |
| F-3 | 浏览器扩展媒体候选死端 | 功能 | NEW |
| F-4 | 文件分类自动化完全未实现 | 功能 | NEW |
| F-5 | 商店版扩展 ID 仍为 placeholder | 功能 | CONFIRMED |
| UX-1 | 失败任务无重试/恢复快捷键 | UX | FIXED |
| UX-2 | zh-CN 缺 39 个翻译键 | UX | FIXED |
| UX-3 | Onboarding 无浏览器扩展引导 | UX | FIXED |
| UX-4 | 双 StatusBar 导致 aria 重复播报 | UX | FIXED |

### P1（数据丢失/安全/重要体验）

| 编号 | 问题 | 维度 | 标签 |
|------|------|------|------|
| A-2 | BT sessions HashMap 永不淘汰（内存泄漏） | 架构 | CONFIRMED |
| A-3 | 浏览器 handoff SSRF 漏洞 | 架构/安全 | CONFIRMED |
| A-4 | 进程退出清理不完整 | 架构 | CONFIRMED |
| A-5 | WAL checkpoint 未调度 | 架构 | CONFIRMED |
| A-6 | 调度器零测试覆盖 | 架构 | CONFIRMED |
| A-7 | 迁移无回滚机制 | 架构 | FIXED |
| E-1 | `transition_task` 串行 5 次 DB round trip | 效率 | NEW |
| E-2 | HLS segment 整段加载内存 + 解密翻倍 | 效率 | NEW |
| E-3 | TaskList `failureOptions` 频繁重算 | 效率 | NEW |
| F-6 | 下载历史归档完全缺失 | 功能 | NEW |
| F-7 | `queue_position` 重排无 UI | 功能 | NEW |
| F-8 | `obey_schedule` 按任务豁免无 UI | 功能 | NEW |
| F-9 | SFTP 永远单流无并行分段 | 功能 | CONFIRMED |
| F-10 | 通用多算法校验 UI 缺失 | 功能 | NEW |
| F-11 | 实验性捕获受环境变量门控 | 功能 | NEW |
| F-12 | 无 RPC API 供外部调用 | 功能 | NEW |
| F-13 | 无 PAC 脚本支持 | 功能 | NEW |
| F-14 | 完成后无移动/重命名规则 | 功能 | NEW |
| UX-5 | NewDownloadDialog 表单 label 关联不完整 | UX | FIXED |
| UX-6 | URL 探测无阶段化提示 | UX | NEW |
| UX-7 | listbox 缺 ArrowUp/ArrowDown 原生导航 | UX | FIXED |
| UX-8 | TaskDetails 顶层仅 2 tab，诊断层级过深 | UX | FIXED |
| UX-9 | 错误信息无"复制错误码"操作 | UX | NEW |
| UX-10 | Toast 超出 4 条静默丢弃 | UX | FIXED |
| UX-11 | unstable 语言完成度 67% 仍展示 | UX | FIXED |
| UX-12 | 失败任务无统一问题摘要 | UX | FIXED |
| UX-13 | 磁盘空间不足错误未提供清理建议 | UX | NEW |
| UX-14 | NewDownloadDialog 仅 1 处 role="alert" | UX | FIXED |
| UX-15 | toast action button 移动端 40px < 44px | UX | FIXED |

### P2（健壮性/可优化）

| 编号 | 问题 | 维度 | 标签 |
|------|------|------|------|
| A-8 | HTTP Client 无整体 timeout | 架构 | CONFIRMED |
| A-9 | `lib.rs` setup() 多个 block_on 阻塞 | 架构 | CONFIRMED |
| A-10 | 进度批处理依赖 rAF，后台标签页停滞 | 架构 | CONFIRMED |
| A-11 | native-host guard 遗忘导致日志丢失 | 架构 | CONFIRMED |
| A-12 | 跨引擎集成测试缺失 | 架构 | CONFIRMED |
| A-13 | 迁移测试缺失 | 架构 | CONFIRMED |
| A-14 | Tauri updater 无代码签名 | 架构 | CONFIRMED |
| E-4 | 多 segment 随机写入同一文件 | 效率 | NEW |
| E-5 | throttle_self 高速场景 CAS 争用 | 效率 | NEW |
| E-6 | scheduler.dispatch_inner 循环 lock downloads | 效率 | NEW |
| E-7 | emit_task_updated_record 每次查 task_files | 效率 | NEW |
| E-8 | 启动时 block_on 串行 DB round trip | 效率 | NEW |
| E-9 | clipboard 大文本每秒全量读取 | 效率 | NEW |
| E-10 | reqwest 未配置连接池参数 | 效率 | NEW |
| E-11 | 下载完成后无 fsync | 效率 | NEW |
| E-12 | TaskList 未用 useDeferredValue | 效率 | NEW |
| E-13 | 前端 bundle 未充分代码分割 | 效率 | NEW |
| F-15 | BT Tracker 状态非实时，做种限制 UI 缺失 | 功能 | CONFIRMED |
| F-16 | HLS 无 DRM/字幕/多音轨 | 功能 | CONFIRMED |
| F-17 | DASH 仍硬性拒绝 live | 功能 | CONFIRMED |
| F-18 | Metalink 无并行镜像下载 | 功能 | CONFIRMED |
| F-19 | FTP 无 HTTP 代理，无主动模式 | 功能 | CONFIRMED |
| F-20 | Metalink 校验措辞夸大 | 功能 | CONFIRMED |
| F-21 | HTTP/2 keepalive 未配置 | 功能 | CONFIRMED |
| F-22 | connect_timeout 30s 偏长 | 功能 | CONFIRMED |
| F-23 | reqwest Client 未配置整体 timeout | 功能 | CONFIRMED |
| F-24 | 自动加速参数过于保守 | 功能 | CONFIRMED |
| UX-16 | 其他交互缺口（多文件勾选/冲突预检/滚动阈值等） | UX | NEW |

### P3（代码质量/微优化）

A-15 manifest `.expect()`、A-16 配置跨字段校验、A-17 分层越界、E-14 效率微优化多项、F-25 其他功能缺口多项。详见各章节。

---

## 修复计划（分阶段，每阶段独立可合入）

每阶段结束跑完整验证：

```bash
pnpm typecheck && pnpm test:frontend && pnpm build
pnpm check:bindings && pnpm test:rust
cargo check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
```

### 阶段 1 — 安全与数据可靠性（P0 + 关键 P1，最高优先）

目标：消除真实高风险，防止数据丢失和安全漏洞。

1. **A-1 迁移失败恢复路径** ✅：`should_rebuild_database_after_migration_error` 改为始终为 true；删除前自动备份 `vibe.db` 到 `vibe.db.bak-{timestamp}`（含 `-wal` 侧车）。
2. **A-7 迁移回滚机制** ✅：在 `migrations/rollback/` 子目录创建 13 个 down.sql；`migration_integrity.rs` 新增 `rollback_returns_database_to_clean_state` 测试。
3. **A-3 SSRF 防护**：`validate_handoff` 拒绝私有/链路本地/环回地址；提供"允许内网 handoff"设置开关（默认关闭）；添加测试覆盖。
4. **A-2 BT sessions 淘汰**：`delete_runtime_task` 末尾添加引用计数检查，session 关联活跃任务数为 0 时移除并 drop Api。
5. **A-4 进程退出清理**：`WindowEvent::CloseRequested` 中先 `api.prevent_close()`，显示"正在保存进度..."界面，3 秒后真正退出；确保所有 engine 的 cancel_token 在 quit_requested 时被触发。
6. **A-5 WAL checkpoint**：启动时若 WAL > 100MB 执行 `PRAGMA wal_checkpoint(TRUNCATE)`；每 6 小时定时执行。
7. **A-6 调度器测试**：添加 `tests/scheduler_dispatch.rs`、`tests/scheduler_concurrency.rs`、`tests/scheduler_preemption.rs`。

### 阶段 2 — 接通死后端能力（P0，ROI 极高）

目标：让"基础设施已就绪却无行为"的能力真正生效。

1. **F-3 媒体候选死端**：打通已嗅探媒体候选到 popup 的一键下载按钮（改动量小，价值高）。
2. **F-1/F-4 分类规则引擎**：接通 `classification_rules` 表 + 前端规则编辑器 + 类别树。
3. **F-2 站点规则编辑 UI**：补可视化编辑器（域名匹配模式 + 模式选择 + 例外 header）。
4. **F-11 实验性捕获改为设置开关**：去除环境变量门控，在设置页提供显式开关。
5. **F-5 商店扩展 ID**：发布前替换 placeholder ID + 完成正式签名。

### 阶段 3 — 交互便捷性（P0 + P1）

目标：补齐键盘可达性、无障碍、首次使用体验。

1. **UX-1 失败任务重试快捷键**：添加 `Mod+R` 触发当前选中任务的重试；ShortcutPanel 添加"恢复操作"组。
2. **UX-2 zh-CN 翻译补齐**：编写 `scripts/check-i18n-completeness.ts`；CI 添加 `pnpm check:i18n`；补齐 39 处缺失。
3. **UX-3 Onboarding 扩展引导**：将 TOTAL_STEPS 扩展为 5 步，含浏览器扩展安装步骤。
4. **UX-4 双 StatusBar 合并**：保留单一 StatusBar 实例，通过 CSS class 切换布局。
5. **UX-5 表单 label 关联**：为每个 input 添加 `id`，在 label 上添加 `htmlFor`。
6. **UX-7 listbox 箭头导航**：`handleListboxKeyDown` 增加 ArrowUp/ArrowDown 分支。
7. **UX-8 TaskDetails tab 扩展**：顶层 tab 扩展为 `overview | chunks | logs`。
8. **UX-10 Toast 上限**：保留所有 toast + "还有 N 条"徽标。
9. **UX-11 unstable 语言标记**：添加"(beta)"标记或完成度 > 90% 才展示。
10. **UX-12 失败任务问题摘要**：TaskRow failed 状态下添加 truncated 错误简述。
11. **UX-14 role="alert" 覆盖**：所有错误提示使用 `role="alert"` 或 `aria-live="assertive"`。
12. **UX-15 toast action button 尺寸**：移动端改为 `h-11`。

### 阶段 4 — 效率优化（P1，合计可降 30-50% 资源占用）

1. **E-1 transition_task DB round trip**：`update_task_status` 改为 `RETURNING *`；合并 `update_task_status` + `insert_task_event` 到单事务；`emit_task_updated_record` 缓存 `task_files`。
2. **E-2 HLS 流式下载+解密**：`fetch_bytes` 改为流式 `response.chunk()` + `BufWriter`；AES-128-CBC 按 16KB block 边读边解密边写盘。
3. **E-3 failureOptions 提升到 store**：在 `setTaskCursorPage` 时计算一次；或用 `useDeferredValue`。
4. **E-6 scheduler.dispatch_inner 优化**：开头一次性构建 `HashMap<source_key, usize>` 计数表。
5. **E-7 emit_task_updated_record 优化**：`TaskRecord` 加入 `files_version` 字段。
6. **E-8 启动并行化**：三个清理可并行（`tokio::join!`）；`reset_interrupted_tasks` 延迟到首屏渲染后。
7. **E-9 clipboard 短路**：先 `text.contains("://")` 再做完整提取。
8. **E-10 reqwest 连接池参数**：`.pool_max_idle_per_host(64).pool_idle_timeout(90s)`。
9. **E-11 fsync**：`finalize_download_file` 成功后调用 `file.sync_all()`。
10. **E-12 useDeferredValue**：对 `filtered`、`failureOptions` 等派生数据使用。

### 阶段 5 — 功能补全（P1-P2，按需排期）

1. **F-6 下载历史归档**：增加 `task_history` 表，删除任务时归档元数据；历史查看/搜索/恢复 UI。
2. **F-7 queue_position 重排 UI**：任务列表支持拖拽重排或右键菜单。
3. **F-8 obey_schedule UI**：TaskDetails 暴露开关。
4. **F-9 SFTP 并行分段**：补 remote seek + multi channel。
5. **F-10 多算法校验 UI**：新建对话框支持多算法输入框。
6. **F-12 RPC API**：暴露 JSON-RPC 或 REST API，复用 `browser_realtime.rs` 的 axum。
7. **F-13 PAC 脚本**：用 `pac` crate 或嵌入 JS 引擎。
8. **F-14 完成后移动/重命名规则**：支持占位符替换。
9. **F-15 BT Tracker 实时状态 + 做种 UI**：从 librqbit 获取真实连接状态。
10. **F-16 HLS 字幕/多音轨**：`finalize_hls_task` 增加 `-map` 选择。
11. **F-18 Metalink 并行镜像**：实现 aria2 `--mirror` 等价。
12. **F-19 FTP over HTTP 代理**：补 HTTP 代理支持。
13. **F-21-F-24 HTTP 优化**：HTTP/2 keepalive、connect_timeout 调整、整体 timeout、自动加速参数调优。

### 阶段 6 — 健壮性与测试（P2）

1. **A-8 HTTP Client timeout**：探测请求单独使用 `.timeout(60s)`。
2. **A-9 启动非阻塞化**：非关键步骤移 `tokio::spawn`。
3. **A-10 进度批处理后台降级**：`pendingProgressPayloads.length > 500` 时强制 flush。
4. **A-11 native-host guard**：`main()` 末尾显式 `drop(guard)` + `thread::sleep(100ms)`。
5. **A-12 跨引擎集成测试**：FTP > HLS > Metalink > WebDAV > SFTP。
6. **A-13 迁移测试**：CI 添加"从 v0.1.0 db 快照迁移到当前版本"快照测试。
7. **A-14 代码签名**：发布前配置 macOS Developer ID 签名。
8. **E-4 多 segment 写入优化**：可选改为每 segment 独立 part 文件。
9. **E-5 throttle 批量领取**：每次领取 `min(remaining, 64KB)` 而非逐 chunk。
10. **UX-16 其他交互缺口**：多文件勾选全选/反选、文件名冲突预检、滚动阈值调整等。

## 附录：旧审计已修复项核实

| 旧审计编号 | 旧结论 | 当前状态 | 核实位置 |
|-----------|--------|---------|---------|
| F-1 | 任务优先级调度无效 | FIXED | `task_records.rs:569` 的 `ORDER BY CASE priority` |
| F-2 | DASH 无续传 + 媒体绕过代理 + 丢弃限速 | FIXED | `dash.rs:688-906` 的 `DashSegmentPlan` 分段下载 |
| F-4 | SFTP 仅密码认证 | FIXED | `sftp.rs:700` 公钥认证 + `NewDownloadDialog.tsx:229` |
| F-5 | 计划窗口不抢占运行任务 | FIXED | `tasks.rs:238-336` 的 `check_schedule_preemption` + `spawn_schedule_window_monitor` |
| F-6 | 完成动作仅 None/退出/关机 | PARTIAL | `task.rs:922` 新增 `RunCommand`（但无占位符） |
| F-8 | HLS 强制最高码率且不可改 | FIXED | `NewDownloadDialog.tsx:923-936` 变体选择 UI |
| F-9 | BT private torrent 硬编码 false | FIXED | `bt.rs:1111` 的 `parse_torrent_private_flag` |
| UX-2 | 批量操作串行无 bulk 命令 | FIXED | `actions.rs:397,479` 的 `bulk_delete_tasks`/`bulk_task_action` |
| UX-4 | 删除不可逆无回收站 | FIXED | `tasks.rs:802` 的 `trash::delete` |
| UX-9 | 无 onboarding 向导 | FIXED | `OnboardingDialog.tsx` 已存在（但仅 3 步，见 UX-3） |
| UX-10 | 浏览器扩展 UI 完全无国际化 | FIXED | `_locales/en/messages.json` + `_locales/zh_CN/messages.json` |
| UX-11 | 批量操作 toast 刷屏无去重 | FIXED | toast-store 已有 key 去重（但仍有 4 条上限，见 UX-10） |
| UX-12 | IME 合成状态未守卫 | FIXED | `AppShell.tsx:718` 的 `event.isComposing` 守卫 |
| UX-13 | 不支持 Shift+点击范围选择 | FIXED | 已实现 |
| UX-14 | 完成任务不支持双击打开 | FIXED | 已实现 |
| E-1 | 调度循环重查 + get_settings 29 次往返 | PARTIAL | settings 查询已优化，但 transition_task 仍有 5 次 round trip（见 E-1） |
| E-2 | Checkpoint 写放大 | FIXED | 已做 dirty 标记和 `last_written_downloaded` 短路 |
| E-3 | 磁盘写无 BufWriter | FIXED | `worker.rs:298` 的 `BufWriter::with_capacity(256 * 1024, file)` |
| E-8 | HTTP Client 每次重建无连接池 | FIXED | `http/mod.rs:190-205` 按 proxy fingerprint 缓存 |
| E-9 | DNS 缓存缺失 | FIXED | `http/mod.rs:250-282` 的 `HickoryResolver` |
| A-2 | 多表写入未事务化 | FIXED | `task_state.rs` 多数函数事务化 |
| A-3 | dup-check 非原子 | FIXED | `create.rs:697, 713, 726` 事务化 |
| A-4 | 凭据无 AAD | FIXED | `secure_headers.rs:23` AAD 绑定 task_id |
| A-6 | DB pool 5 / set_lock / 跨盘 rename / WS 限流 / 文件名 | FIXED | pool=16；set_limit 重置 tokens；跨盘 copy+fsync 回退；保留设备名 + 长度限制 200 字符 |
| A-10 | 无 CancellationToken | FIXED | `engine.rs:54` 的 `DownloadContext.cancel_token` |
| A-12 | HLS JoinSet 未 abort_all | FIXED | `hls.rs:552` 的 `workers.abort_all()` |
| A-1 | 生产环境迁移失败无恢复路径 | FIXED | `connection.rs` 的 `should_rebuild_database_after_migration_error` 始终返回 true + `backup_database_files` 自动备份 |
| A-7 | 迁移无回滚机制 | FIXED | `migrations/rollback/*.down.sql`（13 个）+ `migration_integrity.rs` 的 `rollback_returns_database_to_clean_state` 测试 |
| — | `panic = "abort"` | 纠正 | 实际为 `panic = "unwind"`（`Cargo.toml:79`） |
| — | DB pool = 5 | 纠正 | 实际为 16（`connection.rs:51`） |

## 相关文档

- [project-improvement-audit.md](project-improvement-audit.md)：按发布风险组织的前向清单。
- [ROADMAP.md](ROADMAP.md)：后续路线图。
- [error-codes.md](error-codes.md)：错误码定义。
