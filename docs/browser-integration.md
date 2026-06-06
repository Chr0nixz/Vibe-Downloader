# Browser Integration

Stage 5 uses Native Messaging as the primary browser handoff channel. Localhost
and WebSocket APIs are intentionally left for later real-time extension panels or
automation integrations.

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

## Native Host

The native host binary is `vibe-native-host`. It reads exactly one Native
Messaging message from stdin, validates the payload, writes a handoff JSON file,
and starts `vibe-downloader --browser-handoff-file <path>` when it can resolve
the app executable.

The host never logs to stdout because stdout is reserved for the Native Messaging
protocol. Use stderr or files for diagnostics.

Development environment variables:

- `VIBE_DOWNLOADER_APP_EXE`: absolute path to the app executable.
- `VIBE_DOWNLOADER_HANDOFF_DIR`: directory where handoff JSON files are written.

## Install Host Manifests

Use Settings > Browser integration to install or uninstall manifests per
browser. The app writes manifest files and, on Windows, HKCU registry keys.

The development Chromium extension id is currently fixed as
`abcdefghijklmnopabcdefghijklmnop` in generated native host manifests. Replace
it with the store extension id for release builds, or load a Chromium extension
package that uses the matching deterministic id.

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
