import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { readFile } from "node:fs/promises";
import path from "node:path";
import test from "node:test";
import { promisify } from "node:util";

const execFileAsync = promisify(execFile);
const root = path.resolve(import.meta.dirname, "..");
const sumsPath = path.join(root, "browser", "dist", "packages", "SHA256SUMS.txt");

test("browser extension packages are deterministic", async () => {
  const env = { ...process.env, VIBE_BROWSER_PROFILE: "candidate" };
  await execFileAsync(process.execPath, [path.join(root, "scripts", "build-browser-extensions.mjs")], {
    cwd: root,
    env,
  });
  const first = await readFile(sumsPath, "utf8");
  await execFileAsync(process.execPath, [path.join(root, "scripts", "build-browser-extensions.mjs")], {
    cwd: root,
    env,
  });
  const second = await readFile(sumsPath, "utf8");
  assert.equal(second, first);
});
