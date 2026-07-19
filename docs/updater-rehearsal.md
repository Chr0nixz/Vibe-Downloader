# Updater Rehearsal

Last updated: 2026-07-19

This runbook verifies a signed candidate-to-candidate upgrade without changing the production `latest` updater channel.

## Prerequisites

- `TAURI_SIGNING_PRIVATE_KEY` and its password are available to the candidate workflow.
- Two semver prerelease tags are selected, for example `v0.3.0-rc.0` and `v0.3.0-rc.1`.
- Both candidates use the same Tauri updater public key.
- Test machines or VMs are available for Windows x64, macOS, and Linux x64.

## Produce The Target Candidate

1. Build and publish `v0.3.0-rc.1` as a GitHub prerelease.
2. Confirm the prerelease contains `latest.json`, platform updater archives, and `.sig` files.
3. Confirm the direct endpoint is reachable:

   `https://github.com/Chr0nixz/Vibe-Downloader/releases/download/v0.3.0-rc.1/latest.json`

## Build The Source Candidate

Generate an ignored Tauri overlay that points directly at the target prerelease:

```bash
pnpm prepare:updater-rehearsal -- --tag v0.3.0-rc.1
pnpm tauri build --config src-tauri/tauri.updater-rehearsal.generated.json
```

Build the source package at version `0.3.0-rc.0`. Never commit the generated overlay and never replace the production endpoint with a prerelease URL.

## Acceptance Procedure

For each platform:

1. Install `rc.0` and create a small completed task plus one paused task.
2. Install browser integration and record the Native Messaging manifest path and host path.
3. Check for updates and confirm `rc.1` is offered.
4. Download and install the update, then allow the application to relaunch.
5. Confirm the running version is `rc.1`.
6. Confirm the existing SQLite tasks and settings remain intact.
7. Confirm the Native Messaging manifest still points to an existing sidecar.
8. Send a browser hand-off and confirm exactly one new task is created.
9. Capture updater logs and the before/after version screens.

## Evidence Record

Record OS/version, architecture, source installer, target asset, signature verification result, relaunch result, database result, manifest path, browser used, and log archive for every platform. Any missing platform evidence keeps the updater release gate open.
