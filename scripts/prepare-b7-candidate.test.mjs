import assert from "node:assert/strict";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import { b7CandidateConfig, prepareB7Candidate } from "./prepare-b7-candidate.mjs";

test("B7 packages isolate the application identity and disable updater artifacts", () => {
  const config = b7CandidateConfig("windows-20261012");
  assert.equal(config.identifier, "com.vibe.downloader.b7.windows-20261012");
  assert.notEqual(config.productName, "Vibe Downloader");
  assert.equal(config.bundle.createUpdaterArtifacts, false);
  assert.equal(config.bundle.windows.nsis.installMode, "currentUser");
  assert.deepEqual(config.bundle.externalBin, ["binaries/vibe-native-host"]);
  assert.equal(config.plugins, undefined);
});

test("rejects identifiers that could escape the evidence directory or overwrite the production identity", () => {
  for (const value of [undefined, "", "a", "../escape", "a/b", "C:\\data", "ALL", "a.b", "a".repeat(41)]) {
    assert.throws(() => b7CandidateConfig(value), /B7 run ID/);
  }
});

test("writes only an ignored overlay and refuses to overwrite a prior run", async () => {
  const directory = await mkdtemp(path.join(os.tmpdir(), "vibe-b7-overlay-"));
  try {
    const result = await prepareB7Candidate("test-run", directory);
    const generated = JSON.parse(await readFile(result.output, "utf8"));
    assert.deepEqual(generated, b7CandidateConfig("test-run"));
    assert.equal(result.output, path.join(directory, "logs", "b7", "test-run", "tauri.b7.generated.json"));
    await assert.rejects(prepareB7Candidate("test-run", directory), { code: "EEXIST" });
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});
