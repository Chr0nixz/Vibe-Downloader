import { describe, expect, it } from "vitest";

import { NAV_SHORTCUT_KEYS, navFilterForDigit, navShortcutDigit } from "./nav-shortcuts";

describe("nav shortcuts", () => {
  it("maps Mod+1–4 to the visible primary sidebar items", () => {
    expect(NAV_SHORTCUT_KEYS).toEqual({
      "1": "all",
      "2": "downloading",
      "3": "attention",
      "4": "completed",
    });
    expect(navFilterForDigit("3")).toBe("attention");
    expect(navFilterForDigit("5")).toBeUndefined();
    expect(navShortcutDigit("paused")).toBeUndefined();
    expect(navShortcutDigit("failed")).toBeUndefined();
  });
});
