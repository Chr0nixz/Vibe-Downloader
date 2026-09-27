import { afterEach, describe, expect, it } from "vitest";

import i18n from "@/i18n";
import { formatPercent } from "./utils";

describe("formatPercent", () => {
  afterEach(async () => {
    await i18n.changeLanguage("en");
  });

  it("keeps an unfinished value below 100 after locale formatting rounds it", async () => {
    await i18n.changeLanguage("en");

    expect(formatPercent(9_999, 10_000, 99.4)).toBe("99.4%");
    expect(formatPercent(9_999, 10_000)).toBe("100%");
  });
});
