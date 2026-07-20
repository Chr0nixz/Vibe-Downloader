import { describe, expect, it } from "vitest";

import fixturesJson from "../../scripts/fixtures/capture-policy-cases.json";
import {
  analyzeSiteRuleConflicts,
  type CapturePolicySettings,
  type CaptureSiteRule,
  diagnoseCaptureUrl,
  headerForwardingDecision,
  matchingRule,
  ruleCovers,
  shouldIntercept,
} from "./capture-policy";

interface FixtureCase {
  name: string;
  kind: "headerForwarding" | "shouldIntercept" | "matchingRule";
  url?: string;
  hostname?: string;
  download?: { url: string; totalBytes?: number; filename?: string };
  settings?: CapturePolicySettings;
  rules?: CaptureSiteRule[];
  expected: Record<string, unknown>;
}

const fixtures = fixturesJson as FixtureCase[];

describe("capture-policy fixtures", () => {
  for (const fixture of fixtures) {
    it(fixture.name, () => {
      if (fixture.kind === "headerForwarding") {
        const decision = headerForwardingDecision(fixture.url!, fixture.settings!);
        expect(decision.forward).toBe(fixture.expected.forward);
        expect(decision.state).toBe(fixture.expected.state);
        return;
      }
      if (fixture.kind === "shouldIntercept") {
        const decision = shouldIntercept(fixture.download!, fixture.settings!);
        expect(decision.intercept).toBe(fixture.expected.intercept);
        if ("reason" in fixture.expected) {
          expect(decision.reason).toBe(fixture.expected.reason);
        }
        return;
      }
      const matched = matchingRule(fixture.hostname!, fixture.rules!);
      expect(matched?.id ?? null).toBe(fixture.expected.id);
    });
  }
});

describe("capture-policy conflict analysis", () => {
  it("marks a narrower later rule as shadowed by an earlier broad rule", () => {
    const rules: CaptureSiteRule[] = [
      { id: "broad", hostPattern: "example.com", includeSubdomains: true, mode: "auto" },
      { id: "narrow", hostPattern: "cdn.example.com", includeSubdomains: false, mode: "never" },
    ];
    expect(ruleCovers(rules[0], rules[1])).toBe(true);
    expect(analyzeSiteRuleConflicts(rules)).toEqual([{ ruleId: "narrow", kind: "shadowed", byRuleId: "broad" }]);
  });

  it("marks mutual overlap without full coverage", () => {
    const rules: CaptureSiteRule[] = [
      { id: "a", hostPattern: "cdn.example.com", includeSubdomains: true, mode: "auto" },
      { id: "b", hostPattern: "api.example.com", includeSubdomains: true, mode: "never" },
    ];
    expect(analyzeSiteRuleConflicts(rules)).toEqual([]);
  });

  it("diagnoseCaptureUrl returns first-wins matched rule", () => {
    const settings: CapturePolicySettings = {
      autoIntercept: true,
      forwardHeadersMode: "ask",
      minSizeBytes: "0",
      fileExtensions: [],
      siteRules: [
        { id: "early", hostPattern: "example.com", includeSubdomains: true, mode: "never" },
        { id: "late", hostPattern: "cdn.example.com", includeSubdomains: false, mode: "auto" },
      ],
    };
    const diagnosis = diagnoseCaptureUrl("https://cdn.example.com/file.bin", settings, {
      filename: "file.bin",
      totalBytes: 1_000_000,
    });
    expect(diagnosis.matchedRule?.id).toBe("early");
    expect(diagnosis.intercept).toEqual({ intercept: false, reason: "site-rule" });
  });
});
