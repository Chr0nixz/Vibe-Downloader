import assert from "node:assert/strict";
import { chmod, mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import {
  nativeHostArtifactPaths,
  nativeHostCargoEnvironment,
  resolveTargetTriple,
  verifyStagedNativeHost,
} from "./native-host-build.mjs";

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

test("verifies a staged sidecar without rebuilding it", async () => {
  const root = await mkdtemp(path.join(os.tmpdir(), "vibe-native-host-test-"));
  try {
    const targetTriple = process.platform === "win32" ? "x86_64-pc-windows-msvc" : "x86_64-unknown-linux-gnu";
    const paths = nativeHostArtifactPaths(targetTriple, root);
    await mkdir(path.dirname(paths.staged), { recursive: true });
    await writeFile(paths.staged, "sidecar");
    await chmod(paths.staged, 0o755);
    const result = await verifyStagedNativeHost({
      targetTriple,
      workspaceRoot: root,
    });
    assert.equal(result.bytes, 7);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("rejects a missing staged sidecar", async () => {
  const root = await mkdtemp(path.join(os.tmpdir(), "vibe-native-host-test-"));
  try {
    await assert.rejects(
      verifyStagedNativeHost({ targetTriple: "x86_64-pc-windows-msvc", workspaceRoot: root }),
      /missing or empty/,
    );
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});
