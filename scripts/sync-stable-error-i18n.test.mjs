import assert from "node:assert/strict";
import test from "node:test";

import { parseErrorsBlockKeys, syncLocaleErrorsBlock } from "./sync-stable-error-i18n.mjs";

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
