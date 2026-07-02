# 性能基线

最后更新：2026-06-29（E-5 阶段 10 落地）

本文件记录 Vibe Downloader `0.2.0` 的性能压测工具、方法论与基线数据。目的是让后续性能回归有可对比的参照点，并把"1k/10k/50k 任务库是否可用"从主观感受转化为可重复执行的步骤。

---

## 1. 压测工具：`seed_scale_tasks`

### 1.1 定位

`seed_scale_tasks` 是 **debug-only** 的参数化 seed 命令，用于生成生产规模的 SQLite 任务库。实现在 [src-tauri/src/commands/tasks/mock_seed.rs](../src-tauri/src/commands/tasks/mock_seed.rs)，受 `#[cfg(debug_assertions)]` 保护，release 构建中不存在（已通过 `cargo check --release` 验证）。

### 1.2 调用约定

```ts
// bindings.ts
seedScaleTasks: (
  distribution: ScaleStateDistribution,
  clearBefore: boolean | null,
) => Promise<number>

export type ScaleStateDistribution = {
  queued: number       // i32，生成 Queued 任务数
  downloading: number // i32，生成 Downloading 任务数
  completed: number    // i32，生成 Completed 任务数
  failed: number       // i32，生成 Failed 任务数
}
```

- `clearBefore = true`：先 `db::clear_tasks` 清库再写入。
- `clearBefore = false`（默认）：追加模式，`index` 按 `SELECT COUNT(*) FROM tasks` 偏移，保证 `source_key` 唯一约束不冲突。
- 返回值：本次实际写入的任务数（`u32`，映射到 TS `number`，避免 Specta BigInt 限制）。

### 1.3 生成的关联数据

每条任务除了 `tasks` 行之外，还会写入：

| 状态 | `task_files` | `task_work_units`（segments） | `task_events` | `task_request_diagnostics` |
| --- | --- | --- | --- | --- |
| Queued | 1 | 0（未规划） | 1（task_created） | 0 |
| Downloading | 1 | 4（含进度） | 2（task_created + state_change） | 2 |
| Completed | 1 | 4（全部 Completed） | 2 | 2 |
| Failed | 1 | 4（前 2 Completed，后 2 Failed） | 2 | 2 |

Failed 任务额外携带：
- `error_message = "Connection reset by peer"`
- `error_code = "http_request_failed"`
- 后 2 个 segment 的 `last_error` 同步填充

### 1.4 确定性与变化

- `total_size = 100 MB + (index % 10) * 45 MB`，范围 100–505 MB，避免均匀数据集。
- `source_key = "scale-{index}.example.com"`，唯一。
- `url = "https://scale-{index}.example.com/file.bin"`。
- 时间戳统一为 `seed_scale_data` 调用时刻的 ISO 时间，避免 50k 行写入期间时间漂移。
- 4 个 segment 的 range 在 `[0, total_size)` 上等分并连续（`range_end` 收敛到 `total_size`）。

### 1.5 单元测试覆盖

8 项测试位于 [src-tauri/tests/scale_seed.rs](../src-tauri/tests/scale_seed.rs)：

1. `generates_correct_total_count`
2. `generates_correct_state_distribution`
3. `clear_before_wipes_existing_tasks`
4. `append_mode_preserves_existing_tasks`
5. `generates_segments_for_non_queued_tasks`
6. `generates_events_for_all_tasks`
7. `generates_request_diagnostics_for_non_queued_tasks`
8. `failed_tasks_have_error_metadata`

---

## 2. 推荐压测场景

| 规模 | distribution（queued, downloading, completed, failed） | 用途 | 运行环境 |
| --- | --- | --- | --- |
| 1k | (250, 250, 400, 100) | CI 回归基线 | CI（Linux stable） |
| 10k | (2000, 2000, 5000, 1000) | 日常开发本机压测 | 本地 |
| 50k | (10000, 5000, 30000, 5000) | 内存上限与极限滚动 | 本地高性能机 |

调用示例（dev 模式下通过 Tauri devtools console）：

```js
// 1k 场景，先清库
await window.__TAURI__.seedScaleTasks(
  { queued: 250, downloading: 250, completed: 400, failed: 100 },
  true,
)

// 追加 10k 到现有 1k 上（共 11k）
await window.__TAURI__.seedScaleTasks(
  { queued: 2000, downloading: 2000, completed: 5000, failed: 1000 },
  false,
)
```

---

## 3. 手工测量方法论

以下指标需要在本机运行 `pnpm tauri dev` 后人工测量并填入"基线数据"小节。当前轮次未在 CI 中自动化采集，留作后续 GUI 自动化或 devtools trace 接入后再补全。

### 3.1 冷启动首屏耗时

1. 删除 `%LOCALAPPDATA%\com.vibe-downloader.vibe-downloader`（或等价目录）。
2. `pnpm tauri dev`，从 cargo 编译完成到任务列表渲染出第一屏可交互任务。
3. 用 devtools Performance 录制，取 `first paint` 到 `largest contentful paint` 的时间。
4. 分别在 1k / 10k / 50k 任务库下重复 3 次取中位数。

### 3.2 任务列表滚动 FPS

1. 在 50k 任务库下打开任务列表。
2. devtools Performance → Record。
3. 用滚轮连续滚动 10 秒，覆盖至少 500 行跨度。
4. 取平均 FPS 与最低 FPS。

### 3.3 筛选响应

1. 在 50k 任务库下，从"全部"切到"Completed"。
2. devtools Performance → Record，记录从点击到列表稳定的时间。
3. 重复 3 次取中位数。

### 3.4 详情页 Requests tab 打开时间

1. 选中一个有 2 条 request diagnostics 的 Failed 任务。
2. 打开 TaskDetails → Requests tab。
3. devtools Performance → Record，记录从点击 tab 到渲染完成的时间。

### 3.5 内存峰值

1. 重启 app，记录 devtools Memory → Heap snapshot 的初始大小。
2. 加载 50k 任务库，再次取 Heap snapshot。
3. 取差值作为"50k 任务库增量内存"。

---

## 4. 内存评估：`task-data-store` 全量 `taskById` map

### 4.1 数据结构

[src/stores/task-data-store.ts](../src/stores/task-data-store.ts) 第 198 行：

```ts
taskById: Record<string, Task>;
```

`Task` 类型定义在 [src/types/task.ts](../src/types/task.ts)，是基于 Specta 生成的 `GeneratedTask` 的 Omit + 扩展，将 `totalSize` / `downloadedBytes` / `speedBps` 从 string 归一化为 number，并附加 `files: TaskFile[]`。

### 4.2 单条 Task 估算

`Task` 共 38 个字段（参考 [src/generated/bindings.ts](../src/generated/bindings.ts) 第 619–661 行）。按字段类型估算 V8 堆占用：

| 字段类别 | 示例 | 估算单条占用 |
| --- | --- | --- |
| UUID 字符串（36 字符） | `id` | ~96 B |
| URL / 文件名字符串（avg 30–50 字符） | `url`, `fileName`, `saveDir`, `sourceKey` | 4 × ~100 B = ~400 B |
| ISO 时间字符串（25 字符） | `createdAt`, `updatedAt` | 2 × ~74 B = ~148 B |
| 可空字符串（多数为 null） | `tempPath`, `finalPath`, `etag`, `lastModified`, `contentType`, `errorMessage`, `errorCode`, `retryAfterAt`, `expectedHashSha256`, `actualHashSha256`, `hashError`, `hashVerifiedAt`, `healthSummary`, `categoryKey`, `failureCategory`, `taskSpeedLimitBps`, `finalUrl` | 17 × 8 B（null） = ~136 B（非空时增加） |
| 数值 / 布尔 | `connectionCount`, `supportsResume`, `supportsParallel`, `supportsMultiFile`, `obeySchedule`, `hashStatus` | 6 × 8 B = ~48 B |
| 枚举字符串 | `status`, `taskKind`, `priority`, `hashStatus` | 4 × ~44 B = ~176 B |
| 字节量字符串（数字转 string） | `totalSize`, `downloadedBytes`, `speedBps`, `queuePosition` | 4 × ~44 B = ~176 B |
| 空数组 | `recoveryActions`, `checksums` | 2 × ~32 B = ~64 B |
| `files: TaskFile[]`（1 条单文件任务） | 见下 | ~250 B |
| V8 对象开销（隐藏类 + 属性槽） | — | ~200 B |

**单条 Task（含 1 个 TaskFile，无 checksums/recoveryActions）估算：~1.7 KB**

`TaskFile` 约 11 个字段（`id`, `taskId`, `relativePath`, `fileName`, `saveDir`, `tempPath`, `finalPath`, `totalSize`, `downloadedBytes`, `selected`, `status`, `contentType`），估算 ~750 B 含 V8 对象开销；1 条文件场景下 ~250 B 已足够（多数字段为 null 或短串）。

### 4.3 全量 map 估算

| 规模 | `taskById` 条目数 | 增量内存估算（不含 React 组件、虚拟滚动 buffer、speed-history） |
| --- | --- | --- |
| 1k | 1,000 | ~1.7 MB |
| 10k | 10,000 | ~17 MB |
| 50k | 50,000 | ~85 MB |

V8 `Record`（底层 `OrderedHashTable`）的属性槽开销约 50 B/槽，50k 条额外 ~2.5 MB，已计入上表。

### 4.4 风险评估

- **50k 任务库 ~85 MB** 在主流 16 GB+ 桌面机上可接受，但已接近"全量 map"模式的合理上限。
- 真实增量会更高：React 组件状态、虚拟滚动 overscan buffer、`speed-history-store`（每任务 60 samples × 8 B = 480 B/task → 50k × 480 B ≈ 23 MB）、Tauri IPC 序列化缓冲。
- **建议**：50k 以上规模需要切换到"按页懒加载 + cursor pagination 到 DB"的模式，把 `taskById` 改为 LRU cache（容量 1k–2k）。**当前不实施**，留作后续架构调整，参考 [docs/architecture-audit.md](architecture-audit.md) E-5 章节的后续优化建议。

---

## 5. 已验证项

本轮（2026-06-29）已通过自动化验证的内容：

- **`seed_scale_tasks` 正确性**：8 项单元测试全部通过（见 [src-tauri/tests/scale_seed.rs](../src-tauri/tests/scale_seed.rs)）。
- **release 构建隔离**：`cargo check --release` 成功，确认 debug-only 命令不会泄露到 release。
- **全量 Rust 回归**：`cargo test --no-fail-fast` 314 项通过，0 失败。
- **clippy**：`cargo clippy --tests -- -D warnings` 清洁。
- **前端类型与构建**：`pnpm typecheck`、`pnpm test:frontend`（18 项）、`pnpm build` 均通过。
- **bindings 一致性**：`pnpm specta` 重新生成 `src/generated/bindings.ts` 后，与 Rust 命令注册完全一致。

---

## 6. 后续步骤

| 项 | 状态 | 说明 |
| --- | --- | --- |
| 1k 规模 CI 集成 | 待办 | 需要新增 `cargo test --test scale_seed` 到 `.github/workflows/` 的 nightly 或 ci job |
| 10k / 50k 本机基线数据采集 | 待办 | 需要在真实开发机上按 §3 方法论测量并回填本文件 |
| `taskById` LRU 改造 | 评估中 | 50k 任务 ~85 MB 已接近上限，未来若用户反馈卡顿需优先实施 |
| GUI 自动化 trace 采集 | 待办 | 评估 Playwright / Tauri devtools protocol 自动化 FPS 采集可行性 |
