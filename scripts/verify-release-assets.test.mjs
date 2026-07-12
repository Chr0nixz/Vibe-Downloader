import assert from "node:assert/strict";
import test from "node:test";

import { assertAssetCoverage, classifyAssetCoverage } from "./verify-release-assets.mjs";

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
