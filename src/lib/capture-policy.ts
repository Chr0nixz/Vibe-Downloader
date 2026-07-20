/**
 * Desktop port of browser/extension-core/src/capture-policy.js.
 * Keep behavior identical; shared fixtures lock both implementations together.
 */

export type CaptureForwardHeadersMode = "ask" | "enabled" | "disabled";
export type CaptureSiteRuleMode = "auto" | "ask" | "never";

export interface CaptureSiteRule {
  id?: string;
  hostPattern: string;
  includeSubdomains: boolean;
  mode: CaptureSiteRuleMode;
  minSizeBytes?: string | null;
  fileExtensions?: string[];
  forwardHeaders?: boolean | null;
}

export interface CapturePolicySettings {
  autoIntercept?: boolean;
  forwardHeadersMode?: CaptureForwardHeadersMode;
  minSizeBytes?: string | number | null;
  fileExtensions?: string[];
  siteRules?: CaptureSiteRule[];
}

export interface CaptureDownloadCandidate {
  url: string;
  totalBytes?: number;
  filename?: string | null;
}

export interface HeaderForwardingDecision {
  forward: boolean;
  state: "allowed" | "denied" | "ask";
  available: boolean;
}

export interface InterceptDecision {
  intercept: boolean;
  reason?: "disabled" | "site-rule" | "ask-rule" | "size" | "extension";
}

export function ruleMatchesHost(hostname: string, rule: CaptureSiteRule): boolean {
  const pattern = String(rule.hostPattern ?? "").toLowerCase();
  const host = hostname.toLowerCase();
  if (!pattern) return false;
  if (pattern.startsWith("*.")) {
    const root = pattern.slice(2);
    return host === root || host.endsWith(`.${root}`);
  }
  return rule.includeSubdomains ? host === pattern || host.endsWith(`.${pattern}`) : host === pattern;
}

export function matchingRule(
  hostname: string,
  rules: CaptureSiteRule[] | null | undefined,
): CaptureSiteRule | undefined {
  return (rules || []).find((rule) => ruleMatchesHost(hostname, rule));
}

export function extensionFromUrl(url: string, filename?: string | null): string {
  const candidate = String(filename || "")
    .split(/[\\/]/)
    .pop();
  const fromName = candidate?.includes(".") ? candidate.split(".").pop() : "";
  if (fromName) return fromName.toLowerCase();
  try {
    const pathname = new URL(url).pathname;
    const leaf = pathname.split("/").pop() || "";
    return leaf.includes(".") ? (leaf.split(".").pop() || "").toLowerCase() : "";
  } catch {
    return "";
  }
}

export function headerForwardingDecision(url: string, settings: CapturePolicySettings): HeaderForwardingDecision {
  const parsed = new URL(url);
  const rule = matchingRule(parsed.hostname, settings.siteRules || []);
  if (typeof rule?.forwardHeaders === "boolean") {
    return {
      forward: rule.forwardHeaders,
      state: rule.forwardHeaders ? "allowed" : "denied",
      available: true,
    };
  }
  if (settings.forwardHeadersMode === "enabled") {
    return { forward: true, state: "allowed", available: true };
  }
  // Passive ask: never forward and never prompt.
  if (settings.forwardHeadersMode === "ask") {
    return { forward: false, state: "ask", available: true };
  }
  return { forward: false, state: "denied", available: false };
}

export function shouldForwardHeaders(url: string, settings: CapturePolicySettings): boolean {
  return headerForwardingDecision(url, settings).forward;
}

export function shouldIntercept(
  download: CaptureDownloadCandidate,
  settings: CapturePolicySettings,
): InterceptDecision {
  if (!settings.autoIntercept) return { intercept: false, reason: "disabled" };
  const url = new URL(download.url);
  const rule = matchingRule(url.hostname, settings.siteRules || []);
  if (rule?.mode === "never") return { intercept: false, reason: "site-rule" };
  // Passive ask: do not auto-capture and do not prompt.
  if (rule?.mode === "ask") return { intercept: false, reason: "ask-rule" };
  const minSize = Number(rule?.minSizeBytes ?? settings.minSizeBytes ?? 0);
  const totalBytes = download.totalBytes ?? 0;
  if (totalBytes > 0 && Number.isFinite(minSize) && totalBytes < minSize) {
    return { intercept: false, reason: "size" };
  }
  const extensions = rule?.fileExtensions?.length ? rule.fileExtensions : settings.fileExtensions;
  if (extensions?.length) {
    const ext = extensionFromUrl(download.url, download.filename);
    if (ext && !extensions.map((value) => value.toLowerCase()).includes(ext)) {
      return { intercept: false, reason: "extension" };
    }
  }
  return { intercept: true };
}

/** Representative hosts used to reason about coverage between two patterns. */
export function sampleHostsForRule(rule: CaptureSiteRule): string[] {
  const pattern = String(rule.hostPattern ?? "")
    .trim()
    .toLowerCase();
  if (!pattern) return [];
  if (pattern.startsWith("*.")) {
    const root = pattern.slice(2);
    if (!root) return [];
    return [root, `a.${root}`, `b.a.${root}`];
  }
  if (rule.includeSubdomains) {
    return [pattern, `sub.${pattern}`, `deep.sub.${pattern}`];
  }
  return [pattern];
}

/** True when every host that matches `inner` also matches `outer`. */
export function ruleCovers(outer: CaptureSiteRule, inner: CaptureSiteRule): boolean {
  const samples = sampleHostsForRule(inner);
  if (samples.length === 0) return false;
  return samples.every((host) => ruleMatchesHost(host, outer));
}

export type SiteRuleConflictKind = "shadowed" | "overlap";

export interface SiteRuleConflict {
  ruleId: string;
  kind: SiteRuleConflictKind;
  byRuleId: string;
}

/**
 * Detect first-wins shadowing and mutual host overlaps.
 * Warnings only — never blocks saving.
 */
export function analyzeSiteRuleConflicts(rules: CaptureSiteRule[]): SiteRuleConflict[] {
  const conflicts: SiteRuleConflict[] = [];
  for (let i = 0; i < rules.length; i++) {
    const later = rules[i];
    const laterId = later.id ?? String(i);
    for (let j = 0; j < i; j++) {
      const earlier = rules[j];
      const earlierId = earlier.id ?? String(j);
      if (ruleCovers(earlier, later)) {
        conflicts.push({ ruleId: laterId, kind: "shadowed", byRuleId: earlierId });
        break;
      }
      const laterSamples = sampleHostsForRule(later);
      const earlierSamples = sampleHostsForRule(earlier);
      const overlaps =
        laterSamples.some((host) => ruleMatchesHost(host, earlier)) ||
        earlierSamples.some((host) => ruleMatchesHost(host, later));
      if (overlaps) {
        conflicts.push({ ruleId: laterId, kind: "overlap", byRuleId: earlierId });
        break;
      }
    }
  }
  return conflicts;
}

export interface CaptureDiagnosis {
  matchedRule: CaptureSiteRule | null;
  intercept: InterceptDecision;
  headers: HeaderForwardingDecision;
  effectiveMinSizeBytes: number;
  effectiveFileExtensions: string[];
}

/** Diagnose how capture settings would treat a candidate download. */
export function diagnoseCaptureUrl(
  url: string,
  settings: CapturePolicySettings,
  options?: { filename?: string | null; totalBytes?: number },
): CaptureDiagnosis {
  const parsed = new URL(url);
  const matchedRule = matchingRule(parsed.hostname, settings.siteRules || []) ?? null;
  const download: CaptureDownloadCandidate = {
    url,
    filename: options?.filename ?? null,
    totalBytes: options?.totalBytes ?? 0,
  };
  const intercept = shouldIntercept(download, settings);
  const headers = headerForwardingDecision(url, settings);
  const effectiveMinSizeBytes = Number(matchedRule?.minSizeBytes ?? settings.minSizeBytes ?? 0);
  const effectiveFileExtensions = matchedRule?.fileExtensions?.length
    ? matchedRule.fileExtensions
    : (settings.fileExtensions ?? []);
  return {
    matchedRule,
    intercept,
    headers,
    effectiveMinSizeBytes: Number.isFinite(effectiveMinSizeBytes) ? effectiveMinSizeBytes : 0,
    effectiveFileExtensions,
  };
}
