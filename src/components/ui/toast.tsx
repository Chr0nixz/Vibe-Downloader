import { useCallback, useEffect, useRef } from "react";
import { AnimatePresence, motion, useReducedMotion } from "framer-motion";
import { AlertTriangle, CheckCircle2, Info, X } from "lucide-react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { useToastStore, type AppToast } from "@/stores/toast-store";

const TOAST_TIMEOUT_MS = 4800;

export function ToastViewport() {
  const { t } = useTranslation();
  const toasts = useToastStore((s) => s.toasts);
  const dismissToast = useToastStore((s) => s.dismissToast);
  const reduceMotion = useReducedMotion();

  return (
    <div
      className="pointer-events-none fixed bottom-28 right-3 z-[70] flex w-[calc(100vw-1.5rem)] max-w-sm flex-col gap-2 md:bottom-12 md:right-4"
      aria-live="polite"
      aria-atomic="false"
    >
      <AnimatePresence initial={false}>
        {toasts.map((toast) => (
          <ToastItem
            key={toast.id}
            toast={toast}
            reduceMotion={!!reduceMotion}
            dismissLabel={t("toast.dismiss")}
            onDismiss={() => dismissToast(toast.id)}
          />
        ))}
      </AnimatePresence>
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
  const remainingRef = useRef(TOAST_TIMEOUT_MS);
  const startedAtRef = useRef(Date.now());
  const pausedRef = useRef(false);

  const startTimer = useCallback(() => {
    startedAtRef.current = Date.now();
    pausedRef.current = false;
    timerRef.current = setTimeout(onDismiss, remainingRef.current);
  }, [onDismiss]);

  const pauseTimer = useCallback(() => {
    if (pausedRef.current) return;
    clearTimeout(timerRef.current);
    remainingRef.current -= Date.now() - startedAtRef.current;
    if (remainingRef.current < 0) remainingRef.current = 0;
    pausedRef.current = true;
  }, []);

  const resumeTimer = useCallback(() => {
    if (remainingRef.current <= 0) {
      onDismiss();
      return;
    }
    startTimer();
  }, [onDismiss, startTimer]);

  useEffect(() => {
    startTimer();
    return () => clearTimeout(timerRef.current);
  }, [startTimer]);

  const Icon =
    toast.tone === "success"
      ? CheckCircle2
      : toast.tone === "error"
        ? AlertTriangle
        : Info;

  return (
    <motion.div
      layout={!reduceMotion}
      initial={reduceMotion ? { opacity: 1 } : { opacity: 0, y: 12, scale: 0.98 }}
      animate={{ opacity: 1, y: 0, scale: 1 }}
      exit={reduceMotion ? { opacity: 0 } : { opacity: 0, y: 8, scale: 0.98 }}
      transition={{ duration: 0.2, ease: [0.16, 1, 0.3, 1] }}
      className={cn(
        "pointer-events-auto grid grid-cols-[auto_minmax(0,1fr)_auto] gap-3 rounded-lg border bg-surface-overlay px-3 py-3 shadow-lg backdrop-blur-sm",
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
        <p className="truncate text-sm font-medium text-text-primary">
          {toast.title}
        </p>
        {toast.description ? (
          <p className="mt-0.5 line-clamp-2 text-xs leading-5 text-text-secondary">
            {toast.description}
          </p>
        ) : null}
        {toast.action ? (
          <Button
            type="button"
            variant="ghost"
            size="sm"
            className="mt-2 h-10 px-3 text-xs md:h-7 md:px-2"
            onClick={() => {
              toast.action?.onClick();
              onDismiss();
            }}
          >
            {toast.action.label}
          </Button>
        ) : null}
      </div>
      <Button
        type="button"
        variant="ghost"
        size="icon"
        className="h-10 w-10 shrink-0 md:h-7 md:w-7"
        aria-label={dismissLabel}
        onClick={onDismiss}
      >
        <X className="h-3.5 w-3.5" />
      </Button>
    </motion.div>
  );
}
