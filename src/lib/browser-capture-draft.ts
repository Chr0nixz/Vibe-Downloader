import type { BrowserCaptureSettings, BrowserSiteRule } from "@/generated/bindings";

/** UX-03: debounce window for browser capture settings IPC saves. */
export const BROWSER_CAPTURE_SAVE_DEBOUNCE_MS = 500;

export function mergeBrowserCapturePatch(
  current: BrowserCaptureSettings,
  patch: Partial<BrowserCaptureSettings>,
): BrowserCaptureSettings {
  return { ...current, ...patch };
}

/** Stable equality for draft-vs-saved comparison (order-sensitive for arrays). */
export function captureSettingsEqual(a: BrowserCaptureSettings, b: BrowserCaptureSettings): boolean {
  return (
    a.experimentalCaptureEnabled === b.experimentalCaptureEnabled &&
    a.autoIntercept === b.autoIntercept &&
    a.forwardHeaders === b.forwardHeaders &&
    a.forwardHeadersMode === b.forwardHeadersMode &&
    a.minSizeBytes === b.minSizeBytes &&
    a.allowIntranetHandoff === b.allowIntranetHandoff &&
    a.fileExtensions.length === b.fileExtensions.length &&
    a.fileExtensions.every((ext, index) => ext === b.fileExtensions[index]) &&
    a.siteRules.length === b.siteRules.length &&
    a.siteRules.every((rule, index) => siteRulesEqual(rule, b.siteRules[index]))
  );
}

function siteRulesEqual(a: BrowserSiteRule, b: BrowserSiteRule): boolean {
  return (
    a.id === b.id &&
    a.hostPattern === b.hostPattern &&
    a.includeSubdomains === b.includeSubdomains &&
    a.mode === b.mode &&
    a.minSizeBytes === b.minSizeBytes &&
    a.forwardHeaders === b.forwardHeaders &&
    a.fileExtensions.length === b.fileExtensions.length &&
    a.fileExtensions.every((ext, index) => ext === b.fileExtensions[index])
  );
}

/**
 * UX-03: apply a completed IPC snapshot without clobbering newer local draft edits.
 * Returns the draft to keep; when the draft still matches the submitted snapshot,
 * adopt the server response as the new baseline draft.
 */
export function resolveCaptureDraftAfterSave(
  draft: BrowserCaptureSettings | null,
  submitted: BrowserCaptureSettings,
  saved: BrowserCaptureSettings,
): BrowserCaptureSettings {
  if (!draft || captureSettingsEqual(draft, submitted)) {
    return saved;
  }
  return draft;
}

/** UX-06: validate a site rule before persisting into the parent draft. */
export function validateSiteRule(rule: BrowserSiteRule): string | null {
  const host = rule.hostPattern.trim();
  if (!host) {
    return "settings.siteRuleHostRequired";
  }
  // Allow hostnames, wildcards, and dotted suffixes used by the extension matcher.
  if (!/^[a-zA-Z0-9.*_-]+$/.test(host)) {
    return "settings.siteRuleHostInvalid";
  }
  for (const ext of rule.fileExtensions) {
    const normalized = ext.trim().replace(/^\./, "");
    if (!normalized || !/^[a-zA-Z0-9]+$/.test(normalized)) {
      return "settings.siteRuleExtensionInvalid";
    }
  }
  return null;
}

export function normalizeSiteRule(rule: BrowserSiteRule): BrowserSiteRule {
  return {
    ...rule,
    hostPattern: rule.hostPattern.trim(),
    fileExtensions: rule.fileExtensions
      .map((ext) => ext.trim().replace(/^\./, "").toLowerCase())
      .filter((ext) => ext.length > 0),
  };
}
