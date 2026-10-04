import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { NAV_SHORTCUT_DIGITS, NAV_SHORTCUT_KEYS, NAV_SHORTCUT_LABEL_KEYS } from "./nav-shortcuts";
import { ShortcutPanel } from "./ShortcutPanel";

vi.mock("react-i18next", async (importOriginal) => {
  const actual = await importOriginal<typeof import("react-i18next")>();
  return {
    ...actual,
    useTranslation: () => ({
      t: (key: string) => key,
      i18n: { language: "en" },
    }),
  };
});

describe("ShortcutPanel navigation group", () => {
  it("lists Mod+1–7 in digit order with the label of the view each key opens", () => {
    render(<ShortcutPanel open onOpenChange={() => {}} platform="windows" />);

    const heading = screen.getByRole("heading", { name: "shortcuts.groups.navigation" });
    const section = heading.closest("section");
    expect(section).not.toBeNull();

    const rows = within(section as HTMLElement)
      .getAllByText(/^shortcuts\.nav/)
      .map((label) => {
        const row = label.parentElement as HTMLElement;
        const keys = Array.from(row.querySelectorAll("kbd")).map((kbd) => kbd.textContent);
        return { label: label.textContent, keys };
      });

    expect(rows).toEqual(
      NAV_SHORTCUT_DIGITS.map((digit) => ({
        label: NAV_SHORTCUT_LABEL_KEYS[digit],
        keys: ["Ctrl", digit],
      })),
    );
  });

  it("names every label after the view the key handler actually opens", () => {
    for (const digit of NAV_SHORTCUT_DIGITS) {
      const view = NAV_SHORTCUT_KEYS[digit];
      expect(NAV_SHORTCUT_LABEL_KEYS[digit].toLowerCase()).toBe(`shortcuts.nav${view}`);
    }
  });

  it("keeps the digits contiguous from 1 so the palette and panel read top to bottom", () => {
    expect(NAV_SHORTCUT_DIGITS).toEqual(["1", "2", "3", "4", "5", "6"]);
  });
});

describe("ShortcutPanel reference", () => {
  it.each(["windows", "macos"] as const)("lists the paste action on %s", (platform) => {
    render(<ShortcutPanel open onOpenChange={() => {}} platform={platform} />);
    const row = screen.getByText("contextmenu.list.pasteAndCreate").parentElement as HTMLElement;
    expect(Array.from(row.querySelectorAll("kbd")).map((kbd) => kbd.textContent)).toEqual(
      platform === "macos" ? ["\u2318V"] : ["Ctrl", "V"],
    );
  });

  it("lists each action once with every chord that runs it", () => {
    render(<ShortcutPanel open onOpenChange={() => {}} platform="windows" />);

    expect(screen.getAllByText("shortcuts.showShortcuts")).toHaveLength(1);
    expect(screen.getAllByText("shortcuts.toggleDetails")).toHaveLength(1);
    const row = screen.getByText("shortcuts.showShortcuts").parentElement as HTMLElement;
    expect(Array.from(row.querySelectorAll("kbd")).map((kbd) => kbd.textContent)).toEqual(["Ctrl", "/", "?"]);
  });

  it("filters by action or key and can be closed without the keyboard", () => {
    render(<ShortcutPanel open onOpenChange={() => {}} platform="windows" />);

    fireEvent.change(screen.getByRole("textbox", { name: "shortcuts.searchPlaceholder" }), {
      target: { value: "Space" },
    });
    expect(screen.getByText("shortcuts.toggleSelection")).toBeInTheDocument();
    expect(screen.queryByText("shortcuts.commandPalette")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "shortcuts.close" })).toBeInTheDocument();
  });
});
