import type { BrowserSiteRule, BrowserSiteRuleMode } from "@/generated/bindings";
import { normalizeSiteRule, validateSiteRule } from "@/lib/browser-capture-draft";

const MODES = new Set<BrowserSiteRuleMode>(["auto", "ask", "never"]);

export type SiteRulesImportResult =
  | { ok: true; rules: BrowserSiteRule[] }
  | { ok: false; errorKey: string; detail?: string };

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function parseMode(value: unknown): BrowserSiteRuleMode | null {
  return typeof value === "string" && MODES.has(value as BrowserSiteRuleMode) ? (value as BrowserSiteRuleMode) : null;
}

function parseRule(raw: unknown, index: number): { rule: BrowserSiteRule } | { errorKey: string; detail: string } {
  if (!isObject(raw)) {
    return { errorKey: "settings.siteRulesImportInvalidRule", detail: String(index) };
  }
  const mode = parseMode(raw.mode);
  if (!mode) {
    return { errorKey: "settings.siteRulesImportInvalidMode", detail: String(index) };
  }
  if (typeof raw.hostPattern !== "string") {
    return { errorKey: "settings.siteRulesImportInvalidRule", detail: String(index) };
  }
  const fileExtensions = Array.isArray(raw.fileExtensions)
    ? raw.fileExtensions.filter((ext): ext is string => typeof ext === "string")
    : [];
  const minSizeBytes =
    raw.minSizeBytes === null || raw.minSizeBytes === undefined
      ? null
      : typeof raw.minSizeBytes === "string"
        ? raw.minSizeBytes
        : typeof raw.minSizeBytes === "number" && Number.isFinite(raw.minSizeBytes)
          ? String(Math.trunc(raw.minSizeBytes))
          : null;
  if (raw.minSizeBytes !== null && raw.minSizeBytes !== undefined && minSizeBytes === null) {
    return { errorKey: "settings.siteRulesImportInvalidRule", detail: String(index) };
  }
  const forwardHeaders =
    raw.forwardHeaders === null || raw.forwardHeaders === undefined
      ? null
      : typeof raw.forwardHeaders === "boolean"
        ? raw.forwardHeaders
        : null;
  if (raw.forwardHeaders !== null && raw.forwardHeaders !== undefined && forwardHeaders === null) {
    return { errorKey: "settings.siteRulesImportInvalidRule", detail: String(index) };
  }

  const rule: BrowserSiteRule = {
    id: typeof raw.id === "string" && raw.id.trim() ? raw.id : crypto.randomUUID(),
    hostPattern: raw.hostPattern,
    includeSubdomains: Boolean(raw.includeSubdomains),
    mode,
    minSizeBytes,
    fileExtensions,
    forwardHeaders,
  };
  const normalized = normalizeSiteRule(rule);
  const errorKey = validateSiteRule(normalized);
  if (errorKey) {
    return { errorKey, detail: String(index) };
  }
  return { rule: normalized };
}

/** Parse a JSON array of BrowserSiteRule. Rejects the whole payload on any invalid row. */
export function parseSiteRulesImport(text: string): SiteRulesImportResult {
  let parsed: unknown;
  try {
    parsed = JSON.parse(text);
  } catch {
    return { ok: false, errorKey: "settings.siteRulesImportInvalidJson" };
  }
  if (!Array.isArray(parsed)) {
    return { ok: false, errorKey: "settings.siteRulesImportNotArray" };
  }
  const rules: BrowserSiteRule[] = [];
  const seenIds = new Set<string>();
  for (let index = 0; index < parsed.length; index++) {
    const result = parseRule(parsed[index], index);
    if ("errorKey" in result) {
      return { ok: false, errorKey: result.errorKey, detail: result.detail };
    }
    let rule = result.rule;
    if (seenIds.has(rule.id)) {
      rule = { ...rule, id: crypto.randomUUID() };
    }
    seenIds.add(rule.id);
    rules.push(rule);
  }
  return { ok: true, rules };
}

export function serializeSiteRulesExport(rules: BrowserSiteRule[]): string {
  return `${JSON.stringify(rules, null, 2)}\n`;
}
