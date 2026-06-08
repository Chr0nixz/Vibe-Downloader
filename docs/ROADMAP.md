# Vibe Downloader 路线图

最后更新：2026-06-08

本文档描述当前代码库之后的开发路线。已实现状态以仓库代码为准，产品和设计约束见 [PRODUCT.md](../PRODUCT.md) 与 [DESIGN.md](../DESIGN.md)。

## 当前基线

当前版本：`0.1.0`。

当前仓库已经完成一个可运行的 HTTP/HTTPS 下载管理器基础版本：

- 桌面壳：Tauri 2 + React 19 + Rust，Windows/macOS/Linux 配置已存在。
- 下载核心：HTTP probe、未知大小单连接下载、Range 分段下载、续传校验、分段重试、全局限速。
- 持久化：SQLite 保存任务、分段、设置、浏览器 handoff 消息。
- 调度：全局活跃任务上限、per-host 连接槽限制、queued FIFO 调度、启动时中断任务重置为 paused。
- UI：任务列表、状态导航、搜索、设置页、详情页、Chunks/Connections、toast、删除确认、基础恢复动作、中文/英文 i18n。
- 浏览器交接：WebExtension 开发包、Native Messaging host、manifest 安装/卸载、request id 去重、单实例转发。
- 发布链路：CI、三平台 Tauri build、Release workflow、Tauri updater 配置和状态栏安装入口。

## 已知边界

这些能力尚未完成，不应在发布说明或产品文案中描述为已支持：

- 命令面板还不是完整命令中心，生产构建中无实际用户命令。
- 顶部速度限制按钮尚未绑定快捷设置行为。
- 新建下载没有自动 probe，也没有复用 probe 结果减少二次等待。
- 任务详情没有 Logs、Request、真实逐连接速度和请求头诊断。
- `task_events` 表未写入完整生命周期数据。
- 浏览器扩展不自动接管浏览器下载，不转发 Cookie/header，不包含商店发布 ID 和签名流程。
- Safari wrapper、系统托盘、系统通知、开机启动、关闭到托盘、批量导入、任务优先级、单任务限速、文件分类规则尚未实现。
- HTTP 之外的 BT、HLS、网盘解析、视频嗅探和插件协议尚未实现。

## P0：发布前收敛

目标：让 `0.1.x` 能作为可信的 HTTP 下载器进行内部/公开预览。

1. 完善命令面板
   - 新建下载、暂停/继续、删除、重试、打开文件/目录、切换视图、打开设置。
   - 生产构建中不显示 mock reset。

2. 收紧主界面限速入口
   - 顶部速度限制按钮接入快捷菜单。
   - 预设值：不限速、512 KB/s、1 MB/s、5 MB/s、10 MB/s、自定义。
   - 继续保留设置页精确 B/s 输入。

3. 优化新建下载流程
   - URL 停止输入后自动 probe。
   - 提交时尽量复用最近一次 probe 结果，或明确展示重新验证状态。
   - probe 失败时提供可控的继续尝试路径。

4. 完成任务事件日志闭环
   - 写入 `task_events`：created、started、paused、resumed、retrying、failed、completed、resume_blocked。
   - 详情页增加 Logs tab。

5. 补齐发布安全检查
   - 确认生产构建不可调用 debug-only mock command。
   - 验证 updater `latest.json`、签名、安装包和失败 UI。
   - 复查 Tauri capabilities 是否仍是最小权限集。

## P1：核心体验完善

目标：让日常下载管理更接近成熟工具。

1. 批量操作
   - 多选任务。
   - 批量暂停、继续、删除、重试、打开目录。

2. 排序和筛选
   - 按创建时间、更新时间、文件大小、进度、速度、状态排序。
   - 按文件类型、来源域名、失败原因、是否支持续传筛选。

3. 错误恢复体验
   - 为结构化错误补齐本地化文案和恢复动作。
   - 对远端文件变化、Range 不可用、临时文件缺失、磁盘写入失败分别给出明确路径。

4. 设置页单位友好化
   - 限速支持 KB/s、MB/s、GB/s 单位输入。
   - 多连接阈值支持 MB/GB 单位输入。
   - 高级项和普通项分组，避免普通用户直接面对原始字节值。

5. 完成通知和基础桌面集成
   - 下载完成系统通知。
   - 系统托盘常驻、关闭到托盘、开机启动作为独立开关。

## P2：诊断和可靠性增强

目标：让大文件、失败恢复和浏览器交接更可诊断。

1. 真实连接诊断
   - 后端维护每个 segment/connection 的实时速度、重试、最近错误。
   - Connections tab 不再用任务总速度均分。

2. Request tab
   - 展示最终 URL、响应状态、关键响应头、Range/Content-Range 信息。
   - 保持 URL query 和敏感 header 默认隐藏。

3. 续传策略增强
   - 区分强/弱 ETag。
   - 记录 Range 能力变化。
   - 对弱元数据资源给出风险提示。

4. Native Messaging 稳定性
   - handoff 文件先写临时文件再 rename。
   - request id 使用 create-new 语义避免覆盖。
   - 读入失败也写入诊断结果，并清理过期 handoff 文件。

5. 性能
   - 活跃任务主要依赖事件更新，降低完整列表兜底刷新频率。
   - 详情页 segment 数据改为订阅/分页/摘要，减少轮询压力。
   - 任务列表虚拟化，支持 1000+ 历史任务。

## P3：浏览器集成产品化

目标：从开发验证扩展走向可发布浏览器工作流。

1. 扩展发布身份
   - Chrome Web Store / Edge Add-ons / Firefox AMO ID。
   - Native Messaging manifest 区分开发 ID 与发布 ID。
   - 扩展版本和 app 版本同步策略。

2. 安装引导
   - 设置页展示扩展加载路径、manifest 路径、native host 路径和复制诊断信息。
   - 对常见浏览器给出平台化指引。

3. Safari
   - macOS Safari Web Extension wrapper。
   - 单独签名、打包、审核流程。

4. 后续浏览器能力
   - 自动接管大文件下载提示。
   - 站点级规则。
   - 扩展内实时任务状态面板。
   - Cookie/header 转发必须建立明确授权和脱敏边界后再做。

## P4：协议和高级能力

目标：在 HTTP 下载稳定后再扩展协议面。

优先级建议：

1. 批量 URL 导入和校验。
2. HLS 下载。
3. 校验文件完整性（hash 输入/自动识别）。
4. BT 下载。
5. 插件协议或协议适配层。

明确延后：

- 网盘解析。
- 视频嗅探。
- 云账号/跨设备同步。
- 无实际下载引擎支撑的高级可视化。

## 验证基线

每个重要变更至少运行：

```bash
pnpm typecheck
pnpm build
pnpm check:bindings
pnpm test:rust
cargo check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
```

浏览器集成相关变更额外运行：

```bash
pnpm build:extensions
```

发布相关变更额外验证：

```bash
pnpm tauri build --config src-tauri/tauri.ci.conf.json
```
