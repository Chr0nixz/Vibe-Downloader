import assert from "node:assert/strict";
import test from "node:test";
import {
  checkBudgets,
  classifyAsset,
  loadBudget,
  matchAny,
  measureFile,
  summarizeAssets,
} from "./check-bundle-budget.mjs";

test("budget config loads with required keys", () => {
  const config = loadBudget();
  assert.equal(config.schemaVersion, 1);
  assert.ok(config.budgets.initialShellJsGzipBytes > 0);
  assert.ok(config.initialShell.include.length > 0);
});

test("classifyAsset separates initial shell from deferred routes", () => {
  const config = loadBudget();
  assert.equal(classifyAsset("index-abc123.js", config), "initial");
  assert.equal(classifyAsset("react-vendor-def456.js", config), "initial");
  assert.equal(classifyAsset("index-abc123.css", config), "initial");
  assert.equal(classifyAsset("SettingsPage-xyz.js", config), "deferred");
  assert.equal(classifyAsset("zh-TW-abc.js", config), "deferred");
  assert.equal(classifyAsset("favicon.ico", config), "ignore");
});

test("matchAny uses full-string regex patterns", () => {
  assert.equal(matchAny("index-abc.js", ["^index-.*\\.js$"]), true);
  assert.equal(matchAny("SettingsPage-abc.js", ["^index-.*\\.js$"]), false);
});

test("summarizeAssets aggregates only initial shell into gated totals", () => {
  const config = loadBudget();
  const summary = summarizeAssets(
    [
      { name: "index-a.js", bytes: Buffer.from("a".repeat(1000)) },
      { name: "react-vendor-b.js", bytes: Buffer.from("b".repeat(2000)) },
      { name: "index-c.css", bytes: Buffer.from("c".repeat(500)) },
      { name: "SettingsPage-d.js", bytes: Buffer.from("d".repeat(8000)) },
    ],
    config,
  );
  assert.equal(summary.initialJs.length, 2);
  assert.equal(summary.initialCss.length, 1);
  assert.equal(summary.deferred.length, 1);
  assert.equal(summary.totals.initialShellJsRawBytes, 3000);
  assert.ok(summary.totals.initialShellJsGzipBytes > 0);
  assert.ok(summary.totals.initialShellJsBrotliBytes > 0);
  assert.ok(summary.totals.initialShellCssGzipBytes > 0);
});

test("checkBudgets reports over-limit keys only", () => {
  const failures = checkBudgets(
    {
      initialShellJsGzipBytes: 400_000,
      initialShellJsRawBytes: 100,
      initialShellCssGzipBytes: 10,
    },
    {
      initialShellJsGzipBytes: 348_160,
      initialShellJsRawBytes: 1_153_434,
      initialShellCssGzipBytes: 18_432,
    },
  );
  assert.deepEqual(failures, [{ key: "initialShellJsGzipBytes", actual: 400_000, limit: 348_160 }]);
});

test("measureFile returns raw gzip and brotli", () => {
  const sizes = measureFile(Buffer.from("hello ".repeat(200)));
  assert.ok(sizes.raw > sizes.gzip);
  assert.ok(sizes.gzip > 0);
  assert.ok(sizes.brotli > 0);
});
