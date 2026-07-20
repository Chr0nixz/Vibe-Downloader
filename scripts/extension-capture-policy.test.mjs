import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const policyPath = path.join(root, "browser", "extension-core", "src", "capture-policy.js");
const fixturesPath = path.join(root, "scripts", "fixtures", "capture-policy-cases.json");

function loadPolicy() {
  const code = readFileSync(policyPath, "utf8");
  const context = { URL, console };
  vm.createContext(context);
  vm.runInContext(code, context);
  return context.VibeCapturePolicy;
}

const policy = loadPolicy();
const fixtures = JSON.parse(readFileSync(fixturesPath, "utf8"));

for (const fixture of fixtures) {
  test(fixture.name, () => {
    if (fixture.kind === "headerForwarding") {
      const decision = policy.headerForwardingDecision(fixture.url, fixture.settings);
      assert.equal(decision.forward, fixture.expected.forward);
      assert.equal(decision.state, fixture.expected.state);
      return;
    }
    if (fixture.kind === "shouldIntercept") {
      const decision = policy.shouldIntercept(fixture.download, fixture.settings);
      assert.equal(decision.intercept, fixture.expected.intercept);
      if ("reason" in fixture.expected) {
        assert.equal(decision.reason, fixture.expected.reason);
      }
      return;
    }
    if (fixture.kind === "matchingRule") {
      const matched = policy.matchingRule(fixture.hostname, fixture.rules);
      assert.equal(matched?.id ?? null, fixture.expected.id);
      return;
    }
    assert.fail(`unknown fixture kind: ${fixture.kind}`);
  });
}
