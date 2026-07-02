# 架构与工程审计

最后更新：2026-06-30（Batch 1 修复：A-3/A-4/A-6/UX-2/UX-3/UX-4/UX-5；Batch 2 已完成：F-5；基于工作树代码逐行核实 + 对抗式复核重写）

本审计从**用户交互便捷性**、**程序功能丰富性与完整性**、**项目架构的鲁棒性与稳定性**、**程序运行效率**四个维度复核 Vibe Downloader `0.2.0`。

本次复核方法与以往不同：每条结论都直接打开当前工作树的源码核实，并经过独立的对抗式复核（重新打开每条引用的 `file:line` 再判定）。**不再默认信任本文件历史版本里的 FIXED 断言**——复核确认其中若干项（SSRF、Metalink 并行、probePhase 阶段反馈）只是部分修复或流于表面，已在下文与「旧结论修正清单」中更正。

## 总体结论

项目成熟度很高，HTTP/HTTPS 主路径扎实：探测、分段、续传校验、限速、SQLite 持久化、调度、浏览器交接、设置、命令面板、任务详情、虚拟滚动均已落地，且有 314+ 项 Rust 测试和 CI 验证。

但本次代码核实发现，真正的剩余短板集中在四类，且**部分被历史审计标记为 FIXED**：

1. **新增协议引擎的运行时正确性**：Metalink 并行路径假续传、进度被覆盖；BT 种子率/上传限速未执行；HLS 备选音轨/字幕被丢弃。这些是「声称支持但代码不达预期」。
2. **调度器错误路径的资源管理**：`start_task` 在非 Conflict 错误分支泄漏下载槽位，可静默饿死整个调度器。
3. **安全边界纵深不足**：SSRF 防护只做一次性主机名字符串检查，可经 DNS rebinding 与重定向绕过。
4. **前端事件刷新的规模化效率**：`queue-changed` 整页重查 + store 重建、`task_stats_snapshot` 每秒全表聚合，规模化下是主要卡顿来源。

严重度图例：🔴 高（功能失效/数据或安全风险/可静默停摆）、🟡 中（明显体验或可靠性损耗）、🟢 低（局部优化）。

---

## 一、用户交互便捷性

### UX-1 五种语言仅约 50% 翻译，自动检测把用户丢进半英文界面【🔴 高】（已修复 2026-06-30）

**定位**：`src/i18n/index.ts:14-29`、`src/i18n/index.ts:76-85`、`src/i18n/index.ts:122-142`、`src/i18n/locales/ja.ts:6-12`

`LOCALE_REGISTRY` 只把 `en` 与 `zh-CN` 标为 stable。`en` 约 885 个翻译 key、`zh-CN` 约 847，但 `ja=381 / es=433 / ko=403 / ru=282 / zh-TW=448`，约一半。`ja` 整段缺失 `errors / contextmenu / shortcuts / onboarding / shutdown / about` 等区块。`detectInitialLocale()` 会按 `navigator.language` 自动选中这些 beta 语言，仅 `console.warn`；配合 `fallbackLng:'en'`，命中这 5 种语言的用户**首次启动看到混合语言 UI**（错误提示、右键菜单、整个快捷键面板和 onboarding 仍是英文），无任何应用内说明。

> 更正：用户**始终可切回英文**（`STABLE_LOCALES` 含 `en`，设置页 `SettingsPage.tsx:1557` 始终渲染英文选项）。早期断言「切不回英文」不成立。

**修复**：将 `detectInitialLocale` 限制到 `STABLE_LOCALES`，让不稳定语言回落到 `en`；或补全翻译；若保留自动选中，则用一次性应用内横幅替代 `console.warn`，并在选择器中给 beta 语言加「部分翻译」徽章。

### UX-2 搜索叠加双防抖，结果约 500ms 后才更新【🟡 中】（已修复 2026-06-30）

**定位**：`src/components/shell/CommandBar.tsx:42-46`、`src/components/tasks/TaskList.tsx:103`、`src/components/tasks/TaskList.tsx:202-204`

`CommandBar` 先 `useDebouncedValue(searchInput, 200)` 防抖 200ms 再 `setSearch` 写入 store；`TaskList` 又 `useDebouncedValue(search, 300)` 防抖 300ms 才在 effect 里 `loadPage`。两段防抖串行，查询在最后一次按键后约 500ms 才触发，清空搜索框也滞后约 500ms，且间隙内搜索框无 loading 反馈。

**修复**：只防抖一次——让 `CommandBar` 写入原始按键、仅保留 `TaskList` 的 300ms；或只在 `CommandBar` 防抖、`TaskList` 直接消费已防抖的 store 值。

### UX-3 没有键盘/命令面板聚焦搜索的路径，搜索是鼠标专属【🟡 中】（已修复 2026-06-30）

**定位**：`src/components/shell/AppShell.tsx:858-976`、`src/components/shell/CommandBar.tsx:266-272`

全局 keydown 处理器注册了 `mod+k / mod+/ / mod+n / mod+, / mod+1..5 / mod+方向键 / mod+d/o/enter/r/a / Delete / mod+a / mod+shift+a / ?`，**唯独没有聚焦搜索框的快捷键**（无 `mod+f`、无 `/`）。`CommandBar` 的 Input 也没有 ref、不能编程聚焦，搜索也不在命令面板里。键盘用户必须 Tab 穿过 chrome 才能到达。这违背 PRODUCT.md 的「键盘高效」承诺。

**修复**：给 `CommandBar` Input 加 ref，在 `AppShell` 加 `mod+f`（和/或 `/`）聚焦它（沿用已有的 `isInput`/`isComposing` 守卫），并加一个「搜索任务」命令面板项。

### UX-4 空列表无法区分「筛选无匹配」与「还没有下载」【🟡 中】（已修复 2026-06-30）

**定位**：`src/components/tasks/TaskList.tsx:582-598`、`src/components/tasks/TaskList.tsx:122-129`

空状态分支只检查 `search`：有搜索词显示 `emptySearch`，否则显示通用空态 + `emptyHint`（提示去添加下载）。但 `activeFilterCount`（fileType/source/failure/resume）追踪的筛选条件也能把列表清零，且无搜索词时落入通用分支——明明筛选 chip 显示在上方，却提示用户「去新建下载」。

**修复**：分支同时看 `activeFilterCount`：当 `(search || activeFilterCount>0)` 且 `filtered.length===0` 时，显示「当前筛选无匹配」状态并提供「清除筛选」按钮。

### UX-5 队列重排仅右键菜单，无键盘、无乐观反馈【🟡 中】（已修复 2026-06-30）

**定位**：`src/components/tasks/TaskContextMenu.tsx:67`、`src/components/shell/AppShell.tsx:232-273`

重排（置顶/上移/下移/移到底部）由 `canReorder = status==='queued' && onReorder` 门控，且只暴露在右键菜单里——无拖拽手柄、无键盘快捷键、无命令面板项。`handleReorder` 在 `await reorderQueuedTasks(orderedIds)` 成功后**不**乐观更新 store 或刷新，只在 catch 分支动作。可见行序要等后续列表刷新/事件到达才变，用户得不到即时确认。

**修复**：加键盘快捷键（如选中排队行后 `Alt+↑/↓`）和/或拖拽手柄；成功路径上对 task store 做乐观重排，失败时在 catch 分支回滚。

### UX-6 probe「阶段」指示是 URL 正则静态猜测，把普通 HTTP 标成「检查运行时」【🟢 低】（已修复 2026-06-30）

**定位**：`src/components/shell/NewDownloadDialog.tsx:76-103`、`:353-374`、`:988-995`

`inferProbePhaseFromUrl` 纯按 URL scheme/扩展名猜阶段；任何普通 https URL 都返回 `{ kind: 'checking_runtime' }`。`detect()` 在 await `probeTask` 前设一次阶段，之后只更新为 `done`，**probe 期间从不推进**。于是最常见的 HTTP 添加路径下，指示器一直显示「检查运行时」，而后端实际在做 HEAD/GET。

> 更正：历史审计将 UX-1（probePhase）标记为 FIXED 过于乐观——阶段反馈存在但为装饰性、且会误描述最常见路径。

**修复**：已实施——Rust 各引擎 probe 路径埋点真实阶段事件（`probe-phase` event），前端 `NewDownloadDialog` 订阅事件按 `requestId` 实时推进指示器，替换 `inferProbePhaseFromUrl` 静态猜测；HTTP/HTTPS 初始显示 `connecting` 而非 `checking_runtime`。各引擎 emit 序列：HTTP→`connecting`；HLS/DASH→`checking_ffmpeg`→`fetching_manifest`→`parsing_manifest`；Metalink→`fetching_manifest`→`parsing_manifest`；WebDAV→`connecting`；FTP→`connecting`→`querying_metadata`；SFTP→`connecting`→`verifying_host_key`→`querying_metadata`；BT magnet→`parsing_magnet`；BT .torrent→`fetching_torrent`→`inspecting_metadata`。

### UX-7 多文件「全选」控件从不显示 indeterminate（混合）状态【🟢 低】

**定位**：`src/components/shell/NewDownloadDialog.tsx:889-919`

select-all 控件是 `role='checkbox'` 且 `aria-checked={selectedFiles.size === probe.files.length}`——严格布尔。部分选中时报 `aria-checked=false` 并渲染空框，而相邻计数显示「3 of 5」。屏幕阅读器对部分选中念「未选中」，与可见计数矛盾，是 torrent/metalink 多文件选择器的 a11y 正确性缺口。

**修复**：计算三态 `aria-checked={size===total ? 'true' : size===0 ? 'false' : 'mixed'}`，混合态渲染 dash/indeterminate 字形。

### UX-8 已确认修复仍有效的旧交互项

复核确认以下历史修复在当前代码中仍成立：`Mod+R` 重试、单一 StatusBar、TaskDetails 顶层 Chunks tab、onboarding 扩展引导、错误详情复制（`formatErrorForReport` + TaskRecoveryActions 复制按钮）、磁盘空间差额提示（`query_disk_space`）、队列重排后端能力（`reorder_queued_tasks`，但前端反馈见 UX-5）、toast hidden count。

---

## 二、程序功能丰富性和完整性

### F-1 BitTorrent 种子率限制（seed ratio）已存储但从不执行【🔴 高】（已修复 2026-06-30）

**定位**：`src-tauri/src/download/bt.rs:637-677`、`src-tauri/src/db/torrent.rs:98`、`src-tauri/src/db/torrent.rs:290`

`seed_ratio_limit` 已持久化（`torrent.rs:98`，迁移 `001_init.sql:248`），ratio 也在 `bt.rs:529` 实时计算，但下载循环**从不拿 ratio 与 `seed_ratio_limit` 比较**。finished 分支在 `seeding_enabled` 时设 `seeding_state="seeding"` 后直接 `return Ok(())`；唯一的 `api_torrent_action_forget`（`bt.rs:670`）在做种被**禁用**的 else 分支。grep 确认 `seed_ratio_limit` 在 `download/` 内无任何读取。

**影响**：设了种子率的用户期待到比例后停止做种、释放上传带宽与 peer 槽，实际每个完成且启用做种的种子**一直做种到关闭程序**。这是 qBittorrent/Transmission/aria2 的基线功能。

**修复**：做种时每个 tick 复查——`seeding_enabled && seed_ratio_limit.is_some() && ratio >= limit` 时调用 `api_torrent_action_forget(torrent_id)`、置 `seeding_state="completed"` 并返回。当前 finished 分支立即返回，做种状态从未被再评估。

### F-2 Metalink 并行路径无法续传，却宣告 supports_resume=true【🔴 高】（已修复 2026-06-30）

**定位**：`src-tauri/src/download/metalink.rs:133`、`:441`、`:854`、`:800`

历史审计将 F-2 标为 FIXED，但 probe 无条件设 `supports_resume:true`（`:133`）。并行路径每次运行开头 `cleanup_metalink_part_files(temp_path)`（`:441`），每个 worker 用 `fs::File::create(part_path)`（`:854`，截断），range 请求始终是从 range 绝对起点 `bytes={range_start}-{range_end}`（`:800`），**不带已下载偏移**。cancel 分支（`:523-525`）虽保留 part 文件，但下次运行 `:441` 的 cleanup 会清掉它们。只有串行 fallback 经 temp 文件大小续传。

**影响**：大型多镜像下载（恰恰会走并行）在任何暂停/崩溃后**丢失全部进度、每个 range 从零重启**，与 UI 宣传的续传能力直接矛盾。

**修复**：续传时 stat 每个已存在 part 文件、跳过开头 cleanup、以 append 打开、发 `bytes={range_start+already}-{range_end}`；仅在全新零进度启动时清理 part。

### F-3 Metalink 并行进度被覆盖而非求和【🟡 中】（已修复 2026-06-30）

**定位**：`src-tauri/src/download/metalink.rs:884-887`、`:478-521`

每个 range worker 每 300ms 用**自己的**字节数 `db::update_task_file_progress(pool, file_id, downloaded, ...)`。N 个 worker 写同一 `file_id` 行，持久化进度是最后开火的 worker，而非聚合值。协调器只在 worker 完整完成时（`join_next`，`:487/511`）累加 `downloaded_total`，下载中持久化值反映单个 range、跳变剧烈。

**影响**：并行 Metalink 下载时进度条与 ETA/磁盘估算错误且抖动（会倒退、只显示真实进度的一部分），损害信任。

**修复**：worker 经 mpsc 上报字节增量给协调器（如 SFTP/FTP 的 `WorkerProgress`），仅由协调器写汇总进度。

### F-4 Metalink 并行下载路径零端到端测试覆盖【🟡 中】（已修复 2026-06-30）

**定位**：`src-tauri/tests/metalink_engine.rs:1-23`、`src-tauri/src/download/metalink.rs:366-576`

`metalink_engine.rs` 头部明确写道并行函数（`download_metalink_file_parallel`、`MetalinkRangeWorker`、`download_metalink_range_from_mirror`、`assemble_metalink_part_files`）仅经 `MetalinkEngine::download`（需 `AppHandle`）触达，完整路径「deferred to Phase 8」。当前只测了 probe 层与 DB 镜像健康度，F-2/F-3 的缺陷正坐落在未测代码里。

**修复**：加 fake 多镜像 HTTP server 集成测试驱动并行路径，断言装配文件完整性、下载中进度单调、镜像 failover、暂停/恢复连续性。

### F-5 手动校验输入仅限 SHA-256，尽管引擎支持 SHA-512/SHA-1/MD5【🟡 中】（已修复 2026-06-30）

**定位**：`src-tauri/src/commands/tasks/create.rs:605-613`、`src-tauri/src/download/checksum.rs:40-45`

`hash_file` 计算 Sha256/Sha512/Sha1/Md5，Metalink 也会选最强算法，但手动建任务只接受经 `normalize_sha256` 的 `input.expected_hash_sha256`，无其他算法字段。

**修复**：给创建输入与校验命令加通用 `expected_hash + algorithm` 选择器，按算法校验 hex 长度，并把 MD5/SHA-1 标为弱校验。

### F-6 HLS 忽略备选渲染：无多音轨/字幕选择【🟡 中】（已修复 2026-06-30）

**定位**：`src-tauri/src/download/hls.rs:1470`、`:1109`

HLS 处理 master variant、AES-128、init map、byte range、live polling，但文件内唯一的 `EXT-X-MEDIA` 匹配是 `#EXT-X-MEDIA-SEQUENCE`（live 媒体序号），**不是备选渲染 `#EXT-X-MEDIA` 标签**。扫描 `TYPE=AUDIO/TYPE=SUBTITLES/GROUP-ID` 无结果，独立音频组与 WebVTT 字幕从不被解析/下载/封装。

**修复**：解析 `#EXT-X-MEDIA` 组、在 variant 选择中暴露音轨/字幕选项、下载选中渲染并交 ffmpeg `-map` 封装（按需转换 WebVTT）。

### F-7 BitTorrent 无上传限速（硬编码为无限）【🟡 中】（已修复 2026-06-30）

**定位**：`src-tauri/src/download/bt.rs:190-193`、`:685-688`

`options.ratelimits.upload_bps` 硬编码 `None`。只有 `sync_session_download_limit`（`:685`）调 `set_download_bps`，无 `set_upload_bps`、无上传上限设置或列。

**影响**：做种或 tit-for-tat 上传可打满用户上行且无节流。所有主流 BT 客户端与 aria2 都暴露上传限速。

**修复**：加上传限速设置（全局/逐任务），在 session 创建时接入 `LimitsConfig.upload_bps`，并加 `sync_session_upload_limit` 支持实时变更。

### F-8 无外部自动化 API（JSON-RPC/REST）【🟡 中】

**定位**：`src-tauri/src/browser_realtime.rs:126-128`

唯一的本地监听是浏览器 realtime axum WebSocket（`/browser/ws` 绑环回），只服务 native-messaging 交接。全仓搜 `jsonrpc/json_rpc/api_server/rest_api` 无匹配。所有任务控制经 Tauri 命令，只能从打包前端调用。

**影响**：不同于 aria2（JSON-RPC）和 IDM（CLI/COM），脚本/NAS/cron 无法驱动，是 power user 与 headless 场景的完整性缺口。

**修复**：暴露可选、token 鉴权的 localhost JSON-RPC/REST，复用现有命令层，提供 addTask/pause/resume/remove/status。

### F-9 无 PAC 代理；代理模式仅 direct/system/custom【🟢 低】

**定位**：`src-tauri/src/proxy.rs:16-19`、`src-tauri/src/download/sftp.rs:1176-1182`

`AppProxyMode` 仅 Direct/System/Custom，搜 `pac/wpad/auto-config` 无结果；SFTP 进一步把 custom 限制到 SOCKS5。企业网常经 PAC/WPAD 分发代理配置，这类用户无法正确路由而不丢失 per-host 逻辑。

**修复**：加 PAC 模式，按目标 host 评估 PAC 脚本（沙箱化 JS 引擎、超时受限）选代理。

### F-10 完成后整理仅限静态子目录，无模板占位符【🟢 低】

**定位**：`src-tauri/src/db/classification_rules.rs:201-232`

`apply_classification_rules` 按扩展名/MIME/url-contains 匹配并原样返回 `rule.target_subdir`，无模板展开，路径不能含 `{category}/{host}/{date}/{name}`，所有命中文件落入同一字面目录。

**修复**：将 `target_subdir` 作为模板对任务元数据（category、host、ISO 日期、净化文件名）展开，并对路径段做安全净化。

### F-11 DASH live/SegmentTimeline 不支持、HLS DRM 不支持（确认的设计边界）

DASH 仍拒绝 dynamic/live 与 `SegmentTimeline`（合理的早期边界）。需在 UI 与 README 持续保持「静态/VOD MPD first-pass」「不支持直播/动态 MPD」「不支持 DRM」的明确表述。

---

## 三、项目架构的鲁棒性和稳定性

### A-1 start_task 在非 Conflict 转移错误时泄漏 DownloadControl 槽位，可永久饿死调度器【🔴 高】（已修复 2026-06-30）

**定位**：`src-tauri/src/scheduler/mod.rs:281-293`、`:323`、`:217-231`

`start_task` 在调用 `transition_task` **之前**就把 pending `DownloadControl` 插入共享 `downloads` map（`:282-293`）。Conflict 分支会移除它（`:314`），但通用错误分支 `Err(error) => return Err(error.into())`（`:323`）**直接返回、未移除**。worker 及其自清理 `downloads_map.lock().await.remove`（`:418`）要到 `:337` 才 spawn，此路径下从不运行。dispatch 的 caller Err 分支（`:217-231`）只重写 DB 状态、不碰 downloads map。

**影响**：Downloading 转移期间一次瞬时 sqlx/DB 错误，就在 `downloads` 留下幽灵条目。`active_count` 来自 `downloads.len()`（`:133`），per-host 槽位也靠遍历该 map（`:159-164`、`:476-484`），每次泄漏永久占用一个活动槽 + host 连接槽。长会话累积后可用槽位降到 0，调度器**静默停止派发新下载，直到重启**。

**修复**：在 `:323` 错误分支返回前 `self.downloads.lock().await.remove(&task.id);`（镜像 Conflict 分支）；或用 RAII guard，仅在 `:462` worker spawn 成功后才「提交」该条目。

### A-2 SSRF 防护是一次性主机名字符串检查，可经 DNS rebinding/重定向/缺失 IP 类别绕过【🔴 高】（已修复 2026-06-30）

**定位**：`src-tauri/src/commands/browser.rs:747-776`、`src-tauri/src/download/http/mod.rs:276-291`、`:295`

历史审计将 browser handoff SSRF 标为 FIXED，但 `is_private_or_reserved_url`/`is_private_ip` 只在交接校验时检查**字面主机名字符串**：

1. DNS 解析后**从不复查**：`HickoryResolver::resolve`（`http/mod.rs:276-291`）返回解析器给出的任意 IP，无私有 IP 过滤——公网域名解析/rebind 到 `127.0.0.1` / `169.254.169.254` / `10.x` 照样连接。
2. reqwest 客户端跟随最多 10 次重定向（`http/mod.rs:295`），**每跳无 SSRF 复检**——公网 URL 可 302 到内网地址。
3. `is_private_ip`（`browser.rs:767-776`）漏掉 IPv4-mapped IPv6（无 `to_ipv4_mapped`）、IPv6 unique-local `fc00::/7`、link-local `fe80::/10`、IPv4 CGNAT `100.64/10`（`is_shared`）。V6 分支只查 loopback/unspecified/multicast。

**影响**：私有/保留地址保护（内网开关关闭时）可被精心构造的交接击穿，触达云元数据端点（`169.254.169.254`）或内网服务。WS bridge 路径风险最高——本地进程持 bootstrap token 提交 `createDownload`（`browser_realtime.rs:227`）。

**修复**：在**连接时**而非 URL 字符串上强制 IP 检查——自定义 resolver/connector 拒绝私有/保留解析 IP，自定义重定向策略每跳复验；扩展 `is_private_ip` 覆盖 `to_ipv4_mapped`、`is_unique_local`、`is_unicast_link_local`、`is_shared`。

### A-3 关闭 flush 共享单个 3 秒预算串行耗尽，且追踪的外层任务被 abort 时跳过内层清理【🟡 中】（已修复 2026-06-30）

**定位**：`src-tauri/src/lib.rs:475-481`、`:116-148`、`src-tauri/src/scheduler/mod.rs:337-394`

`shutdown_active_downloads` 先取消所有 token（`lib.rs:110-113`），再用**共享单个 3 秒 deadline** 循环 await join handle（`:130-148`，以 `Duration::from_secs(3)` 调用）。第一个 handle 可吃掉大部分预算，其余命中 `remaining_time.is_zero()`（`:132`）被立即 abort。所存 `handle` 是**外层** scheduler 任务（`scheduler/mod.rs:337`，存于 `:462`），它再 spawn 内层 `download_handle`（`:380`）；abort 外层不会 abort 内层，也跳过外层下载后清理。worker 仅靠 cancel token 在预算内抵达最终 checkpoint。

**影响**：多任务并发退出时，第一个之后的任务确认窗口接近零，最终进度 checkpoint 可能在途中即被 abort，**丢失续传进度**。固定 3 秒总预算不随活动任务数伸缩。

**修复**：用单个 `tokio::time::timeout` 包住 `join_all` 并发等待，而非串行耗预算；和/或按活动数缩放超时。

### A-4 每任务运行时锁注册表除显式删除外无界增长【🟡 中】（已修复 2026-06-30）

**定位**：`src-tauri/src/lib.rs:62-69`、`src-tauri/src/commands/tasks/actions.rs:455`、`:530`

`TaskRuntimeLocks.lock()` 首次使用任何 task id 都插入 `Arc<Mutex<()>>`（`lib.rs:62-69`），`evict()` 只在 `delete_task`/`bulk_delete_tasks` 调用（`actions.rs:455,530`）。`start_task` 与 pause/resume/retry/cancel 都调 `lock()` 但都不 evict。结构体注释（`lib.rs:53-54`）声称「防止 HashMap 无界增长」，但完成/失败/暂停后从不删除的任务条目**永久驻留**。

**影响**：长会话中创建/下载/重试大量任务而不删除，锁条目随触达过的不同 task id 数累积，是受用户删除行为而非活动任务数约束的缓慢内存泄漏。

**修复**：在 worker 完成路径（`scheduler/mod.rs:418` 附近，guard drop 后）与终态用户动作后驱逐，复用已有的 `strong_count==1` 安全检查。

### A-5 DNS resolver 构造用 .expect() 在 build_client 内 panic【🟢 低】（已修复 2026-06-30）

**定位**：`src-tauri/src/download/http/mod.rs:263-273`、`:300`

`HickoryResolver::new()` 以 `.expect("failed to create DNS resolver")`（`:271`）构造，由 `build_client`（`:300`）无条件调用，而后者本身返回 `Result<Client, String>`，用于引擎初始化与代理配置变更。系统 DNS 配置缺失/损坏时此处 panic 而非返回 Result。在 spawned worker 内 panic 被 JoinHandle 捕获（`scheduler/mod.rs:395-416`），但同步的 `set_proxy_config`/启动路径不被包裹，可中断该上下文。

**修复**：`HickoryResolver::new()` 返回 Result，在 `build_client` 用 `?` 传播，给出「DNS resolver 不可用」错误而非 `expect()`。

### A-6 Realtime 广播 lag 静默丢事件给慢 WS 客户端【🟢 低】（已修复 2026-06-30）

**定位**：`src-tauri/src/browser_realtime.rs:65`、`:198-208`

广播通道容量 256（`:65`）。`handle_socket` 中 `Ok(event) = rx.recv()`（`:204`）只匹配 `Ok`；`RecvError::Lagged`（Err）不匹配该模式，遗漏事件被永久跳过，且无重新同步（仅在连接时发一次 `TasksSnapshot`，`:198-200`）。落后客户端（慢 socket、突发超 256）会静默漏掉事件、视图陈旧到重连。影响低（仅影响可选浏览器桥 UI，不涉核心下载完整性）。

**修复**：显式匹配 `Err(Lagged)` 并重发 `TasksSnapshot` 重新同步；或提高通道容量并文档化上界。

### A-7 已确认仍有效的鲁棒性优势

复核确认仍成立：browser handoff 默认拒绝内网/环回/链路本地（**但纵深不足，见 A-2**）；BT session 引用计数 + `SessionRefGuard` 释放；WAL 启动阈值 + 6 小时后台 checkpoint；状态机集中校验、状态更新与事件写入在事务中（`RETURNING *`）；`emit_task_updated_record` 用 `files_version` 缓存；凭据 OS keyring + ChaCha20-Poly1305 + AAD 绑定 task_id；迁移失败恢复覆盖 `Dirty`/`VersionMissing`、`VersionTooOld` 显式不重建（A-1 迁移恢复经 `tests/migration_integrity.rs` 覆盖，与本节 A-1 调度器问题无关）。

---

## 四、程序运行效率

### E-1 queue-changed 事件触发整页 cursor 重查 + store 重建【🔴 高】（已修复 2026-06-30）

**定位**：`src/hooks/use-task-events.ts:221-244`、`src-tauri/src/scheduler/mod.rs:235`、`:471`、`:623`、`src-tauri/src/download/http/segmented/coordinator.rs:493`

`onQueueChanged` 防抖 100ms 后跑 `listTasksCursor(taskCursorInput(null))`——整首页查询（≤100 行 + totalEstimate + filterOptions），再 `mergeTasksFromServer` + `setTaskCursorPage` 重建内存索引。payload **不带任何变更 task id**，永远是全量重查。后端 `emit_queue_changed` 触发点很多：每次 dispatch（`scheduler/mod.rs:235`）、每次 task start（`:471`）、每次 failure（`mark_download_failed`，`:623`）、每次分段 HTTP 完成（`coordinator.rs:493`），外加 `commands/tasks/actions.rs` 6 处。一批小文件完成或一波 dispatch 就是每约 100ms 一次整页重查 + 全 store 重建。

**影响**：任务列表大、队列频繁变动（大量小下载、重试）时，UI 每约 100ms 重查 DB 并重建整个内存索引，产生与真正变化的单个任务无关的查询负载与 React 重渲染抖动。

**修复**：让 queue-changed 携带变更 task id，做增量 upsert/remove 而非全量重查；和/或后端按批合并发射（每次 dispatch 一次而非每任务一次）并拉长前端防抖。

### E-2 下载中约每秒重查全表 task_stats_snapshot，与本地增量统计冗余【🔴 高】（已修复 2026-06-30）

**定位**：`src/hooks/use-task-events.ts:121-143`、`:170`、`:219`、`src-tauri/src/db/task_records.rs:81-141`、`src/stores/task-data-store.ts:404-479`

`flushProgressBatch` 每次批量进度 flush 都 `scheduleStatsRefresh(1_000)`（`:170`），每个 task-updated 又 `scheduleStatsRefresh(150)`（`:219`）。`getTaskStats` 跑 `task_stats_snapshot`（`task_records.rs:81-141`），是**三个全表查询**：扫全部行的 COUNT/SUM 聚合 + 两个 featured-task 查找。持续下载时整张 tasks 表约每秒被聚合扫一次。这与 `patchTasksBatch` 已维护的增量 delta（`task-data-store.ts:404-468`，`dActive/dQueued/...` 产出 `nextStats`）+ `recalculateTaskStats` 形成三套并存统计源。

> 注：snapshot 确实提供 `all` 与 `featuredTaskId`（增量路径只是沿用而不重算），故非 100% 冗余，但每秒节奏没有依据。

**影响**：有大量历史（数千完成行）的用户，只要在下载就每秒付一次全表 SUM/COUNT + IPC 往返 + store 写，多数数字前端已增量算出。

**修复**：显示总数走本地增量统计，`task_stats_snapshot` 只在粗触发（初始加载、显式刷新、状态变更）时调用以取 `all`/featured 字段；至少把刷新间隔拉到远超 1s，并在无状态实际变化时跳过。

### E-3 DASH/HLS 逐条 INSERT segment 行，无事务【🟡 中】（已修复 2026-07-01）

**定位**：`src-tauri/src/download/dash.rs:829-847`、`src-tauri/src/db/dash.rs:184-199`、`src-tauri/src/download/hls.rs:439-512`、`:360-389`

`run_dash_download` 循环 `for plan in &all_plans { db::upsert_dash_segment(...).await? }`（`dash.rs:829-847`），无外层事务；`upsert_dash_segment` 是各自的 `INSERT...ON CONFLICT` 隐式提交。30 分钟 2s 段的 VOD 每轨约 900 段（视频+音频约 1800 次独立提交）。HLS `persist_hls_segment_plans`（`hls.rs:439-512`）同模式，且在 live 刷新循环中每批新增都调用（`:385`，循环在 `:360`）。

**影响**：清单任务启动（与每次 HLS live 刷新）做成百上千次串行单行提交，在字节开始下载前增加延迟，并在 WAL 下放大磁盘 I/O。

**修复**：用单个 `pool.begin()/commit` 包住 segment-persist 循环，或用 `QueryBuilder` `push_values` 批量 INSERT（`task_records.rs` 已有先例），让 N 段一次提交。

### E-4 每完成一个 segment 跑 3 个 DB 查询，含重复的 get_first_segment_record【🟡 中】（已修复 2026-07-01）

**定位**：`src-tauri/src/download/hls.rs:1208-1250`、`:565-574`、`src-tauri/src/download/dash.rs:1162-1205`、`:948-958`

`emit_hls_progress`（`hls.rs:1208`）/`emit_dash_progress`（`dash.rs:1162`）在下载循环里每完成一段调用一次（`hls.rs:565`、`dash.rs:949`）。每次做 `update_task_progress`、`get_first_segment_record`（每次重解析同一 first-segment id）、`update_segment_runtime_progress`——每段三条 DB 语句。`TaskProgressEmitGate`（`emit_or_store`，`hls.rs:1237`/`dash.rs:1192`）只节流 IPC 事件发射，**不节流这三个 DB 写**，故每段 DB 成本不受限。

**修复**：循环前缓存一次 first segment id（避免重复 `get_first_segment_record`），并把运行时 DB 写按与 IPC 发射相同的时间间隔节流（每约 300ms-1s checkpoint，如分段 HTTP coordinator）。

### E-5 其他确认的局部低效【🟢 低】（已修复 2026-07-01）

- **HLS resume 两次扫 segment 表**（`hls.rs:355-356`）：`existing_hls_downloaded_bytes` 与 `existing_hls_sequences` 各自 `list_hls_segments` 并物化整个 Vec。修复：调一次，从单个 Vec 同时导出两者，或把 SUM 下推到 SQL。
- **DASH segment-plan 构建每段重 parse base URL**（`dash.rs:612`、`:1444`）：`SegmentTemplate` 分支在每段循环里 `resolve_url`，后者每次 `Url::parse(base)`。修复：循环前 parse 一次 base 再 `base.join(relative)`。
- **row_to_task 每行回退解析 error JSON 两次**（`task_records.rs:746,759`）：error_code/recovery_actions 列为 null 时，code 与 actions 各自独立 `from_str::<AppErrorPayload>`。修复：写入时回填两列让回退成死路，并在一行内共享一次反序列化结果。

### E-6 已确认仍有效的效率优势

复核确认仍成立：HTTP client 配置 `connect_timeout`/pool 参数/DNS 缓存；HTTP/FTP/SFTP/Metalink/DASH 多处 `BufWriter 256KB`；完成 rename 后 `sync_all`；调度器一次性构建 host slot map；`transition_task` 减少 DB round trip；任务列表 cursor pagination + virtualizer + overscan；HLS `StreamingAes128CbcDec` 块流式解密（E-1 历史项，内存峰值 ≤ chunk_size + 16，仍有效）；debug-only `seed_scale_tasks` 规模 seed 命令。

---

## 优先级总览

### 🔴 高：功能失效、数据/安全风险或可静默停摆

| 编号 | 问题 | 维度 |
| --- | --- | --- |
| A-1 | start_task 错误分支泄漏下载槽位，可静默饿死调度器 | 架构 |
| A-2 | SSRF 防护可经 DNS rebinding/重定向绕过 | 架构/安全 |
| ~~F-1~~ | ~~BitTorrent 种子率限制存储但从不执行~~ | ~~功能~~ |（已修复 2026-06-30）|
| F-2 | Metalink 并行路径假续传（宣告 supports_resume 但截断 part） | 功能 |
| ~~UX-1~~ | ~~5 种语言约 50% 翻译，自动检测进入半英文界面~~ | ~~UX~~ |（已修复 2026-06-30）|
| E-1 | queue-changed 整页重查 + store 重建 | 效率 |
| E-2 | 下载中每秒全表 task_stats_snapshot 聚合 | 效率 |

### 🟡 中：明显体验或可靠性损耗

| 编号 | 问题 | 维度 |
| --- | --- | --- |
| A-3 | 关闭 flush 共享 3 秒预算串行耗尽 + abort 外层跳过内层清理 | 架构 |
| A-4 | 每任务运行时锁注册表无界增长 | 架构 |
| ~~F-3~~ | ~~Metalink 并行进度被覆盖而非求和~~ | ~~功能~~ |（已修复 2026-06-30）|
| ~~F-4~~ | ~~Metalink 并行下载路径零端到端测试~~ | ~~功能/测试~~ |（已修复 2026-06-30）|
| F-5 | 手动校验仅 SHA-256 | 功能 |
| ~~F-6~~ | ~~HLS 无多音轨/字幕选择~~ | ~~功能~~ |（已修复 2026-06-30）|
| ~~F-7~~ | ~~BitTorrent 无上传限速~~ | ~~功能~~ |（已修复 2026-06-30）|
| F-8 | 无外部自动化 API | 功能 |
| UX-2 | 搜索叠加双防抖约 500ms | UX |
| UX-3 | 无键盘聚焦搜索路径 | UX |
| UX-4 | 空列表无法区分筛选无匹配 vs 无下载 | UX |
| UX-5 | 队列重排仅右键、无键盘、无乐观反馈 | UX |
| ~~E-3~~ | ~~DASH/HLS 逐条 INSERT segment 无事务~~ | ~~效率~~ |（已修复 2026-07-01）|
| ~~E-4~~ | ~~每完成 segment 跑 3 个 DB 查询不受节流~~ | ~~效率~~ |（已修复 2026-07-01）|

### 🟢 低：局部优化

| 编号 | 问题 | 维度 |
| --- | --- | --- |
| A-5 | DNS resolver `.expect()` 可 panic | 架构 |
| A-6 | Realtime 广播 lag 静默丢事件 | 架构 |
| F-9 | 无 PAC 代理 | 功能 |
| F-10 | 完成后整理无模板占位符 | 功能 |
| UX-6 | probe 阶段为 URL 正则静态猜测 | UX |
| UX-7 | 多文件全选无 indeterminate 态 | UX |
| ~~E-5~~ | ~~HLS resume 双扫 / DASH 重 parse URL / row_to_task 双解析~~ | ~~效率~~ |（已修复 2026-07-01）|

---

## 建议修复顺序（按用户影响 × 修复成本）

1. **A-1 调度器槽位泄漏**——单行修复，但可导致整个 app 静默停摆。最优先。
2. **F-1 + F-2 + F-3 协议正确性**——BT 种子率不执行、Metalink 并行假续传/进度乱跳，直接违背宣传能力；同步补 **F-4** 端到端测试，缺陷正源于该路径无覆盖。
3. **A-2 SSRF 纵深**——安全边界，历史标 FIXED 但实际可绕过；在连接层与重定向层补检。
4. **E-1 + E-2 事件刷新效率**——规模化下最明显的卡顿来源，主要为前端 + 轻后端改动。
5. **UX-1 i18n**——限制自动检测到稳定语言（快）或补全翻译（慢）。
6. **A-3/A-4 退出与锁清理、其余 UX/功能补全**按排期推进。（E-3/E-4/E-5 清单 DB 节流已于 2026-07-01 完成。）

---

## 旧结论修正清单

以下历史审计断言经本次代码核实已不再准确，后续排期应以本文为准：

| 旧结论 | 当前核实 | 新状态 |
| --- | --- | --- |
| browser handoff SSRF 已 FIXED | 仅一次性主机名字符串检查；DNS 解析后/重定向每跳不复查；`is_private_ip` 漏 IPv4-mapped/fc00::/fe80::/100.64 | 已修复 2026-06-30，见 A-2 |
| F-2 Metalink 并行已 FIXED | probe 无条件 `supports_resume:true`，并行路径每次截断 part、range 无偏移，仅串行 fallback 续传；并行进度被覆盖；该路径零端到端测试 | 已修复 2026-06-30，见 F-2/F-3/F-4 |
| UX-1 probePhase 阶段反馈已 FIXED | 阶段为 URL 正则静态猜测，probe 期间不推进，普通 HTTP 误标「检查运行时」 | 已修复 2026-06-30，见 UX-6 |
| 调度器无资源泄漏 | `start_task` 非 Conflict 错误分支泄漏 `DownloadControl`，槽位永不释放 | 已修复 2026-06-30，见 A-1 |
| 种子率/做种控制完整 | `seed_ratio_limit` 存储但 `download/` 内无任何读取，做种到关闭程序为止 | 已修复 2026-06-30，见 F-1 |
| `TaskRuntimeLocks` 注释称防无界增长 | 仅 delete 时 evict，完成/失败/暂停任务条目永久驻留 | 已修复 2026-06-30，见 A-4 |
| 退出 flush 可靠 | 共享单个 3 秒预算串行耗尽，追踪外层任务 abort 跳过内层清理 | 已修复 2026-06-30，见 A-3 |

仍确认有效的历史修复见 UX-8 / A-7 / E-6。

## 相关文档

- [project-improvement-audit.md](project-improvement-audit.md)：按发布风险组织的前向清单。
- [ROADMAP.md](ROADMAP.md)：后续路线图。
- [error-codes.md](error-codes.md)：错误码定义。
- [RELEASE.md](RELEASE.md)：发布和 updater 验证。
- [browser-extension-permissions.md](browser-extension-permissions.md)：浏览器扩展权限审查与商店审核回复模板。
