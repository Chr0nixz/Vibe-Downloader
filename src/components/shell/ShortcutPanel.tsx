import type { TFunction } from "i18next";
import { Search, X } from "lucide-react";
import { useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { NAV_SHORTCUT_DIGITS, NAV_SHORTCUT_LABEL_KEYS } from "@/components/shell/nav-shortcuts";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogBody,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import type { Platform } from "@/lib/platform";

interface ShortcutEntry {
  /** Every chord that runs this action; listed once, not once per chord. */
  keys: string[];
  label: string;
}

interface ShortcutGroup {
  id: string;
  label: string;
  shortcuts: ShortcutEntry[];
}

function modSymbol(platform: Platform): string {
  return platform === "macos" ? "⌘" : "Ctrl+";
}

function buildGroups(t: TFunction, platform: Platform): ShortcutGroup[] {
  const mod = modSymbol(platform);

  return [
    {
      id: "general",
      label: t("shortcuts.groups.general"),
      shortcuts: [
        { keys: [`${mod}K`], label: t("shortcuts.commandPalette") },
        { keys: [`${mod}N`], label: t("shortcuts.newDownload") },
        { keys: [`${mod}F`], label: t("shortcuts.search") },
        { keys: [`${mod},`], label: t("shortcuts.settings") },
        { keys: [`${mod}/`, "?"], label: t("shortcuts.showShortcuts") },
      ],
    },
    {
      id: "task",
      label: t("shortcuts.groups.task"),
      shortcuts: [
        { keys: ["↑", "↓"], label: t("shortcuts.moveFocus") },
        { keys: ["Enter"], label: t("shortcuts.openDetails") },
        { keys: [`${mod}D`], label: t("shortcuts.toggleDetails") },
        { keys: [`${mod}P`], label: t("shortcuts.toggleTransfer") },
        { keys: [`${mod}R`], label: t("shortcuts.recoverTask") },
        { keys: [`${mod}O`], label: t("shortcuts.openFolder") },
        { keys: [`${mod}↵`], label: t("shortcuts.openFile") },
        { keys: ["Del"], label: t("shortcuts.deleteTask") },
        { keys: ["Shift+Del"], label: t("deleteDialog.deleteFilesToo") },
        { keys: ["Alt+↑", "Alt+↓"], label: t("shortcuts.reorderQueue") },
      ],
    },
    {
      id: "navigation",
      label: t("shortcuts.groups.navigation"),
      // Derived from the same table the key handler reads; a hand-written
      // list here once drifted and sent Mod+4 to Queue while promising Completed.
      shortcuts: NAV_SHORTCUT_DIGITS.map((digit) => ({
        keys: [`${mod}${digit}`],
        label: t(NAV_SHORTCUT_LABEL_KEYS[digit]),
      })),
    },
    {
      id: "bulk",
      label: t("shortcuts.groups.bulk"),
      shortcuts: [
        { keys: ["Shift+↑", "Shift+↓"], label: t("shortcuts.extendSelection") },
        { keys: [`${mod}Space`], label: t("shortcuts.toggleSelection") },
        { keys: [`${mod}A`], label: t("shortcuts.selectAll") },
        { keys: [`${mod}Shift+A`], label: t("shortcuts.clearSelection") },
        { keys: [`${mod}Shift+P`], label: t("taskList.pauseAll") },
        { keys: [`${mod}Shift+R`], label: t("taskList.resumeAll") },
      ],
    },
  ];
}

export function ShortcutPanel({
  open,
  onOpenChange,
  platform,
}: {
  open: boolean;
  onOpenChange: (v: boolean) => void;
  platform: Platform;
}) {
  const { t } = useTranslation();
  const [query, setQuery] = useState("");
  const searchRef = useRef<HTMLInputElement>(null);
  const groups = useMemo(() => buildGroups(t, platform), [t, platform]);
  // Filter by action name or by key ("R", "Shift"), so "what does Ctrl+R do"
  // and "how do I pause" are both one word away.
  const visibleGroups = useMemo(() => {
    const needle = query.trim().toLowerCase();
    if (!needle) return groups;
    return groups
      .map((group) => ({
        ...group,
        shortcuts: group.shortcuts.filter(
          (shortcut) =>
            shortcut.label.toLowerCase().includes(needle) ||
            shortcut.keys.some((keys) => keys.toLowerCase().includes(needle)),
        ),
      }))
      .filter((group) => group.shortcuts.length > 0);
  }, [groups, query]);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        className="max-w-lg"
        // Open on the filter field, not the close button: the panel is opened
        // to look something up.
        onOpenAutoFocus={(event) => {
          event.preventDefault();
          searchRef.current?.focus();
        }}
      >
        <DialogHeader className="flex items-center gap-2">
          <DialogTitle className="min-w-0 flex-1">{t("shortcuts.title")}</DialogTitle>
          <DialogDescription className="sr-only">{t("shortcuts.description")}</DialogDescription>
          <DialogClose asChild>
            <Button
              type="button"
              variant="ghost"
              size="icon"
              className="h-8 w-8 shrink-0"
              aria-label={t("shortcuts.close")}
            >
              <X className="h-4 w-4" aria-hidden />
            </Button>
          </DialogClose>
        </DialogHeader>
        <div className="shrink-0 border-b border-border-subtle px-4 py-2">
          <div className="relative">
            <Search className="pointer-events-none absolute left-2 top-1/2 h-4 w-4 -translate-y-1/2 text-text-muted" />
            <Input
              ref={searchRef}
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder={t("shortcuts.searchPlaceholder")}
              aria-label={t("shortcuts.searchPlaceholder")}
              className="h-8 pl-8"
            />
          </div>
        </div>
        <DialogBody className="space-y-5">
          {visibleGroups.length === 0 ? <p className="text-sm text-text-muted">{t("shortcuts.noResults")}</p> : null}
          {visibleGroups.map((group) => (
            <section key={group.id}>
              <h3 className="mb-2 text-xs font-semibold text-text-muted">{group.label}</h3>
              <div className="space-y-1">
                {group.shortcuts.map((shortcut) => (
                  <div
                    key={`${shortcut.keys.join("|")}-${shortcut.label}`}
                    className="flex items-center justify-between gap-3 rounded-md px-2 py-1.5 transition-colors hover:bg-surface-base"
                  >
                    <span className="text-sm text-text-secondary">{shortcut.label}</span>
                    <span className="flex shrink-0 items-center gap-1.5">
                      {shortcut.keys.map((keys, index) => (
                        <span key={keys} className="flex items-center gap-1.5">
                          {/* A rule, not "/": the slash is itself a key here (Ctrl+/). */}
                          {index > 0 ? <span className="h-3.5 w-px bg-border-subtle" aria-hidden /> : null}
                          <ShortcutKeys keys={keys} />
                        </span>
                      ))}
                    </span>
                  </div>
                ))}
              </div>
            </section>
          ))}
        </DialogBody>
      </DialogContent>
    </Dialog>
  );
}

function ShortcutKeys({ keys }: { keys: string }) {
  // "Ctrl+Shift+A" splits on "+"; a lone "+" or "/" key would not, so it stays whole.
  const parts = keys.length > 1 ? keys.split("+") : [keys];

  return (
    <span className="flex shrink-0 items-center gap-1">
      {parts.map((part, index) => (
        <span key={index} className="flex items-center gap-1">
          {index > 0 ? <span className="text-[10px] text-text-muted">+</span> : null}
          <kbd className="inline-flex min-w-[1.5rem] items-center justify-center rounded border border-border-subtle bg-surface-root px-1.5 py-0.5 font-mono text-xs text-text-muted">
            {part}
          </kbd>
        </span>
      ))}
    </span>
  );
}
