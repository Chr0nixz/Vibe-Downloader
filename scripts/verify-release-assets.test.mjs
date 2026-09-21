import assert from "node:assert/strict";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import { assertAssetCoverage, assertUpdaterAssets, classifyAssetCoverage } from "./verify-release-assets.mjs";

const complete = [
  "latest.json",
  "vibe_0.2.0_x64_en-US.msi",
  "Vibe.Downloader_0.2.0_aarch64.app.tar.gz",
  "Vibe.Downloader_0.2.0_x64.app.tar.gz",
  "vibe-downloader_0.2.0_amd64.AppImage",
  "vibe-downloader_0.2.0_amd64.AppImage.sig",
  "vibe-downloader-chromium-v0.2.0.zip",
  "vibe-downloader-edge-v0.2.0.zip",
  "vibe-downloader-firefox-v0.2.0.zip",
];

test("recognizes a complete multi-platform release candidate", () => {
  const coverage = classifyAssetCoverage(complete);
  assert.doesNotThrow(() => assertAssetCoverage(coverage));
  assert.deepEqual(coverage.extensions, ["chromium", "edge", "firefox"]);
});

test("reports absent browser and platform assets", () => {
  const coverage = classifyAssetCoverage(["latest.json", "app.sig"]);
  assert.throws(() => assertAssetCoverage(coverage), /Windows installer/);
});

test("every updater platform must reference this tag's asset and matching signature", async (t) => {
  const directory = await mkdtemp(path.join(os.tmpdir(), "vibe-release-assets-"));
  t.after(() => rm(directory, { recursive: true, force: true }));
  const platforms = {};
  const files = [];
  for (const platform of ["darwin-aarch64", "darwin-x86_64", "linux-x86_64", "windows-x86_64"]) {
    const asset = `${platform}.bin`;
    const signature = `fixture-signature-${platform}`;
    await writeFile(path.join(directory, `${asset}.sig`), signature);
    files.push(asset, `${asset}.sig`);
    platforms[platform] = { url: `https://github.com/test/vibe/releases/download/v1.0.0/${asset}`, signature };
  }
  const input = { latest: { platforms }, files, directory, tag: "v1.0.0", repository: "test/vibe" };
  await assertUpdaterAssets(input);
  await assert.rejects(assertUpdaterAssets({ ...input, files: files.slice(2) }), /missing/);
  await assert.rejects(assertUpdaterAssets({ ...input, tag: "v1.0.1" }), /this release/);
  const partial = structuredClone(input);
  delete partial.latest.platforms["windows-x86_64"];
  await assert.rejects(assertUpdaterAssets(partial), /missing windows/);
  platforms["windows-x86_64"].signature = "wrong";
  await assert.rejects(assertUpdaterAssets(input), /signature.*does not match/);
});
