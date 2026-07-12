#!/usr/bin/env node
/**
 * Sync app version from a git tag (e.g. v0.2.0) into package.json,
 * src-tauri/tauri.conf.json, and src-tauri/Cargo.toml.
 *
 * With --check: verifies that all three files report the same version
 * and exits non-zero on mismatch (for CI).
 */
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");

const packageJsonPath = resolve(root, "package.json");
const tauriConfPath = resolve(root, "src-tauri/tauri.conf.json");
const cargoTomlPath = resolve(root, "src-tauri/Cargo.toml");

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

// --- Sync mode: write version from tag into all three files -----------------

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

console.log(`Synced version ${version} from tag ${tag}`);
