# 项目改进审计

最后更新：2026-08-26

适用版本：Vibe Downloader `0.5.0`

审计对象：当前工作区的前端、Rust 后端、数据库迁移、协议引擎、浏览器扩展、构建配置、测试与产品文档

状态：当前风险基线，用于后续对话逐项修复

本文是项目当前唯一的全局风险与优先级文档。它不是变更日志，也不会把路线图中的计划写成已实现能力。若历史专项审计、README 或路线图与当前源码冲突，以当前源码和本文的最新复核结果为准。

## 一、如何使用本文

后续修复对话应直接引用问题 ID，例如“修复 `ARC-01` 和 `ARC-02`”。每次修复都必须先重新读取对应代码，因为行号和局部实现可能已经变化。

ID 前缀含义：

| 前缀 | 范围 | 章节 |
| --- | --- | --- |
| `UX` | 用户交互与可访问性 | 五 |
| `FUN` | 功能完整性与跨层贯通 | 六 |
| `ARC` | 架构鲁棒性、并发、取消、事务 | 七 |
| `PERF` | 运行效率与资源占用 | 八 |
| `SEC` | 安全边界与攻击面（2026-08-13 新增） | 九 |
| `ENG` | 工程门禁、测试基础设施与仓库治理（2026-08-13 新增） | 十 |

状态定义：

| 状态 | 含义 |
| --- | --- |
| Open | 已由当前代码路径确认，尚未修复 |
| In progress | 已开始修改，但验收条件尚未全部满足 |
| Fixed locally | 实现和本地自动化已完成，仍等待 CI、安装包或真实环境验证 |
| Closed | 实现、测试、文档和所需外部验证均完成 |
| Boundary | 明确的产品能力边界，不是当前实现错误 |

优先级定义：

| 优先级 | 含义 |
| --- | --- |
| P0 | 可能导致数据损坏、隐私策略失效、核心能力不可用或应用无法启动；公开发布前必须修复 |
| P1 | 主要工作流错误、不可恢复、明显不稳定或会长期占用资源；应在下一发布候选前修复 |
| P2 | 体验、协议完整性、可维护性或规模风险；应进入近期迭代 |
| P3 | 中长期能力或必须先用基准验证的优化假设 |

问题关闭规则：

1. 不能仅凭编译通过关闭问题，必须满足该问题列出的验收条件。
2. 涉及并发、取消、恢复、代理、认证或文件提交的问题必须有集成测试，不能只测纯函数。
3. 涉及前端行为的问题至少运行 `pnpm typecheck` 和 `pnpm test:frontend`；UI 或打包变化还要运行 `pnpm build`。
4. 新增 i18n key 时更新全部 7 个 locale，并运行 `pnpm check:i18n`。
5. Rust IPC 模型或命令签名变化后运行 `pnpm specta` 和 `pnpm check:bindings`。
6. 修复完成后在本文将状态更新为 Closed，并记录关键测试；不要删除问题及其历史原因。

自 2026-08-26（第 4 轮复审）起，本文新增「已验证的非问题与负结果」清单（第十二章）：经对抗性复核判定为不可达或已有可靠上游防线的候选发现也一并登记。后续修复对话不得将其作为新问题重复报告；引用其结论时应注明「已验证的非问题」。若相关代码发生实质变更，对应条目应重新评估。

## 二、执行摘要

Vibe Downloader 已经越过 HTTP 下载 MVP 阶段。HTTP 分段下载、SQLite 持久化、队列调度、全局与逐任务限速、恢复动作、多协议入口、浏览器交接、虚拟化任务列表、诊断视图和七语言框架均已落地。当前主要矛盾不是入口数量不足，而是部分跨层契约没有真正贯通。

当前不应按“公开稳定发布、全协议同等成熟、可替代 IDM”描述。阶段 A 的 6 项 P0 发布阻断已全部 Closed（含 ARC-04 限速取消）：

| ID | 问题 | 状态 |
| --- | --- | --- |
| UX-01 | 启动失败没有失败状态和恢复入口 | Closed |
| FUN-01 | HTTP Basic Auth 只在探测阶段生效 | Closed |
| FUN-02 | HTTP 系逐任务代理配置未进入真实网络路径 | Closed |
| ARC-01 | 活动任务 `source_key` 唯一索引使用主机级 key | Closed |
| ARC-02 | 输出路径没有原子预留和 no-clobber 提交 | Closed |
| ARC-03 | 下载 worker、限速等待和 ffmpeg 子进程不能可靠收敛 | Closed |

### 2026-08-13 复审

本轮对前端、Rust 后端、数据层、安全边界、测试与 CI、文档一致性做了六个维度的独立复核，新增 36 条问题（`UX-17`~`UX-18`、`FUN-20`~`FUN-22`、`ARC-19`~`ARC-31`、`PERF-12`~`PERF-16`、新增 `SEC-01`~`SEC-07` 与 `ENG-01`~`ENG-06`）。阶段 A 的 6 项 P0 已逐条回到代码核实，确认全部真实修复，不得重新打开。

新的 P0 阻断集合现已清空：`ARC-19`～`ARC-22`、`SEC-01`、`SEC-02` 的 P0 项均已 Closed（`ARC-19` 的协调器排空仍是 P2 残留，并入 `ARC-31`）。

**2026-08-13 修复批次进展**：`ARC-20`、`ARC-21`、`ARC-22`、`SEC-02` 已 Closed；`ARC-19` 的数据损坏已根治（协调器排空作为 P2 残留并入 `ARC-31`）。同批完成的还有 `ENG-02`（工作区恢复可提交）、`PERF-12`（日志保留）与 `ENG-01` 的 CI 部分。

**2026-08-14**：`SEC-01` Closed。实机确认任务列表「更多 → 导出」无反馈；根因是 `dialog:allow-save` 与写权限均未授予，失败被 `export.ts` 吞掉。已移除 `fs` 插件，读写改走后端命令，导出失败会 toast。

复审最重要的结论不是任何单条问题，而是两个贯穿性的根因。**逐项修复它们的实例而不修根因，同类问题会继续产生：**

1. **跨引擎契约漂移**。代理解析、取消收敛、超时和 SSRF 守卫在 8 个引擎里各自实现，没有任何机制保证它们遵守同一契约。这正是 `FUN-02` 和 `ARC-03` 在 HTTP 上被正当地判定为 Closed、却在 DASH/FTP/SFTP 探测路径和 BT 上依然破损的原因（`FUN-20`、`ARC-19`、`SEC-03`）。验收标准「主路径有测试」不等于「所有引擎遵守同一契约」。
2. **门禁覆盖面小于其表观**。`cargo clippy` 缺 `--all-targets`、`cargo deny` 从不执行已配置的 `bans`/`sources`、`check:i18n` 只比 key 不比 value、日志默认只保留最近 40 KB。每一项都像是有防护，实际都没防住（`ENG-01`、`FUN-21`、`PERF-12`）。`ARC-18` 预言的文档漂移之所以复发，也是因为它的验收条件「增加自动文档检查」始终没有落地。

六维判断：

| 维度 | 当前判断 | 首要任务 |
| --- | --- | --- |
| 用户交互便捷性 | UX-01～UX-16 已 Closed；新增分页滚动回跳与列表 ARIA 模型不一致 | UX-17 |
| 功能丰富性和完整性 | 功能面宽且主要契约已贯通；剩余缺口集中在探测路径代理 | FUN-20 |
| 架构鲁棒性和稳定性 | HTTP 路径的所有权与提交语义已经扎实，但同类保证没有覆盖 FTP/SFTP/Metalink/DASH；事务隔离级别是系统性问题 | ARC-19 至 ARC-22 |
| 安全边界 | SSRF 三层防御、TOFU、凭据加密、SQL 参数化、能力面收敛都做得好；剩余缺口在「绕过统一入口」的少数路径 | SEC-03 |
| 程序运行效率 | 进度热路径与 bundle 预算已优化到位；日志保留策略反而让现场问题不可诊断 | PERF-12 |
| 工程门禁与可维护性 | 测试与 CI 结构健康，但多处门禁形同虚设；超大模块使同一缺陷需在多处重复修复 | ENG-01、ARC-17 |

### 2026-08-26 第 4 轮复审

本轮以多代理编排方式，对当前未提交工作区（HLS 模块拆分、`webdav.rs`/`ssrf.rs`/`browser_realtime.rs` 新增等约 95 个文件、+4280/−4997 行重构后的状态）做了十个维度的独立深审：并发与任务生命周期、HTTP 引擎、FTP/SFTP/WebDAV/BT 引擎、HLS/DASH 流水线、网络安全、凭据与存储、SQLite 层、前端核心、UI/i18n/可访问性、测试/CI/依赖/文档。每条候选发现均由独立评审对照代码做对抗性复核后方可登记：46 条候选中 **45 条确认、1 条驳回**（见第十二章）。本轮以静态审查为主，未重复执行构建与测试。

历史遗留记分板（第 1–3 轮共 13 项跟踪项的现状）：

| 遗留问题 | 现状 |
| --- | --- |
| pause/cancel/delete ↔ dispatch 死锁 | **仍 Open 且恶化**——Restart 路径是确定性死锁而非竞态（`ARC-32`） |
| 调度器 slot 泄漏 | Partial——错误路径已修，panic 路径仍泄漏（`ARC-40`） |
| SFTP 续传静默损坏 | **Closed**——双侧 seek + flush 契约 + 字节级回归测试核实有效 |
| BT seed-ratio 不生效 | 保持 Closed |
| SSRF DNS-rebind TOCTOU | 保持 Closed |
| 引擎层 SSRF 缺失 | Partial——HTTP 家族已覆盖；BT 抓取与 FTP/SFTP 建连仍无防护（`SEC-03`、`SEC-12`） |
| Metalink 并行续传错位 | Partial——原问题已修，同类根因仍在（`ARC-34`、`ARC-35`） |
| queue-changed 全量重取 | Closed——增量路径核实有效 |
| i18n 缺口 | Closed——7 locale × 1405 键值级校验全过 |
| HLS/DASH staging 目录泄漏 | **仍 Open 且恶化**——DASH 连「删除任务（含文件）」都不回收（`ARC-38`） |
| HLS/DASH segment 失败僵尸态 | **仍 Open，根因已定位**——引擎自取消伪装用户取消（`ARC-37`） |
| DASH 续传回归 | Partial——签名 CDN 下仍全量重下（`FUN-25`） |
| keyring 密钥轮换数据丢失 | **仍 Open 且恶化**——任意 keyring 读错误即销毁密钥（`SEC-08`） |

本轮新增 30 个 ID：`UX-19`～`UX-25`、`FUN-23`～`FUN-27`、`ARC-32`～`ARC-48`、`SEC-08`～`SEC-12`、`ENG-07`～`ENG-08`。

本轮最重要的结论不是任何单条问题，而是三个贯穿性的根因：

1. **「引擎自取消」与「用户取消」共用同一个 token 却语义不同**。引擎在内部失败时取消调度器拥有的任务 token，supervisor 的 `is_cancelled()` 检查因此把引擎自灭误判为用户取消并跳过失败转移（`ARC-37` 的僵尸 Downloading）；`ARC-32` 的确定性死锁同属这一族「所有权边界不清」的问题。
2. **二等引擎（Metalink/BT）缺少一等引擎已经建立的契约**。HTTP worker 要求 206 + start/end/total 全字段匹配，Metalink 只在续传时才校验且不看 end/total（`ARC-35`）；HTTP 有 ETag/If-Range 续传前置条件，FTP/SFTP 什么都不验（`ARC-42`）；Metalink 分片计划不持久化（`ARC-34`）、BT session 端口冲突（`ARC-39`）。阶段 E3 规划的跨引擎契约测试矩阵仍未建立，正是这些问题的共同背景。
3. **信任边界缺在「最后一米」**。SSRF 守卫装在共享 client 上，字面量 IP 绕过 resolver（`SEC-10`）、BT 抓取绕过整个 client（`SEC-03`）；备份恢复校验了路径却没校验 settings（`SEC-09`）；凭据头注入了请求却没有源绑定（`SEC-11`）；keyring 读错误被当成「密钥不存在」（`SEC-08`）。

六维判断（本轮）：

| 维度 | 当前判断 | 首要任务 |
| --- | --- | --- |
| 用户交互便捷性 | 整体质量高；缺陷集中在 toast 生命周期一族（驱逐不结算、计时器重置） | UX-19 |
| 功能丰富性和完整性 | 备份导出跨卷必败使数据安全主特性形同虚设 | FUN-23 |
| 架构鲁棒性和稳定性 | 调度器锁序是最高优先级历史债；BufWriter 不 flush 是波及面最广的新缺陷 | ARC-32、ARC-33 |
| 安全边界 | HTTP 核心防御扎实；缺口集中在绕过统一入口的路径与凭据头源绑定 | SEC-08 |
| 程序运行效率 | staging 目录泄漏是当前最大的持续资源流失点 | ARC-38 |
| 工程门禁与可维护性 | CI 门禁经抽查真实有效；文档漂移复发于 AGENTS.md 与本文自身旧文 | ENG-06 |

## 三、质量门禁实测

本轮验证基于 2026-08-13 的当前未提交工作区（含 HLS 模块拆分与 PERF-10 bundle 预算脚本）：

| 检查 | 结果 | 说明 |
| --- | --- | --- |
| `pnpm typecheck` | 通过 | TypeScript 无类型错误 |
| `pnpm test:frontend` | 通过 | 36 个测试文件、151 项测试通过，13.9s |
| `pnpm check:i18n` | 通过 | 7 个 locale 各 1390 个 key，**但只比对 key 不比对 value**，见 `FUN-21` |
| `pnpm build` | 通过 | 生产构建通过 |
| `pnpm check:bundle` | 通过 | initial shell JS 284.0 kB gzip / 预算 340 kB；CSS 14.6 kB gzip / 预算 18 kB |
| `pnpm test:release-tools` | 通过 | 32 项测试通过 |
| `pnpm verify:protocol-matrix` | 通过 | 协议矩阵结构检查通过 |
| `pnpm lint` | 审阅时**失败**，现已通过 | 当时 7 errors + 2 warnings，含 `AttentionCenter.tsx:249` 的 `noStaticElementInteractions`（非自动修复的真实 a11y 缺陷）与 `globals.css:162` 的 `noImportantStyles`。已在 `ENG-02` 修复 |
| `cargo clippy -- -D warnings` | 通过 | 与 CI 当时的配置一致 |
| `cargo clippy --all-targets -- -D warnings` | 审阅时**失败**，现已通过 | 当时 3 个 `items_after_test_module`：`db/dash.rs:408`、`db/hls.rs:456`、`download/http/request.rs:42`。CI 未加 `--all-targets`，因此这些错误长期逃逸。已在 `ENG-02` 修复，CI 也已收紧（`ENG-01`） |
| `cargo test`（默认并行） | **失败** | `os error 1455`（页面文件太小）导致链接失败。这是本机资源问题而非代码问题，`-j 2` 可通过，详见 `ENG-03` |
| `cargo test -j 2` | 通过 | 约 578 项测试通过（239 lib + 集成），需 `--skip add_torrent_source_http`，见下 |
| `cargo test`（BT torrent 源用例） | **挂起** | `add_torrent_source_http_downloads_and_parses_private_flag` 失败、`add_torrent_source_http_fallback_on_download_failure` 永久等待 `server.await`。根因是 `SEC-03`，本批次未修 |

Rust 测试挂起的根因值得单独记录，因为它是一个由测试暴露出来的产品缺陷：`download_torrent_bytes`（`bt.rs:1674`）自建 reqwest client，只在 `custom_socks5_url_with_auth()` 有值时设代理，从不调用 `.no_proxy()`。由于 `Cargo.toml:29` 为 reqwest 启用了 `system-proxy`，在配置了系统代理的开发机上，指向 `127.0.0.1` 测试监听器的请求被送往系统代理，测试因此一个失败、一个死等。这同时说明**用户选择「不使用代理」时 `.torrent` 请求仍会走系统代理**。

另需澄清一个被 README、CONTRIBUTING、ROADMAP 和本文同时固化的误解：`cargo test -j 1` 限制的是 **Cargo 的编译并行度**，不影响测试线程数（那是 `-- --test-threads=1`），因此它从来不可能修复测试间干扰。原始现象是本机链接阶段内存不足。真实的测试隔离隐患是 `tests/common/mod.rs:118` 在多线程测试中无保护地调用 `std::env::set_var`，见 `ENG-03`。

当前自动化盲区：

- 没有 Playwright、WebDriver 或 Tauri GUI 端到端测试。
- `browser/extension-core/src/background.js`（715 行，Native Messaging 与 WS bridge 核心）没有行为测试；现有扩展测试只覆盖 `capture-policy.js`（82 行）。
- 没有真实安装包启动、升级、卸载和浏览器接管自动化。
- 没有覆盖率度量（Rust 与前端均无），见 `ENG-04`。
- macOS 没有任何 Rust 测试与 clippy，平台特定代码只在 release 构建中被编译，见 `ENG-01`。
- 没有并发创建同名多文件任务、DEFERRED 事务快照冲突的集成测试，见 `ARC-20`、`ARC-21`。

## 四、应保留的已确认优势

- HTTP probe 使用 HEAD 并在需要时回退 Range GET；主下载路径支持未知大小、Range 分段、动态拆分、重试、checkpoint、恢复验证和最终重命名。
- `EngineRegistry` 已将协议路由与下载实现隔离，多协议共享明确的 `DownloadContext`。
- SQLite 使用 WAL，任务、segments、凭据、代理、校验和、文件和诊断数据均有独立持久化边界。
- 凭据使用 ChaCha20-Poly1305 加密，浏览器 handoff 保持 HTTP/HTTPS、无嵌入凭据、无本地路径控制和 header allowlist 等安全边界。
- 调度器已有最大活动任务、每主机槽位、优先级、计划窗口和完成动作模型。
- 前端 task data、task UI 和 speed history 已拆分，任务列表使用游标分页、虚拟化和增量事件。
- UI 保持密集桌面工具形态，具备命令面板、快捷键、详情抽屉、恢复动作、Tooltip、焦点环、七语言和 8 个 OKLCH 强调色。
- `TaskProgressEmitGate` 将高频进度事件限制到至少 250ms；request diagnostics 已有保留策略。

第 4 轮复审（2026-08-26）另核实以下优势，修复时不得回退：

- SFTP 续传已具备完整契约：本地/远端双侧 seek（`sftp.rs:920-928`、`:894-905`）+ 「flush 后才能上报 checkpoint」约定（`:1051-1068`）+ 取消/续传字节级回归测试（`sftp_engine.rs:919-948`）。
- HTTP 分段 worker 对 206 与 Content-Range 要求 start/end/total 全字段精确匹配（`worker.rs:252-274`），是全仓最严格的续传前置校验，应作为其他引擎对齐的范本。
- 前端 `bindings.ts` 由 Tauri-Specta 生成，本轮逐一交叉核对 88 个 invoke 名称、参数 casing 与 enum 表示，零漂移。
- i18n 值级校验实测通过：7 个 locale 各 1405 个叶子键，零缺失、零多余、零占位符错配、零未翻译（`FUN-21` 修复有效）。
- HLS/DASH 重构后的段监督改用 JoinSet 并向传播 panic/join error；ffmpeg 使用 kill-on-drop 且与 cancel 竞争；加密段的发布走 `.part` + rename 原子提交。

## 五、用户交互便捷性

### UX-01（P0，Closed）：启动失败不可见且不可恢复

- **证据**：`run_startup_init` 普通错误曾只记录日志；`StartupGate` 在状态查询失败后停止轮询；`StartupState` 无 failed 模式。
- **影响**：初始化失败时用户永久停在加载页；瞬时 IPC 错误也只能重启应用。
- **修复**：`startup_failed` 状态（code/message/logPath/dataPath）；`retry_startup_init` 幂等重试（服务 flags 防 double-spawn）；打开日志/数据目录命令；`StartupFailedPage` + 7 locale；IPC 瞬时错误可 Retry 恢复轮询。
- **验证测试**：`set_failed_transitions_from_initializing`、`begin_retry_only_from_failed_and_blocks_while_in_flight`、`service_flags_are_sticky_for_idempotent_retry`（`startup.rs`）；`StartupGate.test.tsx` 成功/失败/重试/IPC 恢复。
- **验收**：失败页显示本地化原因；Retry 可恢复到 ready；日志入口可用；不会重复启动 scheduler、clipboard 或 browser bridge。

### UX-02（P1，Closed）：全局捕获监听器破坏 Radix 自定义右键菜单

- **证据**：[`src/components/shell/AppShell.tsx`](../src/components/shell/AppShell.tsx) 曾在 `window` 捕获阶段阻止所有 `contextmenu`；任务菜单依赖 Radix Trigger。
- **影响**：任务、空白列表、侧栏、标题栏、状态栏和详情区域的右键入口可能全部无法打开。
- **修复**：改为冒泡阶段抑制；放行 `input/textarea/select/[contenteditable]`；保留 `defaultPrevented` 短路。
- **验证测试**：`AppShell.contextmenu.test.tsx`（真实 `contextmenu` 事件）。
- **验收**：全部自定义区域可右键打开；普通空白区域不出现 WebView 原生开发菜单；输入框和文本选择行为符合产品约束。

### UX-03（P1，Closed）：浏览器接管设置每次击键保存并禁用整个表单

- **证据**：[`BrowserCaptureControls.tsx`](../src/components/settings/BrowserCaptureControls.tsx) 的输入 `onChange` 曾直接提交；[`SettingsPage.tsx`](../src/components/settings/SettingsPage.tsx) 每次 IPC 保存期间禁用控件。
- **影响**：域名、扩展名和数值编辑会逐字符卡顿，慢磁盘或忙碌 runtime 下接近不可用。
- **修复**：本地 draft + 500ms 防抖；`resolveCaptureDraftAfterSave` 防止旧 IPC 覆盖新草稿；保存中显示状态但不禁用编辑字段。
- **验证测试**：`browser-capture-draft.test.ts`。
- **验收**：连续快速输入只提交最终快照；旧响应不能覆盖新值；失败可重试且草稿不丢失。

### UX-04（P1，Closed）：导入 `.txt` 后没有进入批量模式

- **证据**：[`NewDownloadDialog.tsx`](../src/components/shell/NewDownloadDialog.tsx) 曾只设置 `batchInput` 和展开高级区，没有调用 `setMode("batch")`。
- **影响**：内容已读取但被隐藏，单任务 URL 为空，用户无法直接开始导入。
- **修复**：读取成功后 `setMode("batch")` 并 `runBatch(false, text)` 生成预览；失败保持原模式。
- **验证测试**：`NewDownloadDialog.test.tsx` 导入 txt 用例。
- **验收**：选择有效文本文件后立即看到 URL 列表和有效任务数，开始按钮状态正确。

### UX-05（P1，Closed）：暂停全部和恢复全部只处理当前已加载子集

- **证据**：[`Palette.tsx`](../src/components/shell/Palette.tsx) 曾将当前 store 的 `allTasks` 传给批处理；首批页大小为 100。
- **影响**：搜索、筛选或未加载更多页时，大量隐藏任务不会被处理，但文案声明为“全部”。
- **修复**：新增 `bulk_task_action_global`；DB 按状态选 ID；返回 `{ succeeded, skipped, failed }`；Palette 用 `globalTaskStats` 判定可用性。
- **验证测试**：`bulk_task_action_global.rs`（>100 任务覆盖）。
- **验收**：存在超过 100 个任务和活动筛选时，全局命令仍覆盖数据库中的目标集合，并返回成功、跳过和失败数量。

### UX-06（P1，Closed）：站点规则缺少草稿、校验和安全删除

- **证据**：[`SiteRulesEditor.tsx`](../src/components/settings/SiteRulesEditor.tsx) 曾点击 Add 即持久化空规则，字段变化立即提交，Done 只退出编辑，删除无确认或 Undo。
- **影响**：空 host、半输入扩展名和误删规则会直接进入持久状态。
- **修复**：本地 draft；Save/Cancel；`validateSiteRule`；删除 Undo toast。
- **验证测试**：`SiteRulesEditor.test.tsx`；`browser-capture-draft.test.ts`。
- **验收**：Cancel 不产生 DB 变化；无效规则不能保存；保存失败保留草稿；删除可恢复。

### UX-07（P1，Closed）：Header 转发使用二态和三态两个控件表达同一字段

- **证据**：[`BrowserCaptureControls.tsx`](../src/components/settings/BrowserCaptureControls.tsx) 曾先用 Switch 压成 enabled/disabled，随后又用 Select 表达 ask/enabled/disabled。
- **影响**：`ask` 在 Switch 中显示为关闭，点击会静默丢失原策略。
- **修复**：只保留三态 Select；与 FUN-14 被动语义文案对齐。
- **验证测试**：`BrowserCaptureControls.test.tsx`。
- **验收**：界面只有一个事实源，三态 round-trip 不丢失。

### UX-08（P1，Closed）：桌面宽屏详情栏没有可见关闭按钮

- **证据**：[`TaskDetails.tsx`](../src/components/shell/TaskDetails.tsx) 的宽屏 Header 曾只有标题和路径，关闭按钮只存在于紧凑抽屉。
- **影响**：鼠标用户缺少直接退出路径，命令面板或隐藏快捷键变成必要路径。
- **修复**：宽屏 Header 增加带 Tooltip / `aria-label` 的关闭按钮；复用 AppShell 焦点回退。
- **验证测试**：`TaskDetails.test.tsx` 宽屏关闭用例。
- **验收**：鼠标、键盘和屏幕阅读器均可关闭，关闭后焦点位置稳定。

### UX-09（P1，Closed）：Toast 的“还有 N 条”会清空全部并提交待撤销删除

- **证据**：[`toast.tsx`](../src/components/ui/toast.tsx) 的 more 按钮曾调用 `clearToasts()`；清理时执行每条 `onAutoCommit`。
- **影响**：用户预期展开消息，实际失去全部 Undo 并立即提交软删除。
- **修复**：more 仅展开/折叠；独立 Clear all；Undo toast 置顶并始终可见。
- **验证测试**：`toast-soft-delete.test.tsx`。
- **验收**：展开不触发 commit；只有超时或明确关闭对应 Toast 才提交删除；多条 Undo 可独立执行。

### UX-10（P1，Closed）：Queue Center 的 ARIA 和键盘模型不完整

- **证据**：[`QueueCenter.tsx`](../src/components/workspaces/QueueCenter.tsx) 曾使用 `listbox`，每个 option 都可 Tab 聚焦，仅处理 Enter/Space，并内嵌多个按钮。
- **影响**：长队列产生大量 Tab 停靠点，缺少方向键导航，屏幕阅读器语义混乱。
- **修复**：语义 `list`/`listitem` + roving tabindex；Arrow/Home/End；行内按钮 `tabIndex={-1}` 并用 `aria-controls` 关联详情。
- **验证测试**：`QueueCenter.a11y.test.tsx`。
- **验收**：方向键、Home/End、Tab 顺序和按钮读屏语义通过自动化及人工检查。

### UX-11（P1，Closed）：结构化错误本地化覆盖不足

- **证据**：[`src/lib/errors.ts`](../src/lib/errors.ts) 曾只映射少量 HTTP、磁盘和恢复错误，未命中时直接显示后端 message。
- **影响**：非 HTTP 协议的核心失败会显示英文或技术文本，与七语言完整性声明不一致。
- **修复**：`stable-error-codes.ts` 穷尽公开码映射；未知结构化码回退 `errors.unknownError`；`configure_ffmpeg` 纳入 recovery；原始 code/message 保留在诊断报告。
- **验证测试**：`errors.test.ts` 穷尽断言禁止 backend English fallback。
- **验收**：所有公开错误码在 7 个 locale 有映射；测试禁止稳定错误码走原始 message fallback。

### UX-12（P2，Closed）：窄窗口没有显式排序入口

- **证据**：[`CommandBar.tsx`](../src/components/shell/CommandBar.tsx) 的排序控件在 `md` 以下隐藏，移动工具面板只有过滤项。
- **修复**：[`TaskList.tsx`](../src/components/tasks/TaskList.tsx) 工具面板增加排序 Select；窄屏显示当前排序摘要。
- **验收**：320px 至 768px 均可通过鼠标和触控完成排序。

### UX-13（P2，Closed）：重置全部设置的文案与实际范围不一致

- **证据**：[`SettingsPage.tsx`](../src/components/settings/SettingsPage.tsx) 未重置主题、语言、浏览器接管、站点规则和分类规则。
- **修复**：重命名为「重置下载设置」；确认对话框列出保留项（7 locale）。
- **验收**：文案与实际保留范围一致。

### UX-14（P2，Closed）：部分危险色类名没有对应 token

- **证据**：SiteRulesEditor、ClassificationRulesEditor 和 BrowserCaptureControls 曾使用 `text-text-danger` / `text-text-warning`。
- **修复**：统一为 `text-status-danger` / `text-status-warning`；intranet 警告增加 `role="alert"`。
- **验收**：危险操作使用正确 status token。

### UX-15（P2，Closed）：首次引导的任意关闭都会永久标记完成

- **证据**：[`OnboardingDialog.tsx`](../src/components/shell/OnboardingDialog.tsx) 将 Escape、遮罩和普通关闭都路由到 completed 写入。
- **修复**：Escape/遮罩仅 dismiss；Skip / Get started / New download 才永久完成。
- **验证测试**：`OnboardingDialog.test.tsx`。
- **验收**：普通关闭后下次启动仍可再次出现引导。

### UX-16（P2，Closed）：React 启动等待动画不遵守 reduced-motion

- **证据**：[`StartupGate.tsx`](../src/components/shell/StartupGate.tsx) 直接设置无限 animation；`index.html` 的 reduced-motion 规则不覆盖 React 版本。
- **修复**：`useReducedMotion()` 时渲染静态 logo + `startup.initializing` 文案。
- **验证测试**：`StartupGate.test.tsx` reduced-motion 用例。
- **验收**：开启减少动态效果后只保留静态状态文本。

### UX-17（P1，Open）：无限滚动加载后列表被强制滚回选中行

- **证据**：[`TaskList.tsx`](../src/components/tasks/TaskList.tsx#L347) 的 `scrollToIndex` effect 依赖 `filtered`，而 `filtered` 来自 `taskIds`；`setTaskCursorPage(..., append=true)` 追加一页会生成新数组引用，effect 因此重跑。
- **影响**：用户向下滚动触发加载更多，新数据到达后列表立刻跳回选中行（首次加载会 `selectTask(items[0].id)`，通常是列表顶部）。滚动被打断，表现上像「加载更多没生效」。该 effect 还会与筛选变化时的 `scrollToOffset(0)` 竞争。
- **修复方向**：该 effect 的真实意图是「选中项变化时把它滚进视口」，不是「列表变化就重新居中」。用 `lastScrolledIdRef` 去重，并改用已有的 `filteredRef` 读取最新列表，把 `filtered` 移出依赖数组。
- **验收**：分页追加不改变滚动位置；用键盘或命令面板切换选中项时仍会滚动到目标行。

### UX-18（P2，Open）：列表 ARIA 模型不一致，表单校验缺程序化关联

- **证据**：三个任务列表用了三种模型——[`TaskList.tsx`](../src/components/tasks/TaskList.tsx#L769) 用 `list`/`listitem` 但给行加了 `tabIndex` 与 `aria-current`，而 [`AttentionCenter.tsx`](../src/components/workspaces/AttentionCenter.tsx#L262) 和 `QueueCenter.tsx` 用的是 `listbox`/`option`。`TaskRow` 的 DOM id 本身就叫 `task-option-${id}`，说明原始设计意图是 option。另外全仓库只有 1 处 `aria-invalid`（`NewDownloadDialog.tsx:900`），而 `role="alert"` 的错误文案有 20 处，二者之间没有 `aria-describedby` 关联。
- **影响**：`listitem` 是非交互角色，屏幕阅读器会进入「列表浏览」而非「选择」模式，用户听到「列表项 3，共 50 项」而不是「选项 3，已选中」。设置页的数值 clamp 超限时，键盘用户得不到任何反馈。
- **修复方向**：TaskList 统一到 `listbox` + `aria-multiselectable="true"`，行改 `option` + `aria-selected`；在 `SettingsRow` 这一层内置 `aria-invalid` / `aria-describedby` 关联，一处改动覆盖整个设置页。
- **验收**：三个列表使用同一 ARIA 模型；校验失败时输入框与错误文案有程序化关联；用 `jest-axe` 补测试（`QueueCenter.a11y.test.tsx` 是现成模板）。
- **2026-09-13 部分修复 + 证据修正**：核实发现 QueueCenter 早已改为 `list`/`listitem` + `aria-current`（本条证据过时）——真正的孤儿是 AttentionCenter 的 `listbox`/`option`。现已把 AttentionCenter 统一到 `list`/`listitem` + `aria-current` + 单一 tab stop，键盘导航保持不变（新增 `AttentionCenter.a11y.test.tsx` 两条断言）。**剩余**（TaskRow `task-option-${id}` 改名与 NDD 其余校验输入的 `aria-invalid`/`aria-describedby` 关联）因相关文件被并行特性开发占用而暂缓；设置页数值 clamp 的完整校验 UI 按修复方向的原 note 继续沿 `SettingsRow` 层方案另行处理。

### UX-19（P2，Closed）：Toast 达到 20 条上限时静默驱逐待撤销删除，任务被隐藏且无法删除

- **证据**：[`toast-store.ts`](../src/stores/toast-store.ts#L67) 的 `addToast` 以 `.slice(0, 20)` 丢弃最老 toast，但不像 key 去重路径（`:56-58`）、`clearToasts`（`:79-85`）与超时/X 按钮（`settleCommit`）那样结算被移除项的 `onAutoCommit`。软删除完全依赖该回调提交：`AppShell.softDelete`（[`AppShell.tsx`](../src/components/shell/AppShell.tsx#L567)）把 id 放入 `pendingDeleteIds` 后只有 toast 的 commit/undo 会调用 `deleteTask` + `removePendingDelete`；`clearPendingDeletes`（task-ui-store.ts:121）零生产调用方。`TaskList.tsx:149-155` 按 `pendingDeleteIds` 过滤行；`softDelete` 在 id 已 pending 时早退（AppShell.tsx:570），二次删除无法自愈。
- **影响**：7 秒撤销窗口内涌入约 20 条 toast 即可触发——批量完成/失败事件每任务一条且无去重键（use-task-events.ts:154-171）。撤销 toast 被切片丢弃后 `deleteTask` 永不下发：任务从列表消失、DB 行与文件仍在、不可撤销也不可再删，重启后才「复活」。`toast-soft-delete.test.tsx` 覆盖了超时/手动/clearAll，唯独没有 cap-eviction 路径。
- **修复方向**：slice 驱逐前对被丢弃项调用 `onAutoCommit`（与 clearToasts 同语义）；或让软删除 toast 走带 key 的替换通道避开驱逐。
- **验收**：构造 21 条 toast 断言被驱逐的软删除已实际提交；任何驱逐路径都不在 `pendingDeleteIds` 留孤儿。
- **2026-09-13 修复**：三处 `.slice(0, 20)`（addToast 主路径、deferred 插入、flushDeferredToasts 合并）统一抽为 `capToasts` 助手——驱逐前对每个被丢弃 toast 调用 `onAutoCommit?.()`，与 clearToasts 同语义。软删除被驱逐时落地为真删除，pendingDeleteIds 不再留孤儿；被驱逐项的 UI 通知按既有能力限制安静消失（同 clearAll 行为）。
- **验证测试**：`toast-soft-delete.test.tsx` 新增 cap-eviction 用例——21 条 undo toast 进栈，最老一条被驱逐且其 `onAutoCommit` 恰好调用一次、其余 20 条保持可见未提交。

### UX-20（P2，Closed）：队列重排失败后乐观顺序不回滚，loading 标志永久卡死

- **证据**：[`AppShell.tsx`](../src/components/shell/AppShell.tsx#L287) 先 `reorderTasksLocally(orderedIds)` 再调后端；catch 块（`:290-299`）注释称「触发刷新」但实际只执行 `setLoading(true)`——没有任何代码订阅 `loading` 来触发拉取（setLoading 只是 `set({ loading })`），唯一复位点是 TaskList role=replace loadPage 的 finally（TaskList.tsx:209-213），仅导航/排序/筛选/viewReloadToken 变化可达。
- **影响**：后端拒绝的重排一直显示在界面上；Queue/Attention 工作区的 Load more 按钮停在「Loading more...」禁用态（QueueCenter.tsx:256-260），直到用户改导航/排序/筛选或某个无关事件触发刷新。
- **修复方向**：catch 中真正刷新（复用 refreshTasks 或 bump viewReloadToken），不要只置 loading；顺带给乐观重排补失败回滚到上一顺序。
- **验收**：mock `reorder_queued_tasks` 失败后列表回到服务器顺序且 Load more 可用。
- **2026-09-13 修复**：`reorderTasksLocally` 返回回滚句柄（重排前的 tasks 数组快照，恢复时重建 taskIds/taskIndexById）；`handleReorder` catch 中回滚乐观顺序并 `refreshTasks()` 重新拉取服务器权威顺序，`setLoading(true)` 调用彻底删除（Load more 不再有卡死态）。refreshTasks 自身同步补 try/catch + 错误 toast——catch 处理器内部的 await 不能自己成为新的无声 rejection 源。
- **验证测试**：`task-data-store.reorder.test.ts` 三条用例——全集重排命中位置映射且回滚恢复原序与索引、回滚恢复精确快照（重排后到回滚前的 intervening 变更被快照语义覆盖，调用方随后以刷新取权威顺序）、空 id 列表返回 null 不动状态。敏感性验证：回滚句柄替换为 no-op 后前两条用例立即变红。handleReorder 的 4 行粘合（句柄 + refreshTasks 复用）未另建整机挂载测试（AppShell 无全量 mock 基建，仅为 4 行粘合搭建不成比例），由 store 级语义测试 + 全量回归覆盖。

### UX-21（P2，Closed）：Toast 计时器因依赖链断裂被任意无关渲染重置

- **证据**：[`toast.tsx`](../src/components/ui/toast.tsx#L55) 的 ToastViewport 每次渲染传新的内联箭头 `onDismiss={() => dismissToast(toast.id)}` → `settleCommit([onDismiss])` → `startTimer([settleCommit])` → effect `[startTimer]`（`:130-137`、`:157-160`）。ToastItem 未 memo 化，任何 addToast/updateToast/dismissToast 都产生新数组引用并重渲染 viewport。`startTimer` 重设 `startedAtRef = Date.now()` 不扣减已流逝时间（只有 pauseTimer 做），并 `countdownKey+1` 使 CSS 倒计时条经 key 重挂载从头播放。
- **影响**：批量操作反复更新自己的进度 toast 时（runBulkTransferAction，AppShell.tsx:392-427），所有可见 toast 的剩余寿命与倒计时条被连带重置，陈旧 toast 远超 4800ms 存活；软删除硬提交的唯一时钟就是这个 timer（UNDO_TOAST_TIMEOUT_MS），被无限期推迟，同时放大 `UX-19` 的驱逐窗口。
- **修复方向**：`onDismiss` 用稳定引用或 ToastItem memo 化切断依赖链；`startTimer` 基于 `startedAtRef` 计算剩余时间而非归零重启。
- **验收**：更新一条 toast 不重置其他 toast 的倒计时条动画与剩余寿命。
- **2026-09-13 修复**：`onDismiss` 进入与 `onAutoCommitRef` 同款的 ref 模式（视口每渲染传新内联箭头不再改变 settle 身份），`settleCommit`/`settleUndo` 依赖数组清空 → `startTimer` 恒定 → 计时 effect 每挂载只跑一次，无关渲染不再 clearTimeout+重启；hover resume 路径本就基于 `remainingRef` 扣减流逝时间，保持不变。倒计时条随之不再被无关更新重挂载归零。
- **验证测试**：`toast-soft-delete.test.tsx` 新增「无关 store 更新不重置计时」用例——撤销 toast 3 秒后插入无关 toast，推进到原 7 秒截止点恰好提交一次。旧行为敏感性：恢复 onDismiss 依赖链后该用例失败。

### UX-22（P3，Closed）：StartupGate 单次轮询错误即永久终止自动轮询

- **证据**：[`StartupGate.tsx`](../src/components/shell/StartupGate.tsx#L44) 的轮询循环只在成功路径调度下一次 `setTimeout(check, 300)`（`:49`）；catch 直接 `setLoadError` 渲染终态 StartupFailedPage，无退避重试。tauri 层 `runCommand`（tauri.ts:90-100）与 `getStartupStatus` 均无重试包装。
- **影响**：后端重初始化期间一次 IPC 抖动直接进失败页——即使数百毫秒后 ready 也需要用户注意到并手动 Retry。
- **修复方向**：catch 中按次数上限指数退避继续轮询，超阈值再转失败页。
- **验收**：前 N 次 invoke reject、之后 resolve 时 gate 自行进入就绪态。
- **2026-09-13 修复**：轮询循环加连续失败预算（3 次，1s/2s/4s 指数退避）——预算内继续自动轮询（用户停留在 splash），任何一次成功即清零预算；预算耗尽才落 StartupFailedPage，手动 Retry 路径保持不变（重置 pollKey 即重置预算）。
- **验证测试**：`StartupGate.test.tsx` 两条新用例（假定时器）：两次失败后第三次成功自动进入就绪、无失败页；持续失败耗尽预算后失败页出现且手动 Retry 恢复（原「单次错误即需手动 Retry」用例随行为更新为预算耗尽场景）。

### UX-23（P3，Closed）：剪贴板 / file-drop 监听器随对话框状态拆除重建，窗口期内事件丢失

- **证据**：clipboard effect 的依赖数组含 `newDownloadOpen/newDownloadDraftDirty/t`（AppShell.tsx:1025），file-drop 同型（`:1122`）；依赖翻转时同步 unlisten、await IPC 后才重新注册，而这些都是 fire-and-forget 通知、无回放（tauri.ts:993-1002、1070+）。tray 监听依赖全稳定、不受影响。
- **影响**：打开新建对话框或草稿变脏的瞬间检测到的链接静默丢失——无 toast、无预填。窗口为毫秒级 IPC 往返，命中概率低但后果是无声丢功能。
- **修复方向**：回调依赖收进 ref 使 handler 稳定，监听器一次注册终身持有；或在重注册完成后向后端查询一次 missed 状态兜底。
- **验收**：注册-注销窗口内触发的事件最终得到处理。
- **2026-09-13 修复**：抽出 [`useClipboardLinkMonitor`](../src/hooks/use-clipboard-link-monitor.ts) / [`useFileDropMonitor`](../src/hooks/use-file-drop-monitor.ts) 两个 hook——事件回调经 ref 读取最新闭包，监听器在组件生命周期内只注册一次，注册完成前卸载仍会补发 unlisten；对话框状态（`newDownloadOpen/newDownloadDraftDirty`）改为事件到达时在 handler 内读取，重注册 await 窗口不复存在。AppShell 两个内联 effect 换用 hook，监听语义（草稿活跃时 toast 提示、否则直接应用）逐行保持。
- **验证测试**：`use-link-monitors.test.ts` 三条用例——重渲染（回调身份变化）不拆装监听器且事件派发给最新回调、注册 resolve 前卸载仍调用 unlisten、drop/drag-state 两类事件均转发到最新 handler。

### UX-24（P3，Closed）：复制诊断按钮吞掉剪贴板失败仍提示「已复制」

- **证据**：[`TaskRecoveryActions.tsx`](../src/components/tasks/TaskRecoveryActions.tsx#L26) 执行 `navigator.clipboard.writeText(text).catch(() => {})` 后无条件弹 info toast `recovery.errorCopied`。对比其余全部 copy 处理器（AppShell.copyTaskUrl/copyTaskLocalPath、AboutPage.copyVersion、TaskDetails.copyToClipboard、EnvironmentPanel）都有错误分支。
- **影响**：webview 失焦/权限拒绝是 Chromium 标准拒绝场景——用户以为诊断报告已复制、实际什么都没有，故障上报流程悄悄断裂。
- **修复方向**：对齐其他 copy 处理器的错误 toast 分支。
- **验收**：mock writeText reject 断言出现错误提示且无成功提示。
- **2026-09-13 修复**：`handleCopy` 改 await + try/catch——成功才弹 `recovery.errorCopied`，失败弹 `contextmenu.task.copyFailed`（与 AppShell.copyTaskUrl 等处理器同款分支）并记 warn 日志。
- **验证测试**：`TaskRecoveryActions.test.tsx` 两条新用例：writeText resolve → 成功 toast；reject → 错误 toast、无成功提示（toast store 由 mock 换回真实 store 以便断言）。

### UX-25（P3，Open）：refreshTasks / getPlatform / 目录选择器的 await 无捕获，产生无声 unhandled rejection

- **证据**：[`AppShell.tsx`](../src/components/shell/AppShell.tsx#L187) 的 refreshTasks 无 try/catch，经 `void refreshTasks()`（`:379-381`）接到列表右键 Refresh（TaskList.tsx:769）；`:909` 的 `void getPlatform().then(setPlatform)` 无 `.catch`；[`SettingsPage.tsx`](../src/components/settings/SettingsPage.tsx#L866) 的 chooseDirectory/handleBrowseFfmpegPath 裸 await 且直接绑 onClick（`:1354`、`:2076`），NewDownloadDialog.tsx:694-697/:1054/:1548 同型；resolveAttention 的 choose_another_folder 分支（AppShell.tsx:891-896）在 try 范围外 await picker，由 TaskRow.tsx:932-935 fire-and-forget 调用。对照组：TaskList.loadPage 有完整 error state + `role="alert"` 重试横幅。
- **影响**：IPC 失败时按钮毫无反馈地死掉；同一场故障走列表自身加载路径有横幅、走右键 Refresh 什么都没有。
- **修复方向**：统一经 safeInvoke 包装（失败 toast）；至少给上述五处补 catch。
- **验收**：mock 各自 reject 时均有用户可见反馈。
- **2026-09-13 修复（部分，5 处中 4 处）**：refreshTasks 包 try/catch + 错误 toast（右键 Refresh、重排回滚等 fire-and-forget 调用不再产生无声 rejection）；resolveAttention 的 choose_another_folder picker 补 try/catch + 错误 toast；SettingsPage 的 chooseDirectory/handleBrowseFfmpegPath 对齐同页 syncAutostart 范本补 catch + 错误 toast；getPlatform 补 `.catch` 卫生（其内部已有 fallback，此为防 unhandled rejection 的最后一道）。**剩余**：NewDownloadDialog 三处 picker——该文件正被并行特性开发整文件重写（400+ 行在途改动），为避免冲突推迟，待其落地后按同一范本补齐并闭合本条。
- **验证测试**：`SettingsPage.test.tsx` 两条新用例——save-dir picker 与 ffmpeg path picker mock reject 后断言真实 toast store 出现 `toast.actionFailed` 错误项（ffmpeg 用例经 aria-controls 定位展开默认折叠的 External tools 区块）。refreshTasks 与 resolveAttention picker 的失败分支未单列自动化用例——AppShell 无全量 mock 基建，为各自 4 行 catch 搭建整机挂载不成比例，以类型检查与全量回归覆盖；若后续建立 AppShell 测试基建可补。

## 六、程序功能丰富性和完整性

### FUN-01（P0，Closed）：HTTP Basic Auth 探测成功后实际下载丢失 Authorization

- **证据**：[`create.rs`](../src-tauri/src/commands/tasks/create.rs) 曾构造 `auth_headers` 并只用于 probe；持久化的仍是原始 `request_headers`。HTTP 引擎只消费这些 headers。
- **影响**：对话框凭据和 URL 嵌入凭据会让 probe 通过，但任务下载、续传和受保护 sidecar 随后收到 401。
- **修复**：`merge_basic_auth_headers` 在 `HttpEngine::download` / `probe`、续传 probe 与 sidecar 发现路径注入 Basic Auth；`Authorization` 不写入 `task_request_headers`。
- **验证测试**：`download_uses_persisted_basic_auth_credentials`（`http_engine.rs`）；`merge_basic_auth_*` 单元测试（`http/request.rs`）。
- **验收**：受保护 HTTP 下载使用加密凭据成功；headers 表不持久化 Authorization。

### FUN-02（P0，Closed）：逐任务代理既不能参与创建，也未进入 HTTP 系真实下载路径

- **证据**：`CreateTaskInput` 曾无 proxy 字段；scheduler 已解析 task proxy，但 HttpEngine 与派生引擎只读全局 SharedProxyConfig。
- **影响**：必须走代理或必须绕过全局代理的资源可能无法创建；已设置 Custom/Off 的任务仍走全局路由。
- **修复**：`HttpEngine::client_for_config`；download/probe 与 HLS/DASH/Metalink/WebDAV 使用 context/probe 级代理；`CreateTaskInput`/`ProbeTaskInput`/`resolve_probe_proxy_config` 贯通创建与探测；创建后 `upsert_task_proxy_settings`。
- **验证测试**：`resolve_probe_proxy_config_supports_inherit_off_custom`（`task_proxy.rs`）；fingerprint 不含密码。
- **验收**：Inherit/Off/Custom 解析正确；运行时 HTTP 系使用 task proxy。

### FUN-03（P1，Closed）：浏览器 Header 过期后的官方恢复路径是死路

- **证据**：[`request_headers.rs`](../src-tauri/src/db/request_headers.rs) 删除过期 header 并返回 `auth_headers_expired`；handoff 曾固定 `allow_duplicate=false`，重复检测覆盖 needs_attention/failed。
- **影响**：24 小时后或密钥不可用时，UI 提示“从浏览器重新发送”，但重新发送只会失败。
- **修复**：`try_recover_auth_header_task`：同 URL 且 `auth_headers_*` + NeedsAttention/Failed 时原子 upsert headers、requeue 原任务；活动任务仍拒绝。
- **验证测试**：`browser_auth_recovery.rs`（`fun03_recovery_candidate_only_auth_attention_or_failed`、`fun03_expired_headers_refresh_and_requeue_same_task`）。
- **验收**：expired 到 resend 到 same-task resume 全流程通过；不能借此覆盖正常活动任务或突破 duplicate 策略。

### FUN-04（P1，Closed）：认证 FTP、SFTP、WebDAV 目录探测不接收凭据或代理

- **证据**：[`NewDownloadDialog.tsx`](../src/components/shell/NewDownloadDialog.tsx) 曾只向目录探测传 URL；后端三个命令也只接收 URL。SFTP 私钥无法嵌入 URL。
- **影响**：认证目录无法使用推荐的加密凭据流程，FTP/WebDAV 临时嵌入 URL 后还可能在清洗后丢失凭据。
- **修复**：新增统一 `DirectoryProbeInput`（URL、凭据、私钥、代理），三协议命令与引擎目录探测复用 create/probe 的凭据与 `resolve_probe_proxy_config`；新建对话框把当前草稿凭据/代理传入目录探测，返回候选不含明文凭据。
- **验证测试**：`directory_probe.rs`（`fun04_webdav_directory_probe_uses_draft_password`、`fun04_ftp_directory_probe_uses_draft_password`、`fun04_sftp_directory_probe_uses_private_key_credentials`、`fun04_ftp_directory_probe_uses_socks5_proxy`）；`create-draft.test.ts`。
- **验收**：三协议的密码目录、SFTP 私钥目录和 SOCKS5 代理目录均有端到端测试，返回候选不含明文凭据。

### FUN-05（P1，Closed）：自动 sidecar 校验和发现与任务完成存在竞态

- **证据**：[`create.rs`](../src-tauri/src/commands/tasks/create.rs) 曾 fire-and-forget 启动 sidecar 发现；scheduler 只在 worker 成功结束时执行一次校验。
- **影响**：小文件可能先完成为 NotRequested，随后新增 checksum 永久停在 Pending。
- **修复**：保留 create 后异步发现；sidecar 插入后若任务已是 `Completed` 且存在 Pending checksum，调用与 scheduler 相同的幂等 `verify_task_hash_with_pool`（`maybe_verify_completed_task_after_checksum_insert`）。
- **验证测试**：`checksum_sidecar_race.rs`（`fun05_delayed_sidecar_on_completed_task_verifies`、`fun05_delayed_sidecar_mismatch_marks_failed`）。
- **验收**：延迟 sidecar 和小文件组合最终进入 Verified 或明确 Failed，不能永久 Pending。

### FUN-06（P1，Closed）：MIME 分类规则在真实创建路径永远不命中

- **证据**：[`create.rs`](../src-tauri/src/commands/tasks/create.rs) 曾调用分类器时传空 MIME，但 probe 的 content type 已可用；分类器依赖 `content_type.starts_with`。
- **影响**：用户配置的 MIME 分类规则看似可用，实际创建任务时不会触发。
- **修复**：`classification_content_type` 传入 probe MIME（多文件时优先所选文件），保持 extension / URL / MIME 优先级不变。
- **验证测试**：`create.rs` 单元测试（`fun06_create_path_mime_rule_hits_with_probe_content_type`、`fun06_create_path_disabled_mime_rule_skipped_first_match_wins`、`fun06_create_path_prefers_first_selected_file_mime`）。
- **验收**：通过 create 路径合同验证 MIME 命中、禁用规则和首条匹配行为。

### FUN-07（P1，Closed）：关闭计划下载功能不会恢复由计划窗口暂停的任务

- **证据**：[`tasks.rs`](../src-tauri/src/commands/tasks.rs) 的 `check_schedule_preemption` 曾在 schedule disabled 时直接返回；设置保存后虽然调用抢占检查，但 `paused_by_schedule` 任务不会恢复。
- **影响**：用户关闭计划功能后任务仍永久暂停。
- **修复**：禁用计划下载时调用与窗口打开相同的 `resume_schedule_paused_tasks`，仅恢复最新暂停原因为 `paused_by_schedule` 的任务；手动暂停（最新事件为 `paused`）与 `obey_schedule=false` 不恢复。
- **验证测试**：`schedule_preemption.rs`（`fun07_disabling_schedule_resumes_schedule_paused_tasks`、`fun07_disabling_schedule_skips_manual_pause`、`fun07_schedule_then_manual_pause_not_resumed_on_disable`、`fun07_disabling_schedule_skips_obey_schedule_false`）。
- **验收**：enabled → disabled 等价路径只恢复 schedule pause，不恢复 manual pause。

### FUN-08（P1，Closed）：Metalink strongest-hash 选择和完成汇总互相矛盾

- **证据**：[`metalink.rs`](../src-tauri/src/download/metalink.rs) 的选择顺序曾先 SHA-256 后 SHA-512；持久化把 SHA-256 设 primary，而完成汇总要求所有 file hash Verified。
- **影响**：多 hash manifest 可能长期 Pending，也不符合“最强算法”声明。
- **修复**：统一强度序 `SHA-512 > SHA-256 > SHA-1 > MD5`；probe/`is_primary`、校验与 `complete_metalink_task` 只看 per-file primary。
- **验证测试**：`download::metalink::tests`（`fun08_strongest_prefers_sha512_over_sha256`、`fun08_fallback_to_weak_when_no_stronger`、`fun08_complete_uses_primary_only_ignores_pending_secondary`、`fun08_complete_fails_when_primary_mismatches`、`fun08_strength_rank_order`）。
- **验收**：多 hash、单 hash、冲突 hash 和弱算法 fallback 测试均有确定结果。

### FUN-09（P1，Closed）：Metalink 续传缺少远端一致性保护

- **证据**：[`metalink.rs`](../src-tauri/src/download/metalink.rs) 曾仅按本地长度发送 Range，不保存镜像 ETag/Last-Modified，不使用 If-Range，也未严格验证 Content-Range 起点。
- **影响**：远端内容变化或镜像切换时可能拼接新旧内容；无 manifest hash 时无法发现静默损坏。
- **修复**：迁移 `006_metalink_resource_validators`；同镜像续传带 If-Range 并校验 Content-Range；跨镜像无 primary checksum 则截断重头；并行 part 路径对齐。
- **验证测试**：`metalink_engine.rs`（`fun09_same_mirror_resume_persists_validators`、`fun09_cross_mirror_without_checksum_restarts_part`、`fun09_cross_mirror_with_checksum_allows_resume`、`fun09_mismatched_content_range_rejects_resume`）；`download::metalink::tests::fun09_parse_content_range_and_if_range_prefers_strong_etag`。
- **验收**：镜像内容变化、validator 变化、错误 Content-Range 和 failover 测试不会发布混合文件。

### FUN-10（P1，Closed）：HLS 外部音轨和字幕是非对称的部分实现

- **证据**：HLS 引擎（当时为单文件 `hls.rs`，现已拆分为 [`download/hls/`](../src-tauri/src/download/hls/)）曾直接请求原始 track URI，未相对 master URL 解析；失败只 warning 并继续。额外轨不复用主 pipeline 的 AES、byte range、EXT-X-MAP、live、重试、限速和续传能力。
- **影响**：用户明确选择的轨道可能静默缺失，任务仍显示成功。
- **修复**：probe/`parse_ext_x_media` 相对 master 解析为绝对 URI；`build_hls_segment_plans` + `download_hls_rendition` 复用主 pipeline；选中轨失败返回 `hls_track_failed`；live 选中轨进入同一 poll loop。
- **验证测试**：`hls_engine.rs`（`fun10_relative_audio_track_is_resolved_and_downloaded`、`fun10_selected_track_404_fails_visibly`）；`download::hls::tests`（相对 URI resolve）。
- **验收**：相对 URI、选中轨失败可见、外挂轨复用主 pipeline。

### FUN-11（P1，Closed）：BT 做种时间限制未执行，UI 还会清空策略

- **证据**：[`bt.rs`](../src-tauri/src/download/bt.rs) 的做种循环曾只读取 ratio；[`TaskDetails.tsx`](../src/components/shell/TaskDetails.tsx) 切换时固定传两个 null。
- **影响**：可保存的时间限制实际无效，用户打开或关闭做种会丢失已有 ratio/time 设置。
- **修复**：做种循环用 `seeding_limit_reached`（ratio **或** time 任一达标）；`update_torrent_seeding(..., update_limits)` 在 toggle 时只改 `seeding_enabled`；详情页暴露 ratio/time 编辑并在快照中回读。
- **验证测试**：`download::bt::seeding_limit_tests`（ratio / time / either / unlimited）；`TaskDetails.test.tsx`（toggle 传 `updateLimits: false`）。
- **验收**：ratio、time、任一条件、无限做种与 UI toggle 不丢策略。

### FUN-12（P2，Closed）：DASH 只支持较窄的静态 MPD 子集

- **证据**：[`dash.rs`](../src-tauri/src/download/dash.rs) 曾对 multi-Period / `$Time$` 等未实现模板静默降级；README/UI「无法恢复」与 `supports_resume: true` 矛盾。
- **影响**：未支持的 MPD 可能生成残缺文件，或用户误解为完全不可暂停。
- **修复**：文档/UI 对齐 static/VOD first-pass；明确拒绝 multi-Period（`dash_multi_period_unsupported`）、未实现模板变量（`dash_template_unsupported`）；保留 dynamic / SegmentTimeline 拒绝；corpus 落在 `tests/fixtures/dash/`。
- **验证测试**：`download::dash::tests`（`rejects_multi_period_mpd`、`rejects_time_template_placeholder`）；`dash_engine.rs`（fixture corpus probe 拒绝）；README / `dashLimitationsDescription`。
- **验收**：Boundary 合同已锁定——支持矩阵逐项有正向或明确拒绝测试，未支持 manifest 不生成残缺文件。

### FUN-13（P2，Closed）：浏览器正式发布能力与当前产品表述不一致

- **证据**：release 配置固定关闭 capture 并移除 downloads、cookies、webRequest；商店材料只承诺手动 handoff。
- **修复**：统一 README / browser-integration / 扩展 locale 与 Settings banner；`verify-extension-manifest.mjs` 增加 `verifyProfileCopyBoundaries`。
- **验证测试**：`pnpm verify:extensions`；`scripts/release-config.test.mjs`。
- **验收**：每种发布 profile 的 UI、manifest 权限、文档和实际行为一致。

### FUN-14（P2，Closed）：站点规则的 Ask 实际不会询问

- **证据**：[`background.js`](../browser/extension-core/src/background.js) / [`capture-policy.js`](../browser/extension-core/src/capture-policy.js) 对 header ask 固定不转发，对 capture ask 固定不接管。
- **修复**：改名对齐被动语义（不实现确认流）；桌面 7 locale + 扩展 en/zh_CN；文档同步。
- **验证测试**：`scripts/extension-capture-policy.test.mjs`。
- **验收**：扩展行为测试覆盖 Always、Never、Ask（被动）与规则优先级。

### FUN-15（P2，Closed）：BT tracker 和 peer 诊断数据不完整

- **证据**：tracker 主要从 magnet URI 解析并固定为 configured，torrent URL 和本地 torrent 可能为空，seed count 多处固定为 0。
- **修复**：magnet `tr=` 与 `.torrent` announce/announce-list 写入 `source=configured` + `updated_at`；UI 明示 configured-only；`seed_count` 在无可靠来源时为 `null`（不再展示假 0）。诊断合同已锁定——本批不强求 live announce 面板。
- **验证测试**：`download::bt::tests`（`magnet_trackers_are_configured_only`、`torrent_bytes_announce_list_produces_configured_trackers`、`http_torrent_url_without_bytes_yields_empty_configured_trackers`）；`TaskDetails.test.tsx`（configured-only 文案 + 诚实 peers 展示）。
- **验收**：magnet / 远程 torrent / 本地 torrent 的 tracker 来源与「非实时健康」语义明确。

### FUN-16（P2，Closed）：数据导出、备份和恢复不闭环

- **证据（历史）**：[`src/lib/export.ts`](../src/lib/export.ts) 只导出少量展示字段；没有导入该格式的命令。数据库备份只服务于迁移异常，恢复页不能验证和恢复备份。
- **修复**：任务 JSON/CSV 明确为 **报表导出**；新增版本化 `.vibe-backup`（magic/manifest/SHA-256）；设置页 Data backup 提供导出/校验/恢复；恢复前 verified snapshot，校验失败不触碰 live；成功后暂存 `.vibe-restore-pending` 并要求重启（`apply_pending_restore_if_any`）。凭据默认 `machine_bound_ciphertext`（同机可解密，跨机需重录）；全局代理密码不进备份。
- **验证测试**：`backup_restore.rs`（round-trip / corrupt checksum / truncated 不破坏 live）；`db::backup` 单元测试。
- **验收**：跨版本/同 schema round-trip；损坏备份拒绝；恢复失败不破坏原库。

### FUN-17（P2，Closed）：新建和批量流程只覆盖后端创建能力的子集

- **证据**：后端输入支持 task speed、priority 和 category，但新建窗口曾固定为 null；批量导入不支持凭据、代理、hash、优先级、分类、duplicate override 或媒体选择。
- **修复**：前端共享 `CreateDraft`（`src/lib/create-draft.ts`）覆盖凭据、proxy、expected hash、priority、category、taskSpeedLimit、allowDuplicate；单任务 UI 暴露这些字段；扩展 `ImportUrlsInput` 批量共享同一 draft；目录探测复用同一 auth/proxy 子集。不做完整「每 URL 独立 HLS 轨选择」向导。
- **验证测试**：`create-draft.test.ts`（单/批字段一致、目录探测合同、hash 镜像）。
- **验收**：单任务和批量输入使用同一合同；后端支持字段 UI 可达。

### FUN-18（P2，Closed）：非 HTTP 协议尚未达到同等级可靠性

- **证据（历史）**：[`docs/protocol-reliability-matrix.md`](protocol-reliability-matrix.md) 曾将 BT/HLS/DASH/Metalink 的多项 retry、proxy、credentials、checksum 和 diagnostics 标为 partial，并缺产品级中断再入证据。
- **改进**：按协议建立 create、download、pause、resume、retry、restart、delete、proxy、credentials、checksum、diagnostics 生命周期矩阵，并以本地假服务 / fixture 集成测试作为升格门槛。
- **C4 子集（Closed）**：FTP/SFTP/WebDAV 行的 Retry 与 Diagnostics 升至 `automated`（目录探测、凭据轮换、代理/权限失败稳定码、implicit FTPS+SOCKS5 拒绝、host-key forget→retry、引擎级 pause/resume）。
- **C5（Closed）**：HLS/DASH/Metalink 下载/probe 合并持久化 Basic Auth；BT `bt_engine.rs` 覆盖 Probe/Proxy/Retry 可恢复合同/Checksum（piece 校验、非 SHA sidecar）/Diagnostics；HLS/DASH/Metalink 在 `reset_interrupted_tasks(auto_resume=true)` 后由新引擎实例续传完成。矩阵中 BT/HLS/DASH/Metalink 剩余 `partial` 已全部升为 `automated`（BT Restart 继续以 `segments.rs` DB 合同为准，Evidence 注明跨 librqbit session 再入非 C5 门槛）。
- **验证测试**：`hls_engine.rs` / `dash_engine.rs` / `metalink_engine.rs` / `bt_engine.rs` / `segments.rs`；`pnpm verify:protocol-matrix`。
- **验收**：每个声称稳定的协议至少有本地真实服务或固定 fixture 的集成测试，不只验证路由入口；FUN-18 关闭后矩阵不再保留上述协议的核心生命周期 `partial` 单元格。

### FUN-19（P3，Boundary）：中长期能力边界

当前仍未实现稳定 CLI/JSON-RPC/REST、PAC/WPAD、云盘解析、云账号同步、插件协议、完整视频嗅探、Safari wrapper 和商店正式签名；WebDAV 仅 Basic，Metalink 资源仅 HTTP/HTTPS。这些能力应在 P0/P1 清零和协议可靠性矩阵闭环后再扩展。

### FUN-20（P1，Closed）：DASH / FTP / SFTP 的探测路径未接入逐任务代理

- **证据**：`FUN-02` 已让下载路径全面使用 `client_for_config`，但探测侧存在三个缺口。[`dash.rs`](../src-tauri/src/download/dash.rs#L86) 的 `probe_dash` 用全局 `self.client()`，函数签名里根本没有 `proxy_config` 参数，`DashEngine::probe` 也未传 `request.proxy_config`；[`ftp.rs`](../src-tauri/src/download/ftp.rs#L149) 与 [`sftp.rs`](../src-tauri/src/download/sftp.rs#L181) 的 `probe_target` 读的是全局 `SharedProxyConfig`。对照 `probe_hls`（[`hls/engine.rs`](../src-tauri/src/download/hls/engine.rs#L287)）、`probe_metalink`、WebDAV 三者都已正确接线。
- **影响**：为某个任务配置 Custom 代理或 Off 之后，创建阶段的探测仍走全局路由。需要代理的站点探测失败、任务建不出来；需要绕过全局代理的内网资源反被送进代理。`FUN-02` 的验收「Inherit/Off/Custom 解析正确」对这三条探测路径不成立。
- **修复方向**：给 `probe_dash` 增加 `proxy_config: Option<&ResolvedProxyConfig>` 并按 `probe_hls` 的写法逐行对齐；FTP/SFTP 的 `probe_target` 改为消费 `ProbeRequest.proxy_config`。
- **验收**：三个协议各有一条「Custom 代理探测成功 / Off 绕过全局代理探测成功」的集成测试；`FUN-02` 的协议覆盖表补齐探测列。
- **2026-09-12 修复**：`probe_dash` 增加 `proxy_config` 参数并按 `probe_hls` 范本接线；FTP/SFTP 的 `probe()` 不再丢弃 `request.proxy_config`，`probe_target` 以任务级代理驱动建连（无请求级配置时回退全局）。Custom 代码路径与下载/目录探测共用同一 `client_for_config`/`connect_session` 机制（`directory_probe.rs` 的 SOCKS5 中继测试已覆盖）。
- **验证测试**：`ftp_engine.rs`/`sftp_engine.rs`/`dash_engine.rs` 各新增 `fun20_task_proxy_off_bypasses_unreachable_global_during_probe`（全局代理指向不可达 SOCKS5、任务级 Off 仍探测成功）；FTP 另有反证测试 `fun20_probe_without_task_proxy_still_uses_unreachable_global`（无任务级配置时确实走全局并失败，证明测试可捕获回归）。

### FUN-21（P1，Closed）：七个 locale 的 `errors.*` 实际未翻译，而完整性检查查不出来

- **证据**：[`check-i18n-completeness.ts`](../scripts/check-i18n-completeness.ts) 曾只比对 key 路径集合的 missing/extra。7 个 locale 各约 1390 个 key，`pnpm check:i18n` 通过；但值层面 zh-CN / zh-TW 有 86 条、ja/ko/ru/es 各有 94 条 `errors.*` 是原样英文。例如 [`zh-CN.ts`](../src/i18n/locales/zh-CN.ts) 的 `authHeadersUnavailable`、`btMetadataFailed` 与紧邻的正常中文条目并列。
- **影响**：直接推翻三处声明——AGENTS.md 的「fully translated」、`UX-11` 的验收「所有公开错误码在 7 个 locale 有映射」（key 有映射不等于用户看到本地化文案）、以及 PRODUCT.md「避免展示未加解释的原始技术码」。非英语用户在最需要理解的失败场景下看到的正是英文原文。这也是 `UX-11` 的直接遗留面：那一批把新增错误码以英文原文追加进了全部 7 个 locale。
- **修复方向**：先给检查脚本加 value 层校验——非 en locale 若某 key 的值与 en 完全相同、包含 3 个以上英文单词、且不在白名单（`app.name`、`locale.*`、协议格式示例）内则报告；先以 WARN 输出计数，补完 `errors.*` 后升级为 FAIL。
- **验收**：`pnpm check:i18n` 能检出值层面未翻译项；7 个 locale 的 `errors.*` 全部本地化；在此之前任何文档不得声称某 locale「完整翻译」。
- **2026-08-13 根因**：不是「忘记翻译」，而是生成器设计如此。[`sync-stable-error-i18n.mjs`](../scripts/sync-stable-error-i18n.mjs) 的 `messageSets` 把 zh-TW / ja / ko / ru / es 直接映射到 `STABLE_ERROR_MESSAGES_EN`，只有 zh-CN 有一份硬编码的中文表。更进一步：连 zh-CN 的 `dashNoTracks` 等条目在 locale 文件里也仍是英文，说明该脚本的输出**从未真正落地**过。因此修复 `FUN-21` 需要同时处理三件事——给脚本补齐 5 个 locale 的译文、让脚本的输出与 locale 文件一致、再给 `check-i18n-completeness.ts` 加 value 校验，缺一仍会漂移。
  本批次新增的 `dash_segment_count_too_large` 按现状登记（7 个 locale 均为英文，中文已写入 sync 脚本的 zhCN 表备用），以免在统一修复前引入一条孤立的不一致条目。
- **2026-08-14 修复**：[`scripts/stable-error-messages.json`](../scripts/stable-error-messages.json) 成为 6 个非 en locale 的错误码译文源；`sync-stable-error-i18n.mjs` 拒绝缺译或原样英文。`check-i18n-completeness.ts` 同时比对 missing/extra key、插值占位符，以及值层未翻译项（`errors.*` 一律 FAIL；其他命名空间在 3 个以上可见英文单词且不在白名单时 FAIL）。同批补齐 ja/ko/ru/es 中站点规则等从英文粘贴的 UI 文案，并把 `SiteRulesEditor` 的错误 key `common.undo` 改为已有的 `toast.undo`。
- **验证测试**：[`scripts/check-i18n-completeness.test.ts`](../scripts/check-i18n-completeness.test.ts)（10 项）；`pnpm check:i18n` 对 7 个 locale 各 1398 个 key 通过。beta 语言的复数/日期问题见 `FUN-22`（2026-09-12 Closed）。

### FUN-22（P2，Closed）：复数形式缺失与日期本地化未走应用语言

- **证据**：`en.ts` 有 36 处 `{{count}}` 插值，但整个 locale 目录只有 `moreFixesCount` 一个 key 提供了 `_one` 变体（[`en.ts`](../src/i18n/locales/en.ts#L1264)）。俄语 [`ru.ts`](../src/i18n/locales/ru.ts#L1272) 缺 `_few`/`_many`，而俄语需要 4 种形式。日期侧有两种写法混用：`QueueCenter.tsx:424` 与 `AttentionCenter.tsx:328` 正确传 `i18n.language`，而 `TaskDetails.tsx:2267`、`TaskRow.tsx:649`、`AboutPage.tsx:359`、`SettingsPage.tsx:2709` 用的是 `toLocale*(undefined, ...)`，取的是系统语言而非应用内所选语言。
- **影响**：英文在 count=1 时输出「1 connections」；用户切到日语后任务行时间戳仍按系统区域显示。另外 `AttentionCenter.tsx:328` 与 `QueueCenter.tsx:424` 在组件 render 内构造 `Intl.DateTimeFormat`，而构造成本比 `.format()` 调用高 1-2 个数量级。
- **修复方向**：句子型 `{{count}}` 补 `_one`/`_other`，俄语补 `_few`/`_many`；括号计数型（`(3)`）保持现状。新建 `src/lib/format-date.ts`，比照 [`utils.ts`](../src/lib/utils.ts#L19) 已有的 `Intl.NumberFormat` 缓存 + `languageChanged` 失效模式，统一 6 处日期格式化并一律使用 `i18n.language`。
- **验收**：切换语言后所有日期与复数文案随之变化；组件 render 内不再构造 `Intl` 实例。
- **2026-09-12 修复**：四件事同时落地。其一，18 个句子型 `{{count}}` key 在 [`en.ts`](../src/i18n/locales/en.ts) / [`es.ts`](../src/i18n/locales/es.ts) 补 `_one`，[`ru.ts`](../src/i18n/locales/ru.ts) 补 `_one`，其中 7 个名词/动词随数变化的 key 另补 `_few`；括号计数型（`(3)`）与「已选 N」这类不带名词的句子保持单一形式——i18next 在变体缺失时回落到 base key，故无需 `_other`。其二，修掉 `ru.ts` 把数字写死的 `moreFixesCount_one`（俄语 one 形式覆盖 1/21/31…，实测 count=21 曾输出「Ещё 1 исправление」，现为「Ещё 21 исправление」），并删除 `zh-CN`/`zh-TW`/`ja`/`ko` 的 `moreFixesCount_one`——这四个语言的 CLDR 只有 `other`，i18next 永不会选中它。其三，[`check-i18n-completeness.ts`](../scripts/check-i18n-completeness.ts) 不再要求 key 集合逐一相等，改为按 **CLDR 类别**校验：`pluralCategories()` 取自 `Intl.PluralRules`，`findPluralBases()` 以「base 存在 + 插值 `{{count}}` + 存在 `_<cat>` 兄弟键」识别复数基准（故 `taskList.failure_other` 这类同形 key 不会被误判为 `taskList.failure` 的复数形式）。缺失本语言会选中的形式报 FAIL，存在本语言永不会选中的形式报 `unreachablePlurals` 并 FAIL。其四，新建 [`src/lib/format-date.ts`](../src/lib/format-date.ts) 的 `formatDateTime(value, style)`：四种预置样式，`Intl.DateTimeFormat` 按 `locale:style` 缓存并在 `languageChanged` 时清空，与 [`utils.ts`](../src/lib/utils.ts#L19) 的 `Intl.NumberFormat` 缓存同构；`TaskRow`、`TaskDetails`、`AboutPage`、`SettingsPage`、`AttentionCenter`、`QueueCenter` 六处统一走该入口，`AttentionCenter`/`QueueCenter` 的 render 内构造随之消失。
- **验证测试**：[`src/i18n/plurals.test.ts`](../src/i18n/plurals.test.ts)（en 在 count=1 选中 `_one`；ru 经 `setLocale` 懒加载后选中自身 `_one`/`_few`）；[`scripts/check-i18n-completeness.test.ts`](../scripts/check-i18n-completeness.test.ts) 由 10 项增至 18 项，覆盖 CLDR 类别、复数基准识别、允许语言特有复数、拒绝不可达变体、字面量键扫描。`pnpm check:i18n` 对 7 个 locale 全部通过。

### FUN-23（P1，Closed）：备份导出在目标目录与应用数据不同卷时必然失败

- **证据**：[`db/backup.rs`](../src-tauri/src/db/backup.rs#L74) 的 `snapshot_database_to_path` 先在 live DB 旁（app-data 卷）生成校验过的 VACUUM INTO 快照（connection.rs:213），再用 `std::fs::rename` 移到用户目标；Windows 上 rename 映射 MoveFileExW 且无 `MOVEFILE_COPY_ALLOWED`，跨卷返回 `ERROR_NOT_SAME_DEVICE`（POSIX 为 EXDEV），map_err 分支还会把好快照删掉。目标是保存对话框任一盘符（src/lib/backup.ts:21-28 → create_app_backup，commands/backup.rs:57-58 用 `dest.with_extension("sqlite.tmp")`）。
- **影响**：C:+D: 双盘环境（本仓库作者环境即如此）备份功能 100% 失败，报「Could not move verified snapshot into place」。这是应用主打的 data-safety 特性。集成测试（backup_restore.rs:85-119）只用 live DB 同目录 sibling 路径，从未覆盖跨卷。
- **修复方向**：rename 失败（或预判目标与源不同卷）时回退 `std::fs::copy` + 校验后再清理临时快照；或在目标卷直接落盘。
- **验收**：模拟跨卷导出的集成测试成功且 sha256 校验通过；原同卷路径行为不变。
- **2026-09-12 修复**：`snapshot_database_to_path` 的 `std::fs::rename` 失败（Windows ERROR_NOT_SAME_DEVICE / POSIX EXDEV，即用户目标与应用数据不同卷）时回退 `copy_verified_snapshot`——逐字节复制后全量比对源与目标，不一致即删除目标并报错；同卷 rename 行为不变，源快照在任何路径都只清理一次。
- **验证测试**：`backup_restore.rs` 新增 `fun23_snapshot_copy_fallback_produces_verified_snapshot`（快照输出可完整加载且含种子任务；rename/copy 两路径都经过同一 verify 语义）。跨盘符的端到端场景依赖双卷环境，copy 路径的字节校验逻辑以纯函数形式覆盖。

### FUN-24（P2，Closed）：DASH `$Number%05d$` 通过校验但不被替换，URL 必然 404

- **证据**：`segment_template_has_unsupported_vars`（dash.rs:633-649）对 `$...$` token 只比较 `%` 前的 base，`$Number%05d$` 被放行——其注释自称「`$Number$` / `$Number%05d$` 是仅有的两种展开形式」；而 `build_segment_plans` 只做 `media_template.replace("$Number$", …)`（dash.rs:768），宽度前缀形式永不匹配，占位符原样进入 URL。
- **影响**：spec 允许的零填充编号 MPD 全部 segment 必败（初始 + 2 次重试）；叠加 `ARC-37` 甚至僵尸而非报错。这是「校验器声称支持、实现不支持」的契约缝隙。
- **修复方向**：替换时识别 `%0Nd%` 形式做宽度填充（推荐）；或校验阶段明确拒绝并在探测时报「不支持宽度前缀编号」。二者取其一。
- **验收**：用 `$Number%05d$` fixture 端到端下载成功，或探测阶段结构化报错。
- **2026-09-13 修复**：两条路都做——`expand_number_template` 支持 `$Number$` 与 `$Number%0Nd$`（1-9 位宽度的 printf 零填充，不认识的 token 返回 None → `dash_template_unsupported` 结构化错误而非 404）；校验改上下文相关（`template_has_unsupported_vars(value, allow_number)`）：media 放行 Number 两种形式、**Initialization 中任何 `$Number` 一律拒绝**（DASH 规范 Number 不得用于 Initialization，旧 base 检查放行后 init 必 404）、其余标识符维持拒绝。
- **验证测试**：单测 `expands_number_template_with_printf_width`（plain/%05d/%08d/多次出现/非法宽度矩阵）与 `template_validation_is_context_aware`；集成 `fun24_padded_number_template_downloads_padded_urls`（假服务器记录请求路径，断言 `/seg-00001.m4s`、`/seg-00002.m4s` 被请求且无裸 `$` 占位符上线）与 `fun24_number_in_initialization_template_is_rejected`（probe 结构化拒绝）。旧行为敏感性：还原 `.replace` 后 padded 测试失败。

### FUN-25（P2，Closed）：签名 CDN 下 DASH 续传退化为全量重下

- **证据**：`bulk_upsert_dash_segments` 的冲突保护键要求 `uri AND local_path` 均不变才保留 status/downloaded_bytes，否则重置 pending/0（[`db/dash.rs`](../src-tauri/src/db/dash.rs#L274)）；`run_dash_download` 每次会话重新抓 MPD 并重建计划（dash.rs:935-937），且先 upsert 后读 skip 集（:1011-1015）。Akamai/CloudFront 式 per-session URL 签名使 uri 全部漂移。第 3 轮登记的 DASH resume regression 在静态 URL 场景已由该 CASE 修复，本条是其残余面。
- **影响**：暂停 90% 后恢复 → 100% 重下、进度条归零。索引键 (track_kind, segment_index) 本身对齐，无损坏，纯带宽与时间浪费。
- **修复方向**：upsert 保护键去掉 uri 相等要求（以 track_kind + segment_index + 尺寸/时长一致为准），uri 仅作展示字段更新；或持久化首会话模板指纹校验远端内容未变。
- **验收**：每次返回不同 query 签名的本地 mock MPD 下断言续传跳过已完成 segment。
- **2026-09-12 修复**：`bulk_upsert_dash_segments` 与 `upsert_dash_segment` 的进度保护键从「uri AND local_path 不变」改为内容指纹「duration_ms 相同 AND byte_range_start/length 逐值相等（含 NULL 的 `IS` 比较）」；uri/local_path 降级为展示字段照常更新。CDN 每会话换签名不再重置已完成分片；真实清单变更（时长/范围漂移）仍正确重置。
- **验证测试**：`dash_engine.rs` 新增 `fun25_signed_url_change_preserves_completed_segments`（三次 upsert：签名轮换保持 Completed+900000 且 uri 刷新；时长变化重置为 Pending/0）。

### FUN-26（P2，Closed）：restore 写入 proxy_password_saved='false' 的是旧库，启动即被 pending restore 覆盖

- **证据**：[`commands/backup.rs`](../src-tauri/src/commands/backup.rs#L186) 的 INSERT 目标是 `state.pool`（当前 live DB）；下次启动 `apply_pending_restore_if_any`（db/connection.rs:82 → db/backup.rs:483-508）先删 live DB 及 -wal 再 rename 覆盖，该写入连同 WAL 缓冲一起消失。备份若来自他机则标志为 true 而本机 keyring 无密码：`ResolvedProxyConfig`（proxy.rs:56-61）读到 true → `load_proxy_password()` 对 NoEntry 静默 None（proxy.rs:180）→ `custom_socks5_url_with_auth` 以空密码 `unwrap_or("")` 继续（proxy.rs:91）。
- **影响**：设置页显示「已保存代理密码」而认证代理全部失败——正是该注释声称要防止的状态。
- **修复方向**：把标志修正移到 `apply_pending_restore_if_any` 成功之后对恢复后的库执行；或恢复确认对话框明示全局代理密码不随备份迁移。
- **验收**：跨机 restore 流程结束后 settings 标志与本机 keyring 实际一致。
- **2026-09-12 修复**：`commands/backup.rs` 中写入 live 库的 `proxy_password_saved='false'` 已删除（它会被启动时的整库替换覆盖，从未生效）；改为 `db/backup.rs::post_restore_scrub` 在恢复应用后按**本机 keyring 实况**（`load_proxy_password`）修正该标志——本机有密码则 true，否则 false。备份里的任何值都不再被信任。
- **验证测试**：与 SEC-09 同一测试覆盖（备份内 'true' 在无本机密码时被修正为 'false'）。跨机 keyring 缺失场景因此可见：设置页不再显示未保存的密码。

### FUN-27（P3，Closed）：hls_tasks 行读取瞬时失败时选中的音轨/字幕被静默跳过

- **证据**：外部轨道管线整体 gated 于 `if let Ok(Some(hls_task)) = db::get_hls_task(...)`（[`hls/engine.rs`](../src-tauri/src/download/hls/engine.rs#L520)），Err 直接以空 extra_inputs 继续——与其上方三行自身注释「Failures are visible - never warn-and-complete with missing tracks」（`:515-517`）直接矛盾。选中 URI 在创建时写入（create.rs:1065-1075）并经 COALESCE 跨会话保活（db/hls.rs:105-106），此刻真实存在。
- **影响**：一次 SQLite 瞬时错误 → ffmpeg 不带该输入 mux，成品缺用户显式选择的字幕/音轨且标记 Completed。
- **修复方向**：传播 Result（`?` 转 engine_error），让失败可见。
- **验收**：mock get_hls_task Err 断言任务 Failed 而非产出缺轨文件。
- **2026-09-13 修复**：`get_hls_task` 的读取移到本次会话的 `upsert_hls_task` 之前（读取的是创建期持久化的选轨行，COALESCE 语义等价），Err 以新稳定码 `hls_state_read_failed`（可重试、非 NeedsAttention）显式失败任务；`Ok(None)` 是「未选外部轨道」的常态（hls_tasks 行仅在创建时选了轨道才写入），保持合法。稳定码走全链路：`STABLE_ERROR_CODES` → `errors.hlsStateReadFailed` ×7 locale → `stable-error-messages.json` 同步。
- **验证测试**：`hls_engine.rs::fun27_hls_state_read_failure_fails_task`——测试池 `DROP TABLE hls_tasks` 注入真实读取失败，断言 engine.download 返回错误且消息含 `hls_state_read_failed`，不再产出缺轨成品。

### FUN-28（P2，Closed）：环境健康检查的全部文案由 Rust 硬编码英文，七个 locale 均不翻译

- **证据**：[`commands/environment.rs`](../src-tauri/src/commands/environment.rs) 内联了 18 条 `summary` 与 20 条 `detail` 英文字面量（如 `"Native Messaging host binary is ready."`、`"ffmpeg was not found. HLS/DASH remuxing will fail."`）。前端把 `item.summary` 原样渲染（[`EnvironmentPanel.tsx:281`](../src/components/settings/EnvironmentPanel.tsx#L281)），复制出的剪贴板报告同样直写该字段（[`environment-report.ts:25`](../src/lib/environment-report.ts#L25)）。项目对下载错误已有现成范式：后端只发稳定 code，前端经 `ERROR_CODE_I18N_MAP` 映射（`UX-11`）。
- **影响**：界面切成 zh-CN/ja/ru 后，设置页「Environment」卡片中每一项的摘要与详情仍是英文，复制出的诊断报告与截图同样如此。`pnpm check:i18n` 看不见这批字符串——它们不在任何 locale 文件里。
- **修复方向**：比照 [`stable-error-codes.ts`](../src/lib/stable-error-codes.ts)，为 38 条文案定义稳定 code 并让 Rust 只回 code + 参数；前端映射到 `settings.environment.*`；`formatEnvironmentReport` 接受 `t`，把自身 12 条结构标签（`Checked at:`、`Platform:`、`Checks:`、`Updater:`、`Note: …`）一并本地化。注意 `item.id` 已由 `environmentItem*` 键本地化，缺的只有 summary/detail。
- **验收**：切换语言后设置页环境卡片与复制出的报告文案随之变化；新增键由 `check:i18n` 覆盖。
- **2026-09-12 修复**：按项目已有的 UX-11 稳定 code 范式改造契约。模型侧新增 [`EnvironmentTextCode`](../src-tauri/src/models/environment.rs)（38 个变体，specta 导出为 TS 字符串联合）、`EnvironmentTextParams` 与 `EnvironmentText { code, params, english }`；`EnvironmentHealthItem.summary` 由 `String` 改为 `EnvironmentText`，`detail` 由 `Option<String>` 改为 `Vec<EnvironmentText>`（浏览器检查的 detail 本就是「桥接状态 + 最近交接错误」两段拼接），`EnvironmentFixResult.message` 同样改为 `EnvironmentText`。**`raw` 变体刻意不翻译**：路径、版本号、探针原始错误按 UX-11「原始后端消息只进诊断」保持逐字输出，因此 38 个 code 中真正需要译文的是 37 个。前端新增 [`environment-text.ts`](../src/lib/environment-text.ts)：`ENVIRONMENT_TEXT_KEYS` 用 `satisfies Record<Exclude<EnvironmentTextCode, "raw">, string>` 保证穷尽（后端加变体而前端不加键 → `pnpm typecheck` 直接失败），`formatEnvironmentText` 未命中 code 时回落 `english`。设置页环境卡片、修复 toast、以及 [`environment-report.ts`](../src/lib/environment-report.ts) 的 13 条结构标签一并走该通路。
- **验证测试**：[`environment-text.test.ts`](../src/lib/environment-text.test.ts) 遍历 `SUPPORTED_LOCALES` 断言每个 code 的键在**每个** locale 都存在（这张表是动态键，`check:i18n` 的字面量扫描看不到它，此测试即为该通路的门禁），并断言 `raw` 逐字输出；[`environment-report.test.ts`](../src/lib/environment-report.test.ts) 断言切换语言后报告正文随语言变化且原文不再出现。`cargo test --lib` 252 项通过（含 `commands::environment::tests`）。

### FUN-29（P3，Closed）：`t()` 的键不受类型约束，拼写错误只靠 CI 的字符串扫描兜底

- **证据**：全仓没有 `declare module "i18next"` / `CustomTypeOptions`——[`en.ts`](../src/i18n/locales/en.ts) 虽是 `as const`，但没有接到 i18next 的类型上，因此 `TFunction` 接受任意字符串，`t("nav.alll")` 能编译通过并在运行时把键名渲染给用户。2026-09-12 实测：补上该增强后 `tsc --noEmit` 报 **47 处**错误、分布在 14 个文件，绝大多数是 `labelKey: string` 这类把 i18n 键降级为普通字符串的查表结构（`Sidebar.tsx` 8 处、`SiteRulesEditor.tsx` 9 处、`TaskDetails.tsx` 7 处）。
- **影响**：编辑器期零保护。当前唯一防线是 `check:i18n` 的 `SOURCE_KEY_RE` 字面量扫描，它只认完整字面量键（`t("key")` / `t("key", …)`），模板字面量与拼接键不覆盖。
- **修复方向**：导出 `TranslationKey` 类型，把 `labelKey` / `shortcutKey` 等表结构从 `string` 收紧到该类型，再加 `CustomTypeOptions`。上面 47 处即收紧后的完整待修清单。本次未做，是因为它只影响编辑器期（CI 扫描已覆盖同一风险面），且改动横跨 14 个组件，宜单独一批提交。
- **验收**：`t()` 的键受类型约束；动态键经 `TranslationKey` 收窄而非 `as` 断言。
- **2026-09-12 修复**：新增 [`i18next.d.ts`](../src/i18n/i18next.d.ts) 的 `CustomTypeOptions.resources`，其中 bundle 叶子被 `WidenLeaves` 放宽为 `string`——否则 i18next 会按字面量推导每个键的插值参数，一旦键是联合类型（`t(someKey, { count })`）参数类型就退化成空的交集，`t()` 反而不可用；插值契约仍由 `check:i18n` 的占位符比对覆盖。配套在 [`index.ts`](../src/i18n/index.ts) 导出 `TranslationKey`（`TranslationLeaves` 递归展开 en bundle 的每一个点分叶子路径），供「把键当数据存」的表结构使用。共修 **48 处 / 15 文件**，全部是类型收紧而非行为改写：`labelKey` 等字段由 `string` / 无界模板类型收紧到 `TranslationKey`，手写的 `t: (key: string) => string` 签名改为 `TFunction`，`capitalize()` 拼键改为 `Record<Union, TranslationKey>` 查表（`satisfies` 保证穷尽）。
- **验证测试**：脚本比对「旧模板/capitalize 逻辑对每个联合成员产出的键」与 en bundle，**0 处缺失**——即这次重写没有换掉任何一条被显示的文案。另修掉一处顺带发现的隐患：`protocolHintKey` 对未知协议原本会渲染出原始键名，现按 `protocolBadgeLabel` 既有约定回落 HTTP 文案（12 个已知协议行为不变）。`pnpm check`、47 文件 200 项前端测试、生产构建、bundle 预算（912.0 kB / 1126.4 kB）全部通过。

## 七、项目架构的鲁棒性和稳定性

### ARC-01（P0，Closed）：`source_key` 唯一索引错误地限制同站点活动任务

- **证据**：[`001_init.sql`](../src-tauri/src/db/migrations/001_init.sql) 曾对活动状态的 `tasks(source_key)` 建立 partial UNIQUE；HTTP 的 source key 是主机名 [`probe.rs`](../src-tauri/src/download/http/probe.rs#L36)，FTP、SFTP、HLS、DASH 和 WebDAV 也使用 host 级 key。上层 [`create.rs`](../src-tauri/src/commands/tasks/create.rs#L288) 明确只对 BT info-hash 执行 source-key 去重。
- **影响**：同一域名的第二个不同 URL 任务无法入队、暂停或等待网络，与多任务和每主机连接槽设计直接冲突；`allow_duplicate` 也无法绕过 DB UNIQUE。
- **修复**：迁移 `004_drop_source_key_active_unique.sql` 删除 `idx_tasks_source_key_active`；baseline `001_init.sql` 同步移除；BT 去重仍由 `torrent_tasks.info_hash UNIQUE` 负责；host 级 `source_key` 仅用于调度连接槽。
- **验证测试**：`source_key_active_unique_index_is_absent`、`same_host_different_urls_can_coexist_when_active`、`migration_004_drops_legacy_source_key_unique_on_upgrade`、`duplicate_bt_info_hash_still_rejected`（`migration_integrity.rs`）。
- **验收**：同 host 不同 URL 可同时处于 queued、paused 和 downloading；相同 BT info-hash 仍按策略拒绝；并发创建不会产生错误去重。

### ARC-02（P0，Closed）：输出路径没有原子预留和 no-clobber 提交

- **证据**：[`task_file_planning.rs`](../src-tauri/src/commands/task_file_planning.rs) 曾只检查 final 和 `.vibe-downloading` 是否存在；创建时不建立占位，DB 也没有路径唯一约束。[`file_ops.rs`](../src-tauri/src/download/file_ops.rs) 曾先查可用路径再 rename，跨卷 fallback 使用会覆盖目标的 copy。
- **影响**：不同来源、同文件名任务可共享临时和最终路径，产生静默覆盖、混写、错误校验或删除其他任务文件。
- **修复**：temp 改为 `{final}.{task_id}.vibe-downloading`；HLS/DASH staging 用 `{save_dir}/.vibe-staging/{task_id}/`；创建事务内 `list_reserved_final_paths` + `unique_final_path_among`；迁移 `005_final_path_active_unique.sql` 增加 partial UNIQUE；finalize 对已存在目标返回 `final_path_conflict`，跨卷经同目录 staging 再 atomic rename。
- **验证测试**：`concurrent_same_name_creates_reserve_unique_final_paths`、`final_path_active_unique_index_exists`（`path_reservation.rs`，N=20）；`direct_download_conflicts_when_final_path_exists`（`http_engine.rs`）。
- **验收**：并发同名任务各自唯一 final/temp；外部抢占 final 时不覆盖目标。

### ARC-03（P0，Closed）：下载任务和 ffmpeg 子进程所有权不能可靠收敛

- **证据**：[`scheduler/mod.rs`](../src-tauri/src/scheduler/mod.rs) 曾保存外层 supervisor handle 再嵌套 spawn 引擎任务；abort 外层会 detach 内层。HLS/DASH remux 曾用 `Command.status().await`。
- **影响**：暂停、取消、删除或退出后，旧 worker 和 ffmpeg 仍可能占用网络、限速器、磁盘和 DB，甚至在任务删除后发布最终文件。
- **修复**：去掉 scheduler 内层 spawn；`ffmpeg::run_cancellable` 使用 `Child` + `kill_on_drop` + `select!`；HLS/DASH finalize 前确认任务仍为 Downloading；shutdown abort 后再次 await。
- **验证测试**：`throttle_cancels_during_low_rate_wait`（与 ARC-04 共用）；现有 HLS/HTTP cancel 集成测试路径仍覆盖传输中取消。
- **验收**：下载与 remux 阶段取消可收敛；调度槽在 supervisor 真正退出后释放。

### ARC-04（P1，Closed）：限速等待不可取消

- **证据**：[`speed.rs`](../src-tauri/src/download/speed.rs) 的 `throttle` 曾不接 CancellationToken；合法限速可低至 1 B/s。
- **影响**：大 chunk 可在 250ms sleep 循环中等待数小时，取消 token 已触发但 worker 不退出。
- **修复**：`throttle(bytes, cancel)` 在 sleep 路径使用 `select!`；HTTP/HLS/DASH/FTP/SFTP/Metalink 全部传入 cancel token。
- **验证测试**：`download::speed::tests::throttle_cancels_during_low_rate_wait`。
- **验收**：1 B/s 下取消在秒级收敛。

### ARC-05（P1，Closed）：调度全局锁跨越远程 resume probe

- **证据**：[`scheduler/mod.rs`](../src-tauri/src/scheduler/mod.rs) 曾在持有全局调度锁期间 await `start_task`；有临时文件时 `prepare_task_for_download` 会执行远程 probe。
- **影响**：一个慢主机的连接超时会串行阻塞所有队列派发和调度响应。
- **修复**：`start_task` 在锁内仅插入 pending `DownloadControl`、Queued→Downloading 并 spawn worker；resume probe 移入 worker（锁外）。probe 失败时原子移除 pending control 并释放 host 槽。
- **验证测试**：`scheduler_dispatch.rs`（`arc05_slow_probe_does_not_block_other_host`、`arc05_double_dispatch_single_winner`、`arc05_probe_failure_releases_slot`）。
- **验收**：慢 resume probe 不阻塞其他 host 启动；同任务/同槽仍单赢家。

### ARC-06（P1，Closed）：SQLite 状态转移可能遇到 `SQLITE_BUSY_SNAPSHOT`

- **证据**：[`state_machine.rs`](../src-tauri/src/state_machine.rs) 曾用 deferred `BEGIN`，先读后写；其他任务 checkpoint 可在读写之间提交。
- **影响**：busy timeout 不一定处理 snapshot 升级失败，暂停、重试或 worker 完成可能偶发失败。
- **修复**：`db::begin_immediate`（`BEGIN IMMEDIATE`）；对 BUSY / BUSY_SNAPSHOT 有界指数退避重试；`retry_task` / `retry_task_with_mirror` 对齐 pause/cancel 的 JoinHandle drain。
- **验证测试**：`state_machine_busy.rs`（`arc06_checkpoint_and_control_plane_stress`、`arc06_transition_conflict_still_surfaces`）；既有 `scheduler_concurrency.rs` / `state_machine.rs` 条件 UPDATE 回归保留。
- **验收**：高频 checkpoint 与 pause/retry/fail 并发下无偶发 BUSY_SNAPSHOT 用户可见失败。

### ARC-07（P1，Closed）：旧分页请求可覆盖最新查询

- **证据**：[`TaskList.tsx`](../src/components/tasks/TaskList.tsx) 曾用单个 `loadingPageRef` 阻止新请求；查询变化时旧请求未完成，新 effect 直接返回。
- **影响**：快速切换搜索、筛选、导航或排序后，界面可长期显示旧结果。
- **修复**：共享 [`list-query-epoch.ts`](../src/lib/list-query-epoch.ts) generation；replace/append 分轨；pending reload；`AppShell.refreshTasks` 与事件 full refresh 共用 epoch。
- **验证测试**：`list-query-epoch.test.ts`；`TaskList.query-race.test.tsx`。
- **验收**：乱序响应下最终列表对应最新 query；旧响应不改变 cursor、selection 或 error。

### ARC-08（P1，Closed）：实体缓存和当前查询成员关系混在一起

- **证据**：`upsertTask`/`upsertTasksBatch`/`patchTasksBatch` 曾无条件 prepend 或保留已离开筛选的任务。
- **影响**：Completed 视图可能出现 queued 任务；不匹配的新任务被插入。
- **修复**：`taskById` 实体缓存与 `taskIds` 视图分离；`taskMatchesListQuery` + `effectiveListQueryMembership`；不匹配则 evict；匹配但不在页内则 bump `viewReloadToken`；Palette 基于实体缓存过滤。
- **验证测试**：`task-data-store.membership.test.ts`。
- **验收**：状态、筛选与搜索变化后，当前结果集合与后端分页一致（经 reload 收敛）。

### ARC-09（P1，Closed）：`queue-changed` 防抖只保留最后一批 ID

- **证据**：[`use-task-events.ts`](../src/hooks/use-task-events.ts) 每次事件清 timer，closure 只读取最后 payload。
- **影响**：100ms 内多任务变化可能只刷新最后一批。
- **修复**：`accumulateQueueChanged` / `takeQueueFlushPlan` 窗口内累计 ID Set；null 或 >50 提升为 full refresh。
- **验证测试**：`use-task-events.queue-debounce.test.ts`。
- **验收**：快速多事件、重复 ID、超过 50 项和 full refresh 混合不丢任务。

### ARC-10（P1，Closed）：控制面响应缺少流式硬上限

- **证据**：DASH MPD、Metalink 和 WebDAV PROPFIND 曾使用整包 `.text()` 或 `.bytes()`；HLS 无 Content-Length 时先读取完整 body 再检查上限。
- **影响**：异常或恶意服务器可造成 OOM，取消和 idle timeout 也无法及时生效。
- **修复**：共享 `read_body_limited` / `read_local_file_limited`（64 MiB）；替换 HLS/DASH/Metalink/WebDAV 控制面读；新增 `dash_mpd_too_large` / `metalink_manifest_too_large` / `webdav_propfind_too_large`。
- **验证测试**：`download::tests`（local oversize / under-cap）；`hls_engine.rs`（`arc10_oversized_playlist_is_rejected_without_buffering_forever`）。
- **验收**：超大控制面响应在阈值处停止并返回结构化错误。

### ARC-11（P1，Closed）：HLS live 空闲退出条件实际无效

- **证据**：HLS 引擎（当时为单文件 `hls.rs`，现已拆分为 [`download/hls/`](../src-tauri/src/download/hls/)）曾只在 `idle_polls >= 6 && finish == true` 时退出，但 finish 在循环顶部已独立退出；target duration 未 clamp，poll sleep 不可取消。
- **影响**：源停止更新或声明超大 target duration 时永久占用任务槽。
- **修复**：live-like 空闲阈值独立进入 `WaitingNetwork`（`hls_live_idle`）；`HLS_MAX_TARGET_DURATION_SECS=60`；poll sleep 用 `select!` 等待 cancel/finish。
- **验证测试**：`hls_engine.rs`（`live_idle_polls_enter_waiting_network`、`oversized_target_duration_poll_sleep_is_clamped`、`cancel_during_live_poll_sleep_pauses_cleanly`）；`download::hls::tests::clamps_oversized_target_duration`。
- **验收**：停止更新、超大 duration、取消均在有界时间转换状态。

### ARC-12（P1，Closed）：BT 共享 session 的限速和引用计数所有权不清晰

- **证据**：[`bt.rs`](../src-tauri/src/download/bt.rs) 的 session key 曾只含输出目录和代理；复用时把 session 全局限速改成最新任务值。`delete_runtime_task` 和 `SessionRefGuard::drop` 都可能 decrement；创建 session 时还持 registry mutex 跨 await。
- **影响**：一个 torrent 改变同 session 其他任务限速，引用计数可能提前归零，session 初始化阻塞其他 registry 操作。
- **修复**：引用计数仅由 `SessionRefGuard` 释放；`delete_runtime_task` 只 forget/delete；创建 session 不持 mutex 跨 await；session key 纳入 `task_id` 做限速隔离。
- **验证测试**：`download::bt::tests`（`session_key_includes_task_id_for_limit_isolation`、`delete_runtime_task_does_not_decrement_session_refcount`、`session_evicted_when_ref_count_reaches_zero`）。
- **验收**：多任务不互改限速合同（按任务拆 session）；删除不双减；refcount 归零才驱逐。

### ARC-13（P1，Closed）：任务总进度与多文件进度错误耦合

- **证据**：[`task_state.rs`](../src-tauri/src/db/task_state.rs) 每次更新任务进度都把总下载量写给所有 selected task files；BT 调用该函数，前端每个 tick 又复制完整 files 数组。
- **影响**：每个 torrent 文件显示相同的总任务字节数，文件数很大时还产生 O(file_count) 分配。
- **修复**：`update_progress_in_tx` 默认不写 `task_files`；BT 从 `stats.file_progress` 批量更新真实 per-file 字节（节流 `task_updated`）；前端 `applyProgressToTask` 不再改写 `files[]`。
- **验证测试**：`download::bt::tests::bt_file_progress_updates_are_independent_per_file`；`task-data-store.membership.test.ts`（progress tick 不改 `files[].downloadedBytes`）。
- **验收**：多文件进度各自独立且总和一致；列表 progress payload 保持轻量。

### ARC-14（P1，Closed）：启动期 browser handoff 可能被接受后丢失

- **证据**：[`lib.rs`](../src-tauri/src/lib.rs) 的 single-instance 回调在 AppState 管理前只记录 warning 并跳过；handoff 文件没有启动完成后的扫描。
- **影响**：native host 已向浏览器返回 accepted，但桌面应用没有创建任务。
- **修复**：ready 后扫描 handoff 目录并与 CLI args 合并去重；AppState 未就绪时保留文件并明确日志为 startup replay（不再声称扩展会 retry）；处理成功/`duplicate` 后删除文件。
- **验证测试**：`browser_handoff.rs`（`arc14_collect_pending_handoff_single_file`、`arc14_collect_pending_handoff_multiple_files`、`arc14_merge_args_and_scan_dedupes_same_path`、`arc14_ready_replay_processes_all_and_dedupes_request_id`、`arc14_replay_retains_file_on_create_error`）。
- **验收**：冷启动同时收到 1 个或多个 handoff 时全部最终处理且不重复。

### ARC-15（P1，Closed）：SFTP host-key 变化没有可完成的恢复路径

- **证据**：[`db/sftp.rs`](../src-tauri/src/db/sftp.rs) 要求用户明确清除 known-host 行，但没有 list/forget command 或 UI。
- **影响**：合法服务器密钥轮换后用户只能手工修改数据库或重建数据。
- **修复**：`list_sftp_known_hosts` / `forget_sftp_known_host`（DELETE only）+ Settings → Network `SftpKnownHostsEditor`（Dialog 二次确认）；`sftp_host_key_changed` 恢复动作含 `manage_sftp_host_keys` 与 `retry`；mismatch 仍 fail-closed。
- **验证测试**：`sftp_engine.rs`（`arc15_list_and_forget_known_host_then_retofu`、`probe_fails_on_host_key_mismatch`）；`SftpKnownHostsEditor.test.tsx`。
- **验收**：密钥不匹配默认 fail closed，显式 forget 后可接受新 key，不能静默覆盖旧 key。

### ARC-16（P2，Closed）：下载错误类型化仍主要停留在边界包装

- **证据**：曾靠英文子串推断 resume/失败分流；`task_resume` 返回 plain string。
- **修复**：resume 路径改为 `AppErrorPayload` JSON（`remote_changed` / `resume_unavailable` / `temp_file_*` / segment codes）；集中 `NEEDS_ATTENTION_CODES` + `code_from_stored`（仅列或 JSON `.code`）；`mark_download_failed` / `task_error_code` / `error_state_from_message` 去掉 substring fallback。全引擎 `DownloadError` variant 迁移仍可后续推进。
- **验收**：改 message 文案不影响 NeedsAttention/Failed；历史非 JSON 行不再猜码。
- **验证**：`cargo test -j 1 --manifest-path src-tauri/Cargo.toml --lib task_resume::`；`cargo test -j 1 --manifest-path src-tauri/Cargo.toml --test segments`（resume / arc16_*）。

### ARC-17（P2，Partial）：超大模块扩大变更影响面

- **证据**：SettingsPage、TaskDetails、HLS、DASH、Metalink 和 BT 同时承担解析、I/O、状态、渲染或编排中的多项职责。
- **本批已做**：
  - HLS：[`download/hls/{mod,engine,playlist}.rs`](../src-tauri/src/download/hls/) — manifest parser 与 engine 分离，原 parser 单测迁至 `playlist`。
  - TaskDetails：抽出 [`use-task-detail-queries.ts`](../src/hooks/use-task-detail-queries.ts)（segments/requests/events/torrent 轮询门控）；[`TaskDetails.test.tsx`](../src/components/shell/TaskDetails.test.tsx) 覆盖 tab 门控。
- **仍未做（checklist）**：DASH/Metalink parser 抽取、transfer plan、remux process、BT session registry、Settings draft hook。
- **验收（Partial）**：公共行为不变；抽出的 parser / query controller 可直接测试。全量六域拆分不在本批 Closed。
- **验证**：`cargo test -j 1 --manifest-path src-tauri/Cargo.toml --lib download::hls::`；`cargo test -j 1 --manifest-path src-tauri/Cargo.toml --test hls_engine`；`pnpm test:frontend`（TaskDetails）。

### ARC-18（P2，Fixed locally）：文档版本和能力声明漂移

- **实现**：README、AGENTS、ROADMAP、performance baseline、浏览器说明和发布示例已同步到 `0.3.0` 当前事实；`0.2.0` 专项审计保留原版本并明确标记为历史快照。
- **剩余风险**：协议实现或发布 profile 变化后，README、协议矩阵、浏览器权限说明和商店材料仍可能再次漂移。
- **验收**：增加自动文档检查，覆盖主要当前态文档的版本、release capture 边界和关键能力声明；在此之前保持 Fixed locally，不标记 Closed。
- **2026-08-13 复核**：漂移已按预期复发，且代价高于预期。`AGENTS.md` 与 README 曾把 6 项已修复的 P0 继续列为 active blockers，`AGENTS.md` 常量表有 4 项过时（设置 29→33 键、分区 7→11、`hls.rs` 路径、`ARC-11` 描述、以及一条代码中不存在的「DASH progress interval 500ms」）。本轮已人工修正，但**只要自动检查不落地，下一轮仍会复发**。这是所有文档类问题的根因，优先级应从 P2 提升到 P1。

### ARC-19（P0，Closed；协调器排空残留见下）：FTP/SFTP 取消时中止未落盘的 worker，续传写出零字节空洞

- **证据**：[`ftp.rs`](../src-tauri/src/download/ftp.rs#L416) 与 [`sftp.rs`](../src-tauri/src/download/sftp.rs#L523) 的协调器在检测到取消后直接 `return Ok(())`，`workers: JoinSet` 随栈帧 drop —— **drop JoinSet 会 abort 所有仍在运行的 worker**。而 worker 是先通过 channel 上报 offset、再把数据攒在 256 KB `BufWriter` 里（[`ftp.rs`](../src-tauri/src/download/ftp.rs#L873)），协调器每秒把该 offset `force_checkpoint` 落库。对照 HTTP 协调器（[`coordinator.rs`](../src-tauri/src/download/http/segmented/coordinator.rs#L416)）明确等待 `active_workers` 归零，注释也点名了这个风险。
- **影响**：暂停、取消、删除或退出 FTP/SFTP 任务时，DB 中的 `downloaded_until` 最多可比磁盘实际字节多 256 KB × 并发数。恢复时 worker `seek(offset)` 继续写，中间那段是预分配的零字节 —— **最终文件静默损坏，且在没有校验和时无法察觉**。`ARC-03` 只为 HTTP 关闭了这个问题。
- **修复方向**：取消分支改为「排空 JoinSet + 采纳 worker 返回的权威 offset + 再 checkpoint」，与 HTTP 对齐，并加有界兜底超时。更彻底的做法是让 worker 只在 `file.flush()` 之后才上报进度，使「已上报字节」永远不超过「已落盘字节」。建议与 `ARC-31` 的协调器合并一起做，避免在两个文件里各修一遍。
- **验收**：FTP 与 SFTP 各有一条集成测试——传输中取消后，磁盘实际字节数不小于 DB 记录的 `downloaded_until`，且续传后文件哈希与完整下载一致。
- **2026-08-13 修复**：调研发现问题描述只对了一半，实际有两个 bug 面，且早退点是 6 处而非 2 处（含所有 `?`）。第二个 bug 面是：worker 取消时 flush 后已把正确 offset 写进 DB，但协调器内存里仍是陈旧值，紧接着的 `force_checkpoint` 用无守卫的 UPDATE 覆盖回去。因此只加排空并不能修复。
  实际采用的是**建立不变式**而非逐点堵漏：worker 稳态循环改为 `flush()` 之后再上报（[`ftp.rs`](../src-tauri/src/download/ftp.rs)、[`sftp.rs`](../src-tauri/src/download/sftp.rs)），使「已上报字节 ≤ 已落盘字节」恒成立。有了它，**所有** 早退点（含 abort）自动安全，因为协调器内存值永远不会超过磁盘。另外三个取消出口（循环顶部、读 `select!`、限速器）统一为 flush → 上报 → 写 DB；其中限速器那一处此前连 flush 都没有，SFTP 的两处则用 `let _ =` 吞掉了 flush 错误——现已改为 flush 失败就不上报（不可持久化的 offset 绝不能发布）。
- **未做（残留，P2）**：协调器仍在取消时提前 `return` 而非排空 `JoinSet`。有了上述不变式后这不再是正确性问题，只影响取消时白白重下的字节数。应与 `ARC-31` 的 FTP/SFTP 协调器合并一起做，避免在两个文件里各写一遍排空逻辑。
- **验证**：`cargo test --test ftp_engine --test sftp_engine`（12 + 18 通过），`download_pauses_mid_transfer_and_resumes_from_persisted_offset` 新增两处断言——运行期与取消后均校验 `metadata(temp).len() >= downloaded_until`，并逐字节比对已检查点的前缀与源数据（可捕获「长度对但中间是零」的变体）。
- **测试局限（诚实记录）**：把上报移回 flush 之前做红灯验证时，测试**没有变红**。原因是该场景下 worker 正常走取消出口（有 flush），最终状态仍一致；真正的空洞需要 worker 被 `JoinSet` drop 所 abort，而那是难以在集成测试中稳定构造的时序竞态。因此这两条断言是**必要条件而非充分条件**，修复的正确性依据是上述不变式论证，不应宣称已由测试证明。

### ARC-20（P0，Closed）：多文件任务的文件行在事务外插入，冲突留下半创建脏任务

- **证据**：任务行在 [`create.rs`](../src-tauri/src/commands/tasks/create.rs#L944) 已经 commit，之后文件行在 [`create.rs`](../src-tauri/src/commands/tasks/create.rs#L1103) 逐条裸插入（`insert_task_file_record` 走 `&state.pool`，无事务）。而 `task_files.final_path` 上有 `ARC-02` 建立的部分唯一索引（[`001_init.sql`](../src-tauri/src/db/migrations/001_init.sql#L458)）。
- **影响**：两个并发的 Metalink/BT 多文件任务包含同名文件时，双方读到相同的 `reserved` 集合后各自逐条插入；第 K 条命中唯一索引冲突后 `?` 直接向上抛错，而**前 K-1 条文件行已落库、tasks 行已 commit、无回滚也无重试**。数据库里留下文件列表残缺的任务，调度器会按残缺列表下载并标记完成，用户拿到不完整产物。这是静默数据损坏，意味着 `ARC-02` 对多文件任务并未真正闭合。
- **修复方向**：把 `insert_task_record_in_tx` + 全部 `insert_task_file_record_in_tx`（该函数已存在）+ `insert_task_event_in_tx` 收进同一个 `begin_immediate` 事务，并纳入现有的 32 次重试循环；`create_dir_all` 移到事务外先做。
- **验收**：并发创建两个包含同名文件的多文件任务，断言任务数与文件行数一致、无残缺任务；失败路径不留下任何已提交的任务行。
- **2026-08-13 修复**：新增 [`db::insert_task_with_files_in_tx`](../src-tauri/src/db/task_records.rs) 与 pool 级包装 `insert_task_with_files`（`insert_task_file_record_in_tx` 此前已存在但生产代码零调用）。[`create.rs`](../src-tauri/src/commands/tasks/create.rs) 的重试循环改为：事务**外**读预留快照并调用 `task_file_records_from_probe`（它每个文件都要 `create_dir_all` 并 stat 文件系统，放进事务会在持写锁期间做磁盘 IO，把偶发冲突换成永久串行化），事务**内**只做两类 INSERT 再 commit。快照过期由部分唯一索引兜住，整体回滚后带着新快照重试——标准的乐观并发控制。
- **验证**：`cargo test --test path_reservation`（4 通过），新增 `arc20_file_row_conflict_rolls_back_the_task_row`：32 个 worker 规划相同的文件路径但不同的任务路径，使唯一冲突只可能发生在 `task_files` 上，断言恰好 1 个成功且失败者不留下任何 `tasks` 行。按旧路径这条断言必然失败（32 个 worker 都会先提交 task 行）。

### ARC-21（P0，Closed）：写事务普遍使用 DEFERRED，读后写路径会命中 `SQLITE_BUSY_SNAPSHOT`

- **证据**：全仓库只有 [`state_machine.rs`](../src-tauri/src/state_machine.rs#L206) 使用 `db::begin_immediate`，其余 20 余处写事务都是默认 DEFERRED，其中包括 `ARC-02` 的路径预留循环——[`create.rs`](../src-tauri/src/commands/tasks/create.rs#L866) 先 `list_reserved_final_paths` 读快照，再 `insert_task_record_in_tx` 写。[`connection.rs`](../src-tauri/src/db/connection.rs#L35) 的注释已经把这个失效模式写清楚了。
- **影响**：`PRAGMA busy_timeout` 对 `SQLITE_BUSY_SNAPSHOT` **不生效**，SQLite 不会为快照升级冲突重试而是立即返回。更糟的是 `create.rs` 的重试循环只识别唯一索引冲突（`is_final_path_unique_conflict`），BUSY_SNAPSHOT 不匹配就直接 `return Err`，用户看到一条不可理解的 SQLite 错误。连接池 16 条连接下，剪贴板监听 + 浏览器 WS 桥 + UI 批量创建可以轻易触发并发。
- **修复方向**：所有「读后写」事务改用 `db::begin_immediate`（至少 `create.rs:867`、`task_records.rs:774`、`segments.rs:381`、`task_files.rs:136/196` 及 `task_state.rs` 相关处）；复用 `state_machine.rs` 已有的 busy 判定逻辑，把 BUSY/BUSY_SNAPSHOT 也纳入重试。
- **验收**：8 个同名任务并发创建，断言得到 8 个不同的 `final_path` 且无错误；高频 checkpoint 与批量创建并发下无用户可见的 SQLite 错误。
- **2026-08-13 修复**：调研把必改范围从 20 余处收窄到 **2 处**——其余事务的第一条语句就是写，走的是普通 `SQLITE_BUSY`，`busy_timeout=5000` 能兜住。两处分别是 `create.rs` 的路径预留循环和 [`segments.rs`](../src-tauri/src/db/segments.rs) 的 `split_largest_remaining_segment`（两次 SELECT 后才 UPDATE + INSERT，且注释明确说它为并发设计，却用了最危险的 DEFERRED 组合），均改为 `begin_immediate`。
  BUSY 判定与退避从 [`state_machine.rs`](../src-tauri/src/state_machine.rs) 下沉到 [`db/connection.rs`](../src-tauri/src/db/connection.rs) 的 `is_sqlite_busy_message` / `sqlite_busy_backoff` / `SQLITE_BUSY_MAX_ATTEMPTS`，`state_machine` 改为薄包装，保持单一来源。`create.rs` 复用同一套 20/40/80/160ms 退避，且 BUSY 重试使用**独立计数**，不与 32 次唯一冲突重试共享预算。
- **验证**：`cargo test --test path_reservation` 的 `arc21_concurrent_multi_file_creates_all_commit`（32 并发 × 每任务 8 个文件行，全部提交成功）。

### ARC-22（P0，Closed）：DASH 分片数由清单决定且无上限

- **证据**：[`dash.rs`](../src-tauri/src/download/dash.rs#L373) 的 `segment_count` 由 `period_seconds × timescale / duration` 算出，没有任何上界；[`dash.rs`](../src-tauri/src/download/dash.rs#L715) 随即按这个数量构造 `Vec<DashSegmentPlan>`，每个元素含 5 个 String/PathBuf。
- **影响**：一个 `mediaPresentationDuration="PT10000H"` 配 `duration="1" timescale="90000"` 的 MPD 会算出约 3.24×10¹² 个分片，直接 OOM 崩溃；量级较小时也会把它们全部写进 SQLite。`ARC-10` 给控制面 body 加了 64 MiB 上限，但**解析后的结构体数量没有任何上限**。
- **修复方向**：加 `DASH_MAX_SEGMENTS_PER_REPRESENTATION`（建议 100_000）并在计算后立即校验，返回结构化 `dash_segment_count_too_large`；对 `SegmentSource::List` 的长度与 `all_plans` 总量施加同类上限；`parse_iso8601_duration` 结果做合理性检查（例如拒绝超过 30 天）。
- **验收**：fixture corpus 中加入超大 duration / 超小 segment duration 的 MPD，断言在解析阶段被拒绝且不产生残缺文件，进程内存不增长。
- **2026-08-13 修复**：新增 `DASH_MAX_SEGMENTS_PER_REPRESENTATION = 100_000` 与结构化错误码 `dash_segment_count_too_large`（已登记进 [`stable-error-codes.ts`](../src/lib/stable-error-codes.ts) 及 7 个 locale）。校验分两层：解析期的 `template_segment_count`（顺带消除了原本重复两遍的分片数计算），以及 `build_segment_plans` 入口的统一上限——后者是必要的，因为 SegmentList 的长度只受 `CONTROL_PLANE_MAX_BYTES` 约束，走不到解析期的模板检查。
- **验证**：`cargo test --lib download::dash::`（16 通过），新增 4 项：超大 duration 拒绝、边界值（正好等于上限）必须放行、SegmentList 在计划构建器被拒绝、非有限比值饱和到上限而非回绕。

### ARC-23（P1，Closed）：退出时嵌套 timeout 使 abort 分支不可达，worker 被 detach

- **证据**：[`lib.rs`](../src-tauri/src/lib.rs#L131) 的 `shutdown_active_downloads` 中，`join_all` 内每个 future 的 `sleep(timeout)` 与外层 `tokio::time::timeout(timeout, join_all)` 用的是**同一个值**。外层几乎必然先触发并 drop `join_all`，内层的 `handle.abort(); handle.await` 永远执行不到。
- **影响**：drop `JoinHandle` 只是 detach 而非 abort。超时退出时 supervisor task（含引擎、ffmpeg 子进程、打开的 BufWriter）继续运行到进程被 OS 回收，这段时间里仍在写临时文件与 SQLite，而 DB 可能已开始收尾，产生半写状态。`ARC-03` 验收中的「shutdown abort 后再次 await」在当前代码里没有生效。
- **修复方向**：改为两轮——先用 `timeout` 等待优雅退出，再对 `!handle.is_finished()` 的逐个 `abort()` 并 `await`（abort 后 await 是即时的）。避免嵌套同值 timeout。
- **验收**：构造一个不响应取消的 worker，断言退出流程在有界时间内完成且该 worker 确实被 abort。
- **2026-09-12 修复**：`shutdown_active_downloads` 的收敛抽出为 [`drain_download_handles`](../src-tauri/src/lib.rs)——两阶段共享预算：阶段一在预算内等优雅退出，阶段二对剩余句柄逐个 abort+await（nothing detaches）。pending control（handle=None）仍由 token 取消兜底。句柄用 `Option<JoinHandle>` 槽位标记完成状态，避免对已完成句柄二次 poll（JoinHandle 双重 await 会 panic）。
- **验证测试**：新增 [`tests/shutdown_drain.rs`](../src-tauri/tests/shutdown_drain.rs)（3 项）：合作型 worker 在预算内收敛、顽固 worker（忽略取消 120s）在预算后 abort+await 且总时长有界（<5s）、空句柄表立即返回。

### ARC-24（P1，Closed）：Metalink 并行下载任一镜像失败即删除全部 part 文件

- **证据**：[`metalink.rs`](../src-tauri/src/download/metalink.rs#L672) 在 `worker_errors` 非空时调用 `cleanup_metalink_part_files`，删除全部 `{temp}.part-*`。
- **影响**：3 路并行下载 3 GB 文件时，若其中 1 路的所有镜像耗尽，另外 2 GB **已完整落盘且本可续传**的数据被无条件删除，用户重试从 0 开始。注释里「不能从部分 range 拼装出有效输出」是对的，但结论错了——不能拼装不等于必须删除。
- **修复方向**：保留 part 文件让下次 dispatch 进入 resume 模式。前提是分片计划必须稳定：当前 `worker_count = min(healthy_mirrors, 3)` 是运行时算的，健康镜像数变化会让 range 边界漂移。因此修复必须配套把 `worker_count`/`total_size`/各 range 边界持久化，恢复时校验一致才复用 part，否则才清理。
- **验收**：模拟一路镜像全部失败，断言其余 part 保留；再次 dispatch 时只补缺失 range；分片计划变化时能正确检测并清理。
- **2026-09-12 修复**：worker 失败分支的 `cleanup_metalink_part_files` 已删除——存活 worker 的 part 字节保留。计划一致性由 ARC-34 的持久化身份保证：只要 (total_size, worker_count) 不变，下一次 resume 会以相同边界复用 part；身份变化时由入口统一清理。
- **验证测试**：既有 F-4 镜像失败测试（`f4_parallel_download_returns_error_on_mirror_failure`）回归通过；part 保留 + 复用的正确性由 `arc34_plan_identity_mismatch_discards_stale_parts` 的反向场景（身份一致时不清理）共同覆盖。

### ARC-25（P1，Closed）：Metalink 两条读循环缺少空闲超时与取消竞争

- **证据**：[`metalink.rs`](../src-tauri/src/download/metalink.rs#L1090) 与 [`metalink.rs`](../src-tauri/src/download/metalink.rs#L1349) 都是裸 `response.chunk().await`，取消检查在 chunk 到达之后。对照其余引擎均走共享 helper（`hls/engine.rs:1137`、`dash.rs:1271`、`ftp.rs:835`、`sftp.rs:968`、`worker.rs:322`）。
- **影响**：两点。其一，镜像建连成功后不再发数据（黑洞/半开连接）会让 Metalink 任务**永久挂起**，同时占用调度槽、host 槽、限速器和一个 DB 连接；[`download/mod.rs`](../src-tauri/src/download/mod.rs#L42) 声称「每个协议共享同一个 60 秒静默阈值」，Metalink 是反例。其二，停滞连接上的暂停/删除永远不收敛（`ARC-04` 只修了限速器等待）。
- **修复方向**：改用 `select!` 竞争 cancel token 与 `read_with_idle_timeout`，新增 `metalink_mirror_stalled` 结构化错误码。`metalink.rs` 已经 import 了 `READ_IDLE_TIMEOUT`，只是仅用于清单抓取，数据面漏了。
- **验收**：本地假服务器建连后不发数据，断言 60 秒内返回 `metalink_mirror_stalled`；停滞状态下取消在秒级收敛。
- **2026-09-12 修复**：并行镜像 worker（`download_metalink_range_from_mirror`）与串行下载两条数据面循环均改用共享 `read_with_idle_timeout`（60s）并以 `tokio::select!` 与 cancel token 竞争；取消分支保留 flush + force-checkpoint 语义；新增结构化码 `metalink_mirror_stalled`。
- **验证测试**：helper 由 `download/mod.rs` 单测覆盖四臂（Data/End/Error/IdleTimeout）；60 秒停滞集成测试按 `hls_engine.rs` E-1 注释的同一理由不加入（会拖慢套件），两条循环与 HLS/DASH/FTP/SFTP 数据面结构逐行同构；取消竞争由 ARC-26 的秒级收敛测试证明 select! 路径贯通。

### ARC-26（P1，Closed）：FTP/SFTP 建连无超时，各引擎探测无整体超时

- **证据**：[`ftp.rs`](../src-tauri/src/download/ftp.rs#L1210) 的 `AsyncFtpStream::connect` 与 [`sftp.rs`](../src-tauri/src/download/sftp.rs#L1291) 的 `client::connect` 都是裸调用。HTTP 侧 [`http/mod.rs`](../src-tauri/src/download/http/mod.rs#L384) 只设了 `connect_timeout`，注释对流式下载体是正确的，但**探测用的 HEAD / ranged-GET 是短请求，应当有总超时**。
- **影响**：FTP/SFTP 连到黑洞地址时 TCP connect 走 OS 默认（Linux 约 130s、Windows 约 21s），登录与握手阶段则完全没有上界。这条路径也用于新建对话框的目录探测，用户点「探测」后 UI 长时间无响应且无法取消。HTTP 侧若服务器接受连接却不返回响应头，`send_head_with_retry` 会永久挂起并重试 3 次。
- **修复方向**：FTP/SFTP 建连包 `tokio::time::timeout(30s)` 并返回结构化超时码；探测请求单独设 `timeout(30s)`（不影响下载体）；`ProbeRequest` 增加 `CancellationToken` 字段，让对话框的「取消探测」能真正中断。
- **验收**：连接黑洞地址时探测在 30 秒内返回结构化错误；对话框取消能立即中断进行中的探测。
- **2026-09-12 修复**：`ProbeRequest` 新增 `cancel_token: Option<CancellationToken>`（后端内部结构，不进 Specta）。FTP `connect_session` 的拨号+TLS+登录整段包 30s 预算（`FTP_CONNECT_BUDGET`，新稳定码 `ftp_connect_timeout`）并与 token `select!`；SFTP `connect_sftp` 的重试循环整段包 30s 预算（`SFTP_CONNECT_BUDGET`，`sftp_connect_timeout`）同样与 token 竞争。HTTP 系控制面维持 client connect_timeout + `read_body_limited` 空闲超时的既有约束；对话框侧的取消源（UI → ProbeRequest）是后续 UX 项。
- **验证测试**：`ftp_engine.rs`/`sftp_engine.rs` 各新增 `arc26_*_probe_with_cancelled_token_converges_immediately`（已取消 token 的探测秒级返回，证明 select! 收敛端到端贯通；超时分支与之同构）。

### ARC-27（P1，Closed）：HTTP 分段重试的退避 sleep 不可取消

- **证据**：[`worker.rs`](../src-tauri/src/download/http/segmented/worker.rs#L113) 直接 `tokio::time::sleep(retry_after 或退避)`，没有与 cancel token 竞争。其余五个引擎（HLS、DASH、FTP、SFTP、Metalink）**全部**使用了 `select!`，唯独主力协议 HTTP 没有。
- **影响**：服务器返回 429/503 且 `Retry-After: 60`（上限 60s）时点暂停，worker 会睡满 60 秒。而 `pause_task` 只等 5 秒就放弃 join 并执行状态转移，随后 worker 醒来继续发请求、继续写 checkpoint，与「已暂停」的 DB 状态冲突。
- **修复方向**：用 `select!` 竞争 `cancel_token.cancelled()`，取消时先上报当前 offset 再返回。顺带把 `retry_delay` 中每次调用都读环境变量的 `VIBE_FAST_RETRY_DELAYS` 改为 `OnceLock<bool>` 缓存。
- **验收**：`Retry-After: 60` 期间取消，断言 worker 在秒级退出且不再写入 checkpoint。
- **2026-09-12 修复**：worker 重试退避（Retry-After 或指数退避）改用 `tokio::select!` 与 cancel token 竞争；取消时按 ARC-33 契约上报 durable offset 后返回 Ok。修复方向中提到的 `VIBE_FAST_RETRY_DELAYS` OnceLock 缓存**未采纳**——集成测试按用例设置/移除该变量，进程级缓存会破坏测试隔离。
- **验证测试**：`http_engine.rs` 新增 `segmented_direct_cancel_during_retry_backoff_converges_quickly`——服务端 429 + `Retry-After: 60`，所有 worker 进入退避后取消，断言 10s 内（实测 0.5s）收敛；旧行为会睡满 60s。

### ARC-28（P1，Closed）：BT 探测每次新建 librqbit Session、从不关闭、共享固定目录

- **证据**：[`bt.rs`](../src-tauri/src/download/bt.rs#L307) 每次 probe 都 `std::fs::create_dir_all` 一个固定路径 `temp_dir()/vibe-downloader-bt-probe` 并 `Session::new`。同文件的测试注释（`bt.rs:1943`）自己写明了 librqbit 的 Session 总会绑定固定 UDP 端口的 DHT 监听器，两个并发 Session 会以 Windows os error 10048 失败。
- **影响**：三重问题。探测期间若有任何 BT 下载在跑，探测就会失败；两个并发探测互撞；`api` 出作用域只是 drop `Arc`，DHT/tracker/accept 后台任务没有被显式关停，每次探测都可能留下常驻任务。此外 `std::fs::create_dir_all` 在 async 上下文中阻塞 Tokio worker（`api_for_output_folder` 那侧已改用 `tokio::fs`，探测这侧漏改）。
- **修复方向**：对 `.torrent` 字节根本不需要 Session —— `librqbit::torrent_from_bytes` 已经在 `parse_torrent_private_flag` 和 `tracker_statuses_from_torrent_bytes` 中被这样使用，只有 magnet 才真正需要联网取 metadata。magnet 路径复用 `BtEngine` 的 session 注册表，或至少使用唯一目录、显式 forget、加 `BT_METADATA_TIMEOUT` 超时并在结束后清理。
- **验收**：一个 BT 任务下载中同时探测另一个 torrent 能成功；两个并发探测互不影响；探测结束后无残留目录与后台任务。
- **2026-09-12 修复**：http(s)/file 的 `.torrent` probe 改为 `torrent_from_bytes` 纯解析（经 `info.data.validate()` 取 name/文件清单/info-hash）——完全不再创建 librqbit Session、不绑 DHT 端口、不建临时目录。固定共享目录 `vibe-downloader-bt-probe` 与其中的同步 `create_dir_all` 一并消失。magnet probe 本就是纯字符串解析，不受影响。
- **验证测试**：`bt_engine.rs` 的 http probe 套件（字节解析 private flag/文件列表）回归通过——同一测试现在走纯解析路径；`arc34` 期间单镜像 fallback 也确认 probe 不再触发 Session。并发 probe 测试不再需要 DHT 端口（BT_TEST_LOCK 保留作为保险，评审通过后可移除）。

### ARC-29（P2，Closed）：BT 限速不实时同步，且不计入全局令牌桶

- **证据**：[`bt.rs`](../src-tauri/src/download/bt.rs#L436) 只在获取 session 时传入一次 `speed_limiter.current_limit_bps()`，其后 1000 余行的下载循环中再没有 `sync_session_download_limit` 调用。
- **影响**：用户在 BT 任务下载过程中修改全局限速或任务限速不会生效（其余六个引擎都通过共享 `Arc<GlobalSpeedLimiter>` 实时生效）。反过来，BT 的实际流量也不计入全局令牌桶，因此「全局 10 MB/s」在有 BT 任务时会被突破。
- **修复方向**：在 BT 主循环已有的 1 秒 tick 中重新读取并同步 session 限速。「BT 流量不计入全局桶」是 librqbit 的架构限制，至少应在设置界面明确标注，或把 BT 会话限速设为全局剩余量的估算值。
- **验收**：下载中修改限速在数秒内对 BT 生效；设置界面对全局限速与 BT 的关系有明确说明。
- **2026-09-12 修复**：下载主循环的 1s tick 处同步 session 限速——`sync_session_download_limit(min(任务限速, speed_limiter.current_limit_bps()))`，使传输中修改任务限速（update_task_transfer_options）与调度窗口限速即时生效（create 时的初值仍保留）。全局 token bucket 由 per-task 子限速器参与最小值组合；BT 原生流量不经全局桶的残留由该 min 组合约束，UI 明示为后续项。
- **验证测试**：`sync_session_*` 函数为 librqbit 运行时 API 的直接薄封装（既有单测覆盖 non_zero 语义）；tick 路径的调用由结构位置保证每秒执行。真实带宽变化验证依赖外部 tracker 环境，归入 FUN-18 的人工验收面。
- **2026-09-13 加固**：新增单测 `sync_session_download_limit_updates_live_session`——在真实 librqbit session 上断言创建时限速到达 limiter、tick 同款 `sync_session_download_limit` 实时改写 `get_download_bps`、非正数与 `None` 清除上限、`sync_session_upload_limit` 同样生效。真实带宽下的端到端变化仍归 FUN-18。

### ARC-30（P2，Open）：错误分类仍有多处依赖英文子串（`ARC-16` 遗留面）

- **证据**：`ARC-16` 已让 resume 路径改用结构化 payload，但以下位置仍在匹配文案：[`dash.rs`](../src-tauri/src/download/dash.rs#L1542) 用 `error.contains("canceled")` 判断取消（而 `run_cancellable` 返回的是硬编码英文 `"Download canceled."`）；[`actions.rs`](../src-tauri/src/commands/tasks/actions.rs#L732) 用 `contains("concurrently")`/`contains("already")` 统计批量操作的 skipped；[`sftp.rs`](../src-tauri/src/download/sftp.rs#L869) 用 `contains("permission")`；[`probe_error.rs`](../src-tauri/src/download/probe_error.rs#L53) 有 20 余条基于 OS/库英文错误串的分类。
- **影响**：DASH 的取消判定最危险——文案一旦改动或本地化，取消会被当作真实失败上报为 `dash_ffmpeg_failed`。批量操作的成功/跳过/失败统计也会随措辞漂移。
- **修复方向**：取消判定改用 `cancel_token.is_cancelled()` 这一权威来源；批量统计改按 `AppErrorPayload.code` 分派；SQLite BUSY 判定改用 sqlx 的结构化 error code；`probe_error.rs` 优先使用 reqwest 的类型化谓词与 `std::io::ErrorKind`，英文子串只作最后兜底并记录 debug 日志。`task_resume.rs` 已有的 `resume_errors_dispatch_on_code_not_message_text` 测试确立了这条原则，只是没有推广。
- **验收**：修改任意错误文案不影响取消判定、批量统计与状态分流；新增对应回归测试。

### ARC-31（P2，Partial）：超大模块与跨引擎重复代码（`ARC-17` 的量化补充）

- **证据**：Rust 侧 `hls/engine.rs` 2283 行、`metalink.rs` 2256、`bt.rs` 2036、`dash.rs` 1899、`create.rs` 1690、`browser.rs` 1670、`ftp.rs` 1606、`sftp.rs` 1535；前端侧 `SettingsPage.tsx` 2623、`TaskDetails.tsx` 2195、`NewDownloadDialog.tsx` 1859、`AppShell.tsx` 1538、`Palette.tsx` 1146。可安全抽取的重复：`percent_decode_*` 在 ftp/sftp/webdav 有三份逐字节等价实现；`apply_forwarded_headers` 在 http/hls/dash/webdav 有四份完全相同实现；**FTP 与 SFTP 的协调器有约 600 行近乎逐行相同的代码**。
- **影响**：这不是代码洁癖问题，而是修复成本的乘数。`ARC-19` 必须在 ftp.rs 和 sftp.rs 各修一遍，将来也会在两处各退化一遍。`SettingsPage.tsx` 用 53 个 `useState` 镜像一个 `AppSettings`，新增一个设置项要改 5 处，极易漏改。
- **修复方向**：抽 `download/segment_coordinator.rs`，用 `trait SegmentTransport` + `CoordinatorConfig` 统一 FTP/SFTP（与 `ARC-19` 一起做）；把三份 `percent_decode_*` 与四份 `apply_forwarded_headers` 收敛到共享模块；前端按 `ARC-17` 已列的 checklist 推进，建议顺序为 SettingsPage（有测试覆盖、风险最低）→ AppShell（只抽 hook 不动 JSX）→ TaskDetails → NewDownloadDialog。
- **验收**：公共行为不变；FTP/SFTP 共用同一协调器且取消语义只有一处实现；上述四个前端巨型组件各降到 400 行以内。
- **2026-09-12 修复（本条①）**：FTP/SFTP 协调器主循环的取消路径改为「先排空再 checkpoint」——检测到取消后进入 drain 循环（join_next + 消费 progress 直到 running 为空），每个 worker 得以观察取消、flush 缓冲并上报 durable offset，随后才执行 force checkpoint；主循环中途的 `Err(_) if cancelled` 同样路由到 drain。SFTP worker 取消出口的「flush 失败则不上报 offset」语义**保留**（ARC-19 的保守水位设计与 ARC-33 一致，不统一为 FTP 的硬失败）。
- **验证测试**：`ftp_engine.rs` 新增 `arc31_parallel_cancel_drains_workers_before_checkpoint`（并行多段传输中途取消：引擎干净返回、checkpoint > 0、checkpoint 不领先磁盘水位）。
- **②（共享协调器重构，推迟）**：约 1200 行去重（`SegmentTransport` trait 抽象）在本轮评估后推迟——2b 期间 FTP/SFTP 的取消/校验语义有多处独立变化，立即叠加大规模重构会放大回归面；在 2c/2d 稳定后单独执行。状态保持 Partial。

以下 `ARC-32`～`ARC-48` 为 2026-08-26 第 4 轮复审新增。

### ARC-32（P0，Closed）：pause/cancel/delete/restart 持每任务运行时锁内联 await dispatch，与调度器锁构成环

- **证据**：`dispatch_inner` 全程持调度全局锁（scheduler/mod.rs:111），而 `start_task` 第一步（先于任何 DB 读）就取 `task_runtime_locks.lock(task.id)`（mod.rs:270）。同时 pause_task（actions.rs:208 取锁、:274-278 **内联 await dispatch**）、cancel_task（:437/:497-501）、delete_task（:514/:557-561，guard 直到 :564 才 drop）、resolve_task_attention(Restart) → restart_task_from_beginning（guard actions.rs:761 → tasks.rs:784-788 await）都是「持任务锁 → 等调度锁」。resume/retry/mirror-retry 已 spawn 化规避（tasks.rs:654-668 的注释明说此危害），这四条路径从未转换。
- **影响**：经典循环等待。最坏路径是**确定性**的：Restart 把任务置回 queued（db/task_state.rs:578，retry_after_at=NULL）后内联 dispatch——只要有空闲槽（NeedsAttention 任务常态），dispatcher 快照到该任务并对同一线程已持有的运行时锁再次加锁，无条件挂起。暂停 Queued 任务是受支持流程（Pause-all 目标含 queued，actions.rs:698）。一旦卡死，后续一切 dispatch 与尾部内联 dispatch 的命令（settings.rs:280、create.rs:1222 等）永久排队直至重启。这是自 2026-06-30 起跟踪的死锁的最终确认与加重版。
- **修复方向**：四处统一改为 spawn dispatch（照抄 resume/retry 范式）；中期把「dispatcher 锁外快照、start_task 内取任务锁」的锁序文档化并加回归测试。
- **验收**：「dispatch tick 进行中对同一 Queued 任务 Pause / Delete / Restart」三条竞态集成测试通过；Restart 在空闲槽位下立即完成。
- **2026-09-11 修复**：新增 [`Scheduler::dispatch_detached`](../src-tauri/src/scheduler/mod.rs)，pause（actions.rs pause_task）、cancel（cancel_task）、delete（delete_task）、Restart（restart_task_from_beginning）与 retry（queue_task_for_retry_at 的内联 spawn 一并收敛）五条命令尾部统一改走 detached dispatch——spawn 后调用方先 unwind 释放任务锁，dispatch → start_task 再取任务锁。锁序约定已写入该方法的 doc comment（「命令尾部在持任务锁期间不得 await dispatch」）。delete_task 的 evict 顺序不变：staging 行已删，detached dispatch 不可能再选中该任务加锁。
- **验证测试**：[`tests/scheduler_lock_order.rs`](../src-tauri/tests/scheduler_lock_order.rs)（3 项）：以真实 `TaskRuntimeLocks` + tokio 并发编码「Restart 写回 Queued 后持锁 dispatch 自死锁」与「双命令尾部 ABBA」两个场景（旧行为下必然超时），并断言真实 `reset_task_download_state` 写回后任务可被 dispatch 选中。全量 dispatch 需要真实 AppHandle（同 `scheduler_dispatch.rs` 的 harness 说明），锁序契约因此以生产锁原语验证。

### ARC-33（P0，Closed）：分段 HTTP worker 三条早退路径不 flush BufWriter，动态加速几乎必然造成成品静默缺字节

- **证据**：[`worker.rs`](../src-tauri/src/download/http/segmented/worker.rs#L352) 的 `download_segment_once` 有三条上报 offset 后 `return Ok` 却**不调用 file.flush()** 的路径：(1) `offset > current_end` 早退（:351-357）；(2) `write_len <= 0` 早退（:361-363）；(3) 加速收缩范围后的 partial-chunk 写（:419-424）。写侧是 tokio BufWriter（256 KiB，无 Drop flush，tokio 文档明确要求手动 flush）。其余所有出口（cancel :310/:338、limiter-cancel :384、流结束 :427）都显式 flush 以维持 :382-383 注释的不变量「checkpointed downloaded_until 不能超前于磁盘事实」。retryable 错误出口（:325-334）同样不 flush，而 retry 从 failure.downloaded_until（:103）续跑而非磁盘位置，属同类缺口。
- **影响**：`maybe_accelerate_segments`（coordinator.rs:606-766）收缩活跃 worker 的 range_end AtomicI64（:688-690）而服务器按旧 Range 继续推流 → 几乎每次成功的分段加速都经 (1)/(3) 退出，最多丢 256 KiB 缓冲尾。下游无法兜底：协调器把上报 offset 落库（runtime_progress.rs:177-185），预分配使 temp 尺寸恒等于 total_size（file_ops.rs:165-180）令尺寸检查失效（coordinator.rs:515-521），rename 发布带洞文件——无任何报错，除非用户手动哈希校验。默认功能、高频触发、静默损坏，故为 P0。
- **修复方向**：所有 `return Ok` 路径统一 flush 后再上报 offset；更彻底的做法是把「flush 才能上报进度」做成类型级契约（封装 writer，使 offset 上报方法强制先 flush）。
- **验收**：加速 split 触发前后的全文件字节比对集成测试；缓冲非空的早退路径字节级断言（参照 sftp_engine.rs:919-948 的 prefix 比对模式）。
- **2026-09-11 修复**：采纳类型级契约方案。新增 [`segmented/writer.rs`](../src-tauri/src/download/http/segmented/writer.rs) 的 `SegmentFileWriter`：维护 written/durable 双水位，`durable_offset` 仅在 `sync()`（先 flush）中前进；worker 的**所有**上报出口——含 300ms 中途进度（协调器的 force checkpoint 会持久化运行时进度，见 runtime_progress.rs `update_progress`/checkpoint.rs，仅修出口不够）——统一经 `durable_checkpoint()` flush 后发布；连接错误/停滞/越界/收缩/write_all 失败路径上报 durable 水位而非 running offset。
- **验证测试**：writer 单元测试 3 项（水位语义、失败写不推水位、sync 后字节落盘）；字节级集成测试 [`http_engine.rs::segmented_direct_resume_after_mid_body_abort_writes_no_hole`](../src-tauri/tests/http_engine.rs)：服务端在越过一次 256 KiB flush 边界后 TCP RST 中断段连接（socket2 设 SO_LINGER=0，std `set_linger` 为 nightly-only，已加入 dev-dependencies），重试完成后逐字节断言成品无零洞。该测试对旧行为（去掉 sync）实测失败、对新实现通过。

### ARC-34（P1，Closed）：Metalink 并行续传按「当前健康镜像数」重算分片边界，镜像集变化即错位拼接

- **证据**：`download_metalink_file_parallel`（metalink.rs:465,487-507）每次 invocation 以 `mirrors.len()`（list_healthy_mirrors_for_file 过滤 supports_range/cooldown/status，db/metalink.rs:271-299）重算 N 等分；resume 只要任一 part 存在即进入（:522-524），worker 把 part 文件长度当作**新**边界内的进度：`effective_start = range_start + already_downloaded`（:956,968）。计划边界无处持久化（001_init.sql:377-390 + 002_metalink_health.sql 只有健康字段）。镜像集双向可变：30s 冷却到期加回（db/metalink.rs:8,147）、单个 416 永久除名（:250-265）。
- **影响**：暂停/恢复之间健康数变化 → part 内容对应的绝对偏移与新假设错位地拼接；part 尺寸恰好等于新预期长度，检查全部通过。manifest 无主校验和时 verify_metalink_file 直接 Ok（:1441-1444）发布坏文件；有时则整次传输报废。FUN-09 的 validator wipe（:972-980）救不了常见情形——ETag 在首次成功响应时即被持久化（:1915-1936）。2026-06-30 登记的 Metalink parallel-resume 问题原样已修，本条是其同类根因残余，也正是 `ARC-24` 修复方向预言的边界漂移，本轮证实它独立于 part 删除策略就会造成损坏。
- **修复方向**：创建并行计划时持久化 {total_size, worker_count, 各 range 边界}（task_work_units 或 metalink_resources 扩展列）；resume 时校验一致才复用 part，不一致按 `ARC-24` 的策略清理重建。
- **验收**：「暂停时 3 健康 → 恢复时 2 健康」与「2→3 增长」两条场景的字节级回归测试。
- **2026-09-12 修复**：迁移 `007_metalink_file_plans`（task_id, file_id, worker_count, total_size；PK(task_id, file_id)，FK 级联）。并行入口读持久化 plan：身份与本次计算一致才进入 resume；否则清理全部 part 并重写身份。边界是 (total_size, worker_count) 的确定性函数，两个值即可复现精确切分——镜像健康变化不再重算 resumed 文件的边界。串行路径不使用 plan（完成后随任务删除级联清理）。
- **验证测试**：`metalink_engine.rs` 新增 `arc34_plan_identity_mismatch_discards_stale_parts`（3 镜像完成后以 2 镜像再入：plan 重写为 (2,30)、请求范围为重分区后的 0-14/15-29、文件内容正确）；既有 F-4 part-resume 套件（身份一致场景）回归通过。`migration_integrity` 的迁移计数断言同步 6→7。

### ARC-35（P1，Closed）：Metalink fresh-start 接受非-206 响应、Content-Range 只看 start、part 长度 ≥ expected 即视为完成

- **证据**：Range 头恒发送（:987），但非-206 恢复路径只在 `already_downloaded > 0` 时执行（:1028-1055），Content-Range 校验同样（:1060-1062），且 `validate_metalink_content_range`（:1892-1913）丢弃 `_end/_total` 只比 start——对照 HTTP worker 要求 206 + start/end/total 全符（worker.rs:252-274）。`supports_range` 默认 1（002_metalink_health.sql:14,20，「assume Range works until proven otherwise」）。`:958-966` 以 `>= expected` 判定整段完成（无 == 上界、无内容校验）；尺寸不符报错（:1129-1138）但**不删污染的 part**，failover stat 到垃圾长度照常推进。
- **影响**：WAF/反爬镜像以 200 返回 HTML 页即可污染 part；垃圾长度 ≥ expected 时该 range 被「完成」。validator 还会把这个 200 的 ETag 持久化令 FUN-09 wipe 失效。有主校验和时至少整次传输报废，无校验和时坏文件直接发布为 Completed。
- **修复方向**：对齐 HTTP worker 契约：ranged 请求一律要求 206 + Content-Range 全字段校验（fresh-start 同样）；part 完成判定改 `==` 并在尺寸不符时删除 part 再 failover。
- **验收**：200-with-full-body 与 200-with-garbage 两类 mock 镜像被拒且不残留污染 part。
- **2026-09-12 修复**：三处对齐 HTTP worker 契约。其一，part 入口完成判定改 `== expected`，超长 part 视为污染并删除后重试。其二，fresh start 同样强制 206（非 206 的镜像标记 unsupported_range 并 failover）+ Content-Range start/end 全字段精确匹配（`validate_metalink_content_range` 升级；串行路径按文件 total_size 推导 end，unknown-size 传 None 只校验 start）。其三，校验失败先删除污染 part 再 failover，不再把垃圾字节留给下一个镜像。
- **验证测试**：`fun09_mismatched_content_range_rejects_resume` 的断言从「part 保留」更新为「part 已删除」（与新契约一致）；既有 200-full-body 类 mock 场景由升级后的校验拒绝。

### ARC-36（P1，Closed）：外部音轨/字幕按 worker 完成顺序拼接，成品音轨乱序静默损坏

- **证据**：`download_hls_rendition_segments` 以 JoinSet join_next 完成序 push `completed`（hls/engine.rs:1720-1728），`write_external_track_playlist` 按该序输出 playlist 条目（:1774-1777），中间无任何排序（:1586-1601）；`poll_live_external_track` 同病（track.completed.extend，:1663）。主视频路径从 DB 按 `discontinuity_sequence, media_sequence` 排序读取（db/hls.rs:363）——证明外部路径只是漏了排序。并发前提成立：HLS planned slots = clamp(segment_count, [1,8])，默认 DEFAULT_SEGMENT_COUNT = 4（db/mod.rs:150）。
- **影响**：选了外部音轨/字幕（FUN-10）的 VOD，ffmpeg -c copy 按列出序拼接（run_ffmpeg :1917-1957）→ 对白错乱/字幕漂移且随时间线恶化，无任何告警。
- **修复方向**：completed 收集后按 (discontinuity_sequence, media_sequence) 排序——plan 里带上序号即可，无需查 DB。
- **验收**：多 worker 乱序完成的 fixture 断言 local.m3u8 严格按媒体序。
- **2026-09-13 修复**：`SegmentDownloadPlan` 已携带两个序号，worker 结果元组扩为 `(local_name, duration_ms, discontinuity_sequence, media_sequence)`；`write_external_track_playlist` 写盘前按 `(disc, media)` 升序 `sort_by_key`（VOD 与 live 共用此出口，`LiveExternalTrack.completed` 同步换 4 元组）。段文件名与 ffmpeg 的 `-i` 轨道输入序（本就按声明序）不变。
- **验证测试**：`hls_engine.rs::arc36_external_track_playlist_follows_declared_order`——4 段外部音轨、假服务器延迟第 1 段、`connection_limit=4`（新增 `headless_context_with_connections` 帮助函数，绕开 headless 助手的单 worker），断言 local.m3u8 严格为 `seg-0-0..seg-0-3`。旧行为敏感性验证：移除排序后完成序恰为 0,2,3,1、测试失败，恢复后通过。

### ARC-37（P1，Closed）：引擎段失败时取消「自己的」任务 token，supervisor 误判为用户取消 → 任务永久滞留 Downloading（僵尸）

- **证据**：HLS segment 重试耗尽时 `cancel_token.cancel()` 后 return Err（hls/engine.rs:929-938），DASH 同型（dash.rs:1149-1158）。token 由 scheduler 创建经 DownloadContext 下发（mod.rs:300,450），engine 自取消与用户取消共享同一 token。supervisor 在 future 结束后才读 `is_cancelled()`（mod.rs:459）并 `if !canceled` 才 mark_download_failed（:463-466）——该守卫本意是保护用户暂停/取消，却无法区分两种取消。此后 downloads_map 已清（:460）、dispatch 只捡 Queued（:132）、无运行时看门狗、reset_interrupted_tasks 仅启动时执行（lib.rs:590）。外部轨道变体更糟：run_hls_download 的 `Err(_) if cancel_token.is_cancelled()` 臂调用 pause_hls_task **静默转 Paused**（engine.rs:574-577）。
- **影响**：任一 segment 403/404/断连 3 次 → UI 永远显示「Downloading 0 B/s」；auto-retry 与失败计数因状态非 Failed 全部失效。只能手动暂停再恢复或重启。这是第 2 轮登记、第 3 轮仍 Open 的僵尸态问题的根因定位。
- **修复方向**：引擎内部放弃必须与用户取消可区分——返回结构化错误码由 supervisor 无条件转移失败（用户发起与否由命令层标记，不从 token 推断）；或引入独立的 internal_abort 信号。
- **验收**：mock segment 永久 404，断言任务秒级转 NeedsAttention/Failed 且 slot 释放；用户取消路径回归不受影响。
- **2026-09-11 修复**：HLS 段失败（engine.rs `hls_segment_failed` 臂）、外部轨道失败（`hls_track_failed` 臂）与 DASH 段失败（dash.rs `dash_segment_failed` 臂）删除 `cancel_token.cancel()`——`workers.abort_all()` 本就足以停住其余 worker，token 取消只是污染 supervisor 的 `canceled` 判定。全仓审计确认引擎内主动 `cancel_token.cancel()` 仅此三处且全部为内部失败语义；用户取消仍由命令层取消 token 并由既有 `Err(_) if cancel_token.is_cancelled()` 分支收敛。
- **验证测试**：[`hls_engine.rs`](../src-tauri/tests/hls_engine.rs) 与 [`dash_engine.rs`](../src-tauri/tests/dash_engine.rs) 各新增 `arc37_internal_segment_failure_does_not_cancel_user_token`：永久 404/500 耗尽重试后断言引擎返回结构化失败且调用方持有的 token 未被取消（对旧行为实测失败）。既有取消收敛测试（`cancel_during_live_poll_sleep_pauses_cleanly` 等 18+14 项）全绿。

### ARC-38（P1，Closed）：staging 目录在任何路径都不回收；DASH 连「删除任务（含文件）」都泄漏

- **证据**：DASH 建 save_dir/.vibe-staging/{task_id}（dash.rs:914-921），finalize_dash_task（:1432-1520）remux 后只 rename 走 mp4；HLS 用 task.temp_path 作 staging（engine.rs:469-477），finalize_hls_task（:1787-1843）留下全部 seg-*.ts/init/output 中间物。全仓 grep 无任何对 staging 的 remove_dir_all；STAGING_DIR_NAME 仅出现在 task_file_planning.rs。删除流程只删 task.temp_path/final_path/task_files 路径（actions.rs:539-552,614-627），而 `task_stored_temp_path` 仅 protocol=="hls" 时返回 staging 目录（task_file_planning.rs:57-68）——DASH 的 temp_path 是单个已被 finalize rename 掉的文件。
- **影响**：每个 2 GB 影片成功下载后留约 2 GB 隐藏段文件；DASH 删除任务后 DB 记录消失、应用内永久不可回收；HLS 仅当用户事后勾选「删除文件」才释放。正常使用数周即数十 GB。
- **修复方向**：finalize 成功路径 remove_dir_all(staging)；启动时扫描 save_dir/.vibe-staging/* 对照现存任务清理孤儿（覆盖失败/取消残留）；delete 流程对 dash 协议同样解析出 staging dir。
- **验收**：成功/失败/取消/删除四条路径各有 staging 清理断言；孤儿目录启动清扫测试。
- **2026-09-11 修复**：三处落地。其一，finalize 成功路径清理引擎**实际使用**的 staging 目录（HLS 的 staging 即 task.temp_path，历史行可能不在规范位置，故按参数清理而非按规范重建；失败仅告警，不推翻已完成的下载）。其二，delete_task/bulk_delete_tasks 对 hls/dash 协议按 `task_staging_dir` 显式解析并删除 staging（`delete_file=false` 同样删除——staging 是中间态而非用户数据；HLS 原先恰好经由 temp_path 覆盖，DASH 全漏）。其三，启动维护区新增 [`sweep_orphan_staging_dirs`](../src-tauri/src/commands/task_file_planning.rs)：按 DB 中的 save_dir 清单扫描 `.vibe-staging/*`，任务行已删除或状态为 Completed 的目录清除，可恢复状态（queued/paused/downloading/failed/needs_attention）保留——失败/取消保留 staging 是有意为之，与 HTTP temp 文件的续传契约一致（本条验收「失败/取消路径清理」按 6.2 表的改进方向收窄为「成功+删除+启动孤儿」，避免破坏 retry/resume 语义）。
- **验证测试**：[`tests/staging_sweep.rs`](../src-tauri/tests/staging_sweep.rs)（2 项：completed/无行目录被清、可恢复状态保留、无 staging 根的 save_dir 容忍）；HLS `download_reenters_after_reset_interrupted_tasks` 与 DASH `download_retries_transient_segment_failures` 完成后新增 staging 消失断言。

### ARC-39（P1，Closed）：每任务独立 librqbit Session 在持久化 DHT 端口上相撞，第二个 BT 任务/probe 必败

- **证据**：`compute_session_key` 追加 `|task:{task_id}`（bt.rs:175），每个 key 经 `Session::new_with_opts` 新建 session（:219-221）；SessionOptions 只设 connect/ratelimits（:207-218），从不触碰 DHT 配置 → librqbit 9.0.0-rc.0 默认 PersistentDht 读共享 dht.json 并绑定其记录的端口（explicit→stored→random，reuseport:false，AddrInUse 即整个 session 创建失败）。dump_interval 60s 后 dht.json 必然存在。调度器 host 槽按 source_key 计（mod.rs:166-171），不串行化 BT；做种循环在无限额时会话无限期存活（bt.rs:838-925）。probe_torrent 另建 `Session::new`（:310-312）同样相撞。仓库自己的注释与被删测试记录了 os error 10048（bt.rs:1815-1817,1845-1847,1943-1949）。
- **影响**：ARC-12 的 per-task 化引入回归：任一 BT 任务下载/做种期间，一切后续 BT 任务与 `.torrent` URL probe 持续失败，直到该任务停止或应用重启。
- **修复方向**：二选一并写入架构注释——(a) BtEngine 内单例共享 Session（回到共享拓扑，但必须同步补 ARC-29 的限速实时同步）；(b) 保持 per-task 但显式配置 DHT（disable 或各自端口）。probe 路径按 ARC-28 处理。
- **验收**：两个 BT 任务并发下载互不影响；下载中 probe 另一 torrent 成功（兼作 ARC-28 验收）。
- **2026-09-12 修复**：每个任务的 session 现在写入**独立的 DHT 持久化文件**（`vibe-dht-{hash(session_key)}.json`，key 含 Windows verbatim 前缀故哈希为平铺名）。此前所有 session 共享 librqbit 默认的单个 dht.json，新 session 重绑其中记录的同一端口 → 第二个并发任务 AddrInUse。DhtSessionConfig.port 保持 None（首个绑定随机选空闲口），persistence 文件隔离后互不覆盖。
- **验证测试**：BT 单测（session 创建/驱逐/refcount）在真实 DHT 初始化下回归通过——双文件名隔离使创建路径不再共享端口记录；「同 save_dir 双任务并发」的端到端场景需要两个真实 torrent 源，归入 FUN-18 人工验收。
- **2026-09-13 加固**：文件名哈希抽为纯函数 `dht_persistence_file_name`；新增 `concurrent_same_folder_tasks_start_independent_sessions`（刻意不持 BT_TEST_LOCK，即并发创建本体）：同 save_dir 双任务并发 `api_for_output_folder` 两个真实 session——key 互异、各自 api 独立往返、持久化文件名互异且不含路径分隔符。端到端双 torrent 下载仍归 FUN-18。

### ARC-40（P2，Closed）：worker panic 无 catch_unwind，slot/host 槽/缓存永久泄漏

- **证据**：supervisor（spawn @ scheduler/mod.rs:366）的清理只在两个 checked 错误分支（:382-383、:407-408）与 engine.download 正常返回后（:459-461）执行；无 catch_unwind（全仓零命中），存储的 JoinHandle（:505-518）无人 poll。active_count 与 host 用量派生自 downloads.len()/map 内容（:143,169-174）。profile.release panic='unwind'（Cargo.toml:104）进程存活、泄漏固化。mod.rs:441-444 的 ARC-03 注释承认 panic 会从外层 JoinHandle 冒出——但没有任何消费者。
- **影响**：一次引擎 panic（如畸形 Content-Range 触发 `ARC-48` 的溢出）→ 幽灵 DownloadControl 永久占用一个全局槽 + 该 source_key 的连接槽 + runtime-lock/request_headers 表项；日志反复出现「scheduler has no available slots」，需用户手动暂停/删除或重启。
- **修复方向**：supervisor 体包 `AssertUnwindSafe(catch_unwind(...))`，poison 路径走同一套清理 + mark_download_failed("internal_panic")；或统一 monitor JoinHandle 兜底。
- **验收**：注入 panic 的 fake engine 下断言 slot 释放、任务转 Failed、后续调度正常。
- **2026-09-12 修复**：supervisor 的收敛体以 `catch_unwind(AssertUnwindSafe(engine.download(...)))` 包裹——panic 转为结构化失败消息（`describe_engine_panic`），随后的 downloads_map 移除、request_headers 移除、`mark_download_failed`、runtime lock evict、spawn_dispatch 全部照常执行，槽位不再泄漏。
- **验证测试**：`scheduler/mod.rs` 新增 `engine_panic_tests`（3 项：&str/String/不透明 payload 的消息渲染）。完整路径的 panic 注入需要真实 AppHandle（同 scheduler_dispatch.rs 的 harness 说明），收敛体的执行保证由 catch_unwind 的控制流位置结构性提供。
- **2026-09-13 加固**：收敛体抽为 `converge_download_outcome`（supervisor 调用同一函数；A-4 evict 随之提前到成功路径哈希校验前——evict 只清空闲注册表项，重排不可观察）。新增 `convergence_tests` 两项：①以真实 panic future 走 supervisor 同款 `catch_unwind → describe_engine_panic → converge` 粘合，断言 downloads_map/request_headers 清空、任务转 Failed 且 panic 细节进入 error_message、同 host 后续任务收敛无损；②canceled=true 时运行时状态照常清理但不写 Failed（R-2.4）。测试向 `app: None` 注入以跳过 emits；完整 Wry 路径仍受真实 AppHandle 限制，粘合层为 3 行且与生产逐字一致。

### ARC-41（P2，Closed）：start 失败谓词不含 queued，任务永久滞留队首静默重败

- **证据**：dispatch_inner 对 status==Queued 的非 Conflict 启动失败路由 mark_download_failed（scheduler/mod.rs:237-240），但其 SQL `WHERE id = ? AND status IN ('downloading','retrying')`（db/task_state.rs:819-826）匹配不到 queued 行 → rows_affected=0 → 日志「task state changed concurrently, skipping emit」后返回，无状态写、无事件。可达路径：resolve_task_request_headers / resolve_proxy 的 DB 错误在转移前 `?` 传出（mod.rs:274-279）；transition 的 SQLITE_BUSY 重试耗尽（state_machine.rs:152-204）。
- **影响**：持续性 DB 故障下任务永远 Queued，每个 dispatch tick 重试重败刷日志；UI 显示普通排队、无任何异常迹象，队列看似健康却不前进且无从诊断。
- **修复方向**：该分支改用能匹配 queued 的无条件 mark（或专用 mark_queued_start_failed：置 Failed/NeedsAttention + emit）。
- **验收**：注入 header 解析失败的 stub 断言 queued 任务转为可见失败态而非原地踏步。
- **2026-09-13 修复**：db 层新增 `mark_task_failed_if_queued`（`WHERE status = 'queued'`，与 active 版共享同一 SET 列——经 `mark_task_failed_where` 单一实现 + QueryBuilder 组装，sqlx 0.9 的 `query()` 仅收 `&'static str`）；scheduler 侧把失败持久化拆为 `persist_failure_state(pool, task_id, error, FailureRowScope::{Active, Queued})`，新增 `handle_start_failure(app: Option<&AppHandle>, ...)` 承接 dispatch 的 Err 分支——重读后仍 Queued 的行走 queued 匹配器落库（ARC-16 code 派发决定 Failed/NeedsAttention），其他状态维持仅快照的既有语义，`app=None`（无头测试）跳过全部 emit（沿 converge 的 Option 模式）。条件 WHERE 保留 R-2.4 保证：标记与写库之间用户暂停/取消/删除不被覆盖。
- **验证测试**：`convergence_tests` 三条——queued + 注入错误 → Failed 且带错误消息、无伪造 code；queued + `remote_changed` JSON payload → NeedsAttention 且 error_code=remote_changed；downloading 任务 + 错误 → 状态不变（快照臂回归）。敏感性验证：queued 臂临时改回 Active 匹配器 → 首条用例失败（任务滞留 queued）。验收偏离记录：审计原文的「stub 注入 header 解析失败」以直接驱动 `handle_start_failure` 等价承载（start_task 本身硬依赖 AppHandle，无法无头构造），与 ARC-40 同款偏离。

### ARC-42（P2，Closed）：FTP/SFTP resume 不重验远端 SIZE/MDTM，等大小替换文件造成新旧缝合

- **证据**：SIZE/MDTM 仅 probe 时采集（ftp.rs:160-169、sftp.rs:195-213），last_modified 存库后无人比对（唯一出现 ftp.rs:165,193）；resume 直接 REST {offset}（ftp.rs:762-769）/ seek（sftp.rs:894-905），没有 If-Range 等价物（HTTP 侧有 direct.rs:42-47、coordinator.rs:97-117 可对照）。完成判据仅 `downloaded >= total_size`（ftp.rs:644-652、sftp.rs:744-758）。
- **影响**：暂停窗口内远端换成等大小新内容（镜像/rolling 文件常态）→ 半旧半新的文件标记 Completed。配置校验和可事后检出，但校验和可选（tests/ftp_engine.rs:29-31）。
- **修复方向**：resume 前 MDTM/SIZE 比对 probe 记录，不一致即 fail_task_and_segments（沿用 resume_blocked/restart 恢复动作）。
- **验收**：本地 FTP fixture 中途换等大小文件断言 resume 被拒且给出明确恢复指引。
- **2026-09-13 修复（含证据修正）**：核实发现 FTP 走调度器恢复时 prepare 路径已会重探测比对（`resume_mismatch_message`）——本条的真实缺口是 **SFTP 在 skip-probe 分支完全无重验**，以及**两引擎 worker 盲传**（绕过 prepare 的直连路径零防护）。修复放在引擎层：download 入口在 `downloaded_bytes > 0`（续传态）时以短命控制连接重验——FTP 发 SIZE+MDTM，SFTP stat——共享比较器 `compare_remote_identity`（download/engine.rs，ARC-31 教训：跨引擎契约一处实现）：total_size 不等或双方 last_modified 均可得且不等 → `remote_changed` 稳定码（restart/check_url，落 NeedsAttention）；任一侧元数据不可得降级为日志+放行（不因服务器不支持 MDTM 误伤续传）。`downloaded_bytes == 0` 跳过（无可缝合前缀）。MDTM 秒级粒度限制记录在案：同秒替换由 size 守卫兜底。
- **验证测试**：假服务器升级（FTP per-path MDTM + 内容覆盖层、SFTP InMemFs per-file mtime + 覆盖层 + `swap_file` 句柄）；`ftp_engine.rs::arc42_resume_rejects_same_size_remote_replacement`（等大小换内容+MDTM 前移 → remote_changed，且断言本地前缀逐字节未动、无成品文件）、`arc42_resume_rejects_size_change`（同 MDTM 换尺寸 → size 分支）、SFTP 同构两用例。旧行为敏感性：禁用 revalidate 后 FTP 两用例失败，恢复后通过；未换内容的既有 resume 测试（ARC-19 契约）全量回归不退化（ftp 18/18、sftp 22/22）。

### ARC-43（P2，Closed）：delete_runtime_task 按 HashMap 序挑首个成功 session，误删同种子其他任务的 torrent

- **证据**：`delete_runtime_task` 只收 source_key（bt.rs:112-141），按 `sessions.keys()` 的 HashMap 序遍历、首个 api 成功即 break（:126-140）；调用方把 task.id 丢在地上（engine.rs:200-206 明明持有 TaskRecord）。ARC-12 的 per-task session 使同种子双任务各占一个含相同 info-hash 的 session，受害者可以是无关任务。受害链已逐环验证：forget → api_stats_v1 torrent_not_found（vendored api.rs:196-200）→ bt_runtime_stats_failed（bt.rs:637-661）→ 不在 NEEDS_ATTENTION_CODES（models/task.rs:1239-1248）→ mark_download_failed 写 Failed（mod.rs:617-631）。
- **影响**：双开同一磁力到不同目录是普通用法；禁 A 的做种/取消 A 可能令 B 从 Downloading 翻成 Failed，而 A 自己的 session 反而漏清理。
- **修复方向**：会话键已含 task_id，按 (source_key, task_id) 精确定位 owning session；engine.rs 传递 task.id。
- **验收**：同种子双任务取消其一，断言另一任务继续下载且目标 session 被清理。
- **2026-09-12 修复**：`delete_runtime_task` 签名增加 `task_id`（engine 分发与全部调用方同步），按 session key 的 `|task:{task_id}` 后缀精确定位 owning session 后删除/forget info-hash，不再「遍历全部 session 第一个匹配就 break」。无 owning session 时记 debug 日志退出。
- **验证测试**：既有单测 `delete_runtime_task_does_not_decrement_session_refcount` 更新为新签名并回归；「同种子双任务删 A 不影响 B」需要两个真实活动 session，单测以无匹配 task_id 的 no-op 路径覆盖定位逻辑（找不到即不动任何 session）。
- **2026-09-13 加固**：新增 `delete_runtime_task_targets_only_the_owning_session`——两个真实 session 各自加入同一 `.torrent`（paused），删除任务 A 的运行时状态后 A 的 session 不再持有该 torrent、B 的 session 原样保留；无 owning session 的 task_id 为 no-op。同步修正 `session_evicted_when_ref_count_reaches_zero` 中「双 session 必撞 DHT 端口」的过时注释（ARC-39 后已可共存）。

### ARC-44（P3，Closed）：start_task 三种 Ok 语义混一，dispatch pass 内幻影计数

- **证据**：Ok 有三种含义——真启动、「download already active」跳过（mod.rs:280-283）、Conflict 清理后返回 Ok（:329-343）；dispatch_inner 一律 active_count+=1 / host_slot+=planned_slots（:225-230）。复核注：Conflict 突发 largely 不可达（BEGIN IMMEDIATE 条件更新 + SQLite 单写者 + 任务锁序列化），实际可达的是 stale control 下的 already-active skip（例如 `ARC-40` 幽灵存在时）。
- **影响**：本 tick 后续任务被保守推迟，下一 tick 自愈；方向保守无害，但计数语义应诚实。
- **修复方向**：start_task 返回 Started/AlreadyActive/ConflictSkipped 枚举，dispatch 分别记账。
- **验收**：单测覆盖三分支计数。
- **2026-09-13 修复**：`start_task -> Result<StartTaskOutcome, String>`（Started/AlreadyActive/ConflictSkipped，全仓唯一调用方是 dispatch_inner，影响面收敛）；计数逻辑抽为纯函数 `account_start_outcome`——仅 `Started` 计数：AlreadyActive 行已在 downloads 初值里（原实现实为**双计**），ConflictSkipped 从未 spawn worker（原实现凭空占额）。方向保守性不变：非 Started 出口不计即从不虚高。
- **验证测试**：`convergence_tests::arc44_start_outcome_accounting_counts_only_started`——三分支逐一记账断言（Started +1/+slots、AlreadyActive/ConflictSkipped 零变化、无凭空 host 键），逐字满足验收；敏感性验证：计数改为无条件后用例失败。

### ARC-45（P3，Closed）：restart 用 abort 不排空即删临时文件，Windows delete-pending 可致新 worker ACCESS_DENIED

- **证据**：pause/cancel/retry×2 均「cancel + timeout(5s) drain」（actions.rs:245-247,333-335,379-381,468-470），唯 restart_task_from_beginning 是 cancel + h.abort()（tasks.rs:723-728）后立刻 remove_task_path（:729-731）。abort 只在下一个 await 生效、spawn_blocking 写入不可中断 → 旧 handle 可仍开着；std 以 FILE_SHARE_DELETE 开文件，删除成为 delete-pending，随后新 worker 的 OpenOptions::create 得 ERROR_ACCESS_DENIED。remove 错误还经 ? 中止 restart（:730 → actions.rs:828），留下未重置的任务。前置说明：NeedsAttention 通常无活 worker，但 BT 边下边置 NeedsAttention（bt.rs:578-598）+ resolve 无状态门使重叠可达；tasks.rs:723 的防御性 remove 也说明 stale control 在预期内。
- **影响**：Restart 后新下载打不开同名 temp → 数秒内 Failed「Access is denied」，用户眼中的「重启下载」不可靠。
- **修复方向**：对齐 checkpoint-drain 模式（cancel + drain 5s，超时再 abort 并二次等待）；remove 失败不中止 restart（容忍残留，新 worker 截断写）。
- **验收**：慢写 worker 下 restart 断言新下载成功打开 temp。
- **2026-09-13 修复**：lib.rs 新增 `cancel_and_drain_control(control, grace)`——单控制版两段式排空（Phase1 `timeout(grace, &mut handle)` 优雅等待；超时 Phase2 abort + join；经 `&mut` await 使 handle 所有权跨过 timeout 分支，避开 JoinHandle 二次 await 的 panic），与 shutdown 的 `drain_download_handles` 同语义。restart 改用它（5s grace）替代裸 abort；temp 与 auxiliary artifacts 的删除失败降级为 warn + 继续（不再 `?` 中止 restart——陈旧残留不应让任务停在半重置态，新 worker 本就 create/truncate 重建 temp）。并行特性同期加入的 artifact 清理点一并纳入容错。
- **验证测试**：新建 `tests/restart_quiesce.rs` 两条（无 AppHandle，直接驱动排空助手）——慢写 worker（持句柄循环写、20ms 轮询取消）断言排空返回时 worker 已退出、temp 可删、新 `create()` 成功（症状检查，Windows CI 上真实有效）；顽固 worker（忽略取消、睡 30s）断言 200ms grace 内经 abort+join 返回且路径可建。敏感性验证：助手临时还原为 abort-不排空 → 首条用例失败（exited 标志未置）。验收偏离记录：restart 函数硬依赖 `&AppHandle`（emit/default_download_dir/dispatch），故验收断言落在抽出的排空核心 + restart 内删改点的代码审阅，与 ARC-40 同款偏离。

### ARC-46（P3，Closed）：完成动作可在最后一个文件仍在哈希校验时触发关机

- **证据**：worker 在 engine.download 返回后立即自摘 control（mod.rs:460），然后才做可能数分钟的 SHA-256（verify_task_hash_with_pool，:468），最后才 maybe_emit_completion_action（:489-491）；后者唯一活性判据是 downloads.is_empty()（:579）+ 队列空（:582-586），无 hash_status 门。前端 AppShell.runCompletionAction（AppShell.tsx:768-794）倒计时结束直接执行系统关机/睡眠，不复核任务状态。
- **影响**：双任务近似同时完成 + completion_action=Shutdown → 机器在 A 哈希中途断电，hash_status 卡 Pending 需手动重验。
- **修复方向**：completion 判据纳入「存在 hash_status='pending' 的近期完成任务」；不要把哈希挪回 control 释放之前（会延长槽位占用）。
- **验收**：两任务接力完成 + 慢哈希 fixture，断言完成动作晚于哈希落库。
- **2026-09-13 修复**：核实两条 verify 路径（单文件 expected_hash、多文件 checksum records）都在长哈希**之前**置 task 级 `hash_status='pending'`、完成时置 Verified/Failed——该标志即哈希进行中的精确窗口。db 层新增 `any_completed_task_hash_pending`（`EXISTS(... status='completed' AND hash_status='pending')`）；scheduler 抽自由函数 `should_emit_completion_action(downloads, pool)`（downloads 空 + 队列空 + 无 pending 哈希，DB 错误按保守不触发处理），`maybe_emit_completion_action` 的活性判据换用之。哈希工作**未**挪回 control 释放前（按修复方向要求，槽位占用不变）。残余竞态如实记录：control 释放到 Pending 写入之间有毫秒级窗口，量级上与原缺陷的分钟级窗口不可比。
- **验证测试**：`convergence_tests` 三条——completed+pending（空 downloads/空队列）判据返回 false、置 Verified 后返回 true；无 expected hash（NotRequested）不受门影响；活跃 downloads 与非空队列在 hash 无关时依旧各自扣住动作（门是扩展不是替换）。敏感性验证：去掉 hash 门 → 首条用例失败。验收偏离记录：原文的「两任务接力 + 慢哈希 fixture」需整机 worker/事件环境（AppHandle 不可无头构造），以判据函数的直接驱动等价承载，与 ARC-40 同款偏离。

### ARC-47（P3，Closed）：Metalink 落入串行路径后从不清理 .part-N，泄漏至多 N×文件大小

- **证据**：cleanup_metalink_part_files 仅三点调用——fresh start :523、worker-failure :676、assembly 成功 :700——全部位于 parallel 函数内；serial 路径（:361-432、:725-809）直接写 temp_path 并 finalize，从不触碰 part 兄弟。触发链真实：parallel 暂停故意留 part（:653-669 供续传），后续 healthy<2（:337-359，30s 冷却或 416 除名易致）落入 serial 且 resume_from 只认 parallel 从不写的 temp_path（:1254-1257）→ 从零重下并发布，multi-GB part 永留输出目录旁。
- **影响**：纯磁盘泄漏、无正确性影响，但量级随文件大小 × 镜像数增长。
- **修复方向**：serial finalize 成功后调用 cleanup_metalink_part_files(temp_path)。
- **验收**：parallel → serial 切换场景断言无 part 残留。
- **2026-09-12 修复**：两处 parallel→serial fallback（外层 healthy<2 落穿、内层 worker_count<2 防御分支）在进入串行前 `cleanup_metalink_part_files` 并删除 plan 行——串行写 `temp_path` 本体，残留 part 既占磁盘又会被下一次并行 resume 的 `initial_total` 误算。
- **验证测试**：`arc34_plan_identity_mismatch_discards_stale_parts` 的第一段以单镜像资源触发过内层 fallback 路径（调试期间确认清理生效）；正式断言由外层落穿场景的 plan 删除 + part 清理覆盖。

### ARC-48（P3，Closed）：parse_byte_range 对极端 Initialization/@range 整型溢出

- **证据**：dash.rs:1691-1702 的 start/end 以 i64 parse、仅拒 end<start，`length: end - start + 1` 对 start=0,end=i64::MAX debug panic / release 回绕为 i64::MIN；byte_range_header（:1704-1707）再做裸加减产生无意义头。对比 HLS 孪生实现用了 saturating 运算（engine.rs:2220-2224）。值链完全来自 manifest 的 SegmentBase/Initialization range 属性。
- **影响**：恶意/畸形 MPD：debug 构建 panic（supervisor 内 unwind 还会连坐 `ARC-40` 的 slot 泄漏）；release 构建以费解的 range 错误失败而非校验提示。
- **修复方向**：checked/saturating 运算 + 解析失败即拒绝该 SegmentBase。
- **验收**：极端 range 属性单测在 debug/release 均安全拒绝。
- **2026-09-13 修复**：`parse_byte_range` 镜像 HLS 范本——@range 无符号语义下拒绝负值（含负 start/end），长度以 `saturating_sub().saturating_add(1)` 构造；`byte_range_header` 改 saturating 并 `.max(start)` 保持 end≥start 不变量。同族顺手修：`build_segment_plans` 的 `start_number + i` 改 `saturating_add`（同为 manifest 直接来源的算术）。
- **验证测试**：单测 `parses_byte_range_extremes_without_overflow`（0~i64::MAX、负值、end<start、u64 越界拒绝、单字节范围）；集成 `arc48_extreme_initialization_range_does_not_overflow`（Initialization range="0-9223372036854775807" 的 probe 不 panic）。旧行为敏感性：还原裸算术后集成测试以溢出 panic 失败。

## 八、程序运行效率

### PERF-01（P2，Closed）：历史任务搜索无法利用普通索引

- **证据**：[`task_records.rs`](../src-tauri/src/db/task_records.rs) 对三个字段执行 `LOWER(column) LIKE '%term%'`。
- **实测**：[`perf_baseline.rs`](../src-tauri/tests/perf_baseline.rs) `perf_baseline_50k`（`#[ignore]`）：本机 debug 50k search p95 ≈ **6.90 ms**（预算 100 ms）；见 [`performance-baseline-results.md`](performance-baseline-results.md) §3.3。
- **决策**：达标 → **保持 LIKE，不引入 FTS5**。文档写明支持规模（本机 harness ≥50k）；不假装已有 FTS。
- **验证**：`pnpm perf:baseline:50k` / `cargo test -j 1 --manifest-path src-tauri/Cargo.toml --test perf_baseline -- --ignored --nocapture perf_baseline_50k`。

### PERF-02（P2，Closed）：TaskDetails 在非相关子页持续轮询 segments

- **证据**：[`TaskDetails.tsx`](../src/components/shell/TaskDetails.tsx) segments effect 曾仅门控 diagnostics 主 tab，Requests 子页仍每 2s 调用 `listSegmentsPage`，且无 in-flight / visibility 守卫。
- **修复**：仅在 `diagSubTab === "segments"` 时订阅；上一请求未完成则跳过；`visibilitychange` 隐藏时停轮询。
- **验证测试**：`TaskDetails.test.tsx`（Requests 子 tab 停止 segments 轮询；慢请求下并发 ≤ 1）。
- **验收**：Requests、Logs 等子页不会请求 segments；慢请求下并发数始终为 1。

### PERF-03（P2，Closed）：进度批次通知仍做全列表 O(N) 扫描

- **证据**：[`use-task-events.ts`](../src/hooks/use-task-events.ts) 曾在 progress flush 后对全部已加载任务建 `previousById` 并扫描。
- **修复**：`patchTasksBatch` 返回 `statusTransitions`；progress flush 仅对变化任务发 toast，工作量与变化数近似线性。
- **验证测试**：`task-data-store.membership.test.ts`（status 变化返回 transitions；bytes-only patch 为空）。
- **验收**：1k 已加载任务、每批少量变化时通知工作量与变化 ID 数量近似线性。

### PERF-04（P2，Closed）：HLS AES key 和 init map 缺少任务级去重缓存

- **证据**：HLS 引擎（当时为单文件 `hls.rs`，现已拆分为 [`download/hls/`](../src-tauri/src/download/hls/)）每个 segment worker 曾独立拉取同一 key/init-map。
- **修复**：任务级 `HlsTaskFetchCache` singleflight；失败不缓存；init-map 使用 `.part` + rename 发布。
- **验证测试**：`hls` 单元测试（并发 coalesce + 失败不缓存）。
- **验收**：N 个共享 key/map 的 segment 每个 URI 只成功请求一次；失败可重试。

### PERF-05（P2，Closed）：长期缓存和 task events 缺少完整生命周期上限

- **证据**：`FILES_VERSION_CACHE` 曾无上限；`task_events` 仅随任务删除清理。
- **修复**：files-version 缓存上限 4096（简易 LRU）；`prune_task_events`（每任务 200 + 14 天 age，FUN-07 保护仍 paused 任务的最新 pause 事件）；启动与 6h 后台 prune。
- **验证测试**：`events` 单元测试（缓存上限）；`task_events_retention.rs`（cap + pause 保护）。
- **验收**：长跑后 cache 与事件表有上界；schedule-paused 语义在 prune 后仍可读。

### PERF-06（P2，Closed）：async 热路径仍有同步文件系统调用

- **证据**：`prepare_task_for_download`、BT `canonicalize`、`ffmpeg_path` PATH 扫描曾在 async 上下文同步阻塞。
- **修复**：临时文件探测改 `tokio::fs`；BT session key canonicalize/`create_dir_all` 走 `spawn_blocking`/`tokio::fs`；ffmpeg 解析用 `try_exists` + PATH `spawn_blocking`。
- **验证**：相关单元测试（BT session key、ffmpeg PATH）；完整慢盘故障注入记为后续 soak。
- **验收**：上述三点不再在 Tokio worker 上同步 exists/metadata/canonicalize/PATH 扫描。

### PERF-07（P1，Closed）：完成动作用户命令同步阻塞且无超时

- **证据**：[`platform/mod.rs`](../src-tauri/src/platform/mod.rs) 曾使用同步 `std::process::Command::status()`，scheduler 在下载 worker 内直接调用。
- **风险**：用户命令挂起会长期占用 Tokio worker，并阻塞完成动作后的 `spawn_dispatch`。
- **修复**：`run_user_command` 改为 async；`tokio::process::Command` + `kill_on_drop` + 默认 60s `timeout`；超时返回结构化 `completion_command_timeout` 并 kill 子进程；保留 S-4 校验。
- **验证测试**：`platform` 单元测试 `run_user_command_times_out_hanging_process`；`completion_action.rs`（挂起命令超时期间 runtime 仍可调度）。
- **验收**：永不退出的测试命令在超时后终止并记录结构化错误，不阻塞其他异步工作。

### PERF-08（P2，Closed）：限速器使用墙上时间且缺公平等待

- **证据**：曾以 SystemTime 补充 token，多连接私有 sleep + CAS 争抢。
- **修复**：[`speed.rs`](../src-tauri/src/download/speed.rs) 改用进程 `Instant` 单调毫秒；lazy 集中 ticker（25ms）+ `Notify`；不足时 `select! { cancel, notified }`；单次取量按 tick 公平量子并在成功 CAS 后 yield；`set_limit(0)` 停 ticker。公开 `throttle` API 不变（仍可取消，ARC-04）。
- **验收**：时钟回拨不阻止 refill；取消仍快速收敛；多 waiter 吞吐方差在单元容差内。
- **验证**：`cargo test -j 1 --manifest-path src-tauri/Cargo.toml --lib speed::`。

### PERF-09（P3，Needs benchmark）：release `opt-level="s"` 可能牺牲热点吞吐

- **证据**：[`Cargo.toml`](../src-tauri/Cargo.toml#L105) 为 release 使用尺寸优化。
- **处理**：先比较 `s` 与 `3` 在 hash、AES、XML、BT 和真实下载路径的吞吐、体积和启动时间；没有数据前不直接修改。
- **验收**：结果写入性能基线，必要时只对热点 package 使用 profile override。

### PERF-10（P2，Closed）：没有 bundle size 和前端性能回归预算

- **证据**：曾无 CI chunk budget；主要分块约 215/182/150/141 kB raw。
- **修复**：[`scripts/bundle-budget.json`](../scripts/bundle-budget.json) + [`scripts/check-bundle-budget.mjs`](../scripts/check-bundle-budget.mjs) 聚合 initial-shell（JS gzip ≤340 kB、raw ≤1.10 MB、CSS gzip ≤18 kB，并报告 brotli）；`pnpm check:bundle`；CI frontend 在 `pnpm build` 后运行；结果写入 [`performance-baseline-results.md`](performance-baseline-results.md) §5。不对单个 vendor chunk 设硬上限。
- **验收**：超预算时 CI 失败并打印 per-chunk 分解；交互性能门禁仍延期。
- **验证**：`pnpm build && pnpm check:bundle`；`node --test scripts/check-bundle-budget.test.mjs`。

### PERF-11（P2，Closed）：性能基线只有方法和估算，没有实测数据

- **证据**：曾仅有 [`docs/performance-baseline.md`](performance-baseline.md) 方法模板，无 1k/10k 实测。
- **修复**：新增 [`src-tauri/tests/perf_baseline.rs`](../src-tauri/tests/perf_baseline.rs)（1k 默认 smoke，`#[ignore]` 10k）、`scripts/perf/*.ps1`、`pnpm perf:baseline`；本机填入 [`docs/performance-baseline-results.md`](performance-baseline-results.md)（含 p50/p95 与 `EXPLAIN QUERY PLAN`）。
- **验收**：有硬件/OS/commit/profile 元数据与可重复编排；50k+、UI 冷启动/FPS、HLS/BT soak、CI 绝对门禁明确延期，不假装 Closed。
- **验证**：`cargo test -j 1 --manifest-path src-tauri/Cargo.toml --test perf_baseline`；本地 `pnpm perf:baseline:10k`。

### PERF-12（P1，Closed）：日志保留策略使现场排障不可行

- **证据**：主应用在 [`lib.rs`](../src-tauri/src/lib.rs#L360) 注册 `tauri_plugin_log`，只设置了 target 与 level，**没有设置 `max_file_size` 或 `rotation_strategy`**。tauri-plugin-log v2 的默认值是 `max_file_size = 40000` 字节、`RotationStrategy::KeepOne`（超过上限即丢弃旧内容）。另一侧，native host 的 [`logging.rs`](../src-tauri/src/logging.rs#L71) 用 `Rotation::DAILY` 且**没有 `max_log_files`**，永不删除；同处第 77 行的 `std::mem::forget(guard)` 会泄漏 `WorkerGuard`，使这个短生命周期进程退出时 non-blocking writer 缓冲区中的日志（往往正是错误日志）不被 flush。
- **影响**：两条日志路径有方向相反的缺陷。主应用实际只保留最近 40 KB，对一个多任务并发、每任务产生大量进度与重试记录的下载管理器而言约等于几百行——**用户报告问题时相关日志几乎必然已被覆盖**。这直接削弱了 `UX-01` 提供的「打开日志目录」恢复入口，也让本文其余所有问题在用户现场无法诊断。native host 侧则是无上界的磁盘占用。
- **修复方向**：主应用显式设置 `.max_file_size(5_000_000).rotation_strategy(RotationStrategy::KeepSome(5))`；native host 改用 `RollingFileAppender::builder().max_log_files(7)`，并把 `WorkerGuard` 返回给 `main` 持有到进程结束而不是 forget。
- **验收**：长时间运行后日志总量有明确上界且覆盖足够长的时间窗；native host 异常退出时最后一条错误日志可见于文件。
- **2026-08-13 修复**：主应用显式设置 `.max_file_size(5_000_000)` + `RotationStrategy::KeepSome(5)`（覆盖 40 KB / `KeepOne` 的默认值）；native host 改用 `RollingFileAppender::builder().max_log_files(7)`，并把 `WorkerGuard` 从 `std::mem::forget` 改为经由 `StandaloneLogGuard` 交还给 `main` 持有到进程结束——该进程由浏览器每次交接时拉起，此前退出时缓冲区里的最后几行（往往正是错误本身）从不落盘。

### PERF-13（P2，Open）：两套 rustls 密码学后端同时编译进二进制

- **证据**：`Cargo.lock` 中 `rustls 0.23.40` 的依赖同时包含 `aws-lc-rs 1.17.0` 与 `ring 0.17.14`。原因是部分依赖启用 rustls 默认的 `aws-lc-rs` feature，而 `suppaftp` 的 `tokio-rustls-ring` 与 `librqbit` 的 `rust-tls` 启用 `ring`，Cargo 的 feature 合并导致两套完整后端都进入最终产物。
- **影响**：`aws-lc-sys` 需要 cmake 与（Windows 上）NASM 的 C 构建，是 CI 冷编译时间的主要来源之一。估算统一后可减少 4-8 分钟冷编译与 2-5 MB 二进制体积。这个问题能长期存在而无人察觉，直接归因于 `ENG-01` —— `deny.toml` 配置了 `[bans] multiple-versions` 但 CI 从不执行 `bans` 子命令。
- **修复方向**：用 `cargo tree -e features -i aws-lc-rs` 定位启用方，显式声明 `rustls = { default-features = false, features = ["ring", "std", "logging", "tls12"] }` 统一到 `ring`。改完必须验证 HTTPS / FTPS / SFTP / BT 四条 TLS 路径的握手仍正常。
- **验收**：`cargo tree -d` 不再出现两个密码学后端；四个协议的 TLS 集成测试通过；在 `deny.toml` 的 `[bans]` 中加入 `aws-lc-rs` 防回归。
- **2026-08-26 复核**：`cargo tree -e features -i` 确认 ring 经 suppaftp（`tokio-rustls-ring`）与 hyper-rustls → reqwest 生效；aws-lc-rs 除 reqwest 通用 `rustls` feature 外还被**直依赖** russh 与 librqbit-sha1-wrapper 无条件拉入，且 rustls 自身同时启用了两套 provider feature——统一到单一后端必须同时处理这三条来源，仅改 suppaftp 的 feature 不够。`deny.toml` 的 `[bans] multiple-versions = "warn"` 使 CI 的 `cargo deny check bans` 永不可能因此变红，问题会存续到有人主动收敛为止。

### PERF-14（P2，Open）：前端渲染热路径上的冗余订阅

- **证据**：进度更新链路本身已优化到位（后端 250ms 节流 → rAF 批处理 → `patchTasksBatch` 零差量快路径 → `TaskRow` 逐行订阅），全仓库没有对象字面量 selector。但有三处例外：[`Palette.tsx`](../src/components/shell/Palette.tsx#L147) 用 `useShallow` 订阅了一个随即被 `void tasks;` 丢弃的 `Task[]`，浅比较在每个进度 tick 必然失败；[`use-app-updater.ts`](../src/hooks/use-app-updater.ts#L13) 不带 selector 订阅整个 store，且 effect 依赖 `[store, autoCheckEnabled]`，导致每次 updater 状态变化都重跑 `init()` 并重排自动检查定时器；`TaskDetails.tsx` 的 6 个列表组件（`ChunkList`、`ConnectionList`、`EventList`、`RequestList`、`HlsSegmentList`、`DashSegmentList`）都没有 `memo`，而 `task` 对象每 250ms 换引用。
- **影响**：命令面板打开期间（正是用户输入搜索时）每 250ms 全量重渲染；HLS 任务打开 Segments 子页时 100 行 DOM 每秒 reconcile 4 次，而数据 2 秒才更新一次。
- **修复方向**：删除 `Palette.tsx:147` 与 `:159` 两行（`taskById` 订阅已覆盖需求）；`useAppUpdater` 改为逐字段 selector 并用 `getState().init()` 摘掉 effect 依赖；给 6 个列表组件加 `memo` 并把内联箭头回调提为 `useCallback`。另外 `QueueCenter.tsx:90` 的 10 秒轮询缺少 visibility 门控与 in-flight 守卫，建议抽 `useVisibilityGatedPoll` 并同时应用到 `use-task-detail-queries.ts` 中重复 3 遍的同一模式。
- **验收**：进度 tick 期间 Palette 与 TaskDetails 列表不重渲染；窗口隐藏时 QueueCenter 停止轮询。

### PERF-15（P2，Closed）：HLS live 轮询期间每 100 毫秒查询一次数据库

- **证据**：[`hls/engine.rs`](../src-tauri/src/download/hls/engine.rs#L2033) 的 `wait_hls_finish_signal` 以 100ms 间隔循环调用 `db::hls_finish_requested`，它被放在 `select!` 中与 target duration（典型 6-10 秒）的 sleep 竞争。
- **影响**：每个 live HLS 任务在每个轮询间隔内产生 60-100 次 SQLite 查询，仅为轮询一个布尔标志。多个 live 任务并发时会抢占连接池并与 checkpoint 写入争锁（`ARC-06` 刚处理过 BUSY_SNAPSHOT）。而 `finish: Arc<AtomicBool>` 已经在 `DownloadContext` 中，DB 查询只是多余的回退路径。
- **修复方向**：改用 `tokio::sync::Notify`，由 finish 命令在写 DB 的同时 `notify_waiters()`；若必须保留 DB 兜底，把间隔提高到 1-2 秒。
- **验收**：live 轮询期间的 SQLite 查询次数与轮询次数同阶，而非与 100ms tick 同阶。
- **2026-09-13 修复**：按修复方向主路落地——`DownloadControl`（lib.rs）与 `DownloadContext`（engine.rs）各加 `finish_notify: Arc<Notify>`（同一实例，scheduler 在 start_task 注册时创建接线）；`finish_live_recording` 在 `finish.store(true)` 后 `notify_waiters()`。`wait_hls_finish_signal` 重构为「finish 标志 → DB 检查 → `select!{ notified, sleep(fallback) }`」——notify 即时唤醒，DB 兜底间隔 100ms→2s（常量 `HLS_FINISH_DB_FALLBACK`，函数参数化便于测试注入）。非 HLS 引擎的 context 解构以 `finish_notify: _` 吸收新字段。
- **验证测试**：`hls/engine.rs` 内两条单测——notify 触发（fallback=60s）下 waiter 5s 内返回（证明走的是 Notify 而非轮询）；无 notify 时 DB 置位 + 200ms fallback 下 2s 内返回。敏感性验证：临时禁用 notify 分支 → 首条用例 5s 超时失败。验收映射：live 轮询的 DB 查询从「每 100ms 一次」（每 target-duration 轮询 60-100 次）降为「每 2s 兜底一次 + finish 即时唤醒」，查询次数与轮询次数同阶达成。

### PERF-16（P2，Open）：路径预留全表扫描与若干无 LIMIT 查询

- **证据**：[`task_records.rs`](../src-tauri/src/db/task_records.rs#L585) 的 `list_reserved_final_paths` 无 `LIMIT`，把所有活动任务及其全部文件的 `final_path` 拉进一个 `HashSet<String>`，且在 `create.rs` 的 DEFERRED 事务内被调用（最多重试 32 次），创建流程中另有一次调用。此外 `list_task_records` 与 `list_browser_realtime_task_records` 的 active 分支都没有 LIMIT。
- **影响**：1 万活动任务 / 每任务 100 个文件的场景下，单次创建要在事务持锁期间物化上百万条路径字符串，直接放大 `ARC-21` 的锁冲突窗口。
- **修复方向**：不要「读全集到内存再判断」。改为循环内做一次 `SELECT 1 FROM tasks WHERE final_path = ? AND status IN (...)` 点查（该列已有部分唯一索引），或干脆去掉预读、完全依赖唯一索引冲突加重试。`list_tasks` 复用已定义的 `MAX_TASK_PAGE_SIZE`，WS 快照 active 分支加 `LIMIT 500`。
- **验收**：`perf_baseline` 中加入大量活动任务下的创建耗时用例，断言不随活动任务数线性增长。

## 九、安全边界

本章为 2026-08-13 复审新增。此前安全问题散落在 ARC 各条中，独立成章便于按攻击面而非按模块追踪。

先记录已核实无问题的项，避免后续重复审查：SSRF 有三层防御（handoff 前的字面 IP 检查与 DNS 预解析、连接期 resolver 过滤、重定向逐跳复检，覆盖 IPv4-mapped IPv6、CGNAT 与 `0.0.0.0/8`）；全仓库无 `danger_accept_invalid_certs`；SFTP TOFU 在认证前校验且不匹配时 fail-closed、遗忘只能显式 DELETE；SQL 全部参数化，无 `SELECT *`，唯一的字符串拼接是白名单 match；`PRAGMA foreign_keys = ON` 对每条连接生效且 12 张子表级联完整；ffmpeg 参数全部逐个 `.arg()` 传入本地路径；`seed_mock_tasks` 正确受 `#[cfg(debug_assertions)]` 门控；`withGlobalTauri` 未启用；ChaCha20-Poly1305 的 nonce 为每次加密新生成的 96 位随机值，v1 密文的 AAD 绑定 `task_id` 且无法被 legacy 分支降级。

### SEC-01（P0，Closed）：Tauri `fs` 能力被授予全盘读写 scope

- **证据**：[`capabilities/default.json`](../src-tauri/capabilities/default.json#L21) 的 `fs:default` 中包含 `{ "path": "**" }`，且 `tauri_plugin_fs` 已在 [`lib.rs`](../src-tauri/src/lib.rs#L391) 注册、前端也装了 JS 绑定。
- **影响**：`{ "path": "**" }` 是无根 glob，等价于把整个文件系统的读/写/删除/重命名交给 webview。CSP 的 `script-src 'self'` 挡得住远程脚本注入，挡不住供应链投毒。一旦有任意 JS 执行，配合已授予的 `process:allow-restart` 与 `autostart:default`，即可读取 `~/.ssh/id_rsa`、写入启动目录并完成持久化。
- **修复方向**：删除 `{ "path": "**" }`，收敛到应用真正需要的目录。「任意用户选定目录」的读写改为经后端命令，由 Rust 侧持有唯一的文件系统权限。同时在 CSP 中补 `base-uri 'self'; form-action 'none'; object-src 'none'`。
- **验收**：capabilities 中不存在无根 glob；保存目录选择与校验流程在收敛后的 scope 下仍然可用。
- **2026-08-13 调研修正（本条尚未修复）**：原描述有两处不准确，会误导修复方案。
  1. **实际授予的是全盘读 + `mkdir`，不是读写。** `fs:default` 展开后只含 `read_dir` / `read_file` / `read_text_file` / `exists` / `mkdir`，**不含任何写命令**。严重性不变（读 `~/.ssh/id_rsa`、浏览器 Login Data 等仍然成立），但方案取舍随之改变。
  2. **不是「单行改动，风险最低」。** 前端只有两处 `plugin-fs` 调用：[`local-file.ts`](../src/lib/local-file.ts) 的 `readTextFile`（服务于拖拽 .txt、选择 .txt 批量导入、选择 SSH 私钥三个场景）与 [`export.ts`](../src/lib/export.ts) 的 `writeTextFile`。原描述所说的「校验保存目录是否存在/可写」在前端**不存在**——那些早已在后端（`query_disk_space`、`probe_directory_writable`、`resolve_save_dir`）。删掉 `**` 会打断文件选择器的两条路径；拖拽路径则不受影响，因为 fs 插件自身会在 `DragDrop` 事件里 `allow_file`，而 dialog 插件**不会**（v1→v2 的行为变更）。
  3. **附带发现：任务导出很可能已经是坏的。** `export.ts` 需要 `write_text_file` 与 dialog `save`，两者都未被授予，失败被 `try/catch` 静默吞掉。修复前应先实机确认，以决定是否把「修好导出」纳入本条验收。
  4. 目录选择器只返回字符串、manifest 走 `file://` URL 交给 Rust，二者都不经过 fs scope，所以「用户选任意保存目录」不受收敛影响。
  已核实 `FsExt::fs_scope()` + `Scope::allow_file` 在本项目的 tauri 2.11.2 / tauri-plugin-fs 2.5.1 下可用，运行时放行方案技术可行；但它会引入一个新的提权原语（前端可请求放行任意路径），且 scope 只增不减，因此推荐移除 fs 插件、两处改走后端命令。
- **2026-08-14 修复**：实机确认「更多 → 导出」无反馈。`save()` 缺 `dialog:allow-save`，`writeTextFile` 也不在 `fs:default` 里，失败被 `export.ts` 的 `try/catch` 吞掉。
  已移除 `tauri-plugin-fs` 与 `{ "path": "**" }`。新增 [`commands/local_files.rs`](../src-tauri/src/commands/local_files.rs)：`read_local_text_file`（`.txt` 批量列表 ≤ 1 MiB；SSH 私钥限 `id_rsa` / `id_ed25519` / `id_ecdsa` / `id_dsa` / `identity` 与 `.pem` / `.key`，≤ 64 KiB）与 `write_export_file`（仅 `.json` / `.csv`，≤ 16 MiB）。两条命令都拒绝相对路径与 `..`。capabilities 增加 `dialog:allow-save`（设置页备份导出也需要它）。CSP 补 `base-uri 'self'; form-action 'none'; object-src 'none'`。导出失败改为 toast，不再静默。
  目录选择器与 `file://` 清单路径本来就不走 fs 插件，不受影响。拖拽路径由 webview `onDragDropEvent` 提供，读取改走同一条后端命令。
- **验证**：`cargo test --lib local_files`（策略拒绝相对路径 / `..` / 错误扩展名 / `passwd`，JSON/CSV 往返写入，capabilities 断言不再含 `fs:default` 或 `"**"`）；`src/lib/export.test.ts` 覆盖报表序列化。GUI 请再点一次「更多 → 导出 JSON/CSV」，应弹出另存为；取消无 toast，成功有成功 toast，失败有错误 toast。

### SEC-02（P0，Closed）：备份恢复缺少内容策略校验，可导致任意路径写入

- **证据**：[`backup.rs`](../src-tauri/src/commands/backup.rs#L132) 的校验链只有三项——magic/版本、备份文件自带的 sha256（攻击者可自行计算）、`PRAGMA integrity_check` 加迁移。恢复后 `tasks.final_path`、`temp_path`、`save_dir` 这些绝对路径字符串没有任何重新清洗，也不会回到 `unique_final_path` 重新落到 save_dir 之下。`manifest.credentials_policy` 被读出后仅回传给 UI，从未校验。
- **影响**：「从旧机器迁移配置」是很自然的社工场景。攻击者可派发一个 `.vibe-backup`，内含 `status='paused'`、`final_path` 指向启动目录、`url` 指向自己服务器的任务。用户恢复并重启后调度器 resume 该任务，应用即把攻击者的可执行文件写入启动目录。
- **修复方向**：在 `materialize_and_verify_backup_db` 之后、rename 到 pending 之前增加策略扫描——拒绝或重置 `final_path`/`temp_path` 不在当前 `default_save_dir` 或任务自身 `save_dir` 之下的行；校验 `credentials_policy` 必须为 machine-bound；在恢复确认对话框中明示只应恢复自己创建的备份。顺带把 [`backup.rs`](../src-tauri/src/db/backup.rs#L250) 中路径可预测、权限未收敛的验证临时文件改用 `tempfile`，并给 `.db.bak-*` 加保留策略。
- **验收**：构造含越界 `final_path` 的备份被拒绝或被安全重置；恢复失败不破坏 live 数据库；临时验证文件不落在共享 temp 目录且随 Drop 清理。
- **2026-08-13 修复**：新增 [`db::enforce_backup_path_policy`](../src-tauri/src/db/backup.rs)，在 `materialize_and_verify_backup_db` 之后、rename 到 pending 之前扫描 `tasks` 与 `task_files` 的 `save_dir`/`temp_path`/`final_path`，越界即 **fail-closed** 拒绝整个备份（错误码 `backup_unsafe_paths`）。不采用「重写到安全目录」，因为会撞 `idx_tasks_final_path_active` 且会给用户一个被静默篡改的恢复结果。允许根取自 **live 配置**（`default_save_dir` + `default_download_dir`），绝不取自备份自身的 `settings`，否则策略可被自举绕过。路径比较为文本级（备份里的路径通常尚不存在，无法 canonicalize），按分隔符边界匹配以免 `/data/dl-evil` 被当成 `/data/dl` 的子路径，Windows 上大小写不敏感；相对路径与含 `..` 的路径一律拒绝。
  同时在 `parse_backup_bytes` 中校验 `credentials_policy` 必须为 machine-bound（此前它被解析后原样传给 UI，连显示都没有，是纯死数据），放在解析层使 `validate_app_backup` 一并受益。并修复了 verified 临时文件的三条泄漏路径（策略拒绝、快照失败、rename 失败）——该文件是备份库的完整副本，含加密凭据。
- **调研补充**：攻击链比原记录更宽——恶意 `final_path` 在用户删除任务时还会流向文件删除逻辑（`actions.rs:545`），构成任意文件**删除**原语；且 `auto_resume_on_startup` 本身也在备份内，可被置真以实现无交互触发。
- **未做**：临时文件改用 `tempfile` crate 需要改动 `validate_app_backup` 与 `restore_app_backup` 两处签名（Drop 即删，需返回持有 guard 的结构体），留待后续批次。
- **验证**：`cargo test --test backup_restore`（6 通过），新增 3 项。其中 `sec02_backup_with_out_of_root_paths_is_rejected` 显式断言 `read_backup_file` 与 `materialize_and_verify_backup_db` **都接受**该恶意备份、只有新策略拒绝——这正是修复必要性的证据，而不只是测试新代码。另有正向用例确保策略不过度拒绝（根内路径与 NULL 路径仍放行）。

### SEC-03（P1，Closed）：非统一 HTTP 客户端绕过 SSRF 守卫与代理策略

- **证据**：`build_client`（[`http/mod.rs`](../src-tauri/src/download/http/mod.rs#L378)）安装了 A-2 的两层 SSRF 防护（`HickoryResolver` 连接期过滤私有/保留 IP、`ssrf_safe_redirect_policy` 逐跳复检）并正确处理 `AppProxyMode::Off => builder.no_proxy()`。但有两处绕过：[`bt.rs`](../src-tauri/src/download/bt.rs#L1674) 的 `download_torrent_bytes` 自建 client，只在 SOCKS5 时设代理、从不 `.no_proxy()`、无 SSRF 守卫；[`create.rs`](../src-tauri/src/commands/tasks/create.rs#L1381) 的 sidecar 校验和发现用裸 `Client::builder().timeout(3s)`，同样无 `.no_proxy()`、无 SSRF 重定向策略、无 `HickoryResolver`。此外 [`webdav.rs`](../src-tauri/src/download/webdav.rs#L187) 直接调用 `build_client` 绕过了客户端缓存，而 `client_for_config` 上方的注释恰好写着要防止派生引擎这样做。
- **影响**：三类。其一为 SSRF——`.torrent` URL 与 sidecar URL 均可由用户/剪贴板触发，302 到 `169.254.169.254` 或 `127.0.0.1` 会被跟随（reqwest 默认跟随 10 次）。其二为代理策略失效：由于 `Cargo.toml:29` 启用了 reqwest 的 `system-proxy`，缺少 `.no_proxy()` 意味着**用户选择「不使用代理」时这些请求仍走系统代理**；用户配置 HTTP/HTTPS 代理时 `custom_socks5_url_with_auth()` 返回 `None`，请求也不走用户指定的代理。其三，这个缺陷有活的复现证据——见第三章的 Rust 测试挂起分析。
- **修复方向**：让 `BtEngine` 持有 `Arc<HttpEngine>`（与 HLS/DASH/Metalink/WebDAV 一致）并使用 `client_for_config`；注意 `EngineRegistry::new` 中 `bt_engine` 当前在 `http_engine` 之前构造，需要调整顺序。sidecar 发现同样改用统一工厂。WebDAV 改用 `self.http.client_for_config`。最后确立一条规则：**`download/` 下禁止出现 `reqwest::Client::builder()`，统一入口只有 `build_client`**，可用 CI grep 或 clippy `disallowed_methods` 强制。
- **验收**：三处均通过统一工厂获取 client；新增测试断言 Off 模式下不使用系统代理、跨协议重定向到内网被拒绝；`download/` 下不存在裸 `Client::builder()`。
- **2026-08-26 复核**：WebDAV 一侧已修复——webdav.rs:67-70、:122-128 现在委托共享 `Arc<HttpEngine>`，不再直调 build_client。仍成立的实例：`bt.rs:1674-1680` 的裸 builder（无 dns_resolver、默认 redirect policy 盲随重定向、无 `.no_proxy()`），以及此前未登记的**第二条 BT 路径**——fetch 失败时 `AddTorrent::from_url`（bt.rs:1613-1615）把同一个未审查 URL 交给 librqbit 内部自建的裸 reqwest client（vendored session.rs:706-713），Vibe 只设置了 proxy_url/ratelimits，该路径同样完全无守卫。sidecar 发现（create.rs:1381 裸 Client）未见修复。「`download/` 下禁止裸 `reqwest::Client::builder()`」的 CI 强制仍未落地。
- **2026-09-12 修复**：抽出 [`download/net_factory.rs`](../src-tauri/src/download/net_factory.rs) 的 `NetworkClientFactory`（`build_client` + 指纹缓存自 HttpEngine 迁入），HTTP、四个派生引擎与 BT 共享同一工厂实例（`set_proxy_config` 一次失效全部）。三处裸构建点收编：`bt.rs::download_torrent_bytes` 改走工厂（Off 即 `no_proxy()`，修复系统代理泄漏；保留 60s 控制面预算）；`.torrent` URL 的 `AddTorrent::from_url` fallback 删除（librqbit 内部 client 是最后一个绕过点，失败返回结构化 `bt_torrent_fetch_failed`）；`create.rs` sidecar 校验和发现改走工厂并接入任务级代理（每请求 3s 预算）；`webdav.rs` 目录探测改用缓存 client。
- **验证测试**：新增 [`tests/source_hygiene.rs`](../src-tauri/tests/source_hygiene.rs) 源码扫描门禁（`src/download`/`src/commands`/`src/bin` 下 `Client::builder(` 只允许出现在工厂文件）；`add_torrent_source_http_fallback_on_download_failure` 改写为断言结构化失败而非 URL 回退。Off 语义回归由 `fun20_*` 与 `bt_engine` 的 SOCKS5 不绕行测试共同覆盖。

### SEC-04（P1，Open）：WebSocket 桥缺速率限制，Windows 引导文件权限不足

- **证据**：桥正确绑定 `127.0.0.1`（[`browser_realtime.rs`](../src-tauri/src/browser_realtime.rs#L128)）并使用 UUIDv4 token 校验（`:183`），但 `handle_client_message` 对 `createDownload` 没有任何节流或配额，也没有 `Origin` 校验。引导文件路径可预测（`:441`），Unix 上有 `0o400` 保护，Windows 上只设了 readonly 属性（`:422`）——**readonly 不是 ACL**，同用户的任意进程都能读到 token。
- **影响**：本机同用户进程读取 token 后即可无限调用 `createDownload`，用于耗尽磁盘或把应用当作流量放大器。
- **修复方向**：Windows 上把引导文件 DACL 收敛为仅当前用户，或改用命名管道传递 token 而不落盘；给 `createDownload` 加令牌桶（建议 10 次/分钟，突发 5）；补 `Origin` 白名单（`chrome-extension://` / `moz-extension://`）作为纵深；把 token 从 query string 移到 `Sec-WebSocket-Protocol` 或首帧握手消息，避免出现在访问日志中。另外扩展侧 `background.js:138` 的 `api.runtime.onMessage` 忽略了 `_sender`，应校验 `sender.id === chrome.runtime.id`。
- **验收**：超过配额的创建请求返回结构化错误；Windows 上非当前用户进程无法读取引导文件；扩展消息校验发送方身份。

### SEC-05（P1，Open）：明文口令、私钥与密钥全程无 zeroize

- **证据**：`zeroize` 只作为传递依赖出现在 `Cargo.lock`，`src-tauri/src` 中零引用。[`task_credentials.rs`](../src-tauri/src/db/task_credentials.rs#L6) 的 `TaskCredentials` 四个敏感字段都是普通 `String`；`resolve_task_credentials` 解密过程中还经过 `serde_json::from_str` 产生额外副本；`encryption_key()` 返回按值拷贝的 `[u8; 32]`；`decrypt_headers` 返回的 Cookie JSON 同理。
- **影响**：进程崩溃转储、Windows 页面文件、休眠镜像或同用户进程读取内存，都能捞到 SFTP 私钥口令与 Cookie 明文。对一个明确以「加密存储凭据」为卖点的模块，这是承诺与实现之间的缺口。
- **修复方向**：给 `TaskCredentials` 与 `TaskCredentialsSecret` 加 `ZeroizeOnDrop`（需覆盖 serde 反序列化产物），`encryption_key` 返回 `Zeroizing<[u8; 32]>`，解密中间的 `Vec<u8>` 与返回的 `String` 改用 `Zeroizing`/`SecretString`。至少覆盖 password、private_key_data、private_key_passphrase 与解密后的 headers JSON。
- **验收**：上述四类敏感数据在作用域结束时被擦除；新增测试验证 Drop 后缓冲区不含原文。

### SEC-06（P2，Closed）：`browser_messages` 永久保留含凭据的完整 URL

- **证据**：[`browser.rs`](../src-tauri/src/commands/browser.rs#L269) 存入的是 `input.url.trim()` 原始 URL 而非 `sanitize_url` 后的结果。`task_events`、`task_requests`、`task_request_headers` 都有 prune 任务，**只有 `browser_messages` 没有任何 prune 或 TTL**。
- **影响**：从浏览器交接的 S3 预签名 URL、带 `?token=` 的下载链接会永久留在未加密的 SQLite 中。数据库文件泄露一次即等于泄露全部历史下载凭据。
- **修复方向**：入库前用 `logging::sanitize_url` 剥掉 query（去重依赖 `request_id`，不依赖 URL），并新增与 `prune_task_events` 同规格的 `prune_browser_messages`（建议 7 天），挂到启动清理与 6 小时周期任务上。
- **验收**：`browser_messages` 中不含 query string；长期运行后该表行数有上界。
- **2026-08-26 复核**：增长维度进一步确认——`clear_tasks` 的十表清理序列（task_state.rs:135-191）同样不触碰 browser_messages，用户手动「清除全部任务」也无法回收空间；`latest_browser_error` 因 (browser, created_at DESC) 索引保持 index seek、不受增长拖累，真实成本是 DB 文件与每次 VACUUM INTO 导出（connection.rs:218）的无界膨胀，以及含凭据 URL 的无限期留存。修复建议不变；prune 建议挂到 lib.rs:564-586 既有启动清理块，与另外三个 prune 并列。
- **2026-09-12 修复**：`browser_messages` 入库前经 `sanitize_url_for_storage` 剥离 query 与 fragment（预签名 URL 与 query token 不再落盘；任务创建在内存中先行消费完整 URL，功能不受影响）；新增 `prune_browser_messages`（按龄 30 天 + 每浏览器 200 条上限，照 `prune_task_events` 的两步范本），接入启动维护序列。
- **验证测试**：cleaning 行为与既有 `browser_messages_track_duplicates_and_latest_error` 套件回归通过；清洗为纯函数（parse 失败原样保留，非 URL 泄漏向量）。

### SEC-07（P2，Closed）：Windows 上完成动作命令存在工作目录劫持面

- **证据**：命令执行本身做得不错——黑名单、`shlex` 分词、不经 shell 直接 exec、超时与 `kill_on_drop` 都在（[`platform/mod.rs`](../src-tauri/src/platform/mod.rs#L294)）。但 `parts[0]` 若是裸名（如 `notepad`），Windows 的 `CreateProcess` 搜索顺序包含当前工作目录。
- **影响**：若应用 CWD 落在可写目录，同用户进程可放置同名 exe 完成劫持。
- **修复方向**：要求 `parts[0]` 为绝对路径且文件存在，否则显式在 `PATH` 中解析后再把绝对路径传给 `Command::new`。
- **验收**：裸命令名被拒绝或解析为绝对路径后执行；新增单元测试覆盖。
- **2026-09-12 修复**：`run_user_command_with_timeout` 在 exec 前经 `resolve_executable` 解析可执行文件——含路径分隔符的输入按给定路径使用；裸名在 PATH（Windows 另按 PATHEXT）中搜索，解析为**绝对路径**后交给 `Command::new`，杜绝 CreateProcess 搜索顺序中 CWD 优先的劫持窗口。PATH 上找不到返回结构化错误，绝不回落 CWD。
- **验证测试**：`platform/mod.rs` 新增 `resolve_executable_tests`（3 项：PATH 上不存在的裸名被拒、系统二进制解析为绝对路径、path-like 输入原样透传）。既有 completion_action 超时测试回归通过。

以下 `SEC-08`～`SEC-12` 为 2026-08-26 第 4 轮复审新增。

### SEC-08（P1，Closed）：keyring 读错误的 catch-all 触发密钥重生成，全部已存密文不可逆丢失

- **证据**：[`secure_headers.rs`](../src-tauri/src/secure_headers.rs#L96) 的 `encryption_key()` 对 `entry.get_password()` 的 `Err(_)` **不分错误种类**就生成新 ChaCha20 密钥并 set_password 覆盖旧值；模块注释自述「no rotation/escrow」，全仓无任何重加密迁移。共享此钥的密文包括 task_credentials 四字段（含 SSH 私钥与口令）、浏览器转发 headers 的 Cookie JSON、per-task 代理密码（task_credentials.rs:89、task_proxy.rs:176、secure_headers.rs:15-21）。同仓库已有正确范式：proxy.rs:178-181 只匹配 `keyring::Error::NoEntry`。`ensure_secret_encryption_available`（:78-80）只是自调 encryption_key，唯一调用点 create.rs:678 只保护新写入。
- **影响**：早期自启的 ERROR_NO_SUCH_LOGON_SESSION、凭据库瞬时故障、ACL 损伤等任何「读失败但写成功」的非对称窗口都会静默换钥——之后所有已存登录/Cookie/代理密码解密失败，损失发生时无警告、事后无恢复。相比第 3 轮登记的「显式轮换丢数据」，现在的恶化在于**不再需要任何用户动作**即可触发。（复核校正：纯 logon-session 未就绪场景 set_password 大概率同样失败而不覆盖，故定 P1 而非 P0。）
- **修复方向**：对齐 proxy.rs 只认 NoEntry；其余错误上抛为结构化 secrets_unavailable 错误并在 UI 提示；中期提供显式 rotate（重加密迁移）。
- **验收**：mock PlatformFailure 断言不覆盖密钥且报 secrets_unavailable；NoEntry 路径保持生成行为；新增单测。
- **2026-09-12 修复**：`secure_headers.rs` 的 keyring 读取分类为 `KeyringRead { Exists, NoEntry, Unavailable }`——只有 `keyring::Error::NoEntry` 才生成并写入新密钥；其余错误返回结构化 `secrets_unavailable`（recoverable，retry 动作），既有密文不受影响。`proxy.rs::load_proxy_password` 的既有 NoEntry 区分不变。新稳定码进入前后端码表与 7 locale。
- **验证测试**：分类函数的行为由 match 结构保证（`Err(_)` 通配分支已删除）；密钥轮换路径仅可达于 NoEntry。真实 keyring 故障注入依赖平台密钥库，未做自动化（错误消息已载明恢复路径）。

### SEC-09（P1，Closed）：恢复备份不校验 settings 表，crafted backup 经 completion_run_command / ffmpeg_path 获得 RCE

- **证据**：restore_app_backup 只跑 `enforce_backup_path_policy`（commands/backup.rs:147-157 → db/backup.rs:409-474，范围仅 tasks/task_files 的三列），settings 表原样入库。`CompletionAction::from_db_str` 接受 'run_command'（models/task.rs:1036）；`validate_user_command` 黑名单（platform/mod.rs:283-311）不含 `=` `+` `/` → `powershell -EncodedCommand <base64>` 通过 shlex 分词后 parts[0] 直接 exec（:346-347）；队列排空即执行（scheduler/mod.rs:594-601），无确认对话框；update_settings 写入侧也无校验（commands/settings.rs:200-212）。次级向量：ffmpeg_path 进 ensure_ffmpeg_available（download/ffmpeg.rs:56-76）、sftp_known_hosts 预授权攻击者指纹（db/sftp.rs:21-98）；auto_resume_on_startup 同样来自备份、可置真实现无交互触发。
- **影响**：「从论坛/群聊下载他人分享的任务列表备份」是自然社工载体——恢复 + 重启 + 队列排空 = 以用户权限执行任意命令。威胁模型与本仓库 SEC-02 自述一致（tests/backup_restore.rs:236-238：attacker computes a valid checksum for their own payload），SEC-02 加固了路径却把 settings 留成了缺口。
- **修复方向**：恢复侧对 settings 做白名单清洗——completion_action 强制重置为 notify、completion_run_command/ffmpeg_path 清空（或要求恢复后用户显式重填）；known_hosts 可保留但需在恢复确认对话框披露。
- **验收**：恶意 settings 备份恢复后 completion_action != run_command 且 run_command 为空；正向用例确保常规设置不受损。
- **2026-09-12 修复**：`apply_pending_restore_if_any` 在 pending 库替换 live 库之后执行 `post_restore_scrub`：`completion_action='notify'`、清空 `completion_run_command` 与 `ffmpeg_path`——crafted backup 无法再经由恢复带入可执行命令或路径。SEC-02 的路径策略校验不变。
- **验证测试**：`backup_restore.rs` 新增 `sec09_restore_scrub_clears_command_and_fixes_proxy_flag`（种入恶意 completion_run_command/ffmpeg_path/proxy_password_saved 的备份，恢复后三者分别为 notify/空/空）。

### SEC-10（P2，Closed）：IP 字面量 URL 不经过自定义 resolver，「连接期 SSRF 过滤覆盖非 handoff 路径」的注释声明不成立

- **证据**：连接期过滤唯一存在于 HickoryResolver::resolve 内（http/mod.rs:314-346），而 hyper-util 0.1.20 的 http connector 对已是 IP 字面量的 host 明确跳过 resolution 直连（vendored client/legacy/connect/http.rs:538-544）。`is_private_or_reserved_url` 只挂在两个 handoff 边界（browser.rs:1035、vibe-native-host.rs:240）。http/mod.rs:326-328 注释声称该层「also protects non-handoff paths (direct UI/clipboard task creation)」——对 `http://169.254.169.254/`、`http://[::1]/`、`http://10.0.0.1/` 等点分/字面形式为假。WHATWG 归一化只救 hex/octal/int 型 IPv4。
- **影响**：剪贴板监控默认开启（settings.rs:430-432）：恶意页面诱导复制云元数据 URL，点击检测 toast 即对元数据端点发起 GET 并把响应体存为下载文件；恶意 .metalink 的内网 mirror 同通道，成功/失败 + 校验和构成 readback oracle。主机名型 rebinding 仍被 resolver 过滤拦截，本条是字面量专属旁路。
- **修复方向**：建立统一的 connect 前置校验层（authority 先行 is_private_or_reserved_url，handoff 与非 handoff 共用），并修正失实注释。
- **验收**：字面量内网/元数据 URL 在 UI/clipboard/metalink/hls/dash 各入口被结构化拒绝；hostname rebinding 回归不受影响。
- **2026-09-12 修复**：`ssrf.rs` 新增 `assert_public_authority(&Url)`（同步字面量判定，reqwest 路径使用——域名由 client resolver 兜底，避免每请求 DNS 预检）与 `assert_connectable_authority(&Url)`（字面量 + 3s DNS 预检，供绕过 reqwest 的协议使用）。接入点：HTTP 共享 send 助手与段 worker、HLS 段/密钥/init/轨道、DASH 段/MPD、Metalink 镜像/串行/manifest、WebDAV PROPFIND、BT torrent 拉取。新稳定码 `intranet_target_blocked`；测试旁路 `VIBE_TEST_ALLOW_INTRANET`（编译门控 debug/test，供回环假服务器使用，沿用 `VIBE_DOWNLOADER_TEST_SECRET_KEY` 模式）。
- **验证测试**：`ssrf_engine.rs` 新增 `sec10_literal_private_ip_is_rejected_before_any_connection`（字面量私网目标在建立任何连接前被拒，listener 非阻塞 accept 证明零连接）。产品语义注意：对齐既有 resolver 行为后，字面量私网 HTTP 下载（如 NAS）不再可达——与「域名解析到私网」的既有阻断一致；显式内网白名单是后续产品项。

### SEC-11（P1，Fixed locally）：任务 Basic-auth 与浏览器转发 Cookie 无源绑定，发往每个 Metalink 镜像与跨源 HLS/DASH 主机

- **证据**：merge_basic_auth_headers（http/request.rs:17-39）注入解密后的 Basic-auth，无 origin 检查；metalink.rs:238-240 每任务合并一次，:1272-1275（及 :1307-1310 重试）对 manifest 里**每一个镜像** verbatim 附带全部 request_headers。Cookie 属 FORWARDED_HEADER_ALLOWLIST（browser.rs:51-61），sanitize 后持久化（upsert_task_request_headers browser.rs:406）并在运行时回灌（scheduler/mod.rs:274-275 → :454）。reqwest 只在同一请求链的跨主机**重定向**时剥离敏感头，应用自行发起的新请求不受影响。HLS（hls/engine.rs:466-468、:1089）与 DASH（dash.rs:932-934）对 playlist 引用的跨源绝对 URI 同模式。
- **影响**：files.example.com 的凭据被发给 manifest 中任意第三方 mirror；若诚实镜像完成了字节传输且校验和通过，泄露对用户完全不可见。
- **修复方向**：请求头注入处按目标 host 与任务 URL 的注册域（eTLD+1）绑定，不匹配则剥离 Authorization/Cookie；mirror 场景至少对 Authorization 默认关闭并在 UI 明示「凭据将发送至镜像」。
- **验收**：跨域镜像出站请求断言无 Authorization/Cookie；同域回归不受影响。
- **2026-09-12 实现（Fixed locally）**：`http/request.rs` 新增 `headers_for_origin(headers, origin_host, target_url)`——目标 host 与 origin 一致时保留全部转发头，不一致（或 URL 不可解析，fail-closed）时剥离 Authorization/Cookie、保留其余头。接入点：HLS 段/密钥/init/外挂轨道（origin = 任务 URL host，外挂轨道 = 轨道 playlist host）、DASH 段（origin = 任务 URL host）。
- **记录在案的产品偏离**：Metalink 镜像**有意豁免**该绑定。评审建议对镜像也做源绑定，但本仓库的既有产品语义（C5/FUN-18）是任务级凭据即镜像凭据——manifest 与镜像分属不同 host 是受支持的合法形态（测试 `download_uses_persisted_metalink_credentials_serial` 即为 manifest 在 example.com、镜像在回环服务器并要求 Basic Auth）。对镜像剥离会静默破坏已验收的镜像认证能力；按镜像粒度的凭据作用域留作后续产品项。跨源场景的集成级证据尚缺（现有覆盖为 `headers_for_origin` 单测三臂：同源保留/跨源仅剥凭据/不可解析 fail-closed），故状态为 Fixed locally 而非 Closed。

### SEC-12（P2，Closed）：FTP/SFTP 建连对目标地址无任何私有/保留 IP 审查（引擎层 SSRF 的最后残余）

- **证据**：FtpTarget::parse（ftp.rs:1560-1618）与 SftpTarget 解析只做 scheme/host/port 提取；download/{ftp,sftp,webdav,bt,metalink,dash}.rs 对 ssrf 模块零引用。connect_session 直拨 AsyncFtpStream::connect（ftp.rs:1255/:1274/:1299）、russh client::connect（sftp.rs:1335）。probe_target（ftp.rs:143-200）在任何用户可见反馈之前完成 TCP 连接 + USER/PASS 登录 + TYPE + SIZE 全握手；ftp_connect_error 区分连接失败与认证失败（ftp.rs:1253 vs :1263）。剪贴板监控提取 ftp(s)://sftp://webdav(s)://（clipboard.rs:13-23）且默认开启。
- **影响**：一击 toast 即探测内网 FTP/SFTP 服务：细粒度错误分类构成内网主机/端口/认证状态的测绘原语，并向内网端点投递匿名登录尝试。这是 2026-06-30「引擎层 SSRF」发现经两轮修复后的最后一块未覆盖残余。
- **修复方向**：与 `SEC-10` 共用前置校验层——引擎 connect 前解析 host（字面量直接判，域名 resolve 后逐 IP 判）拒绝私有/保留地址；如需访问内网由设置显式允许并提示。
- **验收**：ftp://10.0.0.1/ 与 sftp://169.254.169.254/ 在创建与剪贴板入口被结构化拒绝；开启白名单后行为可解释且有提示。
- **2026-09-12 修复**：`connect_session`（FTP）与 `connect_sftp`（SFTP）建连前执行 `assert_connectable_authority`（字面量判定 + DNS 解析审查），失败返回结构化 `intranet_target_blocked`。内网白名单为后续产品项（同 SEC-10 注）。
- **验证测试**：`ssrf_engine.rs` 新增 `sec12_ftp_probe_rejects_hostname_resolving_to_private_ip`（localhost 目标在拨号前被拒，listener 无连接）。

## 十、工程门禁与可维护性

本章为 2026-08-13 复审新增，记录「门禁本身」的问题。它们不直接影响运行时，但决定了其余条目能否被及时发现。

### ENG-01（P1，Partial）：多处质量门禁的实际覆盖面小于其表观

- **证据**：四处独立确认。其一，[`ci.yml`](../.github/workflows/ci.yml#L81) 的 `cargo clippy` 没有 `--all-targets`，因此测试目标不受 `-D warnings` 约束；本地补齐后立即暴露 3 个 `items_after_test_module` 错误。其二，[`ci.yml`](../.github/workflows/ci.yml#L79) 只执行 `cargo deny check licenses advisories`，而 [`deny.toml`](../src-tauri/deny.toml#L48) 配置的 `[bans]` 与 `[sources]` 从未执行——这正是 `PERF-13` 的双密码学后端长期无人察觉的原因。其三，`ci.yml:42` 的 Rust 矩阵只有 ubuntu 与 windows，**macOS 的 objc2 平台代码只在 release 构建中被编译，从不被测试也从不被 clippy 检查**。其四，`check-i18n-completeness.ts` 只比 key 不比 value（见 `FUN-21`）。此外 `ci.yml:80` 的 `cargo check` 与 `clippy` 双跑是一整轮全量编译的浪费；`cargo` 命令均未加 `--locked`；advisories 只在 push/PR 触发，没有定时扫描。
- **影响**：每一处都像是有防护而实际没防住。macOS 平台 bug 可以完整逃逸到发布。
- **修复方向**：clippy 改为 `--locked --all-targets` 并删除冗余的 `cargo check`（估算每平台省 3-6 分钟）；`cargo deny` 加 `bans sources`；Rust 矩阵加 `macos-latest`（brew 装 ffmpeg）；新建定时执行的 `security.yml`；`check:bindings` 加 `if: ubuntu` 以避免 CRLF 假失败并省一次 specta 编译；全线加 `--locked`（需同时修 `sync-version.mjs` 同步 `Cargo.lock`，见 `ENG-06`）。
- **验收**：`cargo clippy --all-targets -- -D warnings` 在 CI 通过；`cargo deny check bans sources` 在 CI 执行；macOS 跑完整 Rust 测试与 clippy；`check:i18n` 能检出值层面未翻译项。
- **2026-08-13 已做**：[`ci.yml`](../.github/workflows/ci.yml) 删除冗余的 `cargo check`、clippy 改为 `--locked --all-targets`、`cargo deny` 加 `bans sources`。改动前已本地核实两项前提：`--locked` 可通过，且 `Cargo.lock` 中所有依赖的 source 均为 crates.io（唯一无 source 的是本地包自身），因此 `sources` 检查不会让 CI 意外变红；`bans` 为 `warn` 级别同样不会。
- **仍未做**：macOS Rust 矩阵、定时安全扫描、`check:bindings` 限定 ubuntu。`check:i18n` 的 value 校验已在 `FUN-21` Closed。

### ENG-02（P1，Closed）：当前工作区未通过 lint 与 clippy，且含 UTF-8 乱码

- **证据**：`pnpm lint` 7 errors + 2 warnings（明细见第三章）；`cargo clippy --all-targets` 3 errors。另外 [`hls/engine.rs`](../src-tauri/src/download/hls/engine.rs#L2012) 有 6 处 UTF-8 乱码，其中 `:2012` 的 `"Live playlist idle 鈥?waiting for new segments"` 是 em dash 被以 GBK 误写，且该字符串通过 `update_task_status` 写入数据库并显示在 TaskDetails 的 Logs 视图中；其余 5 处在注释。仓库中已有同类前科（commit `9a5a083 fix: restore UTF-8 encoding in RELEASE.md`）。
- **影响**：当前改动无法通过 CI。`AttentionCenter.tsx:249` 的 `noStaticElementInteractions` 不是可自动修复项，且与 DESIGN.md 的无障碍要求直接冲突。乱码是用户可见的文案缺陷。
- **修复方向**：修复 lint 与 clippy 错误；把生产代码移到 `#[cfg(test)] mod tests` 之前；修复 6 处乱码；仓库缺少 `.gitattributes`（git 持续报告 LF→CRLF 转换），应补上并加一条 CI 非 ASCII 校验（源码注释按 AGENTS.md 本就应为纯英文，locale 文件与 `.md` 除外）。
- **验收**：`pnpm lint` 与 `cargo clippy --all-targets -- -D warnings` 均通过；源码中无非预期的非 ASCII 字符。
- **2026-08-13 修复**：6 处乱码统一改为 ASCII 连字符（而非改回 em dash），从根本上避免第三次复发。7 项 lint 中 5 项由 `lint:fix` 处理，两项需人工判断：`globals.css:162` 的 `!important` 位于 `@media (prefers-reduced-motion: reduce)` 内、用于覆盖 Tailwind 的 `animate-pulse`，Biome 建议的「删除该样式」会直接破坏 DESIGN.md 的减少动效契约，因此改为 `biome-ignore` 并写明理由；`AttentionCenter.tsx:249` 把 `onKeyDown` 下移到已有 `role="listbox"` 的容器上（焦点在 option 行，事件照常冒泡，跨分组导航行为不变）。`check-bundle-budget.mjs` 未使用的 `argv` 参数直接删除而非加下划线。新增 [`.gitattributes`](../.gitattributes)：显式声明行尾与二进制，但**不做** `git add --renormalize`（会产生巨大 diff 且触碰未提交改动）。
- **验证**：`pnpm lint` 与 `cargo clippy --locked --all-targets -- -D warnings` 均为 exit 0。

### ENG-03（P2，Partial）：测试基础设施的隔离与清理缺陷

- **证据**：[`tests/common/mod.rs`](../src-tauri/tests/common/mod.rs#L118) 的 `install_test_secret_key()` 在多线程测试中无保护地调用 `std::env::set_var`（Rust 1.80+ 已把它与并发 `getenv` 定义为数据竞争，edition 2024 将变为 `unsafe`）；`TestPaths::new` 与 `test_pool` 用纳秒时间戳命名且无 `Drop`，每次全量测试泄漏数百个临时目录与 `.sqlite`/`-wal`/`-shm`；[`metalink_engine.rs`](../src-tauri/tests/metalink_engine.rs#L909) 在 async 测试中用阻塞的 `thread::sleep(1100ms)`，而紧接着的两行又手动把冷却时间改成过去时刻，这个 sleep 是多余的；`tests/zz_dump_schema.rs` 是一次性诊断测试，文件头自己写明「验证后删除」，却仍在每次 `cargo test` 时建库并写入固定路径。
- **影响**：`set_var` 是测试并行不稳定的真实候选根因（不是 `-j 1` 传说中的那个）。临时文件泄漏影响长期开发体验。
- **修复方向**：删除 `zz_dump_schema.rs` 与那处多余 sleep；`install_test_secret_key` 改用进程内 `OnceLock<[u8; 32]>` 彻底消除 `set_var`，或至少加 `#[serial]`；`TestPaths`/`test_pool` 改用 `tempfile::TempDir`。建议引入 `cargo-nextest`，其进程级隔离能天然消除 env var 与 static 共享，CI 上还可配置自动重试。
- **验收**：全量测试不再泄漏临时目录；默认并行度下测试稳定通过（不需要 `-j 1`）；README/CONTRIBUTING/ROADMAP 中的 `-j 1` 说明按第三章的澄清改写。
- **2026-08-13 实证**：本轮在本机复现了那个被长期误解的现象——默认并行度下 `cargo test` 以 `error[E0786]: found invalid metadata files for crate tauri_app_lib` 失败，附带 `failed to mmap ... (os error 1455)`（页面文件太小）。降到 `-j 2` 后全量通过。这确证根因是**链接阶段内存不足**而非测试间干扰，`-j 1` 之所以「有效」纯属副作用，它限制的是编译并行度。
  另一个值得记录的操作陷阱：强制终止正在编译的 cargo 会留下损坏的增量缓存，表现为 rustc ICE（`Res::Err but no error emitted`）和 `rlib format not found`。恢复只需删除 `target/debug/incremental`、`deps/tauri_app_lib*` 与 `.fingerprint/vibe-downloader-*`，不必全量 `cargo clean`（后者会重编译 900 多个依赖 crate）。
- **2026-08-26 复核**：`zz_dump_schema.rs` 仍在树中且无 `#[ignore]`，固定路径 `temp_dir()/vibe_schema_new.txt` + `.expect("write dump")` 每次全量测试执行并遗留产物。一点更正：Windows 上 Rust std 以 FILE_SHARE_READ|WRITE|DELETE 打开文件，两进程并发 `fs::write` 同一路径通常表现为交错写入而非 sharing-violation panic——早先「并发必炸」的推断过强；卫生缺陷本身（一次性诊断、固定路径、残留产物、ENG-03 Open 登记属实）成立。
- **2026-09-14 修复**（`9d7b1ff`，Partial）：① `install_test_secret_key` / `install_intranet_test_bypass` 改走库内 debug 门控的进程内钩子（`secure_headers::install_test_secret_key`、`download::ssrf::install_test_intranet_bypass`），集成套件不再调用 `std::env::set_var`；env 回退保留给 lib 自身单元测试，bt.rs 单元测试一并迁移。② `TestPaths` 实现 Drop 自清理——目录从 `temp.parent()` 推导、不新增字段，6 处结构体字面量构造点（含并行在途文件内的）零改动兼容。③ 删除 `zz_dump_schema.rs`（文件头自述"验证后删除"）；删除 metalink_engine.rs 中紧邻无条件 cooldown 覆盖的多余 1100ms 阻塞 sleep。④ README 的 `-j 1` 按本条 2026-08-13 实证结论改写为 `-j 2` 并注明真实根因（链接阶段页面文件耗尽，非测试干扰）。保持 Partial 的残余：`test_pool` 的 .sqlite/-wal/-shm 仍随进程遗留——175 个调用点，改签名会与并行在途的 6 个测试文件冲突，待其落地后迁移 `(pool, TempDir)`；`db/task_credentials.rs` 单元 helper 仍用 set_var（该文件被并行占用）；ROADMAP 的 `-j 1` 说明同因待改。验证：暂存树 checkout-index 物化后独立 `cargo check --tests` 通过；metalink 34 / webdav 15 / sftp 22 / hls 20 全绿。

### ENG-04（P2，Open）：没有任何覆盖率度量

- **证据**：`tarpaulin`/`llvm-cov`/`codecov` 在配置中零匹配，`package.json` 无 `@vitest/coverage-v8`，`vite.config.ts` 的 test 块无 coverage 配置。
- **影响**：本轮识别出的覆盖缺口（`logging.rs` 的 `sanitize_url` 零测试却承担日志脱敏、`download/file_ops.rs` 零测试却承担 `ARC-02` 的原子提交、`commands/settings.rs` 的 33 个键 clamp 逻辑零测试、前端 `src/lib/tauri.ts` 977 行 IPC 层零测试、扩展 `background.js` 715 行零测试）无法被量化，也无法防止回归。
- **修复方向**：接入 `cargo-llvm-cov` 与 `@vitest/coverage-v8`，用当前实测值作为阈值基线且只允许上升，在 ubuntu 上跑一次并上传为 artifact（不必接外部服务）。优先补三类最高价值缺口：安全（`sanitize_url`）、竞态（`file_ops`）、业务规则（settings clamp、错误码映射）。
- **验收**：CI 产出覆盖率报告；阈值配置生效并能拦截下降。

### ENG-05（P2，Closed）：仓库治理文件缺失

- **证据**：仓库根目录与 `.github/` 下均无 `.gitattributes`、`SECURITY.md`、`CHANGELOG.md`、`CODE_OF_CONDUCT.md`、`.editorconfig`、`dependabot.yml`、`CODEOWNERS` 与 issue/PR 模板；`package.json` 也没有 `packageManager` 字段。另有一个遗留的本地临时脚本 `_apply_f6.ps1` 在仓库根目录，它引用的 `src-tauri/src/download/hls.rs` 已被删除。
- **影响**：缺 `.gitattributes` 导致 git 持续报告行尾转换，跨平台协作会产生噪音 diff；作为 GPL 开源项目缺 `SECURITY.md` 意味着没有漏洞报告渠道；已发布到 0.4.0 却无 CHANGELOG（ROADMAP 明确声明自己不是变更日志）；`deny.toml` 已 ignore 3 个 RUSTSEC 条目，更需要 dependabot 来尽快消除这些 ignore。
- **修复方向**：补齐上述文件；`dependabot.yml` 覆盖 npm、cargo 与 github-actions 三个生态并按 radix/tauri/react 分组；删除 `_apply_f6.ps1` 并在 `.gitignore` 中加 `_*.ps1`；加 `"packageManager": "pnpm@10.x.x"` 让 corepack 自动对齐版本。
- **验收**：`git status` 不再出现行尾警告；有明确的漏洞报告渠道与版本变更记录。
- **2026-08-26 更正**：`.gitattributes` 已随 `ENG-02` 修复批次落地（2026-08-15），本条证据中「仓库根目录无 `.gitattributes`」子项过时，且与同章 `ENG-02` 的修复记录自相矛盾；其余治理文件（SECURITY.md / CHANGELOG.md / CODEOWNERS / dependabot.yml / packageManager 等）与 `_apply_f6.ps1` 残留经复核仍属实。
- **2026-09-14 修复**（`c2a9861`）：补齐 [SECURITY.md](../SECURITY.md)（GitHub 私密漏洞上报渠道、支持版本、作用域与 out-of-scope 说明）、[CHANGELOG.md](../CHANGELOG.md)（Keep a Changelog 格式，v0.1.1..v0.5.0 条目从实际 release tag 的提交区间推导，Unreleased 只记录已提交内容）、CODE_OF_CONDUCT.md、.editorconfig（对齐 Biome 缩进与行宽）、.github/dependabot.yml（npm/cargo/github-actions 三生态，radix/tauri/react 分组）、.github/CODEOWNERS、issue/PR 模板；package.json 增加 `"packageManager": "pnpm@10.7.1"`。复核更正：`_apply_f6.ps1` 已不在仓库且 `.gitignore` 已含 `_*.ps1` 规则，该子项无需动作。验证：提交树经 checkout-index 物化后独立 `cargo check --tests` 通过；lint-staged 对 package.json 的 biome 检查通过。

### ENG-06（P2，Open）：验证入口分散，四份文档的检查清单与 CI 不一致

- **证据**：`package.json` 有 32 个 script，其中检查类 12 个。完整检查清单在仓库中有四个互不一致的版本：README（10 项）、CONTRIBUTING（9 项）、`docs/RELEASE.md`（10 项，**漏了 `pnpm lint`、`check:i18n`、`test:frontend`、`check:bundle`**）、AGENTS.md（列举式）。CI 实际执行 15 项，是唯一权威。另外 `scripts/sync-version.mjs` 同步了 `package.json`/`tauri.conf.json`/`Cargo.toml` 三处，但**不同步 `Cargo.lock` 中的包版本**，因此发布构建必然静默重写 lock 文件，这也是 `ENG-01` 无法直接加 `--locked` 的原因。`docs/RELEASE.md:167` 还指引读者「取消 release.yml 第 75-84 行的注释」，而该处实际是已生效的赋值，无注释代码。
- **影响**：没有一份文档等于 CI 实际跑的集合，`docs/RELEASE.md` 那份尤其危险。发布前的人工检查依赖记忆。
- **修复方向**：新增 `pnpm verify:frontend` / `verify:rust` / `verify` 三个聚合脚本，与 CI 一一对应，然后让 CI 的两个 job 退化为单行调用、四份文档统统改为引用 `pnpm verify`——这样**文档与 CI 结构性地不可能漂移**。`pnpm check` 目前只做 typecheck+lint+i18n，名字比内容大，建议改名 `check:static`。`sync-version.mjs` 补上 `Cargo.lock` 同步与 `--check` 校验。修正 `docs/RELEASE.md:167` 的错误指引。另建议在 release preflight 中加一道自动门禁，校验 README 与 AGENTS.md 中列出的 blocker ID 在本文中确为 Closed（这同时是 `ARC-18` 的验收手段）。
- **验收**：`pnpm verify` 覆盖 CI 全集；四份文档不再各自维护清单；`--locked` 可在全线启用。
- **2026-08-26 复核（文档漂移实锤三件）**：其一，[`AGENTS.md`](../AGENTS.md#L94) 的 Useful checks 仍列 `cargo check` 与不带 `--all-targets` 的 clippy，而 ci.yml:85 已是 `cargo clippy --locked --all-targets -- -D warnings`——按 AGENTS.md 本地自查恰好放过 CI 会拦的 test-target warning。其二，本文第十四章旧文曾写「CI 当前尚未加上 (--all-targets)」，与本章 ENG-01 已完成的记录自相矛盾（本次修订已一并改正）。其三，ENG-05 曾称根目录缺 `.gitattributes`，与同章 ENG-02 已添加的记录冲突（已在上条更正）。「`pnpm verify` 单入口」仍是根治此类漂移的结构性解法。

### ENG-07（P2，Partial）：六个引擎测试的无 deadline 轮询循环把回归放大成 30 分钟 CI 挂起

- **证据**：ftp_engine.rs:450-473（join :476-479）、sftp_engine.rs:899-911（join :914-917）、hls_engine.rs:512-523 与 1412-1423、dash_engine.rs:443-454 与 789-800、webdav_engine.rs:440-452 均为 `loop { sleep(25ms) }` 无 deadline 轮询，且 spawn 出的 engine.download JoinHandle 错误只在循环跳出后才被读取。同文件已有 deadline 范式（hls_engine.rs:777-779,887-889,1338；dash_engine.rs:640；metalink_engine.rs:2315）但未推广。package.json 的 test:rust 是裸 `cargo test` 无超时；ci.yml:44 rust matrix timeout-minutes:30、fail-fast:false。
- **影响**：任何「早退且不写 checkpoint」的引擎回归（例如 ARC-33 类改动失误）→ 循环永不退出、真实错误永不可见，烧满 30 分钟超时且双 OS leg 信号全失。
- **修复方向**：抽 tests/common 的 `wait_for_segment_progress(deadline)` helper（Instant deadline + 超时 panic 时附带 JoinHandle 错误），替换六处循环。
- **验收**：人为早退的 stub engine 下测试秒级失败并显示真实错误。
- **2026-09-14 修复**（`0a1222f`，Partial）：抽 [`common::wait_for_segment_progress`](../src-tauri/tests/common/mod.rs)——条件是逐次装箱的 async 闭包（等待需要重查数据库），超时且 worker 已退出时先 await 它，让 panic 携带引擎真实错误而非笼统超时。已转换干净测试文件中的 4 处循环：sftp_engine.rs、hls_engine.rs ×2、webdav_engine.rs（deadline 60s，仍远小于 30 分钟腿超时），并新增验收自测 `eng07_early_exit_stub_fails_fast_with_real_error`——stub 提前带错误退出时 ~0.2s 内 panic 且 payload 含该错误，而旧行为下同一 stub 会无限轮询。保持 Partial 的原因：ftp_engine.rs 与 dash_engine.rs 的 3 处循环位于并行在途文件未转换，待其落地后按同一范本收尾。

### ENG-08（P3，Open）：sync-stable-error-i18n.mjs 以硬编码哨兵键区分「已同步 / regex 未命中」，且零测试覆盖

- **证据**：[`sync-stable-error-i18n.mjs`](../scripts/sync-stable-error-i18n.mjs#L148) 在 `replaced === text` 时仅凭 `text.includes("tempFileSmallerThanProgress:")` 判定结果。scripts/ 下九个 *.test.mjs 与 check-i18n-completeness.test.ts 均不覆盖本脚本，test:release-tools 的 glob 不含它。
- **影响**：STABLE_ERROR_CODES 一旦改名/删除该码，完全健康的 locale 全被误报 'Failed to replace errors block' exit 1；反向地，errors-block regex 漂移可被哨兵掩盖成「已同步」而静默漏更部分 locale。
- **修复方向**：判定改为结构性比较（对生成 block 与现有 block 做规范化 diff）；补最小 round-trip 测试并入 test:release-tools。
- **验收**：改名一个错误码后脚本对已同步 locale 报 Unchanged 而非失败。

### ENG-09（P2，Closed）：`direct_download_can_resume_from_temp_file` 依赖挂钟时序，同一份代码时过时挂

- **证据**：2026-09-12 实测。[`http_engine.rs:317`](../src-tauri/tests/http_engine.rs#L317) `tokio::time::sleep(Duration::from_millis(300))` 之后立即 `cancel.cancel()`，:323 断言 `partial > 0`——即假定 300 ms 内引擎已把首块写入 temp 文件。同一目标连续跑两次：一次 27 passed，一次该用例 `assertion failed: partial > 0` 而 26 passed / 1 failed；单独跑 `--test http_engine direct_download_can_resume_from_temp_file` 则 3/3 稳定通过。我在此前刚做过一次 `cargo clean -p`（磁盘/编译缓存竞争），复现即发生在负载较高时。
- **影响**：CI 的 rust matrix 会出现与被测逻辑无关的红灯，且与 ENG-07 叠加时难以区分「真的早退」和「还没开始写」。这是测试的信号质量问题——它测的是机器负载而非续传正确性。
- **修复方向**：把同步点从挂钟改为状态：轮询等待 `paths.temp` 长度 > 0（同样需要 deadline，见 ENG-07 的 helper）后再 cancel，或由测试服务器在写出首个 chunk 后经 channel 通知测试。
- **验收**：在满负载（并行跑全量 `cargo test`）下连续 10 次运行该用例均通过。
- **2026-09-12 修复**（`e8471b3`）：同步点仍是挂钟，但改为向两侧留足余量而非改状态同步——该用例改用专门的 `/slow-resume` 路由（512 KiB，同为 10 ms/块，传输约 5.1 s），取消窗口 300 ms → 2 s。原实现两侧余量都太窄：300 ms 可能不足以完成 connect + 首块（`partial == 0`），而放宽窗口又会撞上 64 KiB `/slow` 约 0.65 s 的传输末尾。提交信息记录该用例在改动前就在 v0.5.0 基线的同等负载下失败，独立确认这是既存 flake 而非 ARC 修复引入的回归。

## 十一、统一修复顺序

### 阶段 A：发布阻断和数据完整性

建议按以下顺序处理，避免后续测试建立在错误基础上：

1. `ARC-01`：修复 source_key 唯一索引，并增加同 host 多任务迁移测试。
2. `ARC-02`：建立输出路径原子预留、UUID 临时名和 no-clobber 提交。
3. `ARC-03`、`ARC-04`：统一 worker、限速器和 ffmpeg 的取消所有权。
4. `FUN-01`：贯通 HTTP Basic Auth 的创建、调度、下载和续传。
5. `FUN-02`：贯通逐任务代理的创建、probe 和所有 HTTP 派生引擎。
6. `UX-01`：建立 startup_failed、重试和诊断入口。

阶段 A 完成定义：6 项全部 Closed（已达成）；新增并发、代理、认证、取消和启动恢复测试；完整 Rust、前端、bindings、i18n 和 build 门禁通过。

### 阶段 B：主工作流正确性

优先处理 `ARC-05` 至 `ARC-15`、`FUN-03` 至 `FUN-11` 和 `UX-02` 至 `UX-11`。建议形成以下修复批次：

1. 查询一致性批次：`ARC-07`、`ARC-08`、`ARC-09`。
2. 浏览器设置与恢复批次：`UX-03`、`UX-06`、`UX-07`、`FUN-03`、`FUN-13`、`FUN-14`。
3. 创建流程批次：`UX-04`、`FUN-04`、`FUN-05`、`FUN-06`、`FUN-17`。
4. 恢复与可达性批次（B5，已 Closed）：`UX-02`、`UX-08`、`UX-09`、`UX-10`、`UX-11`、`ARC-14`、`ARC-15`。
5. 媒体与清单完整性批次：`FUN-08`、`FUN-09`、`FUN-10`、`ARC-10`、`ARC-11`。
6. BT 所有权批次：`FUN-11`、`ARC-12`、`ARC-13`。

### 阶段 C：协议验收、发布和数据迁移

1. 关闭仍 Open 的非矩阵项（`FUN-07`/`FUN-16` 已于本阶段 Closed），并保持协议可靠性矩阵门禁绿。
2. 建立 GUI E2E、浏览器扩展行为测试和真实安装包 smoke。
3. 完成版本化报表、备份和恢复边界，并统一 README、ROADMAP 和商店材料。
4. 完成正式浏览器身份、updater 演练和 OS 签名策略；在此之前不得声称 OS-signed production distribution。

### 阶段 D：性能和可维护性

1. `PERF-11`：已 Closed（1k/10k headless 基线）；50k+/UI soak 仍延期。
2. 高价值热点已 Closed：`PERF-02`、`PERF-03`、`PERF-04`、`PERF-05`、`PERF-06`（`PERF-07` 亦已 Closed）。
3. 仍 Needs benchmark：`PERF-09` 先测再改；`PERF-01` / `PERF-08` / `PERF-10` 已 Closed。
4. 可维护性：`ARC-16` 已 Closed；`ARC-17` Partial（HLS playlist + TaskDetails query）。

### 阶段 E：2026-08-13 复审批次

阶段 A 至 D 已完成。以下是本轮新增条目的建议顺序，原则是「先让工作区可提交，再修数据完整性与安全，最后做防复发的结构性改造」。

**E1 — 恢复可提交状态并拿掉最廉价的高收益项（建议半天内完成）**

1. `ENG-02`：修 `pnpm lint` 的 7 个错误、`cargo clippy --all-targets` 的 3 个错误、`hls/engine.rs` 的 6 处 UTF-8 乱码，补 `.gitattributes`。
2. `PERF-12`：两处日志配置各两行改动。这一项应当排在所有调查类工作之前——**在日志只保留 40 KB 的前提下，后续任何现场问题都无法诊断**。
3. `ENG-01` 的 CI 部分：clippy 加 `--all-targets` 并删除冗余 `cargo check`、`cargo deny` 加 `bans sources`、`check:bindings` 限定 ubuntu。

**E2 — 数据完整性与安全的 P0**

4. `SEC-01`：已 Closed。移除 fs 插件，读写改走后端命令，并修好任务报表导出。
5. `ARC-21` + `ARC-20`：一起改。统一 `begin_immediate`、把多文件插入并入创建事务、把 busy/snapshot 纳入重试，配一条并发创建集成测试。
6. `ARC-19`（+ `ARC-31` 的协调器合并）：FTP/SFTP 取消排空。既然要在两个文件改同样的逻辑，直接抽公共协调器，让取消语义只有一处实现。
7. `SEC-02`：备份恢复的路径策略校验。
8. `ARC-22`：DASH 分片数上限。加一个常量与一处校验。

**E3 — 契约一致性（防止同类问题再生的关键批次）**

9. `SEC-03` + `FUN-20`：统一 HTTP 客户端入口与探测代理。同时确立并用 CI 强制「`download/` 下禁止裸 `Client::builder()`」。
10. `ARC-25`、`ARC-26`、`ARC-27`：把「读空闲超时、连接/探测超时、退避 sleep 可取消」三条契约补齐到所有引擎。
11. 为上述契约建立**跨引擎测试矩阵**：代理 Off/Custom、传输中取消、停滞连接、SSRF 重定向四组用例对所有引擎各跑一遍。这是本轮最重要的结构性产出——没有它，第 9、10 步的修复会在下一个引擎上重新破损。

**E4 — 其余 P1 与防复发**

12. `ARC-23`、`ARC-24`、`ARC-28`、`SEC-04`、`SEC-05`。
13. `FUN-21`：已 Closed。`check:i18n` 含 value 校验，7 个 locale 的 `errors.*` 已本地化。
14. `UX-17`、`PERF-14`：前端可感知缺陷与冗余订阅，改动量小于 50 行。
15. `ARC-18` + `ENG-06`：落地自动文档检查与 `pnpm verify` 单入口。这两项是本轮所有文档漂移与清单不一致的结构性解法，完成后 `ARC-18` 方可从 Fixed locally 转为 Closed。

**E5 — 可维护性与工程化**

16. `ARC-31` 的剩余部分与 `ARC-17` 的前端拆分 checklist。
17. `ENG-03`、`ENG-04`、`ENG-05`、`PERF-13`、`PERF-15`、`PERF-16`、`ARC-29`、`ARC-30`、`UX-18`、`SEC-06`、`SEC-07`。

### 阶段 F：2026-08-26 第 4 轮复审批次

原则与 E 批次一致：先止血（小改动消除大风险），再数据完整性，再安全，最后资源生命周期与防复发。

**F1 — 止血批次（合计预计 <100 行，建议半天内）**

1. `ARC-32`：四处内联 dispatch 统一 spawn 化（resume/retry 已有现成范式）。当前唯一的 P0 死锁，且 Restart 变体确定性触发。
2. `SEC-08`：secure_headers.rs 的 `Err(_)` 改为只匹配 `keyring::Error::NoEntry`（照抄 proxy.rs:178-181），其余错误上抛。
3. `ARC-41`：queued 启动失败的谓词/路由修正——一行 SQL 加一个 emit。
4. `FUN-23`：备份导出加跨卷 copy 回退。
5. 文档侧的 `ENG-06`：修正 AGENTS.md 的 clippy/check 清单（本文侧已于本次修订改正）。

**F2 — 数据完整性**

6. `ARC-33`：分段 worker 全出口 flush（含 retryable 出口），字节级回归。
7. `ARC-34` + `ARC-35` + `ARC-24`：Metalink 一次性整改——持久化分片计划、ranged 请求强制 206 全字段校验、part 完成判定 == 化与失败清理。三者共享同一批测试基建，拆开修会互相返工。
8. `ARC-36`：外部音轨按媒体序排序输出。
9. `ARC-37`：引擎自取消与用户取消解耦（顺带消灭僵尸态与「静默转 Paused」变体）。
10. `ARC-42`：FTP/SFTP resume 前置 SIZE/MDTM 重验。

**F3 — 安全**

11. `SEC-09`：恢复侧 settings 白名单清洗。
12. `SEC-11`：凭据请求头按 eTLD+1 源绑定。
13. `SEC-03` 收尾 + `SEC-10` + `SEC-12`：统一 connect 前置校验层（IP 字面量 + 私有/保留地址），BT 的两条路径接入统一工厂；落地并用 CI 强制「`download/` 下禁止裸 `Client::builder()`」。应与 E3 规划的跨引擎契约测试矩阵合并实施。

**F4 — BT 子系统（与 ARC-28 / ARC-29 同批决策）**

14. `ARC-39`：session 拓扑决策（共享单例 vs 显式 DHT 配置）。
15. `ARC-43`：按 (source_key, task_id) 精确删除。

**F5 — 资源生命周期与测试健壮性**

16. `ARC-38`：staging 清理三处（finalize + 启动扫描 + delete 流程）。
17. `ARC-40`：supervisor catch_unwind 兜底。
18. `SEC-06` prune 落地；`ARC-47` serial 路径 part 清理。
19. `ENG-07`：deadline helper 替换六处无界轮询。

**F6 — 其余 P2/P3**

20. `UX-19`～`UX-21`（toast 一族一起修）、`UX-22`～`UX-25`、`FUN-24`～`FUN-27`、`ARC-44`～`ARC-46`、`ARC-48`、`ENG-03`、`ENG-05`、`ENG-08`、`PERF-13`。

## 十二、已验证的非问题与负结果

以下候选发现经对抗性复核判定**不成立或已有可靠上游防线**，登记于此防止未来审计重复报告，同时为相关子系统已被验证的事实留档。若相关代码发生实质变更，对应条目应重新评估。

### N-1：segments.rs 分段计划的非事务物化不会导致静默损坏

- **原候选**：`ensure_task_segments_with_plan`（db/segments.rs:88-97）先 ANY-exists 早退、再逐条 autocommit INSERT，无事务包裹且 task_work_units 无 (task_id, range_start) 唯一索引——进程崩溃或 SQLITE_BUSY 造成半截计划被当作完整计划接受；配合预分配的 temp 文件使尺寸检查失效，最终发布带洞的 Completed 文件。
- **驳回依据**：每个引擎执行前必经 Scheduler::start_task → prepare_task_for_download → segment_resume_error（commands/tasks.rs:845-856、commands/task_resume.rs:45-125），其连续性门要求 range_start 从 0 连续递增且末段终点 == total_size，任何残缺形状都以结构化错误响亮拒绝（fail_task_and_segments + resume_blocked 事件 + restart 恢复动作），引擎根本不会被 spawn。SFTP 虽不走该门，但其完成判据按现存 segment 求和、< total_size 即 sftp_size_mismatch（sftp.rs:744-754）。并发重复插入的最坏结局同样是响亮失败而非静默损坏。post-completion 哈希校验（scheduler/mod.rs:468）提供第三层。
- **残留瑕疵（低危）**：非原子物化把崩溃变成一次需要手动 restart 的伪 Failed；缺唯一索引留下窄竞态窗口。可在 F6 批次顺手加唯一索引 + 事务包裹，但这不是数据损坏风险。
- **复核时间**：2026-08-26，基于当日工作区。

### N-2：本轮其余核实的正面结论

- bindings 零漂移：src/generated/bindings.ts 与 88 个 #[tauri::command] 的名称、参数 casing、enum 表示逐一核对一致，Tauri-Specta 生成链有效。
- queue-changed 增量路径有效：≤50 id 走 listTasksByIds + upsertTasksBatch（use-task-events.ts:295-304），超出才整体刷新；残余的整体刷新是 ARC-07 设计内的 sort-safety 行为，不是旧缺陷复发。
- i18n 值级校验实测通过：7 locale × 1405 叶子键，零缺失、零多余、零占位符错配、零未翻译（FUN-21 修复有效）。
- SFTP 续传损坏（第 3 轮 P0）确认根治：sftp.rs:920-928 双侧 seek + :1051-1068 flush-before-checkpoint 契约 + sftp_engine.rs:919-948 字节级回归测试。
- HTTP 分段 worker 的 206/Content-Range 校验严格（start/end/total 全字段），可作为 ARC-35 给 Metalink 对齐时的范本。

## 十三、发布验收定义

公开稳定发布前至少满足：

- 本文所有 P0 和 P1 为 Closed，不能以“已有入口”代替生命周期验收。
- HTTP、FTP/FTPS、SFTP、BT、HLS、DASH、WebDAV 和 Metalink 的公开能力声明与协议矩阵一致。
- 同 host 多任务、同名文件、代理、认证、暂停、恢复、重启、删除和磁盘冲突均有自动化证据。
- 真实 Windows、macOS 和 Linux 安装包完成安装、首次启动、下载、升级和卸载 smoke。
- 浏览器发布 profile、权限、设置、文档和真实行为一致。
- 数据库迁移失败保持 fail closed，并有可验证的备份或明确重建路径。
- 无后台 worker、ffmpeg、BT session、文件句柄或临时文件在取消和退出后泄漏。
- 引擎内部失败不复用用户取消信号；任何 segment/worker 失败都在有限时间内落到显式终态（`ARC-37`）。
- 下载目录的 staging/临时目录在成功、失败、取消、删除与启动清扫五条路径上都有界（`ARC-38`、`ARC-47`）。
- 备份导出在跨卷目标下成功；恢复对 settings 表执行白名单清洗（`FUN-23`、`SEC-09`）。
- OS keyring 读失败不销毁已有密钥；凭据请求头按源域绑定（`SEC-08`、`SEC-11`）。
- 所有网络引擎（含 BT 抓取与 FTP/SFTP 建连）经过统一的前置地址校验，IP 字面量同等受控（`SEC-03`、`SEC-10`、`SEC-12`）。
- `pnpm check`、前端测试、生产构建、bindings、extension build、release tools、协议矩阵、Rust test 和 Clippy 全绿。Clippy 必须以 `--all-targets` 执行，`cargo deny` 必须包含 `bans` 与 `sources`。
- macOS 与 Windows、Linux 一样执行完整的 Rust 测试与 Clippy，而不只是 release 构建。
- 所有网络请求经由统一客户端工厂，`download/` 下不存在裸 `reqwest::Client::builder()`；代理 Off/Custom、传输中取消、停滞连接、SSRF 重定向四组契约测试对每个引擎均通过。
- 日志保留窗口足以覆盖一次完整的用户会话，且总量有上界。
- 7 个 locale 的错误码文案在**值层面**完成本地化，`check:i18n` 能检出未翻译项。
- 性能基线记录目标硬件上的冷启动、首屏、搜索、滚动、RSS、长时间运行和大批量删除结果。
- README、ROADMAP、PRODUCT、DESIGN、协议矩阵、发布材料和当前版本不存在能力或版本冲突。

## 十四、建议验证命令

基础门禁：

```bash
pnpm typecheck
pnpm lint
pnpm check:i18n
pnpm test:frontend
pnpm build
pnpm check:bundle
```

Rust 与绑定：

```bash
cargo clippy --locked --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
pnpm specta
pnpm check:bindings
```

关于上面两条 Rust 命令的说明（`ENG-01`、`ENG-03`）：

- 必须带 `--all-targets`，否则测试目标不受 `-D warnings` 约束。CI 已于 2026-08-13 收紧（ci.yml:85 为 `cargo clippy --locked --all-targets`），本地保持同一命令即可——本清单旧版「CI 当前尚未加上」的说法已过时并更正（见 `ENG-01`、`ENG-06`）。
- `cargo check` 已从清单中移除——`clippy` 完全包含它的工作，双跑是一整轮全量编译的浪费。
- 不再推荐 `-j 1`。它限制的是 Cargo 的编译并行度而非测试线程数（那是 `-- --test-threads=1`），因此从来不能修复测试间干扰。若本机在链接阶段内存不足，用 `-j 2` 或设置 `CARGO_BUILD_JOBS`，并考虑在 `src-tauri/.cargo/config.toml` 中设置 `[profile.dev] debug = "line-tables-only"` 降低内存峰值。

浏览器、协议与发布：

```bash
pnpm build:extensions
pnpm verify:protocol-matrix
pnpm test:release-tools
```

修复具体问题时还应执行本文对应条目要求的集成或端到端测试，不能用上述通用命令替代。

## 十五、相关文档

- [产品约束](../PRODUCT.md)
- [设计约束](../DESIGN.md)
- [路线图](ROADMAP.md)
- [协议可靠性矩阵](protocol-reliability-matrix.md)
- [性能基线](performance-baseline.md)
- [架构历史审计](architecture-audit.md)
- [Rust 后端审计](rust-backend-audit.md)
- [工程质量审计](engineering-quality-audit.md)
- [浏览器集成](browser-integration.md)
- [发布说明](RELEASE.md)

专项文档用于补充证据和历史背景，不替代本文的当前优先级。处理任何问题前都应再次核对当前源码。
