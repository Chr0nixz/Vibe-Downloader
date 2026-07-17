# 项目改进审计

最后更新：2026-07-13
适用版本：Vibe Downloader `0.2.0`
审计对象：当前工作区代码、配置、测试、构建脚本与产品文档（包含尚未提交的改动）

本文是项目当前唯一的**全局风险与优先级基线**。它从用户交互便捷性、程序功能丰富性和完整性、项目架构鲁棒性和稳定性、程序运行效率四个维度回答三件事：目前已经做到什么、仍有哪些可验证缺口、应该按什么顺序整改。

本文不是变更日志，也不把路线图中的计划描述为已实现能力。方向性规划以 [ROADMAP.md](ROADMAP.md) 为准；产品与设计约束以 [PRODUCT.md](../PRODUCT.md) 和 [DESIGN.md](../DESIGN.md) 为准；专项审计是特定日期的证据补充，当旧结论与本文冲突时，以本文和当前实现为准。

## 一、执行摘要

### 1. 总体判断

Vibe Downloader 已经越过“HTTP 下载 MVP”阶段，具备真实桌面下载管理器所需的大部分骨架：HTTP/HTTPS 主路径较成熟，多协议入口、SQLite 持久化、队列调度、限速、恢复动作、浏览器交接、任务诊断、虚拟列表、响应式桌面 UI 和中英文界面均已落地。

当前主要矛盾不是功能数量不足，而是**工程闭环与外部发布验收之间仍有距离**。本轮已实现 Native Messaging sidecar、候选/正式扩展身份、发布 preflight、安全数据库恢复、HLS 统一 ffmpeg、流式 AES 解密、限速并发修复和旧预览数据迁移；但四平台真实安装包、浏览器商店正式身份与签名、updater 跨版本演练仍需外部环境完成。非 HTTP 协议虽然已有统一矩阵，可靠性和诊断证据仍明显弱于 HTTP。

因此，当前版本适合作为积极开发中的 `0.2.0`，但还不应被描述为“可公开稳定发布、全协议同等成熟、可替代 IDM 的正式版本”。

### 2. 四维结论

| 维度 | 当前成熟度 | 主要优势 | 首要缺口 |
| --- | --- | --- | --- |
| 用户交互便捷性 | 良好，已关闭主要崩溃边界 | 主任务流清晰；旧预览任务会迁移协议；数据库恢复有显式安全界面；设置自动保存及失败竞态、错误边界、新建下载探测和任务详情/移动抽屉已有组件回归测试；游标数量下界不再伪装成精确总数；六类 a11y 静态规则均已恢复 | 窄屏设置页仍需多尺寸视觉验收；主壳贯通流程与屏幕阅读器端到端测试仍需改进 |
| 功能丰富性与完整性 | 功能面宽，成熟度不均 | HTTP、队列、限速、代理、凭据、多协议、浏览器交接和诊断能力丰富 | 非 HTTP 协议可靠性仍未与 HTTP 对齐；多项协议边界必须明确；发布链路尚未形成可交付功能 |
| 架构鲁棒性与稳定性 | 核心防护较强，数据恢复已改为 fail closed | 事务化状态机、运行时锁、SSRF 防护、加密凭据、关闭收敛、一致备份和显式恢复模型已建立 | 真实 bundle/updater 尚未跑完；错误类型化和超大模块拆分未完成 |
| 程序运行效率 | 两项 P1 与两项缓存 P2 已修，仍缺生产规模证据 | 游标分页、虚拟列表、事件节流、有界图标缓存、原子 token refill、HLS 小缓冲流式解密均已存在 | 大库搜索和批量删除反馈缺规模验证；尚无生产规模 RSS/吞吐基线 |

### 3. 当前优先级数量

| 优先级 | 数量 | 含义 |
| --- | ---: | --- |
| P0 | 2 | 工程实现已完成，仍由真实安装验收、正式商店凭据和 updater 演练阻断关闭 |
| P1 | 7 | UX-01、FUN-02、ARC-01、PERF-01、PERF-02 已修；ARC-02 本地闭环待 CI；FUN-01 已大部分完成 |
| P2 | 15 | UX-02、UX-03、UX-05、UX-06、ARC-05、PERF-03、PERF-04 已修；UX-04 与 PERF-06 部分闭环，其余进入近期迭代 |
| P3 | 1 组 | 产品广度增强，不应挤占可靠性和发布闭环 |

最高优先事项：

1. 在四目标 candidate 安装包中验证 `vibe-native-host`，并完成安装后浏览器交接与卸载残留冒烟测试。
2. 取得正式扩展商店身份/签名，完成 `rc.0 → rc.1` updater 演练；OS 包保持明确的 unsigned 策略。
3. 在干净 checkout 和目标 CI 关闭 bindings、Windows Rust integration tests 与 bundle 门禁。
4. 按已建立的协议矩阵补齐非 HTTP 协议恢复、代理、凭据、校验和故障诊断自动化。
5. 建立生产规模性能基线，再处理缓存、搜索、批量删除和模块拆分等 P2。

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
- 七种受支持语言均由完整性脚本校验；`zh-TW`、`ja`、`ko`、`ru`、`es` 仍按产品定义标记为 Beta，且不会被自动检测选中。

## 三、质量门禁实测

| 检查 | 结果 | 说明 |
| --- | --- | --- |
| `pnpm check` | 通过 | TypeScript、Biome 与七语言完整性检查均通过；147 个前端/脚本文件无 lint error/warning；六类 a11y 规则全部恢复为 error |
| `pnpm test:frontend` | 通过 | 17 个测试文件、60 个测试通过；包含错误边界、Settings autosave 及失败竞态、axe 无障碍扫描、删除确认、新建下载探测和任务详情/移动抽屉组合测试 |
| `pnpm build` | 通过 | TypeScript 与 Vite 生产构建通过 |
| `pnpm test:release-tools` | 通过 | 22 项通过，包含 sidecar、身份/权限、preflight、确定性扩展 ZIP、资产和 updater rehearsal |
| `pnpm verify:extensions` | 通过 | dev 四变体与 candidate 三变体均通过；capture 关闭、manifest 最小权限与 allowlist 校验通过，candidate ZIP 确定性由 release-tools 覆盖 |
| `pnpm verify:protocol-matrix` | 通过 | 七类非 HTTP 协议、统一列、状态值与仓库测试证据完整 |
| `cargo check --manifest-path src-tauri/Cargo.toml` | 通过 | Rust 编译检查通过 |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | 通过 | Clippy 零 warning |
| Rust 全测试 | 通过（串行复验） | `cargo test --jobs 1` 完整通过：177 library tests、7 native-host tests 及全部 integration/doc tests；FTP/WebDAV 传输中恢复、HLS/DASH staging 恢复和 55 个协议引擎测试均正常执行 |
| Specta bindings | 已生成，待干净树门禁 | `StartupStatus` 与三个 startup command 已生成且 idempotence 校验通过；工作区本身有预期未提交差异，不能用当前 `git diff --exit-code` 代表干净 checkout 结果 |

结论：本轮静态检查、前端、发布工具、扩展、协议矩阵、Rust check/clippy 和全量 Rust 测试均已通过。Windows 默认并行构建曾超过本机页文件，串行复验已证明测试本身可运行；CI 仍需验证默认 runner 资源和四目标 bundle。当前剩余阻断主要是安装包/真实浏览器/updater/商店凭据，而非已知代码门禁失败。

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
| 可访问性 | 3/4 | 主路径有语义、ARIA、键盘和焦点态；六类 Biome a11y 规则均已恢复强制门禁，错误边界、设置和删除确认通过 axe；完整主壳/新建下载的屏幕阅读器端到端验收仍待补 |
| 前端性能 | 3/4 | 虚拟列表、store 拆分和事件批处理方向正确；缓存、effect 风险和规模基准仍需处理 |
| 响应式 | 3/4 | 主任务视图在窄屏可用；设置页在小于 `lg` 时已改为紧凑下拉分区导航，但仍需 `320/390/768/1280px` 多尺寸视觉验收 |
| 主题系统 | 4/4 | OKLCH token、明暗模式和 8 个强调色覆盖完整，未发现主要主题断裂 |
| 反模式控制 | 3/4 | 产品型密集工具风格成立，未落入卡片墙、渐变文字或营销页模板；少量装饰效果需持续克制 |
| **合计** | **16/20** | **良好：界面基础可信，需集中修复边界状态和自动化保障** |

### 2. 反模式判定

**通过。** 当前 UI 不像通用 AI 生成的 SaaS 模板：主界面坚持密集列表而非同构卡片网格，字体和控件以桌面工具熟悉度为先，强调色用于状态和操作而非铺满页面，也没有渐变文字、超大 hero 或无意义页面入场动画。标题栏细线渐变、浮动状态窗的 blur/glow 均局限在允许的层级，没有扩散到高密度内容区域。

需要继续守住的边界：不要把设置分区继续堆成卡片，不要用装饰性动效替代状态反馈，不要因为增加协议而让默认任务行承载所有高级信息。

### 3. 具体问题

#### UX-01（P1，已修复）：旧浏览器预览任务协议迁移

- **原缺陷**：旧 localStorage 任务可能缺失 `protocol`，而任务行曾直接调用 `protocol.toLowerCase()`，可触发整页错误边界。
- **实现**：`src/types/task.ts` 的统一规范化层会按 URL scheme 和文件扩展名推导协议，并为无法识别的旧记录使用 `unknown`；浏览器预览加载后会把迁移结果写回 localStorage。
- **证据**：`src/types/task.test.ts` 覆盖缺失协议、非 HTTP scheme、manifest 扩展名及未知输入；前端 typecheck、lint、测试和生产构建通过。

#### UX-02（P2，已修复）：错误边界翻译键

- **原缺陷**：组件使用不存在的 `errorBoundary.copy`，可能在恢复界面显示原始 key。
- **实现**：已改用 `errorBoundary.copyError`，为 i18n 未初始化场景保留英文 fallback，并由七语言完整性门禁保证 key 存在。
- **证据**：`AppErrorBoundary.test.tsx` 覆盖中文按钮与标题、复制诊断内容以及不重载应用的 reset 恢复路径；七语言完整性检查通过。

#### UX-03（P2，已修复）：关键交互 Hook 依赖与 Settings autosave

- **原风险**：25 个 `useExhaustiveDependencies` warning 集中在设置自动保存、资源探测、详情状态和 toast 计时等异步路径。
- **实现**：已逐项补齐真实依赖、删除伪依赖，并只在一次初始化、订阅所有权和 autosave 快照等有意保持边界的位置使用局部说明。
- **证据**：Biome 告警清零；`SettingsPage.test.tsx` 使用 fake timer 覆盖快速连续编辑只保存最新快照、`1000ms` 防抖和页面卸载取消未提交保存。

#### UX-04（P2，工程实现完成，视觉验收待补）：窄屏设置分区导航

- **实现**：设置页在小于 `lg` 时使用 sticky 的 Radix Select 分区导航，桌面端保留横向紧凑标签；搜索、分区导航和保存状态位于同一固定导航区域，字段在窄屏保持单列和 `44px` 主要输入高度。
- **证据**：`SettingsPage.test.tsx` 断言紧凑分区导航具有可访问名称，保存状态通过 `aria-live="polite"` 暴露；主任务视图既有 `390px` 预览无页面级横向溢出。
- **剩余**：仍需补齐 `320/390/768/1280px` 中英文截图与键盘/触控验收，确认最长标签、Select portal 和软键盘场景无内容遮挡。

#### UX-05（P2，已修复）：游标分页数量下界语义

- **原缺陷**：游标查询用“本页已加载数量 + `has_more` 时的 1”避免额外 COUNT，但 IPC 曾命名为 `totalEstimate`，UI 又把它显示为精确的剩余数和筛选总数。
- **实现**：IPC 字段改为带文档的 `minimumTotal`，明确它只是匹配任务数下界；无限滚动改为不带伪精确数字的“正在加载更多”，全选提示只说明仍有更多匹配任务，不再展示下界为总数。
- **证据**：Specta bindings 已同步；Rust cursor 测试固定 12 条匹配数据的第一页下界为 6，证明该值不是精确总数；前端和七语言文案均不再消费 `{{total}}`/`{{count}}` 作为游标总量。

#### UX-06（P2，已修复）：a11y 静态规则与组件检查

- **实现**：`noLabelWithoutControl`、`noStaticElementInteractions`、`useAriaPropsSupportedByRole`、`useKeyWithClickEvents`、`noSvgWithoutTitle`、`useSemanticElements` 均已从全局关闭改为 error。分段选择改用 `fieldset` 与 `aria-pressed`，步骤指示改为有序列表，操作组使用 `fieldset/legend`，容量摘要使用命名 `section`；虚拟列表、标题栏拖拽和装饰性 SVG 只保留带原因的局部抑制。
- **证据**：六条规则对整个 `src/` 强制执行且 `pnpm check` 通过；`jest-axe` 扫描错误边界、默认设置页、单项和批量删除确认均为零违规，删除测试同时覆盖长 CJK/emoji 文件名、取消、确认和名称截断。
- **后续保障**：完整主壳、新建下载和移动 drawer 的屏幕阅读器/键盘端到端验收继续归入 FUN-04，不再作为静态规则恢复的阻断。

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
| HLS | master variant、流式 AES-128-CBC、init map、byte range、并发分片、live polling、多音轨/字幕、MP4 remux | 不支持 SAMPLE-AES/DRM；仍需真实媒体 RSS 与 ffmpeg setting 冒烟 | 关键 P1 已修，继续补生产证据 |
| DASH | 静态 MPD、ffmpeg 下载/remux、进度监控 | 拒绝 dynamic/live；缺 `SegmentTimeline` | 第一阶段 VOD 能力 |
| WebDAV/WebDAVS | Basic Auth、PROPFIND、委托 HTTP 引擎 | 仅 Basic Auth；无 Digest/OAuth/企业认证矩阵 | 基础能力，不是完整 WebDAV 客户端 |
| Metalink4 | 多文件、镜像优先级/failover、校验、分文件进度 | 非 HTTP 镜像和长期恢复诊断仍有限 | 中等成熟度 |
| 浏览器交接 | Native Messaging sidecar、WebSocket bridge、去重、单实例、下载接管、显式 header/cookie 转发 | 安全边界刻意限制为 HTTP/HTTPS；等待四目标 bundle 与正式商店实测 | 工程闭环，外部发布验收阻断 |
| 本地自动化 | 剪贴板监控、命令面板、批量动作 | 无稳定 JSON-RPC/REST API | 适合交互使用，自动化生态未形成 |

### 2. 具体问题

#### FUN-01（P1，大部分完成）：已建立非 HTTP 协议统一可靠性矩阵

- **影响**：入口已经存在会提高用户预期；如果暂停/恢复、凭据过期、代理失败、磁盘失败和校验失败不能给出同等级恢复动作，功能数量反而削弱信任。
- **实现**：[protocol-reliability-matrix.md](protocol-reliability-matrix.md) 统一记录创建、探测、暂停、恢复、取消、重试、代理、凭据、校验、重启和诊断状态；`pnpm verify:protocol-matrix` 在 CI 中阻止协议行、状态或测试证据缺失。
- **新增证据**：FTP、SFTP、BT、HLS、DASH、WebDAV、Metalink 的取消状态、进程重启恢复和显式从头重启现在共享跨协议契约测试；取消与 Restart 两列已升级为 `automated`。暂停、恢复、取消现在把任务、文件、work unit、重试时间和事件放入同一事务，跨协议测试验证临时文件保留、最终文件不提前发布和进度不回退。FTP 与 WebDAV 又新增真实传输中暂停、持久化偏移和字节级恢复测试；HLS 与 DASH 新增 staging 中断、已完成分片复用、未完成分片重取和 ffmpeg remux 测试，因此四个协议的 Pause/Resume 已升级为 `automated`。这些测试同时修复了 FTP 缓冲区未落盘却提前持久化偏移、HTTP 小文件把 resume 错绑到 parallel、HLS 取消被误报失败及 remux 缺少输出参数、DASH upsert 清空完成分片及 ffmpeg 参数/输出格式错误。FTP、SFTP、WebDAV 的任务级 SHA-512/SHA-1/MD5 也会在完成后持久化校验结果；FTP 凭据拒绝和 FTP/SFTP SOCKS5 失败使用稳定结构化错误码，BitTorrent 已记录 source、metadata 和 stats 诊断并采用稳定 `bt_*` 错误码。
- **剩余**：Metalink 的完整任务级暂停/恢复，以及 BT、媒体协议和各引擎诊断仍有 `partial`；跨进程与真实外部服务兼容性也需继续积累。只有稳定 error code、恢复动作和临时文件一致性均有自动化证据后，本项才能完全关闭。

#### FUN-02（P1，已修复）：HLS 与 DASH 共享 ffmpeg 配置

- **原缺陷**：DASH 使用 env → SQLite setting → PATH 的统一解析，而 HLS 曾有只检查 env/PATH 的私有 resolver。
- **实现**：HLS 私有同步 resolver 已删除；probe、download 和 remux 统一使用 `download::ffmpeg::ensure_ffmpeg_available`，解析顺序与 DASH 相同。
- **剩余验收**：还需在无 PATH/env 的真实安装环境只配置 SQLite `ffmpeg_path`，完成一次 HLS remux 冒烟测试。

#### FUN-03（P2，能力边界）：协议格式支持仍有明确缺口

- DASH 暂不支持 dynamic/live MPD 和 `SegmentTimeline`。
- HLS 暂不支持 SAMPLE-AES、DRM 或受保护媒体；不得用“AES 支持”模糊覆盖 DRM。
- WebDAV 仅支持 Basic Auth，不代表完整企业 WebDAV 兼容性。
- implicit FTPS 经过 SOCKS5 代理仍不支持，应继续显式拒绝而不是绕过代理。
- 浏览器 handoff 仅接收 HTTP/HTTPS 是安全设计边界，不应为了“协议齐全”放宽嵌入凭据、本地路径或任意 scheme。
- Safari wrapper、签名和商店审查尚未实现。

验收方式不是一次性全部实现，而是让 New Download、帮助文档、README 和错误提示对这些边界保持一致，并用负向测试保证“不支持时明确拒绝”。

#### FUN-04（P2，持续改进）：前端测试规模与界面复杂度不匹配

- **现状**：本轮 Vitest 有 17 文件/60 测试通过；错误边界中英文恢复/复制、设置自动保存防抖/卸载取消/失败恢复/过期响应隔离、单项/批量删除确认和关键表面 axe 扫描已有回归证据，旧数据迁移、任务行键盘与恢复动作也有自动化覆盖。
- **新增证据**：`NewDownloadDialog.test.tsx` 覆盖 `650ms` 自动探测防抖、成功探测快照提交、结构化 timeout 恢复提示、URL 变化后的旧 Promise 隔离，以及 request ID phase 过滤和卸载注销；`TaskDetails.test.tsx` 使用真实用户事件覆盖详情 tab 按需加载、任务切换重置到 Overview，以及移动 drawer 的语义、初始焦点和关闭动作。
- **缺口**：主壳级搜索/筛选/选择/详情/删除贯通流程，以及完整主壳和新建下载的屏幕阅读器/键盘端到端验收仍不足。
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

#### RLS-01（P0，工程已闭环，安装验收待完成）：Native Messaging sidecar

- **实现**：bundle overlay 使用 `externalBin`；准备脚本只支持 Windows x64、macOS arm64/x64、Linux x64，按 Tauri target 后缀暂存并校验文件/执行权限。普通 Cargo check 不加载 bundle overlay，避免生成物阻断开发门禁。
- **防失效**：manifest 安装只接受已验证存在的绝对 host 路径，Windows registry 写入失败会回滚本轮 manifest；host 提供 `--self-check` 验证版本、协议和兄弟主程序。
- **自动化**：Tauri build 与 candidate workflow 都会解包代表性 MSI、`.app`、`.deb`、AppImage 并执行 self-check；Node 测试覆盖目标映射、命名、暂存验证和不支持目标拒绝。
- **剩余门禁**：必须等待四目标 CI bundle 真实通过，并在 Chrome、Edge、Firefox 完成安装、交接、去重、卸载残留验收后才能从 P0 完全关闭。

#### RLS-02（P0，工程闭环、外部凭据阻断）：公开发布信任链

- **实现**：dev/candidate/release 三种身份已统一到 Rust 与扩展构建；candidate 使用确定性 Chromium 测试公钥，release 缺任一正式 ID 会 fail closed。商店包仅生成 Chrome、Edge、Firefox，采用最小权限并关闭 capture；Opera 仅 dev，Safari 明确不支持。
- **发布工程**：preflight 校验 tag/版本/签名材料/正式 ID/权限/allowlist；扩展 ZIP 可重复构建并输出版本清单和 SHA-256；candidate 四目标 workflow、发布资产复验、隐私政策、商店资料和 tag-specific updater rehearsal 配置均已加入。
- **已接受边界**：OS 安装包仍 unsigned，release notes 必须显示 Gatekeeper、SmartScreen 和校验说明。
- **外部阻断**：仍需正式 Chrome/Edge/Firefox ID、商店账号、Firefox signed XPI，以及 `rc.0 → rc.1` 三平台 updater 实机证据。取得这些凭据后预期无需代码调整。

#### ARC-01（P1，已修复）：数据库迁移恢复改为 fail closed

- **实现**：迁移 Dirty、VersionMissing 和 VersionMismatch 不再删除原库；启动状态进入专用恢复界面，仅允许打开恢复目录、重试或在明确输入确认后重建。
- **备份保证**：使用参数化 `VACUUM INTO` 创建一致备份，再以只读连接执行 `PRAGMA integrity_check`；只有已验证备份存在时后端才接受重建命令。
- **证据**：migration integration tests 覆盖 Dirty、VersionMissing、原库保留、已验证备份和显式重建后的干净 schema，11 项全部通过。

#### ARC-02（P1，本地已闭环，等待 CI）：质量门禁

- **当前结果**：typecheck、Biome、七语言完整性、前端测试、生产 build、Cargo check/clippy、全量 Rust tests、bindings idempotence 均通过；CI 还新增协议矩阵与 release-tools 验证。
- **剩余门禁**：干净 checkout 的 `check:bindings` 和 Windows/Linux CI 必须复验。Windows 首次默认并行编译曾因本机页文件不足失败，但 `--jobs 1` 后 `dash_engine` 与全部测试正常运行；若 hosted runner 复现，应限制 Cargo jobs 或调整 runner 资源，不能忽略测试。

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

#### ARC-05（P2，已修复）：ffmpeg 单一事实源已统一

HLS 私有 resolver 和 PATH 检测已删除，HLS/DASH 统一通过 `download/ffmpeg.rs` 解析环境变量、SQLite 设置与 PATH。后续新增 ffmpeg 协议不得再次维护独立 resolver。

## 八、程序运行效率

### 1. 已有高价值优化

- 虚拟化任务列表和游标分页避免一次渲染/传输全部历史任务。
- task data、UI selection/query 和 speed history 分 store，降低无关状态更新范围。
- `TaskProgressEmitGate` 将高频事件限制到至少 `250ms`，前端也对事件做批量/增量处理。
- task files/checksums 使用批量加载，`files_version` 用于避免每次状态更新都重查文件列表。
- SQLite WAL、checkpoint 和 request diagnostics 清理限制长期运行增长。
- HTTP client 复用、连接槽位和分段 planner 已考虑网络并发与主机公平性。

### 2. 具体问题

#### PERF-01（P1，已修复）：token bucket refill 与扣减使用原子更新

- **原缺陷**：refill 曾在争抢更新时间后用普通 atomic `store` 写回，可覆盖另一个线程已成功的 token 扣减。
- **实现**：refill 改用同一 token 原子的 `fetch_update` 完成读取、补充和 clamp，不再用 `store` 覆盖并发扣减；代码说明了 Relaxed ordering 只保护数值原子性、不承担跨数据同步。
- **证据**：并发 refill/consume 回归测试与既有限速测试共 5 项通过。长时间多 consumer 吞吐基准仍归入 PERF-07。

#### PERF-02（P1，已修复）：加密 HLS 分片采用流式 AES-CBC 解密

- **原缺陷**：加密路径曾先收集最多 512 MiB ciphertext，再复制出 plaintext，单 worker 可接近 1 GiB 瞬时内存。
- **实现**：ciphertext 按网络 chunk 输入，decryptor 最多保留 16 字节尾块处理 PKCS#7，plaintext 写入 256 KiB `BufWriter` 临时文件，成功后发布；取消、网络、padding 或磁盘错误会清理临时文件。
- **证据**：任意 chunk 边界、非法 padding 和未对齐密文单元测试通过。还需在 PERF-07 的大加密媒体基准记录实际峰值 RSS。

#### PERF-03（P2，已修复）：系统文件图标缓存有界且按扩展复用

- **实现**：缓存与 in-flight 请求都使用小写扩展名 key，无扩展名使用稳定 sentinel；LRU 容量固定为 256，命中会刷新淘汰顺序，`null` 仍是有效缓存值。
- **证据**：前端单元测试覆盖 Windows/Unix 路径归一化、同扩展复用、无扩展名、LRU 淘汰和 `null` 命中。

#### PERF-04（P2，已修复）：`files_version` 缓存跟随任务生命周期

- **实现**：单任务删除、批量删除和 debug clear/scale seed 都会同步淘汰或清空缓存；锁中毒仍按既有策略安全降级。
- **证据**：Rust 单元测试固定单项/批量淘汰后仅保留存量任务条目。

#### PERF-05（P2，待基准验证）：历史任务搜索无法利用普通索引

- **证据**：`src-tauri/src/db/task_records.rs:372-383` 对三个字段执行 `LOWER(column) LIKE '%term%'`。
- **影响**：前导通配符通常导致全表扫描；在 10k–50k 历史任务、频繁输入和多筛选组合下可能拖慢 UI 与 DB pool。
- **改进**：先用真实数据跑 `EXPLAIN QUERY PLAN` 和 p50/p95；若超预算，引入 FTS5/规范化 search column，或限制搜索字段和触发时机。
- **验收**：50k 数据下连续输入不阻塞主界面；目标建议为本地常规 SSD 上 p95 < 100ms，并记录硬件、数据分布和 query plan。

#### PERF-06（P2，核心阻塞已修复，反馈待增强）：文件删除移出 async runtime

- **实现**：单个与批量删除统一收集、去重路径，通过最多 4 个 `spawn_blocking` worker 执行；回收站失败仍不会降级为永久删除，所有 warning 写入诊断日志。
- **证据**：Rust 异步测试覆盖重复路径不会重复处理，临时文件在 worker 完成后被删除。
- **剩余**：IPC 仍只返回删除数量，尚未把部分文件失败和可取消进度呈现给 UI；完成该反馈前本项不标记完全关闭。

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

| ID | 当前状态 | 完成标准 |
| --- | --- | --- |
| RLS-01 | 工程闭环，等待四目标 bundle/浏览器实测 | bundle 内存在正确 host；manifest 路径、执行权限、升级和卸载正确；真实浏览器交接通过 |
| RLS-02 | 工程闭环，外部凭据与 updater 实测阻断 | 固定正式商店 ID/签名；完成 updater E2E；保持明确、可见的 unsigned 风险说明 |

### P1：发布前应完成

| ID | 当前状态 | 剩余完成标准 |
| --- | --- | --- |
| UX-01 | 已修复 | 保持旧 schema fixture 回归测试 |
| FUN-01 | 大部分完成 | 补齐 Metalink 任务级暂停/恢复、跨进程重启和剩余诊断 `partial` 证据 |
| FUN-02 | 已修复 | 真实安装环境完成 SQLite ffmpeg path 冒烟测试 |
| ARC-01 | 已修复 | 保持迁移快照和显式恢复回归测试 |
| ARC-02 | 本地闭环，等待 CI | 干净 checkout bindings 全绿；Windows/Linux 默认 runner 完整通过 |
| PERF-01 | 已修复 | 长时间吞吐统计纳入性能基线 |
| PERF-02 | 已修复 | 大分片/多 worker RSS 纳入性能基线 |

### P2：近期工程迭代

| ID | 事项 |
| --- | --- |
| UX-02 | 已修复：error boundary 翻译键、fallback、复制与 reset 恢复测试 |
| UX-03 | 已修复：Hook dependency 告警清零，autosave 防抖与卸载取消组件测试通过 |
| UX-04 | 工程实现完成：窄屏下拉分区导航；待多尺寸中英文视觉/触控验收 |
| UX-05 | 已修复：cursor IPC 改为 `minimumTotal`，UI 移除伪精确剩余数和筛选总数 |
| UX-06 | 已修复：六类 a11y 规则恢复为 error，错误边界、设置和删除确认通过 axe 扫描 |
| FUN-03 | 在 UI/文档/负向测试中明确协议能力边界 |
| FUN-04 | 已增至 17 文件/60 测试；探测、Settings 失败竞态、详情和移动 drawer 已覆盖，继续补主壳贯通流程和真实辅助技术验收 |
| ARC-03 | 按职责拆分五个超大模块 |
| ARC-04 | 从边界包装继续推进结构化错误 |
| ARC-05 | 已消除 ffmpeg 配置重复事实源，保持单一 resolver |
| PERF-03 | 已修复：图标缓存按扩展归一化并使用 256 项 LRU |
| PERF-04 | 已修复：`files_version` 缓存随任务删除/清库淘汰 |
| PERF-05 | 为 50k 搜索建立 query plan/FTS 方案 |
| PERF-06 | 核心阻塞已修复；继续增加部分失败与可取消进度反馈 |
| PERF-07 | 建立 1k/10k/50k 与 100 活跃任务生产规模基线 |

### P3：可靠性稳定后再规划

- FUN-05：JSON-RPC/REST、PAC、路径模板、完整站点规则、云盘/视频嗅探、云账号同步、插件协议框架。

## 十、下一步修复顺序

### 阶段 A：完成外部 P0 验收

1. 在 candidate workflow 跑完 Windows x64、macOS arm64/x64、Linux x64，保存解包结构和 sidecar `--self-check` 日志。
2. 在 Chrome、Edge、Firefox 分别完成 manifest 安装、未启动/已启动交接、request ID 去重和卸载残留测试。
3. 发布 `rc.0`、`rc.1`，用 tag-specific rehearsal endpoint 完成三平台签名升级、重启、数据库保留和 manifest 路径复验。
4. 取得正式商店 ID、账号与 Firefox signed XPI；配置 secrets 后运行 release preflight。OS 包继续 unsigned，除非另行引入代码签名。

### 阶段 B：关闭剩余 P1 证据缺口

1. 在干净 checkout 与 Windows CI 复验 bindings 和全部 Rust integration tests，解决任何仍存在的 loader/pagefile 问题。
2. 取消、重启、FTP/SFTP/WebDAV 单文件校验，以及 FTP/HLS/DASH/WebDAV 暂停/恢复已自动化；下一步补齐 Metalink 完整任务级暂停/恢复、跨进程重启，以及 BT/媒体协议和诊断 `partial` 项。
3. 为 SQLite `ffmpeg_path` 的 HLS 路径、数据库恢复 UI 和 Settings autosave 增加组件/安装环境冒烟测试。

### 阶段 C：P2 规模和可维护性

1. 建立 1k/10k/50k 历史任务、100 活跃任务和大加密 HLS 的生产基线。
2. 图标/files_version 缓存和删除 `spawn_blocking` 已完成；下一步依据数据实施 FTS/search column，并补删除部分失败与可取消进度 UI。
3. 按职责拆分 Settings、TaskDetails、HLS、DASH、Metalink，并继续推进结构化错误码。
4. 最后再排自动化 API、PAC、云和插件生态。

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
pnpm test:release-tools
pnpm verify:extensions
pnpm verify:protocol-matrix
cargo check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
pnpm test:rust
```

浏览器集成变更：

```bash
pnpm verify:extensions
```

发布候选还必须执行真实 bundle，而不是只跑 compile check：

```bash
VIBE_BROWSER_PROFILE=candidate pnpm tauri build --config src-tauri/tauri.ci.conf.json
```

构建后应使用安装包完成本文“发布验收定义”中的安装、Native Messaging、updater、升级和卸载步骤。

## 十三、相关专项文档

- [architecture-audit.md](architecture-audit.md)：早期四维逐项代码复核及历史修复记录。
- [rust-backend-audit.md](rust-backend-audit.md)：Rust 后端可扩展性、效率与安全专项。
- [engineering-quality-audit.md](engineering-quality-audit.md)：测试、CI、迁移、依赖、文档和脚本专项快照。
- [cross-platform-audit.md](cross-platform-audit.md)：Windows/macOS/Linux 运行、打包与系统集成专项。
- [dependency-modernization-audit.md](dependency-modernization-audit.md)：依赖选型与现代化建议。
- [protocol-reliability-matrix.md](protocol-reliability-matrix.md)：非 HTTP 协议统一生命周期、失败恢复与自动化证据基线。
- [browser-extension-privacy.md](browser-extension-privacy.md) / [browser-store-submission.md](browser-store-submission.md)：扩展隐私、最小权限与商店提交资料。
- [updater-rehearsal.md](updater-rehearsal.md)：候选版本 tag-specific updater 演练流程。

这些文档保留历史证据价值，但不应把其中“当时未实现”或“当时已修复”的文字直接当作当前事实；关闭本文事项时必须回到当前代码、自动化测试和真实安装包重新验证。
