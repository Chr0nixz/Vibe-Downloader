import { useMemo, useState } from "react";
import {
  AlertCircle,
  CheckCircle2,
  ChevronLeft,
  ChevronRight,
  Download,
  LayoutGrid,
  PauseCircle,
  Settings,
} from "lucide-react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";
import { type NavFilter, useTaskStore } from "@/stores/task-store";

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

export function Sidebar() {
  const { t } = useTranslation();
  const nav = useTaskStore((s) => s.nav);
  const setNav = useTaskStore((s) => s.setNav);
  const tasks = useTaskStore((s) => s.tasks);

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

  const counts = useMemo(() => {
    const c: Record<string, number> = { all: tasks.length };
    for (const task of tasks) {
      c[task.status] = (c[task.status] ?? 0) + 1;
    }
    c.paused = (c.paused ?? 0) + (c.queued ?? 0) + (c.waiting_network ?? 0);
    c.failed = (c.failed ?? 0) + (c.needs_attention ?? 0);
    return c;
  }, [tasks]);

  return (
    <nav
      className={cn(
        // ── Mobile: horizontal bottom bar ──
        "order-3 flex h-14 w-full shrink-0 flex-row items-center gap-1 border-t px-1.5 py-1",
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
            "hidden px-3 py-1 text-[10px] font-medium tracking-[0.08em] text-text-muted uppercase lg:block",
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
            onClick={() => setNav(item.id)}
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
          onClick={() => setNav("settings")}
        />

        {/* Collapse / expand toggle */}
        <Button
          type="button"
          variant="ghost"
          onClick={toggleCollapse}
          aria-label={collapsed ? t("nav.expandSidebar") : t("nav.collapseSidebar")}
          className={cn(
            "h-9 w-9 flex-none justify-center p-0 text-text-muted",
            "hover:bg-surface-raised hover:text-text-secondary",
            "md:mt-1 md:w-full",
            "transition-colors duration-[var(--motion-ui)]",
          )}
        >
          {collapsed ? (
            <ChevronRight className="h-4 w-4" aria-hidden />
          ) : (
            <ChevronLeft className="h-4 w-4" aria-hidden />
          )}
        </Button>
      </div>
    </nav>
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
  onClick,
}: {
  item: NavItemDef;
  active: boolean;
  label: string;
  count: number;
  compact: boolean;
  onClick: () => void;
}) {
  const Icon = item.icon;
  const showBadge =
    item.id !== "settings" && item.id !== "all" && count > 0;
  const showActivityDot =
    item.id !== "settings" &&
    item.id !== "all" &&
    count > 0 &&
    (item.id === "downloading" || item.id === "failed");

  return (
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
            "relative h-12 min-w-11 flex-1 gap-2.5 px-0 text-sm",
            "md:h-10 md:w-full md:flex-none md:flex-col md:items-start md:justify-start md:gap-1 md:px-1",
            "lg:h-9 lg:flex-row lg:items-center lg:justify-start lg:gap-3 lg:px-3",
            // ── Override button transition ──
            "transition-all duration-[var(--motion-ui)] ease-out",
            // ── Active: three-layer indicator ──
            active && [
              // Layer 1 — accent background fill
              "bg-accent-primary/10",
              // Layer 2 — left indicator bar (hidden on mobile, visible md+)
              "md:[--nav-indicator-scale:1]",
              "md:shadow-[inset_3px_0_0_var(--accent-primary)]",
              // Layer 3 — accent text + medium weight
              "font-medium text-accent-primary",
              // Subtle glass overlay on active
              "dark:bg-white/[0.04]",
            ],
            // ── Inactive ──
            !active && [
              "text-text-secondary",
              "hover:bg-surface-raised hover:text-text-primary",
            ],
          )}
        >
          {/* Left indicator bar with scaleY entrance (md+ only) */}
          {active && (
            <span
              className="nav-indicator pointer-events-none absolute top-1.5 bottom-1.5 left-0 hidden w-[3px] rounded-r-full bg-accent-primary md:block"
              style={{
                animation:
                  "nav-indicator-enter 180ms cubic-bezier(0.16, 1, 0.3, 1) forwards",
              }}
              aria-hidden
            />
          )}

          {/* Icon — left-aligned (no mx-auto) */}
          <Icon className="h-[18px] w-[18px] shrink-0" aria-hidden />

          {/* Label: hidden on mobile, 10px on compact, normal on wide; hidden when sidebar collapsed */}
          <span
            className={cn(
              "hidden text-[10px] leading-tight md:inline lg:text-sm lg:leading-normal",
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
                item.id === "downloading"
                  ? "bg-accent-primary"
                  : "bg-status-danger",
              )}
              aria-hidden
            >
              {item.id === "downloading" && (
                <span
                  className="nav-dot-pulse absolute inset-0 rounded-full bg-accent-primary"
                  style={{
                    animation: "nav-dot-pulse 2s ease-in-out infinite",
                  }}
                />
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
}
