import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

import {
  bulletHeadlinedIds,
  checkDocConsistency,
  currentVersionClaim,
  parseAuditStatuses,
  referencedAuditIds,
} from "./check-doc-consistency.mjs";

const SAMPLE_AUDIT = `# Audit

### ARC-31（P2，Partial）：超大模块与跨引擎重复代码
body
### ARC-19（P0，Closed；协调器排空残留见下）：FTP/SFTP 取消时中止未落盘的 worker
body
### SEC-11（P1，Closed）：任务 Basic-auth 与浏览器转发 Cookie 无源绑定
body
`;

test("audit statuses parse from the header convention, including suffixed Closed", () => {
  const statuses = parseAuditStatuses(SAMPLE_AUDIT);
  assert.equal(statuses.get("ARC-31"), "Partial");
  assert.equal(statuses.get("ARC-19"), "Closed；协调器排空残留见下");
  assert.equal(statuses.get("SEC-11"), "Closed");
});

test("bullet-headlined IDs are detected document-wide; mid-sentence mentions are exempt", () => {
  const doc = [
    "## Active release blockers:",
    "",
    "- `ARC-19` (residual, P2): fold the drain into the coordinator merge.",
    "  - `ARC-33` nested bullet headline",
    "Cross-reference mid-sentence: fold it into the `ARC-31` coordinator merge.",
    "",
    "## Two cross-cutting root causes",
    "",
    "- no ID at this bullet start, and `FUN-20` stays mid-sentence",
  ].join("\n");
  assert.deepEqual(bulletHeadlinedIds(doc).sort(), ["ARC-19", "ARC-33"]);
});

test("checkDocConsistency passes the checked-in documents", async () => {
  const [readme, agents, audit, packageJson] = await Promise.all([
    readFile(new URL("../README.md", import.meta.url), "utf8"),
    readFile(new URL("../AGENTS.md", import.meta.url), "utf8"),
    readFile(new URL("../docs/project-improvement-audit.md", import.meta.url), "utf8"),
    readFile(new URL("../package.json", import.meta.url), "utf8"),
  ]);
  const problems = checkDocConsistency({
    readme,
    agents,
    audit,
    packageVersion: JSON.parse(packageJson).version,
  });
  assert.deepEqual(problems, []);
});

test("checkDocConsistency catches a stale blocker headline and a fictional ID", () => {
  const agents = [
    "The project is currently at `0.5.0`.",
    "Active release blockers:",
    "",
    "- `SEC-99` (P0): not actually in the audit.",
    "- `ARC-31` (P2): Partial, must not headline a blocker.",
    "",
    "## Something else",
  ].join("\n");
  const problems = checkDocConsistency({
    readme: "",
    agents,
    audit: SAMPLE_AUDIT,
    packageVersion: "0.5.0",
  });
  assert.deepEqual(
    problems.sort(),
    [
      'AGENTS.md: blocker section headlines ARC-31, whose audit status is "Partial" — update the audit to Closed or rewrite the bullet',
      "AGENTS.md: references SEC-99, which does not exist in the audit document",
    ].sort(),
  );
});

test("checkDocConsistency catches a stale AGENTS version claim", () => {
  const problems = checkDocConsistency({
    readme: "",
    agents: "The project is currently at `0.4.0`.",
    audit: SAMPLE_AUDIT,
    packageVersion: "0.5.0",
  });
  assert.deepEqual(problems, ["AGENTS.md: claims version 0.4.0 but package.json reports 0.5.0"]);
});

test("referencedAuditIds deduplicates and ignores non-audit tokens", () => {
  const doc = "`ARC-19` then `ARC-19` again, `FUN-20`, and R-2.4 or S-1.1 are not audit IDs";
  assert.deepEqual(referencedAuditIds(doc).sort(), ["ARC-19", "FUN-20"]);
});

test("currentVersionClaim extracts the version", () => {
  assert.equal(currentVersionClaim("text The project is currently at `1.2.3`. end"), "1.2.3");
  assert.equal(currentVersionClaim("no claim here"), null);
});
