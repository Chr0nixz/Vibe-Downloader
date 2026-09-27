import type { TranslationKey } from "@/i18n";
import type { NavFilter } from "@/stores/task-data-store";

/** Numbered shortcuts follow the visible task navigation order: the four
 * primary entries, then the two under "Other views". */
export const NAV_SHORTCUT_KEYS = {
  "1": "all",
  "2": "downloading",
  "3": "issues",
  "4": "completed",
  "5": "queue",
  "6": "paused",
} as const satisfies Record<string, NavFilter>;

export type NavShortcutDigit = keyof typeof NAV_SHORTCUT_KEYS;

/**
 * Digits in ascending order. The shortcut panel and the palette's Views group
 * both render from this so every list of views reads 1–6 top to bottom and
 * matches the key handler.
 */
export const NAV_SHORTCUT_DIGITS = (Object.keys(NAV_SHORTCUT_KEYS) as NavShortcutDigit[]).sort();

export const NAV_SHORTCUT_LABEL_KEYS = {
  "1": "shortcuts.navAll",
  "2": "shortcuts.navDownloading",
  "3": "shortcuts.navIssues",
  "4": "shortcuts.navCompleted",
  "5": "shortcuts.navQueue",
  "6": "shortcuts.navPaused",
} as const satisfies Record<NavShortcutDigit, TranslationKey>;

const DIGIT_BY_FILTER: Partial<Record<NavFilter, NavShortcutDigit>> = {
  all: "1",
  downloading: "2",
  // The cause filter's narrower views live under the same entry and digit.
  issues: "3",
  attention: "3",
  failed: "3",
  completed: "4",
  queue: "5",
  paused: "6",
};

export function navFilterForDigit(key: string): NavFilter | undefined {
  if (key in NAV_SHORTCUT_KEYS) return NAV_SHORTCUT_KEYS[key as NavShortcutDigit];
  return undefined;
}

export function navShortcutDigit(nav: NavFilter): NavShortcutDigit | undefined {
  return DIGIT_BY_FILTER[nav];
}
