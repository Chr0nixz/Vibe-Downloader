//! Shared capture / header-forwarding decision helpers for the extension.
//! Loaded via importScripts in the service worker and via vm in Node tests.
//!
//! FUN-14: mode `ask` is intentionally passive — it does not show a prompt.
//! It means "do not forward / do not auto-intercept" without confirmation UI.

function headerForwardingDecision(url, settings) {
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

function shouldForwardHeaders(url, settings) {
  return headerForwardingDecision(url, settings).forward;
}

function shouldIntercept(download, settings) {
  if (!settings.autoIntercept) return { intercept: false, reason: "disabled" };
  const url = new URL(download.url);
  const rule = matchingRule(url.hostname, settings.siteRules || []);
  if (rule?.mode === "never") return { intercept: false, reason: "site-rule" };
  // Passive ask: do not auto-capture and do not prompt.
  if (rule?.mode === "ask") return { intercept: false, reason: "ask-rule" };
  const minSize = Number(rule?.minSizeBytes ?? settings.minSizeBytes ?? 0);
  if (download.totalBytes > 0 && Number.isFinite(minSize) && download.totalBytes < minSize) {
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

function matchingRule(hostname, rules) {
  return (rules || []).find((rule) => {
    const pattern = String(rule.hostPattern ?? "").toLowerCase();
    const host = hostname.toLowerCase();
    if (!pattern) return false;
    if (pattern.startsWith("*.")) {
      const root = pattern.slice(2);
      return host === root || host.endsWith(`.${root}`);
    }
    return rule.includeSubdomains ? host === pattern || host.endsWith(`.${pattern}`) : host === pattern;
  });
}

function extensionFromUrl(url, filename) {
  const candidate = String(filename || "")
    .split(/[\\/]/)
    .pop();
  const fromName = candidate?.includes(".") ? candidate.split(".").pop() : "";
  if (fromName) return fromName.toLowerCase();
  try {
    const pathname = new URL(url).pathname;
    const leaf = pathname.split("/").pop() || "";
    return leaf.includes(".") ? leaf.split(".").pop().toLowerCase() : "";
  } catch {
    return "";
  }
}

// Service worker / importScripts global surface.
// biome-ignore lint/correctness/noUnusedVariables: exposed via importScripts for background.js and tests
var VibeCapturePolicy = {
  headerForwardingDecision,
  shouldForwardHeaders,
  shouldIntercept,
  matchingRule,
  extensionFromUrl,
};
