#!/usr/bin/env node
/**
 * Automated doc-consistency gate (ARC-18 acceptance).
 *
 * The 2026-08-13 review showed doc drift recurring between manual sweeps:
 * AGENTS.md/README kept listing fixed P0s as active blockers, and the
 * "currently at" version trailed package.json. This script makes those
 * three failure modes fail fast instead:
 *
 *   1. Every audit ID referenced in README/AGENTS exists in the audit doc
 *      (catches typos and fictional IDs).
 *   2. IDs that headline a bullet in a "blockers" section of README/AGENTS
 *      are Closed in the audit doc. Mid-sentence mentions are cross
 *      references (e.g. "fold into ARC-31") and are exempt.
 *   3. AGENTS.md's "currently at X.Y.Z" matches package.json.
 *
 * With --fix-less design: the script never rewrites docs; it exits 1 with a
 * file-anchored message so the doc owner fixes the wording.
 */
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");

// Audit IDs are zero-padded two-digit numbers; single-digit tags such as
// "UX-1" are legacy review tags from an earlier review, not audit IDs.
export const AUDIT_ID_PATTERN = /\b(?:ARC|UX|FUN|PERF|SEC|ENG)-\d{2,3}\b/gu;

/** Parses `### ID（priority，status）：title` headers into `id -> status`. */
export function parseAuditStatuses(auditMarkdown) {
  const statuses = new Map();
  const headerPattern = /^### ([A-Z]+-\d{2,3})（([^，]+)，([^）]+)）：/gmu;
  for (const [, id, , status] of auditMarkdown.matchAll(headerPattern)) {
    statuses.set(id, status.trim());
  }
  return statuses;
}

export function isClosed(status) {
  return typeof status === "string" && status.startsWith("Closed");
}

/**
 * Returns IDs that headline a list bullet anywhere in the document.
 *
 * Blocker sections (AGENTS "Active release blockers", README "当前发布阻断")
 * headline the IDs they claim are blocking, so document-wide bullet
 * detection covers them without depending on heading spelling. A full
 * bullet-headline scan is safe: mid-sentence cross references (e.g. "fold
 * it into `ARC-31`") are never bullet-initial, and the checked-in docs
 * carry no other bullet-headlined audit IDs.
 */
export function bulletHeadlinedIds(markdown) {
  const ids = new Set();
  for (const line of markdown.split(/\r?\n/u)) {
    const bullet = /^\s*[-*]\s*(?:`)?([A-Z]+-\d{2,3})(`)?/u.exec(line);
    if (bullet) ids.add(bullet[1]);
  }
  return [...ids];
}

/** All audit IDs referenced anywhere in the document. */
export function referencedAuditIds(markdown) {
  return [...new Set(markdown.match(AUDIT_ID_PATTERN) ?? [])];
}

/** AGENTS.md's "currently at X.Y.Z" self-description, or null. */
export function currentVersionClaim(agentsMarkdown) {
  const match = /The project is currently at `(\d+\.\d+\.\d+)`/u.exec(agentsMarkdown);
  return match?.[1] ?? null;
}

export function checkDocConsistency({ readme, agents, audit, packageVersion }) {
  const problems = [];
  const statuses = parseAuditStatuses(audit);
  if (statuses.size === 0) {
    problems.push("docs/project-improvement-audit.md: no `### ID（priority，status）：` headers found");
    return problems;
  }

  for (const [name, markdown] of [
    ["README.md", readme],
    ["AGENTS.md", agents],
  ]) {
    for (const id of referencedAuditIds(markdown)) {
      if (!statuses.has(id)) {
        problems.push(`${name}: references ${id}, which does not exist in the audit document`);
      }
    }
    for (const id of bulletHeadlinedIds(markdown)) {
      const status = statuses.get(id);
      if (status !== undefined && !isClosed(status)) {
        problems.push(
          `${name}: blocker section headlines ${id}, whose audit status is "${status}" — update the audit to Closed or rewrite the bullet`,
        );
      }
    }
  }

  const claimed = currentVersionClaim(agents);
  if (claimed === null) {
    problems.push('AGENTS.md: missing "The project is currently at `X.Y.Z`" version claim');
  } else if (claimed !== packageVersion) {
    problems.push(`AGENTS.md: claims version ${claimed} but package.json reports ${packageVersion}`);
  }
  return problems;
}

function main() {
  const readme = readFileSync(resolve(root, "README.md"), "utf8");
  const agents = readFileSync(resolve(root, "AGENTS.md"), "utf8");
  const audit = readFileSync(resolve(root, "docs/project-improvement-audit.md"), "utf8");
  const packageVersion = JSON.parse(readFileSync(resolve(root, "package.json"), "utf8")).version;

  const problems = checkDocConsistency({ readme, agents, audit, packageVersion });
  if (problems.length > 0) {
    console.error("Doc consistency check failed:");
    for (const problem of problems) console.error(`  - ${problem}`);
    process.exit(1);
  }
  console.log(
    `Doc consistency check passed: ${packageVersion} consistent; blocker sections only headline Closed audit IDs; all referenced IDs exist.`,
  );
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  main();
}
