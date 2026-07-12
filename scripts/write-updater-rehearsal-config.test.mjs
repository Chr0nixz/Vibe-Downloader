import assert from "node:assert/strict";
import test from "node:test";

import { updaterRehearsalConfig, updaterRehearsalEndpoint } from "./write-updater-rehearsal-config.mjs";

test("builds a tag-specific endpoint without using the production latest channel", () => {
  assert.equal(
    updaterRehearsalEndpoint("v0.2.0-rc.1"),
    "https://github.com/Chr0nixz/Vibe-Downloader/releases/download/v0.2.0-rc.1/latest.json",
  );
});

test("rejects unsafe or non-semver tags", () => {
  assert.throws(() => updaterRehearsalEndpoint("latest"), /v-prefixed semver/);
  assert.throws(() => updaterRehearsalEndpoint("v0.2.0/../../main"), /v-prefixed semver/);
});

test("keeps the native host bundle overlay in rehearsal builds", () => {
  const config = updaterRehearsalConfig("v0.2.0-rc.1");
  assert.equal(config.build.beforeBuildCommand, "node scripts/native-host-build.mjs --with-frontend");
  assert.equal(config.build.beforeBundleCommand, "node scripts/native-host-build.mjs");
  assert.deepEqual(config.bundle.externalBin, ["binaries/vibe-native-host"]);
  assert.deepEqual(config.plugins.updater.endpoints, [
    "https://github.com/Chr0nixz/Vibe-Downloader/releases/download/v0.2.0-rc.1/latest.json",
  ]);
});
