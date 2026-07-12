# 项目改进审计

最后更新：2026-07-10
适用版本：Vibe Downloader `0.2.0`
审计对象：当前工作区代码、配置、测试、构建脚本与产品文档（包含尚未提交的改动）

本文是项目当前唯一的**全局风险与优先级基线**。它从用户交互便捷性、程序功能丰富性和完整性、项目架构鲁棒性和稳定性、程序运行效率四个维度回答三件事：目前已经做到什么、仍有哪些可验证缺口、应该按什么顺序整改。

本文不是变更日志，也不把路线图中的计划描述为已实现能力。方向性规划以 [ROADMAP.md](ROADMAP.md) 为准；产品与设计约束以 [PRODUCT.md](../PRODUCT.md) 和 [DESIGN.md](../DESIGN.md) 为准；专项审计是特定日期的证据补充，当旧结论与本文冲突时，以本文和当前实现为准。

## 一、执行摘要

### 1. 总体判断

Vibe Downloader 已经越过“HTTP 下载 MVP”阶段，具备真实桌面下载管理器所需的大部分骨架：HTTP/HTTPS 主路径较成熟，多协议入口、SQLite 持久化、队列调度、限速、恢复动作、浏览器交接、任务诊断、虚拟列表、响应式桌面 UI 和中英文界面均已落地。

当前主要矛盾不是功能数量不足，而是**已实现能力与公开发布可信度不匹配**：浏览器 Native Messaging host 尚未被可靠装入安装包；发布签名和商店链路未闭环；若干已确认的崩溃、数据恢复、限速并发和 HLS 内存问题仍在；非 HTTP 协议的可靠性、诊断和自动化验证明显弱于 HTTP；质量门禁在本次审计快照下也不是全绿。

因此，当前版本适合作为积极开发中的 `0.2.0`，但还不应被描述为“可公开稳定发布、全协议同等成熟、可替代 IDM 的正式版本”。

### 2. 四维结论

| 维度 | 当前成熟度 | 主要优势 | 首要缺口 |
| --- | --- | --- | --- |
| 用户交互便捷性 | 良好，但存在高影响边界缺陷 | 主任务流清晰；键盘、鼠标、命令面板路径齐全；主列表在窄屏可用；主题和状态表达成熟 | 旧浏览器预览数据可令整页崩溃；设置页副作用风险、错误边界文案键错误、移动设置页过密 |
| 功能丰富性与完整性 | 功能面宽，成熟度不均 | HTTP、队列、限速、代理、凭据、多协议、浏览器交接和诊断能力丰富 | 非 HTTP 协议可靠性仍未与 HTTP 对齐；多项协议边界必须明确；发布链路尚未形成可交付功能 |
| 架构鲁棒性与稳定性 | 核心防护较强，仍有发布级风险 | 事务化状态机、运行时锁、SSRF 防护、加密凭据、关闭收敛和恢复模型已建立 | 数据库迁移失败会先自动重建再告知用户；HLS 绕过统一 ffmpeg 配置；错误类型化和模块拆分未完成；质量门禁未全绿 |
| 程序运行效率 | 已有正确方向，缺生产规模证据 | 游标分页、虚拟列表、事件节流、批量查询、WAL、HTTP client 复用均已存在 | token bucket 有并发覆盖竞态；加密 HLS 单 worker 可接近 1 GiB 瞬时内存；缓存无界；大库搜索和批量删除缺规模验证 |

### 3. 当前优先级数量

| 优先级 | 数量 | 含义 |
| --- | ---: | --- |
| P0 | 2 | 阻断公开发布或核心发布能力，必须先闭环 |
| P1 | 7 | 可能造成崩溃、数据丢失、行为错误、显著资源风险或错误成熟度承诺，发布前应完成 |
| P2 | 15 | 影响可维护性、长期规模、诊断一致性或部分设备体验，应进入近期迭代 |
| P3 | 1 组 | 产品广度增强，不应挤占可靠性和发布闭环 |

最高优先事项：

1. 将 `vibe-native-host` 作为真实安装包组成部分交付，并完成安装后浏览器交接冒烟测试。
2. 闭环扩展商店身份、扩展签名、权限审查、updater 演练及 OS 签名或明确的 unsigned 策略。
3. 修复旧预览任务崩溃、数据库迁移自动清空、HLS ffmpeg 设置失效、限速并发竞态和 HLS 内存峰值。
4. 让 lint、bindings、Windows Rust 集成测试等质量门禁恢复为可重复的全绿状态。
5. 在继续扩协议前，补齐非 HTTP 协议的恢复、代理、凭据、校验和故障诊断矩阵。

## 二、审计方法与证据口径

### 1. 审计方式

- 静态核查 `src/`、`src-tauri/`、`browser/`、`.github/workflows/`、`scripts/`、Tauri 配置和现有文档。
- 对前端主任务流、设置页、新建下载、命令面板和窄屏布局进行浏览器预览检查；主任务视图在 `390px` 宽度下未出现页面级横向溢出。
- 执行 TypeScript、Biome、Vitest、Vite build、Specta bindings、Rust check/clippy/test 和 i18n 检查。
- 对关键风险追到具体实现，不仅依据旧审计或 README 声明。

### 2. 证据等级

| 标记 | 含义 | 本文处理方式 |
| --- | --- | --- |
| 已确认缺陷 | 代码路径可直接证明存在错误或危险行为 | 直接进入 P0/P1/P2，并给出验收条件 |
| 发布阻断 | 配置或交付链缺失，开发环境可用不能证明安装包可用 | 必须以真实安装包和实机流程验收 |
| 能力边界 | 当前实现明确不支持，并非实现错误 | 写入产品说明，不得宣传为已支持 |
| 待基准验证 | 设计上可能随规模恶化，但当前没有量化数据 | 先建立基准，再决定是否重构 |

### 3. 审计限制

- 本次快照的工作区有大量未提交改动；结论描述的是**当前文件状态**，不等同于某个 Git commit。
- 未完成 Windows、macOS、Linux 三个平台安装包的全量实机安装、升级、卸载和浏览器商店验证。
- 性能部分没有以生产硬件跑完 `1k/10k/50k` 历史任务及 `100` 活跃任务基准，因此不会把所有潜在热点写成已发生性能故障。
- beta 语言的大量缺键是已知范围；当前主动维护语言仍仅为 `en` 和 `zh-CN`。

## 三、质量门禁实测

| 检查 | 结果 | 说明 |
| --- | --- | --- |
| `pnpm typecheck` | 通过 | TypeScript 类型检查通过 |
| `pnpm lint` | **失败** | 122 个文件中有 4 个 error、25 个 warning；3 个文件需格式化，1 个 import organize error，25 个均为 `useExhaustiveDependencies` |
| `pnpm check:i18n` | 主动语言通过 | `zh-CN` 与 `en` 一致；beta 语言仍有大量已知缺键警告 |
| `pnpm test:frontend` | 通过 | 16 个测试文件、47 个测试通过 |
| `pnpm build` | 通过 | 前端生产构建通过 |
| `pnpm check:bindings` | **失败** | 检出 `extract_system_file_icon` / `SystemFileIcon` 绑定漂移；生成文件已刷新，但在纳入版本并重新验证前门禁仍不闭环 |
| `cargo check --manifest-path src-tauri/Cargo.toml` | 通过 | Rust 编译检查通过 |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | 通过 | Clippy 零 warning |
| Rust 测试 | **未完整通过** | 165 个 library tests 和前序集成测试通过；Windows `dash_engine` 测试二进制在执行测试前以 `0xc0000139 STATUS_ENTRYPOINT_NOT_FOUND` 退出 |

结论：当前代码不是“构建全面失败”，但也不能称为“CI/本地质量门禁全绿”。尤其应区分测试断言失败与 Windows 测试二进制装载失败：后者仍然会阻止可信发布，不能以“测试没跑到”视为通过。

## 四、已确认优势

以下能力应在整改中保留，不应因重构而退化：

- HTTP probe 使用 HEAD 并在需要时回退到 Range GET；HTTP 下载支持未知大小、Range 分段、动态拆分、续传校验、重试、checkpoint、自动重命名和完整性校验。
- 调度器支持最大活跃任务、按 host 槽位、优先级、计划窗口、定时限速和完成动作；全局限速与逐任务限速已有统一组合模型。
- SQLite 状态转换包含事务化/条件更新；下载任务有逐任务运行时锁；关闭路径会收敛任务并 flush 进度。
- 浏览器交接边界具备 SSRF、重定向逐跳复验、header allowlist、凭据拒绝和加密持久化等防护。
- FTP/SFTP/WebDAV 凭据使用 ChaCha20-Poly1305 加密，并包含旧明文迁移；SFTP 使用 TOFU host-key 验证。
- 前端已拆分 task data/UI/speed history store，使用游标分页、虚拟化无限滚动、增量事件和批量文件/校验和加载。
- UI 支持状态筛选、搜索、排序、多选、批量动作、队列重排、命令面板、恢复动作、任务诊断、快捷键和响应式 detail panel/drawer。
- 主题系统使用 OKLCH token，覆盖 8 个强调色和明暗模式；焦点态、ARIA、键盘路径及 `prefers-reduced-motion` 处理整体较完整。
- 高频 progress 事件有 `250ms` 发射闸门；SQLite 使用 WAL；HTTP client 和部分协议资源已复用。

## 五、用户交互便捷性

### 1. UX 技术健康分

该分数采用 `impeccable audit` 的前端技术口径，只反映可访问性、前端性能、响应式、主题和反模式，不代表整个产品发布成熟度。

| 子维度 | 分数 | 结论 |
| --- | ---: | --- |
| 可访问性 | 3/4 | 主路径有语义、ARIA、键盘和焦点态；但多项 Biome a11y 规则被全局关闭，缺自动回归保障 |
| 前端性能 | 3/4 | 虚拟列表、store 拆分和事件批处理方向正确；缓存、effect 风险和规模基准仍需处理 |
| 响应式 | 3/4 | 主任务视图在窄屏可用；设置页仍过密且依赖横向滚动的分区选择器 |
| 主题系统 | 4/4 | OKLCH token、明暗模式和 8 个强调色覆盖完整，未发现主要主题断裂 |
| 反模式控制 | 3/4 | 产品型密集工具风格成立，未落入卡片墙、渐变文字或营销页模板；少量装饰效果需持续克制 |
| **合计** | **16/20** | **良好：界面基础可信，需集中修复边界状态和自动化保障** |

### 2. 反模式判定

**通过。** 当前 UI 不像通用 AI 生成的 SaaS 模板：主界面坚持密集列表而非同构卡片网格，字体和控件以桌面工具熟悉度为先，强调色用于状态和操作而非铺满页面，也没有渐变文字、超大 hero 或无意义页面入场动画。标题栏细线渐变、浮动状态窗的 blur/glow 均局限在允许的层级，没有扩散到高密度内容区域。

需要继续守住的边界：不要把设置分区继续堆成卡片，不要用装饰性动效替代状态反馈，不要因为增加协议而让默认任务行承载所有高级信息。

### 3. 具体问题

#### UX-01（P1，已确认缺陷）：旧浏览器预览任务可导致整页崩溃

- **证据**：`src/lib/tauri-browser.ts:182-196,216-223` 只规范化错误和恢复字段，没有为旧任务补齐 `protocol`；`src/components/tasks/TaskRow.tsx:159-160` 无保护地调用 `protocol.toLowerCase()`。
- **影响**：用户或开发者只要保留旧版本 localStorage 任务，任务列表渲染即可抛错；错误边界接管后，整个主界面不可操作。
- **改进**：在统一 `normalizeTask`/持久化迁移层为缺失协议推导可靠默认值；展示组件仍应对未知协议容错。禁止只在 `TaskRow` 临时写空字符串而不修复数据迁移。
- **验收**：加入旧 schema localStorage fixture；加载后任务列表正常显示、协议标签合理、刷新后数据已迁移；缺失/未知协议均不触发 error boundary。

#### UX-02（P2，已确认缺陷）：错误边界使用不存在的翻译键

- **证据**：`src/components/shell/AppErrorBoundary.tsx:72` 使用 `errorBoundary.copy`，而 `src/i18n/locales/en.ts:1094` 和 `zh-CN.ts:1144` 定义的是 `errorBoundary.copyError`。
- **影响**：应用最需要清晰恢复信息时，按钮可能显示原始 key 或 fallback，不利于用户复制错误求助。
- **改进**：改用正确 key，并为 error boundary 增加 en/zh-CN 组件测试，覆盖复制、重载和回到主界面。
- **验收**：两种主动语言均显示正确按钮文本，复制内容包含错误消息且不会再次抛错。

#### UX-03（P2，回归风险）：25 个 Hook 依赖警告集中在关键交互

- **证据**：Biome 报告 25 个 `useExhaustiveDependencies` warning，涉及 `SettingsPage`、`AppShell`、`NewDownloadDialog`、`TaskDetails`、`TaskList` 和 `toast`；例如 `SettingsPage.tsx:566` 的自动保存 effect、`:988` 的初始化刷新 effect。
- **影响**：闭包过期或重复 effect 可能影响设置自动保存、资源探测、详情状态和 toast 计时；这些问题通常只在快速切换或异步竞态中出现。
- **改进**：逐条判断依赖、稳定 callback 和 effect 职责，不要机械补 dependency；把自动保存拆成可测试 hook/state machine，并用 fake timers 覆盖 debounce、卸载和并发刷新。
- **验收**：`useExhaustiveDependencies` 告警归零或每个例外都有局部注释与测试；设置快速连续修改只保存最终状态，切页/卸载不产生陈旧写入。

#### UX-04（P2，实测）：移动设置页信息密度过高

- **证据**：主任务视图在 `390px` 下无页面级横向溢出，新建下载和命令面板可用；但设置页仍使用横向滚动分区选择器，长表单在窄屏上认知负担明显。
- **影响**：小窗口或触屏设备上难以定位当前分区，保存状态和字段关系不易扫描，横向滑动也可能与页面手势冲突。
- **改进**：窄屏改为原生 select/菜单式分区导航或 sticky 单列目录；保持字段单列、40px 可点击目标和错误就近显示；不要牺牲桌面端密度。
- **验收**：`320/390/768/1280px` 截图与键盘测试无内容遮挡、无页面横向滚动；最长中英文标签不溢出；所有分区可在两次操作内到达。

#### UX-05（P2，已确认语义问题）：游标分页的 `total` 不是总数

- **证据**：`src-tauri/src/db/task_records.rs:337-345` 将 `total` 设为“本页已加载数量 + has_more 时的 1”。
- **影响**：任何把该字段展示为总任务数、选中范围或统计依据的 UI 都可能误导用户；其语义也与普通分页 API 的 `total` 不一致。
- **改进**：将字段重命名为 `loaded_count`/`minimum_total`，或在真正需要时单独查询准确 count；不要为每次无限滚动强制昂贵 count。
- **验收**：IPC 类型明确区分准确值与估算值；UI 不再把下界显示为总数；大库场景不会因 count 查询拖慢首屏。

#### UX-06（P2，系统性风险）：关键 a11y 规则被全局关闭

- **证据**：`biome.json:60-66` 全局关闭点击键盘配对、SVG title、label-control、语义元素、静态元素交互和 ARIA role 支持检查。
- **影响**：现有界面虽然整体可用，但后续新增组件可在无告警情况下引入键盘不可达、错误 role 或无标签控件。
- **改进**：逐项恢复规则；确需例外时在具体组件局部抑制并说明 Radix/自定义控件语义；增加 axe 或 Testing Library 组件级冒烟测试。
- **验收**：至少表单 label、静态元素交互、ARIA role 三类规则重新启用；主壳、新建下载、设置、删除确认和错误边界通过自动 a11y 检查。

### 4. UX 修复工作流建议

1. **P1 `$impeccable harden`**：先处理旧数据迁移、错误边界、异步 effect 和失败恢复状态。
2. **P2 `$impeccable adapt`**：专项验收设置页在 `320/390/768px` 的导航、触控目标和文本容纳。
3. **P2 `$impeccable optimize`**：围绕任务列表、图标缓存、搜索和 effect 建立可测性能基线。
4. **收尾 `$impeccable polish`**：在功能和回归测试完成后统一检查焦点、文案、溢出和明暗主题。

## 六、程序功能丰富性和完整性

### 1. 当前能力矩阵

| 能力 | 当前状态 | 明确边界 / 风险 | 审计结论 |
| --- | --- | --- | --- |
| HTTP/HTTPS | HEAD + Range fallback、单流/未知大小、分段、动态拆分、续传、重试、校验、代理、限速 | 仍需持续做崩溃恢复和真实站点兼容测试 | 当前最成熟主路径 |
| FTP/FTPS | 动态并行、凭据、目录探测、SOCKS5 | implicit FTPS + SOCKS5 明确不支持；诊断/恢复测试弱于 HTTP | 可用但不可宣称同等成熟 |
| SFTP | 单文件、密码/密钥相关能力、TOFU、目录探测、SOCKS5、本地临时文件续传 | 大文件仍以较保守读路径为主；跨服务端兼容矩阵不足 | 中等成熟度 |
| BitTorrent | magnet、torrent URL/本地文件、多文件选择、peer/tracker/DHT 快照、做种、SOCKS5、限速 | 实时 tracker/NAT/端口可达性诊断仍有限 | 功能丰富，运维可见性不足 |
| HLS | master variant、AES-128-CBC、init map、byte range、并发分片、live polling、多音轨/字幕、MP4 remux | 不支持 SAMPLE-AES/DRM；ffmpeg 设置和内存峰值有已确认问题 | 需要先修可靠性再扩格式 |
| DASH | 静态 MPD、ffmpeg 下载/remux、进度监控 | 拒绝 dynamic/live；缺 `SegmentTimeline` | 第一阶段 VOD 能力 |
| WebDAV/WebDAVS | Basic Auth、PROPFIND、委托 HTTP 引擎 | 仅 Basic Auth；无 Digest/OAuth/企业认证矩阵 | 基础能力，不是完整 WebDAV 客户端 |
| Metalink4 | 多文件、镜像优先级/failover、校验、分文件进度 | 非 HTTP 镜像和长期恢复诊断仍有限 | 中等成熟度 |
| 浏览器交接 | Native Messaging、WebSocket bridge、去重、单实例、下载接管、显式 header/cookie 转发 | 安全边界刻意限制为 HTTP/HTTPS；安装包 host 交付未闭环 | 代码能力强，但发布交付被 P0 阻断 |
| 本地自动化 | 剪贴板监控、命令面板、批量动作 | 无稳定 JSON-RPC/REST API | 适合交互使用，自动化生态未形成 |

### 2. 具体问题

#### FUN-01（P1，成熟度缺口）：非 HTTP 协议缺少统一可靠性与诊断验收矩阵

- **影响**：入口已经存在会提高用户预期；如果暂停/恢复、凭据过期、代理失败、磁盘失败和校验失败不能给出同等级恢复动作，功能数量反而削弱信任。
- **改进**：为 FTP/FTPS、SFTP、BT、HLS、DASH、WebDAV、Metalink 建立相同的 capability contract：创建、探测、暂停、恢复、取消、重试、代理、凭据、校验、进程重启和错误诊断。
- **验收**：每个协议都有支持矩阵和自动化证据；不支持项在创建前明确提示；失败任务都能输出稳定 error code、用户说明和至少一个可执行恢复动作。

#### FUN-02（P1，已确认缺陷）：设置中的 ffmpeg 路径对 HLS 不生效

- **证据**：`src-tauri/src/download/ffmpeg.rs:22-58` 已提供 env → SQLite setting → PATH 的统一异步解析；DASH 使用该路径。HLS 却在 `hls.rs:1883-1900` 保留独立同步实现，只检查 `VIBE_FFMPEG_PATH` 和 PATH。
- **影响**：用户在 Settings 中成功选择 ffmpeg 后，DASH 可工作而 HLS 仍报告缺失；同一设置对两个依赖相同工具的功能表现不一致。
- **改进**：删除 HLS 私有 resolver，统一调用 `download::ffmpeg`；probe 和 download 都传入 pool，并统一错误码与检测文案。
- **验收**：在 PATH 和环境变量均无 ffmpeg 时，仅设置 SQLite `ffmpeg_path`，HLS probe、下载和 remux 均成功；无效路径返回可恢复错误并可跳转到设置。

#### FUN-03（P2，能力边界）：协议格式支持仍有明确缺口

- DASH 暂不支持 dynamic/live MPD 和 `SegmentTimeline`。
- HLS 暂不支持 SAMPLE-AES、DRM 或受保护媒体；不得用“AES 支持”模糊覆盖 DRM。
- WebDAV 仅支持 Basic Auth，不代表完整企业 WebDAV 兼容性。
- implicit FTPS 经过 SOCKS5 代理仍不支持，应继续显式拒绝而不是绕过代理。
- 浏览器 handoff 仅接收 HTTP/HTTPS 是安全设计边界，不应为了“协议齐全”放宽嵌入凭据、本地路径或任意 scheme。
- Safari wrapper、签名和商店审查尚未实现。

验收方式不是一次性全部实现，而是让 New Download、帮助文档、README 和错误提示对这些边界保持一致，并用负向测试保证“不支持时明确拒绝”。

#### FUN-04（P2，测试完整性）：前端测试规模与界面复杂度不匹配

- **现状**：Vitest 已有 16 文件/47 测试并全部通过，但复杂界面包含 2407 行 Settings、2038 行 TaskDetails、虚拟列表、命令面板、多个 dialog 和大量异步事件。
- **缺口**：设置自动保存、新建下载探测、任务详情切换、旧数据迁移、错误边界、批量删除、键盘选择和移动 drawer 等关键组合行为覆盖不足。
- **改进**：优先增加组件/集成测试而非继续堆纯 helper 测试；对 Tauri adapter 使用稳定 mock contract。
- **验收**：上述高风险流程均有成功、失败、取消/卸载和中英文测试；真实用户动作不依赖实现细节断言。

#### FUN-05（P3，未来广度）：中长期能力应排在可靠性之后

可规划但不应进入当前发布阻断路径的功能包括：本地 JSON-RPC/REST API、PAC、路径模板/完成后整理、完整站点规则管理（冲突预览、导入导出和命中诊断）、云盘解析、视频嗅探、云账号/同步和插件协议框架。

优先级原则：只有当 P0/P1 清零、HTTP 主路径持续全绿、非 HTTP 协议有统一验收矩阵后，才扩大协议和云功能范围。

## 七、项目架构的鲁棒性和稳定性

### 1. 已有稳健设计

- `EngineRegistry` 将协议路由和下载行为隔离，避免所有协议继续堆入单一 command。
- 状态机使用条件更新和事务，调度、暂停、失败等竞争路径已有真实并发回归测试补强。
- `TaskRuntimeLocks` 将同任务操作串行化，并在删除和 worker 完成后回收。
- SSRF 防护覆盖 URL 字面量、DNS 解析结果、IPv4/IPv6 特殊网段和重定向逐跳校验。
- 凭据、代理、校验、SFTP known host 和任务文件等均有独立 DB 模块，边界基本清晰。
- shutdown、WAL checkpoint、诊断清理和事件 lag 恢复已经有专门处理。

### 2. 具体问题

#### RLS-01（P0，发布阻断）：Native Messaging host 未进入 Tauri bundle 闭环

- **证据**：`src-tauri/src/commands/browser.rs:1128-1135` 假设 `vibe-native-host(.exe)` 与主程序同目录；`src-tauri/tauri.conf.json:30-43` 只打包 extension-core 资源和图标，没有 `externalBin`、sidecar 或等价 host 二进制规则。
- **影响**：开发环境中 host binary 存在，不代表安装后存在。正式安装包可能成功安装 manifest，却让浏览器指向不存在的 executable，第一方核心工作流直接失效。
- **改进**：确定 Tauri 2 sidecar/external binary 方案、目标三元组命名、安装路径和升级/卸载行为；manifest 必须使用安装后的真实绝对路径。
- **验收**：对 Windows x64、macOS arm64/x64、Linux x64 的正式 bundle 安装后检查 host 文件、执行权限和 manifest；Chrome/Edge/Firefox 至少完成一次浏览器发起 → host → app → task 创建的端到端冒烟测试；卸载后无失效残留。

#### RLS-02（P0，发布阻断）：公开发布信任链未闭环

- **缺口**：Chrome/Edge/Firefox 正式 store ID、生产扩展签名、Firefox signed XPI、权限说明/review copy、Safari 取舍、OS 代码签名、真实 updater 升级演练尚未全部完成。
- **影响**：用户会遭遇来源警告、扩展身份变化、Native Messaging allowlist 不匹配或升级失败；即使二进制可构建，也不构成可信发布。
- **改进**：建立 release candidate 清单，固定 app/extension identity；签名与权限文档进入 release job；若短期明确 unsigned，下载页和 release notes 必须如实说明风险。
- **验收**：从上一稳定版本安装，经 `latest.json` 检测、下载、校验、重启到新版本；浏览器扩展升级后 ID 不变且 host 继续可用；三平台安装/卸载证据归档。

#### ARC-01（P1，已确认数据风险）：迁移异常会在用户同意前重建数据库

- **证据**：`src-tauri/src/db/connection.rs:26-50` 遇到特定 migration error 后关闭 pool、备份、删除 DB 文件并重建；`src-tauri/src/lib.rs:567-590` 在应用完成初始化后才告知用户任务和设置已清空。
- **附加风险**：`connection.rs:147-163` 顺序复制主 DB 和 WAL，忽略 WAL copy 失败，也没有使用 SQLite online backup/VACUUM INTO；备份不保证是同一一致性时间点。
- **影响**：一次可恢复的迁移历史问题会自动变成数据丢失事件；通知不是授权。备份若不一致，用户可能无法恢复原任务和设置。
- **改进**：失败时默认 fail closed；先显示“退出/打开备份位置/安全重建”选择。使用 SQLite backup API 或 `VACUUM INTO` 生成一致备份，并验证可打开、`integrity_check` 通过后才允许重建。
- **验收**：Dirty、VersionMissing、降级和损坏快照均有测试；未确认时原库字节不变；备份可迁移、任务/设置数量可核对；重建操作有明确日志和恢复说明。

#### ARC-02（P1，工程稳定性）：质量门禁当前不全绿

- **证据**：见“质量门禁实测”。lint 有 4 error/25 warning；bindings 有漂移；Windows `dash_engine` 测试 binary 在测试前装载失败。
- **影响**：格式/依赖问题本身不是严重运行故障，但持续红门禁会掩盖真正回归；测试进程无法启动意味着该平台的相关能力实际未验证。
- **改进**：先修当前错误，再把 typecheck、lint、active i18n、bindings、frontend test、Rust check/clippy/test 纳入必过 CI；Windows loader 错误需定位 DLL/feature/toolchain 依赖，不能仅在 job 中忽略。
- **验收**：干净 checkout 上所有门禁连续两次通过；生成 bindings 后 `git diff --exit-code` 为零；Windows `dash_engine` 至少真正开始并完成测试枚举。

#### ARC-03（P2，可维护性）：超大模块扩大变更影响面

当前行数：`SettingsPage.tsx` 2407、`TaskDetails.tsx` 2038、`hls.rs` 2083、`metalink.rs` 1829、`dash.rs` 1818。

- **影响**：状态、I/O、解析、渲染和错误处理混在同一文件时，review 难以判断局部修改的真实影响；也会诱发 Hook 依赖遗漏和协议逻辑重复。
- **改进**：按已存在的职责边界拆分，不追求机械小文件。Settings 优先拆 section model、autosave hook 和 browser integration；TaskDetails 按 protocol panel/diagnostics 分区；引擎按 parser、plan、transfer、remux、error 拆分。
- **验收**：拆分后公共入口和行为不变；新增单元测试落在纯模块；核心文件不再同时承担解析、持久化、网络执行和 UI/命令编排四类职责。

#### ARC-04（P2，一致性）：下载错误类型化只完成了边界包装

- **证据**：`src-tauri/src/download/error.rs:8-23` 明确说明引擎内部仍大量生产 `String`，只在 trait 边界包装为 `DownloadError::Other`；静态搜索在 download 目录仍发现约 136 处 `Result<..., String>` 签名。
- **影响**：恢复动作仍可能依赖字符串/JSON 再解析，协议间错误码不稳定，重构文案也可能改变行为。
- **改进**：按网络、认证、代理、远端变更、磁盘、工具缺失、格式不支持和取消等稳定类别逐步迁移；DB 错误可保留内部 source，但 command 层输出结构化 payload。
- **验收**：调度和恢复逻辑只匹配枚举/code，不匹配人类文案；跨协议同类错误共享 code；未知错误仍保留 source chain 和可复制诊断。

#### ARC-05（P2，架构重复）：ffmpeg 单一事实源未真正统一

该问题的用户表现见 FUN-02，架构根因是 `download/ffmpeg.rs` 与 `hls.rs` 同时维护 resolver、可用性检测和文案。修复时应删除重复实现，而不是让两份逻辑再次同步。

## 八、程序运行效率

### 1. 已有高价值优化

- 虚拟化任务列表和游标分页避免一次渲染/传输全部历史任务。
- task data、UI selection/query 和 speed history 分 store，降低无关状态更新范围。
- `TaskProgressEmitGate` 将高频事件限制到至少 `250ms`，前端也对事件做批量/增量处理。
- task files/checksums 使用批量加载，`files_version` 用于避免每次状态更新都重查文件列表。
- SQLite WAL、checkpoint 和 request diagnostics 清理限制长期运行增长。
- HTTP client 复用、连接槽位和分段 planner 已考虑网络并发与主机公平性。

### 2. 具体问题

#### PERF-01（P1，已确认并发正确性）：全局 token bucket refill 可覆盖并发扣减

- **证据**：`src-tauri/src/download/speed.rs:105-120` 用 CAS 争抢 refill 所有权，但随后通过普通 atomic `store(new_tokens)` 写回；`:123-140` 的另一个线程可能在 load 与 store 之间成功 CAS 扣减 token。
- **影响**：refill 线程会把并发扣减覆盖掉，短时间额外发放 token；高并发下载时全局/逐任务限速可能超出配置，且问题难以用普通单线程测试复现。
- **改进**：使用 `fetch_update`/CAS loop 在同一原子变量上完成“读取当前 token + refill + clamp”，或用小粒度 mutex 合并时间和 token 状态；明确内存序理由。
- **验收**：加入多线程确定性测试和长时间统计测试；多个 consumer 下总吞吐在允许 burst 后收敛到配置值，TSAN/loom 可行时补模型测试。

#### PERF-02（P1，已确认内存风险）：加密 HLS 分片存在接近 1 GiB/worker 的瞬时峰值

- **证据**：`hls.rs:48-55` 允许单分片最大 512 MiB；`:903-949` 先收集完整 ciphertext；`:1095-1101` 在原 buffer 上解密后又 `.to_vec()` 生成 decrypted buffer。
- **影响**：单 worker 最坏同时持有约 512 MiB ciphertext backing buffer 和 512 MiB plaintext；多个并发分片可快速触发交换、OOM 或整机卡顿。512 MiB cap 只把“无界”变成“过高的有界”。
- **改进**：实现 AES-128-CBC block streaming decrypt，尾部保留一个 block 处理 PKCS#7；写入临时文件并原子完成。同步设置合理的单 segment 上限和全任务内存预算。
- **验收**：使用大加密 segment fixture 监控峰值 RSS；并发 worker 增加时内存按固定小 buffer 线性增长，不按完整 segment 大小增长；取消/解密失败不留下可误用成品。

#### PERF-03（P2，已确认增长）：系统文件图标缓存按完整文件名且无上限

- **证据**：`src/hooks/use-system-file-icon.ts:4-20` 明知 OS 图标只依赖扩展名，仍以完整 `fileName` 为 key；`iconCache` 和 `inflight` 没有容量或淘汰策略。
- **影响**：大量不同文件名但相同扩展会重复 IPC、重复 base64 PNG，并在长会话中永久占用前端内存。
- **改进**：按规范化扩展/MIME key；使用小容量 LRU（例如 128/256）并对无扩展和特殊协议设置稳定 key；后端也应按扩展缓存原始图标结果。
- **验收**：1 万个不同名称、100 个扩展只触发约 100 次提取；缓存容量有上界；滚动和主题切换不造成图标闪烁或重复请求风暴。

#### PERF-04（P2，已确认增长）：后端 `files_version` 全局缓存无淘汰

- **证据**：`src-tauri/src/events/mod.rs:155-164` 使用静态 `HashMap<String, i64>`；更新时插入，但删除任务路径没有对应 evict。
- **影响**：长时间创建/删除任务后，已删除 task id 永久驻留。单条成本不高，但这是明确的生命周期泄漏。
- **改进**：在任务删除事件中移除；或将缓存放入 AppState，由任务生命周期统一管理；必要时加容量保护。
- **验收**：批量创建/删除后缓存回到接近存量任务数；并发 emit/delete 不死锁、不复活旧项。

#### PERF-05（P2，待基准验证）：历史任务搜索无法利用普通索引

- **证据**：`src-tauri/src/db/task_records.rs:372-383` 对三个字段执行 `LOWER(column) LIKE '%term%'`。
- **影响**：前导通配符通常导致全表扫描；在 10k–50k 历史任务、频繁输入和多筛选组合下可能拖慢 UI 与 DB pool。
- **改进**：先用真实数据跑 `EXPLAIN QUERY PLAN` 和 p50/p95；若超预算，引入 FTS5/规范化 search column，或限制搜索字段和触发时机。
- **验收**：50k 数据下连续输入不阻塞主界面；目标建议为本地常规 SSD 上 p95 < 100ms，并记录硬件、数据分布和 query plan。

#### PERF-06（P2，已确认阻塞路径）：批量文件删除在 async command 中同步串行执行

- **证据**：`src-tauri/src/commands/tasks/actions.rs:474-503,543-597` 在 async command 内循环调用同步 `delete_path`/trash 操作，并对任务逐个串行处理。
- **影响**：回收站 API、网络盘或大量小文件可能长时间占用 Tokio worker；批量删除期间响应变慢，且无法显示可取消进度。
- **改进**：把阻塞文件系统操作放入 `spawn_blocking`；设有界并发；数据库删除与文件删除结果分别建模，向 UI 报告部分成功和剩余项。
- **验收**：删除数千文件时主事件循环保持响应；可报告进度/警告；取消或个别失败不会重复删除已完成项。

#### PERF-07（P2，证据缺口）：缺少可重复的生产规模性能基线

至少建立以下场景：

| 场景 | 指标 |
| --- | --- |
| 1k/10k/50k 历史任务冷启动 | DB 查询、首屏时间、IPC payload、前端 heap |
| 100 活跃任务混合协议 | CPU、RSS、DB write rate、事件率、UI FPS |
| 10k 搜索/筛选/排序 | p50/p95、query plan、取消旧请求能力 |
| 长时间 HLS/BT/诊断运行 | 内存增长、WAL 大小、诊断表大小、句柄/任务数 |
| 1k 文件批量删除 | 总耗时、UI 响应、部分失败恢复 |

基准结果应写入独立 `docs/performance-baseline.md`，记录硬件、OS、构建模式、数据生成方式和前后对比；没有这些元数据的单次数字不能作为回归门禁。

## 九、统一优先级与验收清单

### P0：公开发布前必须完成

| ID | 事项 | 完成标准 |
| --- | --- | --- |
| RLS-01 | Native Messaging host 随安装包交付 | 三平台 bundle 内存在正确 host；manifest 路径、执行权限、升级和卸载正确；真实浏览器交接通过 |
| RLS-02 | 发布信任链 | 固定商店 ID/签名/权限文案；完成 updater E2E；配置 OS signing 或明确、可见地接受 unsigned 风险 |

### P1：发布前应完成

| ID | 事项 | 完成标准 |
| --- | --- | --- |
| UX-01 | 旧预览任务迁移和渲染容错 | 旧 localStorage fixture 不崩溃并持久化新 schema |
| FUN-01 | 非 HTTP 协议可靠性矩阵 | 每协议关键生命周期、失败和恢复路径有自动化证据与明确边界 |
| FUN-02 | HLS 统一 ffmpeg 配置 | Settings 路径对 HLS probe/download/remux 生效，重复 resolver 删除 |
| ARC-01 | 安全数据库恢复 | 未经确认不删除原库；备份一致、可校验、可恢复；迁移快照覆盖 |
| ARC-02 | 质量门禁恢复 | lint/bindings/typecheck/test/build/clippy 全绿；Windows dash tests 真正运行 |
| PERF-01 | token bucket 原子性 | 并发测试证明扣减不会被 refill 覆盖，长期吞吐收敛 |
| PERF-02 | HLS 流式解密和内存预算 | 大分片/多 worker RSS 在预算内，失败和取消清理正确 |

### P2：近期工程迭代

| ID | 事项 |
| --- | --- |
| UX-02 | 修复 error boundary 翻译键并加恢复测试 |
| UX-03 | 清理 25 个 Hook dependency 警告，重点重构 settings autosave |
| UX-04 | 重做窄屏设置分区导航和触控/文本验收 |
| UX-05 | 明确 cursor `total` 的准确度语义 |
| UX-06 | 恢复关键 a11y 静态规则并增加组件检查 |
| FUN-03 | 在 UI/文档/负向测试中明确协议能力边界 |
| FUN-04 | 扩展前端组件和集成测试覆盖 |
| ARC-03 | 按职责拆分五个超大模块 |
| ARC-04 | 从边界包装继续推进结构化错误 |
| ARC-05 | 消除 ffmpeg 配置重复事实源 |
| PERF-03 | 图标缓存按扩展归一化并设容量 |
| PERF-04 | `files_version` 缓存随任务删除淘汰 |
| PERF-05 | 为 50k 搜索建立 query plan/FTS 方案 |
| PERF-06 | 将批量文件删除移出 async worker 的同步串行路径 |
| PERF-07 | 建立 1k/10k/50k 与 100 活跃任务生产规模基线 |

### P3：可靠性稳定后再规划

- FUN-05：JSON-RPC/REST、PAC、路径模板、完整站点规则、云盘/视频嗅探、云账号同步、插件协议框架。

## 十、建议实施顺序

### 阶段 0：恢复可信基线

1. 固化当前工作区，审查并纳入生成 bindings。
2. 清除 lint error；逐项处理而非隐藏 25 个 Hook warning。
3. 定位 Windows `0xc0000139`，确保 Rust 集成测试可执行。
4. 将所有必过命令放入 CI，并在干净 checkout 复验。

### 阶段 1：修正会破坏信任的行为

1. 修复数据库恢复授权和一致备份。
2. 修复旧任务崩溃、HLS ffmpeg 设置、token bucket 竞态和 HLS 内存峰值。
3. 为每项缺陷先加入可复现测试，再修改实现。

### 阶段 2：闭环安装与发布

1. 完成 host sidecar/bundle、manifest 安装和卸载。
2. 完成扩展身份、签名、权限审查和 Safari 范围决策。
3. 完成三平台安装包、updater、升级后浏览器交接和 unsigned/signed 策略验收。

### 阶段 3：协议可靠性对齐

1. 建立统一 capability/error/recovery contract。
2. 先补 FTP/SFTP/BT/HLS/DASH/WebDAV/Metalink 的失败矩阵和长测。
3. 只有矩阵稳定后再扩 DASH live、SAMPLE-AES 等格式能力。

### 阶段 4：规模与可维护性

1. 建立 1k/10k/50k 和 100 活跃任务基线。
2. 根据数据实施 FTS、缓存上限、删除并发和模块拆分。
3. 最后再排自动化 API、PAC、云和插件生态。

## 十一、发布验收定义

只有同时满足以下条件，才能把版本标记为公开发布候选：

- P0 全部关闭，P1 无未接受风险；任何延期都有 owner、原因和用户可见限制说明。
- `pnpm check`、`pnpm test:frontend`、`pnpm build`、`pnpm check:bindings`、`cargo check`、`cargo clippy -D warnings`、Rust 全测试在目标 CI 环境通过。
- 三平台正式安装包能安装、启动、创建 HTTP 下载、暂停/恢复、完成校验、升级和卸载。
- Chrome/Edge/Firefox 中至少覆盖计划支持的正式浏览器，安装后 Native Messaging 端到端成功。
- 数据库从上一公开版本迁移成功；失败场景不会未经确认删除数据，备份可恢复。
- HTTP 主路径完成断网、远端变化、磁盘不足、进程崩溃和重启恢复测试。
- 非 HTTP 协议的 UI 和文档只承诺已通过矩阵验证的能力。
- release notes 如实说明 Safari、DRM、DASH live、WebDAV auth、OS signing 等边界。

## 十二、建议验证命令

常规质量门禁：

```bash
pnpm typecheck
pnpm lint
pnpm check:i18n
pnpm test:frontend
pnpm build
pnpm check:bindings
cargo check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
pnpm test:rust
```

浏览器集成变更：

```bash
pnpm build:extensions
```

发布候选还必须执行真实 bundle，而不是只跑 compile check：

```bash
pnpm tauri build --config src-tauri/tauri.ci.conf.json
```

构建后应使用安装包完成本文“发布验收定义”中的安装、Native Messaging、updater、升级和卸载步骤。

## 十三、相关专项文档

- [architecture-audit.md](architecture-audit.md)：早期四维逐项代码复核及历史修复记录。
- [rust-backend-audit.md](rust-backend-audit.md)：Rust 后端可扩展性、效率与安全专项。
- [engineering-quality-audit.md](engineering-quality-audit.md)：测试、CI、迁移、依赖、文档和脚本专项快照。
- [cross-platform-audit.md](cross-platform-audit.md)：Windows/macOS/Linux 运行、打包与系统集成专项。
- [dependency-modernization-audit.md](dependency-modernization-audit.md)：依赖选型与现代化建议。

这些文档保留历史证据价值，但不应把其中“当时未实现”或“当时已修复”的文字直接当作当前事实；关闭本文事项时必须回到当前代码、自动化测试和真实安装包重新验证。
