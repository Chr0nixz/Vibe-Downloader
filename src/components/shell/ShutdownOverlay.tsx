import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { onShuttingDown } from "@/lib/tauri";

/**
 * Full-screen overlay shown while the app gracefully shuts down.
 *
 * The Rust backend emits `app://shutting-down` when the user closes the
 * window or clicks tray Quit. The backend then cancels active downloads,
 * waits up to 3 seconds for checkpoint flush, and exits. This overlay
 * keeps the user informed during that brief window.
 */
export function ShutdownOverlay() {
  const { t } = useTranslation();
  const [visible, setVisible] = useState(false);

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;

    void (async () => {
      unlisten = await onShuttingDown(() => {
        if (!cancelled) setVisible(true);
      });
      if (cancelled) unlisten?.();
    })();

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  if (!visible) return null;

  return (
    <div
      className="pointer-events-none fixed inset-0 z-[9999] flex items-center justify-center bg-surface-scrim motion-safe:animate-[fade-in_140ms_ease-out]"
      role="alert"
      aria-live="assertive"
    >
      <div className="flex flex-col items-center gap-4 rounded-xl bg-surface-overlay px-8 py-6 shadow-md ring-1 ring-border-subtle">
        <div
          className="h-8 w-8 rounded-full border-2 border-border-subtle border-t-accent-primary motion-safe:animate-spin"
          aria-hidden="true"
        />
        <p className="text-sm font-medium text-text-primary">{t("shutdown.progress")}</p>
      </div>
    </div>
  );
}
