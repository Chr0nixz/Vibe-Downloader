import assert from "node:assert/strict";
import test from "node:test";

import {
  checkLocale,
  findPlaceholderMismatches,
  findUntranslatedLeaves,
  flattenLeaves,
  isValueAllowlisted,
  localeHasFailures,
  placeholderNames,
  visibleEnglishWords,
} from "./check-i18n-completeness.ts";

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
