import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { useTaskEvents } from "@/hooks/use-task-events";
import { createLogger } from "@/lib/logger";
import {
  focusMainWindowFromFloating,
  getTaskStats,
  hideFloatingStatusWindow,
  isTauriRuntime,
  showTrayMenuAt,
} from "@/lib/tauri";
import { cn, formatSpeed } from "@/lib/utils";
import { startWindowDrag } from "@/lib/window-controls";
import { useTaskDataStore } from "@/stores/task-store";

const log = createLogger("floating-status");

const RING_R = 28;
const RING_C = 2 * Math.PI * RING_R;
const BALL_SIZE = 84;
const BAR_WIDTH = 18;
const BAR_HEIGHT = 160;
const EDGE_THRESHOLD = 40;
const UNDOCK_THRESHOLD = 80;
const UNDOCK_OFFSET = 100;

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
    restoreX = waRight - BALL_SIZE - UNDOCK_OFFSET;
  } else if (prevEdge === "left") {
    restoreX = waLeft + UNDOCK_OFFSET;
  } else {
    restoreX = waLeft + (waRight - waLeft) / 2 - BALL_SIZE / 2;
  }
  const clampedY = Math.max(
    wa.position.y / scale,
    Math.min(logicalY, (wa.position.y + wa.size.height) / scale - BALL_SIZE),
  );

  await win.setSize(new LogicalSizeCtor(BALL_SIZE, BALL_SIZE));
  await win.setPosition(new LogicalPositionCtor(restoreX, clampedY));
}

export function FloatingStatusWindow() {
  const { t } = useTranslation();
  const stats = useTaskDataStore((s) => s.globalTaskStats ?? s.taskStats);
  const setGlobalTaskStats = useTaskDataStore((s) => s.setGlobalTaskStats);

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

  // The floating ball only renders global stats; it does not need the task list.
  // Subscribe to task events to keep `globalTaskStats` live-updated.
  useEffect(() => {
    let cancelled = false;

    void (async () => {
      try {
        const snapshot = await getTaskStats();
        if (!cancelled) setGlobalTaskStats(snapshot);
      } catch (err) {
        if (!cancelled) log.error("initial stats load failed", err);
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [setGlobalTaskStats]);

  const percent = stats.totalBytes > 0 ? Math.min(100, (stats.totalDownloaded / stats.totalBytes) * 100) : 0;
  const idle = stats.active === 0;
  const dashoffset = RING_C * (1 - percent / 100);

  const speedText = useMemo(() => {
    if (idle) return "";
    const s = formatSpeed(stats.totalSpeed);
    return s === "—" ? "" : s;
  }, [idle, stats.totalSpeed]);

  const compact = speedText.length > 6;

  // ── Edge snapping ──
  // Mirror dockedEdge in a ref so the `onMoved` listener can read the latest
  // value without resubscribing on every state change. Resubscribing caused
  // events to be missed during the 200ms debounce window.
  const [dockedEdge, setDockedEdgeState] = useState<"left" | "right" | null>(null);
  const dockedEdgeRef = useRef<"left" | "right" | null>(null);
  const setDockedEdge = useCallback((edge: "left" | "right" | null) => {
    dockedEdgeRef.current = edge;
    setDockedEdgeState(edge);
  }, []);
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    if (!isTauriRuntime()) return;
    let unlisten: (() => void) | undefined;
    let cancelled = false;

    void (async () => {
      try {
        const { getCurrentWindow, currentMonitor, LogicalSize, LogicalPosition } = await import(
          "@tauri-apps/api/window"
        );
        const win = getCurrentWindow();

        unlisten = await win.onMoved(async ({ payload: pos }) => {
          if (debounceRef.current) clearTimeout(debounceRef.current);
          debounceRef.current = setTimeout(async () => {
            if (cancelled) return;
            try {
              const monitor = await currentMonitor();
              if (!monitor) return;
              const scale = monitor.scaleFactor;
              const logicalX = pos.x / scale;
              const wa = monitor.workArea;
              const waLeft = wa.position.x / scale;
              const waRight = (wa.position.x + wa.size.width) / scale;
              const currentEdge = dockedEdgeRef.current;
              // Use the window's actual current width so edge detection works
              // correctly whether the window is in ball (84) or bar (BAR_WIDTH) form.
              const physSize = await win.outerSize();
              const currentWidth = physSize.width / scale;

              if (logicalX - waLeft <= EDGE_THRESHOLD) {
                if (currentEdge !== "left") {
                  setDockedEdge("left");
                  await dockToEdge("left", pos.y / scale, win, monitor, LogicalSize, LogicalPosition);
                }
              } else if (waRight - (logicalX + currentWidth) <= EDGE_THRESHOLD) {
                if (currentEdge !== "right") {
                  setDockedEdge("right");
                  await dockToEdge("right", pos.y / scale, win, monitor, LogicalSize, LogicalPosition);
                }
              } else if (
                (currentEdge === "left" && logicalX - waLeft > UNDOCK_THRESHOLD) ||
                (currentEdge === "right" && waRight - (logicalX + currentWidth) > UNDOCK_THRESHOLD)
              ) {
                setDockedEdge(null);
                await undock(pos.y / scale, win, monitor, LogicalSize, LogicalPosition, currentEdge);
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
      cancelled = true;
      unlisten?.();
      if (debounceRef.current) clearTimeout(debounceRef.current);
    };
  }, [setDockedEdge]);

  const handleDrag = useCallback(async (event: React.MouseEvent) => {
    if (event.button !== 0) return;
    if ((event.target as HTMLElement).closest("[data-no-drag]")) return;
    // Start dragging directly. The onMoved listener handles dock/undock
    // based on the final position — no need to undock first, which previously
    // caused the window to jump 100px inward and miss edge snapping.
    void startWindowDrag();
  }, []);

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
          "floating-bar group relative flex h-full w-full select-none items-center justify-center overflow-visible",
          dockedEdge === "left" ? "floating-bar-left" : "floating-bar-right",
        )}
        onMouseDown={handleDrag}
        onDoubleClick={handleDoubleClick}
        onContextMenu={handleContextMenu}
      >
        {/* Tooltip — appears beside the bar */}
        <div
          className={cn(
            "pointer-events-none absolute top-1/2 z-10 -translate-y-1/2 whitespace-nowrap rounded-md bg-surface-overlay px-2.5 py-1 text-[0.65rem] leading-4 text-text-secondary opacity-0 shadow-lg ring-1 ring-border-container transition-opacity duration-200 group-hover:opacity-100",
            dockedEdge === "left" ? "left-full ml-2" : "right-full mr-2",
          )}
        >
          {idle ? t("floatingStatus.idle") : `${speedText} · ${Math.round(percent)}%`}
        </div>

        {/* Vertical progress track */}
        <div className="relative h-[calc(100%-8px)] w-2.5 overflow-hidden rounded-full">
          <div className="absolute inset-0 rounded-full bg-border-subtle/40" />
          {!idle && percent > 0 && (
            <div
              className="absolute bottom-0 left-0 w-full rounded-full transition-[height] duration-500 ease-out"
              style={{
                height: `${percent}%`,
                background: "var(--accent-primary)",
                boxShadow: "0 0 10px color-mix(in oklch, var(--accent-primary) 60%, transparent)",
              }}
            />
          )}
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
            : "bg-surface-overlay ring-1 ring-accent-primary/25 floating-ball-glow",
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
