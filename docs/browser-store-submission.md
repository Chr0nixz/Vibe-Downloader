# Browser Store Submission

Last updated: 2026-07-19

## Release Scope

The first public extension release targets Chrome Web Store, Microsoft Edge Add-ons, and Firefox AMO. Opera remains a development-only package. Safari is outside the current release scope.

Public packages use the minimal-permission manual hand-off profile. They do not include automatic download takeover, Cookie/header forwarding, media sniffing, or broad host permissions.

## Store Metadata

- Name: `Vibe Downloader`
- Category: Utilities / Productivity
- Single purpose: Send user-selected HTTP/HTTPS download links to the locally installed Vibe Downloader desktop application.
- Support URL: `https://github.com/Chr0nixz/Vibe-Downloader/issues`
- Privacy URL: `https://github.com/Chr0nixz/Vibe-Downloader/blob/main/docs/browser-extension-privacy.md`

Short description:

> Send download links from your browser to the Vibe Downloader desktop app.

Review description:

> Vibe Downloader adds a toolbar action and context-menu command for sending the current page, a link, or selected HTTP/HTTPS text to the Vibe Downloader desktop application. Communication stays on the user's computer through the browser Native Messaging API. The extension does not run analytics or send browsing data to a remote Vibe service.

## Permission Justification

- `nativeMessaging`: required to communicate with the separately installed desktop application.
- `contextMenus`: provides the explicit “Download with Vibe Downloader” command for links and selected text.
- `activeTab` and `tabs`: read the active tab URL only when the user invokes the extension action.
- `storage`: stores extension preferences, site rules, and a bounded recent hand-off list locally.

The store build must not contain `downloads`, `cookies`, `webRequest`, `webRequestBlocking`, or `host_permissions` for all HTTP/HTTPS sites.

## Build Outputs

Run the release build with the three formal IDs configured:

```bash
VIBE_BROWSER_PROFILE=release pnpm verify:extensions
```

Submission archives and their checksums are written to `browser/dist/packages/`. Chrome and Edge receive their respective ZIP files. The Firefox ZIP is an unsigned AMO submission artifact and must not be renamed or described as a signed XPI before AMO signing succeeds.

## Reviewer Test Steps

1. Install the matching Vibe Downloader desktop candidate.
2. Open Settings → Browser integration and install the Native Messaging manifest for the browser.
3. Install the store submission extension.
4. Open a public HTTP/HTTPS page and use the toolbar or context-menu hand-off action.
5. Confirm Vibe Downloader opens and creates one task.
6. Repeat the same request ID and confirm no duplicate task is created.
7. Uninstall browser integration from Settings and confirm the extension reports that the native host is unavailable.

## External Release Gates

- Reserve the Chrome Web Store and Edge Add-ons listings and record their stable 32-character IDs.
- Reserve the Firefox AMO ID and configure signing credentials.
- Store all three IDs as GitHub Actions secrets.
- Submit the three generated packages and complete review.
- Download the store-installed Chrome/Edge packages and AMO-signed Firefox XPI, then repeat Native Messaging acceptance testing.

The extension packaging path is candidate-ready, but the desktop repository still has active P0/P1 issues in [project-improvement-audit.md](project-improvement-audit.md). No public extension should be described as generally release-ready until both the external store gates and the desktop release gates are closed.
