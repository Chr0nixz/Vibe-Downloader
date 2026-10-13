# Vibe Downloader 优化计划（2026-09-29）

适用基线：`0.5.0`，`HEAD = 611cbbc`，包含当前未提交修改。本文是实施计划，风险状态仍以 [主审计](project-improvement-audit.md) 为准。

依据：[9 月 27 日评测](project-review-2026-09-27.md)、[第一批修复复核及 9 月 29 日修正记录](agent-evaluation-2026-09-29-r27-batch1.md)、当前工作区代码、[产品约束](../PRODUCT.md)。实施阶段的新验证与交付记录追加在本文末尾，真实安装包和外部协议验收单独标记。

建议主线：**完成第一批验收 → 补齐运行生命周期 → 实时策略 → 内网信任与自动恢复 → 常用功能完整性 → 协议深化。性能测量和发布验证从第一阶段开始积累。**

## 1. 当前完成情况与计划调整

### 1.1 已经实现，进入验收收尾

以下七项在主审计中均为 **Fixed locally**，不再安排重复开发。

| 正式 ID / 评测编号 | 当前代码已完成 | 还需要做什么 |
| --- | --- | --- |
| `ARC-56` / `R27-A01` | HTTP、FTP、SFTP 的本轮重试预算与累计诊断计数分离；FTP/SFTP 补了故障注入与变异复测 | 实际操作“失败→重试→暂停继续→重启”，确认入口到引擎的行为一致 |
| `FUN-33` / `R27-F10` | BT 循环读取有效限值，修复 `Option::min` 清空全局限速的问题 | 实机验证；定时限速更新与 HTTP+BT 混跑的总预算另列工作包 |
| `UX-41` / `R27-U07` | 后端统一重置；设置键由宏同源生成；更新与重置共用运行时副作用 | 确认下载目录、ffmpeg 路径等确实恢复默认，运行时状态与界面一致 |
| `UX-42` / `R27-U02` | 七语言徽标、诊断和 mock 已改为真实的“直播空闲”语义 | HLS 真实空闲与继续录制验收；空闲后完成录制归入媒体包 |
| `ARC-57` / `R27-A02` | 暂停/取消/重试/重新开始复用排空 helper；迟到检查点门控；排空后清理请求头缓存 | 慢盘实测；内部派生 worker 与阻塞文件 I/O 的收敛残项见 B1 |
| `ARC-58` / `R27-A07` | `CompletionRound` 实现单轮去重与未解决失败判断 | 补启动、supervisor 调用点的组合测试；验证真实并发完成只触发一次 |
| `UX-43` / `R27-U01` | 自身复制抑制、非模态提示；点击“使用此链接”后探测；隐藏期间暂存最新一次检测 | 验证 WebView2 托盘隐藏时的可见性信号；后台气泡仍未实现，文件类型过滤转入 `UX-48` |

已有 9 月 29 日记录：前端 410 项通过，Rust 786 项通过、2 项按设计忽略，格式、Clippy、构建等有通过记录。仍有两处完成动作“调用点被删”的变异存活，不能以辅助函数测试代替实际调用链验收。

另一个时间差需要明确：复核报告称 i18n 同步脚本“导入即改写 locale”的问题未修，但**当前工作区已经有后续修复**：CLI 入口隔离、写入前 Biome 格式化、无副作用导入和幂等测试。应验证并纳入基线，不再按“尚未开始”安排开发。

### 1.2 仍需开发的重点

规划时的源码抽查确认：正式构建统一拒绝私网；任务子限速器在启动时创建且 `DownloadControl` 不持有其句柄；调度器只取一页候选；BT `stats.error` 只进入快照；单个/批量删除存在 abort 后立即清文件路径。它们与评测所述缺口一致；实施后的状态以第 5 节和主审计为准。

调整原顺序的理由：

1. **把删除收敛和 BT 错误退出提前。** 它们直接影响文件所有权、下载槽与恢复流程，不必等做种管理器的大改造。
2. **把实时限速、计划重算、设置更新语义一起设计。** 只加 UI 开关会再次出现“设置保存成功、运行任务没有变化”。
3. **内网信任与自动恢复分别交付。** 两者都有较大的跨引擎范围，拆成可独立验收的工作包；安全策略先于新增恢复入口贯通。
4. **性能基准现在开始，优化按证据选择。** Windows 零填充仍是待实测风险，不能写成已确认瓶颈。
5. `ARC-55`、`SEC-14` 已补登记；`ENG-12` 的文档门禁与能力漂移仍需完成。保留已关闭条目的历史结论，残项单独登记，不重复打开旧问题。

## 2. 分批实施计划

成本为相对复杂度，包含测试与兼容性验证，不换算为未经评估的日期。B0—B4 是下一轮可靠性工作的主线；B5、B6 分项交付；B7 贯穿全程。

| 批次 | 目标与范围 | 前置依赖 | 成本 | 退出条件 |
| --- | --- | --- | --- | --- |
| **B0 当前修复验收与基线** | 七项 Fixed locally、i18n 脚本后续修复、`ENG-12` 优先漂移项；保存测试证据 | 当前工作区 | 小—中 | 当前快照门禁通过；列明仍缺实机证据的条目；验证不再修改源文件 |
| **B1 生命周期与调度正确性** | `R27-A06` 删除排空、`A04` BT 错误退出、`A03` 队头阻塞；`ARC-57` 残项；`A08` 更新退出排空 | B0 | 中 | 单任务至多一个写者；删除无竞态残留；BT 错误释放槽；可运行队尾任务能派发 |
| **B2 运行时策略** | `R27-F03`、`U06`、`A05`；`A09` 设置更新语义；BT 混跑总限速残项 | B1 的所有权与调度合同 | 中—高 | 调速/窗口边界 2 秒内生效；修改时段和系统唤醒会重算；混跑总速率符合全局预算 |
| **B3 内网信任** | `R27-F01` 来源与按任务目标授权；`F08` 系统证书信任保留为后续子项 | B0、现有剪贴板确认流程 | 高 | 明确授权的 NAS/Tailscale/localhost 目标可下载；未经授权来源、公网跳私网、元数据地址仍受阻；探测与下载一致 |
| **B4 无人值守恢复** | `R27-F02`、`U03`、`F09` 的 Retry-After；`F04` 未知大小重试/安全续传 | B1；复用 B2 唤醒机制、B3 网络策略 | 高 | 断网 2 分钟后无人操作完成；认证/磁盘/远端变化不盲重试；暂停后不会被自动唤醒 |
| **B5 日常工作流完整性** | `R27-F05` 请求头；`U04/U05` 后台可见性与退出恢复；`U08—U12` 分项体验改进 | B2—B4 相关合同 | 中 | 登录态下载、后台失败提醒、关闭选择、仅加入队列等形成完整可操作流程 |
| **B6 BT 与媒体深化** | `R27-F06/P02` 做种与会话；`F07` 媒体；`R26-F05/F07` 类型识别与轨道选择 | B1—B4 | 高，拆成 BT/媒体两个包 | 做种不占下载槽；字幕成品可播放；直播缺口可解释；会话隔离与限速无回退 |
| **B7 性能与发布证据** | `R27-P01/P03/P04/P05`、`PERF-13/16/09`、实机基线、协议对端、安装与更新演练 | 基准从 B0 开始，修复按测量选择 | 持续 | 前后数据可复现；候选安装包、真实服务器与升级路径有记录 |

### B0：先形成可信的当前基线

- 对七项已有修复做收尾，不恢复已撤销的 `suppressAutoProbe` 链路。沿用“点击使用链接即同意探测”的当前方案；这不等于授权访问任意私网资源。
- 在同一代码快照运行完整门禁并保留日志。Windows 可用 `$env:CARGO_BUILD_JOBS = "4"` 限制编译内存；调整并行度不替代任何测试。
- 验证 i18n 脚本：导入不改文件；CLI 连续执行两次结果幂等；`test:release-tools` 前后 locale 内容和修改时间不变。固定快照上连续两次验证应得到一致结果。
- 扩展 `check:docs` 校验源码注释里的正式审计 ID；修正 README/ROADMAP/隐私文档的内网、做种、已修缺口、验证命令等漂移。注释扫描需要明确历史标签例外，避免把 `R-1` 等旧标签误报。
- 实机验收优先覆盖托盘隐藏与剪贴板、慢盘暂停、并发完成动作，以及 `UX-35` 的真实分段图。条件不具备的条目继续保留 Fixed locally。

### B1：文件与任务生命周期必须先收敛

- 单个删除、批量删除复用统一停止流程：取消、等待、必要时 abort、join，最后才清理文件和记录。清理失败应留下可追踪的待清理记录。
- 处理 `ARC-57` 的内部残项：等待 supervisor 并不自动等于等待其派生 worker 或已提交的阻塞文件 I/O。对无法取消的写入保持资源所有权，确认旧写者退出后才允许重启或删除；仅限制数据库状态回写不足以证明文件安全。
- BT 错误应通过稳定错误码转入 Failed/NeedsAttention 并退出，释放下载槽与运行资源；磁盘问题不进入网络重试。
- 调度候选改为可继续分页或带可调度条件查询，直到填槽或扫描到队尾；保持优先级、主机连接限制和锁序，不通过一次加载全部队列解决。
- 更新安装/relaunch 前进入统一排空；系统退出尽力排空，异常终止仍靠检查点恢复。验证 ffmpeg 的随父进程退出行为，必要时补 Windows Job Object。
- **验收**：延迟刷盘时暂停→立即继续、取消、单删、批删；断言没有重叠写者。A 主机占满且队首超过一页时，B 主机仍启动。BT 注入运行错误后状态、槽位和恢复中心一致。更新期间无遗留转封装进程。

### B2：统一重算正在运行的策略

- 运行任务持有可更新的子限速器。统一计算任务、计划、全局约束；设置变化、窗口边界、系统唤醒触发重算，避免只在启动时读取。
- 计划监视器支持主动唤醒，并以有界兜底轮询处理墙钟变化；不要沿用一次可能持续数小时的旧 sleep。
- 设置接口明确“保持 / 清除 / 设值”，防止状态栏调速与设置页用旧快照互相覆盖。更改 IPC 后生成 Specta bindings。
- 详情页允许运行中调速；优先级可更新，但首版只影响后续派发，不承诺抢占运行任务。
- 单独登记 BT 全局预算残项：`FUN-33` 修复了会话限值被错误清空；`FUN-35` 再把 BT piece 写入接入任务根限速器，让多个 BT 或 HTTP+BT 共同消费一个全局预算，无需切换共享 BT 会话。真实吞吐和动态收敛仍按 B7 的外部验收执行。
- **验收**：HTTP/FTP/SFTP/BT/HLS/DASH/Metalink/WebDAV 的适用路径覆盖运行中调速、进入/退出窗口、清除限速与取消限流等待；边界后 2 秒内策略生效，长窗口吞吐符合上限及事先约定的测量容差。修改计划、休眠唤醒、时钟跳变不会沿用过期时间表。

### B3：让内网可用，同时保留网络边界

- 持久化任务来源与明确授权的目标范围，贯穿创建、探测、重试、续传及派生资源请求。旧任务使用明确的保守迁移规则，不能全部升级为“用户信任”。
- 手动输入或明确确认的内网目标允许按任务授权；浏览器交接/剪贴板来源保留独立来源信息。产品边界采用“按任务绑定目标”，暂不引入跨任务的主机/CIDR 全局白名单。
- 统一策略覆盖 DNS、IP 字面量、重定向、清单子资源和代理路径。用户授权某个 NAS 不应连带授权公网清单任意探测内网；元数据地址始终拒绝。带不同信任策略的连接不能误用同一不区分策略的客户端缓存。
- FTPS 系统证书库/企业 CA 与显式证书指纹信任暂列后续子项，不阻塞本批内网地址授权首版；FTP 主动模式同样后置。
- **验收**：release 配置下的私网 IPv4、IPv6 ULA、主机名、CGNAT；HTTP/WebDAV/FTP/SFTP 及派生下载路径；白名单开/关、DNS rebinding、公网→私网重定向、恶意清单、元数据地址。测试不能用全局测试旁路代替真实策略。

### B4：把短暂中断变成可观察的等待

- 任务级重试放在统一调度/恢复层，复用现有分段重试与 `retry_after_at`，不让每个引擎自行维护另一套任务循环。
- 持久化次数、下次时间与原因；指数退避加抖动；支持秒数和 HTTP-date 的 `Retry-After`；队列变化时重新安排最近唤醒，重启后仍生效。
- 先定义失败分类：网络/适当的 5xx/429 可重试；认证、证书信任、远端身份变化、磁盘错误进入明确恢复动作。用户暂停/取消、计划暂停、自动重试等待分别记录，恢复网络不能覆盖用户意图。
- 分开“直播空闲”与“网络等待”的状态或结构化原因，并同步徽标、详情、恢复中心、mock 和七语言文案。不能只把当前 `waiting_network` 文案整体改回去。
- 先实现确定的定时重试，再接系统网络事件加速唤醒；通过目标请求判断实际可达性，避免把一个公共探测端点的结果当作 NAS/代理均可用的证明。
- 进度按最近字节到达时间判断新鲜度：建议 2 秒无字节速度归零，5—10 秒显示停滞；正常直播等待等场景独立处理。阈值需用 fixture 验证。
- 未知大小响应只有在远端身份可验证、206/Content-Range 合法时续传；`Accept-Ranges` 单独不足以保证安全。不能安全续传时明确说明从头重试，保留有界预算。
- **验收**：断连 2 分钟后自动完成且摘要正确；重启保留等待；退避可取消；429 两种日期格式；恢复时不超并发；认证和磁盘失败不循环；未知大小资源变更不拼接出混合文件。

### B5：补齐高频流程，保持现有密集界面

按以下顺序拆分交付，放入现有新建、详情、设置和任务菜单：

1. **站点请求配置（`R27-F05`）**：先做单任务 UA/Referer/允许的自定义头，再做按站点复用；Cookie 等敏感内容沿用加密、脱敏、过期和同源绑定，跨源重定向不得泄露。candidate/release 浏览器扩展仍保持最小权限。
2. **后台与退出（`U04/U05`）**：聚合失败通知、托盘状态、任务栏进度；活动下载时关闭窗口提供“托盘 / 退出并暂停 / 取消”及记住选择；启动恢复尊重手动暂停，避免简单全局默认恢复所有任务。
3. **创建与完成后操作（`U08/U09`）**：添加但不开始、每任务计划选择、控件可访问标签与 axe；独立 `completed_at`，旧记录缺精确时间时不伪造；重新下载创建新任务、校验复用现有校验作业。
4. **剩余低成本交互（`U10—U12`）**：BT 文件选择使用明确的待用户选择原因；Ctrl+V 避开文本输入焦点；批量重复策略、队列与分类文案逐项验收；剪贴板文件类型过滤及后台气泡另行交付。
5. **小范围增强**：B2 设置语义稳定后做批量编辑（`FE-03`），随后任务预设（`FE-01`）、保存视图（`FE-04`）；脱敏诊断包可随实机反馈提前做。其余 FE 项不混入本轮。

### B6：协议深化按两个独立工作包验收

**BT：** 下载与做种生命周期分离，做种不占下载槽；明确完成动作是否结束后台做种，避免隐式中断。支持完成后开启/停止及重启恢复，快照按变化写入、piece 位图节流。先测多任务 RSS、DHT、WAL 和数据库写次数，再决定共享会话。

共享会话以“逐 torrent 限速、代理隔离、任务删除精确性、端口/DHT 生命周期均可满足”为前提；不能为了节省资源撤回 `ARC-39` 的修复，或以近似全局限速冒充逐任务限速。若依赖不支持，保留隔离会话，先减少写库与实现统一预算。

**媒体：** HLS WebVTT 字幕默认以 `mov_text` 嵌入 MP4；交付 HLS 空闲后完成录制、每任务缺片容忍与缺口记录。DASH 开放受限画质、音频语言及 WebVTT 字幕选择，选择结果持久化；按 Content-Type 识别无后缀清单。无 ffmpeg 仅降级到经过验证的单路 MPEG-TS HLS；fMP4、DASH、外部音频/字幕及需要重封装的组合继续拒绝。

**验收：** 两个 BT 下载加做种任务并行，下载槽、总预算、删除与完成动作正确；完成后重启做种可控。媒体成品经 ffprobe/实际播放验证，字幕、语言、缺口与最终状态一致，不能只断言退出码为 0。

### B7：性能测量与发布验证持续开展

| 项目 | 先收集的证据 | 何时实施优化 |
| --- | --- | --- |
| `R27-P01` Windows 预分配 | NTFS 上 4/8 段大文件首写 p95、总耗时、实际磁盘写入量；预分配/不预分配/稀疏方案对照 | 确认收益后选方案；不使用跳过清零、可能暴露旧磁盘数据的捷径 |
| `ARC-57` 检查点开销 | 多任务进度写入延迟、BUSY、WAL、CPU；新增 SELECT 与 IMMEDIATE 的前后差异 | 有明显竞争时优化事务与写入节奏，保留迟到检查点保护 |
| `PERF-16` 创建与无界查询 | 1k/10k 活动任务下创建 p50/p95、内存与查询计划 | 用索引点查/冲突重试、分页替代全集预读；不能静默截断用户可见数据 |
| `R27-P03/P04` 空闲开销 | 有/无限速、剪贴板开/关、托盘隐藏各运行 1 小时的唤醒次数、DB 查询、CPU | 缓存设置、剪贴板变更通知、无 waiter 时停止 ticker |
| `PERF-13/09` 依赖与编译参数 | 实际依赖 feature 图、冷编译时长、包大小、吞吐 | 先确认 russh/librqbit 等约束，再收敛 crypto 后端；分别验证 TLS 与 SSH 握手 |
| 实机性能与 `R27-P05` | release 启动、滚动、RSS、HLS/BT 长跑；CI 重复测试耗时 | 有数据后调整语言包加载、构建优化等级、覆盖率与测试重复执行 |

发布验证从 Windows 当前主环境启动，后续补 macOS arm64/x64、Linux x64。覆盖真实 NAS/FTP/SFTP/WebDAV、媒体和 BT 对端；安装、首次启动、旧库迁移、备份恢复、卸载，以及 `rc.0 → rc.1` 更新演练。GUI E2E 先覆盖创建→暂停继续→完成、剪贴板确认、设置调速和失败恢复；浏览器 background 行为测试独立补齐。

浏览器商店身份、签名和 OS 代码签名是独立交付事项；本地 fake server 的 automated 证据不替代真实对端，未签名包不宣称为 OS 签名生产分发。

## 3. 实施与验收规则

- 未登记的 `R27-*` 在开工前映射到正式审计 ID，保留历史依据、明确接受标准。一个工作包可覆盖多个 ID，但状态逐条依据证据更新。
- 先按工作包补复现与行为测试，再修改实现。涉及下载/续传的 Rust 回归放在 `src-tauri/tests`；跨引擎能力使用相同场景矩阵，而不是只测 HTTP。
- 完成动作要测实际启动/supervisor 到决策的连接，剪贴板要测实际事件到对话框/网络请求，排空要测写者退出。必要时只提取薄的可测试应用服务边界，避免为测试进行整套框架重写。
- 前端变化至少运行 `pnpm typecheck`、`pnpm test:frontend`，UI 变化加 `pnpm build`；locale 改动运行 `pnpm check:i18n`。Rust IPC 改动生成 Specta bindings；浏览器相关变化构建并验证扩展。
- 每批合入前跑 `pnpm verify` 及其场景验收。`pnpm verify` 不包含 `check:bindings`，IPC 变化需另验；当前 `check:bindings` 与 HEAD 比较，未提交的合法生成变化也会使其非零，需区分“生成不一致”与“尚未纳入基线”，不能手改 bindings 绕过检查。
- Fixed locally 只有在对应未完成验收得到证据后才能 Closed；纯逻辑条目按自身验收标准关闭，不给所有条目机械增加实机要求。真实环境受限时记录具体缺项，不声称已完成。
- `ARC-17/31` 按上述工作包顺带提取重试策略、运行策略、生命周期边界，避免与功能修复同时开展无关的大文件重写。`ENG-03` 清理残项、`R27-A11` 窗口权限与依赖复审持续推进。

## 4. 下一步的具体执行顺序

1. **收尾当前批次**：核验当前快照、i18n 脚本、七项实机缺口与 `ENG-12`；保留未满足验收的状态。
2. **生命周期小批次**：删除排空与内部写者收敛 → BT 错误退出 → 多主机调度 → 更新退出排空。
3. **实时策略批次**：设置保持/清除语义 → 任务限速句柄 → 边界/设置/唤醒重算 → UI 解禁 → 混合协议总预算验收。
4. **网络能力批次**：来源与授权持久化 → 私网/证书合同 → 任务级重试 → 网络事件与停滞诊断 → 未知大小安全恢复。
5. **体验与协议分项交付**：先请求头、后台可见性和退出恢复，再进入 BT 做种与媒体深化。

Windows 性能基线与安装包走查从第 1 步开始。B7 的逐项执行表见 [b7-acceptance-matrix-2026-10-02.md](b7-acceptance-matrix-2026-10-02.md)，先记录真实内网、断网恢复和候选包证据，再更新主审计状态。目录递归下载等待内网信任成熟；云盘、云同步、插件协议、完整视频嗅探、CLI 和多源并行下载继续后置。

下一候选版本的目标是：**现有任务能够稳定停止和恢复，网络中断后自动继续，用户调速立即有效，内网访问可控，后台失败可见，并有真实安装包证据。** 具体版本号和日期在工作包复现、测试资源与外部发布条件明确后再确定。

## 5. 2026-09-29 首批实施记录

本批完成 B0 的自动化基线与文档治理，并实施 B1 的调度、BT 错误退出和删除服务首段。保留进入本批之前的未提交修改；原七项 Fixed locally 没有重复开发，也没有因通用门禁通过而改成 Closed。

| 正式 ID / 工作包 | 本批交付 | 当前状态与边界 |
| --- | --- | --- |
| `ENG-12` / B0 | 源码注释审计 ID 门禁、文档能力纠偏、残项拆分规则 | Closed；实际注入未知 ID 被 CLI 拒绝，14 条脚本测试通过 |
| `ARC-61` / B1 | 有界 keyset 候选分页、查询排除满主机/窗口受限任务、逐候选检查主机槽位 | Fixed locally；4 条真实数据库回归，仍待 CI 和应用派发链验证 |
| `ARC-60` / B1 | BT 错误结束循环，类型化磁盘错误/未知稳定错误分类，诊断及 torrent/session 释放 | Fixed locally；3 条真实 librqbit 引擎运行与数据库收敛测试，仍待真实对端/Tauri 操作 |
| `ARC-59` / B1 | 单删/批删共用服务，锁保持至清理和删库结束，失败留记录，排空后刷新文件路径 | Fixed locally；包含超时保留 owner、阻塞写入期间拒绝删除的验收 |
| `ARC-62` / B1 | 派生 worker、文件写入、BT 存储和 ffmpeg 的生命周期纳入共享 owner；停止超时保留槽位并拒绝重启/删除 | Fixed locally；自动化覆盖通过，真实慢盘与安装包操作仍待验收 |
| `ARC-63` / B1 | 统一 Tauri 退出状态；updater 下载/验签后排空，再安装/relaunch；恢复入口共用排空 | Fixed locally；自动化覆盖通过，真实安装包退出与外部强制终止边界仍待验证 |

验证记录（Windows，本地工作区）：

- 修改前完整 `pnpm verify`：前端 410 项、Rust 786 项通过，Rust 2 项按设计忽略。日志：`%TEMP%/vibe-verify-b0-20260929.log`。
- 首批修改后完整 `pnpm verify`：前端 410 项、Rust 797 项通过，Rust 2 项按设计忽略；包含 TypeScript、Biome、i18n、文档、发布工具、生产构建/体积预算、扩展构建/manifest、Rust fmt、Clippy `--all-targets -D warnings` 与 locked 测试。日志：`%TEMP%/vibe-verify-b0-b1-20260929.log`。
- i18n 同步脚本 10 项测试通过；单独重跑 `pnpm test:release-tools` 也通过，运行前后七个 locale 的 SHA-256 和修改时间均未变化。日志：`%TEMP%/vibe-i18n-side-effects-20260929.log`、`%TEMP%/vibe-b0-release-tools-no-side-effects-20260929.log`。
- 全量门禁之后补充了删除期间成品自动改名的回归与对应路径刷新修正；最新代码的 fmt、Clippy `--all-targets -D warnings`、`task_deletion` 5 项测试全部通过。日志：`%TEMP%/vibe-b1-final-deletion-20260929.log`。该新增场景不混入先前的 797 项全量计数。
- 最后重新执行 `pnpm specta`，生成前后 `src/generated/bindings.ts` 的 SHA-256 一致；本批没有修改 IPC 签名。未以 `check:bindings` 的 HEAD 差异作为失败判据，因为此前的合法未提交 bindings 变更仍在工作区。日志：`%TEMP%/vibe-b0-bindings-consistency-20260929.log`。
- ARC-62 阶段验收记录（早于 ARC-63 实施）：再次运行完整 `pnpm test:rust`，locked Rust 单元、集成和文档测试全部通过，2 个性能基准按设计忽略；首次运行发现 ARC-62 引入的取消后状态收敛与旧测试断言冲突，更新断言验证最终为 Paused 且不被标记为 Failed 后，完整套件通过。`lifecycle_drain` 2 项、`restart_quiesce` 2 项、`task_deletion` 6 项均通过；此前的完整前端门禁与 Clippy `--all-targets -D warnings` 通过。ARC-63 的当前验证见下一条。
- ARC-63 接入后重新运行完整门禁：`pnpm test:rust` 全通过（351 个库测试及所有集成测试；2 个性能基准忽略），`shutdown_drain` 4 项通过；`pnpm test:frontend` 414 项通过，`pnpm typecheck`、`pnpm lint`、`pnpm check:i18n`、`pnpm build`、`pnpm check:bundle`、Rust fmt 与 Clippy `--all-targets -D warnings` 通过。Specta 已生成 `prepareAppRelaunch`/`cancelPreparedAppRelaunch`；`pnpm check:bindings` 的 HEAD 比较仍被工作区原有 `resetSettings`/ARC-58 bindings 修改阻止，生成结果包含新命令且没有被手改。真实候选安装包更新/退出和 ffmpeg 清理验证仍待 B7。

下一实施顺序：**B3 内网信任实现已完成，转入 B4 无人值守恢复；B7 继续承担真实高速对端和候选包验收**。B2 的实时策略核心与 `FUN-35` 本地实现仍为 Fixed locally；混跑总速率、系统睡眠/恢复和候选应用操作仍需外部验收。B0 的托盘隐藏/剪贴板、慢盘和并发完成动作验收，以及 B7 性能基线、真实更新安装与退出后 ffmpeg 检查仍需完成。

## 6. 2026-09-29 B2 运行时策略实施记录

本批实现活动任务限速器更新、计划变更唤醒和设置补丁语义；同值策略刷新不会补满已消耗的 token bucket，也不会重置 librqbit 的会话限速桶。

| 正式 ID / 工作包 | 本批交付 | 当前状态与边界 |
| --- | --- | --- |
| `FUN-34` / B2 | 活动任务共享可更新子限速器；设置变化、窗口边界和 Tauri 恢复事件重算；详情页运行中调速与优先级编辑 | Fixed locally；前端 417 项和 Rust 全量通过，真实协议对端吞吐验收待 B7 |
| `ARC-64` / B2 | 可唤醒的计划监视器、最近边界计算、60 秒墙钟兜底 | Fixed locally；实际系统睡眠/恢复、墙钟跳变和 DST 仍待实机验证 |
| `ARC-65` / B2 | 限速等可清除设置的保持/清除/设值语义；前端增量补丁；后端更新/重置串行合并 | Fixed locally；三态 Rust 测试和完整设置页测试通过，快速并发的候选包操作待手工走查 |
| `FUN-35` / B2 残项 | BT storage 写入接入任务根限速器；多个 BT session 与 HTTP 风格 acquire 共用一个跨协议 token bucket；取消检查保持有界 | Fixed locally；边界回归通过，真实高速对端、HTTP+BT 混跑窗口、动态设置收敛和 Tauri 取消/删除验收待 B7 |

`ARC-66`（状态转换参数语义）和 `ARC-67`（UI 线程同步数据库访问）是同轮评审另行登记的 Open 项，不属于本批 B2 策略交付。

验证记录（Windows，本地工作区）：

- `pnpm verify:frontend` 全通过：TypeScript、Biome、417 项前端测试、release-tools、i18n、协议矩阵、版本与文档门禁、生产构建、bundle 预算及扩展构建/manifest 验证。
- `pnpm test:rust` 全通过：355 个库测试及全部集成/文档测试；2 个性能基准按设计忽略。全量套件结束后补强了 BT session 同值同步不重置额度的断言，并在最终源码上定向重跑该 BT session 测试及 token bucket 同值策略测试，两项均通过。
- `cargo fmt --all --manifest-path src-tauri/Cargo.toml -- --check` 与 `cargo clippy --locked --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` 通过。
- `pnpm specta` 成功生成 IPC bindings；最新三态设置字段体现在生成类型中，重新生成前后的 bindings SHA-256 相同（`8B540297757A0CE756A8731967EAB674EA46A02100324BA989D31E2DDC1DC310`）。工作区含有进入本轮前的 bindings 差异，不能用与 HEAD 的差异检查判定这些既有修改为漂移。
- 未在真实系统休眠/恢复、真实协议高速对端或 BT 跨会话混跑环境中验收；这些项目仍是明确的外部验收条件，`FUN-35` 的本地共享预算实现已完成但保持 Fixed locally。
- **2026-10-01 收尾复验**：BT storage 共享预算两条测试、`bt_loop_limit_sync_keeps_global_limit_without_task_limit` 通过；当前 Rust 库单测 359/359 通过。重跑完整 `cargo test --locked --manifest-path src-tauri/Cargo.toml -- --test-threads=1` 时，`directory_probe` 有 4 条 localhost 场景失败：SSRF 策略拒绝 `127.0.0.1`，与测试预期的连接/取消结果不符；全量集因此未通过，后续需按网络信任合同修正测试授权与预期，并保留私网拒绝断言。该失败不在 B2 限速路径内；B2 的真实 BT/HTTP 混跑和动态吞吐仍由 B7 验收。

## 7. 2026-10-02 B3 内网信任实施记录

B3 按用户确认的边界实现为**按任务授权并绑定目标**：localhost 可以显式授权访问，但不能无授权访问。授权不提升为全局白名单，也不把浏览器设置开关当作下载许可。

| 正式 ID / 工作包 | 本批交付 | 当前状态与边界 |
| --- | --- | --- |
| `FUN-36` / B3 | 来源、目标 authority、DNS 解析地址和任务级授权草稿贯通创建、目录探测、重试、续传、派生资源、浏览器交接与备份恢复 | Fixed locally；真实 NAS/Tailscale/企业 DNS 和候选包仍待外部验收 |
| `SEC-15` / B3 | 跨协议 `NetworkPolicy`；按任务绑定 authority + resolved IP；私网 IPv4、IPv6 ULA、CGNAT、localhost 显式授权；跨 authority、DNS rebinding、公网跳私网、链路本地、组播和 metadata 地址拒绝 | Fixed locally；真实网络矩阵与发布 profile 仍待外部验收 |

实现要点：

- 网络客户端缓存指纹包含完整任务策略，避免不同授权误用同一客户端。
- HTTP、HLS、DASH、Metalink、WebDAV、FTP、SFTP 和 BT 的 manifest/`.torrent` 控制面均经过同一策略；重定向和清单子资源不能扩大授权范围。
- 浏览器 handoff 的 `authorization_required` 会打开桌面新建窗口；扩展显示“请在 Vibe Downloader 中授权”，自动接管场景会恢复浏览器原下载，不会在用户确认前取消它。
- 备份恢复不会迁移临时授权草稿；任务保留显式策略，失效或撤销后回到保守拒绝路径。

验证记录（Windows，本地工作区）：

- 网络策略单测、BT/目录探测/HTTP/SSRF 集成测试通过；Rust 库测试 `363/363` 通过。
- `cargo clippy --locked --all-targets -- -D warnings`、`cargo fmt --check`、`git diff --check` 和 TypeScript 类型检查通过。
- B3 新增迁移后修正迁移完整性计数断言；完整 Rust 门禁和前端/构建门禁在文档修订后重跑。
- 真实 NAS、localhost/Tailscale、企业内网 DNS、候选/发布包以及跨平台浏览器安装仍是 B7/发布验收，不在本地自动化结果中宣称已完成。

## 8. 2026-10-02 B4 无人值守恢复实施记录

B4 落地任务级持久化重试、`Retry-After` 两种格式和未知大小 HTTP 安全恢复。按本轮确认的范围未增加 UI 或多语言设置；`R27-U03` 的速度/剩余时间停滞诊断保留在范围外。

| 正式 ID / 工作包 | 本批交付 | 当前状态与边界 |
| --- | --- | --- |
| `FUN-37` / B4 | 10 次任务级瞬时故障重试预算；指数退避、抖动、30 分钟单次上限；持久化原因/次数/截止时间；429/5xx 的秒数与 RFC 1123 HTTP-date；失败态原子入队、暂停/取消竞态保护 | Fixed locally；真实 2 分钟 Wi-Fi/VPN 中断和系统网络变化唤醒仍待 B7 实机验收，现有 watcher 按持久化截止时间唤醒 |
| `FUN-38` / B4 | 未知大小 HTTP 仅在精确 Range 探测和 validator 合同时续传；响应逐项验证已存 ETag/Last-Modified；无可靠续传能力时重试从零开始并复位检查点 | Fixed locally；fake-server 覆盖通过，真实 CDN/代理响应组合仍待协议对端验收 |

实现要点：

- 错误分类只接纳带明确 `recoverable` 证据的瞬时传输/超时、429 和 5xx；认证、磁盘、TLS、通用 `network_error`、解码/畸形响应体与远端身份变化保留为人工处理。明确的连接中断统一为 `transport_interrupted`，耗尽时仍持久化最终次数和原因。
- 自动重试可接手 HTTP coordinator 已写入的 `failed` 状态；对 task status 使用条件更新，用户暂停/取消或手动操作胜出时不会被 worker 复活。watcher 以可保留的单 waiter 通知和持久化截止时间重新安排最近唤醒。
- 未知大小流中断后以临时文件实际长度决定续传偏移。只有 `206`、正确的 `Content-Range` 起点、强 ETag 或有效 HTTP-date `Last-Modified` 作为身份依据，且所有已存 validator 都匹配才会 append；服务端缺少 Range 或 validator 时清理未完成 part、把 segment offset 归零并重试整个资源。

验证记录（Windows，本地工作区）：

- `pnpm verify` 的前端阶段通过：typecheck、Biome、417 个前端测试、83 个发布工具测试、i18n、协议矩阵、文档门禁、生产构建、bundle 预算和扩展验证均通过。
- 修正 `network_error` 可恢复和 `retryAfterAt` 测试字段后，`cargo test --locked --manifest-path src-tauri/Cargo.toml -j 2` 全量通过：368 个库测试及所有集成/文档测试；2 个性能基准按设计忽略。全目标 Clippy 和 Rust fmt 通过。
- 加入无 Range/validator 时从字节零重试的最终修正后，再跑全目标 Clippy 与 `http_engine` 34/34；`task_retry` 2/2 和 scheduler failed-state 收敛测试也通过。默认并行的 `pnpm verify` Rust 阶段曾遇到 Windows rustc 内存分配失败，使用仓库建议的 `-j 2` 完成全量 Rust 测试。
- 未做真实 2 分钟网络断开、OS 网络事件、休眠/唤醒、真实 CDN/代理矩阵或候选安装包操作；这些仍归 B7/发布验收，`FUN-37`/`FUN-38` 保持 Fixed locally。

### 2026-10-02 B4 加固复核

- 将未知大小续传的身份条件收紧为强 ETag 或有效 HTTP-date `Last-Modified`；弱 ETag 旧任务即使错误保存了 `supports_resume` 也会从零重启。
- 将 HTTP 响应体错误分为可验证的 `transport_interrupted` 与不可自动重试的 `decode_error`/`body_error`；代理认证、TLS、磁盘和远端变化不会落入自动重试预算。新增畸形 chunked、弱 ETag 和结构化错误回归。
- 复核通过：全 Rust `cargo test --locked --manifest-path src-tauri/Cargo.toml -j 2`（371 个库测试及全部集成/文档测试，性能基准按设计忽略）、全目标 Clippy、Rust fmt、`http_engine` 36/36、前端 417/417、发布工具 83/83、i18n、文档门禁和 `git diff --check`。
## 9. 2026-10-02 B7 验收入口与性能 smoke

- 新增 [B7 真实环境与候选包验收矩阵](b7-acceptance-matrix-2026-10-02.md)，把 B3 内网授权、B4 断网恢复、B1/B2 生命周期与运行时策略、候选升级和性能场景拆成逐项记录表。矩阵明确 `Fixed locally` 到 `Closed` 的证据规则，并保留真实环境缺口。
- 在当前 `0.5.0` 工作区执行 `pnpm perf:baseline` 和 `pnpm perf:baseline:10k`，1k smoke 与 10k baseline 均通过。原始证据位于 `artifacts/perf/20261002-192007/`、`artifacts/perf/20261002-200606/`，摘要已写入 [performance-baseline-results.md](performance-baseline-results.md)。
- `pnpm check:docs` 和 `git diff --check` 通过；定向 Rust 回归 `ssrf_engine` 9/9、`directory_probe` 8/8、`http_engine` 36/36 通过。`VIBE_BROWSER_PROFILE=candidate pnpm tauri build --config src-tauri/tauri.ci.conf.json` 生成 Windows 主程序、MSI 和 NSIS，主程序与 Native Host `--self-check` 通过；MSI 管理员解包因 Windows Installer 服务超过 3 分钟无返回而停止。真实 NAS/Tailscale/企业 DNS、断网/休眠恢复、候选安装/升级和跨平台安装仍未执行，不能据此关闭 `FUN-36`、`SEC-15`、`FUN-37`、`FUN-38` 或 B1/B2 的 Fixed locally 条目。


## 10. 2026-10-02 B5 单任务请求配置实施记录

B5 的第一项 R27-F05 已完成本地实现，登记为 FUN-39（Fixed locally）。本批只覆盖单任务 HTTP/HTTPS 请求配置；按站点复用、后台可见性与退出恢复、创建后操作和后续体验项仍按 B5 计划保留。

- 新建下载和任务详情均提供 User-Agent、Referer 和受限自定义头编辑；修改请求配置会使旧 probe snapshot 失效，创建路径在有配置时重新探测。批量创建、目录探测和浏览器 handoff 不携带该配置。
- task_request_profiles 分开保存普通头和敏感头密文；普通字段编辑不会刷新敏感 TTL，敏感字段必须显式替换。敏感值只在任务启动、恢复、重试和定期清理时重新解析并检查 24 小时 TTL；运行中的会话持有本次启动解析出的 headers。
- HTTP 客户端禁用自动重定向，HTTP 与派生引擎的重定向、清单和资源请求统一经过 origin 过滤及 NetworkPolicy。跨源会去除 Cookie、Authorization、全部 X-*，降级到 HTTP 会去除 Referer；保存的 final URL 不会成为新的秘密来源。

本地验证（Windows）：前端测试 427/427；Rust 定向测试 request_profiles 7/7、directory_probe 8/8、hls_engine 23/23、source_hygiene 2/2；pnpm typecheck、pnpm lint、7 个 locale、文档门禁、Rust fmt 和全目标 Clippy 已通过。完整 Rust 套件使用 cargo test --locked --manifest-path src-tauri/Cargo.toml -j 2，避免本机并行编译触发页面文件不足；库测试 371/371、全部集成测试与文档测试通过，全目标 Clippy 和 Rust fmt 通过。Tabbit 已确认主界面和新建下载对话框的可访问结构；展开高级请求配置后的浏览器会话意外断开，未据此宣称完整视觉验收。真实站点、内网服务器、候选/发布包安装和跨平台浏览器仍待 B7/发布验收。

## 11. 2026-10-03 B5 后台可见性与退出恢复实施记录

B5 的 `U04/U05` 已完成本地实现，覆盖后台运行提示、关闭决策和启动恢复边界；真实系统通知权限、任务栏/托盘外观、休眠恢复、候选包安装和跨平台行为仍留在 B7 验收矩阵。

- 任务状态变化会发送完成、失败和需要处理的系统通知；失败与需要处理的转移在 700ms 窗口内聚合，应用内 toast 仍逐任务保留。通知设置文案已改为涵盖“完成或需要处理”。
- Rust 每秒发布全库任务统计，前端按当前语言生成托盘 tooltip，并通过 Tauri 任务栏进度 API 更新聚合进度和错误状态；无活动或未知总大小时不显示虚假进度。
- 有活动下载时关闭主窗口会弹出“托盘 / 退出并暂停 / 取消”选择，记住选择只改变关闭到托盘设置；退出并暂停复用全局暂停路径并继续尊重手动暂停。统计查询失败时保持 fail-closed，显示安全提示而不会误判队列为空并直接退出。
- `auto_resume_on_startup` 默认值为开启，但启动重置只接管 `downloading/retrying` 任务，用户手动暂停的任务仍保持暂停。

本地验证（Windows）：前端 433/433；新增纯逻辑与关闭对话框测试，`pnpm typecheck`、`pnpm lint`、`pnpm build`、`pnpm check:bundle`、`pnpm check:i18n`、`pnpm check:docs`、`git diff --check`、Rust fmt、全目标 Clippy 和 `cargo check --all-targets` 通过；Rust 库测试 371/371，`segments::reset_interrupted_tasks` 2/2，HLS `perf15_finish_notify_wakes_waiter_promptly` 1/1。未将真实系统通知权限、任务栏进度外观或跨平台关闭行为标记为已验收。

## 12. 2026-10-03 B3/B5 审阅加固记录

对前序未提交改动做了第二轮安全与兼容性复核，补上四类容易被边界输入触发的问题，仍不改变 B3 的“按任务授权并绑定目标”边界：

- 网络 authority 统一使用 IPv6 方括号格式；FTPS 未显式端口统一按隐式 TLS 的 990 端口授权和解析，FTP、SFTP、WebDAV 的 source key 与手工连接 authority 复用同一格式化路径。
- HTTP origin 比较复用同一 authority 格式，IPv6 的同源、跨源和不同端口头部过滤不再因字符串格式失配而放行或误删凭据。
- 任务启动/恢复解析请求头时先检查持久化 profile 和浏览器头的过期/解密状态，再合并创建阶段仅保留在内存中的浏览器头；profile 按名称覆盖旧值，数据库错误或过期状态继续 fail-closed。
- 创建阶段把 probe snapshot 当作不可信缓存，重新核对当前 EngineRegistry 协议、`final_url` 的协议形状，以及输入和最终网络 URL 的当前 NetworkPolicy；不满足时丢弃快照并重新探测。

验证记录（Windows）：`cargo check --locked --all-targets`、全目标 Clippy、Rust fmt、`git diff --check` 通过；`cargo test --locked --manifest-path src-tauri/Cargo.toml -j 1 -- --test-threads=1` 通过（库测试 376/376，全部集成测试与文档测试通过，性能基准按设计忽略）。新增 IPv6/FTPS authority、IPv6 origin、请求头内存回退和 probe snapshot engine/URI 契约回归测试均通过；本轮在同一 snapshot 回归中补充了 HLS `file://` `final_url` 拒绝断言，因此测试总数仍为 376。Windows 并行全量链接曾触发 `LNK1140` PDB 限制，降为单任务后完成验证；该构建资源限制不代表代码测试失败。真实 NAS/Tailscale、断网/休眠恢复、候选包和跨平台验收仍维持 B7 的外部待办。

## 13. 2026-10-03 B5 U08 新建下载流程细节

本批完成 B5 的 `U08` 首个窄切片：新建下载支持“添加但不开始”、按任务选择是否遵守全局下载窗口，并把该选择贯通单任务、批量导入、Rust 创建核心和浏览器预览适配器。默认行为保持兼容：立即开始且遵守下载窗口。

- `CreateTaskInput` 增加可选 `startPaused` / `obeySchedule`；初始暂停任务持久化为 `Paused`，健康摘要为 `Paused`，并写入 `created` 与 `paused` 事件，避免启动恢复或计划逻辑误把它当成排队任务。
- 批量导入的 `ImportUrlsInput` 也接受 `obeySchedule`；浏览器 handoff 与旧调用路径显式使用立即开始、遵守窗口的默认值。浏览器预览实现同步模拟暂停状态、任务文件状态和窗口标志。
- 新建窗口保留“开始下载”，增加“添加但不开始”；高级选项中的窗口复选框在单任务和批量模式均有稳定的可访问标签、说明和七语言文案。重复任务覆盖会保留用户选择的初始暂停状态。
- 真实外部验收仍为 `Fixed locally`：任务调度、系统窗口边界和候选包行为需要 B7 在实际应用环境复核，不能以 fake server 测试替代。

验证记录（Windows）：前端 `78` 个测试文件、`435` 项通过；`pnpm typecheck`、生产 `pnpm build`、`pnpm check:i18n`、本次修改文件的 Biome 检查、Rust fmt、全目标 Clippy 通过；Rust 全量 `cargo test --locked --manifest-path src-tauri/Cargo.toml -j 1 -- --test-threads=1` 通过（库测试 `376/376`，全部集成/文档测试通过，性能基准按设计忽略）。`pnpm check:bindings` 仍会因工作区进入本批之前已有的合法生成差异而与 HEAD 不相等；已运行 `pnpm specta`，新增字段存在于生成绑定，未手改生成文件。完整 `pnpm lint` 的失败来自工作区既有 CRLF/格式差异及未触碰文件；本批五个 TypeScript 改动文件单独 Biome 检查通过。

## 14. 2026-10-03 B5 U09 完成后操作与时间证据

B5 `U09` 已完成本地实现：完成时间独立持久化、已完成任务重新下载、完整性校验复用现有校验作业。原任务保留为历史记录，重新下载创建新的排队任务并复制可用配置。

- 新增 `tasks.completed_at` 迁移和索引；所有三类完成路径写入时间，重新开始/重新下载的新任务清空该字段。任务列表、更新事件、详情与 Integrity Passport 读取持久化值。旧数据库记录没有可靠时间时继续显示未知，不使用 `updated_at` 或已保留事件伪造。
- 已完成任务菜单和详情提供“重新下载”和“重新校验”。重新下载保留保存目录、文件名、任务限速、优先级、分类、计划选择、校验配置、请求配置、凭据、代理、HLS 轨道选择和任务绑定网络策略；缺少旧策略的任务不创建空授权 token，仍按创建流程的公开/保守策略处理。浏览器预览同步保留请求配置、代理和网络策略。
- “重新校验”直接调用既有多算法 checksum 作业，更新任务状态、事件和所有任务表面；无校验配置的任务仍明确显示 `not_provided`。

本地验证（Windows，工作区保持 dirty）：迁移完整性 `14/14`、Integrity Passport `10/10`、请求 profile `7/7`、浏览器认证恢复 `2/2`、重新下载回归 `2/2` 均通过；`cargo check --locked --all-targets`、`cargo clippy --locked --all-targets -- -D warnings`、`cargo fmt --all -- --check` 和 `git diff --check` 通过。最终 `cargo test --locked --manifest-path src-tauri/Cargo.toml -j 1 -- --test-threads=1` 通过 `377/377` 个库测试、全部集成测试和文档测试，性能基准按设计忽略 2 项；随后缓存竞态加固又重跑了重新下载 `2/2` 与请求 profile `7/7`。新增回归确认浏览器 Cookie/来源及请求 profile 的敏感过期时间按原任务绝对截止时间复制；过期或解密失败仍 fail-closed，旧任务缺少网络策略时不会生成空授权 token。真实文件服务器、系统通知/托盘外观、候选安装与跨平台行为仍按 B7 验收矩阵保留。

### 2026-10-04 B5 U09 加固复核

- 重新下载现在同时复制持久化请求 profile 与浏览器交接请求头；浏览器来源元数据继续保留，合并顺序保持 profile 按名称覆盖浏览器临时头。
- 新任务不会刷新敏感凭据 TTL：请求 profile 和浏览器头都沿用源任务的绝对过期时间；源头已过期、解密失败或元数据不完整时在创建前返回原稳定错误，不产生半成品任务。
- 代理模式、`no_proxy`、任务凭据与任务级网络策略均有直接回归覆盖；`off` 模式下的任务代理附加字段也不会静默丢失。
- 本轮未改变 IPC 命令形状，因此不生成新的绑定差异；桌面 UI、浏览器预览和候选/发布包的真实验收仍属于 B7 边界。

## 15. 2026-10-04 B5 U11 全局粘贴入口

本批完成 `R27-U11` 的全局粘贴子项，登记为 `UX-44`（Fixed locally）；不代表 `U10—U12` 或 B5 全部完成。

- 非文本输入、非弹层焦点下，`Ctrl+V`（Windows/Linux）或 `⌘V`（macOS）打开剪贴板草稿。输入框、textarea、select、嵌套 contenteditable 和弹层保留原生行为；IME、重复按键、已消费事件及额外修饰键不触发。快捷键面板复用已有“粘贴链接并创建任务”文案，无其他语言改动。
- 每次主动粘贴生成独立请求标识，修复固定 `clipboard` 标识导致第二次请求被忽略；监控提示和主动粘贴均保留 `clipboard-` 来源，新建窗口探测/创建不会误标为手工输入。同一请求仍不会重放并覆盖用户编辑。
- 关闭新建窗口后的焦点恢复移到 Radix `onCloseAutoFocus`，在焦点限制释放后回到原入口。应用菜单、主动粘贴和监控确认复用现有打开路径。

验证记录（Windows，HEAD 与原有 B0—B5 改动共存，工作区 dirty）：

- `pnpm exec vitest run src/components/shell/shell-keys.test.tsx src/components/shell/ShortcutPanel.test.tsx src/components/shell/NewDownloadDialog.test.tsx`：55/55；`pnpm test:frontend`：78 个测试文件、456/456。
- `pnpm typecheck`、`pnpm build`、`pnpm check:i18n`、`pnpm check:docs` 和本轮 7 个 TS/TSX 文件的 `pnpm exec biome check` 通过。`pnpm lint --max-diagnostics=5` 仍因未触碰文件的既有 CRLF/格式差异失败（45 项），未修改这些文件来消除诊断。
- Tabbit/Playwright 浏览器 mock 验证：搜索框内粘贴不读取剪贴板，弹层内按键不触发新任务，连续粘贴更新链接，批量内容进入批量草稿，关闭后焦点回到原入口；检查了 1280×800 和 390×844 视口。本次使用页面临时剪贴板 fixture，不读取或写入系统剪贴板，不能代替原生 WebView、系统权限或跨平台候选包验收。
- 本批未改 Rust 或 IPC，未重跑 Rust/Specta；扩展、下载引擎和真实服务器不在本批验证范围。

## 16. 2026-10-05 B5 U10 BT 多文件待选择流程

B5 的 `U10` 已完成本地实现，登记为 `UX-45`（Fixed locally）。本批复用现有 `check_url` IPC 动作和 `bt_file_selection_required` 稳定错误码，保持恢复动作枚举与任务授权边界不变。

- BT 多文件磁力任务出现待选择错误时，任务行、恢复中心、上下文菜单、命令面板和详情动作统一显示“选择文件”；动作直接打开任务详情并保留稳定错误提示。
- 详情面板不再依赖运行时 torrent session 才显示文件清单。runtime snapshot 为空或加载失败时，仍可勾选相对路径并保存；保存按钮要求至少一个文件，Rust 与浏览器预览都拒绝空选择。
- 增加任务详情和恢复动作回归测试；修复文件行复选框与 `label` 双重关联造成的二次切换，保证一次用户点击对应一次选择状态变化。

本地收尾验证（Windows，工作区 dirty）已完成：定向 BT 测试 2 个文件、36/36；`pnpm test:frontend` 通过 78 个文件、458/458；`pnpm typecheck`、`pnpm build`、`pnpm check:i18n`、`pnpm check:docs` 和 `git -c core.safecrlf=false diff --check` 通过。目标文件的 Biome 检查仍受既有 CRLF/格式差异影响，未进行整文件格式化。真实 BT 对端、候选包 WebView 与多文件磁力元数据流程仍归 B7；在这些证据完成前保持 `Fixed locally`。

随后完成 `U11` 批量重复策略与 `U12` 分类/边界文案；剪贴板类型过滤、后台气泡、筛选全集操作和快捷键定义继续分项安排；B7 保留候选包原生焦点、剪贴板和真实 BT 验收。

## 17. 2026-10-05 B5 U11 批量重复策略

B5 的 `U11` 批量重复子项已完成本地实现，登记为 `UX-46`（Fixed locally）。批量导入仍默认跳过重复任务；用户可在结果区显式创建全部重复项，继续复用现有 `import_urls` 的任务级重复授权。

- 默认预览和创建请求保持 `allowDuplicate: false`；结果区只为重复行显示“创建副本”按钮，点击后仅提交这些 URL 并设置 `allowDuplicate: true`。
- 成功创建的重复任务通过既有批次合并逻辑写回原结果，重新计算创建/失败/重复计数，并按 URL 出现次数从输入框移除已处理行；仍失败或仍重复的行保留，便于再次处理。
- 按钮复用已有 `newDownload.createDuplicate` 文案，不新增 locale key；本批未改 Rust、数据库、IPC 或 Specta bindings。

验证记录（Windows，HEAD 与原有 B0—B5 改动共存，工作区 dirty）：

- `pnpm exec vitest run src/components/shell/NewDownloadDialog.test.tsx`：29/29；新增用例覆盖默认跳过、显式创建重复项、结果合并和输入清理。
- `pnpm test:frontend`：78 个文件、459/459；`pnpm typecheck`、`pnpm build`、`pnpm check:i18n`、`pnpm check:docs` 和 `git -c core.safecrlf=false diff --check` 均通过。完整前端测试在与构建并行运行时曾出现一次 `use-task-events` 5 秒超时，停止并行任务后独立重跑通过；该现象未复现为代码失败。
- 全库 `pnpm lint` 仍可能受工作区既有 CRLF/格式差异影响；本批不整文件格式化或清理无关改动。候选包、真实文件冲突和跨平台 WebView 行为仍归 B7。

## 18. 2026-10-05 B5 U12 队列与分类文案收尾

B5 的 `U12` 已完成本地实现，登记为 `UX-47`（Fixed locally）。本批处理错误筛选的稳定分类、过时的语言覆盖说明，以及内网浏览器交接设置的授权边界文案。

- `failureKind` 在缺少后端 `failureCategory` 时改为按稳定 `errorCode` 映射，并与 Rust `failure_category_for_code` 保持一致；未知或缺失错误码统一归入 `other`，不再从英文错误消息猜测类别。
- `STABLE_LOCALES` 注释移除过时的“约 670 个键”数字，避免把历史规模当成当前契约。
- `en` 与 `zh-CN` 的浏览器设置改为“允许私有 URL 交接”，并明确说明交接开关不授予下载权限，具体目标仍需按任务授权；未扩大内网策略或修改 beta locale，符合本轮语言维护边界。

验证记录（Windows，HEAD 与原有 B0—B5 改动共存，工作区 dirty）：

- `pnpm exec vitest run src/stores/task-query.test.ts`：7/7；覆盖协议前缀、特殊码、未知码和本地化错误消息不参与分类。
- `pnpm test:frontend`：78 个文件、460/460；`pnpm typecheck`、`pnpm build`、`pnpm check:i18n`、`pnpm check:docs` 和 `git -c core.safecrlf=false diff --check` 均通过。
- `pnpm exec biome check src/stores/task-query.ts src/stores/task-query.test.ts src/i18n/index.ts` 通过；U11 相关文件的既有 CRLF/格式差异仍不整文件格式化，也不归因于 U12。真实内网目标、候选包授权提示和 beta locale 母语复核继续归 B7。

## 19. 2026-10-05 B5 剪贴板文件类型过滤

B5 的剪贴板文件类型过滤已登记为 `UX-48`（Fixed locally）。本批只收窄常驻剪贴板监控的事件入口，不改变用户主动粘贴或手动输入的下载能力。

- 后端剪贴板提取对 HTTP、HTTPS、FTP、FTPS、SFTP、WebDAV 和 WebDAVS URL 使用固定的常见下载扩展名白名单；磁力链接和本地 `.torrent`/`.meta4`/`.metalink`/`.mpd` 清单保持可识别。
- 扩展名只从 URL path 的最后一个非空段判断，忽略查询参数和片段；无扩展名 URL、仅在查询参数中携带文件名的 URL 以及未知扩展名 URL 不再产生监控事件。多链接剪贴板只保留命中的 URL，并重新确定 `primaryUrl`。
- 过滤位于 Rust 提取层，避免普通网页 URL 进入前端 toast；没有复用浏览器捕获的 `fileExtensions` 设置，也没有新增 IPC、数据库字段或 locale 文案。全局 `Ctrl/⌘+V` 主动粘贴路径保持原有行为。

本地验证（Windows，工作区 dirty）：

- `cargo test --locked --manifest-path src-tauri/Cargo.toml --test clipboard`：11/11 通过；覆盖未知扩展名、查询参数/片段、大小写扩展名、混合多链接、磁力链接和既有协议兼容性。
- 本批只修改剪贴板提取与其 Rust 集成测试，未修改 IPC/Specta bindings；完整 Rust 门禁、真实系统剪贴板、隐藏托盘可见性和候选包行为仍按 B7 计划验收，状态保持 `Fixed locally`。

## 20. 2026-10-05 B5 全选一致性与后台通知收尾

B5 的筛选全集操作和隐藏态剪贴板反馈已完成本地实现，登记为 `UX-49`（Fixed locally）。本批收紧批量选择的范围语义，避免游标分页列表只对当前已加载页生效；系统通知只作为窗口不可见时的通用提示，不能替代用户确认或泄露剪贴板内容。

- 新增共享分页选择器，固定当前筛选/搜索/排序快照，沿着游标加载所有后续页面，把实体写入已有 `taskById` 缓存后再发布选择结果；重复游标、查询变化、组件卸载和过时响应均停止应用结果，待删除任务不会重新进入选择集合。
- `Ctrl/⌘+A`、任务列表右键菜单和命令面板复用同一实现。当前页已全选但仍有后续页时入口保持可用；到达队尾后才禁用，加载期间显示统一的进行中状态。
- 隐藏窗口收到剪贴板检测时只保留最新 payload，并以 5 秒窗口合并通用系统通知；通知不包含 URL，窗口恢复可见后仍由应用内确认 toast 承担“使用此链接”动作。系统通知权限或原生插件失败只记录诊断，不影响剪贴板确认流程。
- 分页工具、重复游标保护、查询过时保护和剪贴板定向行为均有前端回归测试；通知运行时检测从通用 runtime 模块读取，浏览器 mock 不需要伪造 Tauri 命令模块导出。

本地验证（Windows，HEAD 与原有 B0—B5 改动共存，工作区 dirty）：

- `pnpm typecheck` 通过。
- `pnpm exec vitest run src/lib/select-all-matching.test.ts src/hooks/use-clipboard-link-prompt.test.ts src/components/shell/ShortcutPanel.test.tsx src/components/tasks/TaskList.scroll.test.tsx`：4 个文件、20/20 通过。
- `pnpm test:frontend`：79 个测试文件、464/464；`pnpm build` 成功生成生产 bundle。
- `pnpm check:i18n` 通过，7 个 locale 的新增选择/失败文案完整。
- `pnpm check:docs` 与 `git -c core.safecrlf=false diff --check` 通过；全局 `pnpm lint` 仍会报告工作区既有 CRLF/格式差异，本批未整文件格式化。
- 候选包、系统通知权限、托盘隐藏态的 `visibilitychange` 行为、原生剪贴板和真实批量任务仍按 B7 验收；在这些证据完成前状态保持 `Fixed locally`。

## 21. 2026-10-10 B5 增强实施记录

本批完成用户确认的 B5 增强首版，登记为 `UX-50`（Fixed locally）。包含按站点复用、批量编辑、任务预设（FE-01）、保存视图（FE-04）与统一快捷键；不扩张下载权限或浏览器扩展能力。

- **站点配置**：显式保存与应用，按协议、主机、有效端口组成的精确 origin 匹配；只复用允许的非敏感请求头，排除 Cookie、Authorization、X-*，去掉 Referer 查询与片段。应用保留当前任务独有的敏感头，并重新走既有探测与创建校验。
- **任务预设**：新建单任务／批量共享同一组字段，详情可从已持久化任务保存。支持另存、更新、重命名、删除及替换确认；预设不含密码、Cookie、私钥、文件摘要或网络授权。代理继承／关闭／自定义语义保留，应用清空代理凭据；之后的手动编辑优先，不修改已创建任务。
- **批量编辑（FE-03）**：固定去重 ID 快照并排除待删除项，每批最多 100 个；优先级、限速、分类、计划开关可独立选择“不修改／设置值”，限速与分类另有明确清除。SQL 在写入边界保留省略字段，终态或不存在任务跳过；结果保留成功／跳过／失败／未处理，停止只阻止后续批次，重试只提交未完成 ID。
- **保存视图**：保存现有导航、搜索、筛选、排序，不保存结果、cursor 或选中 ID；应用原子切换查询并推进 epoch、清除选择，复用既有分页／过时响应保护。版本不支持或内容损坏的记录只提示不可应用，保留原始数据。
- **快捷键**：处理器、任务行、命令面板提示及帮助列表共用 registry，统一平台修饰键和 IME／已消费事件保护。修复 `Mod+Enter` 被普通 Enter 吞掉的问题；帮助面板的多组按键可换行，390px 下标签位于按键上方。

配置库通过新增 SQLite 表持久化，完整数据库备份随库保存；首版没有独立配置导入／导出，也没有扩展既有选择性恢复的字段范围。文件路径仍在实际创建时按现有路径策略裁决。

本地验证（Windows，基线 `HEAD = f87c200`，工作区 dirty；既有未跟踪 `biome-report.txt` 保留）：

- `pnpm typecheck`、`pnpm test:frontend`：83 个文件、477/477 通过；覆盖预设实际创建参数、同源／敏感边界、部分成功重试、分块停止／卸载、视图选择重置和快捷键。
- `cargo test --locked --manifest-path src-tauri/Cargo.toml -j 2`：库、集成与文档测试通过，两个规模基准按设计忽略；最后的稳定错误封装和重连持久化加固后，重跑 `cargo test --locked --manifest-path src-tauri/Cargo.toml -j 2 --test workflow_library --test migration_integrity`，18/18 通过。
- `cargo clippy --locked --manifest-path src-tauri/Cargo.toml --all-targets -j 2 -- -D warnings`、`cargo fmt --all --manifest-path src-tauri/Cargo.toml -- --check` 通过。
- `pnpm build`、`pnpm check:bundle` 通过；`pnpm test:release-tools` 通过 83 个工具测试和 19 个 i18n 检查测试。
- `pnpm check:i18n --stable-only` 通过。新增可选参数不改变默认检查；`pnpm check:i18n` 因五种 beta locale 缺少 40 个新增键失败，本轮遵照用户的英文／简体中文维护规则，不宣称七语言或聚合门禁通过。
- `pnpm specta` 成功，`pnpm check:bindings` 重新生成前后 SHA-256 一致（`6DD5F73D3132CD77176451C572891BF1B6210FACD6E5A0455C1440099837AA21`）；命令最终因本批未提交的 64 行新增 bindings 与 HEAD 不同返回 1，已审阅为预期 IPC／类型增量，不是生成漂移。
- 新增配置库／批量／视图／快捷键文件的定向 `pnpm exec biome check` 通过；`pnpm lint --max-diagnostics=5` 仍报告未触碰文件的既有 CRLF／格式差异。完整 lint 和 `pnpm verify` 不作为通过结论。
- Tabbit/Playwright 浏览器 mock 验证保存、更新、复制、重命名、删除、刷新后复用、端口不匹配不列出配置、敏感字段不进入配置库、批量成功／终态跳过、弹层阻止新建快捷键、视图应用清除选择。检查 1440×900、1280×860 和 390×844；独立审阅发现快捷键帮助横向裁切，修复后桌面与窄窗口复核为 resolved（`ship` 只覆盖该修复项）。截图是浅色浏览器预览，不能替代 Tauri、暗色或跨平台候选验收。

原生 IPC、候选升级／完整备份恢复、运行时批量限速、真实同源／跨源请求及系统行为继续按 [B7 增强场景](b7-acceptance-matrix-2026-10-02.md#9-b5-增强候选包与真实环境场景) 验收。

## 22. 2026-10-10 B6 实施记录

B6 分成 BT 做种与媒体成品两个工作包，本地实现与自动化完成，关联 `R27-F06` → `FUN-40`、`R27-F07` → `FUN-41`、`R27-P02` → `PERF-20`，均登记为 `Fixed locally`。工作区保留原有 B5 增强改动，基线为 `HEAD = f87c200`；本节只记录本轮 B6 的实现与当前验证，历史测试数字不作为本轮证据。

- **后台做种**：下载完成后把 API、独立 session 和文件租约交给 `BtSeedingManager`，普通下载 worker 返回并释放槽位。完成任务可显式开始／停止做种；开启前验证本地目录、文件大小、元数据 identity 和 piece 哈希。手动停止持久化，重启只在 `auto_resume_on_startup` 开启时恢复已启用、非手动停止、文件完整且未到限制的任务。
- **累计策略与排空**：追加 migration 015 保存首次做种时间、累计上传／持续时间和停止原因。比例与时间限制按累计值计算；取消、删除、退出、更新及系统电源命令先取消并等待做种 owner 和 storage 租约。超时返回 `task_stop_pending` 并保留 owner，不把超时当成已经停止。
- **完成动作与反馈**：通知／打开目录等动作按下载完成轮执行；退出、关机、睡眠、休眠及用户命令等待后台做种结束。详情显示上传状态、累计时间、停止原因和等待提示，无限做种有可见停止入口。
- **隔离与写入**：保留每任务 session、代理和 DHT/端口隔离；逐 torrent 下载限速与 BT/HTTP 共享下载预算保留，上传改为现有 session 共用上传桶，修改上限保留已消耗的预算。普通运行快照最多每秒一次且按变化写入，piece 位图最多每 5 秒计算／持久化一次，空闲累计时间每 5 秒检查点。
- **HLS 字幕与直播**：选中 WebVTT 先校验／合并本地字幕和时间映射，再显式映射为 `mov_text`，音视频继续复制，语言映射为 MP4 标签。直播缺片上限每任务 0–100，默认 0；仅对重试耗尽的可容忍网络／404 缺片继续，持久化媒体序号、时间范围和稳定原因。空闲进入可恢复等待；“完成录制”用保存的本地片段及选中外部轨道收尾，先检查 finish 标志，不要求源站恢复。
- **无 ffmpeg HLS**：只允许单路 TS、无 init map、无加密／discontinuity／外部轨道的组合；成品拼接前验证 188 字节包、未加扰、完整且 CRC 有效的单节目 PAT/PMT 和一致的流布局。输出 `.ts`，使用既有持久化／无覆盖发布路径；保守拒绝不完整 PSI、布局变化或无法验证的容器。
- **DASH 选择**：探测返回轨道 ID、类型、语言、角色、码率、分辨率、编码及默认选择。创建时重新探测、验证 ID，把视频／音频／WebVTT 字幕选择与任务放在同一事务；恢复按保存 ID 构建片段计划。静态/VOD 的受限 SegmentTemplate/SegmentList/SegmentBase 保留；live、多 Period、SegmentTimeline、DRM、TTML、fMP4 `wvtt` 和不安全轨道继续稳定拒绝。DASH 缺少 ffmpeg 不做直接拼接。
- **清单识别**：后缀无法判断时使用共享工厂、代理、凭据、任务网络策略和 origin-safe 路径读取 Content-Type；缺失／通用二进制类型才读取最多 4 KiB 前缀，正文读取总截止 15 秒。识别 HLS、DASH、torrent、Metalink，不扫描 HTML 或网页中的视频。
- **界面**：新建页提供 DASH 画质／音频／字幕选择和 HLS 缺片配置、字幕输出提示；详情显示媒体输出、已选轨道和缺口。新增文案只维护 `en`／`zh-CN`，符合用户语言规则。

本地验证（Windows，工作区 dirty，ffmpeg／ffprobe 位于 `D:/env/ffmpeg/bin`）：

- `pnpm typecheck`、`pnpm test:frontend`：最终绑定生成后的重跑通过，83 个文件、482/482；新增 DASH 键盘选择／创建参数／axe、旧探测快照缺省轨道、HLS 字幕与缺片配置、直播空闲完成、已完成 BT 停止与等待提示回归。
- `pnpm build`、`pnpm check:bundle` 通过；首屏 gzip JS 328.3 KiB／340 KiB 预算。10 个 B6 前端目标文件的 `pnpm exec biome check` 通过。
- `cargo fmt --all --manifest-path src-tauri/Cargo.toml -- --check`、`cargo clippy --locked --manifest-path src-tauri/Cargo.toml --all-targets -j 2 -- -D warnings` 通过。
- `cargo test --locked --manifest-path src-tauri/Cargo.toml -j 2`：386 个库测试及全部集成、文档测试通过，3 个性能用例按设计忽略。最终重跑已包含旧探测快照缺少 `dashTracks` 的反序列化回归和 migration 015 的重连断言；本机使用 `-j 2` 限制 Windows 编译并行。
- 完整 Rust 命令中的 `bt_seeding` 4/4、`media_b6` 7/7、`migration_integrity` 14/14 通过。两路 HTTP 下载与两个做种 owner 并行且共用下载预算；完成任务 piece 校验、手动停止／恢复、累计限制和相同快照不写库有直接断言。媒体用例实际移除子进程 PATH／ffmpeg 环境验证 TS 下载及 fMP4／外部音轨／DASH 依赖拒绝；认证 torrent／Metalink 和跨源秘密剥离通过。HLS 与 DASH 成品经 ffprobe 检查 `mov_text`／语言／容器；离线 TS 与缺片预算有直接断言。
- `pnpm specta` 成功；`pnpm check:bindings` 再生成前后 SHA-256 均为 `63FAB63E183382B287165D196EEED4F8A15A315B822C1E3907BF7780AB7D1998`。命令因 B5／B6 未提交的 bindings 与 HEAD 相差 105 行返回 1；已审阅为预期 IPC／模型增量，生成器重复运行没有漂移。旧快照兼容默认值生成可选 `dashTracks`，前端按空列表处理。
- `pnpm check:i18n --stable-only`：1992 个基准键通过。`pnpm check:i18n` 因五种 beta locale 各缺 76 个键（B5 40＋B6 36）失败；遵照用户范围未修改 beta locale。`pnpm lint --max-diagnostics=5` 仍报告未触碰文件的既有 CRLF／格式差异；不宣称完整 `pnpm verify` 通过。
- Tabbit/Playwright 浏览器 mock 检查了 1280×860 的 DASH 三类选择、390×844 的选择保留与 HLS 缺片输入；选择器和对话框无横向溢出。`impeccable detect src/components/shell/NewDownloadDialog.tsx src/components/shell/TaskDetails.tsx --json` 返回 `[]`。浅色浏览器预览不能代替原生 WebView、暗色或跨平台 GUI E2E。
- `pnpm check:docs` 与 `git -c core.safecrlf=false diff --check` 通过。

隔离 session 基线命令：`cargo test --locked --manifest-path src-tauri/Cargo.toml --test bt_seeding isolated_sessions_performance_baseline -j 2 -- --ignored --nocapture`，1/1 通过。本机 Windows debug/headless 测试，每个 torrent 为已完整的 4096 字节本地文件，各规模观察 10 秒；2/5/10 组在同一进程顺序执行，RSS 包含运行时及可能保留的分配。数据库 trigger 统计 `torrent_tasks`／`torrent_runtime_snapshots` 的实际 UPDATE，会增加测量开销。

### B6 加固复核（2026-10-10）

- 代码审阅复核做种 owner 注册、超时后保留 owner、退出排空、破坏性完成动作门控、任务网络设置阻断，以及 HLS/DASH 重定向后的资源基址和跨源凭据剥离；未发现需要改变产品边界的新问题。
- `cargo clippy --locked --manifest-path src-tauri/Cargo.toml --all-targets -j 2 -- -D warnings` 首次发现 `media_b6` 两处测试将同步 Mutex guard 带过异步数据库关闭。已把断言限制在独立作用域，随后 Clippy 与 fmt 检查通过；这只调整测试锁生命周期，不改运行行为。
- 修复后 `cargo test --locked --manifest-path src-tauri/Cargo.toml -j 2` 完整通过：386 个库测试、所有集成和文档测试；B6 `bt_seeding` 4/4、`media_b6` 9/9、`migration_integrity` 14/14。Windows 首次重链耗时 16 分 49 秒；ffmpeg/ffprobe 可用。完整前端单线程复跑 83 个文件、482/482 通过，类型检查及生产构建通过。
- `pnpm specta` 成功；`src/generated/bindings.ts` 再生成前后 SHA-256 为 `63FAB63E183382B287165D196EEED4F8A15A315B822C1E3907BF7780AB7D1998`。`pnpm check:bindings` 仍因 dirty 工作区相对 HEAD 的 105 行 B5/B6 预期 bindings 增量退出 1，连续生成无漂移。
- `cargo fmt --all --manifest-path src-tauri/Cargo.toml -- --check`、稳定语言检查（1992 键）、`pnpm check:docs`、`git diff --check` 通过。所有命令基于 dirty Windows 工作区；真实 peer、候选包、直播站点、跨平台电源及 WebView 行为未验证，继续由 B7 验收。

| 并行隔离 session | 全部移交做种耗时（ms） | 进程 RSS（字节） | 10 秒 WAL 文件净增长（字节） | 运行 UPDATE 次数 | DHT IPv4 路由表条目范围 | DHT 未完成请求范围 |
| --- | ---: | ---: | ---: | ---: | --- | --- |
| 2 | 73 | 28,971,008 | 57,680 | 7 | 21–49 | 89–91 |
| 5 | 194 | 33,431,552 | 140,080 | 17 | 53–70 | 53–147 |
| 10 | 361 | 38,539,264 | 0 | 27 | 5–94 | 5–177 |

各 session 的 DHT ID 不同，IPv6 路由表本次均为 0。WAL 数字是文件长度净差，会受自动检查点和已有空间复用影响；10 session 的 0 不代表零写入，实际有 27 次运行 UPDATE。本轮没有真实 torrent peer 上传／下载负载，也没有共享 session 对照，因此不据此认定隔离开销可忽略或重构为共享 session。

候选包、真实 peer/DHT、真实直播、多语言 DASH、实际播放以及退出／更新／系统电源操作仍按 [B7 B6 场景](b7-acceptance-matrix-2026-10-02.md#10-b6-真实做种与媒体候选包场景) 验收，不据本地 fixture 改为 `Closed`。旧版已完成 BT 若未缓存 torrent 元数据，会明确提示元数据缺失；首版不会后台重新解析网页或从不可信来源补齐。

## 23. 2026-10-10 后置开发项收尾

本批针对审计中仍可在本地完成的架构与性能项收尾，保持 B7 的真实环境边界不变。

- **PERF-16 路径预留**：创建事务不再先读取所有活动任务及文件的 `final_path`。路径规划只携带当前尝试已知的冲突候选，最终由既有 partial UNIQUE 索引裁决并在冲突后重新规划；`list_task_records` 与浏览器实时活动快照均限制为 `MAX_TASK_PAGE_SIZE`（500）。这样创建路径不再随活动任务总量物化整表字符串，no-clobber 与并发回滚合同保持不变。
- **ARC-66 状态字段**：新增显式 `TaskTransitionInput` 与 runtime 变体，生产迁移调用分别传递已下载字节、瞬时速度、健康摘要、错误信息和事件消息。兼容包装保留给旧测试/外部调用；HTTP 分段失败会保存当前进度，错误分类仍统一由稳定错误解析器生成。
- **ARC-67 UI 回调**：窗口关闭回调只做快速状态检查、原子 pending 去重和异步任务派发；设置与任务统计通过 Tauri runtime 查询。托盘“打开下载目录”命令改为异步读取设置后再打开路径，避免 SQLite busy 阻塞 UI 线程。
- **PERF-13 依赖复核与收敛**：reqwest 改用 `rustls-no-provider`，russh 改用 `ring`，应用启动时显式安装统一的 rustls `ring` provider。当前 `cargo tree -e features -i aws-lc-rs` 只剩 librqbit 的 `sha1-ring`/`librqbit-sha1-wrapper` 路径及为其提供 Windows `prebuilt-nasm` 的直接 feature；reqwest、russh 和 FTPS 不再引入 aws-lc 的 rustls provider。由于 librqbit 仍需要 aws-lc-rs 做 torrent SHA-1，不能在本地安全移除该 crate；`PERF-13` 继续保持 Open，后续需等待上游哈希 feature 收敛或经验证的 Cargo patch。
- **稳定错误文案门禁**：补齐 B6 新增 13 个 `zh-CN` 稳定错误码源，并让同步脚本只生成 `en`／`zh-CN`，明确跳过 beta locale；新增回归断言保证 beta 文件不被改写。`pnpm test:release-tools` 重新覆盖 83 个工具测试，beta 完整性仍由 `pnpm check:i18n` 单独报告。

本地验证（Windows dirty 工作区）：清理 `src-tauri/target` 后执行 `cargo test --locked --manifest-path src-tauri/Cargo.toml -j 2`，386 个库测试、全部集成测试和文档测试通过，3 个性能基准按设计忽略；随后定向 `test_db_guard` 1/1、`bt_seeding` 4/4（另有 1 个显式 ignored 基准）与 `workflow_library` 4/4 通过。`cargo check --all-targets`、Clippy `--all-targets -D warnings` 和 Rust fmt 检查通过。守卫现在先尝试删除，再释放池句柄并重试；current-thread 测试 runtime 结束时仍可能留下由直接 `db::connect` 或后台句柄持有的文件，因此 `ENG-03` 继续保持 Partial。稳定错误同步器定向回归与 `pnpm test:release-tools` 83/83 通过；`pnpm check:i18n --stable-only` 通过，beta 完整性仍单独报告。`cargo tree -e features -i aws-lc-rs`、`-i ring` 与四条协议定向测试确认 provider 收敛；真实 1 万任务创建曲线、慢 SQLite UI 响应、SSH/BT TLS 握手和候选包仍归 B7/发布验收，不能由本地构建改记为 Closed。

## 24. 2026-10-12 B7 本机候选包与性能执行

本轮先建立独立的本机候选身份和证据目录：`scripts/prepare-b7-candidate.mjs --run-id windows-20261012` 生成被 `.gitignore` 忽略的 Tauri overlay，使用 `com.vibe.downloader.b7.windows-20261012`、关闭 updater artifacts，并把 NSIS 安装模式限定为 current-user。`logs/b7/windows-20261012/prepare-native-fixture.py` 在该数据目录写入 migration 013 的完成／暂停任务，自动更新、剪贴板、通知和托盘设置均关闭；它不会触碰日常 `com.vibe.downloader` 数据目录。

`pnpm test:release-tools` 重新通过 86/86（包含候选 overlay 测试）；`pnpm verify:extensions` 通过 3 个 candidate 浏览器包，capture=false；`pnpm check:bundle` 通过，首屏 JS gzip 328.3 KiB / 340 KiB，CSS gzip 15.9 KiB / 18 KiB；`pnpm check:docs`、`pnpm check:i18n --stable-only`、`pnpm verify:protocol-matrix` 和 `git diff --check` 通过。`pnpm typecheck` 和 `pnpm test:frontend` 通过，前端为 83 个文件、482 项断言。

PERF-16 harness 已加入 1k／10k 活动路径创建、同名冲突、全量路径读取对照和 500 条快照上限；本机 debug 的列表／搜索重复 5 次、创建路径各重复 25 次，原始路径和摘要见 [性能基线摘要](performance-baseline-results.md#05-2026-10-12-b7-创建路径与运行快照-050)。该数据只支持本机趋势判断，真实慢盘和 release UI 仍待候选环境。

本轮加固还把 `list_task_records_by_ids` 的 IPC 与数据库入口限制为最多 500 个任务 ID，浏览器实时快照限制为最多 500 条总记录；HLS 缺片容忍值在创建、持久化读写和运行时消费处统一限制为 `0..100`。新增路径快照、实时快照和 HLS 持久化边界回归，避免调用方绕过前端校验后重新引入无界输入。

候选包构建使用 `VIBE_BROWSER_PROFILE=candidate`、Rust `--locked -j 2` 和 Windows x64 目标；原生 host 自检与包哈希已和主程序、MSI、NSIS 一并回填 [B7 矩阵](b7-acceptance-matrix-2026-10-02.md#6-候选安装包和升级验收)。NSIS 已在工作区隔离目录完成静默安装、启动 smoke 和静默卸载，安装器返回码均为 0 且目录已移除；这不等同于 MSI 服务安装或生产数据目录验收。本机没有 `TAURI_SIGNING_PRIVATE_KEY` 或 Windows 代码签名配置，因此不生成 updater artifacts，也不宣称签名分发。`rc.0 → rc.1`、跨平台包、真实 BT／HLS／DASH、系统电源动作和 WebView 走查保持未执行。

完成候选与加固后的最后 Rust 回归：`cargo test --locked --manifest-path src-tauri/Cargo.toml -j 2` 在 Windows dirty 工作区通过 386 个库测试、全部集成／文档测试；3 个规模基准按设计 ignored。随后 `cargo fmt --all --manifest-path src-tauri/Cargo.toml -- --check`、`cargo clippy --locked --manifest-path src-tauri/Cargo.toml --all-targets -j 2 -- -D warnings` 和 `cargo test --locked --manifest-path src-tauri/Cargo.toml --test media_b6 -j 2`（10/10）通过。

## 25. 2026-10-13 B7 原生候选与短性能基线

本轮刷新 Windows x64 独立 candidate，包含上一轮最后的输入／快照／HLS 加固。使用 `VIBE_BROWSER_PROFILE=candidate`、`--locked -j 2` 和 release/LTO，生成并归档 NSIS／MSI、主程序和 host；哈希、源码指纹及 Code Signing `NotSigned` 状态见 [B7 矩阵 6.2](b7-acceptance-matrix-2026-10-02.md#62-2026-10-13-windows-候选刷新与原生-ui-基线)。Cargo 输出的 host 与打包使用的 staged host 分别记录，避免错误绑定包内哈希。NSIS 在工作区隔离安装与卸载均返回 `0`，安装目录已移除；应用内／包内 native-host 自检、schema 15 的 10k 备份验证和 capture=false 检查通过。

新增 `pnpm perf:native-ui` 与隔离数据、Windows 进程采样脚本，按身份、所有权、进程退出、WAL 检查点／完整性限制换库；适配 Windows MSIX Roaming 虚拟化和 PowerShell 5.1，保留每次检查点数据库及失败短跑日志。工具的 3 项回归验证拒绝生产／错误 identity、不改普通数据、拒绝活进程／busy／损坏 WAL，以及小样本 percentile 定义；Node 内置 SQLite 为可选加载，采集使用已验证的 Node 22.23.2。

正式 1k／10k 各 5 次进程启动与鼠标／方向键选择／搜索、10 秒滚动和 5 分钟空闲采样完成；所有测量退出记录的 7 个进程均结束。首屏观察上界 p50/p95 为 `1944/2007`、`1842/2346 ms`，搜索含防抖为 `345.4/348.2`、`365.4/377.1 ms`；DOM 最多 22 行，单次滚动 rAF 均值 `90.94`／`54.45`，5 分钟主进程 working set `50.77`／`72.13 MiB`。完整表、CPU、WAL、共享内存口径及原始样本见 [性能摘要](performance-baseline-results.md#4-2026-10-13-windows-原生-release-ui-短基线)。不把 CDP 观察称作精确 first paint，不把 rAF 称作实际呈现 FPS，也不以短基线关闭外部验收。

本轮新发现已登记：`PERF-21`（NSIS 多带约 23.1 MiB bindings 导出器）、`UX-51`（系统图标 PNG 被 CSP 阻断且缺少加载失败回退），在该快照时均为 Open；后续修复见第 26 节。本机后续还需补冷缓存／滚动 trace、1 小时空闲、NTFS 预分配与检查点压力；MSI 实际安装、updater／签名、跨平台、真实协议源和系统电源动作仍未执行。

本轮命令与门禁（Windows `10.0.28020`、dirty 工作区）：上述 candidate 构建和 `pnpm perf:native-ui --config logs/b7/windows-20261013-ui/tauri.b7.generated.json --exe logs/b7/windows-20261013-ui/installed-nsis/vibe-downloader.exe --reuse-fixture true` 通过；`pnpm test:release-tools` 为 JS 89/89、i18n 工具 19/19，`pnpm check:bundle`、`pnpm verify:extensions`（candidate、3 包、capture=false）、`pnpm check:i18n --stable-only`、`pnpm check:docs`、定向 `pnpm exec biome check scripts/perf/native-ui.mjs scripts/perf/native-ui-fixture.mjs scripts/native-ui.test.mjs package.json` 和 `git -c core.safecrlf=false diff --check` 通过。最后补上 CDP 连接失败时仍保留已启动 PID、清理失败记录 `cleanupPending`；重新隔离安装、核对主程序／包内 host 哈希并以 `--repetitions 1 --steady-seconds 0 --reuse-fixture true` 短跑两组验证，报告单独保留，没有替换正式五次基线。再次卸载成功、目录移除。此轮只增加采集脚本和文档；全量 Rust／前端测试证据沿用上一节同一产品代码快照，不报告为本轮重跑。

## 26. 2026-10-13 B7 候选图标与包装修复

`UX-51`：CSP 仅在图片来源中增加 `data:`，系统 PNG 可使用既有提取／缓存路径；`TaskRow` 对失败 URL 保留文件类型图标回退，进度更新不会重新挂载该破图，新图标 URL 仍可加载。新增 2 项组件回归，保留空 alt／隐藏装饰图标、原有尺寸和协议提示。

`PERF-21`：bindings 导出器新增显式 feature 和 `required-features`，默认 Cargo／Tauri 产品构建不启用。`pnpm specta` 显式启用并锁定依赖；实际导出成功，生成前后 SHA-256 均为 `63FAB63E183382B287165D196EEED4F8A15A315B822C1E3907BF7780AB7D1998`。`pnpm check:bindings` 首次执行导出成功后因工作区已有 B5/B6 bindings diff 返回 `1`，已逐项复核，没有本次新增漂移。提交准备阶段将这些合法生成变更纳入暂存基线，在 Windows dirty 工作区重新执行同一命令返回 `0`，生成哈希仍一致；未手改生成文件。

第一次新候选构建的主程序、Native Host 和 NSIS 完成，但 MSI 的 WiX ICE30 拒绝 Native Host 同时作为普通 Cargo bin 和 externalBin 安装到同一路径。verbose 重试与中间 MSI 的只读 File 表确认两份 host，保留失败记录，未跳过 ICE 验证。追加 `native-host`／`required-features`，仅独立 staging 脚本启用；产品 Tauri 默认只选主程序，sidecar 由 externalBin 纳入一次。`pnpm test:rust`／`pnpm verify:rust` 同步启用 host feature，CI Windows 包检查新增一份 host／无 exporter／自检返回码断言。新增 feature 的全目标 Clippy 和 host 7/7 单测本机通过；未执行远程 CI。

失败的 MSI 封装使 Tauri 未走到恢复主程序类型标识的步骤；确认生成主程序 `NotSigned` 后，仅恢复 SDK 的 25 字节标记中的 3 个字符，原文件和前后哈希保留在 `failed-msi-main.exe`／`bundle-marker-recovery.json`。最后修改仅是 bin 选取、构建脚本、测试与 CI 边界，主程序 Rust／前端源码及默认 enabled features 仍相同；重建 featured staged host 后复用已编译主程序执行 `tauri bundle`，不把重封装称为再次全量编译。

本轮 Windows dirty 工作区的 `pnpm typecheck`、`pnpm test:frontend`（83 文件、484 测试）、`pnpm build`、`pnpm test:release-tools`（JS 89／i18n 工具 19）、`pnpm check:bundle`、`pnpm check:i18n --stable-only`、`pnpm verify:extensions`（candidate、capture=false）、定向 Biome、`cargo fmt --all --manifest-path src-tauri/Cargo.toml -- --check`、`cargo clippy --locked --manifest-path src-tauri/Cargo.toml --all-targets -j 2 -- -D warnings` 与 `cargo test --locked --manifest-path src-tauri/Cargo.toml -j 2` 已通过。Rust 本轮重新执行，库测试 386、全部集成／文档通过，3 个规模基准按设计 ignored；Windows 测试重编译／链接为 15m54s。最后只调整 bin 构建边界，Rust 源码未变；最终补跑 featured host 全目标 Clippy 和 7/7 单测，release 工具重跑通过。Impeccable 机械检查对修改后的 TaskRow 无发现。

新包已完成验收：MSI File 表与 NSIS 安装清单均无 exporter／只有一份 host；NSIS 安装／卸载返回 `0`，目录已移除，host 自检与 capture=false 通过。实际原生 PNG 为 40×40，坏图回退且无重复提取 IPC；内联脚本／跨域连接继续被 CSP 拒绝，500px 视口无横向溢出／破图，截图人工复核通过，退出的 7 个进程均结束。`pnpm perf:native-ui --config logs/b7/windows-20261013-fixes/tauri.b7.generated.json --exe logs/b7/windows-20261013-fixes/installed-nsis/vibe-downloader.exe --repetitions 1 --steady-seconds 0` 与 `node logs/b7/windows-20261013-fixes/native-fixes-smoke.mjs` 通过；短跑只作功能复核。NSIS／MSI 较 6.2 快照体积减少 33.12%／33.31%，完整哈希、重封装来源、SDK 警告、截图和局限见 [B7 矩阵 6.3](b7-acceptance-matrix-2026-10-02.md#63-2026-10-13-图标与二进制包装修复验收)。两项记为 Fixed locally；远程 CI、MSI 服务安装、签名／updater、跨平台和真实协议验收仍未执行。
