import {
  CheckCircle2,
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  DatabaseBackup,
  Download,
  Filter,
  HardDrive,
  Info,
  LayoutGrid,
  LifeBuoy,
  ListOrdered,
  MoreHorizontal,
  PanelLeftClose,
  PanelLeftOpen,
  PauseCircle,
  Settings,
  TriangleAlert,
} from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { MenuItem, RegionContextMenu } from "@/components/ui/menu-item";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import type { TranslationKey } from "@/i18n";
import { cn } from "@/lib/utils";
import { type NavFilter, useTaskDataStore, useTaskUIStore } from "@/stores/task-store";

const COLLAPSE_KEY = "vibe-sidebar-collapsed";

type NavItemDef = {
  id: NavFilter;
  labelKey: TranslationKey;
  icon: React.ComponentType<{ className?: string }>;
};

/** The three list views the "Needs you" entry covers; its cause filter
 * switches between them inside the list. */
const ISSUE_NAVS: readonly NavFilter[] = ["issues", "attention", "failed"];

/** Count badges wear the colour of the state they count: "Needs you" is red
 * while anything has failed and amber while only decisions wait, as on the
 * row status badges; every other count stays neutral so a busy download list
 * never reads as an alarm. */
function navBadgeTone(id: NavFilter, failed: number): string {
  if (id === "issues") {
    return failed > 0 ? "bg-status-danger/12 text-status-danger" : "bg-status-warning/14 text-status-warning";
  }
  return "bg-surface-raised text-text-muted";
}

/** The four destinations a download manager is checked for: everything, what
 * is moving, what needs the user, and what is done. Failed and Needs attention
 * used to be two entries with two different layouts; they are one list now,
 * with a cause filter. */
const primaryFilterItems: NavItemDef[] = [
  { id: "all", labelKey: "nav.all", icon: LayoutGrid },
  { id: "downloading", labelKey: "nav.downloading", icon: Download },
  { id: "issues", labelKey: "nav.issues", icon: TriangleAlert },
  { id: "completed", labelKey: "nav.completed", icon: CheckCircle2 },
];

const otherTaskFilterItems: NavItemDef[] = [
  { id: "queue", labelKey: "nav.queue", icon: ListOrdered },
  { id: "paused", labelKey: "nav.paused", icon: PauseCircle },
];

/** Maintenance views stay together in the mobile overflow. They remain
 * explicit, labelled destinations rather than an opaque "other" bucket.
 * Recovery Center is a repair tool over the Needs attention and Failed tasks
 * (bulk retry, playbook, history), not a third view of them, so it sits here
 * instead of beside those states as a peer with the same count. */
const maintenanceItems: NavItemDef[] = [
  { id: "recovery", labelKey: "recoveryCenter.title", icon: LifeBuoy },
  { id: "storage", labelKey: "nav.storage", icon: HardDrive },
  { id: "backup", labelKey: "nav.backup", icon: DatabaseBackup },
];

const mobilePrimaryItems: NavItemDef[] = primaryFilterItems;

const mobileMoreItems: NavItemDef[] = [...otherTaskFilterItems, ...maintenanceItems];

const settingsItem: NavItemDef = {
  id: "settings",
  labelKey: "nav.settings",
  icon: Settings,
};

const aboutItem: NavItemDef = {
  id: "about",
  labelKey: "nav.about",
  icon: Info,
};

export function Sidebar({ onNewDownload }: { onNewDownload?: () => void }) {
  const { t } = useTranslation();
  const nav = useTaskUIStore((s) => s.nav);
  const setNav = useTaskUIStore((s) => s.setNav);
  // Combined selector: when globalTaskStats is non-null (backend snapshot),
  // it returns that stable ref and skips re-renders on progress ticks.
  // When null, returns taskStats — which now benefits from the zero-delta
  // fast path in patchTasksBatch (same ref when aggregate stats unchanged).
  const taskStats = useTaskDataStore((s) => s.globalTaskStats ?? s.taskStats);

  const [collapsed, setCollapsed] = useState(() => {
    try {
      return localStorage.getItem(COLLAPSE_KEY) === "1";
    } catch {
      return false;
    }
  });
  const [otherViewsExpanded, setOtherViewsExpanded] = useState(() =>
    otherTaskFilterItems.some((item) => item.id === nav),
  );

  const toggleCollapse = () => {
    const next = !collapsed;
    setCollapsed(next);
    try {
      localStorage.setItem(COLLAPSE_KEY, next ? "1" : "0");
    } catch {
      /* storage unavailable */
    }
  };

  const counts: Record<string, number> = {
    all: taskStats.all,
    downloading: taskStats.active,
    queue: taskStats.queued,
    issues: taskStats.attention + taskStats.failed,
    paused: taskStats.paused,
    completed: taskStats.completed,
  };
  // The cause filter's three views all light up the one "Needs you" entry.
  const isActive = (id: NavFilter) => nav === id || (id === "issues" && ISSUE_NAVS.includes(nav));
  const otherViewsActive = otherTaskFilterItems.some((item) => item.id === nav);
  const mobileMoreActive =
    mobileMoreItems.some((item) => item.id === nav) || nav === settingsItem.id || nav === aboutItem.id;

  useEffect(() => {
    if (otherViewsActive) setOtherViewsExpanded(true);
  }, [otherViewsActive]);

  return (
    <RegionContextMenu
      items={
        <>
          {onNewDownload && <MenuItem icon={Download} label={t("palette.newDownload")} onSelect={onNewDownload} />}
          <MenuItem
            icon={collapsed ? PanelLeftOpen : PanelLeftClose}
            label={collapsed ? t("nav.expandSidebar") : t("nav.collapseSidebar")}
            onSelect={toggleCollapse}
          />
        </>
      }
    >
      <div className="order-2 flex shrink-0 flex-col md:order-1 md:w-[var(--shell-nav-width-compact)] lg:w-auto">
        <nav
          className={cn(
            "hidden min-h-0 flex-1 flex-col items-stretch justify-between gap-1 border-r border-border-subtle bg-surface-base p-1.5 md:flex",
            !collapsed && "lg:w-[var(--shell-nav-width)] lg:p-2",
          )}
          aria-label={t("app.navAria")}
        >
          <div className="flex min-h-0 flex-1 flex-col items-stretch justify-start gap-0.5 overflow-y-auto">
            <span
              className={cn(
                "hidden px-3 py-1 text-[11px] font-medium text-text-muted lg:block",
                collapsed && "lg:hidden",
              )}
            >
              {t("nav.views")}
            </span>
            {primaryFilterItems.map((item) => (
              <NavItem
                key={item.id}
                item={item}
                active={isActive(item.id)}
                label={t(item.labelKey)}
                count={counts[item.id] ?? 0}
                badgeTone={navBadgeTone(item.id, taskStats.failed)}
                compact={collapsed}
                onClick={() => setNav(item.id)}
                contextMenuItems={
                  <MenuItem
                    icon={Filter}
                    label={t("contextmenu.sidebar.showOnly", { name: t(item.labelKey) })}
                    disabled={isActive(item.id)}
                    onSelect={() => setNav(item.id)}
                  />
                }
              />
            ))}
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  type="button"
                  variant="ghost"
                  aria-label={t("nav.otherViews")}
                  aria-expanded={otherViewsExpanded}
                  aria-controls="other-task-views"
                  onClick={() => setOtherViewsExpanded((expanded) => !expanded)}
                  className={cn(
                    "relative h-10 w-full flex-none flex-col items-center justify-start gap-1 px-1 text-xs",
                    "lg:h-9 lg:flex-row lg:items-center lg:justify-start lg:gap-2 lg:px-3",
                    collapsed && "lg:justify-center lg:px-0",
                    otherViewsActive && [
                      "bg-accent-primary/15 dark:bg-accent-primary/20",
                      "shadow-[inset_0_0_0_1px_color-mix(in_oklch,var(--accent-primary)_35%,transparent)]",
                      "font-medium text-accent-primary",
                    ],
                    !otherViewsActive && "text-text-secondary hover:bg-surface-raised hover:text-text-primary",
                  )}
                >
                  <MoreHorizontal className="h-[18px] w-[18px] shrink-0" aria-hidden />
                  <span
                    className={cn("hidden truncate lg:inline lg:text-sm lg:leading-normal", collapsed && "lg:hidden")}
                  >
                    {t("nav.otherViews")}
                  </span>
                  <ChevronDown
                    className={cn(
                      "ml-auto hidden h-3.5 w-3.5 shrink-0 transition-transform lg:inline",
                      collapsed && "lg:hidden",
                      otherViewsExpanded && "rotate-180",
                    )}
                    aria-hidden
                  />
                </Button>
              </TooltipTrigger>
              <TooltipContent side="right" className="hidden md:block lg:hidden">
                {t("nav.otherViews")}
              </TooltipContent>
            </Tooltip>
            <div id="other-task-views" hidden={!otherViewsExpanded} className="flex flex-col gap-0.5">
              {otherTaskFilterItems.map((item) => (
                <NavItem
                  key={item.id}
                  item={item}
                  active={nav === item.id}
                  label={t(item.labelKey)}
                  count={counts[item.id] ?? 0}
                  compact={collapsed}
                  onClick={() => setNav(item.id)}
                  contextMenuItems={
                    <MenuItem
                      icon={Filter}
                      label={t("contextmenu.sidebar.showOnly", { name: t(item.labelKey) })}
                      disabled={nav === item.id}
                      onSelect={() => setNav(item.id)}
                    />
                  }
                />
              ))}
            </div>
            <div className="mx-2 my-1 h-px bg-border-subtle/50 lg:mx-3" aria-hidden />
            <span
              className={cn(
                "hidden px-3 py-1 text-[11px] font-medium text-text-muted lg:block",
                collapsed && "lg:hidden",
              )}
            >
              {t("nav.maintenance")}
            </span>
            {maintenanceItems.map((item) => (
              <NavItem
                key={item.id}
                item={item}
                active={nav === item.id}
                label={t(item.labelKey)}
                count={0}
                compact={collapsed}
                onClick={() => setNav(item.id)}
              />
            ))}
          </div>
          <div className="flex flex-none flex-col items-stretch gap-0.5">
            <div className="mx-2 mb-1 h-px bg-border-subtle/50 lg:mx-3" aria-hidden />
            <NavItem
              item={settingsItem}
              active={nav === "settings"}
              label={t(settingsItem.labelKey)}
              count={0}
              compact={collapsed}
              onClick={() => setNav("settings")}
            />
            <NavItem
              item={aboutItem}
              active={nav === "about"}
              label={t(aboutItem.labelKey)}
              count={0}
              compact={collapsed}
              onClick={() => setNav("about")}
            />
            <Button
              type="button"
              variant="ghost"
              onClick={toggleCollapse}
              aria-label={collapsed ? t("nav.expandSidebar") : t("nav.collapseSidebar")}
              className="group mt-1 flex h-10 w-full flex-none flex-row justify-center gap-2 border-t border-border-subtle/40 p-0 text-text-muted hover:bg-accent-primary/10 hover:text-accent-primary lg:h-9"
            >
              <span className="flex h-5 w-5 items-center justify-center rounded-full bg-surface-raised/80 group-hover:bg-accent-primary/15">
                {collapsed ? (
                  <ChevronRight className="h-3.5 w-3.5" aria-hidden />
                ) : (
                  <ChevronLeft className="h-3.5 w-3.5" aria-hidden />
                )}
              </span>
              <span className="hidden text-xs font-medium lg:inline">
                {collapsed ? t("nav.expandSidebar") : t("nav.collapseSidebar")}
              </span>
            </Button>
          </div>
        </nav>

        <nav
          className="order-2 flex h-14 w-full items-stretch justify-around border-t border-border-subtle bg-surface-base px-1 pb-[env(safe-area-inset-bottom)] md:hidden"
          aria-label={t("app.navAria")}
        >
          {mobilePrimaryItems.map((item) => (
            <MobileBottomItem
              key={item.id}
              item={item}
              label={t(item.labelKey)}
              active={isActive(item.id)}
              count={counts[item.id] ?? 0}
              badgeTone={navBadgeTone(item.id, taskStats.failed)}
              onClick={() => setNav(item.id)}
            />
          ))}
          <Popover>
            <PopoverTrigger asChild>
              <Button
                type="button"
                variant="ghost"
                aria-label={t("nav.more")}
                className={cn(
                  "relative flex h-full min-w-0 flex-1 flex-col items-center justify-center gap-0.5 px-0.5 text-[11px] leading-tight",
                  mobileMoreActive ? "bg-accent-primary/12 font-medium text-accent-primary" : "text-text-secondary",
                )}
              >
                <MoreHorizontal className="h-5 w-5" aria-hidden />
                <span className="max-w-full text-center leading-tight">{t("nav.more")}</span>
              </Button>
            </PopoverTrigger>
            <PopoverContent align="end" side="top" className="w-56 p-1">
              {mobileMoreItems.map((item) => (
                <MobileNavMenuItem
                  key={item.id}
                  item={item}
                  label={t(item.labelKey)}
                  active={nav === item.id}
                  count={counts[item.id] ?? 0}
                  onClick={() => setNav(item.id)}
                />
              ))}
              <div className="my-1 h-px bg-border-subtle" aria-hidden />
              <MobileNavMenuItem
                item={settingsItem}
                label={t(settingsItem.labelKey)}
                active={nav === "settings"}
                onClick={() => setNav("settings")}
              />
              <MobileNavMenuItem
                item={aboutItem}
                label={t(aboutItem.labelKey)}
                active={nav === "about"}
                onClick={() => setNav("about")}
              />
            </PopoverContent>
          </Popover>
        </nav>
      </div>
    </RegionContextMenu>
  );
}

/* ──────────────────────────────────────────────────────────
   Single nav item — compact rail by default, expanded rail at lg
   ────────────────────────────────────────────────────────── */

function NavItem({
  item,
  active,
  label,
  count,
  badgeTone = "bg-surface-raised text-text-muted",
  compact,
  onClick,
  contextMenuItems,
}: {
  item: NavItemDef;
  active: boolean;
  label: string;
  count: number;
  badgeTone?: string;
  compact: boolean;
  onClick: () => void;
  /** Optional context-menu items for this nav entry (filter items only). */
  contextMenuItems?: React.ReactNode;
}) {
  const Icon = item.icon;
  const showBadge = item.id !== "settings" && item.id !== "all" && count > 0;

  const button = (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          type="button"
          variant="ghost"
          onClick={onClick}
          aria-current={active ? "page" : undefined}
          aria-label={showBadge ? `${label} (${count})` : label}
          className={cn(
            // ── Compact rail: full-width row, icon stacked over the label ──
            "relative h-10 w-full flex-none flex-col items-start justify-start gap-1 px-1 text-xs",
            "lg:h-9 lg:flex-row lg:items-center lg:justify-start lg:gap-3 lg:px-3",
            // ── Collapsed (lg): label and badge are hidden, so center the
            // lone icon within the compact nav column instead of left-aligning it.
            compact && "lg:justify-center lg:px-0",
            // ── Override button transition ──
            "transition-[color,background-color,box-shadow,border-color] duration-[var(--motion-ui)] ease-out",
            // ── Active: anchored indicator (no side-stripe; uses inset ring + stronger tint) ──
            active && [
              // Layer 1 — stronger accent fill (15%, was 10%) so the active item reads
              "bg-accent-primary/15 dark:bg-accent-primary/20",
              // Layer 2 — inset accent ring anchors the item (replaces the prior invisible 6% overlay)
              "shadow-[inset_0_0_0_1px_color-mix(in_oklch,var(--accent-primary)_35%,transparent)]",
              // Layer 3 — accent text + medium weight
              "font-medium text-accent-primary",
            ],
            // ── Inactive ──
            !active && ["text-text-secondary", "hover:bg-surface-raised hover:text-text-primary"],
          )}
        >
          {/* Icon — left-aligned (no mx-auto) */}
          <Icon className="h-[18px] w-[18px] shrink-0" aria-hidden />

          {/* The tablet rail keeps only icons; expanded desktop restores labels. */}
          <span className={cn("hidden truncate lg:inline lg:text-sm lg:leading-normal", compact && "lg:hidden")}>
            {label}
          </span>

          {/* Count. Corner superscript where there is no room for an inline pill
              (mobile, tablet, collapsed rail); pill on the expanded desktop rail. */}
          {showBadge && (
            <span
              className={cn(
                "absolute right-0 top-0 inline-flex min-w-4 items-center justify-center rounded-full px-1 text-[10px] font-semibold leading-4 tabular-nums",
                "lg:static lg:min-w-0 lg:px-1.5 lg:py-0.5 lg:text-[11px] lg:leading-none",
                compact && "lg:absolute lg:right-0 lg:top-0 lg:min-w-4 lg:px-1 lg:py-0 lg:text-[10px] lg:leading-4",
                active ? "bg-accent-primary/20 text-accent-primary" : badgeTone,
              )}
            >
              {count > 99 ? "99+" : count}
            </span>
          )}
        </Button>
      </TooltipTrigger>
      {/* The tablet rail has no inline labels; collapsed desktop keeps the same hint. */}
      <TooltipContent side="right" className="hidden md:block lg:hidden">
        {label}
        {showBadge ? ` (${count})` : null}
      </TooltipContent>
    </Tooltip>
  );

  // Filter nav items get a local context menu with "Show only this category";
  // settings/about fall through to the outer <nav> context menu.
  if (!contextMenuItems) return button;
  return <RegionContextMenu items={contextMenuItems}>{button}</RegionContextMenu>;
}

function MobileBottomItem({
  item,
  label,
  active,
  count,
  badgeTone,
  onClick,
}: {
  item: NavItemDef;
  label: string;
  active: boolean;
  count: number;
  badgeTone: string;
  onClick: () => void;
}) {
  const Icon = item.icon;
  const showBadge = item.id !== "all" && count > 0;
  return (
    <Button
      type="button"
      variant="ghost"
      onClick={onClick}
      aria-current={active ? "page" : undefined}
      aria-label={showBadge ? `${label} (${count})` : label}
      className={cn(
        "relative flex h-full min-w-0 flex-1 flex-col items-center justify-center gap-0.5 px-0.5 text-[11px] leading-tight",
        active ? "bg-accent-primary/12 font-medium text-accent-primary" : "text-text-secondary",
      )}
    >
      <Icon className="h-5 w-5 shrink-0" aria-hidden />
      <span className="max-w-full whitespace-normal text-center leading-tight line-clamp-2">{label}</span>
      {showBadge ? (
        <span
          className={cn(
            "absolute right-1 top-1 min-w-4 rounded-full px-1 text-center text-[10px] font-semibold leading-4",
            badgeTone,
          )}
        >
          {count > 99 ? "99+" : count}
        </span>
      ) : null}
    </Button>
  );
}

function MobileNavMenuItem({
  item,
  label,
  active,
  count = 0,
  onClick,
}: {
  item: NavItemDef;
  label: string;
  active: boolean;
  count?: number;
  onClick: () => void;
}) {
  const Icon = item.icon;
  return (
    <Button
      type="button"
      variant="ghost"
      onClick={onClick}
      aria-current={active ? "page" : undefined}
      className={cn(
        "h-10 w-full justify-start gap-3 px-2 text-sm",
        active ? "bg-accent-primary/12 text-accent-primary" : "text-text-secondary",
      )}
    >
      <Icon className="h-4 w-4" aria-hidden />
      <span>{label}</span>
      {count > 0 ? (
        <span className="ml-auto font-mono text-xs text-text-muted">{count > 99 ? "99+" : count}</span>
      ) : null}
    </Button>
  );
}
