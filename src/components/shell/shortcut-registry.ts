import type { TranslationKey } from "@/i18n";
import type { Platform } from "@/lib/platform";
import { NAV_SHORTCUT_DIGITS, NAV_SHORTCUT_LABEL_KEYS } from "./nav-shortcuts";

type Definition = {
  id: string;
  group: "general" | "task" | "bulk" | "navigation";
  label: TranslationKey;
  chords: readonly string[];
};
export const SHORTCUTS = [
  { id: "commandPalette", group: "general", label: "shortcuts.commandPalette", chords: ["mod+k"] },
  { id: "newDownload", group: "general", label: "shortcuts.newDownload", chords: ["mod+n"] },
  { id: "paste", group: "general", label: "contextmenu.list.pasteAndCreate", chords: ["mod+v"] },
  { id: "search", group: "general", label: "shortcuts.search", chords: ["mod+f"] },
  { id: "settings", group: "general", label: "shortcuts.settings", chords: ["mod+,"] },
  { id: "showShortcuts", group: "general", label: "shortcuts.showShortcuts", chords: ["mod+/", "?"] },
  {
    id: "moveFocus",
    group: "task",
    label: "shortcuts.moveFocus",
    chords: ["arrowup", "arrowdown", "mod+arrowup", "mod+arrowdown", "home", "end", "space"],
  },
  { id: "openDetails", group: "task", label: "shortcuts.openDetails", chords: ["enter"] },
  { id: "toggleDetails", group: "task", label: "shortcuts.toggleDetails", chords: ["mod+d"] },
  { id: "toggleTransfer", group: "task", label: "shortcuts.toggleTransfer", chords: ["mod+p"] },
  { id: "recoverTask", group: "task", label: "shortcuts.recoverTask", chords: ["mod+r"] },
  { id: "openFolder", group: "task", label: "shortcuts.openFolder", chords: ["mod+o"] },
  { id: "openFile", group: "task", label: "shortcuts.openFile", chords: ["mod+enter"] },
  { id: "deleteTask", group: "task", label: "shortcuts.deleteTask", chords: ["delete"] },
  { id: "deleteFiles", group: "task", label: "deleteDialog.deleteFilesToo", chords: ["shift+delete"] },
  { id: "reorderQueue", group: "task", label: "shortcuts.reorderQueue", chords: ["alt+arrowup", "alt+arrowdown"] },
  {
    id: "extendSelection",
    group: "bulk",
    label: "shortcuts.extendSelection",
    chords: ["shift+arrowup", "shift+arrowdown"],
  },
  { id: "toggleSelection", group: "bulk", label: "shortcuts.toggleSelection", chords: ["mod+space"] },
  { id: "selectAll", group: "bulk", label: "shortcuts.selectAll", chords: ["mod+a"] },
  { id: "clearSelection", group: "bulk", label: "shortcuts.clearSelection", chords: ["mod+shift+a"] },
  { id: "pauseAll", group: "bulk", label: "taskList.pauseAll", chords: ["mod+shift+p"] },
  { id: "resumeAll", group: "bulk", label: "taskList.resumeAll", chords: ["mod+shift+r"] },
  ...NAV_SHORTCUT_DIGITS.map((digit) => ({
    id: `nav${digit}`,
    group: "navigation" as const,
    label: NAV_SHORTCUT_LABEL_KEYS[digit],
    chords: [`mod+${digit}`],
  })),
] satisfies Definition[];

type KeyEvent = Pick<KeyboardEvent, "key" | "ctrlKey" | "metaKey" | "altKey" | "shiftKey" | "defaultPrevented"> & {
  isComposing?: boolean;
  nativeEvent?: { isComposing?: boolean };
};
export function matchesChord(event: KeyEvent, chord: string, platform: Platform): boolean {
  if (event.isComposing || event.nativeEvent?.isComposing || event.defaultPrevented) return false;
  const parts = chord.split("+");
  const key = parts[parts.length - 1] === "space" ? " " : parts[parts.length - 1];
  // '?' is produced with Shift on common keyboard layouts.
  return (
    event.key.toLowerCase() === key &&
    event.altKey === parts.includes("alt") &&
    (key === "?" || event.shiftKey === parts.includes("shift")) &&
    event.ctrlKey === (parts.includes("mod") && platform !== "macos") &&
    event.metaKey === (parts.includes("mod") && platform === "macos")
  );
}

export function matchesAction(event: KeyEvent, action: string, platform: Platform): boolean {
  return (
    SHORTCUTS.find((entry) => entry.id === action)?.chords.some((chord) => matchesChord(event, chord, platform)) ??
    false
  );
}

export function shortcutLabels(action: string, platform: Platform): string[] {
  return (SHORTCUTS.find((entry) => entry.id === action)?.chords ?? []).map((chord) =>
    chord
      .split("+")
      .map((part) => {
        if (part === "mod") return platform === "macos" ? "⌘" : "Ctrl";
        return (
          (
            {
              arrowup: "↑",
              arrowdown: "↓",
              enter: "↵",
              space: "Space",
              delete: "Del",
              shift: "Shift",
              alt: "Alt",
            } as Record<string, string>
          )[part] ?? part.toUpperCase()
        );
      })
      .join("+")
      .replace("⌘+", "⌘"),
  );
}
