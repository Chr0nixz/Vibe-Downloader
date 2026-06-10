import { Activity, Gauge, X } from "lucide-react";
import { useEffect, useMemo } from "react";
import { useTranslation } from "react-i18next";

import { useTaskEvents } from "@/hooks/use-task-events";
import {
  focusMainWindowFromFloating,
  hideFloatingStatusWindow,
  listTasksPage,
} from "@/lib/tauri";
import { startWindowDrag } from "@/lib/window-controls";
import { cn, formatPercent, formatSpeed } from "@/lib/utils";
import { useTaskStore } from "@/stores/task-store";
import type { Task } from "@/types/task";

export function FloatingStatusWindow() {
  const { t } = useTranslation();
  const tasks = useTaskStore((s) => s.tasks);
  const setTasks = useTaskStore((s) => s.setTasks);
  const setLoading = useTaskStore((s) => s.setLoading);

  useTaskEvents({ notify: false });

  useEffect(() => {
    let cancelled = false;

    void (async () => {
      try {
        const data = await listTasksPage({
          nav: "downloading",
          search: null,
          sortKey: "speed",
          sortDirection: "desc",
          fileType: "all",
          source: "all",
          failure: "all",
          resume: "all",
          page: 0,
          pageSize: 50,
        });
        if (!cancelled) setTasks(data.items);
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [setLoading, setTasks]);

  const stats = useMemo(() => buildFloatingStats(tasks), [tasks]);
  const percent =
    stats.featuredTask && stats.featuredTask.totalSize > 0
      ? Math.min(100, (stats.featuredTask.downloadedBytes / stats.featuredTask.totalSize) * 100)
      : 0;
  const progressLabel = stats.featuredTask
    ? formatPercent(stats.featuredTask.downloadedBytes, stats.featuredTask.totalSize)
    : t("floatingStatus.noTask");
  const idle = stats.active === 0;

  return (
    <main
      className="flex h-full select-none items-center justify-center p-1.5"
      onMouseDown={(event) => {
        if (event.button !== 0) return;
        if ((event.target as HTMLElement).closest("[data-no-drag]")) return;
        void startWindowDrag();
      }}
      onDoubleClick={(event) => {
        if ((event.target as HTMLElement).closest("[data-no-drag]")) return;
        void focusMainWindowFromFloating();
      }}
    >
      <section
        className="flex h-full w-full flex-col overflow-hidden rounded-lg border border-border-subtle/80 bg-surface-overlay px-3 py-2 text-text-primary shadow-[0_16px_34px_oklch(0.12_0.01_255_/_0.28)]"
        aria-label={t("floatingStatus.title")}
      >
        <div className="flex min-h-0 items-start gap-2">
          <div
            className={cn(
              "grid h-7 w-7 shrink-0 place-items-center rounded-md ring-1",
              idle
                ? "bg-surface-raised text-text-muted ring-border-subtle/70"
                : "bg-accent-primary text-text-on-accent ring-accent-primary",
            )}
            aria-hidden
          >
            {idle ? <Activity className="h-3.5 w-3.5" /> : <Gauge className="h-3.5 w-3.5" />}
          </div>
          <div className="min-w-0 flex-1">
            <p className="flex min-w-0 items-baseline gap-2">
              <span className="text-[0.68rem] font-medium uppercase tracking-normal text-text-muted">
                {t("floatingStatus.totalSpeed")}
              </span>
              <span
                className={cn(
                  "truncate font-mono text-[1.08rem] font-semibold leading-6",
                  idle ? "text-text-secondary" : "text-accent-energy",
                )}
              >
                {formatSpeed(stats.totalSpeed)}
              </span>
            </p>
            <p className="mt-0.5 truncate text-[0.7rem] leading-4 text-text-muted">
              {t("floatingStatus.counts", {
                active: stats.active,
                queued: stats.queued,
              })}
            </p>
          </div>
          <button
            type="button"
            data-no-drag
            className="grid h-7 w-7 shrink-0 place-items-center rounded-md text-text-muted outline-none transition hover:bg-surface-raised hover:text-text-primary focus-visible:ring-2 focus-visible:ring-accent-primary/70"
            aria-label={t("floatingStatus.close")}
            onClick={() => void hideFloatingStatusWindow()}
          >
            <X className="h-3.5 w-3.5" aria-hidden />
          </button>
        </div>

        <div className="mt-2 min-w-0">
          <div className="flex min-w-0 items-center justify-between gap-2 text-[0.72rem] leading-4">
            <span className="min-w-0 truncate text-text-secondary">
              {stats.featuredTask?.fileName ?? t("floatingStatus.idle")}
            </span>
            <span className="shrink-0 font-mono text-text-muted">{progressLabel}</span>
          </div>
          <div
            className="mt-1 h-1.5 overflow-hidden rounded-full bg-surface-raised"
            role="progressbar"
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={Math.round(percent)}
            aria-label={stats.featuredTask?.fileName ?? t("floatingStatus.idle")}
          >
            <div
              className={cn(
                "h-full rounded-full transition-[width] duration-200 ease-out",
                idle ? "bg-text-muted/45" : "bg-accent-energy",
              )}
              style={{ width: `${percent}%` }}
            />
          </div>
        </div>
      </section>
    </main>
  );
}

function buildFloatingStats(tasks: Task[]) {
  const activeTasks = tasks.filter(
    (task) => task.status === "downloading" || task.status === "retrying",
  );
  const queued = tasks.filter((task) => task.status === "queued").length;
  const totalSpeed = activeTasks.reduce((sum, task) => sum + task.speedBps, 0);
  const featuredTask =
    [...activeTasks].sort((a, b) => b.speedBps - a.speedBps)[0] ??
    [...tasks]
      .filter((task) =>
        ["queued", "paused", "failed", "needs_attention", "waiting_network"].includes(task.status),
      )
      .sort((a, b) => Date.parse(b.updatedAt) - Date.parse(a.updatedAt))[0] ??
    null;

  return {
    active: activeTasks.length,
    queued,
    totalSpeed,
    featuredTask,
  };
}
