# B7 真实环境与候选包验收矩阵

最后更新：2026-10-13

本文把 B7 的真实协议、系统行为、性能和候选安装包验收拆成可以逐项执行的记录表。它不把本地 fake server 或 CI 结果当成真实环境证据，也不改变主审计中的状态。B3/B4 相关条目在所有外部条件满足前继续保持 `Fixed locally`。

关联计划：[优化计划](optimization-plan-2026-09-29.md)、[主审计](project-improvement-audit.md)、[协议可靠性矩阵](protocol-reliability-matrix.md)、[更新演练](updater-rehearsal.md)。

## 1. 执行基线

每次验收记录都要固定以下信息：

| 字段 | 记录内容 |
| --- | --- |
| 产品版本 | `0.5.0` 或候选包的完整版本号 |
| 源码版本 | `git rev-parse HEAD`；同时记录工作区是否 dirty |
| 构建身份 | `dev`、`candidate` 或 `release` |
| 操作系统 | 版本、架构、WebView/runtime 版本 |
| 硬件 | CPU、内存、磁盘类型；性能场景还要记录网络接口 |
| 数据目录 | 全新目录、既有数据库迁移或恢复数据库 |
| 对端 | 主机类型、协议、地址类别、代理和证书配置 |
| 证据 | 原始日志、截图、任务导出、成品哈希、安装包哈希 |

建议把原始日志和屏幕录制放在本地 `logs/b7/<run-id>/`，把可提交的摘要回填到本文或对应审计条目。不要把密码、Cookie、私钥、完整授权 header 或真实内网拓扑写入仓库。

本轮自动化基线已经完成；重新执行时使用仓库标准入口：

```powershell
pnpm verify
pnpm check:bindings
pnpm verify:protocol-matrix
pnpm test:release-tools
pnpm perf:baseline
```

Rust 在 Windows 上如遇默认并行链接的内存压力，使用仓库已有的低并行方式执行：

```powershell
cargo test --locked --manifest-path src-tauri/Cargo.toml -j 2
cargo clippy --locked --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo fmt --all --manifest-path src-tauri/Cargo.toml -- --check
```

### 本轮本地自动化证据

这些结果只证明共享代码和 fake-server 合同通过，不能替代下面的真实环境场景：

| 测试入口 | 结果 | 覆盖范围 |
| --- | --- | --- |
| `cargo test --locked --manifest-path src-tauri/Cargo.toml -j 2 --test ssrf_engine` | 9/9 通过 | 私网、CGNAT、IPv4-mapped IPv6、字面量 IP、DNS rebinding、FTP 建连前置校验 |
| `cargo test --locked --manifest-path src-tauri/Cargo.toml -j 2 --test directory_probe` | 8/8 通过 | FTP/SFTP/WebDAV 凭据与代理、目录探测取消和预算收敛 |
| `cargo test --locked --manifest-path src-tauri/Cargo.toml -j 2 --test http_engine` | 36/36 通过 | Retry-After、结构化传输错误、未知大小 validator、Range、续传和安全失败 |
| `pnpm perf:baseline` | 1k smoke 通过 | 当前机器 headless 查询基线 |
| `pnpm perf:baseline:10k` | 1k + 10k 通过 | 当前机器规模基线 |
## 2. 结果和关闭规则

每个场景填入以下结果之一：

- `未执行`：尚未具备环境或尚未开始。
- `通过`：步骤、预期和证据全部满足。
- `失败`：行为不满足预期，必须登记复现步骤和影响。
- `阻塞`：外部依赖不可用，但已经记录缺少的条件。

只有对应审计条目的全部验收条件和证据都满足时，才能把主审计中的 `Fixed locally` 改为 `Closed`。真实环境失败时，优先登记新的残项或回归 ID，并保留原始日志；不要用修改文案、放宽安全策略或跳过场景来“通过”。

## 3. B3 内网信任验收

关联条目：`FUN-36`、`SEC-15`。产品边界是“按任务授权并绑定目标”，不建立跨任务的全局私网白名单。

| 场景 | 环境与步骤 | 预期结果 | 结果 / 证据 |
| --- | --- | --- | --- |
| localhost 显式授权 | 手动创建 HTTP 任务，明确授权 `localhost`，完成探测和下载 | 授权任务成功；同一任务的探测、重试、续传使用相同目标范围 | 未执行 |
| localhost 无授权 | 新建任务但不确认授权，访问 `127.0.0.1` 或 `::1` | 在探测前或下载前拒绝；不会静默扩大为全局许可 | 未执行 |
| 真实 NAS | 使用 NAS 主机名和固定地址分别创建任务；测试 HTTP/WebDAV/FTP/SFTP | 仅明确绑定的 authority 和解析地址可用；任务间授权互不复用 | 未执行 |
| Tailscale / 企业 DNS | 使用 Tailscale 地址和企业内网 DNS；重复执行探测、重定向和派生资源请求 | 解析后的目标符合任务授权；客户端缓存不会误用其他任务策略 | 未执行 |
| DNS rebinding | fake DNS 或可控内网 DNS 先返回公网地址，再返回私网地址 | 未经任务授权的解析变化被拒绝；不能借重定向或重解析越界 | 未执行 |
| 公网跳私网 | 公网入口 30x 到私网、localhost、link-local 和 metadata 地址 | 跳转目标逐跳重新执行策略；越界请求拒绝 | 未执行 |
| 恶意清单 | HTTP、HLS、DASH、Metalink、BT `.torrent`、WebDAV 清单引用不同 authority | 子资源不会继承超出任务范围的权限；metadata 和组播目标拒绝 | 未执行 |
| 浏览器 handoff | candidate/release 扩展手动交接内网 URL；分别拒绝授权和同意授权 | 未授权时浏览器原下载保持；授权后桌面任务使用绑定目标 | 未执行 |
| 备份恢复 | 导出任务后恢复到另一数据目录，检查临时授权草稿和已绑定策略 | 临时授权草稿不迁移；任务没有明确授权时回到保守拒绝路径 | 未执行 |

关闭条件：真实 NAS、Tailscale 或企业 DNS 至少各有一组成功证据；拒绝矩阵至少覆盖公网跳私网、DNS rebinding、metadata、link-local 和组播；探测与下载结果一致。

## 4. B4 无人值守恢复验收

关联条目：`FUN-37`、`FUN-38`。所有网络故障场景都要记录断开时间、恢复时间、任务 attempt、最终文件哈希和任务最终状态。

| 场景 | 环境与步骤 | 预期结果 | 结果 / 证据 |
| --- | --- | --- | --- |
| Wi-Fi/VPN 断开 | 下载中断开网络约 2 分钟，再恢复网络 | 任务无人操作自动完成；不超过并发和主机连接预算；最终字节正确 | 未执行 |
| 网络切换 | Wi-Fi 与有线或两个 VPN 接口之间切换 | watcher 按持久化截止时间继续；不需要手动点击“继续” | 未执行 |
| 休眠 / 唤醒 | 任务进入等待或退避后让系统休眠，再唤醒 | 不重复创建 worker；按新的截止时间恢复；用户暂停意图保持 | 未执行 |
| 应用重启 | 任务处于 `retry_after_at` 等待时退出并重启应用 | attempt、原因和截止时间保留；恢复后不会重置预算或提前重试 | 未执行 |
| HTTP 429 秒数 | 可控服务器返回 `Retry-After: <seconds>` | 不早于服务端指定时间重试；任务诊断包含结构化等待信息 | 未执行 |
| HTTP 429/5xx HTTP-date | 返回合法 RFC 1123 日期，再测试过期日期 | 合法日期按绝对时间等待；过期日期不会造成异常长等待 | 未执行 |
| 认证失败 | 401/403 或凭据失效 | 不进入任务级自动重试循环；进入明确的人工恢复路径 | 未执行 |
| 磁盘失败 | 写入目录不可用或磁盘空间不足 | 不自动重试；保留可诊断失败状态和恢复入口 | 未执行 |
| TLS / 远端身份变化 | 证书错误、ETag 变化、Last-Modified 变化 | 不盲目重试或拼接旧数据；要求人工处理或从零开始 | 未执行 |
| 未知大小安全续传 | 依次测试强 ETag、合法 `Last-Modified`、弱 ETag、缺失 validator、错误 Range、无 Range | 只有可靠身份和精确 206/Content-Range 才追加；其他情况从字节零安全重试 | 未执行 |
| 畸形响应体 | 断开的 chunked 响应、解码错误、代理认证错误 | 不被字符串分类为可恢复传输中断；不消耗自动重试预算 | 未执行 |

关闭条件：至少完成一次真实网络中断恢复、一次应用重启恢复、两种 `Retry-After`、一种认证或磁盘失败，以及一组真实 CDN/代理未知大小响应矩阵。所有任务最终文件必须通过哈希或逐字节校验。

## 5. B1 生命周期与 B2 运行时策略验收

关联条目：`ARC-59`、`ARC-60`、`ARC-62`、`ARC-63`、`FUN-34`、`ARC-64`、`ARC-65`、`FUN-35`。

| 场景 | 环境与步骤 | 预期结果 | 结果 / 证据 |
| --- | --- | --- | --- |
| 慢盘暂停 | 使用可控慢写入目录，下载中暂停并等待写入排空 | 最多一个写者；暂停后不继续写；任务进入稳定暂停状态 | 未执行 |
| 慢盘取消 / 删除 | 写入阻塞时取消并删除任务 | 先停止和 join，再删除文件与记录；超时不会清理仍由 owner 持有的文件 | 未执行 |
| BT 错误退出 | 让 BT storage 或 metadata 路径产生真实错误 | 任务进入失败或人工处理状态；下载槽、session 和临时资源释放 | 未执行 |
| 退出 / 更新 | 活动下载时退出应用或执行 updater rehearsal | 先排空 worker、ffmpeg 和存储写入，再退出或安装；不会留下幽灵任务 | 未执行 |
| 单任务限速 | 运行 HTTP、FTP、SFTP、HLS/DASH 和 BT，修改任务限速 | 修改后约 2 秒内收敛；清除限速立即生效 | 未执行 |
| 计划窗口 | 在窗口开始和结束前后运行任务；修改计划设置并唤醒系统 | 计划边界和配置变化触发重算；不需要重启任务 | 未执行 |
| HTTP + BT 混跑 | 高速 HTTP 与两个 BT 任务同时下载，动态修改全局限速 | 所有协议共同消费同一全局预算；逐任务限速仍独立有效 | 未执行 |
| 同值设置更新 | 连续快速设置、清除、恢复默认和窗口切换 | 不重置已经消耗的 burst；最终 UI、数据库和运行时策略一致 | 未执行 |

## 6. 候选安装包和升级验收

候选包流程沿用 [updater-rehearsal.md](updater-rehearsal.md)。每个平台都要保存安装包哈希、签名验证结果、版本页面、更新日志和重启后的任务数据库摘要。

| 阶段 | 步骤 | 预期结果 | 结果 / 证据 |
| --- | --- | --- | --- |
| 构建 | 本机以 `VIBE_BROWSER_PROFILE=candidate` 和独立 identity 构建／修复后重封装，命令见 6.3 | Windows 主程序、MSI 和 NSIS 均生成；本机构建不产生 updater artifacts | 通过（本机）— 2026-10-13 图标及 bin 边界修复已纳入；MSI `5bc92003…`，NSIS `5eb56de4…`；完整哈希、失败重试和覆盖范围见 6.3 |
| 首次安装 | 安装 `rc.0`，创建一个已完成任务和一个暂停任务 | 应用能启动；Native Messaging sidecar 和 manifest 路径存在 | 未执行；MSI 管理员解包尝试因 Windows Installer 服务超过 3 分钟无返回而停止 |
| 更新检查 | 在 `rc.0` 中检查 `rc.1` | 只指向候选 endpoint；签名和版本校验通过 | 未执行 |
| 更新重启 | 下载并安装 `rc.1`，允许应用重新启动 | 运行版本为 `rc.1`；任务、设置和数据库迁移结果保持正确 | 未执行 |
| 浏览器交接 | 更新后执行一次手动 handoff | 只创建一个新任务；manifest 仍指向存在的 sidecar | 未执行 |
| 旧库恢复 | 使用旧版本数据库启动候选包，再执行备份恢复 | 迁移成功；临时授权草稿和敏感数据边界符合 B3 合同 | 未执行 |
| 卸载 / 重装 | 卸载并重新安装，按产品约定检查数据目录 | 安装器行为可解释；不会静默删除用户下载文件或破坏备份 | 未执行（NSIS 隔离 smoke 已在 6.1 记录；正式数据目录仍待验收） |

### 6.1. 2026-10-12 Windows 本机候选 smoke

本次只使用独立的 `com.vibe.downloader.b7.windows-20261012` 应用标识和数据目录；工作区 dirty，构建身份为 candidate，updater artifacts 与代码签名均关闭。证据目录为 `logs/b7/windows-20261012/`（本地忽略目录）。

| 检查 | 结果 / 证据 |
| --- | --- |
| 构建与包 | Windows x64 主程序构建通过；NSIS `Vibe Downloader B7_0.5.0_x64-setup.exe` SHA-256 `43D9D7069A4181CB5A227B42030DD3C7621E5BC8E144626C4D3312AB0BBA0987`；MSI `Vibe Downloader B7_0.5.0_x64_en-US.msi` SHA-256 `C18852AB91532E682E405BC7FF0B87FE95CDBCEB6F015A1AA6C2EC17F56F954E` |
| 主程序／host | 主程序 SHA-256 `65E212950EDECD881235D03FD46122BCB6E050D0D77201856464CEF1EE7DDC75`；构建目录 host SHA-256 `50691B61CD8AB4CE2C0C8494A4FDBD49CF25136B026051F5A6004AB2A3182595`（当时未单独记录包内 staged sidecar 哈希）；`vibe-native-host --self-check` 与应用内 native-host self-check 均返回 `ok=true`、protocol 1 |
| 隔离启动 | Tauri startup `ready`；migration 最大版本从 13 自动迁移到 15；原完成／暂停任务各 1 条仍在 UI 与 DB 中可见；页面通过 WebView2 CDP 端口 19422 读取，未改动日常应用数据 |
| 配置与备份 | `auto_resume_on_startup`、剪贴板、自动更新检查、通知和完成动作均为关闭／none；创建并验证 schema 15 备份，任务 2 条、凭据 0 条、路径违规 0 条 |
| 关闭 | 通过 Tauri tray quit IPC 正常退出，进程在 30 秒内结束 |
| NSIS 隔离安装／卸载 | 通过（本机）：在 `logs/b7/windows-20261012/installed-nsis-run2/` 使用 `/S /D=<隔离目录>` 安装；从安装目录启动并运行同一 smoke，再执行卸载器 `/S`。主程序、Native Host、卸载器和扩展资源落盘；启动与退出正常；安装与卸载均返回 `0`，隔离目录已移除 |

以上是本机候选 smoke，不是 MSI 安装器服务、签名、`rc.0 → rc.1` updater 或跨平台证据。签名密钥和 Windows 代码签名配置在本机不可用，因此首次安装（正式候选数据目录）、更新、正式卸载和生产签名项仍为 `未执行`。

当前候选包仍不能宣称为 OS 代码签名生产分发；签名状态必须在证据中单独记录。

### 6.2. 2026-10-13 Windows 候选刷新与原生 UI 基线

本轮在 `f87c2007e52d026265472e2f2efd6c80ab3d3642` 的 dirty 工作区重建，纳入 10 月 12 日最后的 IPC／快照／HLS 边界加固。identity 为 `com.vibe.downloader.b7.windows-20261013-ui`，release 使用 `opt-level="s"`、LTO、`--locked -j 2`；无 OS 签名、无 updater artifacts。归档文件和完整指纹在 `logs/b7/windows-20261013-ui/packages/`、`candidate-artifacts.json`；产品源码／manifest 共 393 文件的 SHA-256 指纹为 `6275d86043a1e38982c2dd29c570aefa74e878527dce13ff6ed696777a87d732`。

```powershell
$env:VIBE_BROWSER_PROFILE = 'candidate'
$env:CARGO_BUILD_JOBS = '2'
node node_modules/@tauri-apps/cli/tauri.js build --target x86_64-pc-windows-msvc --config logs/b7/windows-20261013-ui/tauri.b7.generated.json --bundles nsis msi -- --locked -j 2
```

| 文件 | SHA-256 |
| --- | --- |
| 主程序（目标目录，成功封装后 CLI 恢复未打包类型 UNK） | `33e0f145d836936437394eb40b8579c6d2bec100a37dd5e2afc89642a43281ff` |
| NSIS 安装后主程序（本轮 UI 采样对象） | `579976d2011389513c23ae3b9428aee4e332f950de1edff02c446f17c0c1d0d5` |
| 包内 staged Native Host | `91a93f0c02b008fed6f20ed5431c4f589d75afd612aed4665651b64e817ed91f` |
| Cargo 输出的另一份 Native Host（独立记录，不作为包内哈希） | `af20bdc16dc7a2e950eef4c43ee261c854511edd160e36b26e7d120f038e1065` |
| NSIS | `6cad82ec3ac43718cf9635851b0adb97a62c76674c4a93893ffabba35fc1e55a` |
| MSI | `aceef7d2115a17708f0c5e36e6e1233f27647c584dd82ccc95257cc44b69ee53` |

NSIS 使用 `/S /D=<工作区隔离目录>` 安装到 `logs/b7/windows-20261013-ui/installed-nsis/`，安装返回 `0`；包内 Native Host `--self-check` 返回 `ok=true`、version `0.5.0`、protocol `1`。MSI 本轮仅构建，实际安装／卸载仍未执行。NSIS 还包含 24,207,872 字节的 `export-bindings.exe`，登记为 `PERF-21` 包装待办，不能据此宣称生产分发已收口。

原生采集使用 `pnpm perf:native-ui`、Windows PowerShell 5.1、Node 22.23.2 和 WebView2 `Edg/156.0.4314.15`，可见 viewport 为 `1200×830`、DPR `1.25`、英文／comfortable 行密度。Codex 的 Windows MSIX 宿主会把新建的 Roaming 子目录虚拟化到宿主 `LocalCache/Roaming`；fixture 同时记录请求路径与实际路径，换库保持 identity、目录所有权、退出检查、WAL 检查点和 SQLite integrity_check。此次只操作该测试 profile，不代表脱离 MSIX 宿主后的标准用户数据目录验收。

正式 1k／10k、每组 5 次测量启动与鼠标／方向键选择／搜索已完成；每组另采集 10 秒滚动、5 分钟空闲、11 个进程快照。每次退出前记录的 7 个主程序／WebView2 进程均结束，数据见 [性能摘要](performance-baseline-results.md#4-2026-10-13-windows-原生-release-ui-短基线)，原始 JSON 为 `native-ui/2026-10-12T18-03-07-276Z/native-ui.json`。启动非真实冷缓存，滚动不是 compositor FPS；短跑修正宿主目录虚拟化、PowerShell 5.1 属性统计与已退出进程的提交 WAL 恢复，失败报告保留，未当作性能样本。

最后 smoke 验证 10,000 任务（完成 5,000、暂停 4,000、失败 1,000，活动／排队为 0），包内 Native Host 与应用内自检通过，capture／自动拦截／请求头转发关闭；完整备份 schema 15 校验通过、凭据 0、路径违规 0。`final-native-smoke.json` 同时确认 PDF 系统图标 PNG 本身有效，原生图片加载却为 `naturalWidth=0`；CSP／图片错误回退登记为 `UX-51`，不宣称候选 UI 全面验收通过。

NSIS `/S` 卸载返回 `0`，隔离安装目录已移除，原始记录为 `nsis-uninstall.json`；测试 profile、备份与原始证据保留。`Get-AuthenticodeSignature` 对归档主程序、host 与两个安装包均为 `NotSigned`，记录在 `authenticode.json`。本机通过范围不含 MSI 安装服务、候选 updater、真实下载对端、系统电源动作或其他平台。

采集脚本最后加固后以同一 NSIS 再安装／短跑／卸载确认：安装后主程序和 host 实测哈希与表中值一致（`installed-hashes.json`）；1k／10k 各 1 次的短跑结果在 `native-ui/2026-10-12T18-39-01-965Z/`，仅作脚本复核；最终卸载为 `nsis-uninstall-final.json`。宿主元数据补录 Windows `10.0.28020`／NTFS，位于 `host-metadata/metadata.json`（采集于基线后）。

### 6.3. 2026-10-13 图标与二进制包装修复验收

独立 identity `com.vibe.downloader.b7.windows-20261013-fixes`，HEAD `f87c2007e52d026265472e2f2efd6c80ab3d3642`、dirty 工作区；产品源码／manifest 393 文件指纹 `e6f91d32949a6a2902711f6d7e7ee314b1032a4900421469722765f1ca67ef7a`。证据为 `logs/b7/windows-20261013-fixes/`，配置、包、原始失败日志、恢复记录与截图都保留；本轮仍使用 Codex Windows MSIX 的独立测试数据目录，不代表普通生产 Roaming 目录验收。

最初的完整 `tauri build ... --bundles nsis msi -- --locked -j 2` 已生成主程序、host 和 NSIS，却在 MSI 的 WiX ICE30 失败：普通 Cargo bin 和 externalBin 将两份不同的 host 安装到同一位置。修复为 exporter／host 分别 feature-gated，产品默认只选主程序，host 只经 externalBin 纳入；CI 包检查和 host 单测／lint feature 同步调整。没有抑制 ICE 错误。最后修改仅是 bin 选取和构建／验证流程，主程序 Rust／前端源码与默认 enabled features 仍相同，重新构建 featured staged host 后复用该主程序重封装：

```powershell
$env:VIBE_BROWSER_PROFILE = 'candidate'
$env:CARGO_BUILD_JOBS = '2'
node scripts/native-host-build.mjs --target x86_64-pc-windows-msvc
node node_modules/@tauri-apps/cli/tauri.js bundle --verbose --target x86_64-pc-windows-msvc --config logs/b7/windows-20261013-fixes/tauri.b7.generated.json --bundles nsis msi
```

失败的 MSI 封装遗留类型标识，复用前已确认主程序 `NotSigned`，只恢复 SDK 标记中的 3 字节，并保留原文件／哈希；`bundle-marker-recovery.json` 记录具体位置和 SDK 来源。成功封装后 CLI 恢复原始 UNK 类型；NSIS 实际安装后主程序与归档未打包主程序的差异仅为 SDK 的 NSS 标记（`archive-installed-main.mjs` 验证）。首次编译 host／主程序分别为 10m27s／16m30s；最终 featured host staging 为 9m47s。缓存、metadata 和编译选择均有差异，这些不是单变量的编译耗时对照。

| 文件 | SHA-256 |
| --- | --- |
| 未打包主程序（UNK） | `95a913ba3e387d578916ab7ee58d40d8492d899132f544768461deca5c9748d3` |
| NSIS 安装后主程序（NSS，原生验收对象） | `5c11b37223ac05036fd0a3ba1e6774aba283180de1dc7b9d3d192511483fb664` |
| staged／NSIS 安装后 Native Host | `7c6c30b5490ad7b5deb6bac1d4b1731460bccbd5125a6e1dd5e932a5e574774f` |
| NSIS | `5eb56de4dfc18656ff3a3b599043d02ee589a1cc945df626f5b7a54123c84e91` |
| MSI | `5bc920031048911830714cc59d6718905c3e3d193980147ebf9e875d7a17b7a5` |

| 检查 | 结果／证据 |
| --- | --- |
| bin 与安装内容 | 最终 MSI File 表仅主程序与一份 host，无 exporter；NSIS 安装清单为主程序、一份 host、卸载器及扩展资源。旧 exporter（24,207,872 字节）仍在构建目录，证明没有靠清目录掩盖自动打包。`msi-file-table.json`／`nsis-install.json`／`stale-exporter-retained.json` |
| 体积 | NSIS 21,006,928 → 14,050,409 字节（减少 33.12%）；MSI 28,626,944 → 19,091,456 字节（减少 33.31%），对照 6.2 快照。包内开发工具的约 23.1 MiB 安装占用已移除；不能据此推出运行性能变化。`package-size-comparison.json` |
| 安装／host | NSIS `/S /D=<工作区隔离目录>` 返回 `0`；安装后 host 哈希与 staged 相同。包内 `--self-check` 和应用内自检均 `ok=true`、version `0.5.0`、protocol `1` |
| 原生图标 | 列表 4 个 PNG data URL 均 `complete=true`、40×40；`native-icons-desktop.png` 已人工查看 |
| 原生坏图回退 | 强制无效 PNG 后图片节点移除、通用 SVG 存在；选行重渲染仍保持回退，新增提取 IPC 为 `0`。`native-icons-fallback.png`／`native-fixes-smoke.json` |
| CSP | 内联脚本未执行，收到 `script-src-elem`／`connect-src` 的拒绝事件；仅 `img-src` 增加 `data:`，其他配置指令保留。完整原生 policy 在 smoke JSON |
| 窄窗口 | 500px viewport 与 scrollWidth 均 500，无破图；`native-icons-narrow.png` 已人工查看 |
| 数据／功能 smoke | startup `ready`，10k 中完成 5k／暂停 4k／失败 1k，活动／排队为 0；capture、自动拦截、转发均关闭。1k／10k 各 1 次启动、鼠标／键盘／搜索短跑通过，原始记录为 `native-ui/2026-10-13T08-21-33-830Z/`；`--steady-seconds 0` 仅作功能检查，不替换 6.2 的正式五次性能基线 |
| 退出／卸载 | 功能短跑的各次退出检查通过；最终 smoke 记录的 7 个主程序／WebView2 进程均结束。NSIS 卸载返回 `0`，安装目录移除；测试 profile 与证据保留。`nsis-uninstall.json` |
| 签名／范围 | 所有归档主程序、host、NSIS／MSI 均 `NotSigned`；updater artifacts 关闭。MSI 实际安装／卸载、候选升级、真实协议、其他平台及电源动作继续未执行 |

本轮 Windows dirty 工作区的前端门禁为 `pnpm typecheck`、`pnpm test:frontend`（83 文件／484 测试）、`pnpm build`、`pnpm test:release-tools`（89 JS／19 i18n 工具）、bundle、candidate 扩展、稳定语言与文档检查通过。Rust 在最后 bin 选取调整前运行 `cargo fmt --all --manifest-path src-tauri/Cargo.toml -- --check`、`cargo clippy --locked --manifest-path src-tauri/Cargo.toml --all-targets -j 2 -- -D warnings`、`cargo test --locked --manifest-path src-tauri/Cargo.toml -j 2`：库 386、全部集成／文档通过，3 个基准 ignored；Rust 源码此后未变。最终 feature 追加执行 `cargo clippy --locked --manifest-path src-tauri/Cargo.toml --features native-host --all-targets -j 2 -- -D warnings`、`cargo test --locked --manifest-path src-tauri/Cargo.toml --features native-host --bin vibe-native-host -j 2`（7/7），实际 helper release 构建与重新封装通过。`pnpm check:bindings` 最终执行导出成功、生成 SHA-256 与修复前一致，但因既有 B5/B6 未提交 diff 返回 `1`，不能把该命令写为通过。远程 CI 未运行。

提交准备阶段，已复核的 B5/B6 生成变更纳入暂存基线后，在 Windows dirty 工作区重新执行 `pnpm check:bindings` 返回 `0`，SHA-256 仍为 `63FAB63E183382B287165D196EEED4F8A15A315B822C1E3907BF7780AB7D1998`；没有手改生成文件。上段返回 `1` 的记录保留为暂存前快照。

WiX 的 ICE30 已消失；SDK 模板仍有 ICE03／40／57／61 警告，完整输出见 `candidate-rebundle.log`，留给实际 MSI 安装／升级场景复核。初次编译的 `STATIC_VCRUNTIME` 弃用提示也保留；没有修改这些发布参数或用禁用校验获取成功结果。`UX-51`／`PERF-21` 更新为 Fixed locally，B7 其他缺失环境不随这次本机修复关闭。

## 7. 性能和长跑基线

先测量再决定优化。每个数据集至少记录冷启动、暖启动、任务数量、CPU、RSS、磁盘写入量、句柄/线程和网络吞吐。

| 项目 | 最小场景 | 产出 |
| --- | --- | --- |
| Windows 预分配 | NTFS、4/8 段大文件，预分配/不预分配/稀疏对照 | 首写 p95、总耗时、实际写入量 |
| 检查点开销 | 多任务并发、WAL、慢盘和快速盘 | 事务等待、BUSY、CPU、进度延迟 |
| 创建和查询规模 | 1k/10k 活动任务，创建、列表、搜索、筛选 | p50/p95、内存、查询计划 |
| 空闲开销 | 限速开关、剪贴板开关、托盘隐藏各运行 1 小时 | 唤醒次数、DB 查询、CPU、RSS |
| 依赖和 release 参数 | `opt-level=s` 与 `3` 的构建和运行对照 | 构建耗时、包大小、启动和热点吞吐 |
| 协议长跑 | HTTP/HLS/BT 混合 30 分钟初筛，具备条件时 8 小时 | CPU、RSS、连接、临时文件、最终文件哈希 |

性能数字在固定机器和固定版本下记录；没有实测数据时不修改性能预算，也不把 loopback 结果当作公网体验。

## 8. 完成后的审计更新

每个场景完成后，把证据路径和结论回填到对应审计条目：

1. 所有验收条件通过：将对应条目从 `Fixed locally` 更新为 `Closed`，保留本地自动化和真实环境证据。
2. 只有部分环境通过：继续保持 `Fixed locally`，明确列出缺失环境。
3. 发现回归：保留原条目历史记录，登记新的回归 ID、影响、复现和修复验收条件。
4. 不因为一次候选包成功就关闭未覆盖的协议、平台或安全路径。

当前按用户选择先完成本机候选包和性能验收；具备真实测试源与候选签名条件后，再执行第 3／4 节的内网授权、断网恢复和升级场景。B5 的请求头和后台工作流实现与此矩阵并行，不用新功能掩盖现有 Fixed locally 条目的外部证据缺口。

## 9. B5 增强候选包与真实环境场景

关联条目：`UX-50`。本地 SQLite／前端自动化和浏览器 mock 只证明合同实现，不改变下面的未执行状态。

| 场景 | 环境与步骤 | 预期结果 | 结果 / 证据 |
| --- | --- | --- | --- |
| 配置库升级与重启 | 旧数据库升级到含 migration 014 的候选包；保存／更新／删除预设、站点配置、视图后重启 | 原任务／设置保留；命名配置和删除结果持久化，IPC 不报错 | 未执行 |
| 完整备份恢复 | 保存命名配置后完整备份，恢复到另一数据目录；核对目录、代理与授权 | 配置库随库恢复；实际创建继续验证本机路径、代理和任务级目标授权，不携带预设凭据 | 未执行 |
| 兼容读取 | 在复制的测试数据库中加入未来格式或损坏配置，再修改其他有效记录 | 显示不可应用提示；有效记录可用，未来／损坏记录不被覆写 | 未执行 |
| 站点配置请求 | 可控真实 HTTP/HTTPS 对端核对 UA／Referer／允许的公开头；改变协议、端口并测试跨源重定向 | 精确 origin 才可手动应用；Cookie／X-* 不进入站点库；任务独有敏感头仍遵循原 TTL 和跨源剥离规则 | 未执行 |
| 预设创建 | 单任务和批量应用同一预设，随后手动覆盖；从任务详情保存并再次应用 | 创建参数与选择一致；应用不改变旧任务；无效路径／代理正常拒绝，不自动直连或授予内网权限 | 未执行 |
| 批量运行策略 | 100 个运行／暂停／失败／完成任务混合，修改部分字段，期间完成／删除任务或停止批次 | 只改指定字段；逐项成功／跳过／失败准确，限速按共享策略收敛；计划／优先级生效方式与单任务一致，重试不重放成功项 | 未执行 |
| 动态视图与键盘 | 快速切换视图并创建／完成任务；Windows/macOS/Linux 的输入、菜单和弹层内外测试快捷键，浅色／暗色及窄窗口走查 | 查询动态更新、旧页不回写、选择清除；各入口提示和实际按键一致，无误操作或裁切 | 未执行 |

## 10. B6 真实做种与媒体候选包场景

关联条目：`FUN-40`（`R27-F06`）、`FUN-41`（`R27-F07`）、`PERF-20`（`R27-P02`）。本地 fixture、ffprobe、SQLite、组件／axe 测试和短时隔离 session 基准单独登记到 [B6 实施记录](optimization-plan-2026-09-29.md#22-2026-10-10-b6-实施记录)，不替代本节的真实环境结果。

| 场景 | 环境与步骤 | 预期结果 | 结果 / 证据 |
| --- | --- | --- | --- |
| 两路下载＋两路做种 | 可控真实 BT peer 持续下载／上传，另运行两个 HTTP 下载；修改全局与任务限速 | 两个后台做种不占普通下载槽；BT／HTTP 共享下载预算，逐任务限速仍有效，上传总预算动态收敛 | 未执行 |
| 完成后启停与文件校验 | 完成单／多文件 torrent，停止后再次开启；删除文件、改同尺寸内容、替换目录链接或移走缓存元数据 | 有效文件开始／停止可控；损坏或元数据缺失稳定拒绝；一个 session 停止不影响其他任务 | 未执行 |
| 重启与限制 | 分别开／关自动恢复，手动停止；设置比例／时间限制，上传后关闭再启动 | 只恢复此前明确启用且未手动停止、文件完整、未到限制的任务；累计上传与时长单调，达到限制停止 | 未执行 |
| 删除、取消与退出排空 | 做种中删除／取消，慢盘或低上传限速下退出、系统关闭和安装更新 | session／文件 I/O 已 join 才释放文件／退出；超时仍显示停止待完成；取消更新后应用保持可用 | 未执行 |
| 完成动作与无限做种 | 分别选择通知／打开目录及退出／关机／睡眠／休眠／运行命令；做种不设上限 | 即时动作可在下载完成触发；破坏性动作等待并有可见提示和停止入口，不重复执行 | 未执行 |
| 2/5/10 torrent 长跑 | 固定机器、真实 peer、不同代理任务并行；采集 30 分钟初筛与 8 小时长跑 RSS、WAL、写次数、线程、句柄、DHT 和端口 | 逐任务代理／限速／删除隔离，关闭后端口与文件释放；实测后才评估共享 session | 未执行 |
| HLS WebVTT 成品 | 真实多语言 WebVTT、时间映射／重复 cue、外部音频和直播字幕；保存 ffprobe JSON 并实际播放 | MP4 `mov_text` 字幕与 ISO 语言正确，选中的音轨保留，字幕时间与播放一致；不支持输入稳定拒绝 | 未执行 |
| 直播缺片与离线收尾 | 默认 0 与有限上限；故意 404／断流并耗尽重试，空闲后断网点击“完成录制” | 默认失败；预算内继续且保存序号／时间／原因；超额失败；已下载片段可离线收尾，不重新请求源站 | 未执行 |
| 缺少 ffmpeg | 候选安装环境移除 PATH／设置／环境 ffmpeg；单路 TS、init map fMP4、外部音频／字幕和 DASH 对照 | 经包与单节目 PAT／PMT 验证的 TS 输出 `.ts` 可播放；其他组合明确依赖错误，不发布半成品 | 未执行 |
| DASH 选择与恢复 | 真实静态多画质／多语言音频／WebVTT，手选不同默认项，暂停并重启 | catalog 与实际源一致；原 ID 恢复，未选轨道不下载；ffprobe 和实际播放验证音视频／字幕／语言 | 未执行 |
| DASH 边界拒绝 | DRM、TTML、fMP4 `wvtt`、live、多 Period、SegmentTimeline 与不能安全展开的模板 | 稳定错误及可查诊断，无隐式选另一轨道或未经验证直接拼接 | 未执行 |
| 无后缀与网络边界 | Content-Type 清单及 octet-stream 前缀，忽略 Range／慢流／HTML；凭据、代理、跨源重定向和私网目标 | 前缀最多 4 KiB 并有截止；不做网页嗅探；敏感头剥离与 task 网络授权／SSRF 拦截保留 | 未执行 |
| 迁移、恢复与候选 UI | 旧库升级 migration 015，完整／选择性备份恢复；Windows/macOS/Linux、浅色／暗色、窄窗口和键盘 | 做种／DASH 选择／HLS 缺口保留，恢复不激活外机内网授权；控件、错误翻译、焦点和等待入口可用 | 未执行 |
