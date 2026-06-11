import { useMemo } from "react";
import { useTranslation } from "react-i18next";

import type { Platform } from "@/lib/platform";
import {
  Dialog,
  DialogBody,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";

interface ShortcutEntry {
  keys: string;
  label: string;
}

interface ShortcutGroup {
  id: string;
  label: string;
  shortcuts: ShortcutEntry[];
}

function modSymbol(platform: Platform): string {
  return platform === "macos" ? "\u2318" : "Ctrl+";
}

function buildGroups(t: (key: string) => string, platform: Platform): ShortcutGroup[] {
  const mod = modSymbol(platform);

  return [
    {
      id: "general",
      label: t("shortcuts.groups.general"),
      shortcuts: [
        { keys: `${mod}K`, label: t("shortcuts.commandPalette") },
        { keys: `${mod}N`, label: t("shortcuts.newDownload") },
        { keys: `${mod},`, label: t("shortcuts.settings") },
        { keys: `${mod}/`, label: t("shortcuts.showShortcuts") },
        { keys: "?", label: t("shortcuts.showShortcuts") },
      ],
    },
    {
      id: "task",
      label: t("shortcuts.groups.task"),
      shortcuts: [
        { keys: `${mod}\u2191`, label: t("shortcuts.selectPrevious") },
        { keys: `${mod}\u2193`, label: t("shortcuts.selectNext") },
        { keys: `${mod}D`, label: t("shortcuts.toggleDetails") },
        { keys: `${mod}O`, label: t("shortcuts.openFolder") },
        { keys: `${mod}\u21B5`, label: t("shortcuts.openFile") },
        { keys: "Del", label: t("shortcuts.deleteTask") },
      ],
    },
    {
      id: "navigation",
      label: t("shortcuts.groups.navigation"),
      shortcuts: [
        { keys: `${mod}1`, label: t("shortcuts.navAll") },
        { keys: `${mod}2`, label: t("shortcuts.navDownloading") },
        { keys: `${mod}3`, label: t("shortcuts.navPaused") },
        { keys: `${mod}4`, label: t("shortcuts.navCompleted") },
        { keys: `${mod}5`, label: t("shortcuts.navFailed") },
      ],
    },
    {
      id: "bulk",
      label: t("shortcuts.groups.bulk"),
      shortcuts: [
        { keys: `${mod}A`, label: t("shortcuts.selectAll") },
        { keys: `${mod}Shift+A`, label: t("shortcuts.clearSelection") },
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
  const groups = useMemo(() => buildGroups(t, platform), [t, platform]);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle>{t("shortcuts.title")}</DialogTitle>
          <DialogDescription className="sr-only">
            {t("shortcuts.description")}
          </DialogDescription>
        </DialogHeader>
        <DialogBody className="space-y-5">
          {groups.map((group) => (
            <section key={group.id}>
              <h3 className="mb-2 text-[11px] font-semibold text-text-muted">
                {group.label}
              </h3>
              <div className="space-y-1">
                {group.shortcuts.map((shortcut) => (
                  <div
                    key={`${shortcut.keys}-${shortcut.label}`}
                    className="flex items-center justify-between rounded-md px-2 py-1.5 transition-colors hover:bg-surface-base"
                  >
                    <span className="text-sm text-text-secondary">
                      {shortcut.label}
                    </span>
                    <ShortcutKeys keys={shortcut.keys} />
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
  const parts = keys.split("+");

  return (
    <span className="flex shrink-0 items-center gap-1">
      {parts.map((part, index) => (
        <span key={index} className="flex items-center gap-1">
          {index > 0 ? (
            <span className="text-[10px] text-text-muted">+</span>
          ) : null}
          <kbd className="inline-flex min-w-[1.5rem] items-center justify-center rounded border border-border-subtle bg-surface-root px-1.5 py-0.5 font-mono text-[11px] text-text-muted shadow-[0_1px_0_oklch(0_0_0/0.08)]">
            {part}
          </kbd>
        </span>
      ))}
    </span>
  );
}
