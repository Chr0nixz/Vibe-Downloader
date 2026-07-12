import assert from "node:assert/strict";
import test from "node:test";

import { validateReleasePreflight } from "./release-preflight.mjs";

const versions = {
  "package.json": "0.2.0-rc.1",
  "src-tauri/tauri.conf.json": "0.2.0-rc.1",
  "src-tauri/Cargo.toml": "0.2.0-rc.1",
};

test("candidate preflight accepts aligned versions and updater signing", () => {
  const result = validateReleasePreflight({
    tag: "v0.2.0-rc.1",
    versions,
    profile: "candidate",
    env: { TAURI_SIGNING_PRIVATE_KEY: "test-key" },
  });
  assert.equal(result.version, "0.2.0-rc.1");
  assert.equal(result.identity.profile, "candidate");
});

test("preflight rejects version drift", () => {
  assert.throws(
    () =>
      validateReleasePreflight({
        tag: "v0.2.0-rc.2",
        versions,
        profile: "candidate",
        env: { TAURI_SIGNING_PRIVATE_KEY: "test-key" },
      }),
    /does not match tag/,
  );
});

test("preflight rejects missing updater signing material", () => {
  assert.throws(
    () => validateReleasePreflight({ tag: "v0.2.0-rc.1", versions, profile: "candidate", env: {} }),
    /TAURI_SIGNING_PRIVATE_KEY/,
  );
});
