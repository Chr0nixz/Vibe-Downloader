# 性能基线实测结果（PERF-11 / E1）

最后更新：2026-07-20

适用版本：Vibe Downloader `0.3.0`

状态：已建立可重复 headless harness，并完成本机 1k / 10k 实测。**50k+、HLS/BT 长跑、1k 批量删除 soak、CI 绝对数值门禁仍延期。**

方法与矩阵见 [performance-baseline.md](performance-baseline.md)。原始 JSON 由本地 `artifacts/perf/<ts>/` 生成（目录 gitignore）；本文保留可复核摘要。

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

### 3.3 观察（非门禁）

- 状态筛选走 covering index，1k→10k 几乎持平。
- 三字段 `LOWER(...) LIKE '%term%'` 搜索在 10k 仍为全表相关 SCAN（对齐 `PERF-01`）；p95 ≈ 9.5 ms，低于暂定预算「10k 搜索 p95 < 100ms」，但预算是 DB 目标而非 SLA，且本机 debug profile 不能外推 release/慢盘。
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

## 5. 明确延期

- 50k / 100k 全矩阵
- HLS / BT 30min–8h soak
- 1k 批量删除 soak
- CI 绝对数值门禁（保留 1k smoke 防 harness 损坏）
- `PERF-01`–`PERF-08` / `PERF-10` 热修复（见审计阶段 E）

## 6. 复现命令

```bash
# CI / 日常 smoke（仅 1k）
cargo test -j 1 --manifest-path src-tauri/Cargo.toml --test perf_baseline

# 完整本地 1k + 10k + metadata
pnpm perf:baseline:10k
```
