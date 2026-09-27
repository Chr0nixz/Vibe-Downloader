import {
  AlertTriangle,
  ArrowDownToLine,
  CheckCircle2,
  CircleX,
  Clock,
  FilePenLine,
  Info,
  Keyboard,
  ListChecks,
  LoaderCircle,
  type LucideIcon,
  Pause,
  PauseCircle,
  Play,
  RefreshCw,
  TriangleAlert,
  WifiOff,
  X,
} from "lucide-react";
import { type ReactNode, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { SpeedLimitControl } from "@/components/shell/SpeedLimitControl";
import { Button } from "@/components/ui/button";
import { MenuItem, MenuSeparator, RegionContextMenu } from "@/components/ui/menu-item";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { useAppUpdater } from "@/hooks/use-app-updater";
import type { Platform } from "@/lib/platform";
import { cn, formatShortcut, formatSpeed } from "@/lib/utils";
import type { NavFilter } from "@/stores/task-data-store";
import { useTaskDataStore, useTaskUIStore } from "@/stores/task-store";

/** Marks where the live speed goes inside a translated sentence. */
const SPEED_SLOT = "\u2063";

export function StatusBar({
  className,
  platform = "unknown",
  onOpenShortcuts,
  onOpenAbout,
  newDownloadState = null,
  onOpenNewDownload,
  onPauseAll,
  onResumeAll,
}: {
  className?: string;
  platform?: Platform;
  onOpenShortcuts?: () => void;
  onOpenAbout?: () => void;
  newDownloadState?: "draft" | "creating" | null;
  onOpenNewDownload?: () => void;
  onPauseAll?: () => void;
  onResumeAll?: () => void;
}) {
  const { t } = useTranslation();
  // Combined selector: when globalTaskStats is non-null (backend snapshot),
  // it returns that stable ref and skips re-renders on progress ticks.
  // When null, returns taskStats — which now benefits from the zero-delta
  // fast path in patchTasksBatch (same ref when aggregate stats unchanged).
  const stats = useTaskDataStore((s) => s.globalTaskStats ?? s.taskStats);
  const { updateVersion, installing, error, installUpdate, dismissUpdate, checkForUpdate } = useAppUpdater();
  const [speedPanelOpen, setSpeedPanelOpen] = useState(false);
  const [summaryOpen, setSummaryOpen] = useState(false);
  const speedPanelRequest = useTaskUIStore((s) => s.speedLimitPanelRequest);
  const setNav = useTaskUIStore((s) => s.setNav);

  // The palette owns no popover of its own for a custom limit, so it asks the
  // status bar to open the panel. Zero means "never requested" — not a bump.
  useEffect(() => {
    if (speedPanelRequest === 0) return;
    setSpeedPanelOpen(true);
  }, [speedPanelRequest]);

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
          "order-2 flex h-8 shrink-0 flex-nowrap items-center justify-between gap-2 border-t border-border-subtle bg-surface-base px-2 text-[11px] sm:px-3 md:order-none md:px-4 md:text-xs",
          className,
        )}
        role="contentinfo"
      >
        {/* One health sentence instead of a strip of counters: what is moving,
            then what needs the user. Queued, waiting, paused, and completed
            counts live one click away in the summary, so the bar never has
            more to say than a snapped window can fit. */}
        <span className="flex min-w-0 flex-1 items-center gap-1.5">
          <ActivitySentence active={stats.active} queued={stats.queued} speed={stats.totalSpeed} />
          {/* The bar is the one surface that is always on screen, so it is
              where a stuck task has to surface: "No active downloads" alone
              read as all-clear while downloads sat waiting on the user. */}
          {stats.attention > 0 ? (
            <HealthChip
              tone="warning"
              icon={TriangleAlert}
              count={stats.attention}
              label={t("statusBar.attentionCount", { count: stats.attention })}
              hint={t("statusBar.showAttention")}
              onClick={() => setNav("attention")}
            />
          ) : null}
          {stats.failed > 0 ? (
            <HealthChip
              tone="danger"
              icon={CircleX}
              count={stats.failed}
              label={t("statusBar.failedCount", { count: stats.failed })}
              hint={t("statusBar.showFailed")}
              onClick={() => setNav("failed")}
            />
          ) : null}
          <TaskSummary
            open={summaryOpen}
            onOpenChange={setSummaryOpen}
            rows={[
              { nav: "downloading", label: t("nav.downloading"), count: stats.active, icon: ArrowDownToLine },
              { nav: "queue", label: t("nav.queue"), count: stats.queued, icon: Clock },
              { nav: null, label: t("task.status.waiting_network"), count: stats.waitingNetwork, icon: WifiOff },
              { nav: "paused", label: t("nav.paused"), count: stats.paused, icon: PauseCircle },
              { nav: "completed", label: t("nav.completed"), count: stats.completed, icon: CheckCircle2 },
            ]}
            onSelect={(next) => {
              setNav(next);
              setSummaryOpen(false);
            }}
          />
          {newDownloadState ? (
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  className="h-8 min-w-0 shrink-0 gap-1 px-1.5 text-text-secondary hover:text-text-primary"
                  aria-label={t(
                    newDownloadState === "creating" ? "newDownload.closeSubmittingTitle" : "newDownload.draftKeptTitle",
                  )}
                  onClick={onOpenNewDownload}
                >
                  {newDownloadState === "creating" ? (
                    <LoaderCircle className="h-3.5 w-3.5 animate-spin text-accent-primary" aria-hidden />
                  ) : (
                    <FilePenLine className="h-3.5 w-3.5 text-accent-primary" aria-hidden />
                  )}
                  <span className="hidden truncate lg:inline">
                    {t(
                      newDownloadState === "creating"
                        ? "newDownload.closeSubmittingTitle"
                        : "newDownload.draftKeptTitle",
                    )}
                  </span>
                </Button>
              </TooltipTrigger>
              <TooltipContent>
                {t(
                  newDownloadState === "creating"
                    ? "newDownload.closeSubmittingDescription"
                    : "newDownload.draftKeptDescription",
                )}
              </TooltipContent>
            </Tooltip>
          ) : null}
        </span>
        <span className="flex shrink-0 items-center justify-end gap-1 sm:gap-2">
          {updateVersion ? (
            <>
              {/* Full banner from `sm` up; below `sm` the text would overflow the
                  480px bar, so the narrow tier gets icon-only controls instead of
                  losing update visibility entirely. */}
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
                        <X className="h-3.5 w-3.5" aria-hidden />
                      </Button>
                    </TooltipTrigger>
                    <TooltipContent>{t("settings.dismissUpdate")}</TooltipContent>
                  </Tooltip>
                ) : null}
              </span>
              <span className="flex items-center gap-1 sm:hidden">
                <Tooltip>
                  <TooltipTrigger asChild>
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon"
                      className="h-8 w-8 shrink-0 text-accent-primary hover:text-accent-primary"
                      aria-label={t("statusBar.updateAvailable", { version: updateVersion })}
                      disabled={installing}
                      onClick={() => void installUpdate()}
                    >
                      <ArrowDownToLine className="h-3.5 w-3.5" aria-hidden />
                    </Button>
                  </TooltipTrigger>
                  <TooltipContent>{t("statusBar.updateAvailable", { version: updateVersion })}</TooltipContent>
                </Tooltip>
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
                        <X className="h-3.5 w-3.5" aria-hidden />
                      </Button>
                    </TooltipTrigger>
                    <TooltipContent>{t("settings.dismissUpdate")}</TooltipContent>
                  </Tooltip>
                ) : null}
              </span>
            </>
          ) : error ? (
            <>
              <span className="hidden truncate text-status-danger sm:inline" title={error}>
                {t("statusBar.updateFailed")}
              </span>
              {/* Narrow-tier text would overflow; a labelled icon keeps the
                  failure state announced instead of silently hidden. */}
              <span role="img" aria-label={t("statusBar.updateFailed")} title={error} className="shrink-0 sm:hidden">
                <AlertTriangle className="h-3.5 w-3.5 text-status-danger" aria-hidden />
              </span>
            </>
          ) : null}
          {/* Rendered unconditionally: an update banner used to occupy this slot
              exclusively, which hid the only narrow-tier path to the speed cap. */}
          {onPauseAll ? (
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  className="h-8 w-8 shrink-0"
                  aria-label={t("taskList.pauseAll")}
                  disabled={stats.active + stats.queued === 0}
                  onClick={onPauseAll}
                >
                  <Pause className="h-3.5 w-3.5" aria-hidden />
                </Button>
              </TooltipTrigger>
              <TooltipContent>{t("taskList.pauseAll")}</TooltipContent>
            </Tooltip>
          ) : null}
          {onResumeAll ? (
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  className="h-8 w-8 shrink-0"
                  aria-label={t("taskList.resumeAll")}
                  // Matches what the backend resumes (paused and waiting for
                  // network). Failed tasks need a retry or restart, which
                  // "Resume all" never ran, so they must not enable it.
                  disabled={stats.paused + stats.waitingNetwork === 0}
                  onClick={onResumeAll}
                >
                  <Play className="h-3.5 w-3.5" aria-hidden />
                </Button>
              </TooltipTrigger>
              <TooltipContent>{t("taskList.resumeAll")}</TooltipContent>
            </Tooltip>
          ) : null}
          <SpeedLimitControl open={speedPanelOpen} onOpenChange={setSpeedPanelOpen} />
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

/**
 * "2 downloading at 6.9 MB/s", "3 waiting in queue", or the idle line. The
 * speed is spliced into the translated sentence so word order stays the
 * translator's, and it gets a fixed-width slot so the chips after it do not
 * shift on every progress tick.
 */
function ActivitySentence({ active, queued, speed }: { active: number; queued: number; speed: number }) {
  const { t } = useTranslation();
  if (active === 0) {
    return (
      <span className="min-w-0 truncate text-text-muted">
        {queued > 0 ? t("statusBar.queuedOnly", { count: queued }) : t("statusBar.idle")}
      </span>
    );
  }
  const [before, after] = t("statusBar.downloadingAt", { count: active, speed: SPEED_SLOT }).split(SPEED_SLOT);
  const speedLabel = formatSpeed(speed, { fixed: true });
  return (
    <span className="flex min-w-0 items-center text-text-secondary">
      <span className="min-w-0 truncate max-sm:sr-only">{withSpeedSlot(before, after, speedLabel)}</span>
      {/* A phone-width bar truncated the sentence right before the speed, the
          one number worth keeping; it shows icon + speed and leaves the
          sentence to screen readers. */}
      <span className="flex items-center gap-1 sm:hidden" aria-hidden>
        <ArrowDownToLine className="h-3.5 w-3.5 shrink-0 text-accent-primary" />
        <span className="font-mono font-semibold tabular-nums text-accent-primary">{speedLabel}</span>
      </span>
    </span>
  );
}

function withSpeedSlot(before: string | undefined, after: string | undefined, speed: string): ReactNode {
  // A mocked or missing translation has no slot; show it as-is.
  if (after === undefined) return before;
  return (
    <>
      {before}
      <span className="inline-block min-w-[8ch] font-mono font-semibold tabular-nums text-accent-primary">{speed}</span>
      {after}
    </>
  );
}

interface SummaryRow {
  /** Null for states with no view of their own (waiting for network). */
  nav: NavFilter | null;
  label: string;
  count: number;
  icon: LucideIcon;
}

/** Every task-state count, one click from the bar instead of always on it. */
function TaskSummary({
  open,
  onOpenChange,
  rows,
  onSelect,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  rows: SummaryRow[];
  onSelect: (nav: NavFilter) => void;
}) {
  const { t } = useTranslation();
  const rowClass = "flex h-8 w-full items-center gap-2 rounded-md px-2 text-left text-sm text-text-secondary";
  return (
    <Popover open={open} onOpenChange={onOpenChange}>
      <Tooltip>
        <TooltipTrigger asChild>
          <PopoverTrigger asChild>
            <Button
              type="button"
              variant="ghost"
              size="sm"
              className="h-8 shrink-0 gap-1 px-1.5 font-normal text-text-muted hover:text-text-primary"
              aria-label={t("statusBar.summary")}
            >
              <ListChecks className="h-3.5 w-3.5" aria-hidden />
              <span className="hidden lg:inline">{t("statusBar.summary")}</span>
            </Button>
          </PopoverTrigger>
        </TooltipTrigger>
        <TooltipContent>{t("statusBar.summary")}</TooltipContent>
      </Tooltip>
      {/* Flush with the window's bottom edge, so it opens upward. */}
      <PopoverContent className="w-56" side="top" align="start">
        <ul className="m-0 list-none space-y-0.5 p-0" aria-label={t("statusBar.summary")}>
          {rows.map(({ nav, label, count, icon: Icon }) => {
            const body = (
              <>
                <Icon className="h-3.5 w-3.5 shrink-0 text-text-muted" aria-hidden />
                <span className="min-w-0 flex-1 truncate">{label}</span>
                <span
                  className={cn(
                    "font-mono tabular-nums",
                    count > 0 ? "font-semibold text-text-primary" : "text-text-muted",
                  )}
                >
                  {count}
                </span>
              </>
            );
            return (
              <li key={label}>
                {nav ? (
                  <button
                    type="button"
                    className={cn(
                      rowClass,
                      "transition-[background-color,color] duration-[var(--motion-ui)] ease-out",
                      "hover:bg-surface-raised hover:text-text-primary",
                      "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-primary",
                    )}
                    onClick={() => onSelect(nav)}
                  >
                    {body}
                  </button>
                ) : (
                  <div className={rowClass}>{body}</div>
                )}
              </li>
            );
          })}
        </ul>
      </PopoverContent>
    </Popover>
  );
}

/** A task-state count that stays visible when work is out of the current list. */
function HealthChip({
  tone,
  icon: Icon,
  count,
  label,
  hint,
  onClick,
}: {
  tone: "neutral" | "warning" | "danger";
  icon: LucideIcon;
  count: number;
  label: string;
  hint: string;
  onClick?: () => void;
}) {
  const className = cn(
    // 32px: DESIGN.md's floor for dense desktop targets, and these chips are
    // the only always-visible failure signal.
    "h-8 min-w-0 shrink-0 gap-1 px-1.5 font-medium",
    tone === "danger"
      ? "text-status-danger hover:bg-status-danger/10 hover:text-status-danger"
      : tone === "warning"
        ? "text-status-warning hover:bg-status-warning/10 hover:text-status-warning"
        : "text-text-secondary hover:bg-surface-raised hover:text-text-primary",
  );
  const content = (
    <>
      <Icon className="h-3.5 w-3.5 shrink-0" aria-hidden />
      {/* The narrow bar keeps the icon and number; the sentence returns
          once there is room for it. */}
      <span className="font-mono tabular-nums sm:hidden">{count}</span>
      <span className="hidden truncate sm:inline">{label}</span>
    </>
  );
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        {onClick ? (
          <Button type="button" variant="ghost" size="sm" className={className} aria-label={label} onClick={onClick}>
            {content}
          </Button>
        ) : (
          <span role="status" aria-label={label} className={cn("inline-flex items-center", className)}>
            {content}
          </span>
        )}
      </TooltipTrigger>
      <TooltipContent>{hint}</TooltipContent>
    </Tooltip>
  );
}
