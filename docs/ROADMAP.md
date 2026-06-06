# Vibe Downloader 开发路线图

本文档描述在**基础框架**（当前仓库状态）之上的分阶段开发计划，与 [PRODUCT.md](../PRODUCT.md) 的战略优先级、[DESIGN.md](../DESIGN.md) 的体验与视觉规范对齐。

> 说明：`docs/functional-design.md` 与 `docs/ui-design-style.md` 已恢复到 `docs/`，后续计划以这两份文档、`PRODUCT.md` 与 `DESIGN.md` 共同约束。

---

## 现状快照

| 已完成 | 未完成 / 占位 |
|--------|----------------|
| Tauri 2 + React 壳层、设计 token、任务列表/详情/命令栏 UI | 全局队列、任务级并发上限、限速与设置页 |
| SQLite schema、`tasks` / `segments`、`list_tasks`、`list_task_segments` | 崩溃后自动继续下载暂不启用 |
| HTTP probe、Range 续传校验、固定 4 路分片多连接、真实 `task.progress` 事件 | 动态连接数与 `connection.changed` 实时事件 |
| Chunks tab 与 Connections tab 只读 segment/连接摘要 | 浏览器扩展交接 |
| Windows 自绘标题栏 + 窗口控制；打开文件/目录命令 | 速度历史、Toast、撤销等体验抛光 |
| 新建下载、暂停/恢复/删除、重试、失败路径与 `needs_attention` | 事件类型未完全纳入 specta event 契约 |
| `tauri-specta` 命令绑定、本地 HTTP/segments/SHA-256 回归测试 | L2 三平台 `tauri build` 仍需持续验证 |

---

## 阶段 0：巩固基础

**目标**：在加功能前减少技术债，避免类型漂移与平台差异。

**周期（参考）**：3–5 天

### 任务

1. **版本管理**：初始化 git，完善 `.gitignore`（`target/`、`dist/`、本地 DB 等）；可选恢复 `docs/functional-design.md`、`docs/ui-design-style.md`。
2. **类型与事件**：为 `TaskProgressPayload` 注册 `tauri-specta` events，或统一事件契约；逐步减少手写 `src/types/*` 与生成 bindings 的双轨维护。
3. **平台与壳层**：验证 Windows / macOS / Linux 标题栏；`tauri.conf` 与 runtime `decorations` 策略一致；推动 L2 `tauri build` 三平台绿。
4. **主题**：按 DESIGN 接入 `next-themes`，默认跟随系统，暗色为主要打磨目标。

### 验收

- `pnpm tauri dev` 稳定启动，`pnpm typecheck` 与 `cargo clippy -D warnings` 通过。
- 无 mock 数据时仍可空列表正常启动。

---

## 阶段 1：HTTP MVP — 单连接可下载

**对齐 PRODUCT 优先级 1**：HTTP 稳定后再扩展其他协议。

**周期（参考）**：2–3 周

### 架构示意

```mermaid
flowchart LR
  UI[React UI] -->|create_task / pause / resume| CMD[Tauri commands]
  CMD --> DB[(SQLite)]
  CMD --> ENG[HttpEngine]
  ENG -->|reqwest + tokio| NET[HTTP]
  ENG -->|task.progress| EVT[Events]
  DB --> ENG
```

### 1.1 后端（Rust）

| 顺序 | 能力 | 说明 |
|------|------|------|
| 1 | **Probe** | HEAD/GET：解析 `Content-Length`、`Accept-Ranges`、`Content-Disposition`、重定向 → `final_url`、`total_size`、`supports_range` |
| 2 | **单连接下载** | 流式写入 `temp_path`；更新 `downloaded_bytes`、`speed_bps`；发 `task.progress`（**替换** demo emitter） |
| 3 | **命令** | `create_task`、`pause_task`、`resume_task`、`cancel_task`、`delete_task` |
| 4 | **状态机** | `queued → downloading → paused \| completed \| failed`（及 `retrying`、`waiting_network`、`needs_attention`） |
| 5 | **错误模型** | 可恢复 / 不可恢复；`health_summary`、`error_message` 符合 PRODUCT 文案风格 |

### 1.2 前端（React）

| 顺序 | 能力 | 说明 |
|------|------|------|
| 1 | **新建下载** | 命令栏 `+` → 对话框（URL、保存路径、文件名预览） |
| 2 | **命令栏接线** | Start / Pause / Delete 绑定选中任务与禁用态 |
| 3 | **列表真实进度** | 进度条接引擎事件；mock `seed_mock_tasks` 仅 dev 可选 |

### 验收

- 粘贴 URL 后 **5 秒内**看到进度与速度（PRODUCT Success Criteria）。
- 暂停/继续后字节数连续；失败时有明确原因与可理解文案。

---

## 阶段 2：可恢复与多连接

**对齐**：大文件断点续传、专家向细节诊断。

**周期（参考）**：2–3 周

### 任务

1. **Range 续传**：`.vibe-downloading` 临时文件 + `segments` 表；`etag` / `last_modified` 校验；远端变更 → `needs_attention`。**已完成**。
2. **多连接**：`supports_range=true` 且 `total_size >= 16 MB` 时固定 4 个 segments；并发 Range worker 随机写入同一个临时文件。**已完成**。
3. **恢复加固**：恢复前校验 segment range 连续性、`downloaded_until` 边界、临时文件大小、远端元信息变化。**已完成**。
4. **详情面板**：Chunks tab 显示真实 segments；Connections tab 显示只读连接摘要。**已完成**。
5. **Schema 定稿**：当前仍沿用 `001_init.sql`；后续如需 settings 或索引，使用 `002_*` migration。

### 验收

- 必跑链路已通过：`pnpm typecheck`、`pnpm build`、`pnpm specta`、`cargo check`、`cargo clippy -- -D warnings`、`cargo test`。
- 本地 HTTP 回归覆盖：probe、HTTP 错误映射、单连接完成、Range 恢复、多 Range 写同一文件、segment 失败不 rename、SHA-256 完整性、多 segment 恢复异常与远端变化阻断。
- UI 验收通过：大文件可显示 4 个 chunks / connections，暂停/继续、退出后恢复、删除与失败路径符合预期。

---

## 阶段 3：队列与设置

**周期（参考）**：1–2 周

### 任务

1. **Settings 表与命令**：新增 `settings` key-value 或结构化表；提供 `get_settings`、`update_settings`；先覆盖 `max_active_tasks` 与 `default_save_dir`。
2. **全局队列 v1**：默认最多 2 个 active tasks；超过上限的新任务进入 `queued`；完成/失败/暂停/删除后自动调度下一个 queued task。
3. **调度状态收敛**：`create_task`、`resume_task`、`retry_task` 统一进入调度器；app 启动后 interrupted task 仍重置为 `paused`，不自动继续。
4. **设置页**：Sidebar「Settings」由占位改为真实页面；支持默认保存目录与同时下载任务数。
5. **队列 UI**：任务行显示 queued；CommandBar 按状态收敛按钮禁用态；StatusBar 显示 active / queued / total speed。
6. **回归测试**：最大并发 2 时创建 3 个任务只启动 2 个；完成或暂停 active task 后 queued 自动顶上；retry failed task 走队列；settings 更新即时影响调度。

### 验收

- 必跑链路继续全绿：`pnpm typecheck`、`pnpm build`、`pnpm specta`、`cargo check`、`cargo clippy -- -D warnings`、`cargo test`。
- UI 能同时创建多个下载，但 active tasks 不超过 `max_active_tasks`。
- queued task 能自动接力启动，且不破坏阶段 2 的单任务 1/4 segment 下载与恢复逻辑。
- 设置页能修改默认目录和并发任务数；更新后调度器立即使用新配置。

---

## 阶段 4：体验抛光

可与阶段 2–3 **交错**进行，按 DESIGN 逐项补齐。

- 任务行展开态；完成/错误动效（Framer，约 150–250ms）
- Toast；破坏性操作确认或撤销
- 速度历史 sparkline；磁盘/网络瓶颈文案
- 响应式：中窄窗口详情抽屉
- 无障碍：焦点环、热力图文字摘要与 tooltip

---

## 阶段 5：浏览器扩展交接

**对齐 PRODUCT 优先级 2**。建议在 HTTP + 续传可靠后启动。

**周期（参考）**：2–4 周

1. Native messaging 或本地 HTTP 服务 + 扩展「发送到 Vibe」。
2. 剪贴板监听或拖拽 URL（可选）。
3. 扩展最小权限与安装/使用说明。

---

## 当前迭代状态

**阶段 1 最小切片已完成**：

| 顺序 | 任务 | 产出 |
|------|------|------|
| 1 | `reqwest` + `create_task` + probe | 可创建真实任务行 |
| 2 | 单连接下载 + 写盘 + `task.progress` | 已移除 demo progress emitter |
| 3 | 新建下载对话框 + probe 信息 | 用户可粘贴 URL、检测元信息并开始下载 |
| 4 | `pause_task` / `resume_task` / `retry_task` | 命令栏与任务行操作可用 |
| 5 | 失败路径与 `failed` UI | 有明确原因、Retry、删除选择 |

## 当前迭代状态（阶段 2：已通过验收）

阶段 2 已完成并通过验收。当前交付范围：

| 顺序 | 任务 | 产出 |
|------|------|------|
| 1 | 固定分片计划 | `supports_range=true` 且 `total_size >= 16 MB` 时生成 4 个不重叠 segments |
| 2 | 随机写入 | 多个 Range worker 写入同一个 `.vibe-downloading` 临时文件不同偏移 |
| 3 | 汇总进度 | `task.progress` 汇总所有 segment 下载字节与速度 |
| 4 | Chunks / Connections | Chunks 显示真实 segments；Connections 显示只读连接摘要 |
| 5 | 恢复保护 | 恢复前校验本地 segment 状态与远端元信息，异常进入 `failed` 或 `needs_attention` |
| 6 | 合并验收 | 所有 segments completed 且临时文件大小等于 `total_size` 后 rename |
| 7 | 回归测试 | 覆盖分片规划、多 Range 写入、恢复跳过已完成段、segment 失败不 rename、SHA-256 完整性 |

### 下一迭代建议

- 进入阶段 3：先实现 Settings 表 + 全局队列调度器 v1。
- 保持单任务内部 1/4 segment 策略不变，先控制任务级并发。
- Settings 页先实现默认保存目录和同时下载任务数，限速后置。
- 继续保持 `pnpm typecheck`、`pnpm build`、`pnpm specta`、`cargo check`、`clippy`、`cargo test` 全绿。

### 刻意延后

- 浏览器扩展（阶段 5）
- 磁力 / BT 等非 HTTP 协议
- 装饰性 Mica/玻璃态大面积铺陈
- 无引擎支撑的高级可视化

---

## 风险与依赖

| 项 | 说明 |
|----|------|
| **大整数与 IPC** | 前端/API 使用 `f64`；引擎内部建议 `u64`，在边界显式转换 |
| **Windows 发布** | WebView2、L2 `pnpm tauri build` 需持续验证 |
| **设计文档** | 恢复 `docs/functional-design.md` 可减少需求口头漂移 |
| **Specta** | 禁止直接导出 `i64` 等；新增命令/事件类型需符合 specta-typescript 约束 |

---

## 文档索引

- [PRODUCT.md](../PRODUCT.md) — 产品目的、用户、优先级、成功标准
- [DESIGN.md](../DESIGN.md) — 设计系统、组件、动效、无障碍
- [README.md](../README.md) — 环境、脚本、CI、架构摘要

---

*最后更新：2026-06-06；阶段 2 已通过验收，下一步进入阶段 3 队列与设置。*
