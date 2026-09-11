import { afterEach, describe, expect, it } from "vitest";

import i18n from "@/i18n";
import { formatDateTime } from "./format-date";

// Local-time constructors keep the rendered fields stable regardless of the CI
// machine's timezone, because every asserted field is itself timezone-local.
const SAMPLE = new Date(2026, 0, 15, 9, 5, 3);

describe("formatDateTime (FUN-22)", () => {
  afterEach(async () => {
    await i18n.changeLanguage("en");
  });

  it("formats with the active app language, not the system locale", async () => {
    await i18n.changeLanguage("zh-CN");
    const zh = formatDateTime(SAMPLE, "date");
    expect(zh).toMatch(/2026/);
    expect(zh).toMatch(/月/);

    await i18n.changeLanguage("en");
    const en = formatDateTime(SAMPLE, "date");
    expect(en).toContain("Jan");
    expect(en).not.toEqual(zh);
  });

  it("re-resolves after a language change even though formatters are cached", async () => {
    const en = formatDateTime(SAMPLE, "date");
    await i18n.changeLanguage("zh-CN");
    const zh = formatDateTime(SAMPLE, "date");
    expect(zh).not.toEqual(en);
    expect(zh).toMatch(/月/);
  });

  it("returns unparseable string input unchanged and non-strings as an em dash", () => {
    expect(formatDateTime("not-a-date", "date")).toBe("not-a-date");
    expect(formatDateTime(Number.NaN, "time")).toBe("—");
  });

  it("applies the requested style fields", () => {
    expect(formatDateTime(SAMPLE, "time")).toContain("09:05");
    expect(formatDateTime(SAMPLE, "dateTimeSeconds")).toContain("09:05:03");
  });
});
