# 性能基线实测结果（PERF-11 / E1）

最后更新：2026-10-02

历史完整基线适用版本：Vibe Downloader `0.3.0`；本页新增的当前 smoke 适用版本为 `0.5.0`（1k 与 10k）。

状态：已建立可重复 headless harness，并完成本机 1k / 10k 实测。**50k+、HLS/BT 长跑、1k 批量删除 soak、CI 绝对数值门禁仍延期。**

方法与矩阵见 [performance-baseline.md](performance-baseline.md)。原始 JSON 由本地 `artifacts/perf/<ts>/` 生成（目录 gitignore）；本文保留可复核摘要。
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

## 4. 手动 release UI 清单（本批未测）

以下需在未签名 release candidate 上人工填写；本批只交付清单与 headless 数字，**不假装已有冷启动/FPS 数据**：

| 指标 | 1k | 10k | 备注 |
| --- | --- | --- | --- |
| 冷启动 → 首屏任务可见 | _待测_ | _待测_ | 完全退出后启动 5 次，分冷/暖缓存 |
| 可交互时间 | _待测_ | _待测_ | |
| 连续滚动 10s 平均/最低 FPS | _待测_ | _待测_ | 固定窗口尺寸 |
| 稳态 RSS | _待测_ | _待测_ | 空闲 5 分钟后采样 |

复现 headless：

```bash
pnpm perf:baseline
pnpm perf:baseline:10k
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
