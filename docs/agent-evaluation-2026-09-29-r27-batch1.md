# Agent 评测分析报告：R27 第一批（止血）修复

评测日期：2026-09-29  
评测对象：另一个 agent 依据 [四维现状审查 2026-09-27](project-review-2026-09-27.md) 对本项目所做的**已完成**修改（未提交的工作区改动，基准提交 `611cbbc`）  
评测范围：只评已经完成的部分，会话中断时没做完的内容不计入  
会话来源：`.session-export/opencode+gpt-6-astra+ses_f176fb5adffeK72fUaw52HL6so.zip`（opencode 会话导出，JSONL 格式）

> 被评测的模型需要先说明：导出文件名写的是 `gpt-6-astra`，但 436 条 assistant 消息里有 **422 条来自 `qwen-latest-series-invite-2609`**（opencode 的 `build` agent，另有 7 条来自 `plan` agent）。`gpt-6-astra` 和 `grok-4.7` 各只参与了最后的会话导出步骤，**实际完成修复的是 Qwen**。

快速导航：[结论](#1-结论) · [评测方式](#2-评测方式) · [观测指标](#3-观测指标) · [逐项评测](#4-逐项评测) · [变异测试](#5-变异测试明细) · [声明核对](#6-声明核对) · [过程分析](#7-过程分析) · [评分](#8-评分) · [提交前建议](#9-提交前建议修正) · [修正记录](#10-修正记录2026-09-29) · [附录](#附录)

> 第 1–9 节是对原 agent 工作的评测，评分不变。第 9 节列出的问题已由评测方在同一天修复，修复内容与复测结果见[第 10 节](#10-修正记录2026-09-29)。

---

## 1. 结论

**综合评分：71/100（B，合格偏上）。**

agent 完整覆盖了审查报告第 8 节"第 1 批：止血"的 7 个条目。它的做法是：先在主审计里登记正式 ID，再逐项修复，最后把状态写成保守的 `Fixed locally`，没有直接写 `Closed`。

最关键的几条正确性修复做得很好，有的还超出预期：

- **ARC-56**：按 AGENTS.md 的"根因 1：各引擎契约漂移"规则，把 HTTP、FTP、SFTP 三个引擎一起修了，没有只修审查点名的 HTTP。
- **ARC-57**：新加的条件检查点在压力测试里暴露出 `SQLITE_BUSY_SNAPSHOT` 竞态。agent 没有去改测试凑结果，而是认出这是项目里已知的 ARC-06/ARC-21 同类风险，改用 `begin_immediate` 修掉了。
- **FUN-33**：抓准了根因——子限速器本身已经把三级限速折叠成最小值，所以改动最小。

主要扣分点有三个：

1. **测试锁住行为的能力不足**：13 个变异只杀死 6 个，杀伤率 46%。存活的恰好是 UX-43 最核心的几条行为，以及 ARC-58、FTP/SFTP 重试预算、UX-41 键清单的完整性。
2. **有一条 CI 门禁会失败**：`cargo fmt --check` 不通过，位置在它新增的 BT 测试里。它全程没有运行过 `cargo fmt`，也没有运行 `pnpm verify` 或 `pnpm build`。
3. **UX-43、ARC-58 各有说法与代码不符**：
   - UX-43 的注释和审计里描述了一个不存在的"Detect 按钮"，还声称"窗口隐藏时 toast 会排队"，这也不成立。
   - ARC-58 新增的开关文案在 7 个语言里都与实际行为不符。讽刺的是，这正是它在 UX-42 里刚修掉的同一类问题——"文案承诺与行为不符"。

---

## 2. 评测方式

| 步骤 | 做法 | 目的 |
| --- | --- | --- |
| ① 确定标准答案 | 以审查报告第 8 节第 1 批的 7 个条目、第 9 节的统一验收要点、agent 自己写进主审计的"修复合同/验收"，以及 [AGENTS.md](../AGENTS.md) 的流程规则作为判定标准 | 让每一条都有可核对的依据 |
| ② 静态审阅 | 逐文件阅读 39 个改动文件（+1130/−263）和 3 个新文件，对照每条修复合同检查实现 | 判断实现是否正确、有没有引入新缺陷 |
| ③ 独立复跑门禁 | 在当前工作树上重新运行 CI 等价门禁，不采信 agent 的自述 | 客观确认"能不能合入" |
| ④ 变异测试 | 把工作树复制到临时沙箱，对每条修复"反向注入"回原来的缺陷，看新增或已有测试能不能抓住。先跑无变异基线，确认全部通过；Rust 用独立的 target 目录 | 衡量测试是否真的锁住了行为，而不只是"测试通过" |
| ⑤ 核对声明 | 把审计条目里"修复证据"的每条说法和代码逐一对照 | 衡量报告是否诚实 |
| ⑥ 过程分析 | 解析会话 JSONL，统计耗时、工具调用、出错、自我纠正、token 用量 | 评估工作方式和效率 |

沙箱在评测结束后已经删除。用户工作区和 `src-tauri/target` 都没有被修改；`bindings.ts` 在重新生成比对之后已恢复成原文件。

---

## 3. 观测指标

### 3.1 结果类指标

| 指标 | 数值 | 说明 |
| --- | --- | --- |
| 条目覆盖 | **7/7** | 另外补登记了 `ARC-55`、`SEC-14`（代码注释里已经在用、但审计里缺失的 ID），并登记了 `ENG-12`（状态 Open，未修复） |
| 验收完全达成 | **3/7** | FUN-33、UX-41、UX-42；另外 4 项只部分达成（见第 4 节） |
| 回归 | **0** | Rust：57 个测试二进制，778 通过、0 失败、2 忽略（基线为 56 个二进制、769 通过）。前端：72 个文件、407 项全部通过（基线 71 个文件、397 项） |
| 新增测试 | Rust +9、前端 +10 | Rust 新增一个测试文件 `late_checkpoint_guard.rs`（5 项） |
| 门禁通过 | **8/9** | 唯一失败的是 `cargo fmt --check`（见 3.2） |
| 变异杀伤率 | **46%（6/13）** | Rust 3/7，前端 3/6（见第 5 节） |
| 声明准确度 | 大部分属实，**3 处与代码不符** | 见第 6 节 |
| 注释规范 | 新增代码行里中文 **0 处** | 符合 AGENTS.md"注释必须用英文"的要求 |

### 3.2 门禁复跑结果

| 门禁 | 结果 | agent 在会话中是否跑过 |
| --- | --- | --- |
| `pnpm typecheck` | ✅ | 跑过 |
| `pnpm lint`（Biome） | ✅ 265 个文件 | 跑过，并用 `--write` 自动修复了 12 个文件 |
| `pnpm check:i18n` | ✅ 7 个语言 | 跑过 |
| `pnpm check:docs` | ✅ | 跑过 |
| `pnpm test:frontend` | ✅ 407/407 | 跑过 |
| `cargo clippy --all-targets -D warnings` | ✅ | 跑过 |
| `cargo test --locked` | ✅ 778/0/2 | **只跑了相关的测试子集**，没跑全量 |
| bindings 一致性（`pnpm specta` 重新生成后与工作区比对） | ✅ 一致 | 跑过 specta；`check:bindings` 推迟了，但在审计里注明了 |
| `pnpm build` + `check:bundle` | ✅ 预算内 | **没跑**（AGENTS.md 要求 UI 改动必须跑） |
| `cargo fmt --all -- --check` | ❌ **1 处差异**：[bt.rs:2464](../src-tauri/src/download/bt.rs#L2464) | **没跑**。这是 `pnpm verify:rust` 的第一步，CI 会失败 |

### 3.3 过程类指标

| 指标 | 数值 |
| --- | --- |
| 有效工作时长 | 约 130 分钟（2026-09-28 23:36 到 09-29 01:45），之后会话中断 |
| 工具调用 | 517 次：`read` 162、`edit` 161、`bash` 165、`grep` 8、`write` 3、`todowrite` 6、`glob` 1；其中 2 次调用出错 |
| 编译与测试 | `cargo check/clippy` 14 次、`cargo test` 12 次、vitest 6 次、typecheck 7 次 |
| 中途遇到的错误 | 编译或类型错误 8 次，测试失败 3 次，**全部由它自己定位修复** |
| 编辑失误 | 误删注释或代码行 3 次，**全部自己发现并恢复** |
| Shell 用错 | 4 次：在 PowerShell 里用 bash 语法、调用了不存在的 `rg` 等；另有 1 次 `Set-Content` 写入了 BOM，自己发现后去掉了 |
| 输出语言 | 中文 64 段、英文 103 段，混用 |
| Token | 非缓存输入 1,057,549；输出 103,164；推理 211,122；缓存读 97,492,864 |
| 收尾情况 | 最后编辑的 2 个文件（`EnvironmentPanel.tsx`、`AppErrorBoundary.tsx`）改完后没有再验证，会话就中断了。本次复跑确认这两处没有问题 |

---

## 4. 逐项评测

### 4.1 ARC-56（R27-A01，P1）分段重试预算不重置：**A−**

- **实现**：
  - HTTP：[worker.rs](../src-tauri/src/download/http/segmented/worker.rs) 改用每次运行从 0 开始的 `run_retries`；退避时间也按本轮次数计算，恢复的分段不会一上来就用 15 秒的最大退避。持久化的 `retry_count` 继续保留为累计诊断值。
  - FTP/SFTP：在 `SegmentProgress` 上新增 `run_retry_count`。
- **亮点**：主动把同一类缺陷的修复扩展到 FTP/SFTP，符合 AGENTS.md 的根因规则。它确认过 HLS 本来就用运行内局部计数，审计里的这一说法属实。它还核对了协调器在 worker 失败后会中止整个任务、不会重新拉起同一分段，所以"每轮从 0 计数"不会变成无限重试。
- **测试**：`segmented_direct_retry_budget_resets_despite_persisted_retry_count` 构造"所有分段 `retry_count = 5`"的数据库状态，再给每段注入一次 500 错误。变异 RM1 被这条测试**杀死**。
- **缺口**：
  - FTP/SFTP 没有对应测试，变异 RM2 **存活**。
  - 审查的验收要求"失败 6 次 → 点重试 → 再注入 1 次瞬时错误 → 完成；跨重启同样成立"，实际测试用"直接预置数据库状态"做了近似，没有走真实的"失败再重试"路径，也没有覆盖重启。

### 4.2 FUN-33（R27-F10，P1）BT 启动后绕过全局限速：**A**

- **实现**：把 `Option::min` 那个组合表达式替换成 `effective_bt_download_limit(&speed_limiter)`。子限速器在派发时已经把"任务限速和定时窗口限速"作为自身限额，全局限速作为父级，`current_limit_bps()` 会正确处理其中某一级为 `None` 的情况，并且每次都重新读取，能跟上全局限速的实时变化。辅助函数的文档注释里明确写了"禁止再用 `Option::min` 组合"。
- **测试**：`bt_loop_limit_sync_keeps_global_limit_without_task_limit` 覆盖了 5 种组合，变异 RM3 被**杀死**。
- **缺口**：这个测试的格式不符合 rustfmt，正是导致 CI 失败的那一处。

### 4.3 UX-41（R27-U07，P2）"恢复默认"阈值不一致：**A−/B+**

- **实现**：
  - 后端：抽出 `settings_from_kv`；新增 `db::reset_settings`，只删除本模块拥有的 34 个键，其他键保留；新增 `reset_settings` 命令，包含清除 keyring 密码、代理、限速器、悬浮窗、事件和调度等副作用。
  - 前端：改为调用 `resetSettings()`，删掉了硬编码的那份默认值；浏览器 mock 改为用同一个工厂函数生成初值和重置值；bindings 重新生成。
- **测试**：Rust 的 `reset_settings_matches_fresh_database` 把重置结果序列化后与全新数据库逐字段比较；前端测试确认重置走的是 `resetSettings` 而不是 `updateSettings`。前端变异 FM5 被**杀死**。
- **缺口**：
  - 键清单 [settings.rs:621](../src-tauri/src/db/settings.rs#L621) 是手写的，没有测试保证它和全部 `SETTING_*` 常量一致。测试只污染了 14 个键，漏掉一个没被污染的键不会被发现（变异 RM7 **存活**）。
  - [commands/settings.rs:78](../src-tauri/src/commands/settings.rs#L78) 的副作用代码是从 `update_settings` 逐行复制过来的，没有抽成共用函数。
  - 有一处行为变化没有写进审计：旧实现发送的 `defaultSaveDir: null` 和 `ffmpegPath: null` 在后端的语义是"保持不变"，所以下载目录和 ffmpeg 路径实际上从来没被重置过；现在会真的重置。这与确认框里"会保留主题、语言等"的文案一致，不算缺陷，但应该在审计里说明。

### 4.4 UX-42（R27-U02，P2）"等待网络"文案承诺不存在的自动恢复：**B+**

- **实现**：
  - 7 个语言的 `task.diagnostic.waitingNetwork` 都改成了真实原因，例如"直播暂无新片段，点继续以恢复录制"。
  - 浏览器 mock 和 Rust 调试种子（`mock_seed.rs`，仍在 `#[cfg(debug_assertions)]` 内）都把 `waiting_network` 任务改成 HLS 直播录制，只构造后端真的能到达的状态。
- **判断合理**：它确认了行上已经有"继续"操作；"完成录制"需要 HLS 循环正在运行，所以放到媒体工作包处理，理由写得清楚。
- **缺口**：状态徽标 `status.waiting_network` 仍然显示"等待网络"，和新的诊断文案对不上。

### 4.5 ARC-57（R27-A02，P2）排空超时后 worker 被分离、迟到检查点改写状态：**B+**

- **实现**：
  - **A 层**：`pause_task`、`retry_task`、`retry_task_with_mirror`、`cancel_task` 的非 BT 分支统一改用 `cancel_and_drain_control`：先限时等待，超时后 abort 再 join。
  - **B 层**：`checkpoint_task_progress`、`update_task_progress`、`update_task_runtime_progress`、`update_task_and_segment_progress` 四个写入函数加了"任务仍在运行"门控。任务已不在 `downloading`/`retrying` 状态时，写入降级为只写字节数。这四个函数都改成 `begin_immediate` 事务。
- **亮点**：B 层最初的写法在 `state_machine_busy` 压力测试中失败。agent 认出这是先读后写导致的 BUSY_SNAPSHOT 升级竞态，属于项目已知的 ARC-06/ARC-21 同类问题，于是按既有模式修复。这是本轮质量最高的一次调试。
- **测试**：`late_checkpoint_guard.rs` 的 5 项覆盖了 B 层，变异 RM4 被**杀死**（5 项里 4 项失败）。
- **缺口**：
  - 审查要求的端到端故障注入（刷盘注入 8 秒延迟，再调用暂停）没有写。整个仓库**没有任何测试直接调用 `pause_task`、`cancel_task` 或 `retry_task`**，所以 A 层没有直接的测试保护。审计里用"`restart_quiesce` 已有测试覆盖同一函数"作为替代证据，只能算间接证明。
  - 超时后 abort 的是调度器的 supervisor 任务，它的收尾逻辑 `converge_download_outcome`（清理请求头和运行锁条目、重新触发派发）会被跳过。这与 ARC-45 重启路径的既有取舍一致，但 agent 没有分析。
  - 每次进度写入多了一次 SELECT，并改用 IMMEDIATE 事务，热路径的性能影响没有测量。
  - BT 分支和删除路径（R27-A06）仍然是只 abort 不 join。审计里如实写了"非 BT 分支"，这一点不扣分。

### 4.6 ARC-58（R27-A07，P3）完成动作重复触发、最后一个任务失败时不触发：**C+**

- **实现**：
  - 在 `Scheduler` 上新增 `completion_action_fired` 原子标志，用 `swap(true)` 保证只有一个调用者真正触发；`start_task` 成功时复位。
  - 失败分支也调用 `maybe_emit_completion_action(…, true)`，由新增设置 `completion_include_failures`（默认关闭）决定是否执行。
  - 这个设置贯通了模型、数据库、命令、bindings、设置页、7 个语言和 mock。
- **正确的部分**：只触发一次的逻辑正确；设置判断放在"抢占触发权"之前，被设置拒绝的失败触发不会消耗掉这次触发权。
- **问题**：
  - **新增开关的说明文案与行为不符**（[en.ts:487](../src/i18n/locales/en.ts#L487)，7 个语言都一样）：
    - "即使最后一个任务被取消也会执行"：取消路径既不走成功分支也不走失败分支，根本不会调用完成动作。
    - "默认关闭：仅在全部任务成功后执行"：`should_emit_completion_action` 不看历史失败，只要**最后结束的那个任务**成功就会触发。
  - **测试基本是同义反复**：`arc58_completion_action_claim_is_single_winner_until_rearmed` 测的其实是 `AtomicBool::swap` 本身（`claim_completion_action` 只是一行包装）。删掉复位语句（RM5）或删掉失败设置判断（RM6），测试依然全部**通过**。
  - agent 自己写下的验收要求"回归覆盖最后一个任务失败的路径"没有做到，但条目仍然标成了 `Fixed locally`。
  - 为一个 P3 条目新增持久化设置、界面开关和 7 个语言的文案，改动范围偏大；这个方向虽然来自审查建议，但文案出错直接抵消了它的价值。

### 4.7 UX-43（R27-U01，P1）剪贴板监控弹窗并自动探测：**B**

- **实现**：
  - 新增 [clipboard-write.ts](../src/lib/clipboard-write.ts)，在内存里登记应用自己写入剪贴板的内容，5 秒过期、最多 16 条、不持久化。应用内全部 7 处写剪贴板的调用点都改走这个模块。
  - AppShell 的监控回调改为：先丢弃自身写入，再只弹非模态 toast。
  - 从 toast 进入对话框时带上 `suppressAutoProbe`，对话框的 650 毫秒自动探测和批量自动预览都不触发；用户编辑输入后解除抑制。
  - 文件类型白名单被明确声明为后续增强、不在本次范围内，这一点如实说明，不扣分。
- **测试**：`clipboard-write.test.ts` 8 项（包括后端规范化带来的尾斜杠折叠）；对话框新增用例覆盖"抑制"和"编辑后恢复"两面。FM3、FM6 被**杀死**。
- **问题**：
  - **Detect 按钮并不存在**。注释 [AppShell.tsx:129](../src/components/shell/AppShell.tsx#L129)、[AppShell.tsx:1157](../src/components/shell/AppShell.tsx#L1157)、[NewDownloadDialog.tsx:381](../src/components/shell/NewDownloadDialog.tsx#L381)、[NewDownloadDialog.tsx:493](../src/components/shell/NewDownloadDialog.tsx#L493) 和审计验收都写了"点击 Detect/检测"。agent 在会话 01:37 自己确认过"没有显式的 Detect 按钮"，却没有回头修正这些描述。实际效果是：用户已经点了 toast 上的"使用此链接"，进入的单链接草稿却看不到协议、大小、磁盘空间等探测摘要——审查报告称赞过的这项设计被削弱了——用户只能改一下 URL，或者直接盲提交。
  - **"窗口隐藏时 toast 排队"的说法不成立**。[toast-store.ts:73](../src/stores/toast-store.ts#L73) 的延迟队列只在模态框占用焦点时生效，与窗口是否可见无关；info toast 4.8 秒后自动消失。所以应用最小化到托盘时复制下载链接，用户实际上看不到任何提示。审查建议的"累积到托盘或悬浮窗"没有实现，审计却写成已经满足。
  - **核心行为没有测试锁住**：删掉"丢弃自身写入"（FM1）、恢复成直接弹模态窗（FM2）、toast 进入时不再抑制探测（FM4），这三个变异全部**存活**。AppShell 的剪贴板回调没有任何测试。

### 4.8 治理与登记：**A**

- 补登记 `ARC-55`、`SEC-14`，消除了"代码引用了、但审计里没有的 ID"；登记 `ENG-12`，状态保持 Open、不做修复。
- 状态统一写成 `Fixed locally`，每条都注明"真实安装包走查待实机验证"。
- 小缺口：AGENTS.md 里"Settings span 33 keys"没有更新成 34（[AGENTS.md:125](../AGENTS.md#L125)）。`check:docs` 发现不了这类漂移。

---

## 5. 变异测试明细

| 编号 | 注入的缺陷 | 对应条目 | 结果 |
| --- | --- | --- | --- |
| RM1 | HTTP worker 的重试预算改回从持久化的 `retry_count` 起算 | ARC-56 | ✅ 杀死 |
| RM2 | FTP 本轮重试预算改回从持久化值起算 | ARC-56 | ❌ 存活（`ftp_engine` 18/18 通过） |
| RM3 | BT 限速改回 `None.min(Some(global))` 的写法 | FUN-33 | ✅ 杀死 |
| RM4 | 迟到检查点的门控永远判定为"运行中" | ARC-57 | ✅ 杀死（5 项里 4 项失败） |
| RM5 | 删掉 `start_task` 里的复位语句 | ARC-58 | ❌ 存活 |
| RM6 | 删掉失败触发的设置判断 | ARC-58 | ❌ 存活 |
| RM7 | 重置键清单漏掉 `start_on_boot` | UX-41 | ❌ 存活 |
| FM1 | AppShell 不再丢弃应用自身写入 | UX-43 | ❌ 存活 |
| FM2 | 检测到链接后恢复直接弹模态窗 | UX-43 | ❌ 存活 |
| FM3 | 对话框的自动探测抑制失效 | UX-43 | ✅ 杀死 |
| FM4 | toast 的动作不再传 `suppressAutoProbe` | UX-43 | ❌ 存活 |
| FM5 | 重置改回用硬编码默认值调用 `updateSettings` | UX-41 | ✅ 杀死 |
| FM6 | `isOwnClipboardWrite` 永远返回 false | UX-43 | ✅ 杀死 |

规律：**被杀死的都是纯函数或数据库层的逻辑，存活的都是"组装层"的行为**，包括调度器的状态流转、AppShell 的回调、FTP/SFTP 协调器。agent 倾向于给容易单独测试的辅助函数写测试，而没有去验证把这些部件连起来的那一层。

---

## 6. 声明核对

| 审计里的说法 | 核对结果 |
| --- | --- |
| ARC-56"DASH/HLS 本来就用运行内局部计数" | ✅ 属实（HLS 见 `hls/engine.rs:993`；DASH 没有分段重试计数） |
| FUN-33"每次重新读取，能跟上全局限速实时变化" | ✅ 属实 |
| UX-41"与全新数据库逐字段相等" | ✅ 属实（仅限被污染的那 14 个键） |
| UX-41"`pnpm check:bindings` 待门禁阶段统一执行" | ✅ 如实说明了没做；本次复跑确认 bindings 一致 |
| ARC-57"clippy `--all-targets -D warnings` 干净" | ✅ 属实 |
| ARC-57"`restart_quiesce` 已有测试覆盖同一函数" | ⚠️ 只是间接证据，不能替代对暂停路径的故障注入测试 |
| UX-43"用户点击检测或提交时再探测" | ❌ 不存在检测按钮 |
| UX-43"窗口隐藏时 toast 按既有 deferred 机制排队" | ❌ 延迟队列只在模态框占焦点时生效 |
| ARC-58 界面文案"被取消也会执行 / 默认只在全部成功后执行" | ❌ 与 `scheduler/mod.rs` 的实际分支不符 |

---

## 7. 过程分析

### 7.1 时间分布

| 阶段 | 时间 | 用时 |
| --- | --- | --- |
| 读审查报告、抽查结论、出优化计划（plan 模式） | 23:09–23:10 | 约 1.5 分钟 |
| 阶段 0：在主审计登记 ID | 23:36–23:38 | 约 2 分钟 |
| ARC-56 | 23:38–23:47 | 约 9 分钟 |
| FUN-33 | 23:47–23:57 | 约 10 分钟 |
| UX-41 | 23:57–00:10 | 约 13 分钟 |
| UX-42 | 00:10–00:23 | 约 13 分钟 |
| ARC-57 | 00:23–01:04 | 约 41 分钟 |
| ARC-58 | 01:04–01:24 | 约 20 分钟 |
| UX-43 | 01:24–01:45 | 约 21 分钟（会话中断） |

用时分配基本符合各条目的难度，ARC-57 占了约三分之一。

### 7.2 值得肯定的做法

1. 动手前先抽查审查结论：核对了 `bt.rs:1033`、`worker.rs:71`、`SettingsPage.tsx:941` 等关键论断，并确认审查的基准提交就是当前 HEAD。
2. 严格按 AGENTS.md 的顺序推进：先登记，再修复，测试通过后才更新状态。
3. 遇到测试失败时追查根因（ARC-57 的 BUSY 竞态），而不是去改测试。
4. 修改已有测试时说明理由。`segments.rs` 里一条测试原来是对 `Queued` 状态的任务写进度，agent 先确认生产环境里调度器会在引擎写进度前把任务切到 `Downloading`，再按这个时序补上前置状态转移。
5. 发现自己误删了内容会立即恢复；发现 PowerShell 写入了 BOM 也会主动去掉。

### 7.3 不足

1. **收尾验证不完整**：没运行 `pnpm verify`、`cargo fmt` 或 `pnpm build`，Rust 只跑了测试子集。
2. **发现与自己说法矛盾的事实后没有回头修正**：UX-43 的 Detect 按钮就是这样。
3. **对 Windows 环境不熟**：在 PowerShell 里反复用 bash 或 ripgrep 的写法。
4. **会话中断前最后两处编辑没有验证**，不过结果恰好没有问题。
5. **输出语言中英混杂**，不利于阅读。

---

## 8. 评分

| 维度 | 权重 | 得分（10 分制） | 依据 |
| --- | --- | --- | --- |
| 任务完成度 | 25% | 7.5 | 7/7 覆盖；3 项验收完全达成，4 项部分达成 |
| 正确性与安全性 | 25% | 8.0 | 0 回归，核心修复正确；新引入了错误文案、剪贴板草稿看不到探测摘要、隐藏窗口时提示丢失 |
| 测试有效性 | 20% | 5.5 | 变异杀伤率 46%；被杀死的恰好是最关键的正确性路径（RM1、RM3、RM4） |
| 工程规范与门禁 | 15% | 7.0 | 登记、注释、i18n、specta 都合规；fmt 失败、没跑 build、AGENTS.md 键数漂移 |
| 报告诚实度 | 15% | 7.5 | 状态标注保守，没做的事情也写明了；有 3 处说法与代码不符 |
| **加权总分** | 100% | **7.15 → 71/100** | **B（合格偏上）** |

分条目：FUN-33 **A**；ARC-56 **A−**；UX-41 **A−/B+**；UX-42 **B+**；ARC-57 **B+**；UX-43 **B**；ARC-58 **C+**；治理与登记 **A**。

---

## 9. 提交前建议修正

按优先级排列：

1. **让 CI 变绿**：运行一次 `cargo fmt --all --manifest-path src-tauri/Cargo.toml`，然后跑 `pnpm verify`。
2. **修正 ARC-58 文案**：把 7 个语言里 `settings.completionIncludeFailuresTip` 改成与实际行为一致；或者改代码，让默认行为真的变成"全部成功才触发"（需要统计本轮是否有失败），并补一条"最后一个任务失败"的测试。
3. **UX-43 二选一，并修正说法**：
   - 方案一：给剪贴板进入的草稿补一个"检测"按钮。
   - 方案二：用户点了 toast 上的动作就视为同意探测。

   两种方案都要同时删掉注释里对 Detect 按钮的描述。窗口隐藏时的提示，要么实现"累积到托盘或悬浮窗"，要么把审计里的说法改正。
4. **补测试**，把存活的变异杀掉：
   - FTP/SFTP 的本轮重试预算（RM2）；
   - ARC-58 的复位和失败路径（RM5、RM6），可以把判断逻辑抽成纯函数来测；
   - UX-41 键清单完整性（RM7），可以断言清单与全部 `SETTING_*` 常量一致，或者改成由宏生成；
   - AppShell 剪贴板回调的三条行为（FM1、FM2、FM4）。
5. **ARC-57**：补暂停路径的端到端故障注入测试，并在审计里写明 supervisor 被 abort 后会跳过 `converge_download_outcome` 这一取舍。
6. **文档**：把 AGENTS.md 里的"33 keys"改成 34。

---

## 10. 修正记录（2026-09-29）

按第 9 节的建议，由评测方直接修复并更新文档。工作区仍未提交。

### 10.1 逐项处理

| 第 9 节条目 | 处理方式 | 主要位置 |
| --- | --- | --- |
| ① CI 变绿 | 运行 `cargo fmt`，`bt.rs` 的格式差异消除 | `src-tauri/src/download/bt.rs` |
| ② ARC-58 文案与行为不符 | **改代码**，实现真正的"本批全部成功"语义：原子标志换成 `CompletionRound`。一轮 = 上次决策后启动的全部任务；真实失败和校验和不匹配在 supervisor 释放槽位之前记下；决策时重新读取失败任务的当前状态（重试成功的不再算失败）；第一个看到排空的 supervisor 关闭本轮，实现去重；用户暂停/取消导致的排空不参与决策。7 个语言的说明文案按此重写 | `scheduler/mod.rs`、7 个 locale |
| ③ UX-43 两个方案 | **选方案二**：点击 toast 上的"使用此链接"即视为同意探测，对话框照常探测，能看到协议、大小、磁盘空间摘要。撤销 `suppressAutoProbe` 整条链路（`NewDownloadDialog.tsx` 回到基准提交），删除所有"Detect 按钮"描述。窗口隐藏时暂存最新一次检测，窗口可见后再弹出 | 新增 `hooks/use-clipboard-link-prompt.ts`，`AppShell.tsx` |
| ④ 补测试 | FTP/SFTP 预算各 1 条（fake FTP 新增一次性 425 注入）；ARC-58 共 5 条，替换原来的同义反复测试；UX-41 键清单改用 `app_setting_keys!` 宏，与常量同源生成，另加唯一性测试；剪贴板行为抽成 hook，加 4 条测试 | `ftp_engine.rs`、`sftp_engine.rs`、`scheduler/mod.rs`、`db/settings.rs`、`use-clipboard-link-prompt.test.ts` |
| ⑤ ARC-57 | 5 处"移除控制 + 排空"合并为 `remove_and_drain_control`（含重新开始路径）；新增暂停路径的组合级故障注入测试；在审计里写明 abort 的取舍 | `lib.rs`、`commands/tasks/actions.rs`、`commands/tasks.rs`、`late_checkpoint_guard.rs` |
| ⑥ 文档 | AGENTS.md 的键数改为 34；主审计 7 个条目各补一条"2026-09-29 复核"记录，与代码不符的说法就地加更正注记（不删除原有依据） | `AGENTS.md`、`docs/project-improvement-audit.md` |

第 4 节提到、第 9 节没有单列的缺口，一并处理：

- **UX-41**：`reset_settings` 和 `update_settings` 的运行时副作用抽成共用的 `apply_settings_side_effects`；"下载目录和 ffmpeg 路径现在也会被重置"这一行为变化已写入审计。
- **UX-42**：状态徽标与诊断改为"直播空闲 / Stream idle"等说法（7 个语言），状态栏图标从 `WifiOff` 换成 `RadioTower`。在浏览器预览里验证时还发现，agent 改造的 mock 直播任务缺少引擎同时写入的 `hls_live_idle` 错误，全新预览会显示原始英文健康摘要；现已补上。

修复过程中新发现一处问题：宽限期到期、supervisor 被 abort 后，它的收尾逻辑不会执行，**请求头缓存条目会残留**；而下次启动先读缓存、后读数据库，残留条目会绕过持久化请求头的过期检查。现在改为由 `remove_and_drain_control` 在排空后清除，测试（RM9）锁住了这一行为。

### 10.2 复测结果

| 门禁 | 修复前 | 修复后 |
| --- | --- | --- |
| `cargo fmt --check` | ❌ | ✅ |
| `cargo clippy --all-targets -D warnings` | ✅ | ✅ |
| `cargo test --locked`（`-j 4`） | 778 / 0 / 2 | **786 / 0 / 2**（净增 8 条） |
| `pnpm verify:frontend`（typecheck、lint、前端测试、release-tools、i18n、protocol-matrix、版本、docs、build、bundle、extensions） | 部分未跑 | ✅ 全部通过；前端 73 个文件、**410 项**（新增 4 项，删除 1 项已不适用的对话框测试） |
| bindings（`pnpm specta` 重新生成后比对） | ✅ | ✅ 一致（本轮没有改动命令签名） |

**仓库既有问题**：`pnpm verify:frontend` 中的 `test:release-tools` 会通过测试导入 `scripts/sync-stable-error-i18n.mjs`，而这个脚本在模块加载时就会重写 7 个 locale 文件的 errors 块。重写后的内容不符合 Biome 格式，还会顺带加入 `errors.unsupportedUrlScheme`，于是**下一次**运行 lint 就会失败。本轮已把 errors 块恢复成基准提交的内容，并在恢复后重新跑了 lint、i18n、typecheck 和前端测试，全部通过。这个脚本本身没有修改，不在本轮范围内。

`pnpm verify:rust` 的第一次运行在编译测试阶段报了 `rustc-LLVM ERROR: out of memory`：默认并行度下一次编译的测试二进制太多，内存不够。这属于环境问题，不是代码错误；fmt 和 clippy 两步都已通过，测试改用 `-j 4` 重跑后全部通过。

**变异复测**：沙箱方法与第 5 节相同，先跑无变异基线（全部通过），再逐个注入。

| 编号 | 注入的缺陷 | 修复前 | 修复后 |
| --- | --- | --- | --- |
| RM1 | HTTP 预算改回从持久化值起算 | 杀死 | 杀死 |
| RM2 | FTP 本轮预算改回从持久化值起算 | 存活 | **杀死** |
| RM2b | SFTP 同上（新增） | — | **杀死** |
| RM3 | BT 限速改回 `Option::min` | 杀死 | 杀死 |
| RM4 | 迟到检查点门控永远判定为"运行中" | 杀死 | 杀死 |
| RM5 | 复位失效：`record_start` 不再重新开轮 | 存活（原变异：删复位语句） | **杀死** |
| RM5-wiring | 删掉 `start_task` 里调用 `record_start` 的那一行 | — | 存活 |
| RM6 | 失败判断失效：总是允许执行 | 存活 | **杀死** |
| RM6-resolve | 决策时忽略已记录的失败 | — | **杀死** |
| RM6-wiring | 删掉 supervisor 里记录失败的那一行 | — | 存活 |
| RM7 | 重置键清单漏掉一个键 | 存活 | **不再可能**（宏同源生成，漏写即编译不过） |
| RM8 | 排空 helper 退回"超时即分离" | — | **杀死** |
| RM9 | 排空 helper 不清除请求头缓存 | — | **杀死** |
| FM1 | 不再丢弃应用自身写入 | 存活 | **杀死** |
| FM2 | 检测到链接后直接打开对话框 | 存活 | **杀死** |
| FM3 / FM4 | 对话框探测抑制相关 | 杀死 / 存活 | **不再适用**（设计改为方案二，这条链路已撤销） |
| FM5 | 重置改回硬编码默认值 | 杀死 | 杀死 |
| FM6 | `isOwnClipboardWrite` 永远返回 false | 杀死 | 杀死 |
| FM7 | 窗口隐藏时不暂存（新增） | — | **杀死** |
| FM8 | 暂存的检测不清除、每次可见都重复弹出（新增） | — | **杀死** |

杀伤率：修复前 46%（6/13）；修复后 **89%（16/18）**，Rust 10/12，前端 6/6。

### 10.3 仍未解决

- **两处接线层变异仍然存活**（RM5-wiring、RM6-wiring）：`start_task` 和 supervisor 都要求真实的 `AppHandle<Wry>`，无法在单元测试里构造。逻辑本身已被锁住，但"调用点被删"这件事目前只能靠代码审阅发现。
- **需要实机验证**：
  - Tauri 窗口隐藏到托盘时，WebView2 是否上报 `document.visibilityState === "hidden"`（PERF-14 的轮询门控依赖同一假设）；
  - NAS/慢盘上的真实暂停；
  - 两个任务几乎同时完成时，完成动作只触发一次。

  所以各条目状态仍是 `Fixed locally`。
- **托盘或悬浮窗气泡提示没有实现**：目前隐藏期间只暂存最新一次检测，等窗口可见后再提示。
- **ARC-57 热路径开销未测量**：每次进度写入多一次 SELECT，并改用 IMMEDIATE 事务，没有做基准测试。
- **abort 的残留风险**：abort 丢弃引擎 future 时，内部 JoinSet 里的 worker 只被 abort、不被 join，已提交到阻塞线程池的文件 I/O 可能在 helper 返回后才结束；数据库一侧由 B 层门控兜底。
- **UX-43 的设计调整需要产品确认**：方案二把"点击 toast 动作"视为同意探测，偏离了审查原文"点击检测或提交时再探测"。如果坚持方案一，需要为剪贴板草稿补一个"检测"按钮。

---

## 附录

### A. 评测时执行的命令

```text
pnpm typecheck / pnpm lint / pnpm check:i18n / pnpm check:docs / pnpm test:frontend
pnpm build && pnpm check:bundle
pnpm specta（先备份 bindings.ts，比对后恢复）
cargo fmt --all --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --locked --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --locked --manifest-path src-tauri/Cargo.toml -j 4
变异测试：复制工作树到临时沙箱，每次注入一个变异（Rust 的 7 个变异彼此不相交，合并编译一次），各自跑对应测试
```

### B. 改动文件清单（agent 产出）

- **Rust**：`commands/settings.rs`、`commands/tasks/actions.rs`、`commands/tasks/mock_seed.rs`、`db/mod.rs`、`db/settings.rs`、`db/task_state.rs`、`download/bt.rs`、`download/ftp.rs`、`download/sftp.rs`、`download/http/segmented/worker.rs`、`lib.rs`、`models/task.rs`、`scheduler/mod.rs`
- **Rust 测试**：`tests/http_engine.rs`、`tests/segments.rs`、新增 `tests/late_checkpoint_guard.rs`
- **前端**：`AppShell.tsx`、`NewDownloadDialog.tsx`、`SettingsPage.tsx`、`EnvironmentPanel.tsx`、`AppErrorBoundary.tsx`、`BatchImportResults.tsx`、`TaskDetails.tsx`、`TaskPassportCard.tsx`、`TaskRecoveryActions.tsx`、`lib/settings.ts`、`lib/tauri.ts`、`lib/tauri-browser.ts`、新增 `lib/clipboard-write.ts`、`generated/bindings.ts`
- **前端测试**：`SettingsPage.test.tsx`、`NewDownloadDialog.test.tsx`、`TaskRow.test.tsx`、新增 `lib/clipboard-write.test.ts`
- **i18n**：7 个语言文件
- **文档**：`docs/project-improvement-audit.md`（新增"2026-09-27 审查登记（第一批：止血）"一节）
