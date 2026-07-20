# 协议可靠性与诊断矩阵

最后更新：2026-07-20

本矩阵是非 HTTP 协议的统一验收基线。`automated` 表示已有自动化证据；`partial` 表示能力存在但只覆盖部分路径或仍依赖人工验收；`unsupported` 表示产品明确拒绝，不能宣传为支持；`n/a` 表示该能力不适用于该协议。

| Protocol | Create | Probe | Pause | Resume | Cancel | Retry | Proxy | Credentials | Checksum | Restart | Diagnostics | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| FTP/FTPS | automated | automated | automated | automated | automated | automated | automated | automated | automated | automated | automated | `src-tauri/tests/ftp_engine.rs` (cred rotation / 550 permission / SOCKS5 fail / implicit FTPS+SOCKS5 reject / pause-resume), `src-tauri/tests/directory_probe.rs`, `src-tauri/tests/proxy.rs`, `src-tauri/tests/segments.rs`, Rust `download::ftp` unit tests |
| SFTP | automated | automated | automated | automated | automated | automated | automated | automated | automated | automated | automated | `src-tauri/tests/sftp_engine.rs` (cred rotation password+key / permission / proxy fail / host-key forget-retry / pause-resume), `src-tauri/tests/directory_probe.rs`, `src-tauri/tests/sftp_concurrency_poc.rs`, `src-tauri/tests/segments.rs` |
| BitTorrent | automated | partial | automated | automated | automated | partial | partial | n/a | partial | automated | partial | `src-tauri/tests/engine_routing.rs`, `src-tauri/tests/segments.rs`, Rust `download::bt` unit tests (C3: ARC-12 session ownership / FUN-11 seeding limits / ARC-13 per-file progress / FUN-15 configured-only trackers) |
| HLS | automated | automated | automated | automated | automated | automated | automated | partial | n/a | automated | partial | `src-tauri/tests/hls_engine.rs` (live idle / track fail-visible / control-plane size), `src-tauri/tests/segments.rs`, Rust `download::hls` unit tests |
| DASH | automated | automated | automated | automated | automated | partial | automated | partial | n/a | automated | partial | `src-tauri/tests/dash_engine.rs` + `tests/fixtures/dash/` corpus (dynamic / timeline / multi-Period / `$Time$` reject), `src-tauri/tests/segments.rs`, Rust `download::dash` unit tests |
| WebDAV/WebDAVS | automated | automated | automated | automated | automated | automated | automated | automated | automated | automated | automated | `src-tauri/tests/webdav_engine.rs` (cred rotation / 401+403 probe+download / PROPFIND 403 / pause-resume), `src-tauri/tests/directory_probe.rs`, `src-tauri/tests/http_engine.rs`, `src-tauri/tests/segments.rs` |
| Metalink4 | automated | automated | automated | automated | automated | automated | partial | partial | automated | automated | partial | `src-tauri/tests/metalink_engine.rs` (pause/resume; `fun09_*` resume validators / Content-Range / cross-mirror checksum gate), Rust `download::metalink` unit tests (`fun08_*` strongest-hash + primary completion) |

暂停/恢复的共享持久化层已由跨协议测试覆盖：任务、文件、work unit、重试时间和事件在同一事务提交，且暂停/重新入队不会删除临时文件或提前发布最终文件。FTP 与 WebDAV 已通过传输中取消、持久化偏移和字节级恢复测试；HLS 与 DASH 已通过 staging 中断、复用已完成分片、重新获取未完成分片并完成 ffmpeg remux 的测试。Metalink 任务级暂停/恢复已通过串行 temp、并行 part-file 与多文件（已完成文件不重下）自动化覆盖；FUN-08/FUN-09 已补齐 strongest-hash 完成汇总与跨镜像续传一致性（If-Range / Content-Range / checksum 门槛）。C2（ARC-11/FUN-10/ARC-10/FUN-12）已锁定 HLS live idle、外挂轨失败可见、控制面 64 MiB 流式上限，以及 DASH static/VOD Boundary 明确拒绝合同。C3（ARC-12/FUN-11/ARC-13/FUN-15）已锁定 BT session 引用计数与按任务限速隔离、做种 ratio/time 任一达标、任务聚合进度与 per-file 进度分离，以及 tracker configured-only / seed_count 诚实展示诊断合同。C4（FUN-18 子集）已将 FTP/SFTP/WebDAV 的 Retry 与 Diagnostics 升至 automated：目录探测 E2E、凭据轮换、代理/权限失败稳定码、implicit FTPS+SOCKS5 显式拒绝、SFTP host-key forget→retry、以及引擎级 pause/resume（含 SFTP 本地 temp seek 续传修复）。

## 发布判定

- `partial` 项仍是可靠性债务，协议不能被描述为与 HTTP/HTTPS 同等成熟。
- 所有 `unsupported` 项必须在创建或探测阶段明确拒绝，并返回稳定错误；不得静默降级到绕过安全或代理设置的路径。
- 将单元格升级为 `automated` 时，Evidence 必须给出仓库内测试路径或明确的 Rust 单元测试模块。
- 发布候选至少运行矩阵校验、对应 Rust 测试，并保存真实服务端或媒体源的人工验收记录。

## 下一轮优先顺序

1. 为各协议增加跨进程重启测试（C5）。
2. 为 BitTorrent、HLS、DASH、Metalink 补齐仍为 `partial` 的 retry/proxy/credentials/diagnostics 单元格。
3. 在扩展新格式前，将每个协议的核心生命周期至少提升为自动化覆盖。
