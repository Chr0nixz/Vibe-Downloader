# Vibe Downloader Product Context

<!-- impeccable:product-schema 1 -->

register: product

## Platform

web

The interface is React rendered in a Tauri 2 desktop webview, not native iOS or Android UI. Windows is the primary target; macOS and Linux stay portable.

## Product Purpose

Vibe Downloader is a modern desktop download manager built with Tauri, React, and Rust. It aims to outperform traditional IDM-style tools in everyday download experience, sustained speed, failure recovery, and interface quality.

The product should feel fast, reliable, and visually polished without becoming noisy. The download engine does the hard work in Rust; the React UI presents task state, control, and diagnosis clearly.

## Target Users

- Everyday desktop users who want browser downloads to be faster and easier to manage.
- Power users who often download large files such as system images, developer tools, game packages, archives, datasets, and model files.
- Users on shared home or office networks who need global speed limits and predictable queue control.
- Users disappointed by old download managers that feel visually dated, cluttered, or hard to trust after crashes.

Today the builds reach a small group of early testers through GitHub candidate packages and source. Vibe Downloader is not yet publicly released.

## Positioning

Vibe Downloader competes on trust the user can check, not on speed claims or protocol count. A resume never puts the file at risk. Crashes and network failures recover. Every failure names its cause, a concrete recovery, and what that recovery costs, such as the bytes a restart throws away. The Integrity Passport reports only what the evidence supports: a missing remote checksum is shown as "not provided", never as "verified".

Speed and protocol breadth are real, but they come second. Dynamic connections are explained through measurable state, and lower-maturity protocols state their limits instead of hiding them.

## Operating Context

- The app stays open beside a browser or code editor for hours while large files download. It also runs in the tray with close-to-tray, a floating status window, notifications, completion actions, and scheduled download windows.
- Downloads arrive through browser extension handoff (Native Messaging and a local WebSocket bridge, HTTP/HTTPS only), clipboard link detection, the New Download dialog, batch import, and dragging `.torrent`, `.metalink`, `.mpd`, or text files onto the window.
- On shared networks, users rely on global and per-task speed limits, per-host connection slots, priorities, and the queue.
- The window may be maximized, snapped to half a screen, or kept narrow. Phone-width layouts exist for small windows, but the product is a desktop app.
- Release stage: `0.5.0`, in active development, used by a small group of early testers. There is no OS code signing or notarization and no browser-store identity yet, and candidate packages are not public releases.

## Capabilities and Constraints

- HTTP/HTTPS is the most mature path. FTP/FTPS, SFTP, BitTorrent, HLS, static DASH, WebDAV, and Metalink work to different depths, and each has documented boundaries in `docs/protocol-reliability-matrix.md`.
- Stable boundaries:
  - Browser handoff accepts HTTP/HTTPS only. It rejects embedded URL credentials and browser-controlled save paths.
  - Header forwarding stays explicit, allowlisted, local-only, and encrypted when persisted.
  - Intranet targets need per-task authorization.
  - HLS AES-128 support does not imply DRM.
  - An unsupported proxy and protocol combination fails explicitly instead of silently bypassing the proxy.
- Automatic browser capture and Cookie/header forwarding are experimental, development-profile features. They are excluded from candidate and release packages.
- Deferred, not promised:
  - cloud-drive parsing
  - complete media sniffing
  - plugin protocols
  - CLI, JSON-RPC, or REST automation
  - PAC/WPAD proxy support
  - Safari distribution
- Languages: English and Simplified Chinese are the stable, maintained locales. Traditional Chinese, Japanese, Korean, Russian, and Spanish ship as beta and are only maintained when that is explicitly requested.
- Product terms used in the UI and docs:
  - Needs you
  - Recovery Center
  - Queue Center
  - Attention Center
  - Storage & Cleanup Center
  - Backup & Migration Center
  - Integrity Passport
  - chunk map
- License: GPL-3.0-only. Commercial licensing is available separately from the copyright holder.

## Core Promise

Users should understand download health at a glance. The app should automatically use available bandwidth well, recover from crashes and network failures, and stay visually calm during normal operation.

## Experience Principles

- Silent power: the app is quiet by default and becomes visually expressive only when speed, errors, or user decisions deserve attention.
- Reliable first: resume correctness, file integrity, clear failure reasons, and crash recovery matter more than feature breadth.
- Fast without theatrics: dynamic connections should be explained through measurable state, not vague claims.
- Consumer-friendly by default: the default view should be approachable, attractive, and easy to scan.
- Expert depth on demand: chunk maps, connection health, Range details, retries, and disk I/O belong in expandable detail views.
- Keyboard efficient: common actions should work through shortcuts and a command palette without making the mouse path harder.
- Native desktop feel: the shell should feel at home on Windows while remaining portable enough for future platforms.

## Brand Personality

- Calm
- Precise
- Modern
- Trustworthy
- Slightly technical
- Visually refined

The product may feel energetic during high-speed download moments, but it should not feel like a game launcher, crypto dashboard, or generic neon admin template.

## Brand Commitments

- The name is Vibe Downloader. The logo is the ring mark in `public/logo-48.png`, `public/logo-64.png`, and `public/favicon.png`.
- The README discloses that the project is developed mainly with AI assistance, with human review of direction and acceptance. Capability claims follow the current implementation and its evidence.

## Tone Of Voice

Use short, clear, action-oriented copy.

Good examples:

- Downloading
- Server limit detected
- Resume unavailable
- Free 2.4 GB to continue
- Remote file changed. Restart download to avoid corruption.

Avoid:

- Vague errors such as "Something went wrong"
- Marketing claims such as "AI accelerated"
- Raw technical codes without explanation
- Overly playful copy during failures

## Evidence on Hand

Citable, with its limits stated:

- **Local performance baseline:** `docs/performance-baseline-results.md` has headless 1k and 10k task measurements on `0.5.0`, taken on one Windows development machine. These are local measurements, not cross-machine guarantees.
- **Automated protocol evidence:** `docs/protocol-reliability-matrix.md` covers local fake servers and integration tests.
- **Candidate-package evidence:** `docs/b7-acceptance-matrix-2026-10-02.md` records candidate-package smoke tests and real-environment acceptance status. Neither document is field-verified reliability.

Absent, and must not be fabricated or implied:

- testimonials, reviews, user counts, case studies, or press
- cross-machine speed benchmarks, or comparisons such as "faster than IDM"
- field-verified protocol reliability
- signed or notarized distribution, store listings, or completed upgrade validation

## Product Principles

1. Correctness before capability. Anything that risks a corrupt file or a silently wrong state waits, however much it is wanted. Reliability outranks new protocols and service entry points.
2. Claims match evidence. The product says only what the engine can prove and names its boundaries. Early-tester builds are never presented as a finished IDM replacement.
3. Every failure leads somewhere. A stopped task names its cause, the recovery, and that recovery's cost before the user commits.
4. The browser is the front door. Handoff from the browser is a first-class daily path, and its permissions never quietly widen into download permission.
5. Problems stay visible. Calm is the default state, but anything waiting on the user is always findable from chrome that stays on screen.

## Strategic Priorities

1. HTTP downloading must be stable before new protocols are added.
2. Browser extension handoff is a first-class daily workflow.
3. Task persistence and resume correctness are trust foundations.
4. UI polish should improve confidence and clarity, not hide problems.
5. Advanced visualizations should help users diagnose speed and reliability.

## Anti-References

- IDM-style crowded toolbars and dated icon-heavy chrome.
- Full neon cyberpunk dashboards where every state competes for attention.
- Excessive glassmorphism that weakens readability.
- Card grids that reduce scan efficiency for many tasks.
- Generic SaaS dashboard layouts with decorative metric cards.
- Dark blue or purple gradient-heavy interfaces that look template-generated.

## Success Criteria

- A first-time user can add a URL and understand progress within 5 seconds.
- A power user can pause, resume, search, limit speed, and batch operate mostly from the keyboard.
- A failed task explains what happened and offers a concrete recovery path.
- A large interrupted download can resume without file corruption.
- The interface looks modern to mainstream users while still rewarding expert inspection.

## Accessibility & Inclusion

- Target WCAG AA contrast in both light and dark themes.
- Every primary workflow is reachable from the keyboard. The command palette speeds things up but is never the only path.
- State is never carried by color alone, and state changes have accessible text equivalents.
- Reduced-motion preferences are honored.
