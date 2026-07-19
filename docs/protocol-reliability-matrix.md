# 协议可靠性与诊断矩阵

最后更新：2026-07-18

本矩阵是非 HTTP 协议的统一验收基线。`automated` 表示已有自动化证据；`partial` 表示能力存在但只覆盖部分路径或仍依赖人工验收；`unsupported` 表示产品明确拒绝，不能宣传为支持；`n/a` 表示该能力不适用于该协议。

| Protocol | Create | Probe | Pause | Resume | Cancel | Retry | Proxy | Credentials | Checksum | Restart | Diagnostics | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| FTP/FTPS | automated | automated | automated | automated | automated | automated | automated | automated | automated | automated | partial | `src-tauri/tests/ftp_engine.rs`, `src-tauri/tests/proxy.rs`, `src-tauri/tests/segments.rs`, Rust `commands::tasks::actions` unit tests |
| SFTP | automated | automated | automated | automated | automated | partial | automated | automated | automated | automated | partial | `src-tauri/tests/sftp_engine.rs`, `src-tauri/tests/sftp_concurrency_poc.rs`, `src-tauri/tests/segments.rs`, Rust `commands::tasks::actions` unit tests |
| BitTorrent | automated | partial | automated | automated | automated | partial | partial | n/a | partial | automated | partial | `src-tauri/tests/engine_routing.rs`, `src-tauri/tests/segments.rs`, Rust `download::bt` unit tests |
| HLS | automated | automated | automated | automated | automated | automated | automated | partial | n/a | automated | partial | `src-tauri/tests/hls_engine.rs`, `src-tauri/tests/segments.rs`, Rust `download::hls` unit tests |
| DASH | automated | automated | automated | automated | automated | partial | automated | partial | n/a | automated | partial | `src-tauri/tests/dash_engine.rs`, `src-tauri/tests/segments.rs` |
| WebDAV/WebDAVS | automated | automated | automated | automated | automated | automated | automated | automated | automated | automated | partial | `src-tauri/tests/webdav_engine.rs`, `src-tauri/tests/http_engine.rs`, `src-tauri/tests/segments.rs`, Rust `commands::tasks::actions` unit tests |
| Metalink4 | automated | automated | automated | automated | automated | automated | partial | partial | automated | automated | partial | `src-tauri/tests/metalink_engine.rs` (`download_pauses_mid_transfer_and_resumes_serial_temp_file`, `download_pauses_mid_transfer_and_resumes_parallel_part_files`, `download_pauses_second_file_without_redownloading_completed_file`), `src-tauri/tests/segments.rs` |

暂停/恢复的共享持久化层已由跨协议测试覆盖：任务、文件、work unit、重试时间和事件在同一事务提交，且暂停/重新入队不会删除临时文件或提前发布最终文件。FTP 与 WebDAV 已通过传输中取消、持久化偏移和字节级恢复测试；HLS 与 DASH 已通过 staging 中断、复用已完成分片、重新获取未完成分片并完成 ffmpeg remux 的测试。Metalink 任务级暂停/恢复已通过串行 temp、并行 part-file 与多文件（已完成文件不重下）自动化覆盖。

## 发布判定

- `partial` 项仍是可靠性债务，协议不能被描述为与 HTTP/HTTPS 同等成熟。
- 所有 `unsupported` 项必须在创建或探测阶段明确拒绝，并返回稳定错误；不得静默降级到绕过安全或代理设置的路径。
- 将单元格升级为 `automated` 时，Evidence 必须给出仓库内测试路径或明确的 Rust 单元测试模块。
- 发布候选至少运行矩阵校验、对应 Rust 测试，并保存真实服务端或媒体源的人工验收记录。

## 下一轮优先顺序

1. 为各协议增加跨进程重启测试。
2. 为 BitTorrent、HLS、DASH 增加进程外依赖与网络失败诊断；继续补充 WebDAV 拒绝访问、BitTorrent 代理和凭据更新后的恢复测试。
3. 补齐仍为 `partial` 的重试、代理、凭据和诊断单元格，保持稳定错误码、恢复动作和临时文件一致性证据。
4. 在扩展新格式前，将每个协议的核心生命周期至少提升为自动化覆盖。
