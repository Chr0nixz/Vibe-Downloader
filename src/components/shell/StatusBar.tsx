import { useMemo } from "react";

import { formatSpeed } from "@/lib/utils";
import { useTaskStore } from "@/stores/task-store";

export function StatusBar() {
  const tasks = useTaskStore((s) => s.tasks);

  const stats = useMemo(() => {
    const active = tasks.filter(
      (t) => t.status === "downloading" || t.status === "retrying",
    );
    const queued = tasks.filter((t) => t.status === "queued");
    const totalSpeed = active.reduce((sum, t) => sum + t.speedBps, 0);
    return { active: active.length, queued: queued.length, totalSpeed };
  }, [tasks]);

  return (
    <footer className="flex h-8 shrink-0 items-center justify-between border-t border-border-subtle bg-surface-base px-4 text-xs text-text-secondary">
      <span>
        Total{" "}
        <span className="font-mono text-text-primary">
          {formatSpeed(stats.totalSpeed)}
        </span>
      </span>
      <span>
        Active <span className="font-mono text-text-primary">{stats.active}</span>
        {" · "}
        Queued{" "}
        <span className="font-mono text-text-primary">{stats.queued}</span>
      </span>
      <span className="text-text-muted">No global speed limit</span>
    </footer>
  );
}
