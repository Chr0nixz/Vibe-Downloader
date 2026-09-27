import { describe, expect, it } from "vitest";

import { NAV_SHORTCUT_KEYS, navFilterForDigit, navShortcutDigit } from "./nav-shortcuts";

describe("nav shortcuts", () => {
  it("maps Mod+1–6 to the visible task navigation items", () => {
    expect(NAV_SHORTCUT_KEYS).toEqual({
      "1": "all",
      "2": "downloading",
      "3": "issues",
      "4": "completed",
      "5": "queue",
      "6": "paused",
    });
    expect(navFilterForDigit("3")).toBe("issues");
    expect(navFilterForDigit("6")).toBe("paused");
    expect(navFilterForDigit("7")).toBeUndefined();
    expect(navShortcutDigit("paused")).toBe("6");
  });

  it("keeps the cause filter's narrower views on the Needs you digit", () => {
    expect(navShortcutDigit("attention")).toBe("3");
    expect(navShortcutDigit("failed")).toBe("3");
  });
});
