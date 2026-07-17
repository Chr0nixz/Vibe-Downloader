import { Info, Keyboard, RefreshCw, X } from "lucide-react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { MenuItem, MenuSeparator, RegionContextMenu } from "@/components/ui/menu-item";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { useAppUpdater } from "@/hooks/use-app-updater";
import type { Platform } from "@/lib/platform";
import { cn, formatShortcut, formatSpeed } from "@/lib/utils";
import { useSettingsStore } from "@/stores/settings-store";
import { useTaskDataStore } from "@/stores/task-store";

export function StatusBar({
  className,
  platform = "unknown",
  onOpenShortcuts,
  onOpenAbout,
}: {
  className?: string;
  platform?: Platform;
  onOpenShortcuts?: () => void;
  onOpenAbout?: () => void;
}) {
  const { t } = useTranslation();
  // Combined selector: when globalTaskStats is non-null (backend snapshot),
  // it returns that stable ref and skips re-renders on progress ticks.
  // When null, returns taskStats — which now benefits from the zero-delta
  // fast path in patchTasksBatch (same ref when aggregate stats unchanged).
  const stats = useTaskDataStore((s) => s.globalTaskStats ?? s.taskStats);
  const settings = useSettingsStore((s) => s.settings);
  const { updateVersion, installing, error, installUpdate, dismissUpdate, checkForUpdate } = useAppUpdater();
  const speedLimit = Number(settings?.globalSpeedLimitBps ?? 0);

  return (
    <RegionContextMenu
      items={
        <>
          {onOpenShortcuts && <MenuItem icon={Keyboard} label={t("statusBar.shortcuts")} onSelect={onOpenShortcuts} />}
          <MenuItem
            icon={RefreshCw}
            label={t("statusBar.checkForUpdate")}
            disabled={installing}
            onSelect={() => void checkForUpdate()}
          />
          {onOpenAbout && (
            <>
              <MenuSeparator />
              <MenuItem icon={Info} label={t("nav.about")} onSelect={onOpenAbout} />
            </>
          )}
        </>
      }
    >
      <footer
        className={cn(
          "order-2 flex h-8 shrink-0 items-center justify-between gap-2 border-t border-border-subtle bg-surface-base px-2 text-[11px] sm:px-3 md:order-none md:px-4 md:text-xs",
          className,
        )}
        role="contentinfo"
      >
        <span className="flex min-w-0 items-center gap-1.5">
          <span className="text-text-muted">{t("statusBar.total")}</span>
          <span
            className={cn(
              "font-mono font-semibold tabular-nums",
              stats.totalSpeed > 0 ? "text-xs text-accent-primary md:text-sm" : "text-text-secondary",
            )}
          >
            {formatSpeed(stats.totalSpeed)}
          </span>
        </span>
        <span aria-live="polite" aria-atomic="true" className="flex shrink-0 items-center gap-3 text-text-muted">
          {/* P1c: replaced the "·" text separator (was text-border-subtle at ~1.4:1
              contrast, failing WCAG 1.4.11) with gap-based grouping. Each label+number
              pair is a discrete visual unit; gap-3 between groups provides separation
              without relying on a low-contrast glyph. */}
          <span className="flex items-center gap-1.5">
            <span>{t("statusBar.active")}</span>
            <span
              className={cn(
                "font-mono font-bold tabular-nums",
                stats.active > 0 ? "text-accent-primary" : "text-text-secondary",
              )}
            >
              {stats.active}
            </span>
          </span>
          <span className="flex items-center gap-1.5">
            <span>{t("statusBar.queued")}</span>
            <span
              className={cn(
                "font-mono font-bold tabular-nums",
                stats.queued > 0 ? "text-text-primary" : "text-text-secondary",
              )}
            >
              {stats.queued}
            </span>
          </span>
        </span>
        <span className="flex min-w-0 items-center justify-end gap-2">
          {updateVersion ? (
            <span className="hidden items-center gap-2 sm:flex">
              <span className="truncate text-accent-primary">
                {t("statusBar.updateAvailable", { version: updateVersion })}
              </span>
              <Button
                type="button"
                variant="outline"
                size="sm"
                className="h-8 shrink-0 px-2 text-xs"
                disabled={installing}
                onClick={() => void installUpdate()}
              >
                {installing ? t("statusBar.updating") : t("statusBar.installUpdate")}
              </Button>
              {!installing ? (
                <Tooltip>
                  <TooltipTrigger asChild>
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon"
                      className="h-8 w-8 shrink-0 text-text-muted hover:text-text-primary"
                      aria-label={t("settings.dismissUpdate")}
                      onClick={dismissUpdate}
                    >
                      <X className="h-3.5 w-3.5" />
                    </Button>
                  </TooltipTrigger>
                  <TooltipContent>{t("settings.dismissUpdate")}</TooltipContent>
                </Tooltip>
              ) : null}
            </span>
          ) : error ? (
            <span className="truncate text-status-danger" title={error}>
              {t("statusBar.updateFailed")}
            </span>
          ) : (
            <span className="hidden text-text-muted md:inline">
              {speedLimit > 0
                ? t("statusBar.globalSpeedLimit", { speed: formatSpeed(speedLimit) })
                : t("statusBar.noGlobalSpeedLimit")}
            </span>
          )}
          {onOpenShortcuts ? (
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  className="h-8 w-8 shrink-0 text-text-muted hover:text-text-primary"
                  aria-label={t("statusBar.shortcuts")}
                  onClick={onOpenShortcuts}
                >
                  <Keyboard className="h-3.5 w-3.5" />
                </Button>
              </TooltipTrigger>
              <TooltipContent>
                {t("statusBar.shortcuts")}&ensp;
                <kbd className="rounded border border-border-subtle bg-surface-root px-1 py-0.5 font-mono text-[10px]">
                  {formatShortcut("mod+/", platform)}
                </kbd>
              </TooltipContent>
            </Tooltip>
          ) : null}
        </span>
      </footer>
    </RegionContextMenu>
  );
}
