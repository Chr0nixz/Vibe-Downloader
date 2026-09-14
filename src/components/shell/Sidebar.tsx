import {
  AlertCircle,
  CheckCircle2,
  ChevronLeft,
  ChevronRight,
  CircleAlert,
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
} from "lucide-react";
import { useState } from "react";
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

/** Primary scan path — keep ≤4 so first paint asks to act, not classify. */
const primaryFilterItems: NavItemDef[] = [
  { id: "all", labelKey: "nav.all", icon: LayoutGrid },
  { id: "downloading", labelKey: "nav.downloading", icon: Download },
  { id: "attention", labelKey: "nav.attention", icon: CircleAlert },
  { id: "completed", labelKey: "nav.completed", icon: CheckCircle2 },
];

/** Secondary views — still reachable via More + command palette + shortcuts. */
const secondaryFilterItems: NavItemDef[] = [
  { id: "queue", labelKey: "nav.queue", icon: ListOrdered },
  { id: "paused", labelKey: "nav.paused", icon: PauseCircle },
  { id: "failed", labelKey: "nav.failed", icon: AlertCircle },
  { id: "storage", labelKey: "nav.storage", icon: HardDrive },
  { id: "recovery", labelKey: "nav.recovery", icon: LifeBuoy },
  { id: "backup", labelKey: "nav.backup", icon: DatabaseBackup },
];

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
    attention: taskStats.attention,
    paused: taskStats.paused,
    completed: taskStats.completed,
    failed: taskStats.failed,
    // Recovery Center spans both failure classes.
    recovery: taskStats.failed + taskStats.attention,
  };

  const secondaryActive = secondaryFilterItems.some((item) => item.id === nav);
  // Tasks hidden behind the overflow that warrant a look. Previously signalled by
  // a bare red dot, which said "something" but never "how much".
  const secondaryAttentionCount = (counts.failed ?? 0) + (counts.queue ?? 0);

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
      <nav
        className={cn(
          // ── Vertical compact rail at every width; `lg` expands it when not
          // collapsed. The former phone-style bottom bar fought the desktop
          // window context (PRODUCT.md "native desktop feel"): a narrow window
          // is a snapped desktop surface with pointer + keyboard, so the icon
          // rail is the native answer at every size.
          "flex h-auto w-[var(--shell-nav-width-compact)] shrink-0 flex-col items-stretch justify-between gap-1",
          "border-r border-border-subtle bg-surface-base p-1.5",
          // ── Desktop: expand only when not collapsed ──
          !collapsed && "lg:w-[var(--shell-nav-width)] lg:p-2",
          // No width/padding transition: animating layout properties forces a
          // reflow every frame on a container bordering the virtualized task
          // list. The collapse reads instantly from the content change itself.
        )}
        aria-label={t("app.navAria")}
      >
        {/* ── View group ── */}
        <div className="flex flex-1 flex-col items-stretch justify-start gap-0.5">
          {/* Group label — only when expanded (wide). These entries switch views;
              the filter facets live in the CommandBar tool panel, so labelling
              this group "Filters" sent users to the wrong control. */}
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

          {/* Desktop/tablet: secondary views behind More */}
          <Popover>
            <PopoverTrigger asChild>
              <Button
                type="button"
                variant="ghost"
                aria-label={
                  secondaryAttentionCount > 0
                    ? t("nav.moreViewsNeedsAttentionCount", { count: secondaryAttentionCount })
                    : t("nav.moreViews")
                }
                aria-current={secondaryActive ? "page" : undefined}
                className={cn(
                  "relative h-10 w-full flex-none flex-col items-start justify-start gap-1 px-1 text-xs",
                  "lg:h-9 lg:flex-row lg:items-center lg:justify-start lg:gap-3 lg:px-3",
                  collapsed && "lg:justify-center lg:px-0",
                  "transition-[color,background-color,box-shadow,border-color] duration-[var(--motion-ui)] ease-out",
                  secondaryActive
                    ? [
                        "bg-accent-primary/15 font-medium text-accent-primary dark:bg-accent-primary/20",
                        "shadow-[inset_0_0_0_1px_color-mix(in_oklch,var(--accent-primary)_35%,transparent)]",
                      ]
                    : "text-text-secondary hover:bg-surface-raised hover:text-text-primary",
                )}
              >
                <MoreHorizontal className="h-[18px] w-[18px] shrink-0" aria-hidden />
                <span
                  className={cn(
                    "max-w-none text-xs leading-tight lg:text-sm lg:leading-normal",
                    collapsed && "lg:hidden",
                  )}
                >
                  {t("nav.moreViews")}
                </span>
                {secondaryAttentionCount > 0 && (
                  <span
                    className={cn(
                      "absolute right-0 top-0 inline-flex min-w-4 items-center justify-center rounded-full px-1 text-[10px] font-semibold leading-4 tabular-nums",
                      "lg:static lg:min-w-0 lg:px-1.5 lg:py-0.5 lg:text-[11px] lg:leading-none",
                      collapsed &&
                        "lg:absolute lg:right-0 lg:top-0 lg:min-w-4 lg:px-1 lg:py-0 lg:text-[10px] lg:leading-4",
                      (counts.failed ?? 0) > 0
                        ? "bg-status-danger/12 text-status-danger"
                        : "bg-status-warning/14 text-status-warning",
                    )}
                    aria-hidden
                  >
                    {secondaryAttentionCount > 99 ? "99+" : secondaryAttentionCount}
                  </span>
                )}
              </Button>
            </PopoverTrigger>
            <PopoverContent align="start" side="right" className="w-52 p-1">
              {secondaryFilterItems.map((item) => (
                <MobileNavMenuItem
                  key={item.id}
                  item={item}
                  label={t(item.labelKey)}
                  active={nav === item.id}
                  count={counts[item.id] ?? 0}
                  onClick={() => setNav(item.id)}
                />
              ))}
            </PopoverContent>
          </Popover>
        </div>

        {/* ── Separator + Settings + Collapse toggle ── */}
        <div className="flex flex-none flex-col items-stretch gap-0.5">
          <div className="mx-2 mb-1 lg:mx-3">
            <div className="h-px bg-border-subtle/50" />
          </div>
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

          {/* Collapse / expand toggle */}
          <div className="mx-1.5 mt-0.5 lg:mx-2.5">
            <div className="h-px bg-border-subtle/40" />
          </div>
          <Button
            type="button"
            variant="ghost"
            onClick={toggleCollapse}
            aria-label={collapsed ? t("nav.expandSidebar") : t("nav.collapseSidebar")}
            className={cn(
              "group mt-1 flex h-10 w-full flex-none flex-row justify-center gap-2 p-0",
              "text-text-muted",
              "hover:bg-accent-primary/10 hover:text-accent-primary",
              "transition-[color,background-color] duration-[var(--motion-ui)]",
            )}
          >
            <span
              className={cn(
                "flex h-5 w-5 items-center justify-center rounded-full",
                "bg-surface-raised/80 group-hover:bg-accent-primary/15",
                "transition-colors duration-[var(--motion-ui)]",
              )}
            >
              {collapsed ? (
                <ChevronRight className="h-3.5 w-3.5" aria-hidden />
              ) : (
                <ChevronLeft className="h-3.5 w-3.5" aria-hidden />
              )}
            </span>
            <span className={cn("text-xs font-medium lg:hidden")}>
              {collapsed ? t("nav.expandSidebar") : t("nav.collapseSidebar")}
            </span>
          </Button>
        </div>
      </nav>
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
  compact,
  onClick,
  contextMenuItems,
}: {
  item: NavItemDef;
  active: boolean;
  label: string;
  count: number;
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

          {/* Label: visible at every tier; hidden only when the desktop rail is collapsed */}
          <span
            className={cn(
              "max-w-16 truncate leading-tight md:max-w-none lg:text-sm lg:leading-normal",
              compact && "lg:hidden",
            )}
          >
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
                active
                  ? "bg-accent-primary/20 text-accent-primary"
                  : item.id === "failed"
                    ? "bg-status-danger/12 text-status-danger"
                    : item.id === "attention"
                      ? "bg-status-warning/14 text-status-warning"
                      : "bg-surface-raised text-text-muted",
              )}
            >
              {count > 99 ? "99+" : count}
            </span>
          )}
        </Button>
      </TooltipTrigger>
      {/* Tooltip: only needed when the label is hidden, i.e. the collapsed rail */}
      <TooltipContent side="right" className={cn("hidden", compact && "lg:block")}>
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
