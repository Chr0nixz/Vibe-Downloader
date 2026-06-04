# Vibe Downloader Product Context

register: product

## Product Purpose

Vibe Downloader is a modern desktop download manager built with Tauri, React, and Rust. It aims to outperform traditional IDM-style tools in everyday download experience, sustained speed, failure recovery, and interface quality.

The product should feel fast, reliable, and visually polished without becoming noisy. The download engine does the hard work in Rust; the React UI presents task state, control, and diagnosis clearly.

## Target Users

- Everyday desktop users who want browser downloads to be faster and easier to manage.
- Power users who often download large files such as system images, developer tools, game packages, archives, datasets, and model files.
- Users on shared home or office networks who need global speed limits and predictable queue control.
- Users disappointed by old download managers that feel visually dated, cluttered, or hard to trust after crashes.

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
