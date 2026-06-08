# Release Guide

本文档说明 Vibe Downloader 如何通过 GitHub Actions 发布安装包，并让应用内自动更新可用。

## Workflows

| Workflow | 文件 | 触发 | 用途 |
| --- | --- | --- | --- |
| CI | `.github/workflows/ci.yml` | push 到 `main`/`master`，或 pull request | 前端 typecheck/lint/build、Rust check/clippy/test、Specta drift |
| Tauri Build | `.github/workflows/tauri-build.yml` | push 到 `main`/`master`，或 pull request | Windows/macOS/Linux `pnpm tauri build --config src-tauri/tauri.ci.conf.json` |
| Release | `.github/workflows/release.yml` | `v*` tag 或手动触发 | 构建安装包，上传 GitHub Release assets，生成 updater `latest.json` |

`tauri.ci.conf.json` 会关闭 updater artifacts，用于 CI 构建验证；正式 Release workflow 会生成 updater artifacts。

## 一次性仓库配置

### 1. Updater endpoint

生产 updater endpoint 配置在 [`src-tauri/tauri.conf.json`](../src-tauri/tauri.conf.json)：

```json
"endpoints": [
  "https://github.com/Chr0nixz/Vibe-Downloader/releases/latest/download/latest.json"
]
```

不要在缺少 `latest.json`、安装包、`.sig` 文件或版本号不一致的情况下发布生产版本。

### 2. Updater signing key

Tauri updater 签名独立于操作系统代码签名。生成密钥：

```bash
pnpm tauri signer generate -w vibe-downloader.key
```

要求：

- 公钥写入 `plugins.updater.pubkey`。
- 私钥作为 GitHub Actions secret 保存。
- 不要提交 `*.key` 私钥文件。

导出私钥给 CI：

```powershell
[Convert]::ToBase64String([IO.File]::ReadAllBytes("vibe-downloader.key"))
```

macOS/Linux：

```bash
base64 -i vibe-downloader.key
```

### 3. GitHub Secrets

| Secret | 必需 | 用途 |
| --- | --- | --- |
| `TAURI_SIGNING_PRIVATE_KEY` | 是 | Base64 编码的 updater 私钥 |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | 视密钥而定 | updater 私钥密码 |
| `GITHUB_TOKEN` | GitHub 内置 | 创建 Release 并上传 assets |

后续操作系统代码签名预留：

| Secret | 用途 |
| --- | --- |
| `APPLE_CERTIFICATE` | macOS `.p12` 证书 Base64 |
| `APPLE_CERTIFICATE_PASSWORD` | macOS 证书密码 |
| `APPLE_SIGNING_IDENTITY` | 例如 `Developer ID Application: ...` |
| `APPLE_ID` / `APPLE_PASSWORD` / `APPLE_TEAM_ID` | macOS notarization |
| `WINDOWS_CERTIFICATE` | Windows `.pfx` 证书 Base64 |
| `WINDOWS_CERTIFICATE_PASSWORD` | Windows 证书密码 |

当证书准备好后，再在 [`.github/workflows/release.yml`](../.github/workflows/release.yml) 中启用对应环境变量。

### 4. Branch protection

建议在 GitHub `Settings -> Branches` 对主分支要求：

- `CI` frontend job
- `CI` rust job
- `Tauri Build` matrix jobs

## 发布流程

1. 确认主分支 CI 和 Tauri Build 全绿。
2. 创建并推送 semver tag：

   ```bash
   git tag v0.2.0
   git push origin v0.2.0
   ```

3. Release workflow 会执行：
   - `scripts/sync-version.mjs`，同步 `package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml`
   - 构建 macOS arm64、macOS x64、Linux x64、Windows x64
   - 创建 GitHub Release
   - 上传安装包、签名文件和 `latest.json`

4. 如果是 draft release，检查 assets 后再发布。
5. 确认 release notes 标注当前限制，例如 unsigned installer、浏览器扩展开发包状态等。

## 手动发布

GitHub Actions -> `Release` -> `Run workflow`，输入 tag，例如 `v0.2.0`，并选择是否 draft。

## 自动更新行为

- 打包后的非 dev Tauri 应用启动约 3 秒后检查更新。
- 检测到新版本时，状态栏显示可安装版本和安装按钮。
- 点击安装后下载、安装并 relaunch。
- `tauri dev` 和浏览器预览不会检查更新。
- 客户端使用 `tauri.conf.json` 中的公钥校验更新。

## 当前签名状态

当前 Release workflow 已配置 updater 签名，但操作系统代码签名仍是预留状态。

没有 Apple/Windows 代码签名时：

- macOS 用户首次启动可能需要右键打开。
- Windows SmartScreen 可能提示未知发布者。

正式对外发布前应配置系统代码签名，或在发布说明中明确 unsigned 风险。

## 发布前本地验证

```bash
pnpm typecheck
pnpm build
pnpm check:bindings
pnpm test:rust
cargo check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
pnpm build:extensions
pnpm tauri build --config src-tauri/tauri.ci.conf.json
```
