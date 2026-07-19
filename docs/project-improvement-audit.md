# 项目改进审计

最后更新：2026-07-19

适用版本：Vibe Downloader `0.3.0`

审计对象：当前工作区的前端、Rust 后端、数据库迁移、协议引擎、浏览器扩展、构建配置、测试与产品文档

状态：当前风险基线，用于后续对话逐项修复

本文是项目当前唯一的全局风险与优先级文档。它不是变更日志，也不会把路线图中的计划写成已实现能力。若历史专项审计、README 或路线图与当前源码冲突，以当前源码和本文的最新复核结果为准。

## 一、如何使用本文

后续修复对话应直接引用问题 ID，例如“修复 `ARC-01` 和 `ARC-02`”。每次修复都必须先重新读取对应代码，因为行号和局部实现可能已经变化。

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

四维判断：

| 维度 | 当前判断 | 首要任务 |
| --- | --- | --- |
| 用户交互便捷性 | 主界面基础扎实，但恢复、右键、规则编辑和操作范围存在断点 | UX-01 至 UX-11 |
| 功能丰富性和完整性 | 功能面宽，但多个已暴露设置和协议能力没有贯通 | FUN-01、FUN-02、FUN-03、FUN-08 至 FUN-11 |
| 架构鲁棒性和稳定性 | 持久化与安全边界较强，但任务所有权、文件提交和查询缓存存在关键竞态 | ARC-01 至 ARC-13 |
| 程序运行效率 | 已有分页、虚拟化和事件节流，但缺规模数据且仍有 O(N)、阻塞和重复 I/O | PERF-01 至 PERF-11 |

## 三、质量门禁实测

本轮验证基于 2026-07-18 至 2026-07-19 的当前未提交工作区：

| 检查 | 结果 | 说明 |
| --- | --- | --- |
| `pnpm typecheck` | 通过 | TypeScript 无类型错误 |
| `pnpm test:frontend` | 通过 | 18 个测试文件、65 项测试通过 |
| `pnpm check:i18n` | 通过 | 7 个 locale 完整 |
| `pnpm build` | 通过 | 生产构建通过 |
| `pnpm test:release-tools` | 通过 | 25 项测试通过 |
| `pnpm verify:protocol-matrix` | 通过 | 协议矩阵结构检查通过 |
| `cargo check --manifest-path src-tauri/Cargo.toml` | 通过 | Rust 编译检查通过 |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | 通过 | Clippy 零 warning |
| `cargo test --manifest-path src-tauri/Cargo.toml -j 1` | 通过 | 421 项 Rust 测试通过 |
| `pnpm lint` | 失败 | 当前未提交改动中有 6 个 Biome 格式错误，涉及 `AppShell.tsx` 和 `en/es/ja/ko/ru` locale |

Windows 默认并行 Rust 测试曾因本机页面文件不足触发 OS 1455 及连锁元数据错误；使用单构建任务后完整测试通过。这不是已确认的代码失败，但若 hosted runner 复现，应限制 Cargo jobs 或提高 runner 资源。

当前自动化盲区：

- 没有 Playwright、WebDriver 或 Tauri GUI 端到端测试。
- `browser/extension-core` 没有行为测试。
- 没有真实安装包启动、升级、卸载和浏览器接管自动化。
- 没有同 host 多任务、同名输出并发、真实代理路由、极低限速取消、ffmpeg 删除和 BT 多任务 session 的集成测试。
- 性能文档没有真实 1k/10k/50k 测量结果。

## 四、应保留的已确认优势

- HTTP probe 使用 HEAD 并在需要时回退 Range GET；主下载路径支持未知大小、Range 分段、动态拆分、重试、checkpoint、恢复验证和最终重命名。
- `EngineRegistry` 已将协议路由与下载实现隔离，多协议共享明确的 `DownloadContext`。
- SQLite 使用 WAL，任务、segments、凭据、代理、校验和、文件和诊断数据均有独立持久化边界。
- 凭据使用 ChaCha20-Poly1305 加密，浏览器 handoff 保持 HTTP/HTTPS、无嵌入凭据、无本地路径控制和 header allowlist 等安全边界。
- 调度器已有最大活动任务、每主机槽位、优先级、计划窗口和完成动作模型。
- 前端 task data、task UI 和 speed history 已拆分，任务列表使用游标分页、虚拟化和增量事件。
- UI 保持密集桌面工具形态，具备命令面板、快捷键、详情抽屉、恢复动作、Tooltip、焦点环、七语言和 8 个 OKLCH 强调色。
- `TaskProgressEmitGate` 将高频进度事件限制到至少 250ms；request diagnostics 已有保留策略。

## 五、用户交互便捷性

### UX-01（P0，Closed）：启动失败不可见且不可恢复

- **证据**：`run_startup_init` 普通错误曾只记录日志；`StartupGate` 在状态查询失败后停止轮询；`StartupState` 无 failed 模式。
- **影响**：初始化失败时用户永久停在加载页；瞬时 IPC 错误也只能重启应用。
- **修复**：`startup_failed` 状态（code/message/logPath/dataPath）；`retry_startup_init` 幂等重试（服务 flags 防 double-spawn）；打开日志/数据目录命令；`StartupFailedPage` + 7 locale；IPC 瞬时错误可 Retry 恢复轮询。
- **验证测试**：`set_failed_transitions_from_initializing`、`begin_retry_only_from_failed_and_blocks_while_in_flight`、`service_flags_are_sticky_for_idempotent_retry`（`startup.rs`）；`StartupGate.test.tsx` 成功/失败/重试/IPC 恢复。
- **验收**：失败页显示本地化原因；Retry 可恢复到 ready；日志入口可用；不会重复启动 scheduler、clipboard 或 browser bridge。

### UX-02（P1，Open）：全局捕获监听器破坏 Radix 自定义右键菜单

- **证据**：[`src/components/shell/AppShell.tsx`](../src/components/shell/AppShell.tsx#L1217) 在 `window` 捕获阶段阻止所有 `contextmenu`；任务菜单依赖 [`TaskContextMenu.tsx`](../src/components/tasks/TaskContextMenu.tsx#L196) 的 Radix Trigger。
- **影响**：任务、空白列表、侧栏、标题栏、状态栏和详情区域的右键入口可能全部无法打开。
- **修复方向**：只在没有自定义菜单的明确区域抑制原生菜单，或在冒泡阶段处理并排除输入、文本选择和 Radix Trigger。
- **验收**：全部自定义区域可右键打开；普通空白区域不出现 WebView 原生开发菜单；输入框和文本选择行为符合产品约束。
- **测试**：使用真实 `contextmenu` 事件的组件集成测试，不能只调用回调。

### UX-03（P1，Open）：浏览器接管设置每次击键保存并禁用整个表单

- **证据**：[`BrowserCaptureControls.tsx`](../src/components/settings/BrowserCaptureControls.tsx#L90) 的输入 `onChange` 直接提交；[`SettingsPage.tsx`](../src/components/settings/SettingsPage.tsx#L976) 每次 IPC 保存期间禁用控件。
- **影响**：域名、扩展名和数值编辑会逐字符卡顿，慢磁盘或忙碌 runtime 下接近不可用。
- **修复方向**：使用本地 draft、300 至 800ms 防抖、顺序合并保存或显式 Save；保存失败保留草稿，不禁用正在编辑的字段。
- **验收**：连续快速输入只提交最终快照；旧响应不能覆盖新值；失败可重试且草稿不丢失。

### UX-04（P1，Open）：导入 `.txt` 后没有进入批量模式

- **证据**：[`NewDownloadDialog.tsx`](../src/components/shell/NewDownloadDialog.tsx#L666) 只设置 `batchInput` 和展开高级区，没有调用 `setMode("batch")`。
- **影响**：内容已读取但被隐藏，单任务 URL 为空，用户无法直接开始导入。
- **修复方向**：读取成功后切换批量模式、聚焦输入并生成预览；读取失败保持原模式并显示就地错误。
- **验收**：选择有效文本文件后立即看到 URL 列表和有效任务数，开始按钮状态正确。

### UX-05（P1，Open）：暂停全部和恢复全部只处理当前已加载子集

- **证据**：[`Palette.tsx`](../src/components/shell/Palette.tsx#L907) 将当前 store 的 `allTasks` 传给批处理；任务 store 会被当前查询替换，首批页大小为 100。
- **影响**：搜索、筛选或未加载更多页时，大量隐藏任务不会被处理，但文案声明为“全部”。
- **修复方向**：新增后端按状态或全局批处理命令；若暂时只处理加载项，文案必须明确数量和范围。
- **验收**：存在超过 100 个任务和活动筛选时，全局命令仍覆盖数据库中的目标集合，并返回成功、跳过和失败数量。

### UX-06（P1，Open）：站点规则缺少草稿、校验和安全删除

- **证据**：[`SiteRulesEditor.tsx`](../src/components/settings/SiteRulesEditor.tsx#L19) 点击 Add 即持久化空规则，每个字段变化立即提交，Done 只退出编辑，删除无确认或 Undo。
- **影响**：空 host、半输入扩展名和误删规则会直接进入持久状态。
- **修复方向**：本地 draft、Save/Cancel、host pattern 和扩展名校验；删除使用确认或 Undo toast。
- **验收**：Cancel 不产生 DB 变化；无效规则不能保存；保存失败保留草稿；删除可恢复。

### UX-07（P1，Open）：Header 转发使用二态和三态两个控件表达同一字段

- **证据**：[`BrowserCaptureControls.tsx`](../src/components/settings/BrowserCaptureControls.tsx#L49) 先用 Switch 压成 enabled/disabled，随后又用 Select 表达 ask/enabled/disabled。
- **影响**：`ask` 在 Switch 中显示为关闭，点击会静默丢失原策略。
- **修复方向**：只保留一个 Ask/Always/Never 三态控件，并明确每个状态的真实行为。
- **验收**：界面只有一个事实源，三态 round-trip 不丢失。

### UX-08（P1，Open）：桌面宽屏详情栏没有可见关闭按钮

- **证据**：[`TaskDetails.tsx`](../src/components/shell/TaskDetails.tsx#L281) 的宽屏 Header 只有标题和路径，关闭按钮只存在于紧凑抽屉。
- **影响**：鼠标用户缺少直接退出路径，命令面板或隐藏快捷键变成必要路径。
- **修复方向**：Header 右侧增加带 Tooltip 和 `aria-label` 的关闭按钮，并把焦点返回原任务行。
- **验收**：鼠标、键盘和屏幕阅读器均可关闭，关闭后焦点位置稳定。

### UX-09（P1，Open）：Toast 的“还有 N 条”会清空全部并提交待撤销删除

- **证据**：[`toast.tsx`](../src/components/ui/toast.tsx#L17) 的 more 按钮调用 `clearToasts()`；[`toast-store.ts`](../src/stores/toast-store.ts#L79) 清理时执行每条 `onAutoCommit`。
- **影响**：用户预期展开消息，实际失去全部 Undo 并立即提交软删除。
- **修复方向**：more 只展开或折叠；Clear all 使用明确文案。带 Undo 的 Toast 应置顶，不能被普通消息挤出可访问区域。
- **验收**：展开不触发 commit；只有超时或明确关闭对应 Toast 才提交删除；多条 Undo 可独立执行。

### UX-10（P1，Open）：Queue Center 的 ARIA 和键盘模型不完整

- **证据**：[`QueueCenter.tsx`](../src/components/workspaces/QueueCenter.tsx#L199) 使用 `listbox`，每个 option 都可 Tab 聚焦，仅处理 Enter/Space，并内嵌多个按钮。
- **影响**：长队列产生大量 Tab 停靠点，缺少方向键导航，屏幕阅读器语义混乱。
- **修复方向**：改用语义列表或表格，或实现完整 roving tabindex listbox；操作按钮与选中项使用 `aria-controls` 关联。
- **验收**：方向键、Home/End、Tab 顺序和按钮读屏语义通过自动化及人工检查。

### UX-11（P1，Open）：结构化错误本地化覆盖不足

- **证据**：[`src/lib/errors.ts`](../src/lib/errors.ts#L41) 只映射少量 HTTP、磁盘和恢复错误，未命中时直接显示后端 message。
- **影响**：非 HTTP 协议的核心失败会显示英文或技术文本，与七语言完整性声明不一致。
- **修复方向**：为稳定错误码建立穷尽映射；用户文案和恢复动作本地化，原始 code/message 放在诊断详情。
- **验收**：所有公开错误码在 7 个 locale 有映射；测试禁止稳定错误码走原始 message fallback。

### UX-12（P2，Open）：窄窗口没有显式排序入口

- **证据**：[`CommandBar.tsx`](../src/components/shell/CommandBar.tsx#L224) 的排序控件在 `md` 以下隐藏，移动工具面板只有过滤项。
- **修复与验收**：在移动工具面板增加排序菜单和当前排序摘要；320px 至 768px 均可通过鼠标和触控完成排序。

### UX-13（P2，Open）：重置全部设置的文案与实际范围不一致

- **证据**：[`SettingsPage.tsx`](../src/components/settings/SettingsPage.tsx#L840) 未重置主题、语言、浏览器接管、站点规则和分类规则。
- **修复与验收**：要么真正覆盖全部设置，要么更名并在确认对话框列出保留项；前后端默认值应由同一合同测试约束。

### UX-14（P2，Open）：部分危险色类名没有对应 token

- **证据**：SiteRulesEditor、ClassificationRulesEditor 和 BrowserCaptureControls 使用 `text-text-danger` 或 `text-text-warning`，全局 token 实际是 `status-danger` 和 `status-warning`。
- **修复与验收**：统一 token；亮色、暗色和 8 个强调色下危险操作仍有足够对比度，且不只依赖颜色表达。

### UX-15（P2，Open）：首次引导的任意关闭都会永久标记完成

- **证据**：[`OnboardingDialog.tsx`](../src/components/shell/OnboardingDialog.tsx#L70) 将 Escape、遮罩和普通关闭都路由到 completed 写入。
- **修复与验收**：区分稍后、跳过和完成；只有显式跳过或完成才持久化，普通关闭应在后续启动重新出现。

### UX-16（P2，Open）：React 启动等待动画不遵守 reduced-motion

- **证据**：[`StartupGate.tsx`](../src/components/shell/StartupGate.tsx#L55) 直接设置无限 animation；`index.html` 的 reduced-motion 规则不覆盖 React 版本。
- **修复与验收**：使用统一 reduced-motion hook 或 CSS variant；开启减少动态效果后只保留静态状态文本。

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

### FUN-03（P1，Open）：浏览器 Header 过期后的官方恢复路径是死路

- **证据**：[`request_headers.rs`](../src-tauri/src/db/request_headers.rs#L47) 删除过期 header 并返回 `auth_headers_expired`；[`browser.rs`](../src-tauri/src/commands/browser.rs#L230) 固定 `allow_duplicate=false`，重复检测覆盖 needs_attention 和 failed，不能把新 header 绑定到原任务。
- **影响**：24 小时后或密钥不可用时，UI 提示“从浏览器重新发送”，但重新发送只会失败。
- **修复方向**：handoff 命中同 URL 且原任务错误为 auth header 类错误时，原子替换 header、刷新 TTL 并重新入队同一任务。
- **验收**：expired 到 resend 到 same-task resume 全流程通过；不能借此覆盖正常活动任务或突破 duplicate 策略。

### FUN-04（P1，Open）：认证 FTP、SFTP、WebDAV 目录探测不接收凭据或代理

- **证据**：[`NewDownloadDialog.tsx`](../src/components/shell/NewDownloadDialog.tsx#L499) 只向目录探测传 URL；后端三个命令也只接收 URL。SFTP 私钥无法嵌入 URL。
- **影响**：认证目录无法使用推荐的加密凭据流程，FTP/WebDAV 临时嵌入 URL 后还可能在清洗后丢失凭据。
- **修复方向**：建立统一 `DirectoryProbeInput`，包含 URL、用户名、密码、私钥和代理，并复用普通 probe 的安全解析与加密边界。
- **验收**：三协议的密码目录、SFTP 私钥目录和 SOCKS5 代理目录均有端到端测试，返回候选不含明文凭据。

### FUN-05（P1，Open）：自动 sidecar 校验和发现与任务完成存在竞态

- **证据**：[`create.rs`](../src-tauri/src/commands/tasks/create.rs#L868) fire-and-forget 启动 sidecar 发现；scheduler 只在 worker 成功结束时执行一次校验。
- **影响**：小文件可能先完成为 NotRequested，随后新增 checksum 永久停在 Pending。
- **修复方向**：在调度前完成发现，或插入 checksum 后检测 Completed 并触发幂等校验。
- **验收**：延迟 sidecar 和小文件组合最终必须进入 Verified 或明确 Failed，不能永久 Pending。

### FUN-06（P1，Open）：MIME 分类规则在真实创建路径永远不命中

- **证据**：[`create.rs`](../src-tauri/src/commands/tasks/create.rs#L678) 调用分类器时传空 MIME，但 probe 的 content type 已可用；分类器依赖 `content_type.starts_with`。
- **影响**：用户配置的 MIME 分类规则看似可用，实际创建任务时不会触发。
- **修复方向**：传递 probe content type 或所选文件 MIME，保持 extension、URL 和 MIME 规则同一优先级模型。
- **验收**：通过真实 create_task 流程验证 MIME 命中、禁用规则和首条匹配行为。

### FUN-07（P1，Open）：关闭计划下载功能不会恢复由计划窗口暂停的任务

- **证据**：[`tasks.rs`](../src-tauri/src/commands/tasks.rs#L245) 在 schedule disabled 时直接返回；设置保存后虽然调用抢占检查，但 `paused_by_schedule` 任务不会恢复。
- **影响**：用户关闭计划功能后任务仍永久暂停，且路线图对当前抢占行为描述过时。
- **修复方向**：禁用时只恢复最新暂停原因为 schedule 的任务，不能恢复用户手工暂停的任务。
- **验收**：enabled 到 disabled 状态转换测试覆盖 schedule pause、manual pause 和并发状态变化。

### FUN-08（P1，Open）：Metalink strongest-hash 选择和完成汇总互相矛盾

- **证据**：[`metalink.rs`](../src-tauri/src/download/metalink.rs#L1305) 的选择顺序先 SHA-256 后 SHA-512；持久化把 SHA-256 设 primary，而完成汇总要求所有 file hash Verified。
- **影响**：多 hash manifest 可能长期 Pending，也不符合“最强算法”声明。
- **修复方向**：定义 SHA-512、SHA-256、SHA-1、MD5 的明确策略；完成汇总只检查 primary，或验证全部并定义冲突规则。
- **验收**：多 hash、单 hash、冲突 hash 和弱算法 fallback 测试均有确定结果。

### FUN-09（P1，Open）：Metalink 续传缺少远端一致性保护

- **证据**：[`metalink.rs`](../src-tauri/src/download/metalink.rs#L1153) 仅按本地长度发送 Range，不保存镜像 ETag/Last-Modified，不使用 If-Range，也未严格验证 Content-Range 起点。
- **影响**：远端内容变化或镜像切换时可能拼接新旧内容；无 manifest hash 时无法发现静默损坏。
- **修复方向**：持久化每镜像 validator；跨镜像续传必须有可信 checksum，否则重头；严格校验 206 和 Content-Range。
- **验收**：镜像内容变化、validator 变化、错误 Content-Range 和 failover 测试不会发布混合文件。

### FUN-10（P1，Open）：HLS 外部音轨和字幕是非对称的部分实现

- **证据**：[`hls.rs`](../src-tauri/src/download/hls.rs#L1224) 直接请求原始 track URI，未相对 master URL 解析；失败只 warning 并继续。额外轨不复用主 pipeline 的 AES、byte range、EXT-X-MAP、live、重试、限速和续传能力。
- **影响**：用户明确选择的轨道可能静默缺失，任务仍显示成功。
- **修复方向**：probe 时把 URI 规范为绝对地址，额外轨复用主 HLS pipeline；已选轨失败应进入 Failed 或 NeedsAttention。
- **验收**：相对 URI、加密、byte-range、init-map、字幕和多音轨端到端输出正确。

### FUN-11（P1，Open）：BT 做种时间限制未执行，UI 还会清空策略

- **证据**：[`bt.rs`](../src-tauri/src/download/bt.rs#L767) 的做种循环只读取 ratio；[`TaskDetails.tsx`](../src/components/shell/TaskDetails.tsx#L1120) 切换时固定传两个 null。
- **影响**：可保存的时间限制实际无效，用户打开或关闭做种会丢失已有 ratio/time 设置。
- **修复方向**：ratio 和 time 任一达到即停止；UI 暴露两者，未编辑字段保持原值。
- **验收**：虚拟时钟测试覆盖 ratio、time、任一条件、无限做种和 UI round-trip。

### FUN-12（P2，Boundary）：DASH 只支持较窄的静态 MPD 子集

- **证据**：[`dash.rs`](../src-tauri/src/download/dash.rs#L277) 拒绝 dynamic，拒绝 SegmentTimeline；AdaptationSet/Period 继承、模板变量、BaseURL 层级、多 Period 和轨道选择不完整。
- **改进**：短期将 README 和 UI 明确为 static/VOD first-pass；中期采用成熟 parser 或 ffmpeg demuxer 兜底，并建立真实 MPD corpus。
- **验收**：支持矩阵逐项有正向或明确拒绝测试，不能对未支持 manifest 生成不完整文件。

### FUN-13（P2，Open）：浏览器正式发布能力与当前产品表述不一致

- **证据**：release 配置固定关闭 capture 并移除 downloads、cookies、webRequest；商店材料只承诺手动 handoff，但 README 将自动接管和 header 转发列为当前能力。
- **改进**：交付经过权限审核的 capture 变体，或统一文案和设置，明确“仅开发包实验能力”。
- **验收**：每种发布 profile 的 UI、manifest 权限、文档和实际行为一致。

### FUN-14（P2，Open）：站点规则的 Ask 实际不会询问

- **证据**：[`background.js`](../browser/extension-core/src/background.js#L697) 对 header ask 固定不转发，对 capture ask 固定不接管，没有 notification、popup queue 或桌面确认。
- **改进**：实现明确确认流，或改名为 Never/Manual 并移除虚假 Ask 语义。
- **验收**：扩展行为测试覆盖 Always、Never、Ask、规则优先级和超时。

### FUN-15（P2，Open）：BT tracker 和 peer 诊断数据不完整

- **证据**：tracker 主要从 magnet URI 解析并固定为 configured，torrent URL 和本地 torrent 可能为空，seed count 多处固定为 0。
- **改进**：接入 librqbit 真实运行时统计；不可获得时 UI 明示 configured-only，不宣称实时健康。
- **验收**：magnet、远程 torrent、本地 torrent 的 tracker 来源和更新时间语义明确。

### FUN-16（P2，Open）：数据导出、备份和恢复不闭环

- **证据**：[`src/lib/export.ts`](../src/lib/export.ts#L7) 只导出少量展示字段；没有导入该格式的命令。数据库备份只服务于迁移异常，恢复页不能验证和恢复备份。
- **改进**：把现有能力命名为报表导出；另设计版本化、可校验、可回滚的备份和恢复格式，明确凭据处理策略。
- **验收**：跨版本 export/import round-trip，损坏备份拒绝，恢复失败不破坏原库。

### FUN-17（P2，Open）：新建和批量流程只覆盖后端创建能力的子集

- **证据**：后端输入支持 task speed、priority 和 category，但新建窗口固定为 null；批量导入不支持凭据、代理、hash、优先级、分类、duplicate override 或媒体选择。
- **改进**：建立统一“创建草稿”模型，单任务和批量预览共享字段、校验和覆盖操作。
- **验收**：单任务和批量输入使用同一合同测试，不能出现后端支持而 UI 永远不可达的字段。

### FUN-18（P2，Open）：非 HTTP 协议尚未达到同等级可靠性

- **证据**：[`docs/protocol-reliability-matrix.md`](protocol-reliability-matrix.md) 仍将多项 retry、proxy、credentials、checksum 和 diagnostics 标为 partial，缺跨进程重启和真实外部服务验收。
- **改进**：按协议建立 create、download、pause、resume、retry、restart、delete、proxy、credentials、checksum、diagnostics 生命周期矩阵。
- **验收**：每个声称稳定的协议至少有本地真实服务或固定 fixture 的集成测试，不只验证路由入口。

### FUN-19（P3，Boundary）：中长期能力边界

当前仍未实现稳定 CLI/JSON-RPC/REST、PAC/WPAD、云盘解析、云账号同步、插件协议、完整视频嗅探、Safari wrapper 和商店正式签名；WebDAV 仅 Basic，Metalink 资源仅 HTTP/HTTPS。这些能力应在 P0/P1 清零和协议可靠性矩阵闭环后再扩展。

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

### ARC-05（P1，Open）：调度全局锁跨越远程 resume probe

- **证据**：[`scheduler/mod.rs`](../src-tauri/src/scheduler/mod.rs#L111) 获取全局调度锁后 await start_task；有临时文件时 [`tasks.rs`](../src-tauri/src/commands/tasks.rs#L705) 会执行远程 probe。
- **影响**：一个慢主机的连接超时会串行阻塞所有队列派发和调度响应。
- **修复方向**：锁内只选择候选并原子预留任务、host 和连接槽；网络准备在锁外 worker 中进行，失败后原子释放 reservation。
- **验收**：一个 30 秒慢 probe 不影响其他 host 在预期时间内启动；同任务和同槽位仍保持单赢家。

### ARC-06（P1，Open）：SQLite 状态转移可能遇到 `SQLITE_BUSY_SNAPSHOT`

- **证据**：[`state_machine.rs`](../src-tauri/src/state_machine.rs#L135) 使用 deferred transaction，先读后写；其他任务 checkpoint 可在读写之间提交。
- **影响**：busy timeout 不一定处理 snapshot 升级失败，暂停、重试或 worker 完成可能偶发失败。
- **修复方向**：使用 `BEGIN IMMEDIATE`，或单条条件 `UPDATE ... RETURNING` 后判定状态；对 BUSY 和 BUSY_SNAPSHOT 做有界重试。
- **验收**：多任务每秒 checkpoint 与 pause/retry/fail 并发压力测试无随机失败，终态不被覆盖。

### ARC-07（P1，Open）：旧分页请求可覆盖最新查询

- **证据**：[`TaskList.tsx`](../src/components/tasks/TaskList.tsx#L194) 用单个 `loadingPageRef` 阻止新请求；查询变化时旧请求未完成，新 effect 直接返回，完成后也不补跑最新条件。
- **影响**：快速切换搜索、筛选、导航或排序后，界面可长期显示旧结果。
- **修复方向**：使用 request generation 或 AbortController，只接收最新响应；替换查询和 append 请求分开管理，并保留 pending latest query。
- **验收**：人为乱序响应时最终列表始终对应最新 query，旧响应不改变 cursor、selection 或 error。

### ARC-08（P1，Open）：实体缓存和当前查询成员关系混在一起

- **证据**：[`use-task-events.ts`](../src/hooks/use-task-events.ts#L212) 对任意 task update 直接 upsert；[`task-data-store.ts`](../src/stores/task-data-store.ts#L366) 会插入新任务并保留状态变化任务；列表只额外过滤软删除。
- **影响**：Completed 视图可能出现 queued 任务，离开当前筛选的任务仍留在列表，不匹配的新任务被插入。
- **修复方向**：分离 entity cache 与 query-keyed ID pages；成员关系或排序字段变化时按当前 query 重取，或使用与后端同源 predicate 更新并使 cursor 失效。
- **验收**：状态、分类、协议、失败码和搜索字段变化后，当前结果集合与后端分页完全一致。

### ARC-09（P1，Open）：`queue-changed` 防抖只保留最后一批 ID

- **证据**：[`use-task-events.ts`](../src/hooks/use-task-events.ts#L220) 每次事件清 timer，closure 只读取最后 payload。
- **影响**：100ms 内 A、B 两任务变化时可能只拉取 B，队列位置和状态长期过时。
- **修复方向**：窗口内累计 ID Set；任一 full-refresh 事件将整批提升为 full refresh。
- **验收**：快速多事件、重复 ID、超过 50 项和 full refresh 混合测试不丢任务。

### ARC-10（P1，Open）：控制面响应缺少流式硬上限

- **证据**：DASH MPD、Metalink 和 WebDAV PROPFIND 使用整包 `.text()` 或 `.bytes()`；HLS 无 Content-Length 时先读取完整 body 再检查上限。
- **影响**：异常或恶意服务器可造成 OOM，取消和 idle timeout 也无法及时生效。
- **修复方向**：抽取共享 `read_body_limited`，逐 chunk 检查大小，支持 cancellation、总超时和 idle timeout；本地 manifest 先检查 metadata。
- **验收**：缺 Content-Length 的超大和慢速响应在阈值处停止，内存保持有界，任务返回结构化错误。

### ARC-11（P1，Open）：HLS live 空闲退出条件实际无效

- **证据**：[`hls.rs`](../src-tauri/src/download/hls.rs#L513) 只在 `idle_polls >= 6 && finish == true` 时退出，但 finish 在循环顶部已独立退出；target duration 未 clamp，poll sleep 不可取消。
- **影响**：源停止更新或声明超大 target duration 时永久占用任务槽。
- **修复方向**：空闲阈值后进入 waiting_network 或有界重试；clamp target duration；sleep 同时等待 cancel 和 finish。
- **验收**：停止更新、超大 duration、取消和恢复测试均在有界时间转换状态。

### ARC-12（P1，Open）：BT 共享 session 的限速和引用计数所有权不清晰

- **证据**：[`bt.rs`](../src-tauri/src/download/bt.rs#L155) 的 session key 只含输出目录和代理；复用时把 session 全局限速改成最新任务值。`delete_runtime_task` 和 `SessionRefGuard::drop` 都可能 decrement；创建 session 时还持 registry mutex 跨 await。
- **影响**：一个 torrent 改变同 session 其他任务限速，引用计数可能提前归零，session 初始化阻塞其他 registry 操作。
- **修复方向**：只由 guard 释放一次；不持 mutex 跨 await；若 librqbit 仅有 session 级限速，则按任务拆 session 或实现 torrent 级调度。
- **验收**：多 torrent 不互改限速，删除任一任务不影响其余任务，session 在最后用户退出后才释放。

### ARC-13（P1，Open）：任务总进度与多文件进度错误耦合

- **证据**：[`task_state.rs`](../src-tauri/src/db/task_state.rs#L39) 每次更新任务进度都把总下载量写给所有 selected task files；BT 调用该函数，前端每个 tick 又复制完整 files 数组。
- **影响**：每个 torrent 文件显示相同的总任务字节数，文件数很大时还产生 O(file_count) 分配。
- **修复方向**：分离 task aggregate 和 file progress 更新；BT 从引擎快照发布真实 per-file progress，列表页不携带完整 files 数组。
- **验收**：多文件 torrent 的单文件进度正确且总和一致；1k 文件任务的进度事件 payload 和前端分配保持有界。

### ARC-14（P1，Open）：启动期 browser handoff 可能被接受后丢失

- **证据**：[`lib.rs`](../src-tauri/src/lib.rs#L604) 的 single-instance 回调在 AppState 管理前只记录 warning 并跳过；handoff 文件没有启动完成后的扫描。
- **影响**：native host 已向浏览器返回 accepted，但桌面应用没有创建任务。
- **修复方向**：在 startup state 中排队路径，ready 后重放；或启动完成时扫描 handoff 目录并使用现有去重逻辑。
- **验收**：冷启动同时收到 1 个或多个 handoff 时全部最终处理且不重复。

### ARC-15（P1，Open）：SFTP host-key 变化没有可完成的恢复路径

- **证据**：[`db/sftp.rs`](../src-tauri/src/db/sftp.rs#L5) 要求用户明确清除 known-host 行，但没有 list/forget command 或 UI。
- **影响**：合法服务器密钥轮换后用户只能手工修改数据库或重建数据。
- **修复方向**：提供显示 host、port、fingerprint 和首次见时间的管理 UI；forget 必须二次确认，下一次连接重新执行 TOFU。
- **验收**：密钥不匹配默认 fail closed，显式 forget 后可接受新 key，不能静默覆盖旧 key。

### ARC-16（P2，Open）：下载错误类型化仍主要停留在边界包装

- **证据**：download 模块仍大量使用 `Result<_, String>`，恢复逻辑需要从字符串或嵌套 JSON 重新解析错误。
- **影响**：错误码、重试策略和跨协议恢复行为容易随文案重构漂移。
- **改进**：按网络、认证、代理、远端变化、磁盘、工具缺失、格式不支持和取消逐步迁移到 typed errors，command 层统一输出稳定 payload。
- **验收**：scheduler 只匹配错误 code，不匹配人类文案；source chain 仍可复制诊断。

### ARC-17（P2，Open）：超大模块扩大变更影响面

- **证据**：SettingsPage、TaskDetails、HLS、DASH、Metalink 和 BT 同时承担解析、I/O、状态、渲染或编排中的多项职责。
- **改进**：按现有边界逐步拆分，不做机械小文件化。优先抽出 browser settings draft、query controller、manifest parser、transfer plan、remux process 和 BT session registry。
- **验收**：公共行为不变，纯模块获得直接测试，核心文件不再同时承担四类职责。

### ARC-18（P2，Fixed locally）：文档版本和能力声明漂移

- **实现**：README、AGENTS、ROADMAP、performance baseline、浏览器说明和发布示例已同步到 `0.3.0` 当前事实；`0.2.0` 专项审计保留原版本并明确标记为历史快照。
- **剩余风险**：协议实现或发布 profile 变化后，README、协议矩阵、浏览器权限说明和商店材料仍可能再次漂移。
- **验收**：增加自动文档检查，覆盖主要当前态文档的版本、release capture 边界和关键能力声明；在此之前保持 Fixed locally，不标记 Closed。

## 八、程序运行效率

### PERF-01（P2，Open）：历史任务搜索无法利用普通索引

- **证据**：[`task_records.rs`](../src-tauri/src/db/task_records.rs#L373) 对三个字段执行 `LOWER(column) LIKE '%term%'`。
- **风险**：大任务库首屏仍全表扫描，游标只能降低翻页成本，不能降低首次查询成本。
- **改进**：先记录 10k、100k、1M 数据的 query plan 和 p50/p95；超预算后引入 FTS5 或规范化 search column。
- **验收**：目标硬件上 100k 任务连续输入搜索不阻塞 UI，预算和数据分布写入性能基线。

### PERF-02（P2，Open）：TaskDetails 在非相关子页持续轮询 segments

- **证据**：[`TaskDetails.tsx`](../src/components/shell/TaskDetails.tsx#L377) 在 diagnostics 区域每 2 秒轮询 segments，即使用户位于 Requests 子页，也没有 in-flight guard。
- **风险**：慢 IPC 可重叠，长期打开详情产生无意义 DB 和序列化负载。
- **改进**：按可见 tab 订阅；上一次请求完成前不启动下一次；窗口隐藏或详情关闭时停止。
- **验收**：Requests、Logs 等子页不会请求 segments；慢请求下并发数始终为 1。

### PERF-03（P2，Open）：进度批次通知仍做全列表 O(N) 扫描

- **证据**：[`use-task-events.ts`](../src/hooks/use-task-events.ts#L86) 每次 flush 构造 previousById 并遍历全部任务。
- **风险**：任务数增加时，高频进度事件放大 CPU 和 GC 压力。
- **改进**：让 `patchTasksBatch` 返回状态变化 ID，通知只处理变化任务。
- **验收**：1k 已加载任务、每批少量变化时工作量与变化 ID 数量近似线性，而不是与总任务数线性。

### PERF-04（P2，Open）：HLS AES key 和 init map 缺少任务级去重缓存

- **证据**：[`hls.rs`](../src-tauri/src/download/hls.rs#L883) 每个 segment worker 调用 init-map 检查；每个加密 segment 重新请求相同 key。
- **风险**：额外网络请求、重复磁盘竞争和 check-then-create 竞态。
- **改进**：key 按 URI 缓存，init map 使用 OnceCell、singleflight 或原子 staging 提交。
- **验收**：N 个共享 key/map 的 segment 每个 URI 只请求和发布一次；失败可按策略重试且不缓存坏值。

### PERF-05（P2，Open）：长期缓存和 task events 缺少完整生命周期上限

- **证据**：`FILES_VERSION_CACHE` 对保留的 completed 任务长期占用条目；`task_events` 没有年龄或每任务条数清理，而 request diagnostics 已有保留策略。
- **风险**：长时间使用和大量历史任务会持续增长内存和数据库。
- **改进**：有界 LRU 或弱缓存；task events 增加按年龄和每任务数量的后台清理。
- **验收**：长期 seed 和删除测试后缓存、事件表和 WAL 大小稳定在文档预算内。

### PERF-06（P2，Open）：async 热路径仍有同步文件系统调用

- **证据**：任务准备、BT session key 和 ffmpeg 路径解析使用同步 exists、metadata 或 canonicalize。
- **风险**：网络盘、不可用盘或异常文件系统会阻塞 Tokio worker。
- **改进**：可用时改为 `tokio::fs`，必须同步的平台操作放入有界 `spawn_blocking`。
- **验收**：慢文件系统故障注入不阻塞无关下载和调度心跳。

### PERF-07（P1，Open）：完成动作用户命令同步阻塞且无超时

- **证据**：[`platform/mod.rs`](../src-tauri/src/platform/mod.rs#L228) 使用同步 `std::process::Command::status()`，scheduler 直接调用。
- **风险**：用户命令挂起会长期占用 Tokio worker，并阻塞完成动作收敛。
- **改进**：使用 Tokio Child、超时、kill-on-drop、受控 stdout/stderr 和取消；保留现有命令安全校验。
- **验收**：永不退出的测试命令会在超时后终止并记录结构化错误，不阻塞其他下载。

### PERF-08（P2，Open）：限速器使用墙上时间且缺公平等待

- **证据**：[`speed.rs`](../src-tauri/src/download/speed.rs#L9) 以 SystemTime 补充 token，多连接通过 CAS 争抢。
- **风险**：系统时钟回拨会停止 refill，活跃连接可能长期抢占新 token。
- **改进**：使用单调时钟的集中 refill/ticker，或公平 semaphore/等待队列。
- **验收**：时钟调整模拟不影响吞吐；多连接长期测试的带宽分配在定义容差内。

### PERF-09（P3，Needs benchmark）：release `opt-level="s"` 可能牺牲热点吞吐

- **证据**：[`Cargo.toml`](../src-tauri/Cargo.toml#L105) 为 release 使用尺寸优化。
- **处理**：先比较 `s` 与 `3` 在 hash、AES、XML、BT 和真实下载路径的吞吐、体积和启动时间；没有数据前不直接修改。
- **验收**：结果写入性能基线，必要时只对热点 package 使用 profile override。

### PERF-10（P2，Open）：没有 bundle size 和前端性能回归预算

- **证据**：当前构建主要分块约为 202、182、150、121 kB，但 CI 没有 chunk budget，也没有交互性能门禁。
- **改进**：记录 raw、gzip 和 brotli 体积；为初始 shell 和延迟页面分别设预算；结合真实启动和交互数据决定是否拆包。
- **验收**：CI 对显著增长给出可解释失败，不能只按单个 chunk 数字机械优化。

### PERF-11（P2，Open）：性能基线只有方法和估算，没有实测数据

- **证据**：[`docs/performance-baseline.md`](performance-baseline.md) 已同步到 `0.3.0` 并建立测量矩阵，但尚无真实 1k、10k、50k 结果。
- **改进**：至少测量冷启动、首屏、搜索、滚动 FPS、RSS、DB 写入率、事件率、长时间 HLS/BT 和 1k 文件删除。
- **验收**：记录硬件、OS、构建模式、数据生成参数、p50/p95、峰值和前后对比；没有元数据的单次数字不能作为回归门禁。

## 九、统一修复顺序

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
4. 媒体与清单完整性批次：`FUN-08`、`FUN-09`、`FUN-10`、`ARC-10`、`ARC-11`。
5. BT 所有权批次：`FUN-11`、`ARC-12`、`ARC-13`。

### 阶段 C：协议验收、发布和数据迁移

1. 关闭 `FUN-12` 至 `FUN-18`，按协议可靠性矩阵补齐真实生命周期测试。
2. 建立 GUI E2E、浏览器扩展行为测试和真实安装包 smoke。
3. 完成版本化报表、备份和恢复边界，并统一 README、ROADMAP 和商店材料。
4. 完成正式浏览器身份、updater 演练和 OS 签名策略；在此之前不得声称 OS-signed production distribution。

### 阶段 D：性能和可维护性

1. 先关闭 `PERF-11`，建立可重复基线。
2. 根据数据处理 `PERF-01`、`PERF-02`、`PERF-03`、`PERF-05` 和 `PERF-10`。
3. 并行处理有明确收益的 `PERF-04`、`PERF-06`、`PERF-07` 和 `PERF-08`。
4. 只有基准证明有收益时才处理 `PERF-09`。

## 十、发布验收定义

公开稳定发布前至少满足：

- 本文所有 P0 和 P1 为 Closed，不能以“已有入口”代替生命周期验收。
- HTTP、FTP/FTPS、SFTP、BT、HLS、DASH、WebDAV 和 Metalink 的公开能力声明与协议矩阵一致。
- 同 host 多任务、同名文件、代理、认证、暂停、恢复、重启、删除和磁盘冲突均有自动化证据。
- 真实 Windows、macOS 和 Linux 安装包完成安装、首次启动、下载、升级和卸载 smoke。
- 浏览器发布 profile、权限、设置、文档和真实行为一致。
- 数据库迁移失败保持 fail closed，并有可验证的备份或明确重建路径。
- 无后台 worker、ffmpeg、BT session、文件句柄或临时文件在取消和退出后泄漏。
- `pnpm check`、前端测试、生产构建、bindings、extension build、release tools、协议矩阵、Rust test 和 Clippy 全绿。
- 性能基线记录目标硬件上的冷启动、首屏、搜索、滚动、RSS、长时间运行和大批量删除结果。
- README、ROADMAP、PRODUCT、DESIGN、协议矩阵、发布材料和当前版本不存在能力或版本冲突。

## 十一、建议验证命令

基础门禁：

```bash
pnpm typecheck
pnpm lint
pnpm check:i18n
pnpm test:frontend
pnpm build
```

Rust 与绑定：

```bash
cargo check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml -j 1
pnpm specta
pnpm check:bindings
```

浏览器、协议与发布：

```bash
pnpm build:extensions
pnpm verify:protocol-matrix
pnpm test:release-tools
```

修复具体问题时还应执行本文对应条目要求的集成或端到端测试，不能用上述通用命令替代。

## 十二、相关文档

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
