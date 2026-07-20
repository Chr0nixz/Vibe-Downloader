# 协议可靠性与诊断矩阵

最后更新：2026-07-20

本矩阵是非 HTTP 协议的统一验收基线。`automated` 表示已有自动化证据；`partial` 表示能力存在但只覆盖部分路径或仍依赖人工验收；`unsupported` 表示产品明确拒绝，不能宣传为支持；`n/a` 表示该能力不适用于该协议。

| Protocol | Create | Probe | Pause | Resume | Cancel | Retry | Proxy | Credentials | Checksum | Restart | Diagnostics | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| FTP/FTPS | automated | automated | automated | automated | automated | automated | automated | automated | automated | automated | automated | `src-tauri/tests/ftp_engine.rs` (cred rotation / 550 permission / SOCKS5 fail / implicit FTPS+SOCKS5 reject / pause-resume), `src-tauri/tests/directory_probe.rs`, `src-tauri/tests/proxy.rs`, `src-tauri/tests/segments.rs`, Rust `download::ftp` unit tests |
| SFTP | automated | automated | automated | automated | automated | automated | automated | automated | automated | automated | automated | `src-tauri/tests/sftp_engine.rs` (cred rotation password+key / permission / proxy fail / host-key forget-retry / pause-resume), `src-tauri/tests/directory_probe.rs`, `src-tauri/tests/sftp_concurrency_poc.rs`, `src-tauri/tests/segments.rs` |
| BitTorrent | automated | automated | automated | automated | automated | automated | automated | n/a | automated | automated | automated | `src-tauri/tests/bt_engine.rs` (HTTP `.torrent` probe / magnet hash boundary `files=[]` / SOCKS5 no-bypass / `bt_torrent_fetch_failed`+`bt_magnet_invalid` recoverability / piece-hash contract not task `expected_hash`), `src-tauri/tests/segments.rs` (Restart DB 合同；BT 跨 librqbit session 再入非 C5 门槛), Rust `download::bt` unit tests (C3: ARC-12 / FUN-11 / ARC-13 / FUN-15) |
| HLS | automated | automated | automated | automated | automated | automated | automated | automated | n/a | automated | automated | `src-tauri/tests/hls_engine.rs` (persisted Basic Auth / 401 `http_denied` / live idle / track fail-visible / control-plane size / `reset_interrupted_tasks` cold engine reentry), `src-tauri/tests/segments.rs`, Rust `download::hls` unit tests |
| DASH | automated | automated | automated | automated | automated | automated | automated | automated | n/a | automated | automated | `src-tauri/tests/dash_engine.rs` (segment retry / persisted Basic Auth / 401 MPD / `reset_interrupted_tasks` cold engine reentry) + `tests/fixtures/dash/` corpus (dynamic / timeline / multi-Period / `$Time$` reject), `src-tauri/tests/segments.rs`, Rust `download::dash` unit tests |
| WebDAV/WebDAVS | automated | automated | automated | automated | automated | automated | automated | automated | automated | automated | automated | `src-tauri/tests/webdav_engine.rs` (cred rotation / 401+403 probe+download / PROPFIND 403 / pause-resume), `src-tauri/tests/directory_probe.rs`, `src-tauri/tests/http_engine.rs`, `src-tauri/tests/segments.rs` |
| Metalink4 | automated | automated | automated | automated | automated | automated | automated | automated | automated | automated | automated | `src-tauri/tests/metalink_engine.rs` (persisted Basic Auth serial+parallel header forward / SOCKS5 no-bypass / all-mirror 401 / `reset_interrupted_tasks` cold engine reentry; `fun09_*` resume validators / Content-Range / cross-mirror checksum gate), Rust `download::metalink` unit tests (`fun08_*` strongest-hash + primary completion) |

暂停/恢复的共享持久化层已由跨协议测试覆盖：任务、文件、work unit、重试时间和事件在同一事务提交，且暂停/重新入队不会删除临时文件或提前发布最终文件。FTP 与 WebDAV 已通过传输中取消、持久化偏移和字节级恢复测试；HLS 与 DASH 已通过 staging 中断、复用已完成分片、重新获取未完成分片并完成 ffmpeg remux 的测试。Metalink 任务级暂停/恢复已通过串行 temp、并行 part-file 与多文件（已完成文件不重下）自动化覆盖；FUN-08/FUN-09 已补齐 strongest-hash 完成汇总与跨镜像续传一致性（If-Range / Content-Range / checksum 门槛）。C2（ARC-11/FUN-10/ARC-10/FUN-12）已锁定 HLS live idle、外挂轨失败可见、控制面 64 MiB 流式上限，以及 DASH static/VOD Boundary 明确拒绝合同。C3（ARC-12/FUN-11/ARC-13/FUN-15）已锁定 BT session 引用计数与按任务限速隔离、做种 ratio/time 任一达标、任务聚合进度与 per-file 进度分离，以及 tracker configured-only / seed_count 诚实展示诊断合同。C4（FUN-18 子集）已将 FTP/SFTP/WebDAV 的 Retry 与 Diagnostics 升至 automated。C5（FUN-18 Closed）已将 BT/HLS/DASH/Metalink 剩余 `partial` 升至 `automated`：HLS/DASH/Metalink 持久化 Basic Auth 接线；BT probe/proxy/retry/checksum 合同与诊断；以及 HLS/DASH/Metalink 在 `reset_interrupted_tasks(auto_resume=true)` 后由**新引擎实例**带着既有 temp/staging 续传完成（不要求 fork 新 OS 进程；BT 跨 librqbit session 再入仍依赖 `segments.rs` DB 合同，非本批门槛）。

## 发布判定

- 矩阵核心生命周期单元格已全部 `automated` 或诚实 `n/a`；剩余可靠性债务见审计中仍 Open 的非 FUN-18 项（例如 FUN-02 运行时代理、FUN-07 计划窗口）。
- 所有 `unsupported` 项必须在创建或探测阶段明确拒绝，并返回稳定错误；不得静默降级到绕过安全或代理设置的路径。
- 将单元格升级为 `automated` 时，Evidence 必须给出仓库内测试路径或明确的 Rust 单元测试模块。
- 发布候选至少运行矩阵校验、对应 Rust 测试，并保存真实服务端或媒体源的人工验收记录。

## 下一轮优先顺序

1. 关闭仍 Open 的非矩阵项（例如 `FUN-02` 运行时代理贯通、`FUN-07` 计划窗口），再进入 Phase D。
2. 在扩展新协议或新格式前，保持 `pnpm verify:protocol-matrix` 与对应引擎集成测试绿。
3. 真实外部服务/媒体源的人工验收记录仍作为发布候选补充证据，不替代自动化单元格。
