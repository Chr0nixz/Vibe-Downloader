import { createHash } from "node:crypto";
import { readdir, readFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { unzipSync } from "fflate";

import {
  CANDIDATE_CHROMIUM_EXTENSION_ID,
  extensionIdFromPublicKey,
  resolveExtensionIdentity,
} from "./release-config.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const distDir = path.join(root, "browser", "dist");
const packagesDir = path.join(distDir, "packages");
const browserRustPath = path.join(root, "src-tauri", "src", "commands", "browser.rs");
const buildRsPath = path.join(root, "src-tauri", "build.rs");
const backgroundJsPath = path.join(root, "browser", "extension-core", "src", "background.js");
const identity = resolveExtensionIdentity(process.env, "dev");
const failures = [];

const SENSITIVE_PERMISSIONS = ["downloads", "cookies", "webRequest"];
const SENSITIVE_HOST_PERMISSIONS = ["http://*/*", "https://*/*"];

function fail(message) {
  failures.push(message);
}

async function readJson(filePath) {
  return JSON.parse(await readFile(filePath, "utf8"));
}

async function verifyVariantSetAndManifests() {
  const entries = await readdir(distDir, { withFileTypes: true });
  const actual = entries
    .filter((entry) => entry.isDirectory() && entry.name !== "packages")
    .map((entry) => entry.name)
    .sort();
  const expected = [...identity.variants].sort();
  if (JSON.stringify(actual) !== JSON.stringify(expected)) {
    fail(`Expected extension variants ${expected.join(", ")}; found ${actual.join(", ") || "none"}.`);
  }

  for (const variant of expected) {
    const manifestPath = path.join(distDir, variant, "manifest.json");
    let manifest;
    try {
      manifest = await readJson(manifestPath);
    } catch (error) {
      fail(`[${variant}] could not read manifest.json: ${error.message}`);
      continue;
    }

    const permissions = manifest.permissions ?? [];
    const hostPermissions = manifest.host_permissions ?? [];
    if (identity.captureAvailable) {
      for (const permission of SENSITIVE_PERMISSIONS) {
        if (!permissions.includes(permission)) fail(`[${variant}] dev capture build is missing ${permission}.`);
      }
      for (const host of SENSITIVE_HOST_PERMISSIONS) {
        if (!hostPermissions.includes(host)) fail(`[${variant}] dev capture build is missing ${host}.`);
      }
    } else {
      const leaked = permissions.filter((permission) => SENSITIVE_PERMISSIONS.includes(permission));
      const leakedHosts = hostPermissions.filter((host) => SENSITIVE_HOST_PERMISSIONS.includes(host));
      if (leaked.length > 0) fail(`[${variant}] contains forbidden permissions: ${leaked.join(", ")}.`);
      if (leakedHosts.length > 0) fail(`[${variant}] contains forbidden host permissions: ${leakedHosts.join(", ")}.`);
    }

    if (variant === "firefox") {
      const firefoxId = manifest.browser_specific_settings?.gecko?.id;
      if (firefoxId !== identity.firefoxId) {
        fail(`[firefox] gecko.id=${firefoxId ?? "missing"} does not match ${identity.firefoxId}.`);
      }
      if (manifest.key) fail("[firefox] Chromium manifest key must not be present.");
    } else if (identity.chromiumPublicKey) {
      if (manifest.key !== identity.chromiumPublicKey)
        fail(`[${variant}] candidate manifest key is missing or incorrect.`);
      if (extensionIdFromPublicKey(manifest.key) !== identity.chromeId) {
        fail(`[${variant}] manifest key does not derive the candidate extension ID.`);
      }
    } else if (manifest.key) {
      fail(`[${variant}] release store package must not contain the candidate manifest key.`);
    }
  }
}

function parseStringSet(source, pattern, label) {
  const match = source.match(pattern);
  if (!match?.groups?.items) throw new Error(`Could not locate ${label}.`);
  return new Set(
    match.groups.items
      .split(",")
      .map((item) =>
        item
          .trim()
          .replace(/(^")|("$)/g, "")
          .toLowerCase(),
      )
      .filter(Boolean),
  );
}

async function verifyHeaderAllowlist() {
  const [rustSource, jsSource] = await Promise.all([
    readFile(browserRustPath, "utf8"),
    readFile(backgroundJsPath, "utf8"),
  ]);
  try {
    const rustSet = parseStringSet(
      rustSource,
      /const\s+FORWARDED_HEADER_ALLOWLIST[^=]*=\s*&\[(?<items>[^\]]*)\]/s,
      "FORWARDED_HEADER_ALLOWLIST",
    );
    const jsSet = parseStringSet(
      jsSource,
      /const\s+ALLOWED_HEADER_NAMES\s*=\s*new\s+Set\(\[(?<items>[^\]]*)\]\)/s,
      "ALLOWED_HEADER_NAMES",
    );
    const rustOnly = [...rustSet].filter((name) => !jsSet.has(name));
    const jsOnly = [...jsSet].filter((name) => !rustSet.has(name));
    if (rustOnly.length || jsOnly.length) {
      fail(
        `Header allowlists differ (Rust only: ${rustOnly.join(", ") || "none"}; JS only: ${jsOnly.join(", ") || "none"}).`,
      );
    }
  } catch (error) {
    fail(error.message);
  }
}

async function verifyCompiledCandidateIdentity() {
  const buildSource = await readFile(buildRsPath, "utf8");
  const match = buildSource.match(/CANDIDATE_CHROMIUM_EXTENSION_ID:\s*&str\s*=\s*"([a-p]{32})"/);
  if (!match) {
    fail("Could not locate the Rust candidate Chromium extension ID in build.rs.");
  } else if (match[1] !== CANDIDATE_CHROMIUM_EXTENSION_ID) {
    fail(`Rust candidate ID ${match[1]} differs from Node candidate ID ${CANDIDATE_CHROMIUM_EXTENSION_ID}.`);
  }
}

async function verifyArtifacts() {
  let metadata;
  try {
    metadata = await readJson(path.join(packagesDir, "extension-artifacts.json"));
  } catch (error) {
    fail(`Could not read extension artifact metadata: ${error.message}`);
    return;
  }
  if (metadata.profile !== identity.profile || metadata.captureAvailable !== identity.captureAvailable) {
    fail("Extension artifact metadata does not match the requested build profile.");
  }
  if (metadata.artifacts?.length !== identity.variants.length) {
    fail(`Expected ${identity.variants.length} packaged extensions; found ${metadata.artifacts?.length ?? 0}.`);
    return;
  }

  for (const artifact of metadata.artifacts) {
    const filePath = path.join(packagesDir, artifact.file);
    try {
      const archive = await readFile(filePath);
      const digest = createHash("sha256").update(archive).digest("hex");
      if (digest !== artifact.sha256) fail(`${artifact.file} SHA-256 does not match metadata.`);
      const entries = unzipSync(new Uint8Array(archive));
      if (!entries["manifest.json"] || !entries["background.js"]) {
        fail(`${artifact.file} is missing manifest.json or background.js.`);
      }
    } catch (error) {
      fail(`Could not verify ${artifact.file}: ${error.message}`);
    }
  }
}

async function main() {
  await verifyVariantSetAndManifests();
  await verifyHeaderAllowlist();
  await verifyCompiledCandidateIdentity();
  await verifyArtifacts();

  if (failures.length > 0) {
    console.error(`\nExtension verification failed (${failures.length} issue(s)):`);
    for (const message of failures) console.error(`  - ${message}`);
    process.exit(1);
  }
  console.log(
    `Extension verification passed for ${identity.profile} (${identity.variants.length} variants, capture=${identity.captureAvailable}).`,
  );
}

await main();
