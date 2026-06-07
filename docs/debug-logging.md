# Debug Logging

Vibe Downloader writes diagnostic logs across the desktop app, native messaging host, frontend, and browser extension.

## Log file locations

### Main application (`vibe.log`)

| Platform | Path |
|----------|------|
| Windows | `%LOCALAPPDATA%\com.vibe.downloader\logs\vibe.log` |
| macOS | `~/Library/Logs/com.vibe.downloader/vibe.log` |
| Linux | `~/.local/share/com.vibe.downloader/logs/vibe.log` |

Release builds always write to this file even when no console is available.

### Native messaging host (`native-host*`)

Written next to the main log directory with daily rotation:

| Platform | Path |
|----------|------|
| Windows | `%LOCALAPPDATA%\com.vibe.downloader\logs\native-host*` |
| macOS | `~/Library/Logs/com.vibe.downloader/native-host*` |
| Linux | `~/.local/share/com.vibe.downloader/logs/native-host*` |

The native host never writes to stdout because stdout is reserved for the Native Messaging protocol.

## Log levels

| Level | Usage |
|-------|-------|
| `error` | Permanent failures (download failed, DB write failed, handoff rejected) |
| `warn` | Recoverable issues (event emit failed, refresh retry, network retry) |
| `info` | Milestones (task created/completed, handoff accepted, scheduler dispatch) |
| `debug` | Development detail (HTTP probe, command calls, scheduler slots) |
| `trace` | High-frequency noise (segment progress ticks; off by default) |

## Rust log filtering (`RUST_LOG`)

Set `RUST_LOG` before starting the app to change verbosity:

```bash
# Windows PowerShell
$env:RUST_LOG="vibe_downloader=debug,tauri=warn,sqlx=debug"
pnpm tauri dev

# macOS / Linux
RUST_LOG=vibe_downloader=debug,tauri=warn,sqlx=debug pnpm tauri dev
```

Or use the helper script:

```bash
pnpm dev:tauri
```

Useful presets:

- `vibe_downloader=debug` — full backend detail
- `vibe_downloader=info` — default release verbosity
- `vibe_downloader=debug,sqlx=debug` — include SQL queries

## Correlation IDs

- **Downloads**: search logs for `task_id=...`
- **Browser handoff**: search for `request_id=...` across extension console, `native-host*`, and `vibe.log`

URLs are sanitized before logging (query strings and credentials are stripped).

## Frontend logs

The React app uses `[vibe:namespace]` prefixes. In the Tauri WebView:

- `info`, `warn`, and `error` are captured into `vibe.log` via `tauri-plugin-log`
- `debug` is emitted only in dev builds (`import.meta.env.DEV`)

Unhandled errors and promise rejections are logged under `[vibe:global]`.

## Browser extension logs

Extension logs use `[vibe-ext:namespace]` and appear in the browser Service Worker console:

1. Open `chrome://extensions` (or the equivalent page in Edge/Firefox/Opera)
2. Find **Vibe Downloader**
3. Click **Inspect views: service worker** (or **背景页**)

Extension logs are not merged into `vibe.log` because the extension runs in a separate browser process.

## Reporting issues

When filing a bug report, attach:

1. `vibe.log` from the main application log directory
2. `native-host*` if the issue involves browser integration
3. A screenshot or copy of the extension Service Worker console if the handoff starts from the browser

Redact personal file paths or URLs if needed before sharing.
