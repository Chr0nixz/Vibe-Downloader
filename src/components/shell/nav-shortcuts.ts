import type { NavFilter } from "@/stores/task-data-store";

/** Numbered shortcuts follow the four primary sidebar items, not hidden filters. */
export const NAV_SHORTCUT_KEYS = {
  "1": "all",
  "2": "downloading",
  "3": "attention",
  "4": "completed",
} as const satisfies Record<string, NavFilter>;

export type NavShortcutDigit = keyof typeof NAV_SHORTCUT_KEYS;

export const NAV_SHORTCUT_LABEL_KEYS = {
  "1": "shortcuts.navAll",
  "2": "shortcuts.navDownloading",
  "3": "shortcuts.navAttention",
  "4": "shortcuts.navCompleted",
} as const;

const DIGIT_BY_FILTER: Partial<Record<NavFilter, NavShortcutDigit>> = {
  all: "1",
  downloading: "2",
  attention: "3",
  completed: "4",
};

export function navFilterForDigit(key: string): NavFilter | undefined {
  if (key === "1" || key === "2" || key === "3" || key === "4") return NAV_SHORTCUT_KEYS[key];
  return undefined;
}

export function navShortcutDigit(nav: NavFilter): NavShortcutDigit | undefined {
  return DIGIT_BY_FILTER[nav];
}
