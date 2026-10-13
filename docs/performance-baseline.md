# 性能基线

最后更新：2026-07-19

适用版本：Vibe Downloader `0.3.0`

状态：已落地可重复 headless harness（1k CI smoke + 可选 10k），并完成本机 1k/10k 实测摘要，见 [performance-baseline-results.md](performance-baseline-results.md)。50k+、UI 冷启动/FPS、HLS/BT 长跑与 CI 绝对数值门禁仍未建立。

本文只记录可重复的性能测量。当前风险优先级见 [project-improvement-audit.md](project-improvement-audit.md) 的 `PERF-01` 至 `PERF-11`；没有硬件、OS、构建模式、数据分布和重复次数的单次数字不能作为性能结论。

## 1. 当前已知事实

- 前端使用游标分页和 `@tanstack/react-virtual`，避免首次加载和渲染全部历史任务。
- Rust 进度事件由 `TaskProgressEmitGate` 限制到至少 250ms，前端按批次应用进度更新。
- SQLite 使用 WAL；request diagnostics 已有按年龄和每任务条数的清理策略。
- HTTP client 按全局代理 fingerprint 复用，但逐任务代理 client 仍未贯通，不能用当前 client cache 数据评价最终设计。
- HLS AES-CBC 主路径采用流式解密，不需要同时保留完整 ciphertext 和 plaintext。
- 2026-07-18 至 2026-07-19 的验证中，421 项 Rust 测试和 65 项前端测试通过，typecheck、build、cargo check 和严格 Clippy 通过；这些结果证明功能回归可运行，不代表性能达标。

当前没有以下实测数据：

- 50k、100k 或更大任务库的冷启动与首屏时间（1k/10k 的 headless search/filter/list 已有，见 results）。
- 搜索、筛选在 50k+ 上的 p50/p95（1k/10k 已记录）。
- 长时间 HLS、BT、事件表、WAL、缓存和文件句柄增长曲线。
- 100 个活动任务的 CPU、RSS、DB write rate、事件率和 UI FPS。
- 1k 文件删除、1k 文件 torrent 和大量分段任务的响应时间。
- release `opt-level="s"` 与 `opt-level="3"` 在 hash、AES、XML 和 BT 热点上的对比。
- 前端 chunk 的 raw、gzip、brotli 预算及其与启动时间的关系。
- release UI：冷启动、滚动 FPS、稳态 RSS（清单已写入 results，数值待测）。

## 2. 当前待测热点

| Audit ID | 热点 | 测量重点 |
| --- | --- | --- |
| `PERF-01` | 三字段 `LOWER(...) LIKE '%term%'` | query plan、p50/p95、输入取消和 DB pool 占用 |
| `PERF-02` | TaskDetails 非相关 tab 轮询 segments | IPC 次数、重叠请求数、DB 查询时间 |
| `PERF-03` | 每批进度构造全任务 Map | loaded task 数量与 CPU/GC 的关系 |
| `ARC-13` | BT task progress 复制完整 files | 100、1k、10k 文件的 payload 和 heap |
| `PERF-04` | HLS key/init-map 重复请求与竞争 | 请求次数、磁盘操作、并发 worker 数 |
| `PERF-05` | files-version cache 和 task events 保留 | 24h/7d 内存、行数、WAL 大小 |
| `PERF-06` | async 热路径同步文件系统调用 | 慢盘/网络盘下 runtime stall |
| `PERF-07` | 用户完成命令同步等待 | 挂起命令对 Tokio worker 和调度的影响 |
| `PERF-08` | SystemTime token refill 和 CAS 竞争 | 时钟变化、连接公平性、吞吐误差 |
| `PERF-09` | release profile 尺寸优化 | 二进制体积、启动、hash/AES/XML/BT 吞吐 |
| `PERF-10` | 无 bundle budget | raw/gzip/brotli 体积、解析、执行和首交互时间 |

## 3. 数据生成工具

Debug 构建暴露 `seed_scale_tasks`，用于生成任务、文件、work unit 和 request diagnostics。该命令必须保持 debug-only，不能注册到 release 构建。

输入模型：

```ts
type ScaleSeedInput = {
  queued: number;
  downloading: number;
  completed: number;
  failed: number;
};
```

调用约定：

- `clearBefore = true`：先清空现有任务，再生成数据。
- `clearBefore = false`：追加数据。
- 总任务数硬上限为 50,000；更大规模测试需要专用 fixture 或提升工具上限，并记录修改。
- 每批插入应保持事务化和确定性分布，避免数据生成时间主导应用测量。

示例：

```ts
await window.__TAURI__.seedScaleTasks(
  { queued: 2000, downloading: 2000, completed: 5000, failed: 1000 },
  true,
);
```

相关测试：[`src-tauri/tests/scale_seed.rs`](../src-tauri/tests/scale_seed.rs)。

## 4. 测试矩阵

每个规模至少重复 5 次，冷启动取中位数并保留全部原始样本；持续运行场景至少运行 30 分钟，发布候选应补 8 小时 soak。

| 场景 | 数据规模 | 指标 |
| --- | --- | --- |
| 冷启动和首屏 | 0、1k、10k、50k tasks | process start、first paint、可交互、首个任务页 IPC、RSS |
| 搜索 | 10k、50k、100k tasks | p50/p95、query plan、取消旧请求、主线程长任务 |
| 筛选和排序 | 10k、50k tasks | p50/p95、payload、临时排序、cursor 正确性 |
| 连续滚动 | 50k tasks | 平均/最低 FPS、长任务、heap、DOM 节点数 |
| 详情页 | 1k tasks，含 diagnostics | tab 首开、轮询次数、重叠请求、关闭后清理 |
| 活动下载 | 10、50、100 mixed tasks | CPU、RSS、网络、DB writes/s、events/s、UI FPS |
| 大 torrent | 100、1k、10k files | snapshot 大小、per-file 更新、heap、渲染时间 |
| 长 HLS/BT | 30min、8h | RSS、句柄、task events、WAL、临时文件、子进程 |
| 批量删除 | 100、1k tasks/files | 总耗时、UI 响应、部分失败、取消和磁盘队列 |
| Hash/AES/XML | 1GB fixture/大型 manifest | MB/s、CPU、峰值 RSS、profile 对比 |

## 5. 测量方法

### 5.1 构建模式

至少记录以下三种模式，不能把 dev 数据当作 release 数据：

1. `pnpm tauri dev`，用于定位和 DevTools trace。
2. 未签名 release candidate，用于真实启动、CPU 和 RSS。
3. 对照 profile，仅用于 `opt-level` 或特定优化实验。

### 5.2 冷启动

1. 为每个规模准备固定数据库快照。
2. 完全退出应用，确认无残留 worker 或 WebView 进程。
3. 从进程创建开始计时，记录窗口首次显示、首屏任务出现和输入可响应时间。
4. 记录 SQLite 文件、WAL 大小、RSS 和 CPU 峰值。
5. 每个规模运行 5 次；分别记录首个进程启动、后续启动与实际缓存条件，未清空缓存时不称作冷缓存启动。

### 5.3 搜索、筛选和排序

1. 保存 `EXPLAIN QUERY PLAN`。
2. 使用短、高频和无匹配关键词，覆盖 filename、URL 和 source key。
3. 连续输入并快速切换筛选，确认旧请求取消且最终查询正确。
4. 记录 IPC、DB、React commit 和主线程总时间。

### 5.4 滚动和交互

1. 在固定窗口尺寸和缩放率下连续滚动 10 秒。
2. 记录平均 FPS、最低 FPS、long task、React commit 和 heap delta。
3. 分别测试普通列表、Queue Center、Attention Center 和打开详情面板的布局。

### 5.5 长时间运行

1. 固定协议混合、连接数、限速和事件频率。
2. 每分钟记录 RSS、CPU、句柄、SQLite/WAL、task_events、request diagnostics、cache entries 和临时文件。
3. 在运行中执行暂停、恢复、删除、网络断开、代理失败和应用退出。
4. 结束后确认没有 ffmpeg、BT session、worker、句柄或 staging 文件残留。

### 5.6 Windows 候选包原生 WebView2 采集

`scripts/perf/native-ui.mjs` 在隔离 B7 identity 下创建 1k／10k 固定数据集，每个任务有一条 file record；分布为完成 50%、暂停 40%、失败 10%。本地采集需要支持 `node:sqlite` 的 Node（已验证 `22.23.2`）；普通工具测试仍可加载，缺少该能力时仅原生 fixture 测试跳过。此数据不对应实际下载文件，不进入调度队列，也不用于协议吞吐验收。脚本核对可执行文件的候选标识与运行中的测试设置，禁止覆盖既有非测试数据目录；换库前确认主程序和退出前记录的 WebView2 进程已结束，再验证 WAL／SHM 已排空。

复现示例（Windows PowerShell，先构建再采样，禁止同时运行其他性能基准）：

```powershell
pnpm prepare:b7-candidate --run-id windows-20261013-ui
$env:VIBE_BROWSER_PROFILE = 'candidate'
$env:CARGO_BUILD_JOBS = '2'
node node_modules/@tauri-apps/cli/tauri.js build --target x86_64-pc-windows-msvc --config logs/b7/windows-20261013-ui/tauri.b7.generated.json --bundles nsis msi -- --locked -j 2
pnpm perf:native-ui --config logs/b7/windows-20261013-ui/tauri.b7.generated.json --exe src-tauri/target/x86_64-pc-windows-msvc/release/vibe-downloader.exe
```

每次首次执行使用新的 run ID；已有数据目录会被拒绝。复跑同一候选时可显式传 `--reuse-fixture true`，仍检查所有权标记并保存新的时间戳证据目录。默认每规模 5 次完整进程启动、5 次鼠标／方向键选择和精确搜索、10 秒滚动、5 分钟空闲采样；短跑 `--repetitions 1 --steady-seconds 0` 只用于验证 harness，不能替代完整基线。

启动指标是从 `Start-Process` 时间戳到 CDP 连接后观察到首行的上界，包含调试连接和观察开销；不等同于准确首次绘制。先执行一次不计入样本的 1k 启动，设置语言与首次引导状态。系统文件缓存未清空，第一轮单列为首个测量启动，其余单列为后续启动，不能称作冷缓存。搜索从输入事件到匹配 DOM 更新后的两帧，包含产品的 300ms debounce，不能与纯数据库 100ms 预算比较。滚动报告 rAF 节奏、逐秒帧数、long task 和虚拟行 DOM 上限，不称作显示器实际呈现 FPS。

内存报告主进程及其 WebView2 后代的 working-set 总和（共享页可能重复计入）、private bytes、线程／句柄和累计 CPU。进程 I/O counter 含 IPC 等逻辑传输，WAL 文件大小是净占用，二者均不直接解释为物理磁盘写入量。原始 JSON、每次退出校验和截图保存在 `logs/b7/<run-id>/native-ui/<UTC>/`；结果回填 [performance-baseline-results.md](performance-baseline-results.md)，不关闭未覆盖的跨平台、真实对端或长跑条目。

## 6. 结果记录模板

每次基线提交都填写：

```text
Date:
Git commit / worktree note:
App version:
OS and build:
CPU / RAM / disk:
Node / Rust / WebView version:
Build profile:
Dataset seed and distribution:
Repetitions:

Metric                 p50       p95       peak/min       notes
Cold start
First interactive
Search
Filter
Scroll FPS
RSS
CPU
DB writes/s
Events/s
WAL size
Open handles
Bundle raw/gzip/brotli
```

原始 trace、query plan 和采样 CSV 应作为 CI artifact 或 release evidence 保存，不要只把汇总数字写入本文。

## 7. 暂定预算

以下只是建立门禁前的初始目标，第一次真实测量后必须校准：

| 指标 | 暂定目标 |
| --- | --- |
| 10k 任务搜索 p95 | 小于 100ms DB 时间 |
| 50k 连续滚动 | 平均不低于 55 FPS，最低不低于 30 FPS |
| 非活动详情 tab | 0 次无关轮询 |
| 长时间空闲内存 | 达到稳定平台，不持续线性增长 |
| 取消和退出 | 目标时限内无残留 worker、子进程和文件写入 |
| HLS 共享 key/map | 每个 URI 每任务最多一次成功获取和发布 |
| 进度通知 | 工作量与 changed IDs 近似线性 |

这些预算不是公开 SLA。任何平台无法达到时，应记录真实数据和取舍，而不是删除失败样本。

## 8. 更新规则

- 性能修复必须提供修复前后相同环境的对比。
- 先证明热点再引入 FTS、LRU、拆包或 profile override。
- 功能正确性、数据完整性和取消收敛优先于吞吐数字。
- 基线结果变化后同步主审计对应 `PERF-*` 状态，但不要删除历史测量。
