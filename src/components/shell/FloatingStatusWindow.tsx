import { X } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { useTaskEvents } from "@/hooks/use-task-events";
import {
  focusMainWindowFromFloating,
  hideFloatingStatusWindow,
  listTasksCursor,
  showTrayMenuAt,
} from "@/lib/tauri";
import { startWindowDrag } from "@/lib/window-controls";
import { cn, formatSpeed } from "@/lib/utils";
import { createLogger } from "@/lib/logger";
import { useTaskStore } from "@/stores/task-store";
import type { Task } from "@/types/task";

const log = createLogger("floating-status");

const RING_R = 28;
const RING_C = 2 * Math.PI * RING_R;

export function FloatingStatusWindow() {
  const { t } = useTranslation();
  const tasks = useTaskStore((s) => s.tasks);
  const setTasks = useTaskStore((s) => s.setTasks);
  const setLoading = useTaskStore((s) => s.setLoading);
  const [hovering, setHovering] = useState(false);

  useTaskEvents({ notify: false });

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        void hideFloatingStatusWindow();
      } else if (event.key === "Enter") {
        event.preventDefault();
        void focusMainWindowFromFloating();
      }
    };

    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  useEffect(() => {
    let cancelled = false;

    void (async () => {
      try {
        const data = await listTasksCursor({
          nav: "downloading",
          search: null,
          sortKey: "speed",
          sortDirection: "desc",
          fileType: "all",
          source: "all",
          failureCategory: "all",
          resume: "all",
          cursor: null,
          pageSize: 50,
        });
        if (!cancelled) setTasks(data.items);
      } catch (err) {
        if (!cancelled) log.error("initial task load failed", err);
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [setLoading, setTasks]);

  const stats = useMemo(() => buildFloatingStats(tasks), [tasks]);
  const percent = stats.totalBytes > 0
    ? Math.min(100, (stats.totalDownloaded / stats.totalBytes) * 100)
    : 0;
  const idle = stats.active === 0;
  const dashoffset = RING_C * (1 - percent / 100);

  const speedText = useMemo(() => {
    if (idle) return "";
    const s = formatSpeed(stats.totalSpeed);
    return s === "—" ? "" : s;
  }, [idle, stats.totalSpeed]);

  const compact = speedText.length > 6;

  const handleDrag = useCallback((event: React.MouseEvent) => {
    if (event.button !== 0) return;
    if ((event.target as HTMLElement).closest("[data-no-drag]")) return;
    void startWindowDrag();
  }, []);

  const handleDoubleClick = useCallback((event: React.MouseEvent) => {
    if ((event.target as HTMLElement).closest("[data-no-drag]")) return;
    void focusMainWindowFromFloating();
  }, []);

  const handleContextMenu = useCallback(
    async (event: React.MouseEvent) => {
      event.preventDefault();
      try {
        await showTrayMenuAt(event.screenX, event.screenY);
      } catch (err) {
        log.error("show tray menu failed", err);
      }
    },
    [],
  );

  return (
    <main
      className="group relative flex h-full select-none items-center justify-center"
      onMouseDown={handleDrag}
      onDoubleClick={handleDoubleClick}
      onContextMenu={handleContextMenu}
      onMouseEnter={() => setHovering(true)}
      onMouseLeave={() => setHovering(false)}
    >
      {/* Tooltip */}
      <div className="pointer-events-none absolute -top-9 left-1/2 z-10 -translate-x-1/2 whitespace-nowrap rounded-md bg-surface-overlay px-2.5 py-1 text-[0.65rem] leading-4 text-text-secondary opacity-0 shadow-lg ring-1 ring-border-container transition-opacity duration-200 group-hover:opacity-100 group-hover:[&:has(~_button:hover)]:opacity-0">
        {idle ? t("floatingStatus.idle") : `${speedText} · ${Math.round(percent)}%`}
      </div>

      {/* Close button — visible on hover */}
      <button
        type="button"
        data-no-drag
        className={cn(
          "absolute -top-0.5 right-0 z-20 grid h-5 w-5 place-items-center rounded-full",
          "bg-surface-raised text-text-muted shadow-md ring-1 ring-border-container",
          "opacity-0 transition-all duration-150 hover:bg-status-danger hover:text-text-on-danger hover:ring-status-danger/30",
          hovering && "opacity-100",
        )}
        aria-label={t("floatingStatus.close")}
        onClick={() => void hideFloatingStatusWindow()}
      >
        <X className="h-2.5 w-2.5" aria-hidden />
      </button>

      {/* Ball */}
      <div
        className={cn(
          "relative grid h-16 w-16 place-items-center rounded-full transition-shadow duration-300",
          idle
            ? "bg-surface-overlay shadow-lg ring-1 ring-border-container"
            : "bg-surface-overlay shadow-xl shadow-accent-primary/25 ring-1 ring-accent-primary/25 floating-ball-glow",
        )}
        aria-label={t("floatingStatus.title")}
      >
        {/* SVG progress ring */}
        <svg
          viewBox="0 0 64 64"
          className="absolute inset-0 h-full w-full -rotate-90"
          aria-hidden
        >
          <circle
            cx="32"
            cy="32"
            r={RING_R}
            fill="none"
            strokeWidth="2.5"
            className="stroke-border-subtle opacity-25"
          />
          {!idle && percent > 0 && (
            <circle
              cx="32"
              cy="32"
              r={RING_R}
              fill="none"
              strokeWidth="2.5"
              strokeLinecap="round"
              strokeDasharray={RING_C}
              strokeDashoffset={dashoffset}
              className="stroke-accent-energy transition-[stroke-dashoffset] duration-500 ease-out"
            />
          )}
        </svg>

        {/* Content */}
        <div className="relative z-10 flex flex-col items-center gap-px">
          {idle ? (
            <img
              src="/logo-48.png"
              alt=""
              width={28}
              height={28}
              className="select-none opacity-70"
              draggable={false}
            />
          ) : (
            <>
              <span
                className={cn(
                  "font-mono font-bold leading-none text-text-primary",
                  compact ? "text-[8px]" : "text-[10px]",
                )}
              >
                {speedText}
              </span>
              <span className="text-[7.5px] leading-none text-text-muted">
                {Math.round(percent)}%
              </span>
            </>
          )}
        </div>
      </div>
    </main>
  );
}

function buildFloatingStats(tasks: Task[]) {
  const activeTasks = tasks.filter(
    (task) => task.status === "downloading" || task.status === "retrying",
  );
  const queued = tasks.filter((task) => task.status === "queued").length;
  const totalSpeed = activeTasks.reduce((sum, task) => sum + task.speedBps, 0);
  const totalDownloaded = activeTasks.reduce(
    (sum, task) => sum + task.downloadedBytes,
    0,
  );
  const totalBytes = activeTasks.reduce((sum, task) => sum + task.totalSize, 0);
  const featuredTask =
    [...activeTasks].sort((a, b) => b.speedBps - a.speedBps)[0] ??
    [...tasks]
      .filter((task) =>
        [
          "queued",
          "paused",
          "failed",
          "needs_attention",
          "waiting_network",
        ].includes(task.status),
      )
      .sort(
        (a, b) => Date.parse(b.updatedAt) - Date.parse(a.updatedAt),
      )[0] ??
    null;

  return {
    active: activeTasks.length,
    queued,
    totalSpeed,
    totalDownloaded,
    totalBytes,
    featuredTask,
  };
}
