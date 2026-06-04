import {
  AlertCircle,
  CheckCircle2,
  Download,
  LayoutGrid,
  PauseCircle,
  Settings,
} from "lucide-react";

import { cn } from "@/lib/utils";
import { type NavFilter, useTaskStore } from "@/stores/task-store";

const items: {
  id: NavFilter;
  label: string;
  icon: React.ComponentType<{ className?: string }>;
}[] = [
  { id: "all", label: "All tasks", icon: LayoutGrid },
  { id: "downloading", label: "Downloading", icon: Download },
  { id: "paused", label: "Paused", icon: PauseCircle },
  { id: "completed", label: "Completed", icon: CheckCircle2 },
  { id: "failed", label: "Failed", icon: AlertCircle },
  { id: "settings", label: "Settings", icon: Settings },
];

export function Sidebar() {
  const nav = useTaskStore((s) => s.nav);
  const setNav = useTaskStore((s) => s.setNav);

  return (
    <nav
      className="flex w-52 shrink-0 flex-col gap-1 border-r border-border-subtle bg-surface-base/80 p-2"
      aria-label="Main navigation"
    >
      {items.map((item) => {
        const Icon = item.icon;
        const active = nav === item.id;
        return (
          <button
            key={item.id}
            type="button"
            onClick={() => setNav(item.id)}
            className={cn(
              "flex h-9 items-center gap-2 rounded-md px-3 text-sm transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-primary",
              active
                ? "bg-accent-primary/15 text-accent-primary"
                : "text-text-secondary hover:bg-surface-raised hover:text-text-primary",
            )}
          >
            <Icon className="h-4 w-4 shrink-0" aria-hidden />
            <span>{item.label}</span>
          </button>
        );
      })}
    </nav>
  );
}
