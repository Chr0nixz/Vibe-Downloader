# 工程质量审计（测试 / CI / 迁移 / 依赖 / 文档 / 脚本）

最后更新：2026-07-02

本文是 Vibe Downloader `0.2.0` 第三轮深度代码审查的「工程质量」分册，聚焦 b4ba790（+18866 行，*"feat: major protocol engine expansion, UI overhaul, and infrastructure hardening"*）提交之后的测试覆盖、CI 工作流、数据库迁移纪律、依赖健康、文档一致性与脚本/钩子质量，并核对构建配置与仓库卫生。

审查方式为只读核实（`Read`/`Grep`/`Glob` + 只读 `git log`/`git show`/`git diff` 命令），**未修改任何文件**。本文与仓库内既有审计文档互补、不重复：

- [architecture-audit.md](architecture-audit.md) — 用户交互便捷性 / 功能完整性 / 架构鲁棒性 / 运行效率四维复核。
- [rust-backend-audit.md](rust-backend-audit.md) — Rust 后端可扩展性、效率、安全性专项。
- [dependency-modernization-audit.md](dependency-modernization-audit.md) — 依赖选型与现代化专项。
- [project-improvement-audit.md](project-improvement-audit.md) — 全局优先级与发布阻断项汇总。

## 总体结论

测试规模与 CI 骨架齐全，HTTP/HTTPS 主路径、SSRF 分类器、DB 状态机竞态、SFTP 并发会话等关键面确有真实端到端覆盖。但本轮核实发现，工程化成熟度与本次提交新增的约 1.9 万行代码**不完全相称**，集中在四类问题：

1. **迁移与测试自相矛盾**：`003_hls_track_selection.sql` 已落地，但 `migration_integrity.rs` 仍硬编码断言只有 2 个迁移——这是已提交状态本身的矛盾，提示 CI 可能带红合并（见 C-0）。
2. **新协议引擎的"编排层"测试缺口**：F-4 Metalink 并行只测了分段原语与 DB 健康层，真正的多镜像故障转移编排从未被驱动；HLS/DASH 集成测试因 CI 环境缺 ffmpeg 而静默全跳过。
3. **并发回归测试缺失**：调度器 dispatch 与 pause/cancel/delete 的真实并发场景无任何回归测试，而这正是最容易"静默停摆"的一类风险。
4. **CI 安全闸门不全**：无 `cargo fmt --check`、无 `cargo audit`/`cargo-deny`，转发头 allowlist 一致性校验脚本（`verify:manifest`）从未接入任何工作流。

严重度图例：🔴 高（会放走回归/发布事故）🟡 中（明显可靠性/一致性损耗）🟢 低（局部优化或置信度较低的观察项）

---

## 第一部分：工程化记分卡

### 一、测试覆盖（评级 B-：量大但结构性偏科）

规模：`src-tauri/tests/` 共 **25 个集成测试文件 / 9717 行 / 约 428 个测试函数**，另有源码内 `#[cfg(test)]` 单测约 30 处分布于 `ssrf.rs`(13)、`sanitize.rs`(21)、`retry.rs`(14)、`hls.rs`(11)、`dash.rs`(10)、`bt.rs`(10) 等模块。前端 vitest 仅 **8 文件 / 473 行**。

- **HTTP 主路径**（`tests/http_engine.rs`，1226 行/50 测试）最扎实：用 `TestServer` 跑真实下载，覆盖未知大小单流、Range 分段、`If-Range`/`Content-Range` 续传校验、限速、取消中途返回、redirect 文件名解析。
- **F-4 Metalink 并行**（`tests/metalink_engine.rs`，1224 行/45 测试）只测了两个经 `#[doc(hidden)] pub mod testing`（`src/download/metalink.rs:1779`）暴露的 AppHandle-free 函数——`download_metalink_range_from_mirror` + `assemble_metalink_part_files`——加 DB 健康层（`list_healthy_mirrors_for_file` 等冷却/排序逻辑）。真正的编排器 `download_metalink_file_parallel`（`metalink.rs:389`）与故障转移队列 `MetalinkRangeWorker`（`metalink.rs:740`）**从未被集成测试驱动**。
- **调度器并发**（`tests/scheduler_logic.rs`，390 行/22 测试）只测纯函数（`compute_available_slots`/`should_skip_for_schedule_window`/`compute_planned_slots`）与 DB 条件 UPDATE（通过调换调用顺序模拟竞态，注释自述 "no spawned workers"）；dispatch 与 pause/cancel/delete 的**真并发**场景无任何回归测试。
- **SSRF 模块**（`src/download/ssrf.rs` 13 个单测 + `tests/browser_handoff.rs` 28 个测试）覆盖全面：IPv4 私有段/CGNAT(`100.64.0.0/10`)/`0.0.0.0/8`、IPv6 loopback/ULA/link-local、IPv4-mapped-IPv6、DNS rebind 拦截、handoff 层字面 IP 拒绝。但引擎层连接期防护（`src/download/http/mod.rs:299` 的 `HickoryResolver::resolve` 过滤 + `http/mod.rs:322` 的 `ssrf_safe_redirect_policy`）**无集成测试**验证公网域名解析到私网 IP 时下载真的会在连接期被阻断。
- **HLS/DASH**（`tests/hls_engine.rs` 14 测试、`tests/dash_engine.rs` 12 测试）只测 `probe()` 清单解析，且每个用例开头 `if !ffmpeg_available() { return; }` 做门控——真实分段下载/AES-128-CBC 解密/ffmpeg remux 需要 `AppHandle`，未被集成测试触达（CI 是否装了 ffmpeg 见下节）。
- **SFTP**（`tests/sftp_engine.rs`，496 行/23 测试）是三个新协议里最真实的：用内存 russh 服务器测试了 Path A 并发分段读（独立 SSH 通道）、offset 续传、认证失败、host-key TOFU/mismatch、read-error 注入。
- **前端**：8 个 vitest 文件（`settings-search.test.ts`/`task-layout.test.ts`/`task.test.ts`/`use-shell-layout.test.ts`/`use-task-events.test.ts`/`errors.test.ts`/`task-query.test.ts`/`i18n/index.test.ts`）全部是纯逻辑 helper 测试（`normalizeTask`、错误恢复模型、`detectInitialLocale`、task-query 游标过滤），**零组件/集成测试**，近期 UX 修复（双防抖、空列表分支、队列重排乐观更新）无回归覆盖。

### 二、CI 工作流（评级 C+：基础齐全，安全/一致性闸门缺失）

`.github/workflows/ci.yml` 两个 job：`frontend`（`pnpm typecheck` → `biome check` → `pnpm test:frontend` → `pnpm build`）、`rust`（`cargo check` → `cargo clippy -- -D warnings` → `pnpm test:rust` → `pnpm check:bindings`）。`tauri-build.yml` 三平台（Windows/macOS/Linux）用 `tauri.ci.conf.json`（关闭 updater 产物）做构建验证。`release.yml` 四目标矩阵（macOS arm64/x64、Linux x64、Windows x64）+ Tauri updater 签名 + `includeUpdaterJson: true` + `scripts/sync-version.mjs` 自动同步版本，updater 产物链路基本闭环。`pnpm check:bindings`（`git diff --exit-code src/generated/bindings.ts`）进 CI 是亮点，能拦 Specta bindings 漂移。

短板：

- **无 `cargo fmt --check`**：仓库不存在 `rustfmt.toml`，也没有对应 CI 步骤。
- **无 `cargo audit`/`cargo-deny`**：`Cargo.lock` 含 899 个 crate，无 CVE/许可证闸门。
- **`verify:manifest`（转发头 allowlist 一致性校验）从未接入任何工作流**，即使是安全关键断言。
- **`check:i18n` 从未接入 CI**。
- **所有 job 均无 `timeout-minutes`**。
- **rust job 仅 `ubuntu-latest` 单平台**跑 `clippy`/`test`，Windows/macOS 专属代码（keyring 后端、SFTP、`platform/mod.rs`）在 PR 阶段不经过 clippy 或单测检验，只在 `tauri-build.yml` 中被构建（不执行测试）。

### 三、DB 迁移纪律（评级 C：机制设计合理，但当前提交状态自相矛盾）

机制层面健全：`build.rs`（+8 行）正确添加了 `cargo:rerun-if-changed=src/db/migrations`（proc macro 无法自行 watch 外部目录，这是官方推荐做法）；`src/db/connection.rs` 对 `VersionMissing`/`VersionMismatch`/`Dirty` 走"备份 + 重建"、对 `VersionTooOld` 明确报错而不静默降级，且都有专门测试覆盖（`rebuilds_for_dirty_migration`、`rebuilds_for_version_missing_after_downgrade`、`rebuild_after_dirty_produces_clean_schema`）。`rollback/*.down.sql` 是纯注释文档（`sqlx::migrate!` 不会自动执行该子目录），受限于 SQLite 历史版本不支持 `DROP COLUMN`，这一设计可以接受，但不是可执行的双向镜像。003 迁移新增的 `hls_tasks.selected_audio_track_uris`/`selected_subtitle_track_uris` 确有真实代码路径消费（`src/download/hls.rs`、`src/db/hls.rs`、`src/commands/tasks/create.rs`）。

**但仓库当前 HEAD 状态存在矛盾**：`src/db/migrations/` 已有 `001_init.sql`/`002_metalink_health.sql`/`003_hls_track_selection.sql` 三个 forward 迁移（003 与相关测试同属 b4ba790 一次提交），`tests/migration_integrity.rs` 却仍在 4 处硬编码断言只有 2 个迁移。已用 `git diff HEAD --stat` 确认这不是本地未提交改动。详见第二部分 C-0。

### 四、依赖健康（评级 B：版本策略克制，自动化守护缺失）

版本号四处一致：`package.json`/`src-tauri/tauri.conf.json`/`src-tauri/Cargo.toml`/`Cargo.lock` 均为 `0.2.0`。`reqwest 0.13.4` 使用 `default-features = false` + `["rustls", "socks", "stream", "system-proxy"]`，避开 OpenSSL 系统依赖，是合理选型。`Cargo.lock` 共 899 个 crate。`[profile.release]`（`Cargo.toml:78-87`）配置 `panic = "unwind"`、`codegen-units = 1`、`lto = true`、`opt-level = "s"`、`strip = true`，是桌面下载器体量/性能权衡下的合理发布配置。GPL-3.0-only 与依赖树中 MIT/Apache-2.0 为主的许可证兼容，未见明显冲突（未逐一核对全部 899 个，置信度中）。

缺口：无 `cargo-deny`/`cargo audit`，无 dependabot/renovate 自动更新提案；`scripts/sync-version.mjs` 只在 `release.yml` 触发时同步版本，PR 阶段没有「四文件版本一致性」检查。

### 五、文档一致性（评级 B+：内容详尽，声明略超前于测试证据）

版本号在 README/ROADMAP/RELEASE/`tauri.conf.json`/`Cargo.toml` 之间保持一致。功能边界明示到位：DASH live/dynamic 不支持（`docs/ROADMAP.md:21` + `dash_engine.rs` 的 `dash_live_unsupported` 测试对应）、HLS SAMPLE-AES 拒绝、DRM 未涉及、Safari wrapper 未支持均有清晰说明。README 构建指令（Node 20+/pnpm 10+，`pnpm install && pnpm tauri dev`）可跑通，命令表与 `package.json` scripts 对应准确。`docs/RELEASE.md` 的 unsigned 发布政策、updater 签名密钥流程、分支保护建议完整可操作。

主要落差：`docs/architecture-audit.md`/`docs/project-improvement-audit.md` 把 A-1 调度器槽位泄漏、F-2/F-3/F-4 Metalink 并行、A-2 SSRF 纵深防护标注为"已修复（2026-06-30）"，但测试证据链并不完整支撑——F-4 未覆盖 failover 编排、调度器无并发回归、引擎层 SSRF 无集成测试（详见第一部分「一、测试覆盖」与第二部分 D-1）。`docs/ROADMAP.md`「Verification Baseline」章节要求开发者本地跑 `check:i18n`/`verify:manifest`，但 CI 并不强制执行，形成「文档要求高于 CI 实际约束」的落差。

### 六、脚本与钩子（评级 B-：脚本本身质量高，拦截面窄）

四个脚本（`scripts/sync-version.mjs`/`scripts/check-i18n-completeness.ts`/`scripts/build-browser-extensions.mjs`/`scripts/verify-extension-manifest.mjs`）代码质量良好，尤其 `verify-extension-manifest.mjs`（222 行）的四项断言（禁用权限泄漏、release 占位 ID 检测、Rust↔JS 转发头 allowlist 集合相等）设计清晰、失败信息可操作。

但拦截面窄：husky `.husky/pre-commit` 只执行 `npx lint-staged`，而 `lint-staged`（`package.json:80-82`）的 glob 仅覆盖 `*.{ts,tsx,js,jsx,json,css,md}`——**Rust 文件提交时没有任何本地钩子**（不 fmt 不 clippy），压力全部下沉给 CI（而 CI 又没有 fmt 检查，见 C-1）。`biome.json` 关闭了 `useExhaustiveDependencies`（React 19 应用不校验 hooks 依赖数组，第 40 行）、`noExplicitAny`（第 44 行）、`noNonNullAssertion`（第 50 行），以及大量 `a11y` 规则（第 60-67 行），规则面偏松。

### 附：构建配置与仓库卫生（评级 A-）

`src-tauri/build.rs` 的迁移目录 watch 正确必要；`src-tauri/tauri.conf.json` 的 CSP（`script-src 'self'`，无 `unsafe-inline`）收紧合理，updater endpoint/pubkey 就位；`vite.config.ts` 有 vendor chunk 拆分（react/i18n/radix-ui/lucide/motion）。`.gitignore` 覆盖 `target/`、`dist/`、`*.sqlite`、`*.key`（保留 `*.key.pub`）、`.claude/` 等，未见异常大文件或应入库未入库的资源。`pnpm check:bindings` 是仓库卫生里少见的「生成物漂移」CI 硬闸门。

---

## 第二部分：缺口清单

### 🔴 C-0：迁移文件与迁移完整性测试自相矛盾，提示 CI 可能带红合并

**证据**：`src-tauri/src/db/migrations/` 现有 `001_init.sql`/`002_metalink_health.sql`/`003_hls_track_selection.sql` 三个 forward 迁移（`git log --oneline -- 003_hls_track_selection.sql` 只有 b4ba790 一次提交）。`sqlx::migrate!("./src/db/migrations")`（`src/db/connection.rs:100`）只读顶层目录，fresh connect 应记录 3 行 `_sqlx_migrations`。但 `src-tauri/tests/migration_integrity.rs` 仍硬编码：

- L36：`assert_eq!(count, 2, "expected exactly 2 migrations (baseline + metalink health)…")`
- L362-364：reconnect 场景同样断言 `count == 2`
- L508-509：降级重建场景断言 `vec![1, 2]`

已用 `git diff HEAD --stat -- src-tauri/tests/migration_integrity.rs src-tauri/src/db/migrations/` 确认这不是本地未提交改动，是**已提交状态本身的矛盾**（高置信度）。

**影响**：`.github/workflows/ci.yml:52` 的 `pnpm test:rust` 必然覆盖这 3 个测试函数，它们在 003 迁移存在的情况下应当失败。要么当前主分支 CI 实际是红的且未被当作合并阻断，要么 `test:rust` 在这次大提交落地时未被真正强制执行——两种情况都指向「迁移纪律 + CI 强制」双重失守。这是与本次 1.9 万行新增最不相称、也是本轮最严重的发现。

**建议**：立即将三处断言改为 3 / `vec![1,2,3]`，补一个 `migration_003_adds_hls_track_selection_columns` 测试（仿 002 的对应测试）；在 GitHub 分支保护中把 `CI / rust` 设为必需检查，避免类似情况再次带红合并。

### 🔴 G-1：F-4 Metalink 并行——测了分段原语与 DB 健康层，唯独没测把二者串起来的多镜像 failover 编排

**证据**：`tests/metalink_engine.rs` 通过 `pub mod testing`（`src/download/metalink.rs:1779`）暴露的只有 `download_metalink_range_from_mirror` 与 `assemble_metalink_part_files` 两个 AppHandle-free 函数，测试内容是单镜像 range 下载、进度单调递增、part-file 续传（校验 `Range: bytes=5-9`）、500 错误处理。真正体现"并行 + 故障转移"价值的编排器 `download_metalink_file_parallel`（`metalink.rs:389`）与 `MetalinkRangeWorker` 的镜像切换队列（`metalink.rs:740`）因需要 `tauri::AppHandle` 从未被集成测试驱动。全仓库搜索 `failover` 在 `tests/` 下只命中文档注释，无对应测试代码。

**影响**：F-4 实为「单镜像 range 下载」+「DB 排序/冷却」两个已验证的半成品，真正易错的"镜像 A 失败 → 冷却 → 切镜像 B → 继续装配"跨镜像编排逻辑毫无回归网，但 `architecture-audit.md` 已将其标记为"已修复"。

**建议**：抽出编排器中不依赖 `AppHandle` 的核心循环（或为 `AppHandle` 相关调用注入 trait 替身），补一个「双镜像清单，镜像 1 返回 500，断言最终从镜像 2 完整装配文件且 DB 中镜像 1 状态为 failed」的端到端测试。

### 🔴 G-2：调度器 dispatch 与 pause/cancel/delete 无真并发回归测试

**证据**：`tests/scheduler_logic.rs`（390 行/22 测试）只测纯函数与 DB 条件 UPDATE 的时序模拟（L171-353，注释明确写"no database or Tauri runtime… no spawned workers"，靠先后调用两个 DB 写操作模拟竞态，而非真正并发）。A-1 槽位泄漏的"修复"验证仅是一个 `matches!` 模式匹配的静态守卫测试（`transition_error_non_conflict_variants_do_not_match_conflict_pattern`），不构成对槽位泄漏本身的运行时检测。

**影响**：项目 memory 中记录的 pause/cancel/delete↔dispatch 锁序反转死锁、调度器槽位泄漏两项高危运行时缺陷，既无法被现有测试捕获，其"已修复"声明也缺乏并发场景下的证据支撑。这是最可能导致"静默停摆"的一类风险，却是测试覆盖最薄弱的环节之一。

**建议**：构造多任务并发压力测试（可考虑 `loom` 做穷举时序验证，或 tokio 多线程 + 随机延迟注入的统计性测试），至少覆盖"dispatch 正在获取 slot 的同时另一线程 cancel 同一任务"与"worker 完成写回的同时用户发起 pause"两类交叉时序。

### 🔴 C-1：CI 无 `cargo fmt --check`，无 `cargo audit`/`cargo-deny`

**证据**：全仓库 `.github/workflows/` 内检索 `fmt`/`rustfmt`/`audit`/`deny` 均无匹配；仓库根目录与 `src-tauri/` 均无 `rustfmt.toml`/`deny.toml`。

**影响**：Rust 格式漂移完全依赖人工 review；899 个依赖 crate 的已知漏洞（CVE）与许可证冲突（GPL-3.0 项目引入不兼容许可证依赖）没有任何自动化闸门，是「本地能挂但 CI 不查」最典型的一项。

**建议**：`ci.yml` 的 rust job 追加 `cargo fmt --all -- --check`（需先提交基线 `rustfmt.toml`）与 `cargo deny check licenses advisories`；评估接入 dependabot 或 renovate 做依赖更新提案。

### 🔴 C-2：安全关键的 `verify:manifest` 从未接入任何 CI 工作流

**证据**：`scripts/verify-extension-manifest.mjs` 的断言 4（L136-202）校验 `src-tauri/src/commands/browser.rs` 的 `FORWARDED_HEADER_ALLOWLIST` 与 `browser/extension-core/src/background.js` 的 `ALLOWED_HEADER_NAMES` 集合相等，这是防止 IPC 边界两侧转发头 allowlist 静默漂移（丢头或越权放行）的核心安全断言；断言 1 校验 dev 构建不泄漏 `downloads`/`cookies`/`webRequest` 权限。但 `ci.yml` 从不调用 `verify:manifest`，`release.yml` 的 `build-extensions` job（L94-131）只执行 `pnpm build:extensions`，同样不跑校验。

**影响**：Rust 与 JS 两侧的转发头 allowlist 一旦有人改一边忘改另一边，要么静默丢失合法转发头、要么放行未授权头跨越 IPC 边界，发布流程也无法拦截；dev 构建混入实验性权限同样无 CI 兜底。

**建议**：在 `ci.yml` 或至少 `release.yml` 的 `build-extensions` job 中于 `pnpm build:extensions` 之后追加 `pnpm verify:manifest`（即 `pnpm verify:extensions`）。

### 🟡 G-3：HLS/DASH 集成测试因 ffmpeg 缺失在 CI 中静默全跳过

**证据**：`tests/hls_engine.rs`（14 测试）、`tests/dash_engine.rs`（12 测试）每个用例开头均为 `if !ffmpeg_available() { eprintln!("skipping…"); return; }`；`ci.yml:48-49` 的 Linux 依赖安装列表（`libwebkit2gtk-4.1-dev`/`libayatana-appindicator3-dev`/`librsvg2-dev`/`patchelf`）不含 `ffmpeg`。

**影响**：这两个引擎共 26 个"集成测试"在 CI 环境中实际全部走 early-return 分支，既不解析清单也不触发下载，只是给出"有测试"的假象，与两引擎新增的代码量明显不相称。

**建议**：`ci.yml` rust job 的 apt 安装列表中加入 `ffmpeg`，让这些测试真正执行；并补充至少一个使用短 TS 分段驱动 `download()`（如通过测试专用入口绕过 AppHandle 依赖）的端到端 remux 用例。

### 🟡 G-4：前端零组件/集成测试

**证据**：8 个 vitest 文件全部是纯函数/hook 逻辑测试，无 `@testing-library/react` 组件渲染测试，无 Zustand store 集成测试，无 Tauri IPC mock 测试。

**影响**：`architecture-audit.md` 中反复提到的 UX 修复（搜索双防抖、空列表"筛选无匹配"分支、队列重排乐观更新、`queue-changed` 增量刷新）大多落在未被测试触达的组件层，无法防止回归。

**建议**：对高频交互路径（任务列表虚拟滚动分页、事件驱动进度合并、恢复动作分发）补组件级测试，优先覆盖近期修复过的 UX 缺陷点。

### 🟡 C-3：CI job 无超时设置，rust job 仅 ubuntu 单平台

**证据**：三个工作流文件检索 `timeout-minutes` 均无匹配；`ci.yml` 的 rust job（L30-53）`runs-on: ubuntu-latest`，是 clippy/test 唯一执行平台。

**影响**：任一网络类测试（SFTP/HTTP 相关较多）若 hang 住，会占满 runner 至平台默认上限（GitHub Actions 默认 6 小时）；Windows/macOS 专属代码路径（`keyring` 后端、SFTP、`platform/mod.rs`）在 PR 阶段完全不经过 clippy 或单测检验，只在 `tauri-build.yml` 里被构建（不执行测试）。

**建议**：每个 job 设置 `timeout-minutes: 20-30`；评估将 rust job 扩展为至少含 Windows 的矩阵（目标用户主要在 Windows，且 keyring 差异最大）。

### 🟡 C-4：版本号一致性缺少自动化闸门

**证据**：`scripts/sync-version.mjs` 同步 `package.json`/`tauri.conf.json`/`Cargo.toml` 三处，仅在 `release.yml`（L67-68、L112-113）触发时执行；PR/CI 阶段没有「四处版本号相等」的断言步骤；`docs/RELEASE.md:27` 自行提示"不要在版本号不一致的情况下发布"，却无自动检查兜底。

**影响**：手工修改版本号时若漏改一处，只能在发布环节甚至发布后才会暴露。

**建议**：加一个轻量 CI 步骤，断言 `package.json`/`tauri.conf.json`/`Cargo.toml`/`Cargo.lock` 四处版本号相等。

### 🟡 S-1：Rust 代码提交零本地钩子拦截；biome 关闭关键规则

**证据**：`.husky/pre-commit` 只执行 `npx lint-staged`；`package.json:80-82` 的 `lint-staged` glob 仅 `*.{ts,tsx,js,jsx,json,css,md}`，不含 `*.rs`。`biome.json:40/44/50-52` 关闭 `useExhaustiveDependencies`（React hooks 依赖数组不校验）、`noExplicitAny`、`noNonNullAssertion`，以及 `a11y` 分类下多项规则（`useKeyWithClickEvents`/`noSvgWithoutTitle`/`noLabelWithoutControl` 等，L60-67）。

**影响**：Rust 文件提交时没有任何本地 fmt/clippy 拦截，压力全部下沉到 CI（而 CI 本身又缺 fmt 检查，见 C-1），格式问题只能靠人工 review 发现；`useExhaustiveDependencies` 关闭在 React 19 应用中是 stale-closure、遗漏依赖更新的常见根因。

**建议**：pre-commit 增加对暂存 `.rs` 文件的 `cargo fmt` 检查；重新评估将 `useExhaustiveDependencies` 至少调整为 `warn`。

### 🟢 D-1：审计文档「已修复」声明超前于可验证的测试证据（置信度中）

**证据**：`docs/architecture-audit.md`/`docs/project-improvement-audit.md` 将 A-1 调度器槽位泄漏、F-2/F-3/F-4 Metalink 并行续传、A-2 SSRF 纵深防护标注为"已修复（2026-06-30）"。但本轮核实：(a) 调度器无并发回归测试（见 G-2）；(b) F-4 未覆盖 failover 编排（见 G-1）；(c) 引擎层连接期 SSRF 防护（`src/download/http/mod.rs:299` 的 `HickoryResolver::resolve` 过滤、`http/mod.rs:322` 的 `ssrf_safe_redirect_policy`）没有集成测试证明"公网域名解析到私网 IP 时下载真的会被阻断"——现有测试只覆盖纯 IP 分类函数与 handoff 层预检。

**影响**：文档给出的完成度高于测试可证明的完成度，可能误导发布决策；运行时死锁/keyring 数据丢失等 memory 中记录的问题本轮未重新复验运行时行为，是否仍然存在需要专门的运行时验证轮次确认。

**建议**：审计文档的"已修复"条目应链接到对应的回归测试用例；补充引擎层 SSRF 集成测试（构造一个解析到 127.0.0.1 的测试域名或本地 DNS mock，验证下载在连接期被拦截）。

### 🟢 S-2：`check:i18n` 只守 en↔zh-CN 两个语言，且未进 CI

**证据**：`scripts/check-i18n-completeness.ts` 硬编码只比较 `en` 与 `zh-CN`（L8-9）；`ja`/`es`/`ko`/`ru`/`zh-TW` 五个 beta 语言不在校验范围内。`package.json` 的 `check` 脚本（含 `check:i18n`）从未被 CI 调用，`ci.yml` frontend job 只分别执行 `typecheck`/`lint`/`test:frontend`/`build`。

**影响**：beta 语言（据 `architecture-audit.md` 记录约 50% 翻译覆盖）后续漂移无自动检测；i18n 守卫连已声明"稳定"的两个语言也未在 CI 强制。

**建议**：将 `check-i18n-completeness.ts` 扩展为对全部 locale 报告缺失/多余 key（至少不失败但报告比例），并将 `check:i18n` 接入 CI frontend job。

### 🟢 补充数据点（非独立缺口，供参考）

- **rollback 非可执行镜像**：001/002/003 三个 `rollback/*.down.sql` 均为 `SELECT 1;` + 注释文档，`sqlx::migrate!` 不会自动执行 `rollback/` 子目录。受 SQLite 历史版本 `DROP COLUMN` 限制，这一设计取舍可以接受，但不构成迁移与回滚的对称镜像，仅是人工恢复文档。
- **依赖重复扫描未完整执行**：899 个 crate 的锁文件规模较大，本轮未能跑通 `cargo tree -d` 精确定位重复版本号（工具环境限制），建议后续单独跑一次 `cargo tree -d` 配合 `cargo-deny` 复核（置信度低，仅作为待办提示，非已确认缺陷）。
- **仓库卫生整体良好**：`.gitignore` 覆盖 `target/`、`dist/`、`*.sqlite`、`*.key`（保留 `*.key.pub`）等；`pnpm check:bindings` 是为数不多在 CI 中强制的生成物漂移闸门；`.claude/worktrees/` 下遗留的旧版 `001_init.sql` 因整个 `.claude/` 目录已被 gitignore，不影响仓库状态，仅为本地工作残留。

---

## 优先级建议

1. **立即修复**：C-0（迁移测试与迁移文件自相矛盾，提示可能带红合并）。
2. **下一迭代**：G-1/G-2（与新增引擎/调度代码量最不相称的 failover 与并发回归缺口）。
3. **CI 加固**：C-1/C-2（fmt/audit/deny 与 `verify:manifest` 安全闸门缺失）。
