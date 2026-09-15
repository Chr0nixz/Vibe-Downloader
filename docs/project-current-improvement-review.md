# Vibe Downloader 当前不足与改进建议

最后更新：2026-09-11  
审查基线：`v0.5.0`（commit `eef88d1`）  
审查范围：React/TypeScript 前端、Tauri/Rust 后端、SQLite、下载引擎、浏览器扩展、构建门禁、发布脚本和现有文档。

## 1. 审查说明

本报告以当前工作区代码为准，并交叉读取以下资料：

- [项目改进审计](project-improvement-audit.md)：当前风险 ID、优先级、证据和验收标准。
- [README](../README.md)、[AGENTS.md](../AGENTS.md)、[ROADMAP](ROADMAP.md)：产品边界和开发规则。
- [协议可靠性矩阵](protocol-reliability-matrix.md)：各协议生命周期自动化证据。
- [性能基线](performance-baseline.md)：已测指标、待测热点和暂定预算。

审查过程包含源码静态检查、关键路径追踪、文件规模统计和本地门禁执行。当前工作区有一组未提交的 i18n/date 修改；本报告不覆盖、不回滚这些改动，并分别记录它们对门禁的影响。

状态沿用主审计：`Open` 表示已由代码路径确认但尚未修复，`Partial` 表示只完成部分契约，`Fixed locally` 表示本地完成但缺少外部验证，`Boundary` 表示明确的产品边界。优先级中，P0 是发布前必须清零的正确性、安全或启动风险，P1 是下一候选版本应清零的主要工作流和资源风险，P2 是近期应处理的体验、完整性和维护风险，P3 是低频问题或需要先测量的优化。

## 2. 结论摘要

项目已经超过 HTTP 下载 MVP。HTTP/HTTPS 主路径具备探测、Range 分段、未知大小、续传校验、限速、队列、重试、诊断和文件冲突处理；前端也有虚拟化列表、游标分页、详情诊断、命令面板、恢复动作、主题和多语言框架。协议可靠性矩阵的核心生命周期单元格已有自动化证据，Specta bindings、SSRF 基础防线、SFTP TOFU、凭据加密、SQLite 外键和 HTTP 分段响应校验也值得保留。

目前的主要风险不是功能入口少，而是同一个承诺在不同引擎、探测阶段、恢复阶段和异常路径上的实现不一致。HTTP 建立的取消、代理、超时、响应校验和文件提交契约没有完整推广到 FTP、SFTP、BT、DASH、HLS 和 Metalink。调度器和文件写入路径仍有 P0 级问题，部分 P1 问题会导致任务僵尸、资源泄漏、成品损坏或恢复退化。

主审计的 2026-08-26 版本列出 64 个 `Open`/`Partial` 跟踪项；当前代码复核确认其核心风险仍然存在。原阶段 A 的 `ARC-19`～`ARC-22`、`SEC-01`、`SEC-02` 的 P0 正确性问题已经修复，不能再次作为当前 P0 报告；但 README 和主审计版本信息仍有漂移，见 [文档一致性](#文档一致性与发布门禁)。

| 维度 | 当前判断 | 首要问题 |
| --- | --- | --- |
| 用户交互便捷性 | 基础交互完整，异常和高频状态下仍有明显不确定性 | 无限滚动跳回、Toast 生命周期、队列重排失败、无声 IPC 失败（`UX-17`～`UX-25`） |
| 功能丰富性和完整性 | 协议覆盖面宽，但成熟度不均，部分声明与真实行为不一致 | 探测代理缺口、跨卷备份失败、DASH 模板/恢复缺陷、备份恢复设置缺口（`FUN-20`～`FUN-27`） |
| 架构鲁棒性和稳定性 | HTTP 的关键契约较强，跨引擎生命周期和所有权边界仍不统一 | 确定性死锁、HTTP worker 未 flush、引擎自取消伪装用户取消、staging 泄漏（`ARC-32`、`ARC-33`、`ARC-37`、`ARC-38`） |
| 程序运行效率 | 已有分页、虚拟化、事件节流和 bundle 预算；长期与大规模数据仍缺实测 | HLS 轮询、路径预留扫描、冗余订阅、双 rustls 后端、缺少 50k/UI/长跑基线（`PERF-09`、`PERF-13`～`PERF-16`） |

安全问题与架构问题有相同的根因：网络请求、凭据、备份和进程边界存在少数绕过统一入口的路径，因此本报告将 `SEC-03`～`SEC-12` 放在架构章节中单独列出。

### 2.1 简洁修复建议和实施方案

| 方向 | 建议 | 实施方案 | 完成标准 |
| --- | --- | --- | --- |
| 用户交互 | 先解决“操作后没有确定反馈”和高频状态跳动。 | 修复分页滚动、Toast 驱逐/计时、队列重排回滚；所有 IPC 入口统一使用 `safeInvoke`，失败显示可重试 Toast；统一列表 ARIA 和表单错误关联。 | 分页追加不跳回；撤销删除没有孤儿；重排失败恢复服务端顺序；键盘/读屏测试通过。 |
| 功能完整性 | 把所有协议的探测和下载行为对齐。 | 建立统一网络客户端工厂，统一代理、超时、SSRF、重定向和凭据源域策略；修复跨卷备份、DASH 占位符/签名续传、Metalink 分片计划、HLS 外轨排序。 | Inherit/Off/Custom 代理在探测和下载一致；异常均返回稳定错误；协议矩阵测试和跨卷备份测试通过。 |
| 架构稳定性 | 先消除锁环和数据写入不变量破坏。 | `ARC-32` 先释放任务锁再 dispatch；`ARC-33` 强制 flush 后 checkpoint；引擎内部失败与用户取消分离；共享 supervisor/segment coordinator 负责 cancel、join、panic、slot 和 staging 清理。 | pause/cancel/delete/restart 无死锁；取消后无后台 worker；文件字节与 checkpoint 一致；P0/P1 生命周期测试全绿。 |
| 安全与恢复 | 将备份、密钥和网络请求纳入同一信任边界。 | keyring 仅对 `NoEntry` 生成新密钥；恢复时清洗 settings；跨卷导出采用 copy+校验回退；凭据按 origin 绑定；所有协议连接前执行私网 IP 审查；WebSocket 增加配额和 Windows ACL。 | keyring 临时失败不丢历史密文；恶意备份不能执行命令；跨域请求不带 Cookie/Authorization；SSRF/bridge 集成测试通过。 |
| 运行效率 | 先测量，再做优化。 | 建立 50k 任务、100 活动任务、HLS/BT 长跑和批量删除基线；减少 HLS DB 轮询和前端冗余订阅；统一 rustls backend；清理 staging/part 残留；按真实 profile 决定 `opt-level`。 | 保存 p50/p95、FPS、RSS、CPU、DB writes/s、events/s 和句柄曲线；优化前后有同环境对比。 |
| 工程门禁 | 让“本地能跑”与“发布可验收”使用同一套入口。 | 新增 `pnpm verify` 聚合前端、Rust、bindings、扩展、协议矩阵、bundle、release tools 和版本检查；CI 增加 macOS Rust test/Clippy、覆盖率、测试 deadline 和定时依赖扫描；自动检查文档 blocker/version。 | CI、README、RELEASE、AGENTS 不再维护不同命令清单；所有发布门禁可重复执行并留存 artifact。 |

建议按以下顺序落地：第一阶段只处理 `ARC-32`、`ARC-33`、`ARC-37`、`ARC-38` 等 P0/P1 生命周期问题；第二阶段完成统一网络/恢复/安全契约和 Metalink/DASH/BT 修复；第三阶段处理 UX、i18n、可访问性和协议边界；第四阶段用实测数据推进性能、覆盖率、CI 和文档治理。每个阶段都应以对应集成测试和回归证据关闭审计 ID，不能只以编译通过作为完成标志。

## 3. 当前门禁和可验证状态

下表来自本次工作区执行。它描述的是当前可复现结果，不等于所有协议已经经过真实外部服务验收。

| 检查 | 当前结果 | 说明 |
| --- | --- | --- |
| `pnpm typecheck` | 通过 | TypeScript 无类型错误。 |
| `pnpm test:frontend` | 通过 | 43 个测试文件、182 项测试通过，约 29 秒。 |
| `pnpm build` | 通过 | 738 个模块构建成功；Rolldown 报告 `src/lib/backup.ts` 的动态 import 无法形成独立 chunk。 |
| `pnpm check:bundle` | 通过 | 初始 shell JS gzip 282.6 kB / 预算 340 kB，CSS gzip 15.4 kB / 预算 18 kB。 |
| `pnpm lint` | 当前失败 | 未提交的 `scripts/check-i18n-completeness.*` 有 2 个 Biome 错误及格式差异；失败来自工作区改动。 |
| `pnpm check:i18n` | 当前失败 | `ja`、`ko`、`zh-CN`、`zh-TW` 各有不可达的 `actions.moreFixesCount_one`；新增 CLDR 复数检查已能发现该问题。 |
| `cargo clippy --locked --all-targets -- -D warnings` | 通过 | 当前 Rust 代码和测试目标通过严格 Clippy。 |
| `cargo test --locked -j 2 --lib` | 通过 | 249 项 Rust lib 测试通过。 |
| `pnpm test:release-tools` | 通过 | 44 个发布脚本测试和 18 个 i18n 脚本测试通过。 |
| `pnpm verify:protocol-matrix` | 通过 | 矩阵结构和状态格式通过。 |
| `pnpm verify:extensions` | 通过 | 4 个开发扩展包构建和 manifest 校验通过，capture=false。 |
| `node scripts/sync-version.mjs --check` | 通过 | package、Tauri、Cargo 当前均为 0.5.0。 |

全量 Rust 集成测试仍不应只看 lib 结果。主审计记录过 BT `.torrent` 测试在系统代理环境下失败或挂起，另有六个引擎集成测试采用无 deadline 轮询；应先完成 `ENG-03`、`ENG-07` 和 `SEC-03` 的修复，再把默认并行全量测试作为发布证据。

## 4. 用户交互便捷性

### 4.1 已有基础

任务列表已经采用游标分页和 `@tanstack/react-virtual`，并有状态筛选、搜索、排序、多选、批量操作、命令面板、Queue Center、Attention Center、详情抽屉、快捷键、Toast、恢复动作、拖放、剪贴板监控和 7 个 locale。`UX-01`～`UX-16` 的启动失败、右键菜单、站点规则草稿、批量命令、焦点、Toast 展开、队列键盘模型、错误码本地化、窄屏排序和 reduced-motion 问题已关闭，后续不应重复当作未修复问题。

### 4.2 当前问题

| ID/优先级 | 当前证据与表现 | 用户影响 | 改进与验收 |
| --- | --- | --- | --- |
| `UX-17` / P1 | `TaskList.tsx` 的滚动 effect 依赖由 `taskIds` 派生的 `filtered`；追加分页会生成新数组并重新执行 `scrollToIndex`。 | 向下滚动触发加载后列表跳回当前选中行，用户误以为加载失败；筛选重置和选中项滚动还可能互相竞争。 | effect 只响应选中 ID，使用 `lastScrolledIdRef` 去重和 `filteredRef` 读取最新列表。追加分页不改变滚动位置，键盘切换选中项仍能滚入视口。 |
| `UX-18` / P2 | 主任务列表使用 `list/listitem` 加 `tabIndex/aria-current`，Queue/Attention 使用 `listbox/option`；设置错误多为 `role=alert`，没有和输入建立完整 `aria-describedby`。 | 屏幕阅读器对三个列表的选择语义不一致；设置数值 clamp 失败时键盘用户不一定知道是哪一个输入有问题。 | 统一 `listbox`、`aria-multiselectable`、`option`、`aria-selected`；在公共设置行统一生成 `aria-invalid` 和错误描述，并补 `jest-axe` 和键盘测试。 |
| `UX-19` / P2 | `toast-store.ts` 达到 20 条时用 `.slice(0, 20)` 静默丢弃最老 Toast，没有执行被丢弃项的 `onAutoCommit`。软删除提交完全依赖该回调。 | 撤销删除 Toast 被驱逐后，任务从列表消失但 DB 和文件仍在，Undo 不可用，重启后任务又出现。批量状态事件会放大触发概率。 | 驱逐前结算 Toast，或让软删除走不会被驱逐的持久队列；验收 21 条 Toast 时无孤儿 `pendingDeleteIds`。 |
| `UX-20` / P2 | `AppShell` 重排先本地乐观更新，失败时只 `setLoading(true)`，没有真正刷新或恢复旧顺序；`loading` 的复位依赖后续列表加载。 | 后端拒绝重排后 UI 继续显示错误顺序，Queue/Attention 的 Load more 可能永久显示 Loading。 | 失败时恢复上一顺序并调用统一刷新；后端拒绝场景必须恢复服务端顺序且解除 loading。 |
| `UX-21` / P2 | ToastViewport 每次渲染创建新的 `onDismiss`，依赖链让 `startTimer` 被无关 Toast 更新重建并重新写入 `startedAtRef`。 | 更新一条进度 Toast 会延长其他 Toast，软删除的硬提交时钟也被推迟，进一步扩大 `UX-19` 窗口。 | 稳定回调或 memo 化 ToastItem；计时器按原始开始时间计算剩余时间。更新一条 Toast 不影响其他 Toast 的 CSS 倒计时和过期时间。 |
| `UX-22` / P3 | `StartupGate` 只在成功路径安排下一次轮询，单次 IPC reject 直接进入失败页。 | 后端只抖动几百毫秒也需要用户手动 Retry，启动体验对偶发 IPC 失败过于敏感。 | 失败时采用有限次数指数退避，超过阈值才显示失败页；前 N 次失败后恢复时自动进入 ready。 |
| `UX-23` / P3 | 剪贴板和 file-drop effect 依赖对话框状态、草稿状态和翻译函数；依赖变化时先注销监听，再异步注册，事件没有回放。 | 在注销和重注册的极短窗口捕获到的链接或文件会静默丢失。 | 监听器使用稳定 handler 和 ref，或注册完成后查询一次 missed 状态；注册窗口内触发的事件必须最终被处理。 |
| `UX-24` / P3 | `TaskRecoveryActions` 对 `navigator.clipboard.writeText()` 使用空 catch，却无条件显示“已复制”。 | WebView 失焦或剪贴板权限拒绝时，用户会带着错误诊断结果去上报问题。 | 与其他复制入口统一处理成功/失败；失败只显示错误提示，不显示成功 Toast。 |
| `UX-25` / P3 | 列表 Refresh、平台查询、目录选择器、ffmpeg 路径选择器和部分 Attention 恢复分支存在 `void` 或裸 `await`，缺少一致的 catch。 | IPC 失败时按钮没有反馈；相同错误通过列表加载入口有错误横幅，通过右键或设置入口却无声失败。 | 建立 `safeInvoke`/统一错误 Toast，覆盖所有文件选择、刷新和平台查询入口；每个 reject 都有可见反馈和可重试路径。 |
| `FUN-22` / P2 | 现有 locale 中 `{{count}}` 句子只有极少数 `_one` 变体；日期格式有的使用 `i18n.language`，有的使用系统 locale；组件 render 内重复构造 `Intl.DateTimeFormat`。 | 英文会出现 “1 connections”；俄语缺少 few/many；切换应用语言后时间显示仍跟随系统区域。当前未提交 CLDR 检查还暴露出日/韩/中不可达 plural key。 | 建立按 CLDR 类别验证的复数资源规则；统一 `format-date.ts` 和 `Intl` 缓存，所有日期传应用语言；切换语言和各 count 边界加入测试。 |

### 4.3 交互层面的产品边界

- 浏览器正式/candidate 包是最小权限的手动交接，自动接管、Cookie/header 转发是显式实验能力；不能把开发扩展行为当成正式发布功能。
- 站点规则的 `ask` 当前是被动跳过，不会弹确认对话框。若继续保留这个名字，应改成更准确的 “不接管/不转发”，或实现真正的询问流程。
- 繁体中文、日文、韩文、俄文和西班牙文仍是 Beta；复数、日期、长文本和错误码应分别验收，不能只用“有 key”描述完成度。
- 没有 Playwright/WebDriver/Tauri GUI 端到端测试。键盘焦点、系统文件选择器、托盘、原生窗口、启动失败页和真实浏览器接管仍需人工 smoke 或桌面自动化补证据。

## 5. 程序功能丰富性和完整性

### 5.1 协议能力现状

| 协议/能力 | 已有能力 | 当前不足或边界 |
| --- | --- | --- |
| HTTP/HTTPS | HEAD/Range 探测、单流和分段、未知大小、自动加速、重试、续传校验、全局/任务限速、诊断、冲突提交 | 主路径最成熟，但分段重试退避仍有不可取消路径（`ARC-27`），统一客户端仍有 sidecar/BT 绕过（`SEC-03`）。 |
| FTP/FTPS | 单文件、动态并行、凭据、目录探测、SOCKS5、暂停/恢复、诊断 | 探测仍使用全局代理（`FUN-20`）；建连和探测缺整体超时（`ARC-26`）；FTP/SFTP resume 不重验远端变更（`ARC-42`）。 |
| SFTP | 密码或 OpenSSH 私钥、加密存储、TOFU、目录探测、SOCKS5、临时文件续传 | 与 FTP 共享 `FUN-20`、`ARC-26`、`ARC-42`；敏感数据仍没有 zeroize（`SEC-05`）。 |
| BitTorrent | magnet、远程/本地 torrent、多文件选择、piece/peer/DHT/做种快照、SOCKS5、ratio/time 做种策略 | 探测创建独立 session 和固定目录；多任务 DHT 端口可能冲突；限速不实时同步且不计入全局桶；torrent URL 有两条网络路径绕过统一 client/SSRF（`ARC-28`、`ARC-29`、`ARC-39`、`SEC-03`）。 |
| HLS | 主变体、AES-128-CBC、EXT-X-MAP、byte range、并发分片、live polling、外挂音轨/字幕、ffmpeg remux | 外部轨道完成顺序未按媒体序排序（`ARC-36`）；段失败可能伪装成取消（`ARC-37`）；staging 不清理（`ARC-38`）；live 轮询期间存在高频 DB 查询（`PERF-15`）。不支持 SAMPLE-AES/DRM 是明确边界。 |
| DASH | 静态/VOD 单 Period、SegmentTemplate/SegmentList/SegmentBase、分段、暂停恢复、ffmpeg remux | `$Number%05d$` 校验通过但未替换（`FUN-24`）；签名 CDN 会让续传退化为全量重下（`FUN-25`）；探测未接任务代理（`FUN-20`）；staging 不清理（`ARC-38`）。Dynamic/live、Timeline、多 Period 等应继续明确拒绝。 |
| WebDAV/WebDAVS | Basic Auth、PROPFIND、HTTP 映射和下载委托 | 单文件路径已共享 HttpEngine，但目录探测仍直接 `build_client`，没有完整复用 client cache/统一策略；需补整体超时和跨引擎 SSRF 验收。 |
| Metalink4 | 多文件、HTTP/HTTPS 镜像优先级和 failover、文件进度、checksum、validator | 失败会删除全部 part（`ARC-24`）；读循环无 idle timeout（`ARC-25`）；健康镜像数变化会重算边界造成续传错位（`ARC-34`）；fresh-start 对 200 和不完整 Content-Range 不够严格（`ARC-35`）；串行 fallback 残留 `.part-N`（`ARC-47`）。 |

### 5.2 当前功能缺口

| ID/优先级 | 代码证据和影响 | 改进方向与验收 |
| --- | --- | --- |
| `FUN-20` / P1 | `DashEngine::probe` 不接收 `request.proxy_config`；FTP/SFTP `probe_target` 从共享全局代理读取。任务设置为 Custom 或 Off 时，探测与实际下载走不同路由，可能建任务失败或误走系统/全局代理。 | 给 DASH、FTP、SFTP 探测统一传入解析后的代理配置；分别用 Custom 和 Off 的真实代理监听器测试探测和下载路径。 |
| `FUN-23` / P1 | 备份先在应用数据卷旁生成 VACUUM 快照，再用 `rename` 移到用户目标；Windows/POSIX 跨卷时 rename 失败并删除好快照。现有集成测试只覆盖同卷 sibling。 | 跨卷 rename 失败时使用 copy+校验+清理，或直接在目标卷生成临时文件；加入不同盘符/跨挂载点测试。 |
| `FUN-24` / P2 | DASH 允许 `$Number%05d$`，但计划生成只替换 `$Number$`，宽度占位符原样进入 URL，导致所有分片 404。 | 实现 `%0Nd` 填充，或在探测阶段明确拒绝；fixture 必须端到端成功或得到稳定 unsupported 错误。 |
| `FUN-25` / P2 | DASH 续传的 DB upsert 同时要求 URI 和 local path 不变；签名 CDN 每次返回新 query 时已完成分片被重置。 | 以 track kind、segment index、大小/时长或模板指纹作为身份，URI 仅更新展示；每次签名变化的 MPD fixture 要跳过已完成分片。 |
| `FUN-26` / P2 | restore 把 `proxy_password_saved=false` 写进当前 live DB，启动时 pending restore 又整体覆盖该库；跨机器恢复时 keyring 无密码但设置仍显示已保存。 | 在 pending restore 成功后对恢复后的库修正标志，或在恢复确认中明确全局代理密码不随备份迁移；跨机 keyring 缺失必须可见。 |
| `FUN-27` / P3 | HLS 外部轨道 gated 于 `if let Ok(Some(hls_task))`，读取瞬时失败就继续无轨道 ffmpeg，和“失败必须可见”的设计注释冲突。 | 将 DB 错误传播为结构化失败，不能生成缺失用户所选音轨/字幕的 Completed 文件；增加模拟 get error 测试。 |
| `FUN-19` / Boundary | 尚未实现稳定 CLI/JSON-RPC/REST、PAC/WPAD、云盘解析、云账号同步、插件协议、完整视频 sniff、Safari wrapper、正式浏览器签名和 OS 代码签名。 | 这些能力应进入后续路线图，不能通过增加入口或文档措辞假装已经完整；先清零 P0/P1 和协议生命周期证据。 |

### 5.3 数据安全功能必须闭环

备份、恢复、跨机器代理密码、完成命令和 ffmpeg 路径实际上共同构成一条数据安全链。当前 `FUN-23` 让跨卷导出失败，`FUN-26` 让恢复后的代理状态失真，`SEC-09` 又表明恢复侧没有清洗 settings，三者应作为一个版本完成，而不是分别标记为“导出已完成”“恢复已完成”。

## 6. 项目架构的鲁棒性和稳定性

### 6.1 最高优先级：先修 P0

#### `ARC-32`：命令锁与调度器锁形成确定性死锁（P0）

`dispatch_inner` 持有调度器全局锁并进入 `start_task`，后者先取得任务运行时锁。暂停、取消、删除和 Restart 路径则先取得任务运行时锁，再在锁内 await dispatch。尤其是 Restart 把任务写回 Queued 后在持锁状态下 dispatch；dispatcher 会对同一任务再次取锁，空闲槽位下可确定性挂起。暂停 queued 任务也能触发同一锁序问题。

影响是整个调度器停止前进，后续创建、设置变化和任务操作都可能排队到应用重启。应仿照 resume/retry 的 spawn 模式，把尾部 dispatch 移到释放任务锁之后；中期固定“调度器锁外快照、start_task 内取任务锁”的锁序，并用 dispatch/pause/delete/restart 竞态集成测试证明没有环。

#### `ARC-33`：HTTP 分段 worker 早退不 flush，可能发布缺字节文件（P0）

`download_segment_once` 在 `offset > current_end`、`write_len <= 0`、动态加速收缩后的 partial-chunk，以及 retryable error 等路径中可能在 flush 前上报 offset。写侧是 256 KiB `tokio::BufWriter`，Drop 不会自动 flush；协调器又把上报 offset 当成 checkpoint，预分配文件长度还会掩盖短写。

动态加速时服务端可能仍按旧 Range 推流，早退会丢掉缓冲尾部；最终文件尺寸看起来完整，但内容存在静默缺口。所有上报都必须建立“先 flush、再上报”的强制契约，最好封装 writer 使调用顺序无法绕过；需要加速分割和每条早退路径的字节级集成测试。

### 6.2 任务生命周期、取消和资源回收

| ID/优先级 | 当前问题 | 影响与改进方向 |
| --- | --- | --- |
| `ARC-23` / P1 | 退出流程外层 timeout 与每个 join future 内层 sleep 使用同一 timeout；外层先 drop `join_all`，逐句柄 abort 分支不可达。 | JoinHandle 被 drop 后只是 detach，worker、ffmpeg、BufWriter 和 DB 写入可能继续运行。改为先等待优雅退出，再逐个 abort 并 await，注入不响应取消的 worker 验收有界退出。 |
| `ARC-24` / P1 | Metalink 任一并行 worker 失败就删除全部 `.part-*`。 | 其他镜像已下载的大量字节被丢弃；应保留 part，并持久化 worker_count、total_size 和 range 边界，计划不一致时才清理。 |
| `ARC-25` / P1 | Metalink 数据面两条 `response.chunk().await` 无 idle timeout，取消只在 chunk 到达后检查。 | 黑洞连接可无限占用调度槽、连接槽、限速器和 DB；复用共享 `read_with_idle_timeout`，取消与读取用 `select!` 竞争。 |
| `ARC-26` / P1 | FTP/SFTP connect 和登录握手是裸调用；多个引擎的 probe 没有整体 deadline。 | 黑洞地址按 OS 默认时间等待，创建对话框无法及时取消。建连、探测和短请求分别设置上限并把 CancellationToken 传入 ProbeRequest。 |
| `ARC-27` / P1 | HTTP worker 对 `Retry-After`/退避直接 sleep，不与 cancel token 竞争。 | 服务器要求等待 60 秒时点击暂停仍会让 worker 醒来后写 checkpoint；改成 cancellable sleep，取消时先提交权威 offset。 |
| `ARC-28` / P1 | BT torrent probe 每次创建 Session，使用固定 `temp_dir()/vibe-downloader-bt-probe`，退出只 drop Arc；目录创建还是同步 IO。 | 与活动 BT 或并发 probe 争抢 DHT/UDP 端口，后台任务和临时目录可能残留。纯 `.torrent` 字节解析不应创建 Session；magnet 应复用受控 session 注册表并有 timeout/cleanup。 |
| `ARC-37` / P1 | HLS/DASH 段失败时调用与用户取消相同的 token；supervisor 看到 cancelled 就跳过失败状态转移。 | 404/重试耗尽后任务可永久停在 Downloading，slot 虽释放但 UI、重试和恢复状态错误。引擎内部 abort 必须返回结构化错误或使用独立 internal_abort，用户取消由命令层记录。 |
| `ARC-38` / P1 | HLS/DASH staging 目录在完成、失败、取消和删除路径没有统一 remove；DASH 删除任务也无法从 `temp_path` 反推出目录。 | 每个大视频可留下接近成品大小的隐藏分片，长期积累数十 GB；成功后清理、启动清理孤儿、删除任务解析 protocol-specific staging。 |
| `ARC-39` / P1 | 每任务 librqbit Session 使用持久 DHT 状态中的端口，多个 session/probe 可能 `AddrInUse`。 | 第二个 BT 任务或 probe 可能持续失败。共享 session，或为每个 session 显式关闭 DHT/分配独立端口，并覆盖“活动下载中 probe/双任务并发”。 |
| `ARC-40` / P2 | supervisor 没有 catch_unwind 或可靠消费 JoinHandle；panic 时 downloads map、host slot、runtime lock 可能不清理。 | 一个引擎 panic 会留下幽灵任务和永久槽位。为 supervisor 加 panic 捕获和统一失败/清理路径，注入 panic engine 验收后续任务仍能调度。 |
| `ARC-41` / P2 | queued 任务的 start 失败最终调用只匹配 `downloading/retrying` 的状态更新。 | 代理/header/DB 错误会让任务永远 Queued，调度 tick 重试并刷日志但 UI 无异常。增加匹配 queued 的显式失败状态和事件。 |
| `ARC-45` / P3 | Restart 直接 abort 后删除 temp，未排空 spawn_blocking 写入；Windows 可能进入 delete-pending。 | 新 worker 打开同一路径会 `ACCESS_DENIED`，Restart 变成 Failed。沿用 cancel+drain，超时再 abort+await，删除失败不应阻断重新初始化。 |
| `ARC-46` / P3 | worker 从 downloads map 摘除后才做可能很慢的 hash；completion action 只看 map/队列是否为空。 | Shutdown/sleep 可能在最后一个文件 hash 尚未落库时执行。completion 判据应包括 pending hash，测试“两任务接力+慢 hash”。 |

### 6.3 续传、分片和协议数据完整性

| ID/优先级 | 当前问题 | 影响与改进方向 |
| --- | --- | --- |
| `ARC-34` / P1 | Metalink 根据当前健康镜像数量重新等分 range；健康数会随 cooldown、416 除名变化。 | part 内容和新边界错位，缺少主 checksum 时可能无声发布坏文件。持久化计划身份和边界，只有一致时才恢复。 |
| `ARC-35` / P1 | Metalink fresh start 对 Range 允许非 206；Content-Range 只比 start；part 长度 `>= expected` 即视为完成且污染 part 不清理。 | 200 HTML/垃圾正文可能被当作分片完成。对齐 HTTP worker：206、start/end/total 全字段精确匹配，长度必须等于 expected，失败时清理该 part。 |
| `ARC-36` / P1 | HLS 外部音轨/字幕用 JoinSet 完成顺序写本地 playlist，没有按媒体序排序。 | 并发完成顺序改变时，对白/字幕乱序或漂移且没有告警。按 discontinuity/media sequence 排序后再写 playlist。 |
| `ARC-42` / P2 | FTP/SFTP 只在 probe 记录 SIZE/MDTM，resume 不重新确认远端文件是否变化。 | 等大小替换文件会产生旧新内容拼接并标记 Completed。resume 前复核 SIZE/MDTM，不一致转为明确的 resume_blocked/restart。 |
| `ARC-43` / P2 | BT `delete_runtime_task` 只收 source_key，按 HashMap 顺序删除第一个成功 session，调用方丢弃 task.id。 | 同一种子多任务时取消 A 可能删除 B 的 torrent，B 进入 Failed。按 `(source_key, task_id)` 精确定位 owning session。 |
| `ARC-47` / P3 | Metalink 从 parallel fallback 到 serial 后没有清理兄弟 `.part-N`。 | 纯磁盘泄漏，规模可达文件大小乘镜像数。serial 成功 finalize 后统一清理 part。 |
| `ARC-48` / P3 | DASH byte range 用 i64 做 `end-start+1` 和 header 加减，没有 checked/saturating 防溢出。 | 恶意 MPD 在 debug panic、release 回绕；又会放大 `ARC-40` 的 slot 泄漏。解析阶段拒绝溢出并加入极端 fixture。 |

### 6.4 错误语义和模块边界

| ID/优先级 | 当前问题 | 改进方向 |
| --- | --- | --- |
| `ARC-30` / P2 | DASH 取消、批量 skipped、SFTP 权限和 probe 分类仍依赖英文 `contains()` 或库/OS 英文文本。 | 用 cancel token、`AppErrorPayload.code`、reqwest 类型谓词和 `io::ErrorKind` 分流；文案变化不应改变状态机。 |
| `ARC-31` / P2 | 当前大文件：HLS 2374 行、Metalink 2366、BT 2179、DASH 2142、FTP 1744、SFTP 1651；`SettingsPage.tsx` 还约 2600 行。FTP/SFTP 协调器有约 600 行近重复代码，header/decode helper 也重复。 | 修复和测试需要多处同步修改，容易重新引入不同步的取消、代理和 flush 语义。抽共享 SegmentCoordinator、网络 header/decode 工具和按领域拆分 Settings/TaskDetails，并保留现有行为测试。 |
| `ARC-44` / P3 | `start_task` 的 Ok 同时表示真正启动、已存在控制和 Conflict 清理，dispatch 统一计入 active/host slot。 | 通常只会保守推迟下一任务，但计数不诚实。返回 `Started/AlreadyActive/ConflictSkipped` 枚举并分别记账。 |

### 6.5 安全边界和统一入口

| ID/优先级 | 当前问题 | 影响 | 改进方向 |
| --- | --- | --- | --- |
| `SEC-03` / P1 | BT `.torrent` 下载和 task create 的 sidecar checksum 自建 reqwest client；BT `AddTorrent::from_url` 还把 URL 交给 librqbit 内部 client。缺少统一 proxy、`no_proxy`、SSRF resolver 和重定向策略。 | SSRF、代理 Off 失效、系统代理误用；已有 BT 测试在系统代理环境下失败/挂起。 | `download/` 禁止裸 `Client::builder()`；所有网络请求经过 HttpEngine/NetworkClientFactory，统一测试 Off/Custom/重定向到私网。 |
| `SEC-04` / P1 | WebSocket bridge 只有 token，没有创建请求配额或 Origin 校验；Windows 引导文件 readonly 不是 ACL；扩展 `onMessage` 未校验 sender。 | 同用户进程拿到 token 后可无限创建任务、耗尽磁盘；引导 token 暴露面过大。 | Windows DACL/命名管道、创建令牌桶、Origin 白名单、把 token 移出 query、校验 `sender.id`。 |
| `SEC-05` / P1 | password、私钥、解密 header 和 encryption key 都是普通 String/数组，没有 zeroize。 | 崩溃转储、页面文件或休眠镜像可能保留敏感明文。 | `ZeroizeOnDrop`/`Zeroizing` 覆盖凭据结构、解密中间 buffer、key 和 headers；用测试验证生命周期结束后不保留原文。 |
| `SEC-06` / P2 | `browser_messages` 保存未 sanitize 的完整 URL，且没有 prune/TTL；其他事件表有清理。 | 预签名 URL、query token 和长期下载凭据永久留在未加密 SQLite。 | 入库前 sanitize query，增加按年龄和上限清理，并接入启动/周期清理；验收表中无 query 且行数有界。 |
| `SEC-07` / P2 | Windows 完成命令允许裸 executable 名，CreateProcess 搜索顺序包含当前工作目录。 | 可写 CWD 放置同名 exe 时存在工作目录劫持。 | 要求绝对路径，或先按 PATH 解析为绝对路径后执行；补 Windows 单测。 |
| `SEC-08` / P1 | keyring `get_password()` 任意 Err 都被当作 NoEntry，生成新密钥并覆盖旧密钥。 | keyring 瞬时故障/ACL 错误可能让全部历史凭据、Cookie、代理密文不可逆解密。 | 只对 `NoEntry` 生成密钥，其余返回 `secrets_unavailable`；中期提供显式密钥轮换和迁移。 |
| `SEC-09` / P1 | 备份恢复只检查 tasks/task_files 路径，不清洗 settings；`completion_run_command` 和 `ffmpeg_path` 可由 crafted backup 带入。 | 恢复备份后重启/队列排空可能以用户权限执行外部命令。 | 恢复时强制 `completion_action=notify`、清空 command/path，known_hosts 需显式披露；恶意 settings 集成测试必须拒绝。 |
| `SEC-10` / P2 | 连接期 SSRF 过滤在 resolver 中，IP 字面量被 connector 直接连接，绕过 resolver；handoff 边界检查不能覆盖剪贴板/直建任务。 | `169.254.169.254`、`10.0.0.1` 等字面 IP 可在 UI/clipboard/metalink 等路径访问。 | 在 connect 前统一检查 authority；字面量直接判定，域名解析后逐 IP 判定；为 HTTP/HLS/DASH/Metalink 和其他协议共用。 |
| `SEC-11` / P1 | Basic Auth、Cookie/header 在 Metalink mirror 和 HLS/DASH 跨源 URI 上无 origin 绑定。 | 凭据可能发给 manifest 中第三方镜像或跨源媒体主机，泄露可能完全无感。 | 按注册域/精确 host 绑定 Authorization/Cookie，跨域默认剥离并在 UI 明示；同域和跨域集成测试。 |
| `SEC-12` / P2 | FTP/SFTP parse 只提取 host/port，connect/probe 前没有私网/保留 IP 审查。 | 剪贴板一击即可探测内网 FTP/SFTP，细粒度错误还形成测绘原语。 | 与 `SEC-10` 共用前置地址策略；必要的内网访问只能由显式白名单开启并有提示。 |

## 7. 程序运行效率

### 7.1 已有优化和当前实测

现有代码已经使用游标分页、虚拟列表、增量 queue 事件、进度事件 250 ms 节流、HLS key/init-map singleflight、有限 cache、WAL 和 bundle budget。当前生产前端构建初始 shell JS gzip 为 282.6 kB，低于 340 kB 预算；`pnpm test:frontend` 的 182 项测试通过，说明前端纯逻辑回归基础不错。

但 bundle 构建提示 `src/lib/backup.ts` 同时被静态和动态 import，动态 import 不能把它移到独立 chunk；初始 shell 仍包含约 55.9 kB gzip 的 React vendor、49.8 kB 的 utils、44.9 kB Radix、38.2 kB Motion。预算通过不等于首屏、解析时间和长期 RSS 已达标。

### 7.2 当前性能问题

| ID/优先级 | 热点和证据 | 影响 | 改进和测量 |
| --- | --- | --- | --- |
| `PERF-09` / P3 | release profile 使用 `opt-level="s"`；没有和 `opt-level="3"` 对 hash/AES/XML/BT 热点做对照。 | 可能以少量体积换取下载/校验吞吐下降，当前没有数据支持决策。 | 在相同硬件、release 构建下比较大小、启动、hash MB/s、AES/XML/BT 吞吐后再决定。 |
| `PERF-13` / P2 | Cargo.lock 同时引入 rustls 的 `aws-lc-rs` 和 `ring`；suppaftp 与 librqbit 的 feature 组合导致两套后端进入二进制。 | 增加编译时间、二进制体积、冷启动和供应链审查面。 | 统一 rustls crypto backend，跑 `cargo tree -e features`、`cargo deny check bans sources` 和 release size 对比。 |
| `PERF-14` / P2 | 前端任务列表、事件 hook、详情和 speed history 有多层订阅；部分 selector 会因新对象引用反复触发渲染。 | 活动任务数提高时 React commit、GC 和电量消耗上升，可能抵消虚拟化收益。 | 用 React Profiler 和 100/1000 活动任务 fixture 找出重复订阅，按 ID/字段选择器、memo 和批量 patch 收敛；以 FPS、commit 和 CPU 设基线。 |
| `PERF-15` / P2 | HLS live 轮询期间约每 100 ms 查询数据库，远高于可见 UI 更新和媒体目标时长。 | 长直播会产生大量 SQLite read、锁竞争和无意义 wake-up。 | 用内存态 segment snapshot 或 250/500 ms 合并 tick；按 live latency 目标测 DB reads/s、CPU 和内存。 |
| `PERF-16` / P2 | 路径预留会扫描较大的保留集合，若干查询缺少明确 LIMIT；多文件任务在事务外/重试中物化大量路径字符串。 | 1 万活动任务或每任务百个文件时，创建延迟和锁持有时间放大，影响调度和其他写入。 | 增加 `(save_dir, final_path)` 等索引、分页/批量查询和 bounded input；用 1k/10k/50k 任务、100/1000 文件 fixture 测 p95。 |
| `ARC-29` / P2 | BT session 的限速只在创建时读取一次，且流量不进入全局 token bucket。 | 下载中修改限速无效，全局“最小值生效”承诺被 BT 绕过。 | 1 秒 tick 同步任务/全局限速；若库无法进入全局桶，UI 必须明确说明，最好以全局剩余量估算 session limit。 |
| `ARC-47` / P3 | Metalink `.part-N` 残留是持续磁盘增长问题，属于资源效率而非单次正确性。 | 长期运行或频繁 mirror failover 会耗尽下载目录。 | 与 staging manager 一起做成功、失败、取消、parallel→serial 和启动清扫测试，记录目录大小曲线。 |

### 7.3 目前缺少的性能证据

性能文档已有 harness 和方法，但仍缺少发布候选所需的绝对数据：

- 50k/100k 任务库冷启动、首屏、搜索、筛选、排序、连续滚动 FPS 和 RSS。
- 10/50/100 个混合活动任务的 CPU、RSS、DB writes/s、events/s 和 UI FPS。
- 100/1000 个多文件 torrent 的快照 payload、heap 和 per-file 更新成本。
- HLS/BT 30 分钟及 8 小时 soak 的 RSS、WAL、task_events、句柄、临时文件和子进程曲线。
- 1k 文件删除和大量 Metalink/DASH staging 清理的尾延迟。
- release `opt-level` 对 hash/AES/XML/BT 的真实吞吐对比。

没有上述数据时，不应贸然引入 FTS5、全局 LRU、进一步拆包或 profile override；先证明热点，再用修复前后相同环境的 p50/p95、峰值 RSS 和资源曲线做决策。

## 8. 文档一致性与发布门禁

这是当前一个独立的可维护性问题，既影响用户预期也影响审查结论。

1. `package.json`、Cargo、Tauri 和 README 已是 `0.5.0`，但 [project-improvement-audit.md](project-improvement-audit.md) 顶部仍写适用版本 `0.4.0`。
2. README 的“当前发布阻断”仍列 `ARC-19`～`ARC-22`、`SEC-01`、`SEC-02`，而 `AGENTS.md` 和主审计后文已说明这些 P0 正确性问题已关闭；README 还把 `ARC-19` 描述成会造成数据损坏，当前代码修复后只剩协调器取消时未排空的 P2 资源/重下载残留。
3. `docs/RELEASE.md`、README、CONTRIBUTING、AGENTS 和 CI 的检查清单不是同一个集合；当前 CI 已使用 `cargo clippy --locked --all-targets`，部分文档仍推荐不带 `--all-targets` 的命令。
4. CI Rust 矩阵仍只有 Ubuntu 和 Windows；macOS 平台代码没有完整 Rust test/Clippy 证据。
5. `.gitattributes` 已存在，但 `SECURITY.md`、`CHANGELOG.md`、`CODE_OF_CONDUCT.md`、`.editorconfig`、Dependabot、CODEOWNERS 和 issue/PR 模板仍缺失；根目录还有引用已删除 HLS 文件的 `_apply_f6.ps1`。
6. 当前未提交 i18n/date 改动让 `pnpm lint` 和 `pnpm check:i18n` 失败。新增 CLDR/source-key 检查的方向正确，但必须先修复不可达 plural key、Biome 格式和测试字符串写法，再把它们纳入提交门禁。

建议新增一个唯一验证入口，例如 `pnpm verify`，由它调用前端、Rust、bindings、extensions、protocol matrix、release tools、bundle 和版本检查；CI、README、RELEASE 和 AGENTS 只引用这个入口。`sync-version.mjs` 应同时检查 Cargo.lock 是否需要更新，发布流程才能稳定使用 `--locked`。

## 9. 建议修复顺序

### 第 0 批：阻止数据损坏和确定性死锁

1. `ARC-32`：释放任务锁后再 dispatch，先加入 pause/cancel/delete/restart 竞态测试。
2. `ARC-33`：所有 HTTP worker 早退统一 flush 后 checkpoint；加动态加速和 retry 早退字节级测试。
3. `ARC-37`：区分内部失败和用户取消；失败必须进入 Failed/NeedsAttention 并释放 slot。
4. `ARC-38`：统一 HLS/DASH staging 的成功、失败、取消、删除和启动清扫生命周期。

### 第 1 批：统一网络和恢复契约

1. 建立统一 `NetworkClientFactory`：代理 Inherit/Off/Custom、连接/idle/总超时、SSRF、重定向、header origin 绑定全部由共享入口执行；解决 `FUN-20`、`SEC-03`、`SEC-10`、`SEC-11`、`SEC-12`。
2. 建立共享 `DownloadSupervisor`/`SegmentCoordinator`：统一取消、JoinSet 排空、flush-before-checkpoint、panic 清理、slot 释放；覆盖 `ARC-23`、`ARC-24`、`ARC-25`、`ARC-26`、`ARC-27`、`ARC-31`。
3. 持久化 Metalink/DASH 分片计划身份和边界，严格对齐 HTTP 的 206/Content-Range/validator 契约；解决 `ARC-34`、`ARC-35`、`FUN-25`。
4. 修复 keyring 错误分类、备份 settings 清洗、跨卷导出和 proxy password 状态；解决 `SEC-08`、`SEC-09`、`FUN-23`、`FUN-26`。
5. 修复 BT session 所有权、探测复用、端口、删除定位和动态限速；解决 `ARC-28`、`ARC-29`、`ARC-39`、`ARC-43`。

### 第 2 批：恢复用户工作流和协议体验

1. 处理 `UX-17`～`UX-21`，让分页、Toast、重排和撤销动作在压力下仍可预测。
2. 统一 ARIA、表单错误关联、复制/刷新/目录选择器的异常反馈；处理 `UX-18`、`UX-24`、`UX-25`。
3. 修复 DASH `$Number%05d$`、HLS 外部轨排序、FTP/SFTP 远端变更检测和 Metalink part 清理；处理 `FUN-24`、`FUN-27`、`ARC-36`、`ARC-42`、`ARC-47`。
4. 完成复数/日期本地化，修正当前工作区 i18n 门禁；处理 `FUN-22`。

### 第 3 批：性能和防复发

1. 先完成 50k/UI/100 活动任务/HLS-BT soak 基线，再处理 `PERF-09`、`PERF-14`、`PERF-15`、`PERF-16`。
2. 统一 rustls feature，完成 `cargo deny check bans sources` 和 release 体积/吞吐对比，处理 `PERF-13`。
3. 建立覆盖率（Rust `llvm-cov` + Vitest v8）、macOS Rust CI、测试 TempDir/超时、扩展 background 行为测试，处理 `ENG-03`、`ENG-04`、`ENG-07`。
4. 建立 `pnpm verify` 和文档自动检查，修复版本、阻断项、命令清单和能力声明漂移，处理 `ARC-18`、`ENG-05`、`ENG-06`、`ENG-08`。

## 10. 发布前验收清单

公开稳定发布前至少应满足：

- `ARC-32`、`ARC-33`、`ARC-37`、`ARC-38` 等 P0/P1 生命周期和数据完整性问题全部 Closed。
- HTTP、FTP/FTPS、SFTP、BT、HLS、DASH、WebDAV、Metalink 的代理、认证、暂停、恢复、重启、取消、删除和同名文件冲突都有真实服务或固定 fixture 证据。
- 所有网络引擎经过统一 client 工厂；`download/` 下没有裸 `reqwest::Client::builder()`；Off/Custom 代理、SSRF 字面 IP、重定向和停滞连接都有契约测试。
- staging、part、临时文件、文件句柄、BT session、ffmpeg 和 worker 在成功、失败、取消、删除、重启和退出后都有界。
- 备份导出支持跨卷；恢复会清洗 settings 并正确处理本机 keyring 缺失；凭据 header 按源域绑定。
- TaskList、Queue Center、Attention Center 统一 ARIA 模型，表单错误与输入程序化关联；所有危险 IPC 失败都有可见反馈。
- 7 个 locale 的 key、占位符、CLDR 复数、日期和稳定错误码检查全部通过；Beta locale 的未完成翻译仍明确标注。
- `pnpm verify`、前端测试、生产 build、bundle、bindings、扩展、协议矩阵、release tools、Rust test 和 `cargo clippy --all-targets` 全绿；Rust 测试和 Clippy 在 macOS 也执行。
- 有目标硬件上的冷启动、首屏、搜索、滚动、RSS、CPU、DB writes/s、events/s、长时间运行和批量删除基线，并保存原始 trace/query plan/CSV。
- README、ROADMAP、PRODUCT、DESIGN、协议矩阵、发布说明、版本号和当前审计状态一致；正式安装包仍明确标注未代码签名的事实。

## 11. 应保留的设计和实现优势

- HTTP worker 对 206 和 Content-Range 的 start/end/total 做严格校验，应作为 Metalink 等引擎的范本。
- SFTP 已有双侧 seek、flush-before-checkpoint 和字节级恢复测试，不应在重构中退回为只按长度恢复。
- SQLite 使用 WAL、外键级联和参数化查询；路径预留、最终发布和 backup 内容校验的 P0 基础已经建立。
- 凭据使用 ChaCha20-Poly1305，SFTP TOFU 在认证前校验，浏览器 handoff 保持 HTTP/HTTPS、无嵌入凭据、无本地路径控制和 header allowlist 边界。
- 前端 task data、task UI、speed history 已拆分；任务列表分页、虚拟化、增量事件和 bundle 预算方向正确。
- 所有明确不支持的能力（HLS DRM、DASH dynamic/timeline/multi-Period、implicit FTPS over SOCKS5、浏览器正式自动接管）应继续显式拒绝并返回稳定错误，而不是静默降级。

## 12. 可新增或完善的功能和页面

本节只列当前产品定位下有实际价值的增量。主任务列表已经是核心工作台，不建议再增加一个大面积、卡片化的营销式首页；新页面应服务于监控、恢复、批量管理或配置闭环。

### 12.1 建议新增的页面

| 页面 | 优先级 | 主要内容 | 推荐实现方案 |
| --- | --- | --- | --- |
| 下载历史与统计 | P1 | 按日期、协议、域名、状态和分类统计成功率、平均速度、总流量、失败原因和耗时。 | 复用任务和事件表，增加聚合查询与时间范围筛选；图表只展示趋势，详细数据仍可导出 CSV/JSON。先做 7/30/90 天统计，不做实时 BI 系统。 |
| 存储与清理中心 | P1 | 显示下载目录、临时文件、`.part-*`、HLS/DASH staging、孤儿任务和可回收空间。 | 后端扫描器返回只读报告；清理操作必须按文件类型、任务关联和校验状态确认，接入启动清扫和 `ARC-38`/`ARC-47` 生命周期。 |
| 全局诊断中心 | P1 | 展示活动任务、调度槽、主机连接、代理、失败任务、worker/ffmpeg/BT session、数据库/WAL 和最近错误。 | 把现有 Environment、Task Details Logs 和 Attention 数据聚合为只读视图；每一项提供“查看任务”“重试”“打开日志”等动作。 |
| 计划与队列日历 | P1 | 以时间轴查看下载窗口、限速时段、队列优先级和即将启动/暂停的任务。 | 复用现有 scheduled window 和 priority 模型；日历只负责可视化和编辑，不复制另一套调度规则。 |
| 浏览器集成中心 | P1 | 显示扩展安装状态、Native Messaging manifest、连接状态、当前构建 profile、权限说明和测试按钮。 | 复用 EnvironmentPanel 与 BrowserCaptureControls；按 release/candidate/dev 明确区分能力，提供安装、修复、诊断和重新发送 handoff。 |
| 备份与恢复中心 | P1 | 显示最近备份、文件校验、恢复前预览、跨卷提示、恢复后需重新配置的 keyring/代理信息。 | 从 Settings 中抽出独立工作流，恢复前执行内容策略检查，展示任务数、文件数、设置白名单和风险提示；复用现有 `.vibe-backup` 协议。 |
| 任务模板与预设 | P2 | 保存下载目录、分类、优先级、限速、代理、校验和、完成动作等组合配置。 | 新建 `task_presets` 数据模型；NewDownloadDialog 选择预设后仍允许逐项覆盖；敏感凭据只引用已保存 credential ID，不把明文写入模板。 |
| 协议能力与健康页 | P2 | 展示 HTTP/FTP/SFTP/BT/HLS/DASH/WebDAV/Metalink 的支持范围、代理/认证/续传状态和最近失败率。 | 从协议可靠性矩阵和运行时诊断生成“支持/限制/需配置/失败”状态；不把自动化 fixture 结果冒充真实外部服务质量。 |
| 全局日志查看器 | P2 | 按时间、任务、模块、级别筛选日志，支持复制脱敏片段和导出诊断包。 | 后端提供分页、级别过滤、大小上限和脱敏输出；默认只显示摘要，原始日志仍保留在本地文件。 |
| 网络与连接监视器 | P2 | 查看每个任务的连接数、主机槽位、代理路径、重试、吞吐和最近响应状态。 | 复用 request diagnostics 和 scheduler snapshot；使用 250 ms 到 1 s 的采样窗口，避免为显示监控而增加高频 DB 写入。 |

### 12.2 现有页面应补齐的功能

| 现有页面 | 建议完善内容 | 价值和完成标准 |
| --- | --- | --- |
| New Download | 增加 URL 规范化预览、重复任务/同名文件冲突预览、预计文件大小、代理与凭据生效提示、批量导入去重、每文件选择摘要。 | 用户在点击开始前能知道“将保存到哪里、会覆盖什么、走哪个代理、哪些文件会下载”；所有探测失败都有可操作的恢复动作。 |
| Task List | 增加列显示自定义、保存筛选视图、批量编辑分类/优先级/限速、全选当前查询结果、列宽记忆和更明确的队列等待原因。 | 大量任务下不依赖命令面板才能完成常用操作；分页追加不跳回，筛选和批量操作作用于完整查询集合。 |
| Task Details | 增加任务时间线、远端 validator/文件变更摘要、重试历史、代理路径、磁盘/网络瓶颈和“复制脱敏诊断包”。 | 用户能回答“为什么慢、为什么暂停、为什么不能续传”；所有协议展示真实字段，Unsupported 不显示伪造的通用分片信息。 |
| Queue Center | 增加拖放排序预览、按主机/优先级分组、每主机槽位占用、计划窗口倒计时和批量移动。 | 重排失败回滚；队列状态与 scheduler snapshot 一致；不再通过隐藏快捷键完成关键操作。 |
| Attention Center | 增加按错误类别批量修复、网络恢复后重新探测、凭据/代理缺失的直接跳转和磁盘空间释放建议。 | 同类失败可以一次处理，恢复动作带明确前置条件；不会把内部错误、用户取消和远端拒绝混成一个按钮。 |
| Settings | 增加“未保存更改”摘要、按影响范围重置、设置导入导出预览、敏感设置说明和搜索结果定位。 | 保存失败保留草稿；恢复/重置前显示范围；数值错误与输入有 `aria-describedby` 关联。 |
| Environment | 将当前健康检查升级为持续状态摘要，显示检查时间、结果来源、修复是否需要重启，以及“复制完整脱敏报告”。 | 不把“可检测”误报成“可修复”；代理、ffmpeg、目录、数据库、扩展和 updater 的状态均能追溯。 |
| Browser Integration | 增加手动测试 handoff、Native Messaging 重连、扩展 profile/权限差异说明和过期 header 恢复记录。 | 用户能判断问题在浏览器、扩展、manifest、bridge 还是桌面端；正式包仍保持最小权限。 |
| About | 增加版本、更新通道、变更摘要、许可证、依赖信息和诊断入口。 | About 不只是静态版本页；更新失败有重试和日志入口，但不宣称未签名包已经完成生产分发。 |

### 12.3 值得增加的后台能力

这些能力不一定都需要独立页面，但会显著改善长期使用：

1. **智能重试策略**：按错误类型、HTTP 状态、Retry-After、网络恢复和镜像健康度选择退避，而不是所有失败都使用同一种重试；用户可看到下一次重试时间和原因。
2. **断点与任务迁移**：导出不含明文凭据的任务包，在另一台机器恢复后重新绑定本地 keyring、代理和保存目录；显示哪些字段被清洗或需要确认。
3. **下载完成校验策略**：支持“校验和不匹配自动保留现场”“重新下载损坏分片”“只报告不阻断发布”等明确模式，避免把校验失败处理成单一失败状态。
4. **磁盘配额和预警**：按保存目录设置软/硬阈值，下载前检查剩余空间，运行中在预计不足时提前暂停并提供清理入口。
5. **网络恢复策略**：系统从离线恢复、代理恢复或网络切换后，对可重试任务分批唤醒，避免所有任务同时重连造成雪崩。
6. **可选自动化接口**：在稳定性和权限模型成熟后提供本地 CLI/JSON-RPC；默认仅绑定本机、需显式开启、按 token/权限分级，并复用同一任务和安全策略。

### 12.4 中长期功能，暂不应提前实现

以下方向有产品价值，但应排在当前 P0/P1 风险、跨引擎契约和性能基线之后：

- 云盘解析、云账号和跨设备同步。
- 更完整的视频嗅探、媒体候选管理、字幕/音轨选择和格式转码。
- PAC/WPAD 代理和企业级凭据/证书管理。
- 插件协议和第三方引擎扩展机制。
- Safari wrapper、浏览器商店正式发布、自动接管和 Cookie 权限扩大。
- 远程控制、REST 服务和多用户协作。

这些功能的共同前提是：权限边界、凭据生命周期、取消/恢复语义、审计日志和资源配额先有统一实现。否则只是把当前的跨引擎契约缺口扩大到更多入口。

### 12.5 推荐新增顺序

1. **先补闭环页面**：存储与清理中心、全局诊断中心、浏览器集成中心、备份与恢复中心；它们直接消费现有数据和命令，能明显减少用户“任务出问题后无从处理”的情况。
2. **再补高频效率**：任务预设、保存筛选视图、批量编辑、Queue 日历、网络连接监视器；优先改善重复操作，不改变下载核心协议。
3. **最后扩展新产品面**：历史统计、CLI/JSON-RPC、云盘、媒体嗅探、插件和 Safari；每项先写权限、数据模型、失败恢复和性能预算，再进入实现。

每个新增页面都应满足四项最低标准：有空状态、加载状态、错误状态和权限/能力说明；所有异步动作可取消或可重试；不会绕过统一网络和安全入口；至少有一个前端交互测试和一个真实后端/集成验收场景。
