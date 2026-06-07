# Browser Integration

Stage 5 uses Native Messaging as the primary browser handoff channel. Localhost
and WebSocket APIs are intentionally left for later real-time extension panels or
automation integrations.

## Current Implementation Status

Implemented in the current working tree:

- Shared WebExtension source in `browser/extension-core`.
- Development package generation through `pnpm build:extensions`.
- Standalone Rust native host binary: `vibe-native-host`.
- Tauri commands for integration status, manifest install/uninstall, and browser
  handoff task creation.
- Single-instance handoff forwarding for already-running app instances.
- SQLite `browser_messages` table for request de-duplication and diagnostics.
- Settings > Browser integration UI for manifest status and install/uninstall.

Still intentionally incomplete:

- Production Safari Web Extension target.
- Store extension ids, signing, and review flows.
- Localhost / WebSocket realtime browser panel.

## Supported Browsers

Vibe targets desktop Chrome, Edge, Firefox, Safari, Brave, Opera, Vivaldi, and
Chromium.

- Chrome, Edge, Brave, Opera, Vivaldi, and Chromium use the Chromium Native
  Messaging manifest shape.
- Firefox uses the Firefox Native Messaging manifest shape with
  `vibe-downloader@local` as the development extension id.
- Safari is macOS-only and requires a separate Safari Web Extension wrapper for
  production distribution.

## Build Extension Packages

```bash
pnpm build:extensions
```

Outputs:

- `browser/dist/chromium`
- `browser/dist/firefox`
- `browser/dist/opera`

Brave, Vivaldi, and Chromium can load the Chromium package during development.

Recommended local verification:

```bash
pnpm build:extensions
pnpm specta
pnpm typecheck
pnpm build
cargo check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
```

## Native Host

The native host binary is `vibe-native-host`. It reads exactly one Native
Messaging message from stdin, validates the payload, writes a handoff JSON file,
and starts `vibe-downloader --browser-handoff-file <path>` when it can resolve
the app executable.

If the app is already running, the Tauri single-instance plugin forwards the
second launch arguments to the existing instance. The existing instance then
loads the handoff file, creates the task, focuses the main window, and removes
the handoff file after successful processing.

The host never logs to stdout because stdout is reserved for the Native Messaging
protocol. Use stderr or files for diagnostics.

Development environment variables:

- `VIBE_DOWNLOADER_APP_EXE`: absolute path to the app executable.
- `VIBE_DOWNLOADER_HANDOFF_DIR`: directory where handoff JSON files are written.

The host logs to `native-host*`; see [debug-logging.md](debug-logging.md).

## Install Host Manifests

Use Settings > Browser integration to install or uninstall manifests per
browser. The app writes manifest files and, on Windows, HKCU registry keys.

The development Chromium extension id is currently fixed as
`abcdefghijklmnopabcdefghijklmnop` in generated native host manifests. Replace
it with the store extension id for release builds, or load a Chromium extension
package that uses the matching deterministic id.

Manifest behavior:

- Windows: app-owned manifest files plus HKCU NativeMessagingHosts registry keys.
- macOS/Linux: per-browser NativeMessagingHosts directories.
- Safari: macOS-only placeholder until the Safari Web Extension wrapper is added.

## Handoff Payload

```json
{
  "version": 1,
  "requestId": "uuid",
  "browser": "chrome",
  "action": "download_url",
  "url": "https://example.com/file.zip",
  "pageUrl": "https://example.com/page",
  "referrer": "https://example.com/page",
  "userAgent": "optional",
  "suggestedFileName": "optional.zip"
}
```

Security rules:

- Only `http` and `https` URLs are accepted.
- URLs with embedded credentials are rejected.
- The browser extension cannot set a local save directory.
- Cookies and sensitive headers are not forwarded in Stage 5.

## Manual Browser Checks

1. Run `pnpm build:extensions`.
2. Start the app once so Settings can install the native host manifest.
3. Open Settings > Browser integration and install the target browser manifest.
4. Load the development extension:
   - Chrome/Edge/Brave/Vivaldi/Chromium: load `browser/dist/chromium`.
   - Firefox: load `browser/dist/firefox` as a temporary add-on.
   - Opera: load `browser/dist/opera`.
5. Right-click an HTTP/HTTPS link and choose **Download with Vibe Downloader**.
6. Confirm a new queued/downloading task appears in Vibe and a row is written to
   `browser_messages`.

If the extension reports a native host error, check:

- The browser extension id matches the generated native host manifest.
- `vibe-native-host` is next to the app executable or the manifest path points to
  a valid host binary.
- `VIBE_DOWNLOADER_APP_EXE` is set when running unpackaged development binaries.
- `native-host*` for request id and validation errors.
