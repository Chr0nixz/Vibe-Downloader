# 项目改进审计

最后更新：2026-06-08

本审计基于当前仓库代码、配置、测试和文档状态。它不替代 [docs/ROADMAP.md](ROADMAP.md)，而是按风险和用户影响列出需要继续处理的问题。

## 总体结论

项目已经超过最早 HTTP MVP：后端下载核心、SQLite 持久化、分段续传、队列调度、设置、日志、浏览器 Native Messaging 基础、自动更新配置和 CI/CD 验证都已经落地。

当前主要问题集中在四类：

1. 用户操作入口还不完整：命令面板、限速快捷菜单、批量操作、排序筛选仍偏 MVP。
2. 诊断深度不足：任务日志、请求详情、真实逐连接速度和错误时间线还没形成闭环。
3. 浏览器集成仍是开发验证阶段：缺少商店 ID、签名、Safari wrapper、自动接管和发布安装引导。
4. 大列表和高频进度性能仍需打磨：任务列表未虚拟化，详情页 segment 仍使用轮询。

## 已确认优势

- HTTP probe 支持 HEAD 与 Range GET fallback，并识别文件名、大小、content type、Range 能力和来源 host。
- 下载引擎支持未知大小单连接、Range 分段、分段 retry、断点续传校验和全局 token bucket 限速。
- 完成文件不再覆盖同名已有文件，会自动选择可用文件名。
- `seed_mock_tasks` 只在 debug 构建注册，生产 invoke handler 不包含该命令。
- 设置项已包含默认保存目录、活跃任务上限、全局限速、多连接阈值、初始分段数和 per-host 最大连接数。
- 队列调度已按 `max_active_tasks` 和 per-host 连接槽限制启动任务。
- 结构化错误模型 `AppErrorPayload` 已存在，前端可解析恢复动作。
- UI 已具备搜索、状态导航、行展开、详情抽屉、Chunks/Connections、toast、删除确认和基础恢复动作。
- CI 覆盖前端 typecheck/build、Rust check/clippy/test、Specta 绑定漂移和三平台 Tauri build。

## P0：发布前必须处理

### 1. 命令面板只有开发命令

当前 `Mod+K` 打开后，生产构建没有实际用户命令；开发构建主要是 mock reset。这会让高可见入口失去价值。

建议：

- 添加新建下载、暂停/继续、删除、重试、打开文件、打开目录、切换导航、打开设置。
- 对无选中任务或状态不支持的动作禁用。
- 生产构建完全隐藏 mock reset。

### 2. 主界面限速入口未绑定行为

顶部工具栏有速度限制按钮，但没有菜单或动作。真实限速只能在设置页用 B/s 输入。

建议：

- 接入快捷菜单：不限速、512 KB/s、1 MB/s、5 MB/s、10 MB/s、自定义。
- 菜单值写入 `global_speed_limit_bps`。
- 状态栏继续显示当前限速。

### 3. 新建下载存在重复等待

用户需要手动点击 Detect；提交时后端会再次 probe。对大文件或慢服务器会造成两次等待。

建议：

- URL 输入停止 500-800ms 后自动 probe。
- 提交时复用最近一次有效 probe，或显示“正在重新验证资源”。
- probe 失败时明确允许“仍然尝试下载”的条件和风险。

### 4. 任务日志表没有业务闭环

`task_events` 表已存在，但核心任务生命周期没有系统性写入，详情页也没有 Logs tab。

建议：

- 写入 task created/started/progress checkpoint/paused/resumed/retrying/failed/completed。
- 写入 segment retry/fail 和 resume blocked。
- 详情页增加 Logs tab，先展示生命周期、错误和重试记录。

### 5. 发布链路需要一次端到端演练

配置已经存在，但正式发布前仍需要实际验证。

建议：

- 用测试 tag 触发 Release workflow。
- 确认 `latest.json`、`.sig`、安装包和版本号一致。
- 验证打包应用能检查更新、安装并 relaunch。
- 在 Windows/macOS 未配置代码签名前，发布说明明确 unsigned 风险。

## P1：核心体验完善

### 1. 批量任务管理缺失

当前主要围绕单选任务操作，不适合大量下载任务。

建议：

- 多选任务。
- 批量暂停、继续、删除、重试。
- 批量打开目录可以先只对首个任务执行，避免同时打开大量窗口。

### 2. 排序和筛选不足

已有搜索和状态导航，但缺少常用下载管理维度。

建议：

- 排序：创建时间、更新时间、文件大小、进度、速度、状态。
- 筛选：来源域名、文件类型、失败原因、是否支持续传。

### 3. 设置输入对普通用户不友好

全局限速和多连接阈值仍使用原始 B/s 和字节值。

建议：

- 支持 KB/s、MB/s、GB/s。
- 支持 MB、GB 阈值输入。
- 高级项增加折叠分组，保留准确值但不强迫普通用户理解字节单位。

### 4. 错误恢复动作覆盖仍不完整

结构化错误模型已经存在，但后端并非所有失败都返回结构化 payload，前端也不是所有恢复动作都有完整交互。

建议：

- 将 remote changed、resume unavailable、temp file missing、disk write failed、HTTP 403/404/429 都稳定映射为 `AppErrorPayload`。
- 对 `choose_another_folder`、`choose_another_name`、`restart` 提供完整确认路径。
- 对 `retry_later` 明确是立即重新排队还是延迟调度。

## P2：可靠性和诊断

### 1. Connections tab 的速度不是逐连接真实值

当前连接详情用任务总速度按活跃 segment 平均分配。这对诊断慢连接、服务器限流、磁盘瓶颈价值有限。

建议：

- 后端维护 segment/connection 级实时速度。
- `list_task_segments` 或新接口返回真实速度、最近错误、首包/重试信息。
- UI 明确区分 segment 和 live connection。

### 2. 详情页缺少请求信息

用户无法在 UI 中看到最终 URL、响应状态、Range/Content-Range 和关键响应头。

建议：

- 增加 Request tab。
- 默认隐藏敏感 URL query 和 header。
- 提供复制诊断摘要，而不是直接展示所有原始 header。

### 3. Native Messaging handoff 文件写入可更稳

native host 目前直接写 request id JSON 文件，再启动主程序读取。

建议：

- 先写临时文件，完成后 rename。
- request id 文件使用 create-new 避免覆盖。
- 主程序读取失败也记录 `browser_messages` 诊断。
- 定期清理过期 handoff 文件。

### 4. 启动恢复策略偏保守

启动时 downloading/retrying 会重置为 paused，不会自动恢复。

建议：

- 设置页增加“启动后自动恢复未完成任务”开关。
- 默认可继续保守关闭。
- 自动恢复前仍执行本地临时文件与远端元数据校验。

## P3：性能

### 1. 任务列表未虚拟化

目标体验是 1000+ 历史任务仍可流畅浏览，但当前直接渲染过滤后的任务列表。

建议：

- 引入虚拟列表。
- 活跃任务可置顶或单独小集合渲染。
- 非活跃任务减少 tooltip、动画和 sparkline 成本。

### 2. 活跃任务期间仍有完整列表刷新

前端接收 `task.progress` 和 `task.updated`，同时 `queue.changed` 会触发完整列表刷新。虽然已有 100ms debounce 和 merge，仍可能增加 DB/invoke/渲染压力。

建议：

- 活跃任务主要由事件更新。
- 任务完成、失败、队列变化时刷新完整列表。
- 兜底刷新降到 2-5 秒并保留 merge 防回退。

### 3. 详情页 segment 轮询

Chunks/Connections 打开时对活跃任务每 2.5 秒拉取 segment。

建议：

- 后端按详情订阅推送 segment 摘要。
- 或先做分页/摘要接口，避免一次返回大量 segment。

## P4：浏览器集成产品化

### 当前已支持

- 右键链接发送 URL。
- 选中文本中的 URL 发送。
- popup 发送当前 tab URL。
- Native Messaging host 接收、校验、写 handoff 文件、启动/转发 app。
- Settings 页面安装/卸载 native host manifest。

### 当前未支持

- 自动接管浏览器下载。
- Cookie/header 转发。
- 站点规则。
- 商店扩展 ID 和签名。
- Safari 生产 wrapper。
- 扩展内实时任务状态面板。

建议先完成 Chrome/Edge/Firefox 的端到端开发验证矩阵，再做发布身份和安装引导。

## 建议验证命令

常规变更：

```bash
pnpm typecheck
pnpm build
pnpm check:bindings
pnpm test:rust
cargo check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
```

浏览器相关：

```bash
pnpm build:extensions
```

发布相关：

```bash
pnpm tauri build --config src-tauri/tauri.ci.conf.json
```
