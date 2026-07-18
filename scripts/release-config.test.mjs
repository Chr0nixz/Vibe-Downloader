import assert from "node:assert/strict";
import test from "node:test";

import {
  applyCapturePermissions,
  CANDIDATE_CHROMIUM_EXTENSION_ID,
  CANDIDATE_CHROMIUM_PUBLIC_KEY,
  extensionIdFromPublicKey,
  resolveExtensionIdentity,
} from "./release-config.mjs";

test("candidate Chromium key derives the ID compiled into the desktop app", () => {
  assert.equal(extensionIdFromPublicKey(CANDIDATE_CHROMIUM_PUBLIC_KEY), CANDIDATE_CHROMIUM_EXTENSION_ID);
});

test("candidate and release profiles cannot enable experimental capture", () => {
  for (const profile of ["candidate", "release"]) {
    assert.throws(
      () => resolveExtensionIdentity({ VIBE_BROWSER_PROFILE: profile, VIBE_BROWSER_EXPERIMENTAL_CAPTURE: "true" }),
      /Experimental capture cannot be enabled/,
    );
  }
});

test("release profile fails closed when formal IDs are missing", () => {
  assert.throws(() => resolveExtensionIdentity({ VIBE_BROWSER_PROFILE: "release" }), /VIBE_CHROME_EXTENSION_ID/);
});

test("release profile can fall back to candidate IDs when explicitly allowed", () => {
  const identity = resolveExtensionIdentity({
    VIBE_BROWSER_PROFILE: "release",
    VIBE_ALLOW_CANDIDATE_EXTENSION_IDS: "true",
  });
  assert.equal(identity.usingCandidateFallback, true);
  assert.equal(identity.chromeId, CANDIDATE_CHROMIUM_EXTENSION_ID);
  assert.equal(identity.edgeId, CANDIDATE_CHROMIUM_EXTENSION_ID);
  assert.equal(identity.firefoxId, "vibe-downloader-candidate@local");
  assert.equal(identity.chromiumPublicKey, CANDIDATE_CHROMIUM_PUBLIC_KEY);
  assert.equal(identity.captureAvailable, false);
});

test("release profile prefers formal IDs over candidate fallback", () => {
  const identity = resolveExtensionIdentity({
    VIBE_BROWSER_PROFILE: "release",
    VIBE_ALLOW_CANDIDATE_EXTENSION_IDS: "true",
    VIBE_CHROME_EXTENSION_ID: "abcdefghijklmnopabcdefghijklmnop",
    VIBE_EDGE_EXTENSION_ID: "ponmlkjihgfedcbaponmlkjihgfedcba",
    VIBE_FIREFOX_EXTENSION_ID: "vibe-downloader@example.com",
  });
  assert.equal(identity.usingCandidateFallback, false);
  assert.equal(identity.chromeId, "abcdefghijklmnopabcdefghijklmnop");
  assert.equal(identity.chromiumPublicKey, null);
});

test("release profile accepts the three formal identity shapes", () => {
  const identity = resolveExtensionIdentity({
    VIBE_BROWSER_PROFILE: "release",
    VIBE_CHROME_EXTENSION_ID: "abcdefghijklmnopabcdefghijklmnop",
    VIBE_EDGE_EXTENSION_ID: "ponmlkjihgfedcbaponmlkjihgfedcba",
    VIBE_FIREFOX_EXTENSION_ID: "vibe-downloader@example.com",
  });
  assert.deepEqual(identity.variants, ["chromium", "edge", "firefox"]);
  assert.equal(identity.captureAvailable, false);
  assert.equal(identity.usingCandidateFallback, false);
  assert.equal(identity.chromiumPublicKey, null);
});

test("minimal permission profile strips every sensitive capture permission", () => {
  const manifest = applyCapturePermissions(
    {
      permissions: ["nativeMessaging", "downloads", "cookies", "webRequest"],
      host_permissions: ["http://*/*", "https://*/*"],
    },
    false,
  );
  assert.deepEqual(manifest.permissions, ["nativeMessaging"]);
  assert.equal("host_permissions" in manifest, false);
});
