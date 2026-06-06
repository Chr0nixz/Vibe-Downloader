import { useMemo } from "react";
import { useTranslation } from "react-i18next";

import { formatSpeed } from "@/lib/utils";
import { useTaskStore } from "@/stores/task-store";

export function StatusBar() {
  const { t } = useTranslation();
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
    <footer className="flex h-9 shrink-0 items-center justify-between gap-3 border-t border-border-subtle bg-surface-base px-3 text-xs text-text-secondary md:h-8 md:px-4">
      <span className="min-w-0 truncate">
        {t("statusBar.total")}{" "}
        <span className="font-mono text-text-primary">
          {formatSpeed(stats.totalSpeed)}
        </span>
      </span>
      <span className="shrink-0">
        {t("statusBar.active")}{" "}
        <span className="font-mono text-text-primary">{stats.active}</span>
        <span className="hidden sm:inline">
          {" · "}
          {t("statusBar.queued")}{" "}
          <span className="font-mono text-text-primary">{stats.queued}</span>
        </span>
      </span>
      <span className="hidden text-text-muted md:inline">
        {t("statusBar.noGlobalSpeedLimit")}
      </span>
    </footer>
  );
}
