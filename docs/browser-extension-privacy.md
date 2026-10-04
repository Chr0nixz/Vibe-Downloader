# Vibe Downloader Browser Extension Privacy Policy

Last updated: 2026-10-02

This policy applies to the Vibe Downloader browser extensions distributed for Chrome, Microsoft Edge, and Firefox.

## Summary

The extension sends user-requested HTTP/HTTPS download links to the Vibe Downloader desktop application on the same computer. It does not operate an analytics service, sell data, or send browsing data to Vibe Downloader servers.

## Data Handled Locally

The extension may store the following data in browser extension storage:

- Extension preferences.
- Site rules configured by the user.
- A bounded recent hand-off list containing URL, timestamp, and local status.
- Local connection state for the Native Messaging and loopback WebSocket bridge.

The desktop application stores created download tasks and diagnostics in its local SQLite database. Users control retention by deleting tasks, clearing extension data, uninstalling the extension, or removing the desktop application's data directory.

## Data Transfer

Extension data is sent only to software on the same computer:

- Native Messaging uses the local `com.vibe_downloader.native_host` process over browser-managed stdin/stdout.
- Realtime status uses `127.0.0.1:48365` with a short-lived local token issued by the desktop application.

The extension does not transmit data to a Vibe Downloader cloud service. The destination download server naturally receives the HTTP/HTTPS request made by the desktop download engine.

## Store-Build Permissions

The public store build uses only `nativeMessaging`, `contextMenus`, `activeTab`, `tabs`, and `storage`. It does not request `downloads`, `cookies`, `webRequest`, or broad HTTP/HTTPS host permissions.

Automatic browser-download takeover and Cookie/header forwarding are experimental development-build capabilities. They are not present in the public candidate or store packages covered by this policy.

## Security Boundaries

- Browser hand-off accepts only HTTP/HTTPS URLs.
- URLs with embedded credentials are rejected.
- Release download engines allow private targets only after an explicit, task-bound authorization in the desktop application. The intranet hand-off setting only allows a browser candidate to reach that authorization flow; it never grants access by itself. The grant is limited to the confirmed authority and resolved addresses, while metadata, link-local, unauthorized redirects, and DNS rebinding remain blocked. Real NAS, Tailscale, enterprise DNS, and candidate-package acceptance are still external validation work tracked in the [optimization plan](optimization-plan-2026-09-29.md).
- Authorization headers are never accepted from the browser hand-off boundary.
- Browsers may only invoke the Native Messaging host when their installed extension ID is allowlisted by the desktop application.

## User Control

Users can disable or uninstall browser integration from Vibe Downloader Settings, uninstall the extension from the browser, and delete download tasks from the desktop application. Uninstalling the extension removes its browser-managed local storage according to the browser's normal behavior.

## Contact

Questions and privacy reports can be filed at [github.com/Chr0nixz/Vibe-Downloader/issues](https://github.com/Chr0nixz/Vibe-Downloader/issues).

Material changes to this policy will be recorded in the repository and release notes before an updated extension is submitted.
