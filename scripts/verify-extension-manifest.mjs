// Verifies the built browser extension manifests and source-level header
// allowlist against the security boundary enforced in
// `src-tauri/src/commands/browser.rs`. Run after `pnpm build:extensions`:
//
//   pnpm build:extensions && pnpm verify:manifest
//
// Assertions (all must pass for a release-ready build):
//   1. The dev profile manifest MUST NOT contain `downloads`, `cookies`,
//      `webRequest`, or `host_permissions` — those are gated behind the
//      experimental-capture build flag (`VIBE_BROWSER_EXPERIMENTAL_CAPTURE=1`)
//      and must never ship in a regular dev/release build.
//   2. The release profile Firefox `gecko.id` MUST NOT be the placeholder
//      `vibe-downloader@example.invalid`.
//   3. The release profile Chromium/Edge/Opera extension IDs MUST NOT start
//      with `replace-with-`.
//   4. The `FORWARDED_HEADER_ALLOWLIST` constant in
//      `src-tauri/src/commands/browser.rs` MUST be set-equal to the
//      `ALLOWED_HEADER_NAMES` constant in
//      `browser/extension-core/src/background.js`. Drift between the two
//      would silently drop or leak headers across the IPC boundary.
//
// Exit code is non-zero on any failure so the script can be wired into CI.

import { readdir, readFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const distDir = path.join(root, "browser", "dist");
const browserRustPath = path.join(root, "src-tauri", "src", "commands", "browser.rs");
const backgroundJsPath = path.join(root, "browser", "extension-core", "src", "background.js");

const FORBIDDEN_PERMISSIONS = ["downloads", "cookies", "webRequest"];
const FORBIDDEN_HOST_PERMISSIONS = ["http://*/*", "https://*/*"];

const PLACEHOLDER_FIREFOX_ID = "vibe-downloader@example.invalid";
const PLACEHOLDER_ID_PREFIX = "replace-with-";

const failures = [];

function fail(message) {
  failures.push(message);
}

async function readJson(filePath) {
  const text = await readFile(filePath, "utf8");
  return JSON.parse(text);
}

// --- Assertion 1: forbidden permissions / host_permissions ------------------

async function assertManifestsContainNoForbiddenPermissions() {
  let entries;
  try {
    entries = await readdir(distDir, { withFileTypes: true });
  } catch (error) {
    fail(
      `Could not read ${path.relative(root, distDir)}: ${error.message}. Did you run \`pnpm build:extensions\` first?`,
    );
    return;
  }

  const variantDirs = entries.filter((entry) => entry.isDirectory());
  if (variantDirs.length === 0) {
    fail(`No variant directories under ${path.relative(root, distDir)}. Run \`pnpm build:extensions\` first.`);
    return;
  }

  for (const entry of variantDirs) {
    const manifestPath = path.join(distDir, entry.name, "manifest.json");
    let manifest;
    try {
      manifest = await readJson(manifestPath);
    } catch (error) {
      fail(`[${entry.name}] could not read manifest.json: ${error.message}`);
      continue;
    }

    const permissions = manifest.permissions ?? [];
    const leaked = permissions.filter((permission) => FORBIDDEN_PERMISSIONS.includes(permission));
    if (leaked.length > 0) {
      fail(
        `[${entry.name}] manifest.json contains forbidden permissions: ${leaked.join(", ")}. ` +
          "These are experimental-capture-only and must not ship in default builds.",
      );
    }

    const hostPermissions = manifest.host_permissions ?? [];
    const leakedHost = hostPermissions.filter((host) => FORBIDDEN_HOST_PERMISSIONS.includes(host));
    if (leakedHost.length > 0) {
      fail(
        `[${entry.name}] manifest.json contains forbidden host_permissions: ${leakedHost.join(", ")}. ` +
          "Host permissions are experimental-capture-only.",
      );
    }
  }
}

// --- Assertion 2 + 3: release-only ID placeholders --------------------------
//
// The release profile is opt-in via `VIBE_BROWSER_PROFILE=release`. When it
// is NOT set, we skip the release assertions (the dev build intentionally
// uses placeholder IDs). When it IS set, the environment must supply real
// IDs via `VIBE_CHROME_EXTENSION_ID` / `VIBE_EDGE_EXTENSION_ID` /
// `VIBE_FIREFOX_EXTENSION_ID`.

async function assertReleaseIdsAreNotPlaceholders() {
  const profile = process.env.VIBE_BROWSER_PROFILE === "release" ? "release" : "dev";
  if (profile !== "release") {
    // Dev build: placeholders are expected. Skip release assertions.
    return;
  }

  const chromeId = process.env.VIBE_CHROME_EXTENSION_ID;
  const edgeId = process.env.VIBE_EDGE_EXTENSION_ID;
  const firefoxId = process.env.VIBE_FIREFOX_EXTENSION_ID;

  for (const [label, id] of [
    ["VIBE_CHROME_EXTENSION_ID", chromeId],
    ["VIBE_EDGE_EXTENSION_ID", edgeId],
    ["VIBE_FIREFOX_EXTENSION_ID", firefoxId],
  ]) {
    if (!id) {
      fail(`[release] ${label} is not set. Release builds require real extension IDs.`);
      continue;
    }
    if (id.startsWith(PLACEHOLDER_ID_PREFIX)) {
      fail(`[release] ${label}=${id} is still a placeholder (must not start with "${PLACEHOLDER_ID_PREFIX}").`);
    }
    if (label === "VIBE_FIREFOX_EXTENSION_ID" && id === PLACEHOLDER_FIREFOX_ID) {
      fail(`[release] VIBE_FIREFOX_EXTENSION_ID=${id} is the placeholder (must be the real addons.mozilla.org ID).`);
    }
  }
}

// --- Assertion 4: Rust ↔ JS header allowlist set equality ------------------

function parseRustAllowlist(source) {
  // Match `const FORWARDED_HEADER_ALLOWLIST: &[&str] = &[ ... ];`
  const match = source.match(/const\s+FORWARDED_HEADER_ALLOWLIST[^=]*=\s*&\[(?<items>[^\]]*)\]/s);
  if (!match) {
    throw new Error("Could not locate FORWARDED_HEADER_ALLOWLIST in browser.rs");
  }
  const items = match.groups.items
    .split(",")
    .map((item) => item.trim())
    .filter((item) => item.length > 0)
    .map((item) => item.replace(/(^")|("$)/g, ""))
    .map((item) => item.toLowerCase());
  return new Set(items);
}

function parseJsAllowlist(source) {
  // Match `const ALLOWED_HEADER_NAMES = new Set([ ... ]);`
  const match = source.match(/const\s+ALLOWED_HEADER_NAMES\s*=\s*new\s+Set\(\[(?<items>[^\]]*)\]\)/s);
  if (!match) {
    throw new Error("Could not locate ALLOWED_HEADER_NAMES in background.js");
  }
  const items = match.groups.items
    .split(",")
    .map((item) => item.trim())
    .filter((item) => item.length > 0)
    .map((item) => item.replace(/(^")|("$)/g, ""))
    .map((item) => item.toLowerCase());
  return new Set(items);
}

async function assertHeaderAllowlistMatchesBetweenRustAndJs() {
  const rustSource = await readFile(browserRustPath, "utf8");
  const jsSource = await readFile(backgroundJsPath, "utf8");

  let rustSet;
  let jsSet;
  try {
    rustSet = parseRustAllowlist(rustSource);
  } catch (error) {
    fail(error.message);
    return;
  }
  try {
    jsSet = parseJsAllowlist(jsSource);
  } catch (error) {
    fail(error.message);
    return;
  }

  const rustOnly = [...rustSet].filter((name) => !jsSet.has(name));
  const jsOnly = [...jsSet].filter((name) => !rustSet.has(name));

  if (rustOnly.length > 0) {
    fail(
      `Rust FORWARDED_HEADER_ALLOWLIST has entries not present in JS ALLOWED_HEADER_NAMES: ${rustOnly.join(", ")}. ` +
        "Headers allowed by Rust but not by JS would be silently dropped before reaching the desktop app.",
    );
  }
  if (jsOnly.length > 0) {
    fail(
      `JS ALLOWED_HEADER_NAMES has entries not present in Rust FORWARDED_HEADER_ALLOWLIST: ${jsOnly.join(", ")}. ` +
        "Headers allowed by JS but not by Rust would be rejected at the handoff boundary and never forwarded to the download engine.",
    );
  }
}

// --- Runner ----------------------------------------------------------------

async function main() {
  await assertManifestsContainNoForbiddenPermissions();
  await assertReleaseIdsAreNotPlaceholders();
  await assertHeaderAllowlistMatchesBetweenRustAndJs();

  if (failures.length > 0) {
    console.error(`\nExtension manifest verification failed (${failures.length} issue(s)):`);
    for (const message of failures) {
      console.error(`  - ${message}`);
    }
    process.exit(1);
  }

  console.log("Extension manifest verification passed.");
}

await main();
