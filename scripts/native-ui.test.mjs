import assert from "node:assert/strict";
import { mkdir, mkdtemp, readFile, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { summarize } from "./perf/native-ui.mjs";
import {
  activateNativeFixture,
  assertCandidateExecutable,
  candidateIdentifier,
  loadNativeFixtures,
  nativeSqliteAvailable,
  prepareNativeFixtures,
} from "./perf/native-ui-fixture.mjs";
import { b7CandidateConfig } from "./prepare-b7-candidate.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const { DatabaseSync } = nativeSqliteAvailable ? await import("node:sqlite") : {};

test("native performance fixtures reject production, traversal and updater-enabled identities", () => {
  for (const identifier of [
    "com.vibe.downloader",
    "com.vibe.downloader.b7.../escape",
    "com.vibe.downloader.b7.X",
    "com.vibe.downloader.b7.a",
  ]) {
    assert.throws(() => candidateIdentifier({ identifier, bundle: { createUpdaterArtifacts: false } }));
  }
  assert.throws(() =>
    candidateIdentifier({ identifier: "com.vibe.downloader.b7.test-run", bundle: { createUpdaterArtifacts: true } }),
  );
  assert.throws(
    () => assertCandidateExecutable(Buffer.from("com.vibe.downloader"), "com.vibe.downloader.b7.test-run"),
    /expected isolated candidate identity/,
  );
  assert.throws(
    () => assertCandidateExecutable(Buffer.from("com.vibe.downloader.b7.old-run"), "com.vibe.downloader.b7.test-run"),
    /expected isolated candidate identity/,
  );
  assertCandidateExecutable(
    Buffer.from("binary\0com.vibe.downloader.b7.test-run\0"),
    "com.vibe.downloader.b7.test-run",
  );
});

test("owned inert fixtures preserve ordinary app data and refuse live, busy or corrupt database replacement", {
  skip: !nativeSqliteAvailable && "Requires built-in node:sqlite; verified on Node 22.23.2",
}, async () => {
  // Kept as inspectable artifacts; no recursive deletion is needed for this test.
  const directory = await mkdtemp(path.join(os.tmpdir(), "vibe-native-ui-test-"));
  const appData = path.join(directory, "appdata");
  await mkdir(appData);
  const ordinary = path.join(appData, "com.vibe.downloader");
  await mkdir(ordinary);
  await writeFile(path.join(ordinary, "vibe.db"), "ordinary data");
  const configPath = path.join(directory, "tauri.b7.generated.json");
  await writeFile(configPath, JSON.stringify(b7CandidateConfig("test-run")));
  const fixture = await prepareNativeFixtures(configPath, root, appData);
  assert.deepEqual(await loadNativeFixtures(configPath, appData), fixture);
  await assert.rejects(
    activateNativeFixture({ ...fixture, dataDirectory: ordinary }, 1000, async () => {}),
    /redirected candidate data directory/,
  );
  await assert.rejects(prepareNativeFixtures(configPath, root, appData), { code: "EEXIST" });
  await assert.rejects(
    activateNativeFixture(fixture, 1000, async () => {
      throw new Error("still running");
    }),
    /still running/,
  );
  await activateNativeFixture(fixture, 1000, async () => {});
  const writer = new DatabaseSync(path.join(fixture.dataDirectory, "vibe.db"));
  try {
    writer.exec(
      "PRAGMA journal_mode=WAL; UPDATE settings SET value='test-committed' WHERE key='default_save_dir'; BEGIN IMMEDIATE; UPDATE settings SET value='test-uncommitted' WHERE key='default_save_dir';",
    );
    await assert.rejects(
      activateNativeFixture(fixture, 10000, async () => {}),
      /checkpoint is still busy/,
    );
    assert.equal(writer.prepare("SELECT COUNT(*) AS total FROM tasks").get().total, 1000);
  } finally {
    writer.exec("ROLLBACK");
    writer.close();
  }
  const before = await readFile(path.join(fixture.dataDirectory, "vibe.db"));
  await writeFile(path.join(fixture.dataDirectory, "vibe.db-wal"), "pending checkpoint");
  await assert.rejects(
    activateNativeFixture(fixture, 10000, async () => {}),
    /WAL\/SHM has not drained/,
  );
  assert.deepEqual(await readFile(path.join(fixture.dataDirectory, "vibe.db")), before);
  assert.equal(await readFile(path.join(ordinary, "vibe.db"), "utf8"), "ordinary data");
  const seed = new DatabaseSync(path.join(fixture.evidence, "seed-10000.sqlite"), { readOnly: true });
  try {
    assert.equal(seed.prepare("SELECT COUNT(*) AS total FROM tasks").get().total, 10000);
    assert.equal(
      seed.prepare("SELECT COUNT(*) AS active FROM tasks WHERE status IN ('queued','downloading','resuming')").get()
        .active,
      0,
    );
    assert.equal(seed.prepare("SELECT value FROM settings WHERE key='auto_resume_on_startup'").get().value, "false");
    assert.equal(seed.prepare("SELECT COUNT(*) AS total FROM task_files").get().total, 10000);
    assert.equal(seed.prepare("PRAGMA integrity_check").get().integrity_check, "ok");
  } finally {
    seed.close();
  }
});

test("native metrics use nearest-rank p95 without extrapolating small samples", () => {
  assert.deepEqual(summarize([4, 1, 5, 3, 2]), { samples: 5, min: 1, p50: 3, p95: 5, max: 5 });
  assert.throws(() => summarize([]));
  assert.throws(() => summarize([Number.NaN]));
});
