#!/usr/bin/env node
/**
 * Sync app version from a git tag (e.g. v0.2.0) into package.json,
 * src-tauri/tauri.conf.json, src-tauri/Cargo.toml, and the vibe-downloader
 * entry in src-tauri/Cargo.lock (ENG-06: the lockfile was previously left
 * stale, so release builds silently rewrote it and made `--locked` gates
 * impossible).
 *
 * With --check: verifies that all four sources report the same version
 * and exits non-zero on mismatch (for CI).
 */
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");

const packageJsonPath = resolve(root, "package.json");
const tauriConfPath = resolve(root, "src-tauri/tauri.conf.json");
const cargoTomlPath = resolve(root, "src-tauri/Cargo.toml");
const cargoLockPath = resolve(root, "src-tauri/Cargo.lock");

function readVersionFromPackageJson() {
  return JSON.parse(readFileSync(packageJsonPath, "utf8")).version;
}

function readVersionFromTauriConf() {
  return JSON.parse(readFileSync(tauriConfPath, "utf8")).version;
}

function readVersionFromCargoToml() {
  const cargoToml = readFileSync(cargoTomlPath, "utf8");
  const match = cargoToml.match(/^version = "(.*)"$/m);
  if (!match) {
    throw new Error("Could not find version in Cargo.toml");
  }
  return match[1];
}

/**
 * Reads the version of the `vibe-downloader` package entry in Cargo.lock.
 * Only that entry (and vibe-native-host's) tracks the workspace version;
 * every other entry is a dependency pin that must not be touched.
 */
function readVersionFromCargoLock(packageName = "vibe-downloader") {
  const cargoLock = readFileSync(cargoLockPath, "utf8");
  const match = cargoLock.match(new RegExp(`\\[\\[package\\]\\]\\nname = "${packageName}"\\nversion = "(.*)"\\n`, "m"));
  if (!match) {
    throw new Error(`Could not find ${packageName} in Cargo.lock`);
  }
  return match[1];
}

function parseVersion(tag) {
  const raw = tag.startsWith("v") ? tag.slice(1) : tag;
  if (!/^\d+\.\d+\.\d+(-[\w.]+)?(\+[\w.]+)?$/.test(raw)) {
    console.error(`Invalid version tag: ${tag}`);
    process.exit(1);
  }
  return raw;
}

const isCheckMode = process.argv[2] === "--check";

if (isCheckMode) {
  const versions = {
    "package.json": readVersionFromPackageJson(),
    "src-tauri/tauri.conf.json": readVersionFromTauriConf(),
    "src-tauri/Cargo.toml": readVersionFromCargoToml(),
    "src-tauri/Cargo.lock": readVersionFromCargoLock(),
  };
  const uniqueVersions = new Set(Object.values(versions));

  if (uniqueVersions.size === 1) {
    console.log(`Version consistency check passed: all sources report ${versions["package.json"]}`);
    process.exit(0);
  }

  console.error("Version consistency check failed — sources disagree:");
  for (const [file, version] of Object.entries(versions)) {
    console.error(`  ${file}: ${version}`);
  }
  process.exit(1);
}

// --- Sync mode: write version from tag into all four sources ----------------

const tag = process.argv[2];
if (!tag) {
  console.error("Usage: node scripts/sync-version.mjs <tag>");
  console.error("       node scripts/sync-version.mjs --check");
  process.exit(1);
}

const version = parseVersion(tag);

const packageJson = JSON.parse(readFileSync(packageJsonPath, "utf8"));
packageJson.version = version;
writeFileSync(packageJsonPath, `${JSON.stringify(packageJson, null, 2)}\n`);

const tauriConf = JSON.parse(readFileSync(tauriConfPath, "utf8"));
tauriConf.version = version;
writeFileSync(tauriConfPath, `${JSON.stringify(tauriConf, null, 2)}\n`);

let cargoToml = readFileSync(cargoTomlPath, "utf8");
cargoToml = cargoToml.replace(/^version = ".*"$/m, `version = "${version}"`);
writeFileSync(cargoTomlPath, cargoToml);

let cargoLock = readFileSync(cargoLockPath, "utf8");
// vibe-native-host is a [[bin]] inside the vibe-downloader package, so this
// single entry is the only workspace version in the lockfile; dependency
// pins keep their own versions.
cargoLock = cargoLock.replace(/(\[\[package\]\]\nname = "vibe-downloader"\nversion = )"(.*?)"/m, `$1"${version}"`);
writeFileSync(cargoLockPath, cargoLock);

console.log(`Synced version ${version} from tag ${tag}`);
