import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { useTaskEvents } from "@/hooks/use-task-events";
import { createLogger } from "@/lib/logger";
import {
  focusMainWindowFromFloating,
  getTaskStats,
  hideFloatingStatusWindow,
  isTauriRuntime,
  listTasksCursor,
  showTrayMenuAt,
} from "@/lib/tauri";
import { cn, formatSpeed } from "@/lib/utils";
import { startWindowDrag } from "@/lib/window-controls";
import { useTaskDataStore } from "@/stores/task-store";

const log = createLogger("floating-status");

const RING_R = 28;
const RING_C = 2 * Math.PI * RING_R;
const BAR_WIDTH = 240;
const BAR_HEIGHT = 72;
const EDGE_THRESHOLD = 30;
const UNDOCK_THRESHOLD = 80;

type TauriWindow = Awaited<ReturnType<typeof import("@tauri-apps/api/window").getCurrentWindow>>;
type TauriMonitor = NonNullable<Awaited<ReturnType<typeof import("@tauri-apps/api/window").currentMonitor>>>;

async function dockToEdge(
  edge: "left" | "right",
  logicalY: number,
  win: TauriWindow,
  monitor: TauriMonitor,
  LogicalSizeCtor: typeof import("@tauri-apps/api/window").LogicalSize,
  LogicalPositionCtor: typeof import("@tauri-apps/api/window").LogicalPosition,
): Promise<void> {
  const scale = monitor.scaleFactor;
  const wa = monitor.workArea;
  const waLeft = wa.position.x / scale;
  const waRight = (wa.position.x + wa.size.width) / scale;

  const x = edge === "left" ? waLeft : waRight - BAR_WIDTH;
  const clampedY = Math.max(
    wa.position.y / scale,
    Math.min(logicalY, (wa.position.y + wa.size.height) / scale - BAR_HEIGHT),
  );

  await win.setSize(new LogicalSizeCtor(BAR_WIDTH, BAR_HEIGHT));
  await win.setPosition(new LogicalPositionCtor(x, clampedY));
}

async function undock(
  _logicalX: number,
  logicalY: number,
  win: TauriWindow,
  monitor: TauriMonitor,
  LogicalSizeCtor: typeof import("@tauri-apps/api/window").LogicalSize,
  LogicalPositionCtor: typeof import("@tauri-apps/api/window").LogicalPosition,
  prevEdge: "left" | "right" | null,
): Promise<void> {
  const scale = monitor.scaleFactor;
  const wa = monitor.workArea;
  const waLeft = wa.position.x / scale;
  const waRight = (wa.position.x + wa.size.width) / scale;

  // Restore ball near the docked edge, beyond UNDOCK_THRESHOLD to avoid re-trigger
  let restoreX: number;
  if (prevEdge === "right") {
    restoreX = waRight - 84 - 100;
  } else if (prevEdge === "left") {
    restoreX = waLeft + 100;
  } else {
    restoreX = waLeft + (waRight - waLeft) / 2 - 42;
  }
  const clampedY = Math.max(wa.position.y / scale, Math.min(logicalY, (wa.position.y + wa.size.height) / scale - 84));

  await win.setSize(new LogicalSizeCtor(84, 84));
  await win.setPosition(new LogicalPositionCtor(restoreX, clampedY));
}

export function FloatingStatusWindow() {
  const { t } = useTranslation();
  const stats = useTaskDataStore((s) => s.globalTaskStats ?? s.taskStats);
  const setTasks = useTaskDataStore((s) => s.setTasks);
  const setGlobalTaskStats = useTaskDataStore((s) => s.setGlobalTaskStats);
  const setLoading = useTaskDataStore((s) => s.setLoading);

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
        const stats = await getTaskStats();
        if (!cancelled) {
          setTasks(data.items);
          setGlobalTaskStats(stats);
        }
      } catch (err) {
        if (!cancelled) log.error("initial task load failed", err);
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [setGlobalTaskStats, setLoading, setTasks]);

  const percent = stats.totalBytes > 0 ? Math.min(100, (stats.totalDownloaded / stats.totalBytes) * 100) : 0;
  const idle = stats.active === 0;
  const dashoffset = RING_C * (1 - percent / 100);

  const speedText = useMemo(() => {
    if (idle) return "";
    const s = formatSpeed(stats.totalSpeed);
    return s === "—" ? "" : s;
  }, [idle, stats.totalSpeed]);

  const compact = speedText.length > 6;

  // Peak speed tracking for glow intensity
  const peakSpeedRef = useRef(0);
  useEffect(() => {
    if (stats.totalSpeed > peakSpeedRef.current) {
      peakSpeedRef.current = stats.totalSpeed;
    }
  }, [stats.totalSpeed]);

  const isPeak = !idle && stats.totalSpeed > peakSpeedRef.current * 0.8 && peakSpeedRef.current > 0;

  // Completion burst detection: transition from active downloads to idle
  const prevIdleRef = useRef(idle);
  const [completionBurst, setCompletionBurst] = useState(false);
  useEffect(() => {
    if (idle && !prevIdleRef.current) {
      setCompletionBurst(true);
      const timer = setTimeout(() => setCompletionBurst(false), 1200);
      return () => clearTimeout(timer);
    }
    prevIdleRef.current = idle;
  }, [idle]);

  // ── Edge snapping ──
  const [dockedEdge, setDockedEdge] = useState<"left" | "right" | null>(null);
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    if (!isTauriRuntime()) return;
    let unlisten: (() => void) | undefined;

    void (async () => {
      try {
        const { getCurrentWindow, currentMonitor, LogicalSize, LogicalPosition } = await import(
          "@tauri-apps/api/window"
        );
        const win = getCurrentWindow();

        unlisten = await win.onMoved(async ({ payload: pos }) => {
          if (debounceRef.current) clearTimeout(debounceRef.current);
          debounceRef.current = setTimeout(async () => {
            try {
              const monitor = await currentMonitor();
              if (!monitor) return;
              const scale = monitor.scaleFactor;
              const logicalX = pos.x / scale;
              const wa = monitor.workArea;
              const waLeft = wa.position.x / scale;
              const waRight = (wa.position.x + wa.size.width) / scale;

              if (logicalX - waLeft <= EDGE_THRESHOLD) {
                setDockedEdge("left");
                await dockToEdge("left", pos.y / scale, win, monitor, LogicalSize, LogicalPosition);
              } else if (waRight - (logicalX + 84) <= EDGE_THRESHOLD) {
                setDockedEdge("right");
                await dockToEdge("right", pos.y / scale, win, monitor, LogicalSize, LogicalPosition);
              } else if (
                (dockedEdge === "left" && logicalX - waLeft > UNDOCK_THRESHOLD) ||
                (dockedEdge === "right" && waRight - (logicalX + BAR_WIDTH) > UNDOCK_THRESHOLD)
              ) {
                setDockedEdge(null);
                await undock(logicalX, pos.y / scale, win, monitor, LogicalSize, LogicalPosition, dockedEdge);
              }
            } catch (err) {
              log.error("edge check failed", err);
            }
          }, 200);
        });
      } catch (err) {
        log.error("onMoved listener setup failed", err);
      }
    })();

    return () => {
      unlisten?.();
      if (debounceRef.current) clearTimeout(debounceRef.current);
    };
  }, [dockedEdge]);

  const handleDrag = useCallback(
    async (event: React.MouseEvent) => {
      if (event.button !== 0) return;
      if ((event.target as HTMLElement).closest("[data-no-drag]")) return;
      if (dockedEdge) {
        try {
          const winApi = await import("@tauri-apps/api/window");
          const { LogicalSize, LogicalPosition } = winApi;
          const win = winApi.getCurrentWindow();
          const scale = (await win.scaleFactor()) || 1;
          const pos = await win.innerPosition();
          const monitor = await winApi.currentMonitor();
          await win.setSize(new LogicalSize(84, 84));
          if (monitor) {
            const wa = monitor.workArea;
            const waLeft = wa.position.x / scale;
            const waRight = (wa.position.x + wa.size.width) / scale;
            const restoreX = dockedEdge === "right" ? waRight - 84 - 100 : waLeft + 100;
            await win.setPosition(new LogicalPosition(restoreX, pos.y / scale));
          }
        } catch {
          // ignore
        }
        setDockedEdge(null);
      }
      void startWindowDrag();
    },
    [dockedEdge],
  );

  const handleDoubleClick = useCallback((event: React.MouseEvent) => {
    if ((event.target as HTMLElement).closest("[data-no-drag]")) return;
    void focusMainWindowFromFloating();
  }, []);

  const handleContextMenu = useCallback(async (event: React.MouseEvent) => {
    event.preventDefault();
    try {
      await showTrayMenuAt(event.screenX, event.screenY);
    } catch (err) {
      log.error("show tray menu failed", err);
    }
  }, []);

  if (dockedEdge) {
    return (
      <main
        className={cn(
          "floating-bar group relative flex h-full w-full select-none items-center overflow-visible",
          dockedEdge === "left" ? "floating-bar-left" : "floating-bar-right",
        )}
        onMouseDown={handleDrag}
        onDoubleClick={handleDoubleClick}
        onContextMenu={handleContextMenu}
      >
        {/* Tooltip */}
        <div className="pointer-events-none absolute -top-9 left-1/2 z-10 -translate-x-1/2 whitespace-nowrap rounded-md bg-surface-overlay px-2.5 py-1 text-[0.65rem] leading-4 text-text-secondary opacity-0 shadow-lg ring-1 ring-border-container transition-opacity duration-200 group-hover:opacity-100">
          {idle ? t("floatingStatus.idle") : `${speedText} · ${Math.round(percent)}%`}
        </div>

        {/* Circular progress ball at the edge side */}
        <div
          className={cn(
            "floating-ball relative z-10 grid h-16 w-16 shrink-0 place-items-center rounded-full",
            idle
              ? "bg-surface-overlay shadow-[var(--shadow-idle-ball)] ring-1 ring-border-container"
              : cn(
                  "bg-surface-overlay ring-1 ring-accent-primary/25 floating-ball-glow",
                  isPeak && "floating-ball-peak",
                  completionBurst && "floating-ball-completion",
                ),
          )}
          style={dockedEdge === "left" ? { marginLeft: 4 } : { marginRight: 4, order: 1 }}
        >
          {/* SVG progress ring */}
          <svg viewBox="0 0 64 64" className="floating-ring absolute inset-0 h-full w-full -rotate-90" aria-hidden>
            <defs>
              <filter id="bar-glow-dot-filter">
                <feGaussianBlur stdDeviation="2.5" result="blur" />
                <feMerge>
                  <feMergeNode in="blur" />
                  <feMergeNode in="SourceGraphic" />
                </feMerge>
              </filter>
            </defs>
            <circle
              cx="32"
              cy="32"
              r={RING_R}
              fill="none"
              strokeWidth="2.5"
              className="stroke-border-subtle opacity-25"
            />
            {!idle && percent > 0 && (
              <>
                <circle
                  cx="32"
                  cy="32"
                  r={RING_R}
                  fill="none"
                  strokeWidth="3"
                  strokeLinecap="round"
                  strokeDasharray={RING_C}
                  strokeDashoffset={dashoffset}
                  stroke="var(--accent-primary)"
                  className="transition-[stroke-dashoffset] duration-500 ease-out"
                />
                <circle
                  cx={32 + RING_R * Math.cos((percent / 100) * Math.PI * 2)}
                  cy={32 + RING_R * Math.sin((percent / 100) * Math.PI * 2)}
                  r="2"
                  fill="var(--accent-energy)"
                  filter="url(#bar-glow-dot-filter)"
                />
              </>
            )}
          </svg>

          {/* Ball content */}
          <div className="relative flex flex-col items-center gap-px">
            {idle ? (
              <img
                src="/logo-48.png"
                alt=""
                width={24}
                height={24}
                className="select-none opacity-70"
                draggable={false}
              />
            ) : (
              <span className="font-mono text-[9px] font-bold leading-none text-text-primary">
                {Math.round(percent)}%
              </span>
            )}
          </div>
        </div>

        {/* Info panel */}
        <div className="relative z-10 flex flex-1 flex-col items-center justify-center px-2">
          <span className="font-mono text-[11px] font-bold leading-none text-text-primary">{speedText || "—"}</span>
          <span className="mt-1 text-[9px] leading-none text-text-muted">
            {idle ? t("floatingStatus.idle") : `${Math.round(percent)}%`}
          </span>
        </div>
      </main>
    );
  }

  return (
    <main
      className="group relative flex h-full select-none items-center justify-center overflow-visible"
      onMouseDown={handleDrag}
      onDoubleClick={handleDoubleClick}
      onContextMenu={handleContextMenu}
    >
      {/* Tooltip */}
      <div className="pointer-events-none absolute -top-9 left-1/2 z-10 -translate-x-1/2 whitespace-nowrap rounded-md bg-surface-overlay px-2.5 py-1 text-[0.65rem] leading-4 text-text-secondary opacity-0 shadow-lg ring-1 ring-border-container transition-opacity duration-200 group-hover:opacity-100">
        {idle ? t("floatingStatus.idle") : `${speedText} · ${Math.round(percent)}%`}
      </div>

      {/* Ball */}
      <div
        className={cn(
          "floating-ball relative grid h-16 w-16 place-items-center rounded-full",
          idle
            ? "bg-surface-overlay shadow-[var(--shadow-idle-ball)] ring-1 ring-border-container"
            : cn(
                "bg-surface-overlay ring-1 ring-accent-primary/25 floating-ball-glow",
                isPeak && "floating-ball-peak",
                completionBurst && "floating-ball-completion",
              ),
        )}
        aria-label={t("floatingStatus.title")}
      >
        {/* SVG progress ring */}
        <svg viewBox="0 0 64 64" className="floating-ring absolute inset-0 h-full w-full -rotate-90" aria-hidden>
          <defs>
            <filter id="glow-dot-filter">
              <feGaussianBlur stdDeviation="2.5" result="blur" />
              <feMerge>
                <feMergeNode in="blur" />
                <feMergeNode in="SourceGraphic" />
              </feMerge>
            </filter>
          </defs>
          <circle
            cx="32"
            cy="32"
            r={RING_R}
            fill="none"
            strokeWidth="2.5"
            className="stroke-border-subtle opacity-25"
          />
          {!idle && percent > 0 && (
            <>
              <circle
                cx="32"
                cy="32"
                r={RING_R}
                fill="none"
                strokeWidth="3"
                strokeLinecap="round"
                strokeDasharray={RING_C}
                strokeDashoffset={dashoffset}
                stroke="var(--accent-primary)"
                className="transition-[stroke-dashoffset] duration-500 ease-out"
              />
              <circle
                cx={32 + RING_R * Math.cos((percent / 100) * Math.PI * 2)}
                cy={32 + RING_R * Math.sin((percent / 100) * Math.PI * 2)}
                r="2"
                fill="var(--accent-energy)"
                filter="url(#glow-dot-filter)"
              />
            </>
          )}
        </svg>

        {/* Content */}
        <div className="relative flex flex-col items-center gap-px">
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
                  compact ? "text-[9px]" : "text-[10px]",
                )}
              >
                {speedText}
              </span>
              <span className="text-[9px] leading-none text-text-muted">{Math.round(percent)}%</span>
            </>
          )}
        </div>
      </div>
    </main>
  );
}
