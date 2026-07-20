import { AlertTriangle, CheckCircle2, Info, X } from "lucide-react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { type AppToast, TOAST_TIMEOUT_MS, useToastStore } from "@/stores/toast-store";

const COLLAPSED_VISIBLE = 4;

function orderToasts(toasts: AppToast[]): AppToast[] {
  // UX-09: Undo toasts stay reachable — pin them above ordinary messages.
  const undo: AppToast[] = [];
  const rest: AppToast[] = [];
  for (const toast of toasts) {
    if (toast.onAutoCommit) undo.push(toast);
    else rest.push(toast);
  }
  return [...undo, ...rest];
}

export function ToastViewport() {
  const { t } = useTranslation();
  const toasts = useToastStore((s) => s.toasts);
  const dismissToast = useToastStore((s) => s.dismissToast);
  const clearToasts = useToastStore((s) => s.clearToasts);
  const reduceMotion = useReducedMotion();
  const [expanded, setExpanded] = useState(false);

  const ordered = useMemo(() => orderToasts(toasts), [toasts]);
  const undoCount = ordered.filter((toast) => toast.onAutoCommit).length;
  // Always keep every Undo toast visible even when collapsed.
  const collapsedLimit = Math.max(COLLAPSED_VISIBLE, undoCount);
  const visible = expanded ? ordered : ordered.slice(0, collapsedLimit);
  const hiddenCount = ordered.length - visible.length;

  useEffect(() => {
    if (hiddenCount === 0 && expanded) setExpanded(false);
  }, [expanded, hiddenCount]);

  return (
    <div
      className="pointer-events-none fixed bottom-28 right-3 z-[70] flex w-[calc(100vw-1.5rem)] max-w-sm flex-col gap-2 md:bottom-12 md:right-4"
      aria-live="polite"
      aria-atomic="false"
    >
      <AnimatePresence initial={false}>
        {visible.map((toast) => (
          <ToastItem
            key={toast.id}
            toast={toast}
            reduceMotion={!!reduceMotion}
            dismissLabel={t("toast.dismiss")}
            onDismiss={() => dismissToast(toast.id)}
          />
        ))}
      </AnimatePresence>
      {hiddenCount > 0 || expanded || ordered.length > 1 ? (
        <div className="pointer-events-auto flex flex-wrap items-center justify-center gap-2">
          {hiddenCount > 0 || expanded ? (
            <button
              type="button"
              onClick={() => setExpanded((value) => !value)}
              className="rounded-full border border-border-subtle bg-surface-overlay px-3 py-1 text-xs text-text-secondary shadow-md transition-colors hover:bg-surface-raised hover:text-text-primary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-primary"
            >
              {expanded ? t("toast.showLess") : t("toast.showMore", { count: hiddenCount })}
            </button>
          ) : null}
          {ordered.length > 1 ? (
            <button
              type="button"
              onClick={() => clearToasts()}
              className="rounded-full border border-border-subtle bg-surface-overlay px-3 py-1 text-xs text-text-secondary shadow-md transition-colors hover:bg-surface-raised hover:text-text-primary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-primary"
            >
              {t("toast.clearAll")}
            </button>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}

function ToastItem({
  toast,
  reduceMotion,
  onDismiss,
  dismissLabel,
}: {
  toast: AppToast;
  reduceMotion: boolean;
  dismissLabel: string;
  onDismiss: () => void;
}) {
  const timerRef = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const duration = toast.durationMs ?? TOAST_TIMEOUT_MS;
  const remainingRef = useRef(duration);
  const startedAtRef = useRef(Date.now());
  const pausedRef = useRef(false);
  // One settle path per mount: commit (timeout/X) or undo — never both.
  const settledRef = useRef(false);
  // Keep callbacks in refs so the dismiss timer is not reset when the store
  // replaces toast object identity without changing duration.
  const onAutoCommitRef = useRef(toast.onAutoCommit);
  const actionRef = useRef(toast.action);
  onAutoCommitRef.current = toast.onAutoCommit;
  actionRef.current = toast.action;

  const settleCommit = useCallback(() => {
    if (settledRef.current) return;
    settledRef.current = true;
    clearTimeout(timerRef.current);
    onAutoCommitRef.current?.();
    onDismiss();
  }, [onDismiss]);

  const settleUndo = useCallback(() => {
    if (settledRef.current) return;
    settledRef.current = true;
    clearTimeout(timerRef.current);
    actionRef.current?.onClick();
    onDismiss();
  }, [onDismiss]);

  const startTimer = useCallback(() => {
    startedAtRef.current = Date.now();
    pausedRef.current = false;
    timerRef.current = setTimeout(settleCommit, remainingRef.current);
  }, [settleCommit]);

  const pauseTimer = useCallback(() => {
    if (pausedRef.current || settledRef.current) return;
    clearTimeout(timerRef.current);
    remainingRef.current -= Date.now() - startedAtRef.current;
    if (remainingRef.current < 0) remainingRef.current = 0;
    pausedRef.current = true;
  }, []);

  const resumeTimer = useCallback(() => {
    if (settledRef.current) return;
    if (remainingRef.current <= 0) {
      settleCommit();
      return;
    }
    startTimer();
  }, [settleCommit, startTimer]);

  useEffect(() => {
    startTimer();
    return () => clearTimeout(timerRef.current);
  }, [startTimer]);

  const Icon = toast.tone === "success" ? CheckCircle2 : toast.tone === "error" ? AlertTriangle : Info;

  // Countdown progress: 1 → 0 across the toast lifetime. Drives the bottom
  // edge bar width so users can see how long is left at a glance. Frozen while
  // hovered/focused (the bar's transition is paused alongside the timer).
  const [progress, setProgress] = useState(1);
  useEffect(() => {
    if (reduceMotion) return;
    const start = Date.now();
    const total = remainingRef.current;
    let raf = 0;
    const tick = () => {
      const elapsed = Date.now() - start;
      const remaining = Math.max(0, total - elapsed);
      setProgress(remaining / total);
      if (remaining > 0 && !pausedRef.current) {
        raf = requestAnimationFrame(tick);
      }
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [reduceMotion]);

  return (
    <motion.div
      layout={!reduceMotion}
      initial={reduceMotion ? { opacity: 1 } : { opacity: 0, y: 12, scale: 0.98 }}
      animate={{ opacity: 1, y: 0, scale: 1 }}
      exit={reduceMotion ? { opacity: 0 } : { opacity: 0, y: 8, scale: 0.98 }}
      transition={{ duration: 0.2, ease: [0.16, 1, 0.3, 1] }}
      className={cn(
        // Layered brand-tinted shadow (was generic shadow-md) + relative for the countdown bar.
        "pointer-events-auto relative grid grid-cols-[auto_minmax(0,1fr)_auto] gap-3 overflow-hidden rounded-lg border bg-surface-overlay px-3 py-3 shadow-[var(--shadow-toast)] backdrop-blur-sm",
        toast.tone === "success" && "border-border-success-strong",
        toast.tone === "error" && "border-border-danger-strong",
        toast.tone === "info" && "border-border-subtle",
      )}
      onMouseEnter={pauseTimer}
      onMouseLeave={resumeTimer}
      onFocusCapture={pauseTimer}
      onBlurCapture={resumeTimer}
      role={toast.tone === "error" ? "alert" : "status"}
    >
      <Icon
        className={cn(
          "mt-0.5 h-4 w-4 shrink-0",
          toast.tone === "success" && "text-status-success",
          toast.tone === "error" && "text-status-danger",
          toast.tone === "info" && "text-accent-primary",
        )}
        aria-hidden
      />
      <div className="min-w-0">
        <p className="truncate text-sm font-medium text-text-primary" title={toast.title}>
          {toast.title}
        </p>
        {toast.description ? (
          <p className="mt-0.5 line-clamp-2 text-xs leading-5 text-text-secondary">{toast.description}</p>
        ) : null}
        {toast.action ? (
          <Button
            type="button"
            variant="ghost"
            size="sm"
            className="mt-2 h-11 px-3 text-xs md:h-7 md:px-2"
            onClick={settleUndo}
          >
            {toast.action.label}
          </Button>
        ) : null}
      </div>
      <Button
        type="button"
        variant="ghost"
        size="icon"
        className="h-11 w-11 shrink-0 md:h-8 md:w-8"
        aria-label={dismissLabel}
        onClick={settleCommit}
      >
        <X className="h-3.5 w-3.5" />
      </Button>
      {/* Countdown bar — bottom edge, accent-tinted, shrinks 1 → 0.
          Hidden under reduced-motion (the timer still runs; we just skip the viz). */}
      {!reduceMotion && (
        <span
          aria-hidden
          className="toast-countdown-bar pointer-events-none absolute inset-x-0 bottom-0 h-px origin-left bg-accent-primary/40"
          style={{ transform: `scaleX(${progress})` }}
        />
      )}
    </motion.div>
  );
}
