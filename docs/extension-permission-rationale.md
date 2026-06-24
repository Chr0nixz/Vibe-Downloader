## Browser Extension Permission Rationale

This document explains why each permission is declared in the Vibe Downloader browser extension manifest.

### Base Permissions (always declared)

**nativeMessaging** — Required for the extension to communicate with the Vibe Downloader desktop app through the Native Messaging protocol. The desktop app registers a messaging host under `com.vibe_downloader.native_host` and the extension sends download requests through this channel.

**contextMenus** — Used to register the "Download with Vibe Downloader" right-click menu item on links, images, and selected text. The menu item is created in `api.contextMenus.create` during `onInstalled`.

**activeTab** — Allows the extension to access the current tab when the user explicitly interacts with the extension (clicking the toolbar icon or the context menu). Used to extract page URL context for download requests without persistent tab access.

**tabs** — Used by `popup.js` to read the active tab's URL for the "Send current tab" action via `chrome.tabs.query({ active: true, currentWindow: true })`. While `activeTab` covers this in Chrome on toolbar click, the `tabs` permission ensures `tab.url` is reliably populated across all target browsers (including Firefox MV3).

**storage** — Used via `api.storage.local` to persist user preferences (browser capture settings) and a short list of recent handoff records for the popup UI.

### Capture Permissions (only when `VIBE_BROWSER_EXPERIMENTAL_CAPTURE=true`)

These permissions are added at build time by `scripts/build-browser-extensions.mjs` when the experimental capture feature is enabled, and stripped from the manifest otherwise.

**downloads** — Enables `api.downloads.onCreated` to observe browser-initiated downloads and `api.downloads.pause`/`cancel`/`resume`/`erase` to transfer control to the desktop app. Required for the browser download takeover flow.

**cookies** — Enables `api.cookies.getAll` to optionally forward cookies with download requests when the user has explicitly enabled cookie forwarding in capture settings. This is needed for authenticated downloads (e.g., file hosting sites).

**webRequest** / **webRequestBlocking** (Firefox) — Used via `api.webRequest.onBeforeSendHeaders` and `api.webRequest.onHeadersReceived` to observe response headers for media stream detection (HLS/DASH/m3u8) and to capture request headers for forwarding. Only active when experimental capture is enabled.

**host_permissions: `http://*/*` and `https://*/*`** — Required for `webRequest` listeners to observe network traffic across all HTTP/HTTPS origins. Without host permissions, `webRequest` cannot fire for specific URLs.

### Permissions Not Declared

**history** — No browsing history access is needed.

**bookmarks** — No bookmark access is needed.
