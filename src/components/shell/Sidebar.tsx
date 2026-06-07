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
                  "h-12 min-w-11 flex-1 justify-center gap-2 px-0 text-sm md:h-11 md:w-full md:flex-none md:justify-start lg:h-9 lg:px-3",
                  active
                    ? "bg-accent-primary/10 text-text-primary ring-1 ring-accent-primary/35"
                    : "text-text-secondary hover:bg-surface-raised hover:text-text-primary",
                )}
              >
                <Icon className="mx-auto h-4 w-4 shrink-0 lg:mx-0" aria-hidden />
                <span className="hidden lg:inline">{label}</span>
              </Button>
            </TooltipTrigger>
            <TooltipContent side="top" className="lg:hidden">
              {label}
            </TooltipContent>
          </Tooltip>
        );
      })}
    </nav>
  );
}
