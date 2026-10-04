import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const projectRoot = fileURLToPath(new URL("../", import.meta.url));
const scriptUrl = new URL("./sync-stable-error-i18n.mjs", import.meta.url);
const localesRoot = path.join(projectRoot, "src/i18n/locales");

function snapshotLocales(root) {
  return fs
    .readdirSync(root)
    .filter((name) => name.endsWith(".ts"))
    .sort()
    .map((name) => {
      const file = path.join(root, name);
      return { name, content: fs.readFileSync(file, "utf8"), mtimeNs: fs.statSync(file, { bigint: true }).mtimeNs };
    });
}

// Static imports run before snapshots, which would hide import-time writes from this regression check.
const beforeImport = snapshotLocales(localesRoot);
const { parseErrorsBlockKeys, syncLocaleErrorsBlock } = await import(scriptUrl.href);
const afterImport = snapshotLocales(localesRoot);

test("importing the helpers preserves locale contents and modification times", () => {
  assert.equal(beforeImport.length, 7);
  assert.deepEqual(afterImport, beforeImport);
});

const CODES = ["temp_file_missing", "temp_file_smaller_than_progress"];
const MESSAGES = {
  temp_file_missing: "Temp file is missing",
  temp_file_smaller_than_progress: "Temp file is smaller than progress",
};
const REPORT = { code: "Code" };

function localeFile(errorsBlock) {
  return [`export default {`, `  common: {`, `    ok: "OK",`, `  },`, errorsBlock, `};`, ""].join("\n");
}

function errorsBlock(entries, report = REPORT) {
  const lines = ["  errors: {"];
  for (const [key, value] of entries) lines.push(`    ${key}: ${JSON.stringify(value)},`);
  lines.push("    report: {");
  for (const [key, value] of Object.entries(report)) lines.push(`      ${key}: ${JSON.stringify(value)},`);
  lines.push("    },");
  lines.push("  },");
  return lines.join("\n");
}

const SYNCED_BLOCK = errorsBlock([
  ["tempFileMissing", MESSAGES.temp_file_missing],
  ["tempFileSmallerThanProgress", MESSAGES.temp_file_smaller_than_progress],
]);

function createSyncFixture(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "vibe i18n sync "));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  for (const relative of [
    ".gitignore",
    "biome.json",
    "src/lib/stable-error-codes.ts",
    "scripts/stable-error-messages.json",
    "scripts/stable-error-causes.json",
  ]) {
    const destination = path.join(root, relative);
    fs.mkdirSync(path.dirname(destination), { recursive: true });
    fs.copyFileSync(path.join(projectRoot, relative), destination);
  }
  const locales = path.join(root, "src/i18n/locales");
  fs.mkdirSync(locales, { recursive: true });
  for (const { name } of beforeImport) {
    fs.writeFileSync(path.join(locales, name), localeFile(errorsBlock([["obsolete", "Needs syncing"]])));
  }
  return { root, locales };
}

test("a fresh import does not sync stale locales or produce CLI output", (t) => {
  const { root, locales } = createSyncFixture(t);
  const before = snapshotLocales(locales);
  for (const args of [[], ["unrelated-entry.mjs"]]) {
    const output = execFileSync(
      process.execPath,
      ["--input-type=module", "--eval", `await import(${JSON.stringify(scriptUrl.href)})`, ...args],
      { cwd: root, encoding: "utf8" },
    );
    assert.equal(output, "");
    assert.deepEqual(snapshotLocales(locales), before);
  }
});

test("direct sync writes Biome-formatted locales and leaves a second sync unchanged", (t) => {
  const { root, locales } = createSyncFixture(t);
  const runSync = () => execFileSync(process.execPath, [fileURLToPath(scriptUrl)], { cwd: root, encoding: "utf8" });
  const first = runSync();
  assert.equal(first.match(/^Updated /gm)?.length, 7);

  const biome = fileURLToPath(import.meta.resolve("@biomejs/biome/bin/biome"));
  execFileSync(process.execPath, [biome, "format", locales], { cwd: root, encoding: "utf8" });
  for (const { content } of snapshotLocales(locales)) {
    assert.ok(parseErrorsBlockKeys(content).includes("unsupportedUrlScheme"));
    assert.match(content, /common: \{\n {4}ok: "OK",\n {2}\},/u);
    assert.match(content, / {4}cause: \{/u);
  }
  const english = path.join(locales, "en.ts");
  const englishText = fs.readFileSync(english, "utf8");
  assert.match(englishText, /secretsUnavailable:\n {6}"/u);
  fs.writeFileSync(english, englishText.replace(/\n/g, "\r\n"));

  const before = snapshotLocales(locales);
  const second = runSync();
  assert.equal(second.match(/^Unchanged /gm)?.length, 7);
  assert.doesNotMatch(second, /^Updated /m);
  assert.deepEqual(snapshotLocales(locales), before);
});

test("a locale already carrying the generated block is reported unchanged", () => {
  const result = syncLocaleErrorsBlock({
    text: localeFile(SYNCED_BLOCK),
    messages: MESSAGES,
    report: REPORT,
    codes: CODES,
  });
  assert.equal(result.status, "unchanged");
});

test("a changed message is written back as updated", () => {
  const result = syncLocaleErrorsBlock({
    text: localeFile(SYNCED_BLOCK),
    messages: { ...MESSAGES, temp_file_missing: "临时文件丢失" },
    report: REPORT,
    codes: CODES,
  });
  assert.equal(result.status, "updated");
  assert.match(result.text, /tempFileMissing: "临时文件丢失"/u);
});

test("renaming a code keeps a healthy locale unchanged instead of failing", () => {
  // ENG-08: the old sentinel check ("tempFileSmallerThanProgress:") turned
  // every healthy locale into a hard failure once that code was renamed or
  // dropped. The block below is exactly what the script produces for the
  // reduced code list, so it must be recognised as already synced.
  const reducedCodes = ["temp_file_missing"];
  const reducedMessages = { temp_file_missing: MESSAGES.temp_file_missing };
  const reducedBlock = errorsBlock([["tempFileMissing", MESSAGES.temp_file_missing]]);

  const result = syncLocaleErrorsBlock({
    text: localeFile(reducedBlock),
    messages: reducedMessages,
    report: REPORT,
    codes: reducedCodes,
  });
  assert.equal(result.status, "unchanged");
  assert.deepEqual(reducedCodes, ["temp_file_missing"]);
});

test("a file without an errors block fails loudly instead of silently passing", () => {
  const result = syncLocaleErrorsBlock({
    text: localeFile(`  other: {\n    ok: "OK",\n  },`),
    messages: MESSAGES,
    report: REPORT,
    codes: CODES,
  });
  assert.equal(result.status, "failed");
});

test("CRLF checkouts are normalized and still recognized as synced", () => {
  const result = syncLocaleErrorsBlock({
    text: localeFile(SYNCED_BLOCK).replace(/\n/g, "\r\n"),
    messages: MESSAGES,
    report: REPORT,
    codes: CODES,
  });
  assert.equal(result.status, "unchanged");
});

test("parseErrorsBlockKeys reads the block keys and returns null when absent", () => {
  const keys = parseErrorsBlockKeys(localeFile(SYNCED_BLOCK));
  assert.ok(keys.includes("tempFileMissing"));
  assert.ok(keys.includes("tempFileSmallerThanProgress"));
  assert.equal(parseErrorsBlockKeys("export default {};"), null);
});

test("a generated cause sub-block survives a second sync", () => {
  // ENG-10: cause copy used to be hand-written inside the errors block and
  // vanished on the next wholesale replacement. It is generated data now, so
  // re-running the sync must keep it.
  const causes = { finalPathConflict: "Another file already occupies the destination path." };
  const first = syncLocaleErrorsBlock({
    text: localeFile(SYNCED_BLOCK),
    messages: MESSAGES,
    report: REPORT,
    codes: CODES,
    causes,
  });
  assert.equal(first.status, "updated");
  assert.match(first.text, / {4}cause: \{/u);
  assert.match(first.text, /finalPathConflict: "Another file/u);

  const second = syncLocaleErrorsBlock({
    text: first.text,
    messages: MESSAGES,
    report: REPORT,
    codes: CODES,
    causes,
  });
  assert.equal(second.status, "unchanged");
  assert.match(second.text, / {4}cause: \{/u);
});
