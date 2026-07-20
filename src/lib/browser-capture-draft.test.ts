import { describe, expect, it } from "vitest";
import type { BrowserCaptureSettings, BrowserSiteRule } from "@/generated/bindings";
import {
  BROWSER_CAPTURE_SAVE_DEBOUNCE_MS,
  captureSettingsEqual,
  mergeBrowserCapturePatch,
  normalizeSiteRule,
  resolveCaptureDraftAfterSave,
  validateSiteRule,
} from "@/lib/browser-capture-draft";

const BASE: BrowserCaptureSettings = {
  experimentalCaptureEnabled: true,
  autoIntercept: true,
  forwardHeaders: false,
  forwardHeadersMode: "ask",
  minSizeBytes: "0",
  fileExtensions: ["mp4"],
  siteRules: [],
  allowIntranetHandoff: false,
};

describe("browser-capture-draft", () => {
  it("uses a debounce window in the 300–800ms band", () => {
    expect(BROWSER_CAPTURE_SAVE_DEBOUNCE_MS).toBeGreaterThanOrEqual(300);
    expect(BROWSER_CAPTURE_SAVE_DEBOUNCE_MS).toBeLessThanOrEqual(800);
  });

  it("preserves ask across merge and equality", () => {
    const next = mergeBrowserCapturePatch(BASE, { minSizeBytes: "1048576" });
    expect(next.forwardHeadersMode).toBe("ask");
    expect(captureSettingsEqual(BASE, { ...BASE })).toBe(true);
    expect(captureSettingsEqual(BASE, next)).toBe(false);
  });

  it("does not let a stale save overwrite a newer draft", () => {
    const submitted = BASE;
    const saved = { ...BASE, minSizeBytes: "1" };
    const newerDraft = { ...BASE, minSizeBytes: "2" };
    expect(resolveCaptureDraftAfterSave(newerDraft, submitted, saved)).toEqual(newerDraft);
    expect(resolveCaptureDraftAfterSave(submitted, submitted, saved)).toEqual(saved);
  });

  it("validates site rules before save", () => {
    const emptyHost: BrowserSiteRule = {
      id: "1",
      hostPattern: "  ",
      includeSubdomains: true,
      mode: "auto",
      minSizeBytes: null,
      fileExtensions: [],
      forwardHeaders: null,
    };
    expect(validateSiteRule(emptyHost)).toBe("settings.siteRuleHostRequired");

    const badExt = normalizeSiteRule({
      ...emptyHost,
      hostPattern: "example.com",
      fileExtensions: ["mp4!"],
    });
    expect(validateSiteRule(badExt)).toBe("settings.siteRuleExtensionInvalid");

    const ok = normalizeSiteRule({
      ...emptyHost,
      hostPattern: "*.example.com",
      fileExtensions: [".MP4", "mkv"],
    });
    expect(validateSiteRule(ok)).toBeNull();
    expect(ok.fileExtensions).toEqual(["mp4", "mkv"]);
  });
});
