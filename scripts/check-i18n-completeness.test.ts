import assert from "node:assert/strict";
import { copyFileSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import {
  checkLocale,
  extractLiteralTranslationKeys,
  findMissingSourceKeys,
  findPlaceholderMismatches,
  findPluralBases,
  findUntranslatedLeaves,
  flattenLeaves,
  isValueAllowlisted,
  localeHasFailures,
  placeholderNames,
  pluralCategories,
  runCompletenessCheck,
  visibleEnglishWords,
} from "./check-i18n-completeness.ts";

test("stable-only verifies both stable locales while the default still rejects missing beta keys", async (context) => {
  const dir = mkdtempSync(join(tmpdir(), "i18n-stable-"));
  context.mock.method(console, "log", () => {});
  context.mock.method(console, "error", () => {});
  try {
    copyFileSync(new URL("../src/i18n/locales/en.ts", import.meta.url), join(dir, "en.ts"));
    assert.equal(await runCompletenessCheck(dir, true), 1);
    copyFileSync(new URL("../src/i18n/locales/zh-CN.ts", import.meta.url), join(dir, "zh-CN.ts"));
    writeFileSync(join(dir, "ja.ts"), "export default {};");
    assert.equal(await runCompletenessCheck(dir, true), 0);
    assert.equal(await runCompletenessCheck(dir), 1);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("flattenLeaves walks nested string leaves", () => {
  assert.deepEqual(flattenLeaves({ a: { b: "x", c: "y" }, d: "z" }), { "a.b": "x", "a.c": "y", d: "z" });
});

test("placeholderNames extracts interpolation names", () => {
  assert.deepEqual(placeholderNames("Show {{count}} of {{total}}"), ["count", "total"]);
  assert.deepEqual(placeholderNames("no placeholders"), []);
});

test("visibleEnglishWords ignores interpolation names", () => {
  assert.deepEqual(visibleEnglishWords("{{action}} {{done}}/{{total}}…"), []);
  assert.deepEqual(visibleEnglishWords("Show {{count}} more"), ["Show", "more"]);
});

test("allowlist covers product name and protocol tokens", () => {
  assert.equal(isValueAllowlisted("app.name"), true);
  assert.equal(isValueAllowlisted("locale.en"), true);
  assert.equal(isValueAllowlisted("format.byteUnit.mb"), true);
  assert.equal(isValueAllowlisted("taskList.failure_http"), true);
  assert.equal(isValueAllowlisted("errors.authHeadersExpired"), false);
});

test("findUntranslatedLeaves flags errors.* even with short English", () => {
  const hits = findUntranslatedLeaves(
    { "errors.timeout": "The request timed out.", "nav.all": "All tasks" },
    { "errors.timeout": "The request timed out.", "nav.all": "全部任务" },
  );
  assert.deepEqual(
    hits.map((h) => h.key),
    ["errors.timeout"],
  );
  assert.equal(hits[0]?.reason, "errors");
});

test("findUntranslatedLeaves flags 3+ word English copy outside errors", () => {
  const hits = findUntranslatedLeaves(
    {
      "settings.siteRuleDeleted": "Site rule removed",
      "settings.cancelRule": "Cancel",
    },
    {
      "settings.siteRuleDeleted": "Site rule removed",
      "settings.cancelRule": "Cancel",
    },
  );
  assert.deepEqual(
    hits.map((h) => h.key),
    ["settings.siteRuleDeleted"],
  );
  assert.equal(hits[0]?.reason, "english-sentence");
});

test("findUntranslatedLeaves skips allowlisted identical values", () => {
  const hits = findUntranslatedLeaves(
    { "app.name": "Vibe Downloader", "errors.report.url": "URL" },
    { "app.name": "Vibe Downloader", "errors.report.url": "URL" },
  );
  assert.deepEqual(hits, []);
});

test("findPlaceholderMismatches reports dropped interpolations", () => {
  const hits = findPlaceholderMismatches({ "toast.showMore": "Show {{count}} more" }, { "toast.showMore": "显示更多" });
  assert.equal(hits.length, 1);
  assert.deepEqual(hits[0]?.en, ["count"]);
  assert.deepEqual(hits[0]?.locale, []);
});

test("checkLocale aggregates missing extra untranslated and placeholders", () => {
  const result = checkLocale(
    "zh-CN",
    { "nav.all": "All tasks", "errors.timeout": "The request timed out." },
    { "nav.all": "All tasks", extra: "多余" },
  );
  assert.deepEqual(result.missing, ["errors.timeout"]);
  assert.deepEqual(result.extra, ["extra"]);
  assert.equal(localeHasFailures(result), true);
});

test("translated locale with matching placeholders is clean", () => {
  const result = checkLocale(
    "zh-CN",
    { "toast.showMore": "Show {{count}} more", "errors.timeout": "The request timed out." },
    { "toast.showMore": "显示另外 {{count}} 条", "errors.timeout": "请求超时。" },
  );
  assert.equal(localeHasFailures(result), false);
});

test("pluralCategories reflects CLDR, not the app's own locale list", () => {
  assert.deepEqual([...pluralCategories("zh-CN")].sort(), ["other"]);
  assert.equal(pluralCategories("ru").has("few"), true);
  assert.equal(pluralCategories("ru").has("many"), true);
  assert.equal(pluralCategories("en").has("few"), false);
});

test("findPluralBases only treats {{count}} keys with a suffixed sibling as plural", () => {
  const bases = findPluralBases({
    "taskList.bulkDelete": "Delete {{count}}",
    "taskList.bulkDelete_one": "Delete {{count}}",
    "taskList.failure": "Failure",
    "taskList.failure_other": "Other failure",
    "toast.showMore": "Show {{count}} more",
  });
  assert.deepEqual([...bases], ["taskList.bulkDelete"]);
});

test("checkLocale allows a locale to add the plural forms its own CLDR requires", () => {
  const en = { "toast.tasksDeleted": "{{count}} tasks removed", "toast.tasksDeleted_one": "{{count}} task removed" };
  const ru = {
    "toast.tasksDeleted": "{{count}} задач удалено",
    "toast.tasksDeleted_one": "{{count}} задача удалена",
    "toast.tasksDeleted_few": "{{count}} задачи удалены",
    "toast.tasksDeleted_many": "{{count}} задач удалено",
  };
  const result = checkLocale("ru", en, ru);
  assert.deepEqual(result.extra, []);
  assert.deepEqual(result.unreachablePlurals, []);
  assert.equal(localeHasFailures(result), false);
});

test("checkLocale does not demand a plural form a locale never selects", () => {
  const en = { "toast.tasksDeleted": "{{count}} tasks removed", "toast.tasksDeleted_one": "{{count}} task removed" };
  const zh = { "toast.tasksDeleted": "已删除 {{count}} 个任务" };
  const result = checkLocale("zh-CN", en, zh);
  assert.deepEqual(result.missing, []);
  assert.equal(localeHasFailures(result), false);
});

test("checkLocale flags plural forms the locale never selects", () => {
  const en = { "toast.tasksDeleted": "{{count}} tasks removed", "toast.tasksDeleted_one": "{{count}} task removed" };
  const zh = { "toast.tasksDeleted": "已删除 {{count}} 个任务", "toast.tasksDeleted_one": "已删除 1 个任务" };
  const result = checkLocale("zh-CN", en, zh);
  assert.deepEqual(result.unreachablePlurals, ["toast.tasksDeleted_one"]);
  assert.equal(localeHasFailures(result), true);
});

test("checkLocale still fails when a locale drops a required plural form", () => {
  const en = { "toast.tasksDeleted": "{{count}} tasks removed", "toast.tasksDeleted_one": "{{count}} task removed" };
  const ru = { "toast.tasksDeleted": "{{count}} задач удалено" };
  const result = checkLocale("ru", en, ru);
  assert.deepEqual(result.missing, ["toast.tasksDeleted_one"]);
});

test("extractLiteralTranslationKeys finds literal t() calls only", () => {
  const source = [
    't("nav.all")',
    "t('nav.about')",
    'i18n.t("format.speed")',
    // biome-ignore lint/suspicious/noTemplateCurlyInString: the string intentionally contains a dynamic t() call to assert it is not extracted
    "t(`taskList.failure_${kind}`)",
    't("taskList.failure_" + kind)',
    "t(key)",
    'something.t("not.ours")',
  ].join("\n");
  assert.deepEqual(extractLiteralTranslationKeys(source), ["nav.all", "nav.about", "format.speed"]);
});

test("findMissingSourceKeys reports literals absent from the English bundle", () => {
  const dir = mkdtempSync(join(tmpdir(), "i18n-scan-"));
  try {
    writeFileSync(
      join(dir, "Widget.tsx"),
      'export const a = () => t("nav.all");\nexport const b = () => t("nav.typo");\n',
    );
    writeFileSync(join(dir, "Widget.test.tsx"), 'export const c = () => t("nav.onlyInTest");\n');
    mkdirSync(join(dir, "nested"));
    writeFileSync(join(dir, "nested", "deep.ts"), 'export const d = () => t("nav.deep");\n');

    const hits = findMissingSourceKeys({ "nav.all": "All tasks" }, dir);
    assert.deepEqual(
      hits.map((h) => [h.file.replace(/\\/g, "/").split("/").pop(), h.line, h.key]),
      [
        ["deep.ts", 1, "nav.deep"],
        ["Widget.tsx", 2, "nav.typo"],
      ],
    );
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
