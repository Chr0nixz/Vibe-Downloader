import { useMemo } from "react";
import {
  AlertCircle,
  CheckCircle2,
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

const items: {
  id: NavFilter;
  labelKey: `nav.${string}`;
  icon: React.ComponentType<{ className?: string }>;
}[] = [
  { id: "all", labelKey: "nav.all", icon: LayoutGrid },
  { id: "downloading", labelKey: "nav.downloading", icon: Download },
  { id: "paused", labelKey: "nav.paused", icon: PauseCircle },
  { id: "completed", labelKey: "nav.completed", icon: CheckCircle2 },
  { id: "failed", labelKey: "nav.failed", icon: AlertCircle },
  { id: "settings", labelKey: "nav.settings", icon: Settings },
];

export function Sidebar() {
  const { t } = useTranslation();
  const nav = useTaskStore((s) => s.nav);
  const setNav = useTaskStore((s) => s.setNav);
  const tasks = useTaskStore((s) => s.tasks);

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
        "order-3 flex h-14 w-full shrink-0 flex-row items-center justify-around gap-1 border-t border-border-subtle bg-surface-base/80 px-1.5 py-1 md:order-none md:h-auto md:w-[var(--shell-nav-width-compact)] md:flex-col md:items-stretch md:justify-start md:border-r md:border-t-0 md:p-1.5 lg:w-[var(--shell-nav-width)] lg:p-2",
      )}
      aria-label={t("app.navAria")}
    >
      {items.map((item) => {
        const Icon = item.icon;
        const active = nav === item.id;
        const label = t(item.labelKey);
        const count = item.id !== "settings" ? (counts[item.id] ?? 0) : 0;
        const showBadge = item.id !== "settings" && item.id !== "all" && count > 0;

        return (
          <Tooltip key={item.id}>
            <TooltipTrigger asChild>
              <Button
                type="button"
                variant="ghost"
                onClick={() => setNav(item.id)}
                aria-current={active ? "page" : undefined}
                aria-label={label}
                className={cn(
                  "relative h-12 min-w-11 flex-1 justify-center gap-2.5 px-0 text-sm md:h-10 md:w-full md:flex-none md:justify-start lg:h-9 lg:px-3",
                  active
                    ? "bg-accent-primary/12 font-semibold text-accent-primary shadow-[inset_0_-3px_0_var(--accent-primary)] md:shadow-[inset_3px_0_0_var(--accent-primary)]"
                    : "text-text-secondary hover:bg-surface-raised hover:text-text-primary",
                )}
              >
                <Icon className="mx-auto h-[18px] w-[18px] shrink-0 lg:mx-0" aria-hidden />
                <span className="hidden lg:inline">{label}</span>
                {showBadge ? (
                  <span
                    className={cn(
                      "hidden rounded-full px-1.5 py-0.5 text-[10px] font-bold leading-none tabular-nums lg:inline-flex",
                      active
                        ? "bg-accent-primary/20 text-accent-primary"
                        : item.id === "failed"
                          ? "bg-status-danger/15 text-status-danger"
                          : "bg-surface-raised text-text-muted",
                    )}
                  >
                    {count > 99 ? "99+" : count}
                  </span>
                ) : null}
              </Button>
            </TooltipTrigger>
            <TooltipContent side="top" className="lg:hidden">
              {label}
              {showBadge ? ` (${count})` : null}
            </TooltipContent>
          </Tooltip>
        );
      })}
    </nav>
  );
}
