# Vibe Downloader 四维现状审查与功能改进建议（2026-09-27）

审查日期：2026-09-27  
项目版本：`0.5.0`  
基准提交：`611cbbc`（`fix(ffmpeg): drain capped probe output`），工作区仅有未跟踪的 `biome-report.txt`，业务代码无未提交修改。  
范围：用户交互便捷性、程序功能丰富性与完整性、项目架构鲁棒性与稳定性、程序运行效率，以及功能层面的扩展方向。

快速导航：[阅读说明](#1-阅读说明与证据边界) · [总体结论](#2-总体结论) · [用户交互](#3-用户交互便捷性) · [功能完整性](#4-程序功能丰富性和完整性) · [架构与稳定性](#5-项目架构的鲁棒性和稳定性) · [运行效率](#6-程序运行效率) · [功能扩展](#7-功能层面可以添加或改进的方向) · [实施顺序](#8-建议实施顺序) · [验收要点](#9-统一验收要点) · [附录](#附录)

## 1. 阅读说明与证据边界

本文是基于当前源码、本地门禁运行和浏览器预览走查的审查快照，**不修改** [项目改进审计](project-improvement-audit.md) 中任何条目的状态，也不替代其作为唯一风险登记册的地位。本轮新发现使用本地编号 `R27-U`（交互）、`R27-F`（功能）、`R27-A`（架构）、`R27-P`（效率），修复前应先在主审计登记正式 ID 并重新核对源码。

本文刻意不重复已关闭条目。2026-09-21 [四维现状审查](project-review-2026-09-21.md)（`R26-*`）之后，主审计已关闭其中大部分项；延续项的现状见 [附录 A](#附录-a2026-09-21-审查条目的跟踪状态)。

| 证据等级 | 含义 |
| --- | --- |
| 代码确认 | 可达调用链已在当前源码中逐行核对，触发条件明确；尚未在真实 Tauri 安装包中复现 |
| 预览确认 | 在 `pnpm dev` 浏览器预览（mock 数据）中实际操作观察到 |
| 待实测风险 | 机制推导成立，但尚无本项目的测量数据，修复前应先做基准或 fixture |
| 能力缺口 | 当前未实现或明确不支持，属于产品完整性或竞品对标意义上的缺口 |

优先级沿用主审计定义：P0 为发布阻断；P1 为主要工作流错误或长期资源占用，下一候选版本前处理；P2 进入近期迭代；P3 为低频增强或需先测量的优化。

## 2. 总体结论

项目在 0.5.0 基线上已经相当扎实：HTTP 主路径的所有权、取消、提交与续传契约经过多轮修复，恢复中心、存储中心、备份中心、完整性护照、详情诊断和七语言框架都已落地，前端门禁全部通过。**本轮最大的问题不在"缺入口"，而在三个"默认行为"与真实使用场景的错位**：

1. **安全防护的作用范围过宽**。为修补 SSRF 而引入的私有地址拦截在正式版对所有协议、所有来源无条件生效，局域网 NAS、公司内网、`localhost`、Tailscale 等目标全部无法下载，而"允许内网交接"开关在引擎层并不生效（`R27-F01`）。
2. **自动化行为过于积极、恢复行为过于保守**。剪贴板监控默认开启，任何复制的链接都会弹出新建窗口并自动发起网络探测（`R27-U01`）；与此同时，网络中断约 30–90 秒即判整任务失败，没有任务级自动重试、没有网络恢复后续传，重启后默认也不自动恢复（`R27-F02`、`R27-U05`）。
3. **若干"已实现"能力只在任务启动那一刻生效，或实际没有生效**。定时限速窗口、逐任务限速、优先级都无法作用于运行中的任务（`R27-F03`、`R27-U06`）；BT 任务在启动 1 秒后甚至不再受全局限速约束（`R27-F10`）；分段重试预算一旦用完便永久为 0（`R27-A01`）。

| 维度 | 当前优势 | 主要不足 | 优先行动 |
| --- | --- | --- | --- |
| 用户交互便捷性 | 密集列表、命令面板、快捷键、恢复/存储/备份中心、分块图、诚实的状态文案体系 | 剪贴板弹窗与自动探测；后台运行时失败不可见；运行中无法调速；个别文案承诺了不存在的行为 | `R27-U01`、`R27-U02`、`R27-U04`、`R27-U06` |
| 功能丰富性与完整性 | 8 类协议入口、Metalink/HLS/DASH/BT 各有自动化合同测试 | 内网目标全部被拦；无自动重试/网络恢复；定时限速不作用于运行中任务；BT 绕过全局限速；请求头/UA 无法定制；BT 做种占槽 | `R27-F01`、`R27-F02`、`R27-F03`、`R27-F10` |
| 架构鲁棒性与稳定性 | 状态机条件更新、调度锁序、catch_unwind、原子提交、staging 回收 | 分段重试计数不重置；排空超时后 worker 被分离且检查点无条件写回状态；调度队头阻塞；BT 运行时错误成僵尸 | `R27-A01`、`R27-A02`、`R27-A03`、`R27-A04` |
| 程序运行效率 | 虚拟列表、游标分页、250ms 节流、实体缓存上限、bundle 预算 | Windows 预分配可能触发同步零填充（待测）；BT 每任务独立会话且每秒多次写库；若干常驻轮询 | `R27-P01`、`R27-P02` |

### 2.1 最值得先做的十件事

| 顺序 | 编号 | 事项 | 理由 |
| --- | --- | --- | --- |
| 1 | `R27-A01` | 重试/续传时重置分段 `retry_count` | 一行级修复，直接决定"点重试是否真的会重试" |
| 2 | `R27-U01` | 剪贴板监控改为"先提示、不自动探测、忽略自身复制" | 同时消除打扰、隐私外发和 SSRF 的主要触发面 |
| 3 | `R27-F01` | 建立按来源分级的内网信任模型与白名单 | 解除 NAS/内网/Tailscale 场景的整体不可用，且不回退安全边界 |
| 4 | `R27-F10` | 修正 BT 限速同步中的 `Option::min`，并补循环级测试 | 一行级修复；当前全局限速对 BT 实际无效 |
| 5 | `R27-F02` | 任务级自动重试 + 网络感知的等待/恢复 | 下载管理器的核心承诺，当前中断约 30–90 秒即失败 |
| 6 | `R27-F03` | 定时限速与逐任务限速实时作用于运行中任务 | 已宣传能力对长任务实际无效 |
| 7 | `R27-A02` | 暂停/取消排空改为"取消 + 限时等待 + abort + join"，检查点条件写 | 消除慢盘/NAS 目标下的僵尸"下载中"和双 worker 风险 |
| 8 | `R27-U07` | 修正"恢复默认设置"的阈值并改由后端提供默认值 | 明确的缺陷，改动很小 |
| 9 | `R27-U02` | 修正"等待网络"文案或补齐其承诺的行为 | 违背"失败原因清晰"的产品原则 |
| 10 | `R27-A03` | 调度器跳过被阻塞候选后继续向后取 | 多主机混合队列时下载槽空转 |

其后建议尽早安排 `R27-P01`：在 Windows 上测量预分配 + 多段写入的零填充成本，它可能是 Windows 大文件多连接下载的隐性瓶颈。

### 2.2 本轮实际执行的检查

| 检查 | 结果 | 说明 |
| --- | --- | --- |
| `pnpm typecheck` | 通过 | |
| `pnpm lint` | 通过 | Biome 检查 263 个文件 |
| `pnpm check:i18n` | 通过 | en/zh-CN/zh-TW/ja/ko 1840 键，ru 1871、es 1867（含复数变体） |
| `pnpm check:docs` | 通过 | 该门禁只检查 README/AGENTS 与审计的一致性，不覆盖本文列出的语义漂移 |
| `pnpm test:frontend` | 通过 | 71 个文件、397 项，21.2s |
| `cargo test --locked -j 4` | 通过 | 56 个测试二进制，769 项通过、0 失败、2 项按设计忽略（10k/50k 性能基线） |
| 浏览器预览走查 | 完成 | Vite + mock：新手引导、主列表、新建下载、设置、快捷键面板、右键菜单 |
| 未执行 | — | `pnpm build`/`check:bundle`、Clippy、`verify:extensions`、真实 Tauri 安装包与真实协议对端 |

## 3. 用户交互便捷性

### 3.1 应保留的基础

新建窗口的探测摘要（协议、可续传、大小、可用磁盘空间）、草稿保留、批量结果完整保留、恢复方案中量化"重新开始会丢弃多少字节"、分块图、状态栏健康徽标、Tooltip 与焦点环，都是同类工具中少见的"诚实"设计，后续改动不应回退。

### R27-U01｜P1｜剪贴板监控默认开启，任何复制的链接都会弹出新建窗口并自动探测

**证据：代码确认。**

- 默认值：[db/settings.rs:95](../src-tauri/src/db/settings.rs#L95) `clipboard_monitor_enabled` 默认 `true`；[clipboard.rs:11](../src-tauri/src/clipboard.rs#L11) 每秒轮询，`URL_PREFIXES` 覆盖 `http(s)`、`ftp(s)`、`sftp`、`webdav(s)`、`magnet`，不做文件类型过滤。
- 弹窗：[AppShell.tsx:1142-1167](../src/components/shell/AppShell.tsx#L1142) 在没有未保存草稿时直接 `applyClipboardDownload`，即 `setNewDownloadOpen(true)`；窗口隐藏在托盘时同样执行。
- 自动探测：[NewDownloadDialog.tsx:697-701](../src/components/shell/NewDownloadDialog.tsx#L697) 对话框打开且有 URL 时 650ms 后自动 `detect`，后端先发 HEAD，拿不到大小时再发 `Range: bytes=0-0` 的 GET（[http/mod.rs:163-193](../src-tauri/src/download/http/mod.rs#L163)）。
- 自触发：应用自身的"复制下载 URL"（[AppShell.tsx:406](../src/components/shell/AppShell.tsx#L406)）、详情页复制 URL、批量结果"复制失败 URL"都会写入剪贴板，而监控端没有任何"自身写入"的抑制，下一秒即把刚复制的已有任务 URL 弹回新建窗口并再次探测。

**影响：** 用户在浏览器、聊天软件里复制普通网页链接，下载器就会弹出模态窗口，这与 PRODUCT.md 的"安静默认"相冲突；更重要的是，应用会对用户复制的**每一个链接**发起网络请求，包括带令牌的签名链接、一次性登录/找回密码链接、追踪链接。这也是主审计 `SEC-10`/`SEC-12` 当初描述的 SSRF 威胁的主要触发入口，最终促成了 `R27-F01` 中过宽的拦截。

**建议：**

1. 默认改为非模态提示（toast/悬浮窗气泡）："检测到 N 个链接，点击添加"，仅在用户确认后打开新建窗口；从剪贴板进入的草稿不自动探测，由用户点击"检测"或提交时再探测。
2. 增加可配置的文件类型/扩展名白名单（参考 IDM），默认只对明显的下载资源（压缩包、镜像、安装包、媒体、清单文件、`magnet:`、`ftp://` 等）提示。
3. 前端在自身写剪贴板时登记"最近写入的文本"，监控命中相同文本时静默；或在后端比较 Windows `GetClipboardSequenceNumber` 与应用写入序号。
4. 窗口不可见时只累积到托盘/悬浮窗提示，不打开隐藏对话框。

**验收：** 复制普通网页 URL 不弹模态窗口、不产生任何网络请求；复制下载资源链接出现可忽略的提示；应用内"复制下载 URL"后 5 秒内无检测事件；窗口隐藏时不产生探测。

### R27-U02｜P2｜"等待网络"的文案承诺了不存在的自动恢复，且原因描述错误

**证据：代码确认 + 预览确认。** [TaskRow.tsx:482-483](../src/components/tasks/TaskRow.tsx#L482) 对 `waiting_network` 固定显示 `task.diagnostic.waitingNetwork`，文案为"网络不可用，恢复后将自动继续"（[zh-CN.ts:135](../src/i18n/locales/zh-CN.ts#L135)）/"Network unreachable — will resume automatically"（[en.ts:138](../src/i18n/locales/en.ts#L138)）。而后端唯一产生该状态的路径是 HLS 直播播放列表空闲（[hls/engine.rs:699-701](../src-tauri/src/download/hls/engine.rs#L699)、`waiting_network_hls_task`），调度器只派发 `queued`，没有任何代码会把 `waiting_network` 自动恢复。浏览器 mock 还预置了一个真实后端不可能出现的"fonts-bundle.zip 等待网络"场景（[tauri-browser.ts](../src/lib/tauri-browser.ts)），会误导基于预览的界面评审。

**影响：** 直播录制在主播暂停推流后停在"网络不可用"，用户以为会自己恢复而不去操作，实际永远不会继续。

**建议：** 短期按真实原因显示"直播暂无新片段，点继续重新轮询"，并提供"继续/完成录制"按钮；长期在 `R27-F02` 落地网络感知后，再让 `waiting_network` 表达真正的"等待网络恢复"语义。mock 数据应只构造后端可达的状态。

### R27-U03｜P2｜断网或连接停滞时，速度与剩余时间冻结最长 60 秒

**证据：代码确认。** 分段 worker 只有在收到数据块时才上报速度（[worker.rs:405-418](../src-tauri/src/download/http/segmented/worker.rs#L405)）；等待数据块的超时是 60 秒（[http/mod.rs:38](../src-tauri/src/download/http/mod.rs#L38)）。协调器每秒用各段"最后一次上报的速度"求和（[runtime_progress.rs:145-150](../src-tauri/src/download/http/segmented/runtime_progress.rs#L145)）并发出进度事件。

**影响：** 拔掉网线后，列表、状态栏和悬浮窗仍显示原来的速度和剩余时间，直到 60 秒超时后才进入"重试中"。这与"一眼看懂下载健康"的核心承诺相反，也会让 `R27-F02` 的网络感知判断缺少依据。

**建议：** 协调器在每秒 tick 中按"距上次字节到达的时间"衰减各段速度（例如 2 秒无数据即计 0），并在 5–10 秒无数据时把任务健康摘要切换为"连接停滞"；FTP/SFTP/HLS 等引擎复用同一规则。

### R27-U04｜P2｜后台运行时的状态不可见：失败没有系统通知，托盘与任务栏没有进度和告警

**证据：代码确认。** 系统通知只在任务完成时发送（[use-task-events.ts:154-161](../src/hooks/use-task-events.ts#L154)），失败与"需要处理"只有应用内 toast；全仓没有 `set_progress_bar`、托盘 tooltip 或图标状态更新（托盘 tooltip 固定为 "Vibe Downloader"，[lib.rs:955](../src-tauri/src/lib.rs#L955)）。

**影响：** 开启"关闭到托盘"后，夜间批量下载失败要等用户主动打开窗口才会发现；Windows 用户习惯的任务栏绿色进度条也不存在。

**建议：** 失败/需要处理发送系统通知（可合并同一批次、可在设置中分级关闭）；主窗口任务栏显示聚合进度与错误状态（Tauri 2 `Window::set_progress_bar`）；托盘 tooltip 显示"N 个下载中 · 速度 · M 个需要处理"，有问题时切换带角标的托盘图标。

### R27-U05｜P2｜关闭窗口默认直接退出且不确认，重启后默认不自动续传

**证据：代码确认。** `close_to_tray` 默认 `false`（[db/settings.rs:91](../src-tauri/src/db/settings.rs#L91)）；关闭请求在该设置为 false 时直接排空并退出，无确认（[lib.rs:550-569](../src-tauri/src/lib.rs#L550)）。`auto_resume_on_startup` 默认 `false`（[db/settings.rs:93](../src-tauri/src/db/settings.rs#L93)），重启后所有进行中任务被置为"已暂停"（[task_state.rs:1082-1107](../src-tauri/src/db/task_state.rs#L1082)）。

**影响：** 新用户点窗口右上角 X 即中止全部下载；下次启动它们停在暂停状态，需要逐一或"全部开始"恢复。PRODUCT.md 的核心承诺之一是"从崩溃和网络故障中自动恢复"。

**建议：** 有活动下载时关闭窗口给出一次性选择（"最小化到托盘 / 退出并暂停 / 取消"，可勾选记住）；首次引导中询问启动恢复策略，或将默认值改为自动恢复（仍尊重用户手动暂停的任务，现有 `paused_by_schedule` 的事件区分可复用）。

### R27-U06｜P2｜运行中无法调整单任务限速与优先级，必须先暂停

**证据：代码确认。** 详情页传输设置在下载中/重试中整体禁用（[TaskDetails.tsx:1637](../src/components/shell/TaskDetails.tsx#L1637)）。后端 `update_task_transfer_options` 只写数据库（[actions.rs:138-187](../src-tauri/src/commands/tasks/actions.rs#L138)），运行中的子限速器在任务启动时创建后不再更新，`DownloadControl` 也不持有限速器句柄（[lib.rs:36-45](../src-tauri/src/lib.rs#L36)）。只有全局限速是实时的（共享父限速器）。

**影响：** 想临时给某个大文件让路，只能暂停它；对不支持续传的来源，暂停意味着丢弃全部进度。IDM 等工具允许对单个下载实时调速。

**建议：** `DownloadControl` 持有任务子限速器的 `Arc`，`update_task_transfer_options` 在任务运行时直接 `set_limit`；优先级变化只影响排队任务，界面说明即可。与 `R27-F03` 一起实现"限速策略运行时重算"。

### R27-U07｜P2｜"恢复默认设置"写入的多连接阈值与真实默认值不一致

**证据：代码确认。** [SettingsPage.tsx:937-941](../src/components/settings/SettingsPage.tsx#L937) 重置时写入 `multiConnectionThresholdBytes: "1048576"`（1 MiB），而后端默认值是 16 MiB（[db/mod.rs:175](../src-tauri/src/db/mod.rs#L175)，AGENTS.md 同样记载 16 MB）。整组默认值在前端另写一份，任何后端默认调整都会再次漂移。

**影响：** 用户"恢复默认"后，1–16 MiB 的小文件也会被切成多段，对服务器更激进，行为与首次安装不一致。

**建议：** 新增后端 `reset_settings`（或 `get_default_settings`）命令，由后端常量生成默认值；前端测试断言"重置后的设置 == 全新数据库读出的设置"。

### R27-U08｜P3｜新建窗口的几个细节

**证据：代码确认 + 预览确认。**

- 只有"开始下载"一个提交方式，任务总以 `Queued`、`obey_schedule = true` 创建（[create.rs:990](../src-tauri/src/commands/tasks/create.rs#L990)、[create.rs:1004](../src-tauri/src/commands/tasks/create.rs#L1004)），缺少"仅加入队列（暂停添加）""定时开始"和"本任务不受计划窗口约束"。
- "更多下载选项"中优先级、限速单位、代理模式三个下拉框没有可访问名称（可见文字是普通 `span`，未与控件关联），子目录和限速输入仅靠 placeholder 命名；读屏只会读出"组合框"。这是 `UX-18` 修复范围之外的残留。

**建议：** 提交按钮旁增加"添加但不开始"；把可见标签改为 `<label htmlFor>` 或 `aria-labelledby`，并为新建窗口补 `jest-axe` 用例（项目已依赖 `jest-axe`）。

### R27-U09｜P3｜已完成任务的时间与操作不完整

**证据：代码确认 + 预览确认。** 行内"完成于 {{time}}"只显示时分（[TaskRow.tsx:843-845](../src/components/tasks/TaskRow.tsx#L843)），且取自 `updatedAt`，后续校验、改分类都会改变它；一周前完成的任务显示"完成于 14:05"。已完成任务的右键菜单只有打开文件/文件夹、查看详情、复制 URL/路径和删除，缺少"重新下载""重新校验""重命名/移动"；由于 `Completed` 是终态，重新下载只能复制 URL 再新建。

**建议：** 当天显示时分、其他日期显示"昨天/月-日"；持久化独立的 `completed_at`；菜单增加"重新下载（新建同配置任务）"和"校验文件"，文件移动类能力随 `FE-06` 实现。

### R27-U10｜P3｜磁力多文件的选择流程经由"需要处理"状态

**证据：代码确认。** 多文件磁力链接在元数据就绪后被置为 `NeedsAttention`（错误码 `bt_file_selection_required`，[bt.rs:650-688](../src-tauri/src/download/bt.rs#L650)），于是它会出现在"需要处理"视图、状态栏健康徽标和恢复中心里，与真正的故障混在一起。

**建议：** 引入"等待选择文件"的明确子状态（或非故障类 attention），在任务行内直接提供"选择文件"按钮；新建窗口可提供"等待元数据后选择"的选项。

### R27-U11｜P3｜键盘与批量效率的剩余缺口

- 没有全局 Ctrl+V 从剪贴板直接新建（`pasteAndCreate` 只挂在空列表状态，[AppShell.tsx:851](../src/components/shell/AppShell.tsx#L851)）。
- Ctrl+A 只选择已加载的任务（[AppShell.tsx:1433-1447](../src/components/shell/AppShell.tsx#L1433)），筛选全集操作仍缺（`R26-U06` 延续）。
- 快捷键帮助面板已补齐 Ctrl+F、Alt+↑↓、Shift+Del，但键位匹配（AppShell 中的字面量）与帮助列表仍是两份定义（`R26-U09` 部分完成）。
- 批量导入 UI 仍固定 `allowDuplicate: false`（[NewDownloadDialog.tsx:934](../src/components/shell/NewDownloadDialog.tsx#L934)），后端已支持（`R26-F02` 前半已修）。

### R27-U12｜P3｜文案与分类的小漂移

- 前端 `failureKind` 在缺少错误码时仍按英文子串分类（[task-query.ts:202-210](../src/stores/task-query.ts#L202)），与 `ARC-30` "按码分类"的方向不一致。
- [i18n/index.ts:42](../src/i18n/index.ts#L42) 注释仍称稳定语言约 670 键，实际为 1840 键。
- 设置中的"允许内网交接"会让用户以为内网下载可用，实际引擎层仍会拦截（见 `R27-F01`）。

## 4. 程序功能丰富性和完整性

### R27-F01｜P1｜私有与保留地址在正式版被所有协议无条件拦截，局域网场景整体不可用

**证据：代码确认。**

- [ssrf.rs:133-143](../src-tauri/src/download/ssrf.rs#L133)：`intranet_guard_bypassed()` 在非 debug/test 构建中恒为 `false`，唯一旁路是测试钩子和 `VIBE_TEST_ALLOW_INTRANET`（仅 debug 构建读取）。
- HTTP 家族：共享客户端的 DNS 解析器过滤所有私有/保留 IP 且没有任何旁路（[net_factory.rs:116-129](../src-tauri/src/download/net_factory.rs#L116)）；IP 字面量在 [http/request.rs:50](../src-tauri/src/download/http/request.rs#L50)、[:76](../src-tauri/src/download/http/request.rs#L76) 被 `assert_public_authority` 拒绝；HLS、DASH、Metalink、WebDAV、BT 种子抓取同样接入。
- FTP/SFTP：[ftp.rs:1384](../src-tauri/src/download/ftp.rs#L1384)、[sftp.rs:1431](../src-tauri/src/download/sftp.rs#L1431) 建连前做字面量 + DNS 审查。
- 被拦截的范围包括 RFC1918（10/8、172.16/12、192.168/16）、回环与 `localhost`、链路本地、CGNAT 100.64/10（Tailscale 的地址段）、IPv6 ULA/链路本地（[ssrf.rs:35-65](../src-tauri/src/download/ssrf.rs#L35)）。
- "允许内网交接"只放行浏览器交接边界的预检（[browser.rs:1240](../src-tauri/src/commands/browser.rs#L1240)），创建后的探测与下载仍被引擎层拒绝，因此该开关端到端无效；[browser-extension-privacy.md:41](browser-extension-privacy.md) 却写明"用户在桌面应用中显式启用后可访问内网"。
- 主审计 `SEC-10` 的修复记录已注明"字面量私网 HTTP 下载（如 NAS）不再可达……显式内网白名单是后续产品项"（[project-improvement-audit.md:1299](project-improvement-audit.md)）；`SEC-12` 的验收条件包含"开启白名单后行为可解释且有提示"，但白名单并未实现，条目已标 Closed。

**影响：** 家用 NAS（群晖/威联通的 SFTP、FTP、WebDAV、HTTP 共享）、公司内网文件服务器、本机开发服务器、Tailscale/ZeroTier 组网中的设备全部无法作为下载源。FTP/SFTP/WebDAV 恰恰主要用于这些场景，因此这三类协议在正式版中的实际可用面大幅缩小。错误提示为"已被 SSRF 防护拦截"，普通用户无法理解也无法自行解除。

**建议：** 按请求来源建立信任分级，而不是一刀切：

| 来源 | 私有地址策略 |
| --- | --- |
| 用户手动输入/确认的 URL（新建窗口、批量导入、文件拖放） | 允许，界面标注"内网地址" |
| 浏览器交接、剪贴板自动检测 | 需要 `allow_intranet_handoff` 或白名单命中 |
| 从公网目标重定向到私网、公网清单（m3u8/mpd/meta4/torrent）声明的私网子资源 | 始终拒绝（DNS rebinding 与跨源探测防线保持不变） |
| 云元数据地址（169.254.169.254、fd00:ec2::254 等） | 始终拒绝 |

同时提供"允许的内网主机/网段"设置（CIDR 与主机名），决策集中在 `ssrf` 模块并由任务携带"来源信任级别"贯穿探测、重试、续传。修复 `R27-U01` 后，剪贴板不再自动探测，原威胁模型的主要入口随之消失。

**验收：** 手动输入 `http://192.168.x.x/…`、`sftp://nas.local/…`、`http://100.x.y.z/…` 可创建并完成下载；浏览器交接与剪贴板在未授权时仍被拒并给出可操作提示；公网 URL 302 到私网、公网 m3u8 引用私网分片、元数据地址在任何来源下均被拒；开关与白名单的 UI 文案与行为一致。

### R27-F02｜P1｜缺少任务级自动重试与网络感知恢复，各引擎的重试预算极短

**证据：代码确认。**

- HTTP 分段：单段最多重试 5 次，退避 1、2、4、8、15 秒（[segmented/mod.rs:44](../src-tauri/src/download/http/segmented/mod.rs#L44)、[worker.rs:495-513](../src-tauri/src/download/http/segmented/worker.rs#L495)），用尽后协调器中止所有段并把**整个任务**置为失败（[coordinator.rs:293-336](../src-tauri/src/download/http/segmented/coordinator.rs#L293)）。
- 探测阶段 HTTP 请求重试 3 次、间隔 25ms（[retry.rs:37-44](../src-tauri/src/download/retry.rs#L37)）；FTP/SFTP/HLS 为 3 次尝试、总等待约 1.5 秒（[retry.rs:47-77](../src-tauri/src/download/retry.rs#L47)；HLS 实际使用 `HLS_SEGMENT_RETRIES = 2`，[hls/engine.rs:50](../src-tauri/src/download/hls/engine.rs#L50)）。
- `retry_after_at` 只由用户点击"5 分钟后重试"写入（[actions.rs:897](../src-tauri/src/commands/tasks/actions.rs#L897)、[recovery.rs:201](../src-tauri/src/commands/recovery.rs#L201)）；`schedule_retry_after_wakeup` 只在启动时调用一次（[lib.rs:817-819](../src-tauri/src/lib.rs#L817)）。
- `WaitingNetwork` 仅由 HLS 直播空闲使用（见 `R27-U02`），没有网络连通性检测，也没有设置项调整重试次数与间隔。

**影响：** Wi-Fi 重连、路由器重启、VPN 切换、笔记本合盖唤醒等持续约 30–90 秒以上的中断（取决于连接是立即报错，还是先停滞 60 秒才超时）都会让大文件任务变成"失败"，夜间无人值守下载在第一次抖动后就停止。结合 `R27-A01`，用户点一次"重试"后，下一次抖动会立即再次失败。

**建议：**

1. 任务级自动重试策略：可重试错误（网络、5xx、429、超时）失败后进入 `waiting_network` 或带 `retry_after_at` 的排队状态，按指数退避加抖动自动重试 N 次（默认如 10 次、上限 30 分钟），并尊重 `Retry-After`（同时支持 HTTP-date 格式，当前 [request.rs:121-129](../src-tauri/src/download/http/request.rs#L121) 只解析秒数）。
2. 网络感知：监听系统网络变化（或低频探测默认网关/公共端点），网络恢复时分批唤醒 `waiting_network` 任务，避免雪崩重连。
3. 调度器维护"下一次重试时间"的唤醒定时器，而不是只在启动时计算一次。
4. 设置中暴露"最大重试次数/重试间隔/连接与读取超时"，并在任务详情中显示"第 k 次自动重试，将于 hh:mm 继续"。
5. 认证失败、远端文件变化、磁盘写失败等不可重试错误保持现状，不进入自动重试。

**验收：** 本地 fake server 模拟 2 分钟断连后恢复，任务在无人操作下完成且字节正确；认证失败不自动重试；重试退避与 `Retry-After` 被遵守；网络恢复时不超过设定并发数的任务同时重连。

### R27-F03｜P1｜定时限速窗口只在任务启动时生效

**证据：代码确认。** 调度器在 worker 启动时读取一次设置，按"当前是否处于限速时段"计算子限速器的上限（[scheduler/mod.rs:465-487](../src-tauri/src/scheduler/mod.rs#L465)），此后没有任何代码在时段开始/结束或设置变更时更新运行中任务的限速器。计划窗口监视器只负责下载时段的暂停/恢复（[tasks.rs:430-532](../src-tauri/src/commands/tasks.rs#L430)），`update_settings` 只更新全局限速器（[settings.rs:268-273](../src-tauri/src/commands/settings.rs#L268)）。BT 引擎虽每秒同步一次会话限速，但使用的是任务启动时的记录与子限速器，而且在任务没有单独限速时会把会话限速清为不限速（见 `R27-F10`）。

**影响：** 典型场景"晚上 18:00–23:00 限速 1 MB/s，给家人看视频让路"：17:00 开始的大文件下载整晚不受限；反之 22:00 启动的任务在 23:00 后仍被限速到任务结束。

**建议：** 把"有效限速 = min(任务限速, 定时窗口限速)"做成可重算的策略对象：调度器持有每个运行任务的子限速器句柄，计划窗口监视器在限速窗口边界与设置变更时重算并 `set_limit`；与 `R27-U06` 共用同一机制。

**验收：** 任务运行中跨越限速窗口开始与结束边界，实测吞吐在边界后 2 秒内切换；修改限速设置后运行任务即时生效；BT、HTTP、FTP、SFTP、HLS 同一合同测试。

### R27-F10｜P1｜BT 任务在启动 1 秒后不再受全局限速与定时限速约束

**证据：代码确认。** BT 主循环每秒执行（[bt.rs:1029-1036](../src-tauri/src/download/bt.rs#L1029)）：

```rust
sync_session_download_limit(
    &api,
    db::parse_speed_limit_bps(task.task_speed_limit_bps.as_deref())
        .min(speed_limiter.current_limit_bps().or(Some(i64::MAX))),
);
```

Rust 中 `Option` 的排序规定 `None < Some(_)`，因此当任务**没有**单独限速（最常见的情况，左侧为 `None`）时，`None.min(Some(全局限速))` 的结果是 `None`，`sync_session_download_limit(None)` 会把 librqbit 会话的下载限速清除为"不限速"。会话创建时本已正确应用了 `speed_limiter.current_limit_bps()`（[bt.rs:519-527](../src-tauri/src/download/bt.rs#L519)），但第一次循环同步后就被覆盖。现有测试 `sync_session_download_limit_updates_live_session`（[bt.rs:2379-2423](../src-tauri/src/download/bt.rs#L2379)）只直接调用同步函数，没有覆盖这个组合表达式。注释中"mid-transfer changes … take effect"的说法同样不成立：表达式读取的是启动时捕获的 `task` 记录。

**影响：** 设置了全局限速（或定时限速窗口）的用户，BT 下载在启动约 1 秒后即跑满带宽，与 AGENTS.md/README 中"逐任务与全局限速对 BT 生效、取最小值"的描述不符；另外 BT 流量本就不经过共享令牌桶，HTTP 与 BT 同时下载时总流量可超过全局上限。

**建议：** 改为显式的 `min_optional_limit`（调度器中已有同名函数，[scheduler/mod.rs:882-888](../src-tauri/src/scheduler/mod.rs#L882)）并每次重新读取任务当前限速；补一条"无单独限速 + 有全局限速"的循环级测试。长期让 BT 流量计入全局预算（例如按全局剩余额度动态分配会话上限）。

### R27-F04｜P2｜未知大小（chunked）HTTP 下载没有重试也无法续传

**证据：代码确认。** 无 `Content-Length` 的下载走 `run_unknown_size_download`：每次启动都删除旧临时文件从零开始（[segmented/mod.rs:195-199](../src-tauri/src/download/http/segmented/mod.rs#L195)），读取过程中任何网络错误或 60 秒停滞都直接返回错误（[segmented/mod.rs:272-277](../src-tauri/src/download/http/segmented/mod.rs#L272)），没有 worker 级重试。

**影响：** 动态生成的导出文件、部分 CDN 的 chunked 响应、流式打包下载在网络抖动时必然从头再来。

**建议：** 已写入字节 > 0 且服务端声明 `Accept-Ranges: bytes` 时，用 `Range: bytes=N-` 续传并校验 `206` 与 `Content-Range` 起点；不支持 Range 时至少提供与分段 worker 一致的"从头重试 N 次"，并在界面标明"无法续传"。

### R27-F05｜P2｜用户创建的任务无法自定义 User-Agent、Referer、Cookie 与请求头

**证据：代码确认。** 所有 HTTP 客户端的 UA 固定为 `VibeDownloader/0.5.0`（[net_factory.rs:169](../src-tauri/src/download/net_factory.rs#L169)）；`CreateTaskInput` 没有请求头字段（[create.rs:34-70](../src-tauri/src/commands/tasks/create.rs#L34)）；自定义请求头只能经由浏览器交接转发，而 candidate/release 扩展不含 Cookie/header 转发能力。

**影响：** 需要登录态（Cookie）、防盗链（Referer）或拒绝非浏览器 UA 的站点，在正式版中只能下载失败或拿到 HTML 页面；这是 IDM、aria2（`--header`、`--user-agent`、`--referer`）的基础能力。

**建议：** 站点规则扩展为"站点请求配置"：按域名配置 UA、Referer、额外请求头、Cookie（加密存储，沿用 `task_request_headers` 的加密与源绑定规则 `SEC-11`）、每主机连接数和代理；新建窗口高级选项允许一次性覆盖。敏感头继续只发往同源。

### R27-F06｜P2｜BT 做种占用下载槽、完成后无法开始做种、重启后不恢复，与路线图表述不符

**证据：代码确认。** 做种循环运行在下载 supervisor 内部（[bt.rs:890-1009](../src-tauri/src/download/bt.rs#L890)），任务的 `DownloadControl` 在做种期间一直留在 `downloads` 中，调度器按其计数占用 `max_active_tasks`（默认 2，[scheduler/mod.rs:188](../src-tauri/src/scheduler/mod.rs#L188)），完成动作也因 `downloads` 非空在做种期间不会触发，未设置比例/时长限制时即永不触发（[scheduler/mod.rs:815-830](../src-tauri/src/scheduler/mod.rs#L815)）。做种默认关闭，而在任务完成后打开做种开关只写数据库，没有重新加入会话的路径（`Completed` 为终态）；应用重启后已完成任务也不会恢复做种。[ROADMAP.md:173](ROADMAP.md) 却写明"在不占用普通下载槽的前提下执行做种比例与时长限制"。

**建议：** 做种任务交给独立的"做种管理器"，不占下载槽也不阻塞完成动作；允许对已完成任务开始/停止做种；持久化做种状态并在启动时恢复；设置中提供全局默认做种策略（比例/时长/上传限速）。同时修正路线图表述或实现。

### R27-F07｜P2｜HLS/DASH 的媒体完整性缺口

**证据：代码确认（字幕一项为高度可疑，待 fixture 复现）。**

- 字幕封装：选中的外挂字幕轨与视频一起以 `-c copy` 输出为 `.mp4`（[hls/engine.rs:2025-2031](../src-tauri/src/download/hls/engine.rs#L2025)）。HLS 字幕通常是 WebVTT，而 MP4 容器只接受 `mov_text`，ffmpeg 在这种组合下通常报"codec not currently supported in container"并失败；现有集成测试的字幕选择全部为 `None`，未覆盖这条路径。
- 直播容错：任一分片 3 次尝试失败即整任务失败（[hls/engine.rs:955-968](../src-tauri/src/download/hls/engine.rs#L955)、[:1047](../src-tauri/src/download/hls/engine.rs#L1047)），直播录制无法"跳过缺口继续录"，而直播的过去片段无法重下。
- ffmpeg 依赖：HLS/DASH 在没有 ffmpeg 时完全不可用（探测阶段即拒绝），应用不内置也不提供一键获取；MPEG-TS 分片本可无损拼接为 `.ts`，fMP4 可与 init 段拼接。
- DASH 轨道：仍按每个 AdaptationSet 取最高码率，后出现的同类集合覆盖前者（[dash.rs:841-850](../src-tauri/src/download/dash.rs#L841)），无语言、画质、字幕选择（`R26-F07` 延续）。

**建议：** 字幕输出为同名 `.vtt`/`.srt` 旁挂文件，或在 MP4 输出时对字幕流使用 `-c:s mov_text`，并补 WebVTT fixture；直播模式允许配置"最多容忍 N 个缺失片段"，缺口写入时间线与完整性护照；无 ffmpeg 时提供"保存为 .ts/.m4s 拼接"降级与环境诊断中的获取引导；DASH 先在探测结果中展示自动选择及理由，再开放显式选择。

### R27-F08｜P2｜FTPS 只信任内置根证书，FTP 仅支持被动模式

**证据：代码确认。** FTPS 连接器只装载 `webpki_roots` 的 Mozilla 根证书（[ftp.rs:1572-1577](../src-tauri/src/download/ftp.rs#L1572)），不读取系统证书库，也没有自签名证书信任或指纹固定；所有连接固定 `Mode::Passive`（[ftp.rs:159](../src-tauri/src/download/ftp.rs#L159) 等三处）。

**影响：** NAS 的自签名 FTPS、企业内网用私有 CA 签发的 FTPS 均握手失败；少数只支持主动模式的旧服务器不可用。与 `R27-F01` 叠加后，FTP 类协议在局域网场景几乎没有可用路径。

**建议：** 采用平台证书校验（与 HTTP 一致），并为 FTPS/HTTPS 提供类似 SFTP TOFU 的"首次信任该证书指纹"流程，指纹持久化并可在设置中撤销；主动模式作为高级选项。

### R27-F09｜P3｜其他功能细节

- `Retry-After` 不支持 HTTP-date 格式（[request.rs:121-129](../src-tauri/src/download/http/request.rs#L121)）。
- 探测阶段的 HTTP 请求重试间隔 25ms，瞬时 DNS 故障几乎等同无重试（[retry.rs:37-44](../src-tauri/src/download/retry.rs#L37)）。
- 完成动作的"运行命令"只能在整个队列完成后执行一次，且不支持 `{path}`/`{name}`/`{dir}` 等占位符，无法做"每个文件下载后解压/扫描/归档"（见 [第 7 节](#7-功能层面可以添加或改进的方向)）。
- 浏览器扩展只提供"用 Vibe 下载此链接/选中 URL"两个菜单项（[background.js:111-121](../browser/extension-core/src/background.js#L111)），没有页面链接批量抓取。

### 4.1 2026-09-21 审查功能项的延续状态

| 编号 | 事项 | 当前状态 |
| --- | --- | --- |
| `R26-F05` | 媒体/清单只按 URL 后缀路由，不看 Content-Type | 仍开放（[url_classify.rs](../src-tauri/src/download/url_classify.rs) 全部为后缀判断；本地 `.m3u8` 也不支持） |
| `R26-F07` | DASH 画质/语言/字幕选择 | 仍开放（并入 `R27-F07`） |
| `R26-F08` | 远端目录无法转为递归下载计划 | 仍开放 |
| `R26-F09` | BT 实时 tracker 状态 | 仍开放（路线图明确延期） |
| `R26-F10` | 浏览器集成中心后端未接入前端 | 仍开放：4 个命令只出现在 `bindings.ts`，`tauri.ts`、mock 与组件均未引用 |
| `R26-F02` | 批量 `allowDuplicate` | 后端已按参数执行；批量 UI 入口仍缺 |
| `R26-F12` | 真实服务器、真实安装包验收 | 仍开放 |

## 5. 项目架构的鲁棒性和稳定性

### R27-A01｜P1｜HTTP 分段的 `retry_count` 永不重置，失败后"重试"的实际重试预算为 0

**证据：代码确认。** worker 以数据库中的 `segment.retry_count` 作为本次运行的起点（[worker.rs:71](../src-tauri/src/download/http/segmented/worker.rs#L71)），并与 `MAX_SEGMENT_RETRIES = 5` 比较（[worker.rs:99](../src-tauri/src/download/http/segmented/worker.rs#L99)）；每次重试都把递增后的计数写回（[runtime_progress.rs:192](../src-tauri/src/download/http/segmented/runtime_progress.rs#L192)、[segments.rs:341-356](../src-tauri/src/db/segments.rs#L341)）。而暂停、恢复、重试、启动恢复所用的 `update_segments_status_for_task(_in_tx)` 只改状态与错误，不重置 `retry_count`（[segments.rs:544-567](../src-tauri/src/db/segments.rs#L544)），`reset_interrupted_tasks` 同样不重置（[task_state.rs:1109-1118](../src-tauri/src/db/task_state.rs#L1109)）。只有"重新开始"删除全部分段后计数才归零。

**触发与影响：** 某段在一次网络中断中用尽 5 次重试 → 任务失败 → 用户点"重试" → 该段下一次瞬时错误**不再重试**，整个任务立即失败。长时间、跨天、跨重启的大文件下载会逐步累积计数，越到后期越脆弱。这与 `R27-F02` 叠加，是当前 HTTP 主路径最影响可靠性体验的缺陷。

**建议：** 用户重试/恢复、调度器派发时把运行时预算与历史统计分开：`retry_count` 继续作为诊断累计值，worker 使用每次运行从 0 开始的本地计数；或在 `transition_task_with_runtime_state` 进入 `Queued` 时重置未完成分段的计数。

**验收：** fake server 让某段连续失败 6 次使任务失败；点"重试"后再注入 1 次瞬时错误，任务应重试并完成；跨应用重启后同样成立。

### R27-A02｜P2｜暂停/取消/重试的排空超时后 worker 被分离，迟到的检查点无条件写回 `downloading`

**证据：代码确认。**

- `pause_task`、`retry_task`、`retry_task_with_mirror`、`cancel_task` 在移除 `DownloadControl` 后执行 `tokio::time::timeout(5s, handle)`（[actions.rs:255-266](../src-tauri/src/commands/tasks/actions.rs#L255)、[:378-389](../src-tauri/src/commands/tasks/actions.rs#L378)、[:424-435](../src-tauri/src/commands/tasks/actions.rs#L424)、[:516-527](../src-tauri/src/commands/tasks/actions.rs#L516)）；超时后 `JoinHandle` 随 `Timeout` 被丢弃，任务被**分离**而非中止。`ARC-45` 为"重新开始"引入的 `cancel_and_drain_control`（先等待、超时后 abort 并 join，[lib.rs:201-218](../src-tauri/src/lib.rs#L201)）没有推广到这些路径。
- 协调器在取消后会做最后一次强制检查点，状态参数固定为 `Downloading`（[coordinator.rs:469-477](../src-tauri/src/download/http/segmented/coordinator.rs#L469)）；`checkpoint_task_progress` 的 `UPDATE tasks SET … status = ? WHERE id = ?` 没有状态条件（[task_state.rs:915-929](../src-tauri/src/db/task_state.rs#L915)），工作单元状态同样被覆盖。

**触发与影响：** 当排空超过 5 秒（下载目标是 NAS/SMB 共享、休眠中的 USB 硬盘、杀毒软件锁文件，或数据库繁忙）时，`pause_task` 先把任务写成 `Paused`，随后迟到的检查点把它改回 `downloading`：数据库中出现没有 worker 的"下载中"僵尸任务，而界面仍停留在"已暂停"；此时点"继续"会因 `Downloading → Queued` 不是合法转移而报错，直到重启才被 `reset_interrupted_tasks` 纠正。若用户在迟到检查点发生之前就点"继续"，新旧两个 worker 可能同时写同一临时文件和同一批工作单元记录。

**建议：** 所有用户控制路径统一使用 `cancel_and_drain_control`（限时等待 + abort + join）；检查点写入改为条件更新 `WHERE id = ? AND status IN ('downloading','retrying')`，取消路径只写字节进度不写状态；FTP/SFTP/HLS/DASH/Metalink 的收尾写入同样检查。

**验收：** 在 worker 刷盘处注入 8 秒延迟，暂停后任务稳定停在 `Paused`，无残留 worker；暂停后立即继续不会出现两个写者。

### R27-A03｜P2｜调度器只看队首 N 个候选，存在队头阻塞

**证据：代码确认。** 每次派发只读取 `max_active_tasks × max_connections_per_host`（下限 `max_active_tasks`，上限 100）个排队任务（[scheduler/mod.rs:171-183](../src-tauri/src/scheduler/mod.rs#L171)）。循环对"主机连接已满"和"不在下载时段"的候选执行 `continue`（[scheduler/mod.rs:229-250](../src-tauri/src/scheduler/mod.rs#L229)），但不会继续向后读取。

**触发与影响：** 例如 `max_active_tasks = 4`、单主机 8 连接：队首 32 个任务都来自同一主机 A 且 A 已满时，排在后面的主机 B 任务永远不会被看到，两个下载槽空转；所有任务默认 `obey_schedule = true`，若队首一批都受时段约束，后面不受约束的任务同样被饿死。

**建议：** 查询层直接排除已满主机与受时段约束的任务（SQL 条件带入 `host_slot_map` 中已满的 `source_key` 列表），或分页继续读取直到填满可用槽位/扫描到队尾；补一条"多主机混排"调度测试。

### R27-A04｜P2｜BT 运行时错误不会使任务失败，任务停在"下载中"

**证据：代码确认。** BT 主循环每秒读取 `api_stats_v1`，`stats.error`（librqbit 的 torrent 进入错误状态，如磁盘写满、权限不足、文件被删除）只写入运行时快照的 `last_error_*` 字段（[bt.rs:867-868](../src-tauri/src/download/bt.rs#L867)），循环本身不退出，任务状态持续写为 `Downloading`（[bt.rs:809-817](../src-tauri/src/download/bt.rs#L809)）。

**影响：** 磁盘写满后 BT 任务显示"下载中、0 B/s、无 peer"，不会进入失败或需要处理，恢复中心和状态栏健康徽标都看不到它；也不会释放下载槽。

**建议：** `stats.error` 出现时按错误类型转为 `Failed`/`NeedsAttention`（磁盘类映射到 `disk_write_failed` 与"释放空间"恢复动作），并对"长时间 0 进度且 0 peer"给出停滞诊断。

### R27-A05｜P2｜计划窗口监视器的休眠时长不随设置、系统休眠和时钟变化重算

**证据：代码确认。** 监视器在循环开头按当时设置计算"距下一个边界的时长"并整段 `sleep`（[tasks.rs:490-532](../src-tauri/src/commands/tasks.rs#L490)）；设置变更时只做一次即时检查（[settings.rs:282](../src-tauri/src/commands/settings.rs#L282)），不会唤醒或重排这次休眠。`tokio::time::sleep` 基于单调时钟，在 Linux/macOS 上系统睡眠期间不计时。

**触发与影响：** 当前 20:00、原时段 22:00 开始，用户改为 20:30 开始：20:30 不会恢复任务，直到原定的 22:00 醒来重算；笔记本合盖跨过边界后，开盖可能迟迟不切换；夏令时切换当天边界偏移。

**建议：** 监视器改为"最长睡 60 秒 + 每次醒来按墙钟判定"，或用 `Notify` 在设置变更时立即重排；启动、唤醒（系统电源事件）时强制判定一次。与 `R27-F03` 的限速窗口共用一个"计划策略"组件。

### R27-A06｜P3｜删除任务只 abort 不 join，随即删除临时文件

**证据：代码确认。** `delete_task` 与 `bulk_delete_tasks` 调用 `h.abort()` 后立刻删除临时文件和 staging（[actions.rs:572-620](../src-tauri/src/commands/tasks/actions.rs#L572)、[:668-716](../src-tauri/src/commands/tasks/actions.rs#L668)）。abort 只在下一个 await 点生效，Windows 上 worker 仍持有句柄时删除会失败，失败仅记日志（[actions.rs:620-625](../src-tauri/src/commands/tasks/actions.rs#L620)）。这正是 `ARC-45` 在重新开始路径上修复过的同一问题。

**影响：** "删除任务及文件"后磁盘上残留大体积临时文件，直到下次启动的孤儿清扫才可能回收。

**建议：** 删除路径同样使用 `cancel_and_drain_control`，失败的文件删除进入存储中心的"待清理"列表并提示用户。

### R27-A07｜P3｜完成动作可能重复触发，最后一个任务失败时又不触发

**证据：代码确认。** 每个 supervisor 在成功后独立调用 `maybe_emit_completion_action`（[scheduler/mod.rs:529-554](../src-tauri/src/scheduler/mod.rs#L529)），判定条件只看 `downloads` 为空、队列为空、无哈希进行中（[scheduler/mod.rs:815-830](../src-tauri/src/scheduler/mod.rs#L815)），没有"本轮已触发"的去重；两个任务几乎同时完成时，两者都可能通过判定，"运行命令"会执行两次，关机/睡眠会弹两次。另一方面，只有成功路径会调用它，队列最后一个任务失败时完成动作不会发生。

**建议：** 用原子标志或"完成动作世代号"去重，队列重新有任务时复位；让用户选择"全部结束（含失败）时执行"还是"全部成功时执行"。

### R27-A08｜P3｜应用更新与系统注销不经过停机排空

**证据：代码确认。** 停机排空只挂在主窗口关闭和托盘"退出"两条路径（[lib.rs:523-570](../src-tauri/src/lib.rs#L523)、[tray.rs:42-60](../src-tauri/src/commands/tray.rs#L42)）。更新安装直接 `downloadAndInstall` 后 `relaunch()`（[updater-store.ts:124-146](../src/stores/updater-store.ts#L124)），后端没有 `RunEvent::ExitRequested` 或会话结束的处理。

**影响：** 更新时活动下载被直接终止：检查点语义保证不会损坏文件，但会丢失最近一秒左右的进度；正在执行的 ffmpeg 转封装若未随进程退出，可能成为孤儿进程继续写 staging。

**建议：** 安装更新前先执行与"退出"相同的排空（或提示"有 N 个下载进行中，将暂停后更新"）；Windows 上把 ffmpeg 子进程放入随父进程关闭的 Job Object。

### R27-A09｜P3｜若干接口语义不一致，容易在后续修改中引入缺陷

- `transition_task` 把 `message` 同时写入 `health_summary` 与 `error_message`（[state_machine.rs:227-236](../src-tauri/src/state_machine.rs#L227)），暂停任务的 `error_message` 为 "Paused"、启动时为 "Downloading"；其名为 `downloaded_bytes` 的参数实际写入 `speed_bps`。
- `update_settings` 中 `global_speed_limit_bps` 与 `schedule_speed_limit_bps` 的 `None` 表示"清除"（[settings.rs:81-83](../src-tauri/src/commands/settings.rs#L81)、[:189-191](../src-tauri/src/commands/settings.rs#L189)），其余字段 `None` 表示"保持"；前端目前总是发送完整对象，因此尚未触发，但多个入口并发保存时"最后写入者用旧快照覆盖"的问题已存在（状态栏限速与设置页自动保存）。
- 主窗口关闭事件在主线程上 `block_on` 读取数据库（[lib.rs:542-549](../src-tauri/src/lib.rs#L542)），托盘"打开下载目录"同样（[lib.rs:925-929](../src-tauri/src/lib.rs#L925)）；数据库繁忙时界面线程会卡住最长 5 秒 `busy_timeout`。

**建议：** 状态机分离"健康摘要"与"错误信息"两个参数并修正命名；设置接口统一为三态（`Option<Option<T>>`）或拆出 `patch_settings`；关闭事件读取缓存的设置快照。

### R27-A10｜P3｜治理与文档漂移

| 位置 | 问题 |
| --- | --- |
| 代码注释 `ARC-55`（[download/mod.rs:60](../src-tauri/src/download/mod.rs#L60)、[ftp.rs:242](../src-tauri/src/download/ftp.rs#L242)）、`SEC-14`（[secure_headers.rs:76](../src-tauri/src/secure_headers.rs#L76)） | 主审计中不存在这两个条目，"唯一风险登记册"已不完整；`check:docs` 只校验 README/AGENTS，覆盖不到代码注释 |
| 主审计 `SEC-12` | 验收条件含"开启白名单后行为可解释且有提示"，白名单未实现但已 Closed（见 `R27-F01`） |
| [ROADMAP.md:173](ROADMAP.md) | 声称做种"不占用普通下载槽"，与代码不符（`R27-F06`） |
| [ROADMAP.md:323-325](ROADMAP.md) | 验证基线仍写 `cargo check` 与不带 `--all-targets` 的 Clippy，与审计第十四章和 `pnpm verify:rust` 不一致 |
| [README.md:5](../README.md) 与 [:89](../README.md) | 前者称"当前代码存在若干发布阻断问题"，后者称"没有已登记的 P0 发布阻断" |
| [README.md:47-48](../README.md) | 仍写 DASH 签名 CDN 续传退化（`FUN-25` 已关闭）、WebDAV 目录探测绕过客户端缓存且无整体超时（已修） |
| [README.md:101](../README.md) | 仍把覆盖率度量、macOS Rust CI、依赖更新自动化列为未完成（CI 已有 macOS Rust job 与覆盖率产物，`ENG-05` 已加 dependabot） |
| [browser-extension-privacy.md:41](browser-extension-privacy.md) | 称启用内网交接即可访问内网，实际引擎层仍拦截 |

**建议：** 扩展 `check:docs` 扫描 `src`、`src-tauri/src` 注释中的审计 ID 必须存在于审计；为每个 Closed 条目的验收条件增加"未完成项需拆出新 ID"的规则；README 与路线图的能力描述改由协议矩阵等结构化数据生成。

### R27-A11｜P3｜依赖与权限面

- BT 引擎依赖 `librqbit = "9.0.0-rc.0"`（[Cargo.toml:65](../src-tauri/Cargo.toml#L65)），生产依赖使用预发布版本，升级与安全修复节奏不可控。
- `capabilities/default.json` 把全部插件权限授予 `main`、`tray-menu`、`floating-status` 三个窗口（[default.json:5](../src-tauri/capabilities/default.json#L5)）；项目没有为自定义命令定义权限清单，因此删除任务、恢复备份、修改设置等命令对三个窗口同样可调用，而悬浮窗与托盘菜单只需要极少数命令。CSP 严格，风险较低，但最小权限更稳妥。

**建议：** 跟踪 librqbit 正式版并在 `cargo deny` 中对预发布依赖显式豁免与复审；为托盘菜单与悬浮窗建立独立 capability，并用 Tauri 2 的命令权限清单限制可调用的命令。

### 5.1 仍在进行中的既有条目

| 既有 ID | 状态 | 说明 |
| --- | --- | --- |
| `ARC-17`、`ARC-31` | Partial | 超大模块仍在：`SettingsPage.tsx` 2815 行、`TaskDetails.tsx` 2391 行、`NewDownloadDialog.tsx` 2287 行、`dash.rs` 2669 行、`hls/engine.rs` 2634 行、`bt.rs` 2494 行、`metalink.rs` 2475 行 |
| `ENG-03` | Partial | 测试数据库清理仍依赖有限重试（`R26-A10`） |
| `UX-35` | Fixed locally | 分块图仍待真实 Tauri 分段下载验证 |
| 自动化盲区 | 未改变 | 无 GUI E2E；扩展 `background.js`（793 行）无行为测试；真实协议对端与三平台安装包未验收 |

## 6. 程序运行效率

### R27-P01｜P2（待实测）｜Windows 上 `set_len` 预分配叠加远端分段写入，可能触发同步零填充

**证据：代码确认机制，未测量。** 分段下载开始时用 `set_len` 把临时文件扩展到完整大小（[file_ops.rs:288-303](../src-tauri/src/download/file_ops.rs#L288)、[coordinator.rs:187-195](../src-tauri/src/download/http/segmented/coordinator.rs#L187)），随后各段 worker 从各自偏移开始写入。NTFS 上 `SetEndOfFile` 只移动文件末尾而不推进"有效数据长度"（ValidDataLength），首次写入远端偏移时文件系统必须把此前未写区域同步填零；Linux/macOS 的 `ftruncate` 生成稀疏文件，没有这一成本。全仓没有稀疏文件或 `SetFileValidData` 相关处理。

**可能影响：** 以 20 GB 文件、4 段为例，从 15 GB 偏移开始的那一段首次写入前，文件系统可能要先补写约 15 GB 的零，期间该段 worker 阻塞、表现为 0 速度，TCP 接收窗口被塞满；被补零的区域随后又被真实数据覆盖，磁盘写入量最多接近文件大小的 2 倍，对 HDD 速度和 SSD 寿命都不友好。具体表现取决于 NTFS 缓存与写回策略，因此列为待实测。

**建议：** 先用 `scripts/perf` 在 Windows 上对比"预分配 + 4/8 段"与"不预分配"的首写延迟、总耗时和磁盘写入量；若确认，改为把临时文件标记为稀疏（`FSCTL_SET_SPARSE`）后再扩展，或在 Windows 上取消预分配、改用"剩余空间预检"满足原先的防磁盘满目的。

### R27-P02｜P2｜BT：每任务一个 librqbit 会话，且每秒多次写库

**证据：代码确认。**

- 会话键包含 `task_id`（[bt.rs:177-194](../src-tauri/src/download/bt.rs#L177)），每个 BT 任务都有独立的 librqbit `Session`、独立 DHT 节点与持久化文件（[bt.rs:225-253](../src-tauri/src/download/bt.rs#L225)），每个任务冷启动 DHT；代码使用 `SessionOptions::default()`，未显式配置监听端口范围或 UPnP，入站连接能力取决于 librqbit 预发布版的默认值，需要实测。
- 主循环每秒：更新任务进度、同步多文件进度、读首段、写分段进度、`upsert_torrent_runtime_snapshot`（含每秒重新编码的整幅 piece 位图 base64，10 万 piece 约 17 KB）、读做种开关（[bt.rs:784-871](../src-tauri/src/download/bt.rs#L784)）；做种期间每 10 秒再写一次快照。

**影响：** 多个 BT 任务并行时 DHT 流量与内存成倍增长、元数据获取慢；持续的小事务写入放大 WAL，与 HTTP 检查点争用写锁。

**建议：** 回到单会话（或按代理配置少量会话）+ librqbit 的逐 torrent 控制，逐任务限速在会话内按 torrent 实现或以全局上限近似；快照按变化写入，位图改为节流（例如 5 秒）或只在详情页可见时生成；配置监听端口与 UPnP 并在设置中展示。此项与 `R27-F06` 的做种管理器一起设计。

### R27-P03｜P3｜剪贴板轮询每秒读一次数据库并读取完整剪贴板

**证据：代码确认。** 每个 tick 都查询 `clipboard_monitor_enabled`（[clipboard.rs:60](../src-tauri/src/clipboard.rs#L60)），即使功能关闭也每天约 8.6 万次查询；开启时每秒 `read_text` 整段剪贴板（[clipboard.rs:73](../src-tauri/src/clipboard.rs#L73)），Windows 上频繁 `OpenClipboard` 可能与其他程序的剪贴板操作冲突。

**建议：** 设置值随 `settings-changed` 缓存；Windows 先比较 `GetClipboardSequenceNumber`（macOS 比较 `changeCount`）再读取内容，或改用系统剪贴板变更通知。

### R27-P04｜P3｜全局限速 ticker 在空闲时仍以 40 Hz 唤醒

**证据：代码确认。** 限速器的 ticker 在首次限流后启动，只在限速被清除或限速器被释放时退出（[speed.rs:126-156](../src-tauri/src/download/speed.rs#L126)）；全局限速器由 `AppState` 长期持有，因此设置了全局限速后，即使没有任何下载，也每 25ms 唤醒一次。

**建议：** 没有 waiter 超过 1 秒时让 ticker 退出，下次 `throttle` 时惰性重启（`ensure_ticker` 已支持）。对笔记本续航与系统定时器精度有益。

### R27-P05｜P3｜工程侧的小额效率

- CI 在 ubuntu 上先 `cargo llvm-cov` 跑一遍全量测试，再在 `pnpm verify:rust` 中跑第二遍（[ci.yml:96-112](../.github/workflows/ci.yml#L96)），可合并为覆盖率运行即测试门禁。
- 首包同时内联 `en` 与 `zh-CN` 两个完整语言包（[i18n/index.ts:57-60](../src/i18n/index.ts#L57)），可改为只内联当前语言、英文按需加载作回退。

### 6.1 仍开放的既有效率条目

| 编号 | 状态 | 说明 |
| --- | --- | --- |
| `PERF-13` | Open | `aws-lc-rs` 与 `ring` 两套密码学后端并存 |
| `PERF-16` | Open | 创建时读取全部已预留路径；旧全量列表接口与浏览器全快照无上限 |
| `PERF-09` | Needs benchmark | release `opt-level = "s"` 的吞吐取舍 |
| `R26-P03` | 仍开放 | 进度批次复制容器；实体缓存上限 500（`PERF-18`）后影响有限，优先级可下调 |
| `R26-P06` | 仍开放 | 多算法校验重复读文件、无共享校验作业预算 |
| `R26-P08`～`P10` | 仍开放 | release 实机启动/内存/长跑数据与交互性能门禁仍缺 |

## 7. 功能层面可以添加或改进的方向

2026-09-21 的 [功能增强建议](feature-enhancement-proposals-2026-09-21.md) 提出的 `FE-01`～`FE-15` 目前**均未实现**（代码中没有任务预设、保存视图、批量编辑、下载集合等模型或迁移）。下表把它们与本轮新提议合并排序，排序依据是"用户价值 / 与现有可靠性缺口的关系 / 实现成本"。标注"新"的是本轮新增方向。

| 优先 | 方向 | 内容 | 与现有条目关系 | 成本 |
| --- | --- | --- | --- | --- |
| 1 | 内网信任模型（新） | 按来源分级的私网策略、主机/网段白名单、FTPS/HTTPS 证书首次信任 | 解决 `R27-F01`、`R27-F08` | 中 |
| 2 | 自动重试与网络感知（新，扩展 `FE-09`） | 任务级退避重试、网络变化唤醒、可配置重试与超时、停滞检测 | 解决 `R27-F02`、`R27-U03` | 中高 |
| 3 | 实时调速与策略引擎（新） | 运行中改单任务限速/优先级、定时限速实时生效、按主机限速、托盘快捷限速档 | 解决 `R27-F03`、`R27-U06` | 中 |
| 4 | 站点请求配置（新） | 按域名的 UA/Referer/Cookie/请求头/连接数/代理，新建时可一次性覆盖 | 解决 `R27-F05`，扩展现有站点规则 | 中 |
| 5 | 智能剪贴板（新） | 类型白名单、非模态提示、忽略自身复制、不自动探测 | 解决 `R27-U01` | 低 |
| 6 | 系统集成（新） | 任务栏进度、托盘状态与角标、失败通知、`.torrent`/`.metalink`/`magnet:` 文件与协议关联 | 解决 `R27-U04` | 中 |
| 7 | 任务预设 + 保存视图 + 批量编辑 | 见 `FE-01`、`FE-04`、`FE-03` | 高频效率 | 中 |
| 8 | 远端目录下载（扩展 `R26-F08`） | FTP/SFTP/WebDAV/HTTP 目录索引的多选与递归计划，保留相对结构 | 依赖 `R27-F01` 后才对 NAS 有意义 | 中高 |
| 9 | BT 做种管理（新） | 单会话、做种不占下载槽、完成后可做种、重启恢复、全局默认做种策略、监听端口/UPnP | 解决 `R27-F06`、`R27-P02` | 高 |
| 10 | 媒体下载完善（新） | 字幕旁挂/`mov_text`、DASH 轨道选择、直播缺口容忍、无 ffmpeg 降级与获取引导 | 解决 `R27-F07` | 中高 |
| 11 | 完成后处理与文件管理 | 按任务的完成钩子（`{path}`/`{name}` 占位符）、自动解压、调用杀毒扫描、归档移动；见 `FE-06`、`FE-12` | 扩展完成动作 | 高 |
| 12 | 批量链接生成与页面链接抓取 | `FE-10` 的编号区间展开；扩展端在用户点击时（`activeTab`）列出页面链接供勾选 | 不扩大常驻权限 | 中 |
| 13 | 更换失效链接 / 多源下载（扩展 `FE-02`） | 同一文件配置多个 URL（镜像）并行取段，链接过期时换源续传 | 复用 Metalink 镜像与 validator 校验 | 高 |
| 14 | 一键诊断包（新） | 导出脱敏的日志、设置、环境检查、最近事件为 zip，便于反馈问题 | 复用环境诊断与完整性护照的脱敏协议 | 低 |
| 15 | 本地自动化接口 | `FE-14` 的本地 CLI；可兼容 aria2 输入文件格式的任务清单导入导出 | 边界 `FUN-19` | 高 |
| 16 | 磁盘空间预算、定期资源检查、分片级修复 | `FE-05`、`FE-13`、`FE-15` | 中长期 | 高 |

落地原则保持与既有建议一致：新能力不得绕过调度器、逐任务锁、统一网络工厂与凭据源绑定；浏览器发布包保持最小权限；新增文案覆盖 7 个 locale 且不宣称母语校对完成。

## 8. 建议实施顺序

| 批次 | 内容 | 预期改动规模 | 退出条件 |
| --- | --- | --- | --- |
| 第 1 批：止血 | `R27-A01` 重置重试预算；`R27-F10` BT 限速取最小值；`R27-U07` 默认值由后端提供；`R27-U02` 修正等待网络文案；`R27-U01` 忽略自身复制 + 不自动探测 + 非模态提示；`R27-A02` 统一排空 + 条件检查点；`R27-A07` 完成动作去重 | 小，多数在单个函数内 | 对应回归测试通过；手动"重试"确实会重试；暂停不再出现僵尸 |
| 第 2 批：网络与信任 | `R27-F01` 信任分级与白名单；`R27-F02` 任务级自动重试与网络感知；`R27-U03` 停滞时速度衰减；`R27-F08` 证书信任流程 | 中，涉及 `ssrf`、调度器与设置模型 | NAS/内网/Tailscale 可下载；断网 2 分钟后自动完成；安全回归（重定向、清单子资源、元数据地址）保持拒绝 |
| 第 3 批：运行时策略 | `R27-F03`、`R27-U06`、`R27-A05` 合并为"计划与限速策略"组件；`R27-A03` 调度队头阻塞 | 中 | 跨边界吞吐切换 ≤2 秒；多主机混排不空转 |
| 第 4 批：引擎完整性 | `R27-F04` 未知大小续传；`R27-A04` BT 错误转失败；`R27-F06` + `R27-P02` BT 做种管理与会话拓扑；`R27-F07` 媒体完善 | 中高 | 各引擎合同测试扩展到新场景 |
| 第 5 批：可见性与体验 | `R27-U04` 通知/任务栏/托盘；`R27-U05` 关闭确认与启动恢复；`R27-U08`～`U12` | 中 | 后台运行时失败可被发现；读屏可识别新建窗口所有控件 |
| 第 6 批：效率证据 | `R27-P01` Windows 零填充基准与修复；`R27-P03`/`P04`；`PERF-13`/`PERF-16`；`R26-P08` 实机数据 | 视测量结果 | 有前后对照数据才调整 |
| 持续：治理 | `R27-A10` 审计 ID 登记与文档门禁扩展；`R27-A11` 依赖与权限收敛；`ARC-17`/`ARC-31` 模块拆分 | 小到中 | `check:docs` 能发现代码引用的未登记 ID |

功能扩展（第 7 节）建议在第 1、2 批完成后进入，其中"智能剪贴板""实时调速""站点请求配置"与可靠性修复高度重叠，可以作为同一工作包交付。

## 9. 统一验收要点

| 合同 | 必测场景 | 关键断言 |
| --- | --- | --- |
| 重试与恢复 | 单段连续失败后手动重试；断网 2 分钟；429 带 `Retry-After`（秒与 HTTP-date）；应用重启后继续 | 重试预算每次运行重新计算；无人值守完成；不可重试错误不自动重试 |
| 控制面收敛 | 刷盘延迟 8 秒时暂停/取消/删除/重试；暂停后立即继续 | 状态停在用户选择的终态；同一任务任意时刻至多一个写者；临时文件可删除 |
| 信任边界 | 手动输入私网 URL；浏览器交接私网 URL（开关关/开）；公网→私网重定向；公网清单引用私网子资源；元数据地址 | 用户确认的私网目标可下载；自动来源与跨源跳转被拒并给出可操作提示 |
| 运行时策略 | 任务运行中跨越限速窗口边界；运行中修改单任务限速；修改计划时段；BT 任务无单独限速但有全局限速 | 2 秒内生效；与全局限速取最小值；BT/HTTP/FTP/SFTP/HLS 一致 |
| 剪贴板 | 复制普通网页、下载链接、应用内复制 URL、窗口隐藏 | 无模态打扰；无未经确认的网络请求 |
| 媒体 | HLS WebVTT 字幕、直播缺片、无 ffmpeg；DASH 多语言多码率 | 成品可播放且轨道正确；缺口被记录；降级输出可用 |
| 效率 | Windows 预分配 + 4/8 段大文件；BT 多任务；空闲 1 小时 | 首写延迟与磁盘写入量有对照数据；空闲时无高频唤醒与查询 |

前端改动至少执行 `pnpm typecheck`、`pnpm test:frontend`，界面改动加 `pnpm build`；Rust 命令或模型变化执行 `pnpm specta` 与 `pnpm check:bindings`；下载、恢复、取消相关修改在 `src-tauri/tests` 增加真实 fixture 或故障注入测试；新增文案更新 7 个 locale 并运行 `pnpm check:i18n`。

## 附录

### 附录 A：2026-09-21 审查条目的跟踪状态

| R26 编号 | 当前状态 |
| --- | --- |
| `R26-U01`～`U05` | 已由 `UX-26`～`UX-30` 关闭（`UX-31` 同批修复了常驻对话框占用模态焦点的回归） |
| `R26-U06` | 仍开放：全选仅限已加载 |
| `R26-U07` | 仍开放：新建窗口缺规则命中解释（路线图延期） |
| `R26-U08` | 仍开放：无读屏/缩放/高 DPI 实机验收；本轮另发现新建窗口三个无名下拉框（`R27-U08`） |
| `R26-U09` | 部分完成：帮助面板已补齐，仍为两份定义 |
| `R26-F01`、`F03`、`F06` | 已由 `FUN-30`、`FUN-32`、`FUN-31` 关闭 |
| `R26-F02` | 后端已修，批量 UI 入口仍缺 |
| `R26-F04` | 代码已有目录探测总预算（注释标为 `ARC-55`），但主审计无对应条目；关闭对话框仍不取消后端请求（[tasks.rs:248-251](../src-tauri/src/commands/tasks.rs#L248)） |
| `R26-F05`、`F07`～`F10`、`F12` | 仍开放（见 [4.1](#41-2026-09-21-审查功能项的延续状态)） |
| `R26-F11` | 产品边界 |
| `R26-A01`、`A02`、`A04`～`A08` | 已由 `ARC-49`、`SEC-13`、`ARC-50`、`ARC-53`、`ARC-54`、`ARC-51`、`ARC-52` 关闭 |
| `R26-A03` | 代码已修（先按 v1 解密、失败后按旧格式重试，注释标为 `SEC-14`），主审计无对应条目 |
| `R26-A09`、`A10` | 对应 `ARC-17`/`ARC-31`、`ENG-03`，仍为 Partial |
| `R26-A11` | 仍开放，并在 `R27-A10` 中补充了新的漂移点 |
| `R26-P01`、`P02`、`P04` | 已由 `PERF-17`、`PERF-18`、`PERF-19` 关闭 |
| `R26-P05`、`P07` | 对应 `PERF-16`、`PERF-13`，仍 Open |
| `R26-P03`、`P06`、`P08`～`P10` | 仍开放 |

### 附录 B：本轮门禁与走查记录

| 项目 | 结果 |
| --- | --- |
| `pnpm typecheck` | 通过 |
| `pnpm lint` | 通过（263 个文件） |
| `pnpm check:i18n` | 通过 |
| `pnpm check:docs` | 通过 |
| `pnpm test:frontend` | 71 个测试文件、397 项全部通过 |
| `cargo test --locked -j 4` | 通过：56 个测试二进制，769 项通过、0 失败、2 项忽略（`perf_baseline_10k`/`perf_baseline_50k`，按设计手动运行）；编译 4 分 47 秒，测试累计约 115 秒 |

浏览器预览走查（1280×800，简体中文，mock 数据）记录：

- 首次引导三步（创建第一次下载 / 浏览器交接 / 键盘），未涉及保存目录、ffmpeg 与局域网限制。
- 主列表：状态徽标、可续传标记、速度/进度/剩余时间/连接数、失败行内恢复横幅（含"重新开始会丢弃 84.9 MB"量化代价）、状态栏健康徽标均正常呈现；mock 中"等待网络"行显示"网络不可用，恢复后将自动继续"（见 `R27-U02`）。
- 新建下载：探测后显示协议、"暂停后可以继续"、文件名、大小与可用磁盘空间；"更多下载选项"含认证、校验算法、优先级、子目录、单任务限速、代理，无"仅加入队列"。
- 设置：11 个分区与自动保存；网络分区说明"系统代理仅作用于 HTTP/HTTPS"。
- 快捷键面板与右键菜单：快捷键列表完整；已完成任务右键菜单无重新下载/校验。
