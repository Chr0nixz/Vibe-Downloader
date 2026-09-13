import { useEffect, useRef } from "react";

import type { ClipboardLinkDetectedPayload } from "@/generated/bindings";
import { onClipboardLinkDetected } from "@/lib/tauri";

/**
 * UX-23: registers the backend clipboard-link listener exactly once for the
 * component's lifetime. Handlers are kept in a ref so a changing callback
 * identity (dialog state, locale) never tears the listener down — the old
 * inline effect unlistened and re-listened across an await window in which
 * detected links were silently dropped.
 */
export function useClipboardLinkMonitor(onDetected: (payload: ClipboardLinkDetectedPayload) => void): void {
  const handlerRef = useRef(onDetected);
  handlerRef.current = onDetected;

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;

    void (async () => {
      unlisten = await onClipboardLinkDetected((payload) => handlerRef.current(payload));
      // The listener may have been unmounted before registration finished.
      if (cancelled) unlisten?.();
    })();

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);
}
