import assert from "node:assert/strict";
import path from "node:path";
import test from "node:test";

import { nativeHostArtifactPaths, nativeHostCargoEnvironment, resolveTargetTriple } from "./native-host-build.mjs";

test("maps every release matrix platform to the expected Rust target", () => {
  assert.equal(resolveTargetTriple("windows", "x64"), "x86_64-pc-windows-msvc");
  assert.equal(resolveTargetTriple("macos", "arm64"), "aarch64-apple-darwin");
  assert.equal(resolveTargetTriple("darwin", "x86_64"), "x86_64-apple-darwin");
  assert.equal(resolveTargetTriple("linux", "x86_64"), "x86_64-unknown-linux-gnu");
});

test("rejects bundle targets outside the supported release matrix", () => {
  assert.throws(() => resolveTargetTriple("linux", "aarch64"), /Unsupported native-host bundle target/);
});

test("uses Tauri target-suffixed sidecar names", () => {
  const root = path.resolve("workspace");
  const windows = nativeHostArtifactPaths("x86_64-pc-windows-msvc", root);
  assert.equal(windows.stagedName, "vibe-native-host-x86_64-pc-windows-msvc.exe");
  assert.equal(path.basename(windows.source), "vibe-native-host.exe");

  const mac = nativeHostArtifactPaths("aarch64-apple-darwin", root);
  assert.equal(mac.stagedName, "vibe-native-host-aarch64-apple-darwin");
  assert.equal(path.basename(mac.source), "vibe-native-host");
});

test("does not pass the bundle overlay into the nested sidecar Cargo build", () => {
  const env = nativeHostCargoEnvironment({
    TAURI_CONFIG: '{"bundle":{"externalBin":["binaries/vibe-native-host"]}}',
    VIBE_BROWSER_PROFILE: "release",
  });
  assert.equal(env.TAURI_CONFIG, undefined);
  assert.equal(env.VIBE_BROWSER_PROFILE, "release");
});
