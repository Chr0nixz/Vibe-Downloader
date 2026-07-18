import { access, readFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import { resolveBrowserProfile, resolveExtensionIdentity } from "./release-config.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

export function validateReleasePreflight({ tag, versions, profile, env }) {
  const normalizedProfile = resolveBrowserProfile(profile, "candidate");
  const normalizedTag = String(tag ?? "").trim();
  if (!/^v\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(normalizedTag)) {
    throw new Error(`Release tag must be v-prefixed semver; received ${normalizedTag || "(empty)"}.`);
  }
  const version = normalizedTag.slice(1);
  for (const [source, actual] of Object.entries(versions)) {
    if (actual !== version) {
      throw new Error(`${source} version ${actual} does not match tag ${normalizedTag}.`);
    }
  }
  if (!String(env.TAURI_SIGNING_PRIVATE_KEY ?? "").trim()) {
    throw new Error("TAURI_SIGNING_PRIVATE_KEY is required for candidate and release builds.");
  }
  const identity = resolveExtensionIdentity({ ...env, VIBE_BROWSER_PROFILE: normalizedProfile }, normalizedProfile);
  if (identity.captureAvailable) {
    throw new Error("Candidate and release preflight require the minimal-permission extension profile.");
  }
  return { version, tag: normalizedTag, identity };
}

function formatIdentitySummary(identity) {
  const fallback = identity.usingCandidateFallback ? ", candidate extension IDs" : "";
  return `${identity.profile}${fallback}, ${identity.variants.join(", ")}`;
}

function cargoVersion(source) {
  const match = source.match(/^version\s*=\s*"([^"]+)"/m);
  if (!match) throw new Error("Could not read package version from src-tauri/Cargo.toml.");
  return match[1];
}

async function readWorkspaceVersions(workspaceRoot) {
  const [packageJson, tauriConfig, cargoToml] = await Promise.all([
    readFile(path.join(workspaceRoot, "package.json"), "utf8").then(JSON.parse),
    readFile(path.join(workspaceRoot, "src-tauri", "tauri.conf.json"), "utf8").then(JSON.parse),
    readFile(path.join(workspaceRoot, "src-tauri", "Cargo.toml"), "utf8"),
  ]);
  return {
    "package.json": packageJson.version,
    "src-tauri/tauri.conf.json": tauriConfig.version,
    "src-tauri/Cargo.toml": cargoVersion(cargoToml),
  };
}

async function assertRequiredFiles(workspaceRoot) {
  for (const relative of [
    "docs/browser-extension-privacy.md",
    "docs/browser-store-submission.md",
    "docs/updater-rehearsal.md",
  ]) {
    await access(path.join(workspaceRoot, relative)).catch(() => {
      throw new Error(`Required release document is missing: ${relative}.`);
    });
  }
  const bundleConfig = JSON.parse(
    await readFile(path.join(workspaceRoot, "src-tauri", "tauri.bundle.conf.json"), "utf8"),
  );
  if (!bundleConfig.bundle?.externalBin?.includes("binaries/vibe-native-host")) {
    throw new Error("Tauri bundle overlay does not include binaries/vibe-native-host.");
  }
  if (bundleConfig.build?.beforeBuildCommand !== "node scripts/native-host-build.mjs --with-frontend") {
    throw new Error("Tauri bundle overlay does not prepare the native host before Rust compilation.");
  }
  if (bundleConfig.build?.beforeBundleCommand !== "node scripts/native-host-build.mjs --verify-staged") {
    throw new Error("Tauri bundle overlay does not verify the native host before bundling.");
  }
}

async function main() {
  const tagIndex = process.argv.indexOf("--tag");
  const profileIndex = process.argv.indexOf("--profile");
  const tag = tagIndex >= 0 ? process.argv[tagIndex + 1] : process.env.GITHUB_REF_NAME;
  const profile = profileIndex >= 0 ? process.argv[profileIndex + 1] : process.env.VIBE_BROWSER_PROFILE;
  await assertRequiredFiles(root);
  const result = validateReleasePreflight({
    tag,
    profile,
    versions: await readWorkspaceVersions(root),
    env: process.env,
  });
  if (result.identity.usingCandidateFallback) {
    console.warn(
      "Store extension IDs are missing; continuing with candidate extension identities because VIBE_ALLOW_CANDIDATE_EXTENSION_IDS is set.",
    );
  }
  console.log(`Release preflight passed for ${result.tag} (${formatIdentitySummary(result.identity)}).`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  await main();
}
