import { describe, expect, it } from "vitest";

import type { BrowserSiteRule } from "@/generated/bindings";
import { parseSiteRulesImport, serializeSiteRulesExport } from "@/lib/site-rules-io";

const sample: BrowserSiteRule = {
  id: "rule-1",
  hostPattern: "example.com",
  includeSubdomains: true,
  mode: "auto",
  minSizeBytes: null,
  fileExtensions: ["mp4"],
  forwardHeaders: null,
};

describe("site-rules-io", () => {
  it("round-trips export and import", () => {
    const text = serializeSiteRulesExport([sample]);
    const result = parseSiteRulesImport(text);
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.rules).toEqual([sample]);
    }
  });

  it("rejects invalid JSON and non-arrays", () => {
    expect(parseSiteRulesImport("{").ok).toBe(false);
    expect(parseSiteRulesImport("{}").ok).toBe(false);
  });

  it("rejects invalid host without partial import", () => {
    const payload = JSON.stringify([sample, { ...sample, id: "bad", hostPattern: "" }]);
    const result = parseSiteRulesImport(payload);
    expect(result).toEqual({
      ok: false,
      errorKey: "settings.siteRuleHostRequired",
      detail: "1",
    });
  });

  it("assigns fresh ids when duplicates collide", () => {
    const payload = JSON.stringify([sample, { ...sample, hostPattern: "other.com" }]);
    const result = parseSiteRulesImport(payload);
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.rules[0].id).not.toBe(result.rules[1].id);
    }
  });
});
