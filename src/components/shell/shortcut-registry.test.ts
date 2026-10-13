import { describe, expect, it } from "vitest";
import { matchesAction, SHORTCUTS, shortcutLabels } from "./shortcut-registry";

describe("shortcut registry", () => {
  it("every declared chord matches on Windows and macOS and IDs stay unique", () => {
    expect(new Set(SHORTCUTS.map((entry) => entry.id)).size).toBe(SHORTCUTS.length);
    for (const platform of ["windows", "macos"] as const) {
      for (const action of SHORTCUTS)
        for (const chord of action.chords) {
          const parts = chord.split("+");
          const key = parts[parts.length - 1] === "space" ? " " : (parts[parts.length - 1] ?? "");
          const event = new KeyboardEvent("keydown", {
            key,
            ctrlKey: platform === "windows" && parts.includes("mod"),
            metaKey: platform === "macos" && parts.includes("mod"),
            shiftKey: parts.includes("shift") || key === "?",
            altKey: parts.includes("alt"),
          });
          expect(matchesAction(event, action.id, platform), `${platform} ${chord}`).toBe(true);
        }
    }
  });
  it("rejects extra modifiers, composition and already-consumed events", () => {
    expect(
      matchesAction(
        new KeyboardEvent("keydown", { key: "p", ctrlKey: true, altKey: true }),
        "toggleTransfer",
        "windows",
      ),
    ).toBe(false);
    expect(matchesAction(new KeyboardEvent("keydown", { key: "Delete", ctrlKey: true }), "deleteTask", "windows")).toBe(
      false,
    );
    expect(
      matchesAction(new KeyboardEvent("keydown", { key: "v", ctrlKey: true, isComposing: true }), "paste", "windows"),
    ).toBe(false);
    const event = new KeyboardEvent("keydown", { key: "a", ctrlKey: true, cancelable: true });
    event.preventDefault();
    expect(matchesAction(event, "selectAll", "windows")).toBe(false);
    expect(shortcutLabels("openFile", "macos")).toEqual(["⌘↵"]);
  });
});
