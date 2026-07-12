import { createHash } from "node:crypto";
import { readdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

async function listFiles(directory, prefix = "") {
  const files = [];
  const entries = await readdir(directory, { withFileTypes: true });
  for (const entry of entries) {
    const relative = prefix ? `${prefix}/${entry.name}` : entry.name;
    if (entry.isDirectory()) files.push(...(await listFiles(path.join(directory, entry.name), relative)));
    if (entry.isFile()) files.push(relative);
  }
  return files.sort();
}

export function classifyAssetCoverage(files) {
  const names = files.map((file) => file.toLowerCase());
  return {
    latestJson: names.some((name) => name.endsWith("latest.json")),
    signatures: names.filter((name) => name.endsWith(".sig")).length,
    windows: names.some((name) => name.endsWith(".msi") || name.endsWith("-setup.exe")),
    macArm: names.some((name) => /(aarch64|arm64).*\.(dmg|tar\.gz)$/.test(name)),
    macX64: names.some((name) => /(x64|x86_64).*\.(dmg|tar\.gz)$/.test(name)),
    linux: names.some((name) => name.endsWith(".appimage") || name.endsWith(".deb")),
    extensions: ["chromium", "edge", "firefox"].filter((browser) =>
      names.some((name) => name.includes(`vibe-downloader-${browser}-`) && name.endsWith(".zip")),
    ),
  };
}

export function assertAssetCoverage(coverage) {
  const missing = [];
  if (!coverage.latestJson) missing.push("latest.json");
  if (coverage.signatures === 0) missing.push("updater signatures");
  if (!coverage.windows) missing.push("Windows installer");
  if (!coverage.macArm) missing.push("macOS arm64 updater/bundle");
  if (!coverage.macX64) missing.push("macOS x64 updater/bundle");
  if (!coverage.linux) missing.push("Linux package");
  for (const browser of ["chromium", "edge", "firefox"]) {
    if (!coverage.extensions.includes(browser)) missing.push(`${browser} extension package`);
  }
  if (missing.length > 0) throw new Error(`Release assets are incomplete: ${missing.join(", ")}.`);
}

async function main() {
  const dirIndex = process.argv.indexOf("--dir");
  const versionIndex = process.argv.indexOf("--version");
  const directory = path.resolve(root, dirIndex >= 0 ? process.argv[dirIndex + 1] : ".release-assets");
  const version = versionIndex >= 0 ? process.argv[versionIndex + 1]?.replace(/^v/, "") : null;
  const files = await listFiles(directory);
  const coverage = classifyAssetCoverage(files);
  assertAssetCoverage(coverage);

  const latestPath = files.find((file) => file.toLowerCase().endsWith("latest.json"));
  const latest = JSON.parse(await readFile(path.join(directory, latestPath), "utf8"));
  if (version && latest.version !== version) {
    throw new Error(`latest.json version ${latest.version} does not match expected ${version}.`);
  }

  const sums = [];
  for (const relative of files.filter((file) => path.basename(file) !== "SHA256SUMS.txt")) {
    const bytes = await readFile(path.join(directory, relative));
    sums.push(`${createHash("sha256").update(bytes).digest("hex")}  ${relative.replaceAll("\\", "/")}`);
  }
  await writeFile(path.join(directory, "SHA256SUMS.txt"), `${sums.join("\n")}\n`);
  console.log(`Verified ${files.length} release assets for ${latest.version}; wrote SHA256SUMS.txt.`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  await main();
}
