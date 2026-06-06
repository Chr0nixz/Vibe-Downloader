#!/usr/bin/env node
/**
 * Sync app version from a git tag (e.g. v0.2.0) into package.json,
 * src-tauri/tauri.conf.json, and src-tauri/Cargo.toml.
 */
import { readFileSync, writeFileSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");

function parseVersion(tag) {
  const raw = tag.startsWith("v") ? tag.slice(1) : tag;
  if (!/^\d+\.\d+\.\d+(-[\w.]+)?(\+[\w.]+)?$/.test(raw)) {
    console.error(`Invalid version tag: ${tag}`);
    process.exit(1);
  }
  return raw;
}

const tag = process.argv[2];
if (!tag) {
  console.error("Usage: node scripts/sync-version.mjs <tag>");
  process.exit(1);
}

const version = parseVersion(tag);

const packageJsonPath = resolve(root, "package.json");
const packageJson = JSON.parse(readFileSync(packageJsonPath, "utf8"));
packageJson.version = version;
writeFileSync(packageJsonPath, `${JSON.stringify(packageJson, null, 2)}\n`);

const tauriConfPath = resolve(root, "src-tauri/tauri.conf.json");
const tauriConf = JSON.parse(readFileSync(tauriConfPath, "utf8"));
tauriConf.version = version;
writeFileSync(tauriConfPath, `${JSON.stringify(tauriConf, null, 2)}\n`);

const cargoTomlPath = resolve(root, "src-tauri/Cargo.toml");
let cargoToml = readFileSync(cargoTomlPath, "utf8");
cargoToml = cargoToml.replace(
  /^version = ".*"$/m,
  `version = "${version}"`,
);
writeFileSync(cargoTomlPath, cargoToml);

console.log(`Synced version ${version} from tag ${tag}`);
