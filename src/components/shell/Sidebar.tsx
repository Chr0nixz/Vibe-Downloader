import {
  AlertCircle,
  CheckCircle2,
  ChevronLeft,
  ChevronRight,
  Download,
  Filter,
  Info,
  LayoutGrid,
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
import { cn } from "@/lib/utils";
import { type NavFilter, useTaskDataStore, useTaskUIStore } from "@/stores/task-store";

const COLLAPSE_KEY = "vibe-sidebar-collapsed";

type NavItemDef = {
  id: NavFilter;
  labelKey: `nav.${string}`;
  icon: React.ComponentType<{ className?: string }>;
};

const filterItems: NavItemDef[] = [
  { id: "all", labelKey: "nav.all", icon: LayoutGrid },
  { id: "downloading", labelKey: "nav.downloading", icon: Download },
  { id: "paused", labelKey: "nav.paused", icon: PauseCircle },
  { id: "completed", labelKey: "nav.completed", icon: CheckCircle2 },
  { id: "failed", labelKey: "nav.failed", icon: AlertCircle },
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
  // Subscribe to the two slices separately so React only re-renders this
  // subtree when `globalTaskStats` is null AND `taskStats` actually changed.
  // Without this split, `s => s.globalTaskStats ?? s.taskStats` re-runs on
  // every 250ms progress tick (which always produces a new taskStats ref),
  // re-rendering the whole sidebar even when the global stats snapshot is
  // being used.
  const globalTaskStats = useTaskDataStore((s) => s.globalTaskStats);
  const localTaskStats = useTaskDataStore((s) => s.taskStats);
  const taskStats = globalTaskStats ?? localTaskStats;

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
    paused: taskStats.paused,
    completed: taskStats.completed,
    failed: taskStats.failed,
  };

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
          // ── Mobile: horizontal bottom bar ──
          "order-3 flex h-12 w-full shrink-0 flex-row items-center gap-1 border-t px-1 py-0.5",
          // ── Mica surface ──
          "bg-surface-base/60",
          "[backdrop-filter:blur(12px)]",
          "border-border-subtle/60",
          // ── Tablet: vertical compact column (always compact width) ──
          "md:order-none md:h-auto md:w-[var(--shell-nav-width-compact)]",
          "md:flex-col md:items-stretch md:justify-between md:gap-1",
          "md:border-r md:border-t-0 md:p-1.5",
          // ── Desktop: expand only when not collapsed ──
          !collapsed && "lg:w-[var(--shell-nav-width)] lg:p-2",
          // ── Width transition ──
          "transition-[width,padding] duration-[var(--motion-ui)] ease-out",
        )}
        aria-label={t("app.navAria")}
      >
        {/* ── Filter group (top on md+) ── */}
        <div className="flex flex-1 flex-row items-center justify-around gap-1 md:flex-col md:items-stretch md:justify-start md:gap-0.5 lg:justify-start">
          {/* Group label — only when expanded (wide) */}
          <span
            className={cn(
              "hidden px-3 py-1 text-[11px] font-medium text-text-muted lg:block",
              collapsed && "lg:hidden",
            )}
          >
            {t("nav.filters")}
          </span>

          {filterItems.map((item) => (
            <NavItem
              key={item.id}
              item={item}
              active={nav === item.id}
              label={t(item.labelKey)}
              count={counts[item.id] ?? 0}
              compact={collapsed}
              mobileHidden={item.id === "paused"}
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

        {/* ── Separator + Settings + Collapse toggle (bottom on md+) ── */}
        <div className="flex flex-none flex-row items-center gap-1 md:flex-col md:items-stretch md:gap-0.5">
          <div className="hidden md:mx-2 md:mb-1 md:block lg:mx-3">
            <div className="h-px bg-border-subtle/50" />
          </div>
          <NavItem
            item={settingsItem}
            active={nav === "settings"}
            label={t(settingsItem.labelKey)}
            count={0}
            compact={collapsed}
            mobileHidden
            onClick={() => setNav("settings")}
          />
          <NavItem
            item={aboutItem}
            active={nav === "about"}
            label={t(aboutItem.labelKey)}
            count={0}
            compact={collapsed}
            mobileHidden
            onClick={() => setNav("about")}
          />

          <Popover>
            <PopoverTrigger asChild>
              <Button
                type="button"
                variant="ghost"
                aria-label={t("nav.more")}
                aria-current={nav === "paused" || nav === "settings" || nav === "about" ? "page" : undefined}
                className={cn(
                  "relative h-11 min-w-10 flex-1 flex-col gap-0.5 px-1 text-[10px] md:hidden",
                  nav === "paused" || nav === "settings" || nav === "about"
                    ? "bg-accent-primary/15 font-medium text-accent-primary shadow-[inset_0_0_0_1px_color-mix(in_oklch,var(--accent-primary)_35%,transparent)]"
                    : "text-text-secondary hover:bg-surface-raised hover:text-text-primary",
                )}
              >
                <MoreHorizontal className="h-[18px] w-[18px]" aria-hidden />
                <span>{t("nav.more")}</span>
              </Button>
            </PopoverTrigger>
            <PopoverContent align="end" side="top" className="w-52 p-1 md:hidden">
              <MobileNavMenuItem
                item={filterItems[2]}
                label={t(filterItems[2].labelKey)}
                active={nav === "paused"}
                count={counts.paused}
                onClick={() => setNav("paused")}
              />
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

          {/* Collapse / expand toggle */}
          <div className="hidden md:mx-1.5 md:mt-0.5 md:block lg:mx-2.5">
            <div className="h-px bg-border-subtle/40" />
          </div>
          <Button
            type="button"
            variant="ghost"
            onClick={toggleCollapse}
            aria-label={collapsed ? t("nav.expandSidebar") : t("nav.collapseSidebar")}
            className={cn(
              "group hidden h-8 w-8 flex-none justify-center p-0 md:flex",
              "text-text-muted",
              "hover:bg-accent-primary/10 hover:text-accent-primary",
              "md:mt-1 md:h-10 md:w-full md:flex-row md:gap-2",
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
            <span className={cn("hidden text-xs font-medium md:inline lg:hidden", collapsed && "lg:hidden")}>
              {collapsed ? t("nav.expandSidebar") : t("nav.collapseSidebar")}
            </span>
          </Button>
        </div>
      </nav>
    </RegionContextMenu>
  );
}

/* ──────────────────────────────────────────────────────────
   Single nav item — handles all three responsive tiers
   ────────────────────────────────────────────────────────── */

function NavItem({
  item,
  active,
  label,
  count,
  compact,
  mobileHidden = false,
  onClick,
  contextMenuItems,
}: {
  item: NavItemDef;
  active: boolean;
  label: string;
  count: number;
  compact: boolean;
  mobileHidden?: boolean;
  onClick: () => void;
  /** Optional context-menu items for this nav entry (filter items only). */
  contextMenuItems?: React.ReactNode;
}) {
  const Icon = item.icon;
  const showBadge = item.id !== "settings" && item.id !== "all" && count > 0;
  const showActivityDot =
    item.id !== "settings" && item.id !== "all" && count > 0 && (item.id === "downloading" || item.id === "failed");

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
            // ── Base sizing per breakpoint (left-aligned) ──
            "relative h-11 min-w-10 flex-1 gap-1.5 px-1 text-xs",
            mobileHidden && "hidden md:flex",
            "md:h-10 md:w-full md:flex-none md:flex-col md:items-start md:justify-start md:gap-1 md:px-1",
            "lg:h-9 lg:flex-row lg:items-center lg:justify-start lg:gap-3 lg:px-3",
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
            active && "min-w-20 flex-[1.45] md:min-w-0 md:flex-none",
          )}
        >
          {/* Icon — left-aligned (no mx-auto) */}
          <Icon className="h-[18px] w-[18px] shrink-0" aria-hidden />

          {/* Label: hidden on mobile, 10px on compact, normal on wide; hidden when sidebar collapsed */}
          <span
            className={cn(
              "hidden max-w-16 truncate text-[10px] leading-tight md:inline md:max-w-none lg:text-sm lg:leading-normal",
              active && "inline",
              compact && "lg:hidden",
            )}
          >
            {label}
          </span>

          {/* Badge count — wide mode only, hidden when collapsed */}
          {showBadge && (
            <span
              className={cn(
                "hidden rounded-full px-1.5 py-0.5 text-[11px] font-semibold leading-none tabular-nums lg:inline-flex",
                compact && "lg:hidden",
                active
                  ? "bg-accent-primary/20 text-accent-primary"
                  : item.id === "failed"
                    ? "bg-status-danger/12 text-status-danger"
                    : "bg-surface-raised text-text-muted",
              )}
            >
              {count > 99 ? "99+" : count}
            </span>
          )}

          {/* Status dot — compact mode only (md, not lg; or collapsed) */}
          {showActivityDot && (
            <span
              className={cn(
                "absolute right-1.5 top-1 h-2 w-2 rounded-full md:block lg:hidden",
                compact && "lg:block",
                item.id === "downloading" ? "bg-accent-primary" : "bg-status-danger",
              )}
              aria-hidden
            >
              {item.id === "downloading" && (
                <span className="nav-dot-pulse absolute inset-0 rounded-full bg-accent-primary" />
              )}
            </span>
          )}
        </Button>
      </TooltipTrigger>
      {/* Tooltip: show on mobile and when sidebar is compact/collapsed */}
      <TooltipContent side={compact ? "right" : "top"} className={cn("lg:hidden", compact && "lg:block")}>
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
