# 性能基线实测结果（PERF-11 / E1）

最后更新：2026-10-13

历史完整基线适用版本：Vibe Downloader `0.3.0`；本页新增的当前 smoke 适用版本为 `0.5.0`（1k 与 10k）。

状态：已建立可重复 headless harness，完成当前本机 1k / 10k 实测及 Windows 原生候选短基线。**当前版本 50k+、真实冷缓存与呈现帧率、HLS/BT 长跑、1k 批量删除 soak、CI 绝对数值门禁仍延期。**

## 0.5. 2026-10-12 B7 创建路径与运行快照（`0.5.0`）

本次使用 Windows dirty 工作区、debug profile；列表／搜索各重复 5 次，创建／冲突／旧全量读取／运行快照各重复 25 次（harness 对创建路径设置了最低样本数）。原始 JSON 位于 `logs/b7/20261012-windows/perf-corrected/`（本地证据目录，未纳入版本库）。除列表／搜索外，harness 先把种子任务补成真实活动路径，再测缓存 probe 创建、同名冲突重试、旧的全量路径读取和运行时快照上限。该对照用于复核 `PERF-16`，不把本机数字当作跨机器门禁。

| 活动任务数 | 创建 p50/p95 (ms) | 同名冲突 p50/p95 (ms) | 全量路径读取 p50/p95 (ms) | 快照 p50/p95 (ms) | 快照返回 |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 1,000 | 3.43 / 21.97 | 4.24 / 20.72 | 5.97 / 8.99 | 15.18 / 16.55 | 500 |
| 10,000 | 4.50 / 18.17 | 5.17 / 7.44 | 107.26 / 124.02 | 26.43 / 28.56 | 500 |

结果：当前创建路径的 p95 没有随活动任务数线性增长，快照稳定截断为 500；保留全量路径读取作为历史对照，证明移除预读的收益。WAL 净增长在这组 SQLite 检查点后为 0，不解释为“无写入”。

方法与矩阵见 [performance-baseline.md](performance-baseline.md)。历史 1k／10k JSON 仍由本地 `artifacts/perf/<ts>/` 生成（目录 gitignore）；本次 PERF-16 创建路径 JSON 保存在上文的 B7 证据目录，本文保留可复核摘要。
## 0. 2026-10-02 B7 当前 1k smoke（`0.5.0`）

本次运行使用当前工作区 `611cbbc65b6ff76e8b92e1564d6538b4110e25a5`，工作区为 dirty；原始数据位于 `artifacts/perf/20261002-192007/`。这是当前版本的 1k headless smoke，只更新可复核基线，不替代真实 release UI、协议对端或长跑验收。

| 字段 | 值 |
| --- | --- |
| Collected (UTC) | `2026-10-02T11:20:09.6366930Z` |
| OS | Windows 11 Enterprise Insider Preview `10.0.28020` (AMD64) |
| CPU / RAM | Intel Core i7-14700HX（20 核 / 28 逻辑）；约 63.7 GiB，总运行时约 36.7 GiB free |
| Toolchain | rustc/cargo `1.95.0`；Node `v22.23.2` |
| Profile | `debug`（`cargo test`） |
| Command | `pnpm perf:baseline` |
| Repetitions / page size | 5 / 100 |
| Seed time | 2726.45 ms |

| Case | p50 (ms) | p95 (ms) | EXPLAIN QUERY PLAN |
| --- | ---: | ---: | --- |
| `list_all_updated_at` | 2.46 | 3.15 | `SCAN tasks USING COVERING INDEX idx_tasks_updated_at_id` |
| `search_filename_prefix`（`scale-file-1`） | 4.22 | 4.46 | `SCAN tasks USING INDEX idx_tasks_updated_at_id` |
| `filter_completed` | 2.94 | 3.01 | `SEARCH tasks USING COVERING INDEX idx_tasks_status_updated_at_id (status=?)` |
| `filter_failed_sort_size` | 2.89 | 2.99 | `SEARCH tasks USING INDEX idx_tasks_queue_order (status=?)` + `USE TEMP B-TREE FOR ORDER BY` |

本次 smoke 通过。它只说明当前 headless 数据库路径在这台机器上的 1k 规模结果；冷启动、滚动、RSS、真实下载吞吐、10k/50k 复测和 HLS/BT 长跑仍按 B7 矩阵执行。
## 0.1 2026-10-02 B7 当前 10k baseline（`0.5.0`）

本次运行与 1k smoke 使用同一源码快照和机器，原始数据位于 `artifacts/perf/20261002-200606/`。10k 数据生成耗时 `104083.44 ms`，因此该场景适合作为低频基线，不纳入日常快速门禁。

| Case | p50 (ms) | p95 (ms) | EXPLAIN QUERY PLAN |
| --- | ---: | ---: | --- |
| `list_all_updated_at` | 10.10 | 10.74 | `SCAN tasks USING COVERING INDEX idx_tasks_updated_at_id` |
| `search_filename_prefix`（`scale-file-1`） | 22.70 | 23.43 | `SCAN tasks USING INDEX idx_tasks_updated_at_id` |
| `filter_completed` | 9.47 | 11.53 | `SEARCH tasks USING COVERING INDEX idx_tasks_status_updated_at_id (status=?)` |
| `filter_failed_sort_size` | 13.12 | 13.82 | `SEARCH tasks USING INDEX idx_tasks_queue_order (status=?)` + `USE TEMP B-TREE FOR ORDER BY` |

10k baseline 通过。当前结果没有触发新的优化决策：查询计划与既有基线一致，仍需 release UI、50k、冷启动和长跑数据后再评估性能调整。

## 1. 运行环境

| 字段 | 值 |
| --- | --- |
| Date (UTC) | 2026-07-20T10:20:26Z |
| Git commit | `6fdec89a7119b641c7c9be0da05fe4818455965e`（工作区 dirty：含本批 PERF-07/PERF-11 改动） |
| App version | `0.3.0` |
| OS | Windows 11 Enterprise Insider Preview 10.0.28120 (AMD64) |
| CPU | Intel Core i7-14700HX（20 核 / 28 逻辑） |
| RAM | ~64 GB（测量时约 40 GB free） |
| Toolchain | rustc 1.95.0 / cargo 1.95.0 / node v22.22.0 |
| Build profile | `debug`（`cargo test`，非 release） |
| Harness | [`src-tauri/tests/perf_baseline.rs`](../src-tauri/tests/perf_baseline.rs) |
| Orchestration | `pnpm perf:baseline` / `pnpm perf:baseline:10k` |

## 2. 数据生成

使用 debug-only `seed_scale_data`，分布 20% queued / 20% downloading / 50% completed / 10% failed：

| 规模 | queued | downloading | completed | failed | seed 耗时 |
| --- | ---: | ---: | ---: | ---: | ---: |
| 1k | 200 | 200 | 500 | 100 | 2.80 s |
| 10k | 2000 | 2000 | 5000 | 1000 | 33.88 s |

每用例重复 5 次；`page_size = 100`；先 warmup 一次 `list` 再计时。

## 3. Headless 查询结果（DB cursor path）

指标为 `db::list_task_records_cursor` 墙钟时间（毫秒），不含 UI/IPC。

### 3.1 1k tasks

| Case | p50 (ms) | p95 (ms) | EXPLAIN QUERY PLAN |
| --- | ---: | ---: | --- |
| `list_all_updated_at` | 3.00 | 3.62 | `SCAN tasks USING COVERING INDEX idx_tasks_updated_at_id` |
| `search_filename_prefix`（`scale-file-1`） | 4.18 | 4.35 | `SCAN tasks USING INDEX idx_tasks_updated_at_id` |
| `filter_completed` | 3.06 | 3.17 | `SEARCH tasks USING COVERING INDEX idx_tasks_status_updated_at_id (status=?)` |
| `filter_failed_sort_size` | 2.89 | 3.09 | `SEARCH ... idx_tasks_queue_order (status=?)` + `USE TEMP B-TREE FOR ORDER BY` |

### 3.2 10k tasks

| Case | p50 (ms) | p95 (ms) | EXPLAIN QUERY PLAN |
| --- | ---: | ---: | --- |
| `list_all_updated_at` | 3.28 | 3.68 | `SCAN tasks USING COVERING INDEX idx_tasks_updated_at_id` |
| `search_filename_prefix`（`scale-file-1`） | 7.60 | 9.52 | `SCAN tasks USING INDEX idx_tasks_updated_at_id` |
| `filter_completed` | 2.84 | 2.96 | `SEARCH tasks USING COVERING INDEX idx_tasks_status_updated_at_id (status=?)` |
| `filter_failed_sort_size` | 4.33 | 4.81 | `SEARCH ... idx_tasks_queue_order (status=?)` + `USE TEMP B-TREE FOR ORDER BY` |

### 3.3 50k tasks（PERF-01）

| Case | p50 (ms) | p95 (ms) | EXPLAIN QUERY PLAN |
| --- | ---: | ---: | --- |
| `list_all_updated_at` | 3.79 | 7.49 | `SCAN tasks USING COVERING INDEX idx_tasks_updated_at_id` |
| `search_filename_prefix`（`scale-file-1`） | 5.64 | 6.90 | `SCAN tasks USING INDEX idx_tasks_updated_at_id` |
| `filter_completed` | 2.58 | 2.59 | `SEARCH tasks USING COVERING INDEX idx_tasks_status_updated_at_id (status=?)` |
| `filter_failed_sort_size` | 7.22 | 8.56 | `SEARCH ... idx_tasks_queue_order (status=?)` + `USE TEMP B-TREE FOR ORDER BY` |

seed 耗时 ≈ 182 s（debug）。**决策**：50k search p95 = 6.90 ms ≪ 100 ms 预算 → **保持三字段 `LOWER LIKE`，不引入 FTS5**。支持规模记录为「本机 debug harness 上至少 50k 任务搜索可接受」；慢盘/低内存设备仍可能更慢，不假装已有 FTS。

### 3.4 观察（非门禁）

- 状态筛选走 covering index，1k→50k 几乎持平。
- 三字段 `LOWER(...) LIKE '%term%'` 搜索在 50k 仍为全表相关 SCAN（对齐原 `PERF-01` 证据）；p95 仍远低于预算，故不引入 FTS。
- `file_size` 排序对 failed 子集使用临时 B-Tree。

## 4. 2026-10-13 Windows 原生 release UI 短基线

本次使用 NSIS 隔离安装后的 `0.5.0`，identity `com.vibe.downloader.b7.windows-20261013-ui`，主程序 SHA-256 `579976d2011389513c23ae3b9428aee4e332f950de1edff02c446f17c0c1d0d5`。源码 HEAD `f87c2007e52d026265472e2f2efd6c80ab3d3642`、dirty；release `opt-level="s"`、LTO。机器为 Windows 11 Enterprise、i7-14700HX、28 逻辑 CPU、约 63.7 GiB RAM；Node 22.23.2、WebView2 `Edg/156.0.4314.15`。可见 viewport 固定 `1200×830`、DPR `1.25`、英文／comfortable 行密度；宿主是 Codex Windows MSIX，数据目录虚拟化按 [矩阵 6.2](b7-acceptance-matrix-2026-10-02.md#62-2026-10-13-windows-候选刷新与原生-ui-基线) 单独记录。

每组 5 个启动／鼠标／键盘／搜索样本，p95 为 nearest-rank，因此这组小样本的 p95 等于最大值；每组仅 1 次 10 秒滚动、1 次 5 分钟空闲（11 个进程采样）。任务为完成 50%、暂停 40%、失败 10%，每任务一条 file record；没有实际下载文件或网络传输。原始 JSON、截图和逐次退出记录为 `logs/b7/windows-20261013-ui/native-ui/2026-10-12T18-03-07-276Z/`，汇总为 `native-summary.json`。

| 指标 | 1k | 10k | 方法／限制 |
| --- | ---: | ---: | --- |
| 首屏任务观察上界 p50/p95 (ms) | 1944 / 2007 | 1842 / 2346 | 从 Start-Process 到 CDP 观察，非真实 first paint／冷缓存 |
| 首次测量启动 (ms) | 1944 | 1816 | 有一次不计入样本的 1k primer；OS 缓存未清空 |
| 后续 4 次首屏 p50/p95 (ms) | 1892 / 2007 | 1842 / 2346 | 完整退出后重启 |
| 启动到首行选择成功 p50/p95 (ms) | 1989 / 2084 | 1914 / 2445 | 含元数据查询与输入验证开销，为可交互观察上界 |
| 鼠标选行 p50/p95 (ms) | 13.6 / 26.9 | 22.4 / 32.1 | 输入事件到匹配 DOM 后两帧 |
| 方向键选行 p50/p95 (ms) | 21.9 / 22.7 | 21.1 / 26.3 | 另核对焦点落在下一行 |
| 精确搜索 p50/p95 (ms) | 345.4 / 348.2 | 365.4 / 377.1 | 包含 300ms debounce，非纯 DB 耗时 |
| 滚动 rAF 平均/最少完整秒计数 | 90.94 / 68 | 54.45 / 36 | CDP wheel 10 秒；非 compositor／显示器呈现 FPS |
| 滚动 long task (≥50ms) | 0 | 0 | 单次记录；不能替代长跑或所有交互 |
| 最大虚拟行 DOM / 已加载页行数 | 22 / 300 | 22 / 300 | 只滚动到部分分页，未遍历整个数据集 |
| 5 分钟主进程 working set (MiB) | 50.77 | 72.13 | 最后一次采样 |
| 主程序＋WebView2 working set 总和 (MiB) | 622.48 | 606.06 | 7 进程；共享页可能重复计入，不是去重 RSS |
| 同一进程族 private bytes 总和 (MiB) | 327.03 | 332.93 | 最后一次采样，非峰值 |
| 空闲 CPU 均值（按 28 逻辑 CPU 归一化） | 0.075% | 0.101% | 进程族 CPU counter 差值；单核口径约 2.11% / 2.83% |
| 进程族句柄 / 线程 | 4133 / 211 | 4119 / 210 | 最后一次采样 |
| WAL 文件占用 (bytes) | 61,832 | 61,832 | 11 次空闲采样中保持同值，不代表无写入 |

两组任务数量与查询结果正确，所有测量退出前记录的 7 个进程都结束。内存从滚动／截图后的高点下降，但 5 分钟不能证明一小时／长跑无泄漏。10k 的 rAF 单次均值低于 1k，且进程逻辑读取差值为 706,977,196 字节（1k 为 22,643,483）；逻辑 I/O 含 IPC，不是物理磁盘吞吐，需要在后续空闲／SQLite profile 中拆分。未据此调整 `55 FPS` 等既有预算，也未推出跨机器或 50k 的结论。

该快照的原生截图发现系统 PNG 被 CSP 阻断且缺少图片错误回退，登记 `UX-51`；NSIS 包含多余 `export-bindings.exe`，登记 `PERF-21`。随后修复的 NSIS／MSI 已排除 exporter 和重复 sidecar，实际原生图标／坏图回退／CSP 验收通过，详见 [矩阵 6.3](b7-acceptance-matrix-2026-10-02.md#63-2026-10-13-图标与二进制包装修复验收)；两项为 Fixed locally。新包 1k／10k 各 1 次短跑仅作功能 smoke，不能替换本节五次基线或证明性能提升。候选 UI 全面走查、真实冷缓存、50k 实际帧率、NTFS 预分配、检查点压力、1 小时空闲与真实协议长跑继续列入 B7。

复现入口与计时定义见 [采集方法 5.6](performance-baseline.md#56-windows-候选包原生-webview2-采集)：

```powershell
pnpm perf:native-ui --config logs/b7/windows-20261013-ui/tauri.b7.generated.json --exe <重新隔离安装后的程序路径> --reuse-fixture true
```

## 5. Bundle 体积门禁（PERF-10）

CI 在 `pnpm build` 后运行 `pnpm check:bundle`（[`scripts/check-bundle-budget.mjs`](../scripts/check-bundle-budget.mjs) + [`scripts/bundle-budget.json`](../scripts/bundle-budget.json)）。只对 **initial shell 聚合**设硬上限，不对单个 vendor chunk 设硬上限；超限时打印 per-chunk raw/gzip/brotli。

本机实测（`2026-07-20`，`pnpm build`）：

| 指标 | 实测 | 预算 | 状态 |
| --- | ---: | ---: | --- |
| Initial shell JS raw | 896.4 kB | ≤ 1126.4 kB (1.10 MB) | OK |
| Initial shell JS gzip | 280.3 kB | ≤ 340.0 kB | OK |
| Initial shell JS brotli | 234.6 kB | ≤ 290.0 kB | OK（报告项，同预算表） |
| Initial shell CSS gzip | 14.6 kB | ≤ 18.0 kB | OK |

Initial shell 包含：`index`、`react-vendor`、`radix-ui`、`utils`、`motion`、`i18n`、`lucide`、`tauri`、`bindings`、runtime/preload helpers、主 CSS。Settings / TaskDetails / Palette / 语言包等为 deferred，不计入门禁。

```bash
pnpm build
pnpm check:bundle
```

## 5.5. Release `opt-level` 对比（PERF-09，框架已就位，数据待采集）

`src-tauri/Cargo.toml` 的 `[profile.release]` 当前为 `opt-level = "s"`（配合 `lto = true`、`codegen-units = 1`、`strip = true`）。PERF-09 要求先用数据判断 `s` 是否牺牲了热点吞吐，再决定是否只对热点 package 做 profile override——**没有数据前不改**。

`scripts/perf/run-baseline.ps1 -CompareOptLevel` 已搭好采集骨架：分别以 `opt-level="s"` 与 `"3"` 构建 release，并记录构建耗时与二进制体积到 `artifacts/perf/<ts>/opt-level/opt-level-<level>.json`。

| 指标 | opt-level=`s` | opt-level=`3` | 结论 |
| --- | --- | ---: | ---: | --- |
| 构建耗时（冷/热） | 待采集 | 待采集 | — |
| 二进制体积 | 待采集 | 待采集 | — |
| 启动时间 | 待采集 | 待采集 | — |
| 热点吞吐：hash / AES | 待采集 | 待采集 | — |
| 热点吞吐：XML / BT | 待采集 | 待采集 | — |
| 真实下载路径吞吐 | 待采集 | 待采集 | — |

吞吐项需要专用 microbench 或手工测量（现有 DB harness 只跑 debug profile，`perf_baseline.rs` 带 `#![cfg(debug_assertions)]`），本批不采集。采集完成后把结论回填本表，并在必要时用 `[profile.release.package.<name>]` 只对热点 crate 覆盖 `opt-level`。

## 6. 明确延期

- 100k 全矩阵
- HLS / BT 30min–8h soak
- 1k 批量删除 soak
- CI 绝对数值门禁（DB harness；bundle 门禁已启用）
- PERF-09 `opt-level` 吞吐对比（框架已就位，见 §5.5）

## 7. 复现命令

```bash
# CI / 日常 smoke（仅 1k）
cargo test -j 2 --manifest-path src-tauri/Cargo.toml --test perf_baseline

# 完整本地 1k + 10k + metadata
pnpm perf:baseline:10k

# PERF-09 opt-level 采集骨架（构建耗时 + 二进制体积）
pwsh -File scripts/perf/run-baseline.ps1 -CompareOptLevel

# Bundle budget
pnpm build && pnpm check:bundle

# PERF-01 50k search（ignore）
pnpm perf:baseline:50k
```
