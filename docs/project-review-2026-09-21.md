# Vibe Downloader 四维现状审查与改进建议

审查日期：2026-09-21  
项目版本：`0.5.0`  
基准提交：`d787a6ff9540c82a5fe44682d8865f9eea3e03e0`，**包含审查开始时的未提交工作区修改**。  
范围：用户交互便捷性、程序功能丰富性与完整性、项目架构鲁棒性与稳定性、程序运行效率。

快速导航：[审查说明](#1-阅读方式与证据边界) · [总体结论与检查结果](#2-总体结论) · [用户交互](#3-用户交互便捷性) · [功能完整性](#4-程序功能丰富性和完整性) · [架构与稳定性](#5-项目架构的鲁棒性和稳定性) · [运行效率](#6-程序运行效率) · [实施顺序](#7-建议实施顺序与工作包) · [验收方案](#8-统一验收方案) · [范围取舍](#9-应明确保留的产品边界与后续决策)

## 1. 阅读方式与证据边界

这是一次基于当前源码、既有测试和局部复现实验的审查快照，不替代 [项目改进审计](project-improvement-audit.md) 的唯一风险登记地位。本次不修改业务实现、不变更既有问题的关闭状态，也不覆盖用户已有工作区修改。后续决定修复本轮发现时，应先在主审计中登记正式 ID，并重新核对当时的源码。

本轮编号 `R26-U`、`R26-F`、`R26-A`、`R26-P` 分别对应交互、功能、架构、效率，只用于本报告内引用。关联历史 ID 表示需要保留其既有保证，**不表示重新打开该历史条目**。

| 证据等级 | 含义 | 阅读注意 |
| --- | --- | --- |
| 实验确认 | 用当前组件／Rust 模块，或明确标注的底层原语／独立算法 fixture 观察到具体行为 | 实验通过表示成功复现现状，不表示缺陷已修复；局部实验不等于完整安装包复现 |
| 代码确认 | 当前可达调用链或明确的数据流已显示问题 | 明确列出触发条件；修复时仍需集成回归测试 |
| 待实测风险 | 存在算法、规模、并发或资源方面的风险，但未测得用户侧损害 | 不把 O(N)、大文件或某个依赖直接写成“必然卡顿” |
| 能力边界／改进机会 | 当前明确不支持，或已有基础上可以提升 | 不将未承诺的协议、云服务、DRM 等写成实现错误 |
| 验收缺口 | 代码或自动化存在，但真实服务、桌面或发布证据不齐 | 不等于功能必然不可用，也不等于已经通过验收 |

优先级沿用主审计的原则：P0 涉及必须阻断发布的数据完整性、隐私或核心不可用问题；P1 应在下一候选版本前处理；P2 进入近期迭代；P3 是低频增强或必须先测量的优化。本文没有凭旧报告重新宣布一批已关闭的 P0。对本轮发现的文件覆盖等高影响路径，应优先完成完整故障注入，再由主审计确定最终发布级别。

审查包含前端、Rust 命令与引擎、SQLite、浏览器边界、CI／发布脚本、依赖图和性能资料。没有执行三平台安装包验收、真实外部协议服务器验收、长时间资源压力测试或完整 Rust 测试／Clippy。本轮尝试打开本地界面预览，但 computer-use 因无法可靠确认浏览器 URL 而中止；未获得可用的界面截图，以下不宣称完成了视觉、缩放、读屏或 GUI 端到端验收。

## 2. 总体结论

项目已经具有较完整的桌面下载管理基础。HTTP 的分段、续传、校验、调度、代理、文件冲突处理，以及前端的虚拟化、分页、恢复入口、设置、多语言、数据备份与诊断，都不是空壳功能。非 HTTP 协议也有相当数量的本地自动化证据。下一阶段最有价值的工作，是把跨阶段契约和异常路径补齐，再取得可发布、可长期运行的实测证据。

| 维度 | 当前优势 | 主要不足 | 优先行动 |
| --- | --- | --- | --- |
| 用户交互便捷性 | 高频操作、批量入口、快捷键、恢复中心、任务诊断已形成体系 | 新建窗口状态隔离、批量失败结果可见性、草稿保护及原生环境验收仍有缺口 | 先修复能够稳定复现的创建／导入问题，再补真实桌面交互测试 |
| 功能丰富性和完整性 | 协议面广，规则、备份、完整性记录、存储管理等辅助能力已落地 | 认证信息跨探测／创建阶段不一致；部分 DASH 子集、浏览器分发与诊断深度仍不完整 | 用同一凭据与代理上下文贯穿全过程，明确媒体支持范围 |
| 架构鲁棒性和稳定性 | 状态转移、取消、SSRF、TOFU、加密、失败恢复和门禁已有修复基础 | 文件提交原子性、备份写入、异常密文输入、发布引用一致性，以及巨大模块仍需处理 | 优先守住文件与备份，再补边界故障注入；逐步收敛重复契约 |
| 程序运行效率 | 列表虚拟化、游标分页、增量事件、250ms 节流、bundle 预算有效 | 限速器后台任务生命周期有实证问题，实体缓存与部分查询无界；当前版本的 UI／长跑实测不足 | 先消除资源泄漏，再测量缓存、查询和前端更新成本 |

### 2.1 与历史结论的关系

从主审计的条目标题统计，当前共有 **140 项：133 Closed、2 Open、3 Partial、1 Needs benchmark、1 Boundary**。七项非 Closed 内容如下，不能与历史正文中的旧状态表混读：

| 既有 ID | 当前状态 | 本轮应继续做什么 |
| --- | --- | --- |
| `ARC-17`、`ARC-31` | Partial | 继续模块拆分和跨引擎去重；FTP／SFTP 的取消排空已经修复，不应重报 |
| `ENG-03` | Partial | 完成测试临时资源清理和隔离收尾，保留已完成的环境变量等修复 |
| `PERF-13` | Open | 核对并收敛双 rustls 密码学后端，先验证兼容性和体积收益 |
| `PERF-16` | Open | 消除路径全集物化及无界快照；当前路径扫描已移到创建事务之前，不能沿用旧的“持事务扫描两次”描述 |
| `PERF-09` | Needs benchmark | 比较 release `s`／`3` 的体积、启动和真实热点吞吐，不能无数据改配置 |
| `FUN-19` | Boundary | 云盘、视频嗅探、云同步、插件协议、正式自动化 API 等属产品取舍 |

尤其不应再把已关闭的 `ARC-32` 死锁、`ARC-33` HTTP flush、`ARC-37` 僵尸状态、`ARC-38` staging 清理，以及历史 i18n／探测代理问题写成当前仍未修复。9 月 11 日的 [旧现状报告](project-current-improvement-review.md) 和主审计中的历史回顾部分保留了当时结论；它们不能覆盖最新条目状态与当前代码。

### 2.2 本轮实际执行的检查

| 检查 | 结果 | 可证明范围 |
| --- | --- | --- |
| `pnpm typecheck` | 通过 | 当前 TypeScript 类型检查 |
| `pnpm lint` | 通过，239 个文件 | 当前 Biome 检查；未执行自动修复 |
| `pnpm test:frontend` | 59 个文件、294 项通过 | 仓库已有前端测试；不含下述临时复现实验 |
| `pnpm check:i18n` | 通过 | 7 locale 的键、值规则、占位符与复数检查；不证明母语自然度 |
| `pnpm build` | 通过 | 当前生产前端构建；不是 Tauri 安装包构建 |
| `pnpm check:bundle` | 通过 | initial shell JS gzip 295.6 kB／预算 340.0 kB；CSS gzip 15.5 kB／18.0 kB，采用脚本显示口径 |
| `pnpm check:docs` | 通过 | 已实现的版本／ID／状态叙述规则；仍存在规则之外的文档漂移 |
| `pnpm test:release-tools` | 两组分别 61／61、18／18 通过 | 发布和检查脚本测试，不是实际发布演练 |
| `pnpm verify:extensions` | 通过 | 构建并验证 dev profile 的 4 个变体，capture=false；不是商店／release 实装验收 |
| `pnpm verify:protocol-matrix` | 通过 | 矩阵声明和测试引用格式检查；本命令不会重新执行所有协议下载 |
| `node scripts/sync-version.mjs --check` | 通过 | 当前版本源均为 0.5.0 |
| 3 项临时组件复现实验 | 全部观察到目标缺陷 | 清空 URL 后旧结果回填、换 URL 复用旧自动文件名、批量第六条失败隐藏且输入清空 |
| 限速器生命周期 Rust 探针 | 复现后台持有引用 | 直接引用当前 `speed.rs`；释放业务 Arc 后仍存活，清除 limit 后释放 |
| 实体缓存状态探针 | 复现只合并不淘汰 | 连续替换 10 组各 100 个任务后，可见 100、缓存 1000；清空可见页后缓存仍为 1000 |
| Windows rename 最小实验 | 确认覆盖语义 | 目标在检查后出现时，普通 rename 可替换目标；完整提交函数竞态还需故障注入 |
| 旧密文格式独立 fixture | 确认格式判定碰撞 | 合成的有效旧密文首字节为 0x01，按当前格式分支处理会认证失败；不是应用用户库实测 |
| impeccable 静态检测器 | 对实际组件与 CSS 文件未报命中 | 只能说明该检测器没有报告；不能据此认定无交互或可访问性问题 |

前端临时复现文件已移出源码目录，原有测试与业务代码未改。复现实验与检查日志留在本机临时目录；关键现象、条件和验收要求已写入本文，长期回归证据应在修复时进入正式测试集。本次没有运行 `pnpm verify` 全链路、Rust 全量测试、Clippy、bindings 导出或安装包验证，因此不宣称“全门禁通过”。

## 3. 用户交互便捷性

### 3.1 应保留的交互基础

应保留密集列表、低噪声色彩、可展开诊断、键盘与鼠标并行入口。当前已经有新手引导、启动失败页、错误边界、设置搜索、撤销删除、队列原因、恢复中心、快捷键、响应式布局、reduced-motion 处理，以及真实组件交互测试。改进不需要把主界面重做成营销式卡片仪表盘。

### R26-U01｜P2｜URL 变化没有立即隔离旧探测，自动文件名也没有随资源重置

**证据：实验确认。** [NewDownloadDialog.tsx](../src/components/shell/NewDownloadDialog.tsx) 的 506–533 行仅在真正开始下一次 `detect()` 时增加 request ID；580–599 行有 650ms 防抖；934–940 行修改 URL 时清空结果却不立即废弃旧请求。522–524 行只在文件名为空时写入探测名称。

**两个可重复场景：** A 的探测开始后清空输入，再让 A 返回，空 URL 下会重新出现 A 的信息；A 探测得到 `old.zip` 后输入 B，B 探测完成并开始下载，创建参数仍是 `url=B, fileName=old.zip`，而 snapshot 中已是 `new.zip`。既有 stale-response 测试只覆盖“B 已开始探测之后 A 才返回”，没有覆盖防抖窗口与清空输入。

**影响：** 用户看到的资源信息与当前输入不同，新的资源使用旧扩展名／名称。创建函数已有 `probeUrl === currentUrl` 防线，因此不能把这个问题扩大为“必然下载错 URL”。

**改进：** URL、凭据、代理、目录上下文变更时立即递增会话代数；响应同时核对代数和输入快照。区分“自动生成文件名”与“用户手动改名”，仅保留后者。目录探测采用相同的结果隔离方式。

**验收：** 使用可控 Promise 覆盖清空 URL、防抖期间旧响应、连续三次换 URL、认证／代理变更和目录探测乱序；自动名称跟随新资源，人工名称按明确规则保留。

### R26-U02｜P1｜批量部分成功后丢失重试输入，后续失败明细不可达

**证据：实验确认。** `NewDownloadDialog.tsx:798–811` 在任意 `createdCount > 0` 时清空整个输入；1973 行固定 `result.items.slice(0, 5)`，没有展开、分页或失败项导出入口。临时组件实验使用 6 条结果、5 成功和第 6 条失败，确认输入变空且失败原因不出现在界面。

**影响：** 大批量导入最需要处理的失败项可能恰好被隐藏。总数能显示“失败 1 条”，但用户不能直接确认是哪条、修正或仅重试失败集合；需要返回原始文本重新整理。

**改进：** 以成功／重复／失败分组保留完整结果；默认展示失败，支持逐条编辑、复制失败 URL、仅重试失败和导出报告。只从编辑区移除成功项，或者保留只读原始批次。结果和输入绑定批次 ID，避免再次预览覆盖创建结果。

**验收：** 100 条导入，第 6、50、100 条分别失败，全部可查、可导出、可单独重试；成功项不会重复创建；重新预览不能丢失上一批尚未处理的失败记录。

### R26-U03｜P2｜批量预览／创建没有一致的请求互斥和结果版本

**证据：代码确认。** `NewDownloadDialog.tsx:801` 使用 `setSubmitting(create)`，预览时为 false；1478–1498 行两个按钮仅依据 `submitting` 禁用。预览可以重复发起，也可以与创建并行；两者共享 `batchResult` 和 `finally setSubmitting(false)`，没有批次令牌。

**影响：** 慢预览可覆盖较新的创建结果；先完成的预览可能在创建仍进行时解除按钮禁用。此处不能据此断言数据库必然重复创建，后端仍有去重规则；确定的问题是界面状态与进行中的操作不一致。

**改进与验收：** 分离 preview／create 状态，绑定输入版本和请求代数，明确允许的并发关系；预览慢于创建、两次预览乱序和处理中编辑输入时，旧结果不得覆盖新结果，创建期间不应被无关请求解除锁定。添加导入进度和可取消能力时，应让取消真实传递到后端。

### R26-U04｜P2｜普通业务失败仍可能以原始英文或错误载荷出现在批量结果

**证据：代码确认。** [create.rs](../src-tauri/src/commands/tasks/create.rs) 的 411、504、522 行生成 `Task already exists`、`URL is invalid`、`Duplicate URL in this import` 等英文；598、652 行把错误直接放入 `error_message`。`NewDownloadDialog.tsx:1984` 直接渲染 `item.errorMessage`。

**影响：** 主流程已有稳定错误码本地化，但批量结果是另一条用户可见出口，可能显示英文甚至结构化错误文本。现有 i18n 检查不会自动发现 Rust 预渲染的英文。这是该出口的独立残留，不重开历史 `UX-11`／`FUN-21`。

**改进与验收：** `BatchImportItem` 使用稳定 outcome／error code 与 params，由前端映射本地化；原始错误仅放进可展开诊断。无效 URL、批内重复、已有任务、认证失败、超时在全部 locale 都显示对应文案。Rust 模型变更后生成 Specta bindings，并测试数据映射表的全部 locale。

### R26-U05｜P2｜新建草稿关闭即销毁，进行中操作的关闭语义也不够明确

**证据：代码确认。** [AppShell.tsx](../src/components/shell/AppShell.tsx) 1517–1537 行按 `newDownloadOpen` 条件挂载，关闭即卸载。草稿全部在 `NewDownloadDialog` 的局部 state；`onDraftStateChange` 用于外部链接冲突判断，没有形成关闭保护。Dialog 根节点直接使用 `onOpenChange`，虽禁用页脚取消按钮，未同步拦截 Escape／遮罩关闭。

**影响：** 编辑复杂认证、代理、哈希或批量内容时，误按 Escape 会丢失输入；提交中的窗口若通过其他关闭路径退出，命令可能继续执行，用户不清楚是否取消成功。

**改进：** 优先在本次应用会话内保留非敏感草稿，避免把密码／私钥明文写入 localStorage。关闭正在提交的窗口时明确区分“隐藏窗口”“取消请求”和“放弃草稿”；让后台进度仍有可见入口。

**验收：** 填写复杂草稿后 Escape、点击遮罩、窗口切换和浏览器交接均不会无说明丢失工作；取消行为有明确结果；机密不会出现在草稿持久化、诊断或日志中。

### R26-U06｜P2｜“全选已加载”诚实但仍不足以处理大规模筛选结果

**证据：改进机会。** `AppShell.tsx:1324–1337` 明确只选当前 cursor 已加载 ID，并用 Toast 告知范围。这是正确防误导的实现，不是已修复的 `UX-05` 全局暂停／恢复问题复发。

**不足与改进：** 用户筛选出数千条失败任务后，仍需要逐页加载才能针对筛选全集操作。增加“选择全部匹配项”的服务端查询令牌，提交前显示范围与数量；对删除文件等危险动作继续保留强确认。

**验收：** 1 万条数据下仅加载 100 条，也能选择准确的筛选全集；后续新任务、状态变化和排除个别条目都有明确定义，不能悄悄扩大危险操作集合。

### R26-U07｜P2｜分类和站点规则已有试运行，但创建现场缺少决策解释

**证据：能力边界。** [ClassificationRulesEditor.tsx](../src/components/settings/ClassificationRulesEditor.tsx)、[SiteRulesEditor.tsx](../src/components/settings/SiteRulesEditor.tsx) 已有试运行／规则管理；[ROADMAP](ROADMAP.md) 的 Later Product Scope 仍明确延期新建对话框分类实时预览、动态子目录模板和运行时命中遥测。

**影响：** 用户需要离开新建流程去设置里试算，难以判断最终目录来自默认值、分类规则还是手动覆盖；规则冲突排查依赖记忆。

**改进与验收：** 新建窗口显示“最终目录＋命中规则＋手动覆盖状态”，后端返回实际解析结果；URL、MIME、文件名、规则顺序和手动目录变化时，预览与创建后的真实路径一致。运行记录保存必要的规则 ID 和决策原因即可，不复制凭据或完整敏感 URL。

### R26-U08｜P2｜可访问性、缩放与多语言质量缺少完整桌面验收

**证据：验收缺口。** 现有 TaskRow、Queue／Attention、Dialog、Settings 等已有 ARIA、键盘和组件测试；不能再写成“没有可访问性支持”。但本轮没有取得真实 WebView 的 NVDA／系统读屏、200% 缩放、跨主题对比度、长语言布局和多显示器 DPI 证据。

**改进：** 以新手、批量管理者和键盘／读屏用户三条任务旅程验收。对主要操作验证焦点进入、关闭后返回、错误播报、虚拟列表位置、正在下载状态的更新频率；对 en／zh-CN 和至少一个长文本 Beta locale 检查布局，其他 locale 做相应回归。

**验收：** 在实际支持的桌面最小窗口尺寸、100%／150%／200% 缩放、亮暗主题和 reduced-motion 下完成创建、失败恢复、批量操作与设置修改；对比度需测量，不能凭 OKLCH token 或截图印象宣布合规。点击目标按项目桌面约束检查，不机械套用手机页面规则。非英文文案仍需母语审阅，键齐全不等于翻译成熟。

### R26-U09｜P3｜快捷键帮助与实际可用操作尚未完全对齐

**证据：代码确认／增强。** [ShortcutPanel.tsx](../src/components/shell/ShortcutPanel.tsx) 的说明列表没有列出 `AppShell.tsx` 已实现的 Ctrl／Cmd+F、Alt+上下重排、Shift+Delete 等入口。大部分核心键已有文档，问题是两份定义需要手动保持一致。

**改进与验收：** 从一个类型化命令表生成键位匹配、帮助说明和命令面板提示；区分平台以及输入框／弹窗中的可用性。检查每个公开快捷键均能在帮助中找到，并可通过正常鼠标入口执行同一动作。自定义键位可后置，不应先于 U01–U05。

### 3.2 界面质量判断的限度

使用 impeccable 的五个维度检查了可访问性、效率、主题、响应式和实现一致性。源码层面可以确认已有语义颜色、reduced-motion、虚拟列表和组件基础；创建／批量流程仍存在上述具体问题。没有实机视觉证据时，不提供貌似精确的总分，也不宣称某主题对比度失败或已完全合规。修复宜以 `impeccable harden`／`clarify` 收敛异常交互，再用 `audit`／`adapt` 验证目标桌面尺寸，最后 `polish`，不需要先重设视觉语言。


## 4. 程序功能丰富性和完整性

### 4.1 能力与成熟度矩阵

| 协议/领域 | 已实现且应保留 | 当前真实局限/改进机会 | 验收证据边界 |
| --- | --- | --- | --- |
| HTTP/HTTPS | HEAD/Range fallback、未知长度与分段下载、远端validator重验、动态分段、全局/任务限速、认证、代理、校验与恢复 | 直接输入的无后缀媒体/清单链接按HTTP文件处理；Basic Auth凭据修复中心不覆盖HTTP | 最成熟路径；仍需真实代理/CDN、三平台安装包与网络切换测试 |
| FTP/FTPS | 单文件、动态并行最多4连接、密码加密、目录列表、SOCKS5、远端SIZE/MDTM重验 | 普通文件probe没有消费草稿凭据；目录列表不是递归下载；implicit FTPS+SOCKS5明确拒绝 | 本地假FTP及目录认证/代理测试存在；不可推导所有公网FTP实现兼容 |
| SFTP | 密码/OpenSSH私钥、TOFU、known-host修复、本地temp续传、SOCKS5；源码还含最多2连接动态分段 | create二次probe丢草稿凭据；目录递归缺失 | 本地真实SSH fixture与字节级恢复已有；外部服务器和真实多任务长期验收未完成 |
| BitTorrent | magnet、远程/本地torrent、多文件选择、piece/peer/DHT快照、按任务session、ratio/时长做种限制 | tracker只表示配置而不是实时announce；跨真实session/进程再入及真实限速吞吐需实测 | 矩阵automated包含合同测试；Restart明确只是DB合同，不等于完整跨librqbit会话下载验收 |
| HLS | 多变体、AES-128、init map/byte range、外部音轨/字幕、live轮询、ffmpeg remux | SAMPLE-AES/DRM明确不支持；批量导入没有逐URL媒体选择向导 | 本地fixture、音轨失败可见、冷引擎恢复有证据；真实直播、多CDN来源验证仍需补齐 |
| DASH | static/VOD、单Period、Number/补零模板、受限SegmentList/SegmentBase、staging续传、ffmpeg remux | dynamic/live、Timeline、多Period、Time等变量明确拒绝；SegmentList子元素/Range处理有下述遗漏；无画质/语言选择 | corpus的拒绝合同不能代表全标准覆盖；签名URL身份修复已Closed，不应继续报“必然全量重下” |
| WebDAV | Basic Auth、Depth-1 PROPFIND、HTTP委托、目录草稿认证/代理；目录已共享HTTP client cache | 目录请求无整体deadline；无递归/企业认证通用支持 | 本地401/403、pause/resume已有；真实WebDAV服务器互通未完成 |
| Metalink4 | 多文件选择、HTTP/HTTPS镜像优先级与failover、part续传保护、strongest-hash、分文件进度 | 资源只支持HTTP/HTTPS；按镜像独立凭据作用域是后续产品项 | 本地镜像切换/校验/冷启动有证据；真实跨域镜像和大文件长期验证仍需补齐 |
| 浏览器 | 手动HTTP/HTTPS交接、Native Messaging/WebSocket、去重、过期header恢复、设置中的集成诊断 | candidate/release无自动捕获/Cookie转发；dev能力需显式开关；Safari/store/signing尚未完成 | profile权限/行为有自动化证据；商店包与真实安装/升级仍需人工验收 |
| 队列/恢复/备份/规则 | 全局批量暂停/恢复、优先级/排序、计划窗口、完成动作；恢复记录、子集恢复、迁移路径重映射、规则try-run | 批量入口边界仍不完整；备份密文绑定本机、无下载文件载荷；规则运行时命中解释/动态模板未做 | 存在对应单元/集成测试，不能再声称功能不存在；跨机和实际灾难恢复需单独演练 |

矩阵证据入口：[docs/protocol-reliability-matrix.md:7](../docs/protocol-reliability-matrix.md#L7)、[src-tauri/src/download/sftp.rs:57](../src-tauri/src/download/sftp.rs#L57)、[src-tauri/src/download/sftp.rs:237](../src-tauri/src/download/sftp.rs#L237)、[docs/ROADMAP.md:25](../docs/ROADMAP.md#L25)。

### R26-F01｜P1：文件探测与创建入口没有一致传递FTP/SFTP草稿凭据

- 类型/关联：源码确认的新候选；关联FUN-04（Closed，目录探测）、FUN-17（Closed，共享draft字段），不是重新报告这两条的原始问题。
- 证据：[src-tauri/src/download/ftp.rs:305](../src-tauri/src/download/ftp.rs#L305) 从URI建立FtpTarget后直接调用probe_target，不读取ProbeRequest.credentials；`ftp.rs:1727` 在URI未带用户时使用anonymous。相对地，目录路径`ftp.rs:240`明确消费草稿凭据。[src-tauri/src/commands/tasks/create.rs:199](../src-tauri/src/commands/tasks/create.rs#L199)的resolve_create_probe不接收credentials，`:230`固定credentials=None、task_id=None。批量先在`:563`传入batch_credentials，随后`:626`又以probe_snapshot=None创建；SFTP在[src-tauri/src/download/sftp.rs:325](../src-tauri/src/download/sftp.rs#L325)消费request.credentials，但该次已丢失，`:1383`空用户直接拒绝。单任务已有snapshot仅在create.rs:271的5分钟期限内复用。
- 触发/影响：FTP URL无嵌入口令、用户在安全凭据栏填写正确用户名密码时，普通文件探测仍可能匿名登录失败；SFTP批量预览使用私钥/密码成功，真正创建时二次探测失败；单任务编辑停留超过5分钟后也会重新暴露。用户可能被迫重新建立任务或改用嵌入凭据URL。
- 建议：创建、单次probe、批量probe、目录probe共用同一经过解析的credential+proxy合同；FTP文件probe补齐显式凭据，再由统一resolve_create_probe将凭据交给各引擎。保留加密落库和URL清洗边界。
- 验收：本地禁止匿名FTP用纯URL+独立凭据成功；SFTP密码与私钥在无snapshot、过期snapshot、批量preview→create四条路径都成功；错误凭据明确失败；返回URL/诊断无明文凭据。

### R26-F02｜P2：批量允许重复参数存在，实际被前置去重短路

- 类型/关联：源码确认；FUN-17已Closed，但该参数的业务语义未贯通。
- 证据：[src-tauri/src/commands/tasks/create.rs:583](../src-tauri/src/commands/tasks/create.rs#L583)、`:604`发现既有任务后无条件continue；允许重复仅在`:628`传到后续创建。单任务在`:727`正确受allow_duplicate门控。[src/components/shell/NewDownloadDialog.tsx:803](../src/components/shell/NewDownloadDialog.tsx#L803)还固定allowDuplicate=false，批量UI没有对应开关；[src/lib/create-draft.test.ts:55](../src/lib/create-draft.test.ts#L55)只验证参数映射。
- 触发/影响：IPC调用import_urls并传allowDuplicate=true仍不能再次导入已有链接；普通用户也无法在批量流程中执行单任务已有的允许重复创建选择。当前批量 UI 没有此开关。
- 建议：明确区分输入内重复行与数据库已有任务，预览显示冲突来源；创建阶段按显式策略执行，同步提供UI入口。
- 验收：probe开/关、create开/关、allowDuplicate开/关的表驱动合同测试；允许重复生成独立task及无冲突最终文件路径，拒绝重复不改变既有任务。

### R26-F03｜P2：DASH SegmentList遗漏初始化子元素及媒体字节范围

- 类型/关联：源码确认遗漏，运行时成品影响待fixture复现；关联FUN-12（Closed，其已拒绝的dynamic/Timeline/multi-Period不是本条）。
- 证据：[src-tauri/src/download/dash.rs:438](../src-tauri/src/download/dash.rs#L438)从SegmentList的initialization属性读取初始化文件；`:450`、`:478`的Initialization分支只读range而不读取sourceURL；`ListSegment`在`:284`只有uri；`:473`的SegmentURL仅读media，`:875`生成List计划时byte_range固定None。现有测试[src-tauri/tests/dash_engine.rs:34](../src-tauri/tests/dash_engine.rs#L34)和`dash.rs:2236`也使用SegmentList initialization属性，未覆盖Initialization sourceURL和SegmentURL mediaRange。
- 触发/影响：带独立init文件的SegmentList会遗漏init请求，可能直到 remux 阶段才失败；多个SegmentURL引用同一文件的不同mediaRange时会全量重复下载该资源。未执行成品复现，不应直接断言所有此类清单均发布损坏文件。
- 建议：准确解析这两种声明并将range传到下载计划；当前不实现的语法必须在probe明确拒绝，避免接受后才失败。
- 验收：增加真实形状的SegmentList+Initialization/sourceURL和同文件多mediaRange fixture；断言服务端收到精确初始化/Range请求、成品时长及解码成功；不支持形状在下载前给稳定码。

### R26-F04｜P2：目录探测仍缺完整请求预算与取消闭环

- 类型/关联：源码确认的新合同遗漏；ARC-26 Closed修复了建连预算，不应等同已覆盖整个目录操作。
- 证据：[src-tauri/src/commands/tasks.rs:283](../src-tauri/src/commands/tasks.rs#L283)~309直调WebDAV目录函数；[src-tauri/src/download/webdav.rs:212](../src-tauri/src/download/webdav.rs#L212)等待send无deadline，`:220`仅对响应body提供idle timeout且cancel=None。共享[src-tauri/src/download/net_factory.rs:167](../src-tauri/src/download/net_factory.rs#L167)~178只设置connect_timeout(30s)。FTP目录`ftp.rs:252`后CWD/PWD/MLSD/LIST、SFTP目录读列表也应纳入同一合同检查。
- 触发/影响：服务端连接成功但不返回响应头，或持续缓慢发送数据，目录探测可长期占据新建窗口；关闭窗口不等于后端停止请求。
- 建议：目录probe使用统一总deadline和request-id取消；并对send、认证后控制命令、body/listing读完阶段明确预算。
- 验收：连接后不发headers、周期滴流、FTP LIST/SFTP read_dir停滞、关闭弹窗取消四类服务器fixture；有限时间返回稳定码，连接/后台future收敛。确认取消和timeout不是同一状态。

### R26-F05｜P2：媒体/清单引擎只按URL后缀路由

- 类型/关联：源码确认的兼容性边界，建议产品增强；不同于完整网页视频嗅探。
- 证据：[src-tauri/src/download/url_classify.rs:6](../src-tauri/src/download/url_classify.rs#L6)、`:15`、`:25`、`:33`分别按torrent/meta4/m3u8/mpd路径后缀匹配；[src-tauri/src/download/engine.rs:246](../src-tauri/src/download/engine.rs#L246)先做后缀路由后回退scheme。HTTP probe返回content_type后未重新选择引擎。
- 触发/影响：`https://cdn.example/play?id=...`返回m3u8/MPD，或不带torrent后缀的下载endpoint，会被当普通HTTP文件保存清单，用户无法达到下载媒体/种子内容的目的。
- 建议：对用户直接输入链接增加受限的Content-Type/少量内容识别，并提供显式类型覆盖；继续使用同一认证、代理、SSRF与控制体积合同，不引入网页嗅探权限。
- 验收：无后缀HLS/DASH/torrent/Metalink、重定向后的类型、错误Content-Type和普通文本fixture；路由可解释且不会将普通文件错误执行为媒体任务。

### R26-F06｜P2：恢复中心凭据修复没有覆盖HTTP派生引擎

- 类型/关联：功能完整性增强；FUN-01、FUN-03均Closed，HTTP下载认证和浏览器header恢复本身不应被重报。
- 证据：[src-tauri/src/commands/recovery.rs:23](../src-tauri/src/commands/recovery.rs#L23)的CREDENTIAL_PROTOCOLS仅FTP/FTPS/SFTP/WebDAV/WebDAVS，`:75`拒绝其他协议；[src/components/workspaces/RecoveryCenter.tsx:53](../src/components/workspaces/RecoveryCenter.tsx#L53)同步限制入口。
- 触发/影响：用户直接用Basic Auth创建HTTP/HLS/DASH/Metalink后口令变化，虽然底层消费已加密凭据，恢复中心却不能原地更新。浏览器重新发送只解决特定header恢复合同，不能覆盖所有直接创建的Basic Auth任务。
- 建议：按认证机制而非协议白名单定义修复能力；提供同源作用域说明，更新后重试保留已有字节；镜像凭据范围应显式建模。
- 验收：HTTP/HLS/DASH/Metalink口令轮换后同task恢复，不泄漏跨源凭据；活跃下载禁止半途替换；历史操作记录及持久化仍加密。

### R26-F07｜P2：DASH画质、语言与字幕选择缺少明确控制

- 类型/关联：有意受限引擎上的增强，非FUN-12 Closed回归。
- 证据：[src-tauri/src/download/dash.rs:244](../src-tauri/src/download/dash.rs#L244)的AdaptationSet未保存语言/角色；`:701`select_tracks按每个AdaptationSet最高bandwidth选择，`:725`/`:727`后一个同类AdaptationSet覆盖前一个，最终只有一个video和一个audio。[src-tauri/src/commands/tasks/create.rs:57](../src-tauri/src/commands/tasks/create.rs#L57)仅有HLS选择字段。HLS用户选择已实装，不应扩写为整个媒体系统都不支持。
- 触发/影响：多语言MPD可能选中用户不理解的音轨；弱网/流量受限用户不能主动降画质；不能自主选择字幕，或从同时包含音视频的清单中仅下载音轨。
- 建议：先在probe展示最终自动选择及原因，再以显式Representation/language/role选择扩展DASH；避免直接扩张到live/DRM。
- 验收：至少两种语言、不同带宽、多AdaptationSet、音频单独、字幕有/无fixture；默认选择稳定且可见，用户选择在恢复后不变。

### R26-F08｜P3：目录列表不能转化为完整目录下载计划

- 类型/关联：Boundary/产品增强；FUN-04目录凭据修复保持Closed。
- 证据：[src-tauri/src/download/sftp.rs:214](../src-tauri/src/download/sftp.rs#L214)目录作为文件拒绝；[src-tauri/src/download/webdav.rs:198](../src-tauri/src/download/webdav.rs#L198)固定Depth=1；[src/components/shell/NewDownloadDialog.tsx:1027](../src/components/shell/NewDownloadDialog.tsx#L1027)根据probableFileUrl区分选单个文件和目录项。当前只提供选文件的入口。
- 触发/影响：下载软件发行目录、研究数据集、多层资源包需要手动多次列目录与建任务，易漏文件。
- 建议：先增加当前目录多选批量创建，再做可选递归扫描；显示文件数/大小/未知项预览、保留相对结构，设置深度/数量上限并处理符号链接循环。
- 验收：嵌套目录、重名、无权限子目录、符号链接循环、取消扫描、部分创建失败和恢复；安全路径校验与凭据/代理全程一致。

### R26-F09｜P3：BT诊断能显示配置，尚不能解释连接失败的动态原因

- 类型/关联：明确延期功能；FUN-15 Closed已完成configured-only诚实展示。
- 证据：[src-tauri/src/download/bt.rs:1468](../src-tauri/src/download/bt.rs#L1468)的configured_tracker_status固定status/source为configured、last_error=None；[docs/ROADMAP.md:291](../docs/ROADMAP.md#L291)明确延期BT live tracker。
- 触发/影响：没有下载速度时，用户只能知道“配置了哪些tracker”，不能区分tracker不可达、announce拒绝、未发现peers或已连接但无数据。
- 建议：增加上次/下次announce、HTTP/UDP错误与重试、DHT/peer发现阶段；继续对未知seed数量显示未知，不伪造健康状态。
- 验收：成功/拒绝/timeout tracker、本地peer、无peer、代理失败场景；状态有来源和更新时间，停止任务后无陈旧实时状态。

### R26-F10｜P2：浏览器集成中心的新增后端尚未贯通用户入口

- **类型：当前工作区集成未完成，不判定为已发布功能回归。** 现有设置页浏览器安装诊断和手动交接仍可用。
- **证据：** [src-tauri/src/commands/browser.rs:196](../src-tauri/src/commands/browser.rs#L196) 新增交接历史，`:265` 新增安全边界试运行，`:275` 新增过期认证任务，`:288` 新增 native-host 自检；[src-tauri/src/lib.rs:287](../src-tauri/src/lib.rs#L287) 起已注册相关命令。但当前 [src/generated/bindings.ts](../src/generated/bindings.ts)、[src/lib/tauri.ts](../src/lib/tauri.ts)、[src/lib/tauri-browser.ts](../src/lib/tauri-browser.ts) 和组件中没有这些新命令的对应生成类型／调用链，也没有 Browser Center 页面。不能因后端和测试文件已经出现就写成中心已完整交付。
- **影响与建议：** 交接失败历史、自检和过期认证任务仍不能作为完整用户工作流使用。先完成 Specta 生成、前端适配和浏览器 mock，再把入口放进现有集成设置或独立中心；明确安全试运行并不代表真实扩展→Native Messaging→应用传递成功。
- **验收：** `pnpm specta`、`pnpm check:bindings` 通过；真实 Tauri 与浏览器预览都有加载／错误／空状态；历史窗口与累计统计范围标明；试运行不创建任务；真实扩展发送另有端到端证据。工作区本轮未执行 bindings 检查，不把源码差异描述成该命令已实跑失败。

### R26-F11｜P3：备份适合数据库恢复，跨机凭据与下载实体迁移仍有边界

- 类型/关联：设计取舍与验收增强，非备份缺失；FUN-16、FUN-23、FUN-26 Closed保持。
- 证据：[src-tauri/src/db/backup.rs:18](../src-tauri/src/db/backup.rs#L18)固定machine_bound_ciphertext；[src-tauri/src/commands/backup.rs:97](../src-tauri/src/commands/backup.rs#L97)~98导出不含global proxy password；`:276`已有显式路径remap，`:456`已有post-restore报告；[src-tauri/src/models/backup.rs:94](../src-tauri/src/models/backup.rs#L94)描述恢复后重配报告。
- 触发/影响：新机器导入后需重新提供密钥绑定凭据、外部工具/代理配置，并另行迁移最终文件和temp/staging；数据库备份成功不能推导文件实体完整或断点可用。
- 建议：保留本机密钥安全策略，在迁移验收中明确数据包与实体文件关系；提供缺失文件映射/批量凭据重配向导。可移植加密包仅在有明确需求后设计独立口令与安全审查。
- 验收：两台真实机器（含不同盘符）备份→复制实体→remap→重配→恢复，逐项显示需重配内容；错误凭据不导致原记录/已下载字节丢失；备份不泄漏明文密钥。

### R26-F12｜P1（稳定发布验收）：协议与浏览器的自动化证据尚不能替代真实发行链路

- 类型/关联：外部验收空白；FUN-18 Closed只证明其本地合同，不是重新打开；FUN-13 Closed的release权限边界正确。
- 证据：[docs/protocol-reliability-matrix.md:11](../docs/protocol-reliability-matrix.md#L11)明确BT Restart是DB合同、跨session非C5门槛；文末要求真实服务/媒体源人工记录。[docs/ROADMAP.md:37](../docs/ROADMAP.md#L37)明确仍有三平台installer smoke、store identity、OS signing、GUI E2E。[README.md:85](../README.md#L85)~97说明release/candidate手动交接，实验capture/header仅dev开启。
- 触发/影响：本地fake server通过不验证真实公网代理、FTP服务器差异、NAS WebDAV、BT tracker/peer生态、动态CDN链接、原生权限和升级安装；开发扩展正常不等于商店发布包完整可用。
- 建议：发布候选固定版本/commit/安装包hash，建立Windows/macOS/Linux安装→首次下载→中断恢复→升级→卸载矩阵，记录协议对端版本/网络/证据与未支持项；商店身份、Native Messaging注册、签名独立验收。没有必要为丰富性强行把自动捕获移入release。
- 验收：真实HTTP/CDN、FTP/FTPS、OpenSSH、BT双任务/恢复/限速、HLS直播、DASH、NAS WebDAV、跨域Metalink均有脱敏记录；三平台installer/updater流程可复现，已知Boundary与包内行为一致。


## 5. 项目架构的鲁棒性和稳定性

已有 SQLite 状态门、任务监督与取消、SSRF 前置审查、TOFU、凭据加密、staging 清理和错误恢复应继续保留。下面集中检查这些保护之外的当前路径；尤其不把普通引擎 panic 兜底误认为覆盖整个命令和派发过程。

### R26-A01（P1，当前代码确认 + Windows 文件原语实证）：最终提交尚未具备原子拒绝覆盖语义

- **触发链**：最终目标路径检查时不存在 → 外部程序在检查之后创建同名文件 → 下载器调用普通 rename → 外部文件被替换。DB 唯一索引只能协调本应用的预留，不能锁住文件系统中的其他程序。
- **源码证据**：src-tauri/src/download/file_ops.rs:30 的 try_exists 与 :37 的 tokio::fs::rename 分离；跨卷分支 :95 与 :103 同样如此。Tokio 锁文件版本 1.52.3 的 fs/rename.rs:6 明确写替换存在目标，:16 实际调用 std::fs::rename。HTTP/FTP/SFTP/HLS/DASH/Metalink 都调用此共享函数。当前 dirty diff 只新增测试，生产实现不是本轮未提交修改新引入。
- **本轮实证**：在系统临时目录用独立 rustc 小程序执行“检查目标不存在 → 写入外部目标内容 external-file → rename 下载文件”后输出 target_after_rename=download。Windows 上也不会自然以 AlreadyExists 拒绝此替换。探针不涉及用户文件，也未执行真实下载函数的并发集成测试；Linux/macOS 本轮未运行。
- **现有防护与关系**：已有路径预留、预先存在文件冲突错误、同名任务并发测试均有效。当前 file_ops.rs 的新增测试只覆盖检查前已存在的文件。既有已列测试未覆盖检查与发布之间的外部文件竞争；本轮以新候选记录，不修改 ARC-02 的 Closed 历史。
- **改进**：共享提交层使用 OS 支持的原子 no-replace rename，或经验证的 create-new / hard-link 提交策略；跨卷先在目标目录独占创建 staging，完成写入与同步后再原子 no-replace 发布。保留原下载临时文件直至成功。
- **验收**：在检查与发布之间注入可控 barrier，由独立进程创建目标，三平台验证外部文件字节不变、任务返回 final_path_conflict、原临时文件可恢复；同卷与跨卷分支都覆盖。

### R26-A02（P1，静态已确认；未做完整恢复复现）：持久化 nonce 长度错误可触发 panic

- **触发链**：本地 DB 损坏或恢复的备份含合法 Base64、但长度不是 12 字节的 nonce → 本机密钥可读取、ciphertext 可 Base64 解码且进入对应解密路径 → 固定长度数组转换 panic，绕过 Result 错误恢复路径。无需网络攻击成立。
- **证据**：src-tauri/src/secure_headers.rs:60-65 仅 Base64 decode，直接 Nonce::from_slice；锁定依赖 generic-array 0.14.7 的 from_slice 明确要求长度完全一致，否则 panic。src-tauri/src/db/migrations/001_init.sql:142、:172、:183 的 nonce 仅 TEXT，无长度 CHECK。db/backup.rs:281 的验证只有 integrity_check、schema migrations；commands/backup.rs:329 校验路径；db/backup.rs:914 的 scrub 只重设 settings，未验证 nonce。因此有效 SQLite 备份中一字节 nonce 不会被现有语义门禁拒绝。
- **影响范围**：headers 与 task_proxy 解密还出现在 scheduler/mod.rs:320 附近的派发预留阶段，早于 :509 的 engine catch_unwind；不能把引擎 panic 兜底视为整条派发路径兜底。不据此断言整个桌面进程必然崩溃，具体表现需 Tauri / Tokio 集成测定。
- **已有防护**：Base64 格式错误、AEAD 认证失败与 keyring 不可用均可返回错误；SEC-08 的“不因读取失败覆盖密钥”已修。
- **改进**：使用 fallible 长度转换；校验 ciphertext 最小 tag/版本长度，返回稳定错误码和重新输入凭据动作；恢复预检对密文元数据做结构校验，不要求异机解密。
- **验收**：0/1/11/12/13 字节 nonce、截断 tag、无效 Base64 均无 panic；headers/proxy/credentials 三入口返回结构化错误；一个坏任务不能阻断后续任务派发；备份校验能定位对应字段。

### R26-A03（P2，代码确认 + 独立密码学 fixture 实证）：密文版本探测与旧密文首字节冲突

- **触发链**：历史无版本前缀的 ChaCha20-Poly1305 密文首字节恰好为 0x01 → secure_headers.rs:68 把它当成新格式 → 删掉首字节并应用 AAD → 合法历史凭据认证失败。均匀分布密文首字节下概率约为 1/256；本轮未测历史用户库中的实际发生比例。
- **证据**：src-tauri/src/secure_headers.rs:14 声明支持 legacy，:68-77 仅依据 raw.first() 决定格式，命中 v1 失败后不会按真实 legacy 标记处理。此处与 SEC-08 的 keyring 读失败保护是不同问题。
- **实证**：Node 标准 crypto 的独立合成 key/nonce fixture 中，nonce counter=25 生成首字节 0x01 的合法旧密文；按原文解密成功 legacy-credential，按当前分支删首字节后 authentication_failed。没有读取实际密钥或用户凭据；未调用应用 Rust 解密函数。
- **改进**：用 DB 显式格式列或无歧义 envelope，并做一次性迁移；对历史格式提供经过认证的兼容路径，避免随意降级新格式的 AAD 保护。
- **验收**：固定包含首字节 0x01 的旧格式 fixture 必须成功迁移；新格式错误 AAD、跨任务替换、截断密文仍失败；不要用随机测试碰运气覆盖 1/256 分支。

### R26-A04（P1，代码确认；磁盘故障注入待做）：保存新备份会先截断旧备份

- **触发链**：用户覆盖原有有效备份 → File::create 立即清空旧文件 → write_all 遇磁盘满、设备移除或进程退出 → 原有备份已消失，新备份不完整。即使正常返回，也未显式 sync_all 持久化。
- **证据**：src-tauri/src/db/backup.rs:256-277，File::create(path) 后直接 write_all；commands/backup.rs:104 调用该函数。snapshot_database_to_path 的跨卷修复保证中间 SQLite 能导出，但不等同于最终 .vibe-backup 写入具备原子性。
- **已有防护**：备份有 SHA-256、schema/integrity 校验，恢复前有磁盘余量检查与 pre-restore snapshot；能发现坏备份不等于保住覆盖前的好备份。
- **改进**：目标目录独占临时文件 → 完整写入 → flush/sync → 校验 → 原子替换，替换成功前保留旧备份；对 staging 采用可清理的随机命名。同步失败必须使导出失败。
- **验收**：注入第 N 字节写入失败、磁盘满、发布前崩溃，旧备份哈希均不变；成功后再读取与恢复通过；同卷/跨卷均测。关联 FUN-23 的邻接路径，不重报已修复的“导出跨卷必败”。

### R26-A05（P2，确定代码路径，异卷环境复现待做）：恢复 staging 仍要求系统 TEMP 与应用 DB 同卷

- **触发链**：TEMP 在另一块磁盘或另一挂载点 → 备份 materialize 到 TEMP → restore_app_backup 把该文件 rename 到 DB 旁边 pending → EXDEV / ERROR_NOT_SAME_DEVICE，恢复失败。用户选择的备份所在卷不一定相关，关键是 TEMP 与应用数据目录的卷关系。
- **证据**：src-tauri/src/db/backup.rs:298 使用 std::env::temp_dir()；commands/backup.rs:354-364 只有 fs::rename(&verified, &pending)，失败删除 verified 并返回错误。与同文件的 snapshot_database_to_path 已有跨卷 copy fallback 形成不一致。
- **已有防护**：失败发生在替换 live DB 之前；pre-restore backup 已建立，因此不能描述为必然丢失当前数据。
- **改进**：把 verified/staging 创建在 live DB 同目录，或跨卷复制到该目录的独占临时文件、同步与校验后再同卷发布 pending。
- **验收**：TEMP 与 DB 位于不同卷的 Windows/Linux 环境中完整恢复成功，重启后数据正确；复制中断不留下可被 startup 当作成功恢复的 pending 文件。

### R26-A06（P2，代码确认）：ffmpeg 版本探测可无限等待并保留子进程

- **触发链**：设置中的可执行文件响应 -version 时不退出或持续输出 → output().await 一直等并缓存输出 → 设置验证或环境健康检查不返回。下载 remux 的取消治理不能自动覆盖此路径。
- **证据**：src-tauri/src/download/ffmpeg.rs:102-114 没 timeout、取消或 kill_on_drop；commands/ffmpeg.rs:41 直接 await；commands/environment.rs:199 / :417 也直接 await，因此可卡住整份环境检查。相比 ffmpeg.rs:25 的 run_cancellable 已具 kill_on_drop + kill/wait，版本探测是独立路径。
- **改进**：短总超时、输出字节上限；超时/取消 kill 后 wait 回收；设置验证与环境检测复用此实现，返回稳定超时/无效程序错误。
- **验收**：临时测试程序模拟不退出、大量 stdout/stderr、错误码，均在限定时长内返回且无残留 PID；环境检查保留其他已完成项，不无限转圈。

### R26-A07（P1，workflow 静态确认；未触发远程发布）：手动发布 tag 与实际 checkout 不绑定

- **触发链**：workflow_dispatch 选择 main 分支，inputs.tag 填另一既有 tag → actions/checkout 默认检出事件分支 → sync-version 只改版本字符串 → 产物附到输入 tag；标签源码、产物代码和审查对象可能不同。
- **证据**：.github/workflows/release.yml:27 / :70 将 inputs.tag 作为 RELEASE_TAG，但 :37、:79、:132、:165 所有 checkout 都未设置 ref；:47 / :100 仅调用 sync-version。scripts/release-preflight.mjs:9 的检查比较 tag 格式、版本和签名配置，没有核验 git SHA。
- **已有防护**：push tag 的正常路径使用 github.ref；preflight 检查版本、Updater 私钥、最小浏览器权限，均保留价值。问题限定为手动发布输入与源码引用的绑定。
- **改进**：预检解析且验证输入 tag 的 commit，所有 job checkout 同一固定 SHA，拒绝不存在/不一致 tag；产物记录该 SHA。若允许从分支创建新 tag，应明确独立流程，不能由版本字符串暗示一致。
- **验收**：合成 main/tag 指向不同提交的 fixture 或受控 workflow 测试，确认最终构建 SHA 与 tag 一致；所有平台、扩展、资产校验使用同一源提交。

### R26-A08（P1，workflow 静态确认；公开发布风险限定在显式非草稿路径）：公开与验证的先后关系尚未闭合

- **触发链**：手动运行 release_draft=false → publish job 直接建立公开非 prerelease 版本 → 后续 extension build 或 verify-release 失败 → 用户/更新通道可能已看见未完成验证的发布。另 release 只 needs preflight，preflight 不运行完整质量门禁，也不检查 tag commit 的 CI 结果；独立 CI 只监听 main/master push 与 PR。
- **证据**：.github/workflows/release.yml:52 needs preflight；:113 releaseDraft 可为 false；:114 prerelease 固定 false；:119 扩展构建依赖 publish；:156-179 资产验证在 publish 与扩展之后。ci.yml:3-6 未包含 tag push，release-preflight.mjs 只做配置预检。允许 semver -rc tag 而 prerelease=false 的组合也需要明确规则。
- **已有防护**：tag push 和手动默认都建立 draft，正常默认路径风险较低；四平台构建、Updater 签名、资产覆盖及 checksum 验证已存在。并非“项目没有 CI”。
- **改进**：所有产物首先进入 draft；绑定 exact commit 的完整检查成功后执行唯一 publish/promote job；扩展和 checksum 也必须完成。预发布 tag 与 GitHub prerelease 属性一致，生产 latest 仅接受正式版本。
- **验收**：人为让任一平台构建、测试、扩展或资产验证失败，release 保持 draft 且不影响生产 updater；成功后一次性发布；rc 不被公开为 stable/latest。无需在本轮为验证而创建远程 release。


### R26-A09｜P2｜巨大模块与重复协调逻辑继续放大契约漂移

**类型：既有 `ARC-17`／`ARC-31` 的 Partial 收尾。** 当前代码规模统计：

| 组件／模块 | 当前行数 | 更有价值的拆分边界 |
| --- | ---: | --- |
| [src/components/settings/SettingsPage.tsx](../src/components/settings/SettingsPage.tsx) | 2782 | 设置分组、草稿保存状态、字段校验与恢复导航 |
| [src/components/shell/TaskDetails.tsx](../src/components/shell/TaskDetails.tsx) | 2318 | 协议摘要、资源查询、传输配置与诊断列表 |
| [src/components/shell/NewDownloadDialog.tsx](../src/components/shell/NewDownloadDialog.tsx) | 1990 | 草稿状态机、单文件探测、批量导入、协议选择器 |
| [src/components/shell/AppShell.tsx](../src/components/shell/AppShell.tsx) | 1713 | 全局命令、窗口布局、链接交接、任务动作编排 |
| [src-tauri/src/download/ftp.rs](../src-tauri/src/download/ftp.rs)／`sftp.rs` | 1876／1815 | 传输适配、分段协调、持久化检查点 |
| [src-tauri/src/commands/tasks/create.rs](../src-tauri/src/commands/tasks/create.rs) | 1856 | 输入策略、统一探测上下文、路径预留、事务创建 |

**影响：** 行数本身不是错误，但创建窗口的异步状态问题、FTP 文件探测漏凭据、目录请求预算遗漏等，都说明同一契约仍散落在不同入口。前端同时存在数组、ID 列表、实体 map 和统计状态，修改时也需要维护多个不变量。

**改进：** 先补合同测试，再抽纯解析／决策模块与共享上下文，最后迁移协调器；不要把“单文件少于 400 行”当成比正确性更高的目标。FTP／SFTP 的取消排空已经完成，共享协调器是后续去重，不应再次以修复取消为名大改已验收代码。保留协议间确实不同的 flush 失败、远端校验和连接策略。

**验收：** 新建、探测、重试、恢复的认证／代理／取消／超时使用同一可追踪合同；公共协调行为只维护一份且原集成测试不减弱；UI 抽取后键盘、焦点、草稿和请求代数测试保持有效。重构与本轮数据保护修复分批提交，便于审查和回归定位。

### R26-A10｜P2｜测试数据库的退出与清理仍依赖有限重试

**类型：既有 `ENG-03` 的 Partial 收尾。** [tests/common/mod.rs](../src-tauri/tests/common/mod.rs) 183 行起的 `TestDbGuard::drop` 最多同步重试 5 次，然后分离线程重试最多约 60 秒；连接池异步关闭与测试 runtime 退出有竞态。进程可以早于清理线程退出，清理线程也缺少一个由测试框架等待的所有者。

**影响：** 不应再声称“完全没有 Drop 清理”或“仍统一用不受保护的环境变量注入”，这些已经修过。剩余问题是清理结果不可确定，可能遗留 SQLite／WAL／SHM，污染磁盘占用和后续压力测试。主审计曾记录的残留数量是历史实测，本轮没有重跑该统计。

**改进与验收：** fixture 提供显式异步结束路径，先 `pool.close().await` 再清理并汇报失败；用 guard 作为异常兜底。默认测试并行度下连续完整运行后，测试独占临时目录无残留、无后台清理线程。`cargo -j` 控制编译并行度，不能代替测试运行隔离；不要通过关闭并行掩盖根因。

### R26-A11｜P2｜文档语义一致性仍有门禁盲区

**证据：代码与文档确认。** 本轮 `check:docs` 通过，但以下陈述仍可与当前代码或主审计冲突：

| 位置 | 仍存在的旧表述 | 当前应采用的事实 |
| --- | --- | --- |
| [README.md:47](../README.md#L47) | DASH 签名 CDN 续传可能退化为全量重下 | `FUN-25` 已完成签名 URL 身份归一化；不能照旧理由列为未修复，真实远端变化等其他条件另论 |
| [README.md:48](../README.md#L48) | WebDAV 目录探测绕过客户端缓存 | `commands/tasks.rs:303` 起已使用共享客户端；总 deadline 缺口仍存在，见 F04 |
| [README.md:101](../README.md#L101) | 覆盖率度量、macOS Rust CI 等未完成 | 当前已有 coverage 阈值和 macOS Rust job；覆盖率范围与原生 E2E 缺口应另述 |
| [docs/protocol-reliability-matrix.md](../docs/protocol-reliability-matrix.md) 文末 | 把 `FUN-02`／`FUN-07` 当 Open 示例 | 两项在主审计中均为 Closed |
| [docs/performance-baseline-results.md](../docs/performance-baseline-results.md) 顶部与 §3.3 | 顶部称 50k+ 延期，正文已列 50k 查询结果 | 应区分已有 50k DB 查询数据与未完成的 UI／完整矩阵／长跑 |
| [docs/project-current-improvement-review.md](../docs/project-current-improvement-review.md) | 标为“当前”的旧快照列出许多已关闭阻断 | 它是 2026-09-11 快照；后续使用必须与最新主审计对照 |

**影响：** 读者可能重新安排已完成的修复，或误判协议成熟度与公开发布条件。一个只检查关键词和同一行 ID 的门禁不可能充分理解所有散文，因此通过不能代表产品说明完全准确。

**改进与验收：** README 只维护简洁能力边界与当前风险入口；历史报告显式强调日期；高频变化的协议能力／验证状态用结构化数据生成表格。添加上述典型反例测试，同时避免把历史“证据／修复前表现”误报为当前承诺。更新文档不能替代修复 F04 等真实问题。

## 6. 程序运行效率

本节区分已经复现的资源生命周期问题、代码显示的规模成本，以及尚待采集的性能数据。运行效率不仅是峰值下载速度，还包括空闲占用、响应时间、长期资源斜率、磁盘放大和取消后的回收。

### 6.1 已有优化与历史基准的适用范围

- 主列表已有 `@tanstack/react-virtual`，`TaskList.tsx:275` 使用虚拟列表与 `overscan: 6`；任务行按实体订阅，不能再描述为全列表 DOM 同步渲染。
- 后端进度 250 ms 节流、前端 rAF 批处理、零差量快路径、按状态变化生成通知已存在；`PERF-03` 的通知全表扫描修复有效。
- `use-visibility-gated-poll.ts` 已被 QueueCenter 和详情查询使用，详情六类列表已 memo；`PERF-02`/`PERF-14` 的已修路径不应重开。下文剩余轮询问题专指另一个 `useQueueReasons` 调用链。
- HLS key/init-map singleflight、finish Notify、files-version 缓存上限、事件保留窗口及日志轮转已有实现。`PERF-04/05/12/15` 不能再写成不存在。
- `PERF-08` 的单调时钟、公平量子和取消等待已经落地。下文限速器问题是新查出的后台 ticker 生命周期，独立于已验收的限速公平性。
- DB cursor 查询与索引已有历史实测。0.3.0 debug 合成数据、50k 任务搜索 p95=6.90 ms 是有范围的已有证据；不得称为当前 0.5.0 release UI 性能，也不宜因 SQL 使用 LIKE 就直接要求上 FTS。
- 本轮审查实跑构建的 initial shell JS gzip **295.6 kB / 340 kB**、CSS gzip **15.5 kB / 18 kB**，体积门禁通过。包体积门禁存在；交互耗时与持续运行的门禁是另一层。

### R26-P01｜P1｜逐任务限速器在任务结束后遗留永久 ticker（新候选，已最小复现）

**证据**：[src-tauri/src/download/speed.rs:114](../src-tauri/src/download/speed.rs#L114) 的 `ensure_ticker` 在 `:125` 把 `Arc::clone(self)` 放入 spawned task；task 在 `:127` 创建 25 ms interval，只有自身 `limit_bps <= 0` 才退出。limiter 在 `:282` 的 `Drop` 才尝试 stop/abort ticker，但 ticker 的强引用阻止该 Drop 发生。[src-tauri/src/scheduler/mod.rs:486](../src-tauri/src/scheduler/mod.rs#L486) 每次启动调用 `with_parent`，仅 `effective_task_limit > 0` 时创建 child，否则复用全局 parent；`:499` 交给引擎；结束路径没有把这个 child 的 limit 置零或主动停止 ticker。取消传输的 CancellationToken 仅让 `throttle` 返回，不归 ticker 所有。

**触发与影响**：启用逐任务限速，或时间窗限速形成 child 后，首次非零字节 throttle 且未预先取消时启动 ticker；仅启用全局限速不会逐任务新建 ticker；下载完成、暂停/重试或失败后，业务对象释放而 ticker 仍持有 child。每次运行都可多留一个后台任务及其 limiter，并继续约 40 次/秒的 interval tick；这个数字是定时间隔推导，不是测得 CPU 占用。长时间反复使用会使空闲后台活动随历史运行次数增长。

**本轮复现**：在临时目录编译 Rust 探针，直接 `#[path = "D:/Projects/Vibe-Downloader/src-tauri/src/download/speed.rs"] mod speed;` 引用当前源文件，复用已有 Tokio rlib，无需修改应用。调用 `with_parent(..., Some(100_000))`、`throttle(1)`，保存 Weak 后 drop 最后一个业务 Arc，等待 100 ms；输出：

```text
strong_before_drop=2
alive_after_last_owner_drop=true
strong_after_last_owner_drop=1
alive_after_explicit_stop=false
```

最后一行来自探针额外调用 `set_limit(None)`，证明缺少 stop 正是释放受阻的原因。探针源文件位于 `$env:TEMP/vibe-audit-perf-20260921/limiter-lifecycle-probe.rs`；执行临时生成的 `limiter-lifecycle-probe.exe` 可重放。直接编译形式：`rustc --edition=2021 --crate-name limiter_lifecycle_probe <probe.rs> --extern tokio=<现有libtokio.rlib> --extern tokio_util=<现有libtokio_util.rlib> -L dependency=<src-tauri/target/debug/deps> -o <probe.exe>`。

**建议**：ticker 改持有 Weak，并在每次 tick 后释放升级得到的 Arc；或让业务 owner 显式拥有/停止 ticker，确保完成、取消、错误和 panic 统一回收。避免仅在某一引擎结束时打补丁。

**量化验收**：完成、暂停、失败各循环 100 次，最后业务 owner 释放后 ≤100 ms，Weak 全部不可 upgrade；instrumented ticker 数回到初始值。低限速取消与现有多 waiter 公平性测试仍通过。

### R26-P02｜P2｜任务实体缓存只有合并，没有容量或删除生命周期（新候选，已状态探针验证）

**证据**：[src/stores/task-data-store.ts:179](../src/stores/task-data-store.ts#L179) 的 `mergeEntities` 浅拷贝既有 map，再合并 incoming，从不驱逐；`:395` 的 `setTaskCursorPage` 即使 `append=false` 也使用它。更换查询只替换 `taskIds`，不是替换 `taskById`；`setTasks` 也采用相同策略。速度历史已会 `pruneToIds`，实体缓存没有等价流程。

**触发与影响**：连续翻页、切换多个查询、批量任务创建删除后，一个窗口曾看过的实体继续驻留到应用退出。保留当前页以外实体本身是 ARC-08 查询一致性设计的一部分，问题在于缺少可控的保留范围和删除失效。每次进度 map 浅拷贝也会连带复制所有历史 key。[src/components/shell/AppShell.tsx:636](../src/components/shell/AppShell.tsx#L636) 删除成功只清除 pending-delete 标记，随后的查询刷新仍走合并；删除失效缺口来自静态调用链，下面的探针只验证页面替换保留。

**本轮复现**：临时 Node/Vite SSR 脚本直接加载当前 store，以 10 组不同的 100 条合成任务依次调用 `setTaskCursorPage(..., append=false)`。结果为 `visible=100, cached=1000`；再以空结果替换，得到 `visible=0, cached=1000`。这验证保留策略，不测量 RSS 或 UI 卡顿。源文件 `$env:TEMP/vibe-audit-perf-20260921/entity-cache-probe.mjs`。

**建议**：保留可见页、活动任务、当前详情/选中项的 pin，再给剩余实体设置容量/LRU；删除成功显式失效缓存。不要直接清空所有非当前页项而破坏详情与查询一致性。

**量化验收**：1 万次不同查询/创建删除后，实体数 ≤配置上限 + 明确的 pin 数；硬删除完成的实体不能继续常驻。50k 历史场景反复搜索/详情切换 30 分钟，记录 heap/RSS 并验证回到同一视图后没有持续阶梯增长。

### R26-P03｜P2｜进度批次仍复制完整容器，并可能重复全量统计（新候选，静态成本，待基准）

**证据**：[src/stores/task-data-store.ts:618](../src/stores/task-data-store.ts#L618) 每批首个真实变化复制整个 `tasks` 数组，`:619` 复制整个 `taskById`；`:774` `recalculateTaskStats` 调用全量 `calculateTaskStats`（`:95`）。[src/hooks/use-task-events.ts:234](../src/hooks/use-task-events.ts#L234) 在已做增量统计的进度 patch 后仍安排一次 250 ms 防抖统计。[src/hooks/use-queue-reasons.ts:28](../src/hooks/use-queue-reasons.ts#L28) 的 Zustand selector 每次 store 更新遍历传入的全部 taskIds，返回相同字符串只能防 React render，不能免去 selector 计算。

**触发与影响**：长滚动累计加载大量任务、或 R26-P02 的实体缓存累积时，即使只有少量活动下载，单批任务变化仍携带 O(已加载数/缓存数) 的容器复制与 selector 扫描。这里不是 PERF-03 已修复的 toast 扫描，也没有证据可直接声称 10k 必然卡顿。防抖统计的实际频率还受连续事件节奏影响。

**建议**：先测 1k/10k/50k store，固定每批更新 1/8 个任务的耗时、分配量和 React commit。优先去掉可由增量值维护的重复统计，稳定 queued-id 派生集合；确有收益后再考虑分片实体表或更细 store，避免为理论 O(N) 引入不必要复杂度。

**量化验收**：固定目标机器，记录每批 patch p50/p95 与 JS 分配量，建议 p95 预算 8 ms；10k/50k 下 8 个活跃任务与输入/滚动并行时记录长任务数。优化前后使用相同数据、相同进度事件，保留 ARC-07/08 的排序与成员一致性测试。

### R26-P04｜P2｜列表排队原因轮询未接入隐藏与 in-flight 门控（新候选，代码确认）

**证据**：[src/hooks/use-queue-reasons.ts:38](../src/hooks/use-queue-reasons.ts#L38) 的 effect 在 `:46` 调 `getSchedulerSnapshot`，`:59` 无条件每 10 秒再次调用；只有 unmount 后的 cancelled 结果丢弃，没有 `document.hidden` 或 running 守卫。[src/components/tasks/TaskList.tsx:192](../src/components/tasks/TaskList.tsx#L192) 使用这个 hook。当前 QueueCenter 与 TaskDetails 已用新的统一 hook，此处是残留独立路径。

**触发与影响**：列表中有排队任务时，窗口隐藏仍发 IPC/读 settings/查询任务；若一次快照慢于 10 秒，后续请求可并发堆叠。后端 `commands/tasks/query.rs:411` 已把任务 ID 截到 500，因此不是无界 SQL IN；不足在轮询生命周期。

**建议**：复用现有 `useVisibilityGatedPoll`，真正 return/await 请求 promise；可进一步共享列表与队列中心的快照缓存、在 queue-change 后失效。恢复可见时立即刷新。

**量化验收**：隐藏 60 秒后快照调用增量为 0；单次请求耗时 25 秒时最大并发为 1；恢复可见立即请求一次；无 queued id 时不轮询。仅返回相同 key 不算停止后台工作。

### R26-P05｜P2｜创建路径预读、旧列表与浏览器全快照仍无界（现有 PERF-16，复核并校正旧描述）

**证据**：[src-tauri/src/db/task_records.rs:587](../src-tauri/src/db/task_records.rs#L587) `list_reserved_final_paths` 把全部活动任务/已选文件路径 UNION 后 fetch_all 到 HashSet；`commands/tasks/create.rs:896` 每次路径冲突重试都会调用它，预算最多 32 次。`db/task_records.rs:52` `list_task_records` 无 LIMIT，`commands/tasks/query.rs:125` 仍暴露旧全量 IPC；[src-tauri/src/db/task_records.rs:188](../src-tauri/src/db/task_records.rs#L188) 浏览器 realtime 的 active 分支也无 LIMIT。`browser_realtime.rs:270` 首次连接及 `:287` lagged 恢复会重新生成全快照。

**现状校正**：当前路径全集读取在 `begin_immediate`（create.rs:969）**之前**，只有一处调用；不能沿用旧审计的“DEFERRED 写事务内两次全量物化”描述。ARC-20/21 已缩短持锁窗口，残留是事务外查询/分配成本和重试放大。主列表正常 cursor 路径也不调用旧全量接口。

**影响**：大批排队/暂停任务与多文件任务使新建任务成本随整个活动历史增长；浏览器重连/落后恢复产生大 JSON，慢消费端可能反复全量同步。尚未测得具体毫秒/内存峰值。

**建议**：按候选 final_path 使用索引点查或唯一约束重试；旧接口分页/明确用途与上限；浏览器快照分页、限定范围或分块发送。不能用任意 LIMIT 截断路径集合而破坏 no-clobber。

**量化验收**：1k/10k 活动任务、每任务 100 文件场景，记录 create p95、查询行数、峰值内存；相同候选名下不随路径全集线性物化。WS 首包/恢复有约定数量与字节上限，最终状态无遗漏；并发同名创建和跨任务文件冲突回归仍通过。

### R26-P06｜P2｜多算法校验重复读文件，缺少共享校验作业资源预算（新候选，优化项）

**证据**：[src-tauri/src/download/checksum.rs:14](../src-tauri/src/download/checksum.rs#L14) 每次 `hash_file` 都打开文件，用 1 MiB buffer 流式读取；`:31` Digest::update 在 async task 内同步计算。`commands/tasks/actions.rs:1122` 对每个不同算法调用一次 `hash_file`（`:1129`），同算法虽已去重，但多算法各读一遍。手动 compute/verify（`:973/:987`）与 scheduler 完成校验（`scheduler/mod.rs:530`）共用函数，函数 API 未接收取消令牌，没有共享作业去重或并发信号量。

**触发与影响**：大文件多 hash、用户手动校验与自动校验重合、多任务同时完成，会增加磁盘读取与 CPU 竞争。当前 1 MiB 分块已有界，不能说整文件进内存；也未实测它阻塞下载。网络卷/HDD/低核设备应重点测量。

**建议**：同一次读取更新多个所需摘要；把 CPU 计算与磁盘校验放入可取消、有并发上限的作业队列，并对同任务/文件版本去重。是否迁入 spawn_blocking 应由 profiler 决定，不能每个小块都新建阻塞任务。

**量化验收**：同文件 4 算法的应用层累计 read 返回字节数接近 1×文件大小而非 4×（物理磁盘读取还受系统页缓存影响）；重复请求共享一次校验或明确排队；并发哈希数量不超过配置预算；大文件取消在 ≤1 秒内停止。以 HDD/SSD 两类环境比较同时下载吞吐与校验总耗时。

### R26-P07｜P2｜双密码学依赖仍存在，收益估算不能当成实测（现有 PERF-13）

**证据**：本轮实际运行 `cargo tree --locked --manifest-path src-tauri/Cargo.toml -e normal -i aws-lc-rs` 与 `-i ring`，确认 `aws-lc-rs 1.17.0` 与 `ring 0.17.14` 都在依赖树；aws-lc 由 rustls、russh、librqbit-sha1-wrapper 等路径引入，ring 经 rustls/FTPS 链引入。`Cargo.toml:29/:33/:65/:67` 与 `Cargo.lock:6062` 支持这一结论。

**影响**：两套底层库扩大依赖维护、构建工具链与潜在产物成本；具体冷构建时间和最终链接体积需测。不能把历史审计“4–8 分钟/2–5 MB”估计写成本轮实测，也不能仅凭 lockfile 断言所有未使用代码都进入最终 binary。

**建议**：按实际 feature 反向依赖统一 provider 或记录确需共存的边界；仅修改直接 rustls 声明不保证消除 russh/BT 依赖。SFTP 是 SSH，验收应覆盖它的 SSH 握手/算法，而不是称作 SFTP TLS。

**量化验收**：依赖树符合选定 provider 策略；同机/同缓存条件记录 3 次构建耗时和 release binary 大小；HTTPS、FTPS、SFTP/SSH、BT tracker/元数据链路回归通过，再启用准确的防复发规则。

### R26-P08｜P2｜当前 release 的交互与长跑性能证据仍不完整（PERF-11 的明确延期范围，非重开）

**证据**：[docs/performance-baseline-results.md:15](../docs/performance-baseline-results.md#L15) 记录历史 0.3.0/2026-07-20/debug 环境，`:78` 后 release 冷启动、可交互、滚动 FPS、稳态 RSS 仍待测；`:131` 后 HLS/BT 30min–8h soak、批量删除、100k 全矩阵延期。[src-tauri/tests/perf_baseline.rs:6](../src-tauri/tests/perf_baseline.rs#L6) 只在 debug assertions 下编译，测的是 DB cursor 查询。新脚本已经读取真实 package version，并搭好 opt-level 框架，应保留这些改进。

**影响**：已有查询数据不能回答当前 0.5.0 的冷启动、WebView 内存、后台 CPU、电池、持续下载后的资源斜率；本轮发现的 ticker 与实体缓存问题说明长跑验收有实际价值。文档顶部仍说 50k+ 延期，而 §3.3 已有 50k 查询数据，宜明确“50k UI/soak 未测”，避免误读。

**建议**：在真实 release candidate 上补 Windows/macOS/Linux 至少各一组；含低规格机器、1k/10k/50k历史、0/2/8活动任务、隐藏/显示/浮窗切换、8小时下载。首轮先固化可信测量，再决定预算。

**量化验收**：每组有 commit/dirty/profile/硬件/OS/WebView 元数据、冷/暖启动各 ≥5 次，UI操作 p50/p95、CPU/RSS、frame/long-task、线程/句柄/ticker/子进程数量；8小时结束后所有活动归零，资源趋势无随已完成任务数持续增长。PERF-11 原 headless 验收保持 Closed。

### R26-P09｜P2｜运行时回归预算仍是秒级 smoke，不能检出常见体验退化（现有延期门禁范围）

**证据**：[src-tauri/tests/perf_baseline.rs:23](../src-tauri/tests/perf_baseline.rs#L23) 默认每用例仅 5 次；`:391` 1k list p95 只要求 <5秒，10k <30秒；`:430` 注释写 50k search 的决策预算 100 ms，但 `:436` 实际只 assert <60秒。当前实现清楚注明为 soft smoke，故不是假门禁；只是还没承担交互性能防退化职责。bundle 已有硬门禁且本轮通过，二者应区分。

**影响**：从几毫秒退化到数百毫秒的变化仍可全绿。5样本的 p95 基本接近最大样本值，不足以稳定估计尾延迟；共享 CI 直接加入很紧绝对阈值又会易抖。

**建议**：保留普通 CI 的宽松 smoke，在固定 runner/nightly 上增加更多样本、基准分布与相对退化比较，并把 UI patch/IPC/启动纳入产物。不要无数据直接把所有环境 assert 改为 100 ms。

**量化验收**：固定环境每用例预热后 ≥30 样本；设定绝对预算与相对退化阈值（如 p95 >基线1.2×且超过噪声带时失败）；人为加入延迟后能使门禁失败，正常连续10轮不过度误报；产物包含样本而不只单一汇总值。

### R26-P10｜P3｜release 尺寸优化的吞吐取舍尚未做实验（现有 PERF-09，Needs benchmark）

**证据**：[src-tauri/Cargo.toml:112/:120](../src-tauri/Cargo.toml#L112) release 使用 `opt-level="s"`。[scripts/perf/run-baseline.ps1:52](../scripts/perf/run-baseline.ps1#L52) 已有 `-CompareOptLevel` 分别构建 s/3，并在 `:77/:78` 写构建秒数与 binary 字节；没有 hash/AES/XML/BT/真实下载吞吐采集。[docs/performance-baseline-results.md:114](../docs/performance-baseline-results.md#L114) 的对比表仍待采集。

**影响**：现在无法证明 s 是吞吐瓶颈，也无法证明改 3 有用户可感收益。对 I/O 为主的下载，盲目改全局优化级别可能只扩大包体和构建成本。

**建议**：保留当前 release 设置；补 CPU 热点与真实下载分别对比，标注冷/暖构建、缓存与硬件，按结果决定是否只覆盖少量热点 crate。现有脚本顺序复用同一 target，采集构建耗时必须说明缓存条件，不能称为公平冷编译对照。

**量化验收**：s/3 各 ≥3轮，包含体积、冷/暖启动、hash/AES/XML/BT与真实下载吞吐；报告中位数/方差，收益超过测量噪声且功能/安全回归通过才调整。继续保留 Needs benchmark，不能因框架已存在就标 Closed。

## 7. 建议实施顺序与工作包

优先级表示风险与处理顺序，不是承诺工期。新发现先进入主审计，明确负责人和验收用例；同一根因跨引擎处理，避免每个协议各加一个不一致的补丁。完整清单共 42 个局部条目：交互 9 项、功能 12 项、架构 11 项、效率 10 项；建议优先级为 P1 9 项、P2 28 项、P3 5 项。它们包含缺陷、既有审计残余、能力增强和验收缺口，**不能解释为 42 个已复现 bug**，也不替代主审计的正式风险计数。

| 顺序 | 工作包与关联项 | 建议交付物 | 退出条件 |
| --- | --- | --- | --- |
| 1 | 文件、备份与密文边界：A01–A05 | 共享文件提交原语、原子备份写入、同卷恢复 staging、无 panic 解密输入验证、无歧义密文迁移 | 外部同名竞争、写入中断、异卷恢复、坏 nonce、旧格式 fixture 全部通过；旧文件／备份仍可恢复 |
| 2 | 长期资源生命周期：P01、P02、P04 | 限速器 owner 回收、实体缓存保留规则、剩余轮询统一门控 | 有限次数下载／暂停／失败后 ticker 回基线；缓存容量有界；隐藏无轮询，慢请求不重叠 |
| 3 | 创建与批量主流程：U01–U05、F01–F04 | 单一草稿／探测上下文、请求代数、批量结果模型、结构化错误、协议凭据与目录预算 | 认证预览到创建一致；100 条部分失败可完整处理；旧结果不回填；不支持媒体形状提前拒绝 |
| 4 | 发布可追溯性：A07、A08 | 固定 commit 的预检／构建／扩展／校验／推广流程 | 任一步失败不公开版本；tag、构建 SHA、校验资产和 updater 一致；rc 与 stable 分离 |
| 5 | 产品集成与真实使用：U06–U09、F05–F12、A06 | 浏览器中心前端接线、认证恢复补齐、诊断与规则解释、原生可访问性与真实服务验收记录 | 新命令 bindings 无漂移，用户入口完整；三平台／真实协议／升级路径有固定版本证据 |
| 6 | 规模与维护：A09–A11、P03、P05–P10 | 查询／校验基准、release UI／长跑数据、资源预算、模块拆分、语义文档校正 | 改动有前后对照；去重不破坏已关闭合同；性能和质量门禁能检测人为注入的退化 |

工作包 1、2、3 可以由不同人员并行，但应先稳定合同与测试再重构共享协调器。发布流程可以独立推进。DASH 画质选择、递归目录、BT tracker 深度和动态目录模板，应排在数据保护、凭据、导入恢复和资源回收之后。

### 7.1 最值得先处理的具体事项

1. **A01：原子拒绝覆盖。** 文件安全承诺不能只靠一次 exists 检查，先补确定性并发测试。
2. **A04：备份失败时保住旧备份。** “识别出坏备份”与“避免毁掉最后一份好备份”是两个验收目标。
3. **P01：清理已结束限速任务的 ticker。** 已有模块级实证，修复范围小于一次协议重构，却直接影响长期运行。
4. **F01：贯通 FTP／SFTP 创建凭据。** 以不含嵌入凭据的 URL 测所有入口，特别是批量和过期 snapshot。
5. **U02：完整保留批量失败集合。** 是大量任务管理中的直接效率损失，有明确的组件级复现。
6. **A02 与 A07／A08：坏数据隔离和发布顺序。** 分别阻止异常持久化记录绕过正常恢复、未验证产物过早公开。

## 8. 统一验收方案

### 8.1 跨引擎合同矩阵

已有协议可靠性矩阵继续保留，同时把本轮容易漏掉的“入口／阶段”作为第二维。只测引擎本身不能替代用户创建命令的集成测试。

| 合同 | 必须覆盖的入口／异常 | 关键断言 |
| --- | --- | --- |
| 凭据与代理 | 单文件 probe、目录 probe、无 snapshot 创建、过期 snapshot、批量预览／创建、恢复；Inherit／Off／Custom | 同一草稿上下文，敏感值加密且日志脱敏；无错误的匿名或直连回退 |
| 取消与超时 | 连接前、连接后不返回头、慢速滴流、限速等待、目录扫描、ffmpeg 探测与 remux | 取消与 timeout 分类不同；所有 future／子任务／PID 最终收敛，UI 可见地结束 |
| 提交与恢复 | 同名下载、外部程序插入同名文件、跨卷、磁盘满、发布中断 | 不覆盖外部文件；checkpoint 不领先可恢复数据；失败保留原始临时文件 |
| 备份与迁移 | 覆盖旧备份时失败、TEMP 与 DB 异卷、坏密文元数据、跨机路径映射 | 旧备份和 live DB 安全；失败不会留下可被误应用的 pending；重配事项准确列出 |
| 多文件与媒体 | 100+ 文件、部分失败、SegmentList init／Range、多语言、多源清单 | 计划与请求一致；不支持项提前失败；完成判定按真实选择；范围／凭据不越界 |
| 前端请求版本 | 快速换 URL、清空输入、改变配置、关闭／重开、两次预览与创建乱序 | 只有当前输入会话可提交或回填；手动名称受保护；进度与禁用状态对应真实请求 |
| 长期资源 | 多轮开始／暂停／重试／完成，窗口隐藏，浏览器重连，长时间切换查询 | ticker／连接／子进程／句柄返回基线；缓存可解释地有界；后台流量不随历史累计 |

### 8.2 性能测量最小可交付集

以下是建议采集计划，不是本轮已经完成的跑分，也不是现有产品保证：

| 场景 | 样本／规模 | 记录项 | 决策用途 |
| --- | --- | --- | --- |
| 启动 | 固定 release candidate；冷／暖各至少 5 次 | 从进程创建到首屏、可交互、任务加载；进程与 WebView RSS | 判断 bundle、初始化查询和平台启动成本 |
| 列表和输入 | 1k／10k／50k 历史；0／2／8 活动任务 | 搜索与筛选 p50／p95、JS patch、长任务、滚动帧时间 | 决定是否优化容器复制、统计、selector 或搜索结构 |
| 大批创建／操作 | 1k／10k 活动任务，多文件 100 条／任务；批量 1k 操作 | 创建耗时、查询行数、锁等待、IPC 字节、内存峰值 | 验证路径全集物化、全量接口和批量结果设计 |
| 校验 | 同一大文件 1／4 个算法，下载与校验并行 | 应用读取字节、CPU、磁盘、下载吞吐、取消延迟 | 决定多摘要单遍、共享作业和并发预算 |
| 长跑 | HLS／BT／HTTP 混合 30min 初筛、8h 验收 | CPU／RSS 曲线、连接、PID、线程、句柄、ticker、临时文件 | 查明资源是否随已结束任务数量增长 |
| 依赖／编译 | provider 方案、`opt-level=s/3`，各至少 3 轮 | 二进制体积、构建缓存条件、启动与实际热点吞吐 | 避免为了估计收益付出功能兼容性代价 |

发布候选记录必须包含 commit、dirty 标记、配置、系统／WebView、硬件、协议对端与原始样本。先得到可信基线，再在固定 runner 上收紧预算；共享 CI 保留防挂起的宽松 smoke。不要用 gzip 体积替代桌面启动性能，也不要用 loopback 下载速度替代公网使用体验。

### 8.3 修复时的门禁组合

- 普通前端行为修改：`pnpm typecheck`、`pnpm test:frontend`；UI／打包变化加 `pnpm build`、`pnpm check:bundle`。
- Rust 命令或模型变化：生成 Specta bindings，并运行 `pnpm check:bindings`；不得手工修改生成类型以掩盖漂移。
- 下载、恢复、取消、文件提交变化：在 `src-tauri/tests` 添加真实 fixture／故障注入集成测试，再跑受影响协议与共享层回归。
- 新用户文案：全部 7 locale、稳定 code→key 映射覆盖测试及 `pnpm check:i18n`。
- 浏览器及其说明变化：`pnpm build:extensions`／`pnpm verify:extensions`，并根据发布 profile 验证权限和实际能力。
- 合并候选前：`pnpm verify`、bindings、平台条件检查及场景验收；发布前再加真实安装、升级、卸载与资产验证。不能把一项编译成功当作问题关闭条件。

## 9. 应明确保留的产品边界与后续决策

以下事项可以提升丰富性，但应保持明确取舍，不能因功能列表更长就挤占可靠性工作：

| 方向 | 当前边界 | 建议 |
| --- | --- | --- |
| 云盘、账号同步、插件协议 | 尚未实现；既有 `FUN-19` Boundary | 有明确用户与维护资源后再定协议和数据模型；当前先巩固下载与恢复 |
| 网页视频嗅探 | 尚未实现 | 与 F05“直接链接内容识别”分开决策，后者可在现有权限内改善体验 |
| HLS／DASH 扩展 | HLS AES-128 不代表 SAMPLE-AES／DRM；DASH 是受限静态子集 | 扩展语法需完整 fixture 和明确拒绝路径，不把“能读 MPD”宣传为全标准兼容 |
| 浏览器自动接管与 Cookie 转发 | candidate／release 保持最小权限手动交接；实验能力仅 dev | 不为功能丰富性放松既定安全边界；商店身份、签名和 Native Messaging 实装另行验收 |
| Safari 与正式商店分发 | Safari 包装、商店身份／审核等未完成 | 作为交付工作包管理，不能只因 WebExtension 源码共用就认为已支持 |
| OS 签名 | Updater 产物签名已配置，但不是 Windows Authenticode／Apple Developer ID | 在真正配置并验收前继续标明未签名；签名不能替代安装／升级 smoke |
| CLI／远程 API、PAC／WPAD | 路线图中的后续能力 | 优先形成版本化任务模型、认证与访问边界，再提供自动化接口 |
| 跨机完整迁移 | 备份含数据库、部分密文绑定本机；不包含下载实体 | 先把重配置和文件映射向导做好，再决定是否增加独立口令保护的便携备份 |

本报告不建议扩大浏览器控制本地路径的权限、不建议静默绕过代理、不建议移除 SSRF／TOFU／源域凭据绑定，也不建议通过降低测试要求来关闭问题。最应形成的产品承诺是：创建参数全程一致，失败可恢复，文件不被误覆盖，任务停止后资源会释放，发布资产可追溯，并且这些承诺有对应的实测证据。

